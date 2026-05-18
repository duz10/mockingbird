//! History page commands. Lookups go through the existing `db::*`
//! repos — no new SQL here unless the UI needs a shape the repo
//! doesn't expose (e.g. the `final_text` joined onto the summary).

use rusqlite::{params, Connection};
use tauri::State;

use crate::commands::types::{
    LatencyBreakdown, SessionDetail, SessionSummary, TranscriptSearchHit,
};
use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::db::{search, sessions, transcripts};

/// Single-line text for the summary row. Falls back across stages:
/// `final` → `cleaned` → `raw` so we always show something.
fn pick_summary_text(conn: &Connection, session_id: i64) -> Option<String> {
    for stage in [
        transcripts::Stage::Final,
        transcripts::Stage::Cleaned,
        transcripts::Stage::Raw,
    ] {
        if let Ok(Some(t)) = transcripts::get_stage(conn, session_id, stage) {
            return Some(t.text);
        }
    }
    None
}

fn stage_text(
    conn: &Connection,
    session_id: i64,
    stage: transcripts::Stage,
) -> Result<String, String> {
    Ok(transcripts::get_stage(conn, session_id, stage)
        .map_err(into_err)?
        .map(|t| t.text)
        .unwrap_or_default())
}

fn mode_slug_for(conn: &Connection, mode_id: i64) -> Option<String> {
    conn.query_row("SELECT slug FROM modes WHERE id = ?1", [mode_id], |r| {
        r.get::<_, String>(0)
    })
    .ok()
}

fn duration_ms(started_at: &str, recording_ended_at: &str) -> i64 {
    // Sessions store ISO-8601 timestamps. Try RFC-3339 first; SQLite's
    // `datetime('now')` output (no `T`, no timezone) is the fallback.
    // Returns 0 on any parse hiccup — the UI handles 0 as "—".
    fn parse(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&chrono::Utc))
            .or_else(|_| {
                chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").map(|d| d.and_utc())
            })
            .ok()
    }
    match (parse(started_at), parse(recording_ended_at)) {
        (Some(a), Some(b)) => (b - a).num_milliseconds().max(0),
        _ => 0,
    }
}

fn to_summary(conn: &Connection, s: &sessions::Session) -> SessionSummary {
    SessionSummary {
        id: s.id,
        uuid: s.uuid.clone(),
        mode_slug: mode_slug_for(conn, s.mode_id).unwrap_or_else(|| "unknown".into()),
        started_at: s.started_at.clone(),
        duration_ms: duration_ms(&s.started_at, &s.recording_ended_at),
        foreground_app: s.foreground_app.clone(),
        foreground_window_title: s.foreground_window_title.clone(),
        final_text: pick_summary_text(conn, s.id).unwrap_or_default(),
        injection_status: s
            .injection_status
            .clone()
            .unwrap_or_else(|| "unknown".into()),
    }
}

#[tauri::command]
pub fn list_sessions(
    db: State<'_, AppStateHandle>,
    limit: usize,
    offset: usize,
) -> Result<Vec<SessionSummary>, String> {
    let conn = lock_db(&db)?;
    let mut stmt = conn
        .prepare("SELECT id FROM sessions ORDER BY started_at DESC LIMIT ?1 OFFSET ?2")
        .map_err(into_err)?;
    let mapped = stmt
        .query_map(
            params![limit as i64, offset as i64],
            |r: &rusqlite::Row<'_>| r.get::<_, i64>(0),
        )
        .map_err(into_err)?;
    let mut ids: Vec<i64> = Vec::new();
    for r in mapped {
        ids.push(r.map_err(into_err)?);
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(s) = sessions::get_by_id(&conn, id).map_err(into_err)? {
            out.push(to_summary(&conn, &s));
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn get_session_detail(db: State<'_, AppStateHandle>, id: i64) -> Result<SessionDetail, String> {
    let conn = lock_db(&db)?;
    let session = sessions::get_by_id(&conn, id)
        .map_err(into_err)?
        .ok_or_else(|| format!("session {id} not found"))?;
    let summary = to_summary(&conn, &session);
    let raw = stage_text(&conn, id, transcripts::Stage::Raw)?;
    let cleaned = stage_text(&conn, id, transcripts::Stage::Cleaned)?;
    let final_text = stage_text(&conn, id, transcripts::Stage::Final)?;

    // Pull the model_used + prompt_version off the cleaned transcript row.
    //
    // **TYPE-AFFINITY GOTCHA (was a silent bug pre-2026-05-17):**
    // `prompts.version` is `INTEGER` in the schema (migrations/003),
    // not `TEXT`. The DTO field `prompt_version` is `Option<String>`,
    // matching what the UI shows ("v1"). If we ask rusqlite to read
    // the integer column straight into `Option<String>`, the row
    // visitor returns `Err(InvalidColumnType)` — and because the
    // outer call used to be `.unwrap_or((None, None, None))`, ALL
    // THREE fields silently became NULL on the UI. Three pieces of
    // metadata vanished because of one type mismatch. Classic
    // swallow-the-error anti-pattern.
    //
    // Two fixes applied here:
    //   1. SQL-side: `'v' || p.version` forces TEXT affinity and
    //      produces the human-readable form ("v1", "v2", …) — same
    //      pattern already used by `commands::modes::list_modes`
    //      (which left a comment about this exact pitfall).
    //   2. Rust-side: log the error instead of silently nuking the
    //      tuple. A future column-type drift will surface in logs
    //      within seconds instead of producing a mystery UI bug that
    //      survives three smoketest rounds. (LESSONS 2026-05-17
    //      phase5-smoketest, fourth pass.)
    let (model_used, prompt_version, dictionary_version) = conn
        .query_row(
            "SELECT t.model_used, 'v' || p.version, s.dictionary_snapshot_id \
             FROM transcripts t \
             JOIN sessions s ON s.id = t.session_id \
             LEFT JOIN prompts p ON p.id = s.prompt_id \
             WHERE t.session_id = ?1 AND t.stage = 'cleaned' LIMIT 1",
            [id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .unwrap_or_else(|e| {
            tracing::warn!(
                session_id = id,
                error = ?e,
                "session metadata lookup failed; UI will show — for model/prompt/dict"
            );
            (None, None, None)
        });

    Ok(SessionDetail {
        session: summary,
        raw,
        cleaned,
        r#final: final_text,
        model_used,
        prompt_version,
        dictionary_version,
        latency: LatencyBreakdown {
            stt_ms: session.stt_latency_ms.map(|v| v as f64),
            cleanup_ms: session.cleanup_latency_ms.map(|v| v as f64),
            inject_ms: session.injection_latency_ms.map(|v| v as f64),
        },
    })
}

#[tauri::command]
pub fn search_transcripts(
    db: State<'_, AppStateHandle>,
    query: String,
    limit: usize,
) -> Result<Vec<TranscriptSearchHit>, String> {
    let conn = lock_db(&db)?;
    let hits = search::search(&conn, &query, limit).map_err(into_err)?;
    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        if let Some(s) = sessions::get_by_id(&conn, h.session_id).map_err(into_err)? {
            out.push(TranscriptSearchHit {
                session_id: h.session_id,
                stage: h.stage.as_str().to_string(),
                snippet: h.snippet,
                started_at: s.started_at.clone(),
                mode_slug: mode_slug_for(&conn, s.mode_id).unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn delete_session(db: State<'_, AppStateHandle>, id: i64) -> Result<(), String> {
    let conn = lock_db(&db)?;
    conn.execute("DELETE FROM sessions WHERE id = ?1", [id])
        .map_err(into_err)?;
    Ok(())
}

#[tauri::command]
pub fn mark_session_as_example(db: State<'_, AppStateHandle>, id: i64) -> Result<(), String> {
    let conn = lock_db(&db)?;
    let session = sessions::get_by_id(&conn, id)
        .map_err(into_err)?
        .ok_or_else(|| format!("session {id} not found"))?;
    let mode_slug = mode_slug_for(&conn, session.mode_id)
        .ok_or_else(|| "session's mode row missing".to_string())?;
    let raw = stage_text(&conn, id, transcripts::Stage::Raw)?;
    let mut final_text = stage_text(&conn, id, transcripts::Stage::Final)?;
    if final_text.is_empty() {
        final_text = stage_text(&conn, id, transcripts::Stage::Cleaned)?;
    }
    if raw.is_empty() || final_text.is_empty() {
        return Err("can't mark — session lacks both raw and final text".into());
    }
    crate::db::examples::insert(
        &conn,
        &crate::db::examples::NewStyleExample {
            mode_slug,
            session_id: Some(id),
            raw_input: raw,
            ideal_output: final_text,
            app_context: session.foreground_app.clone(),
            source: "user_marked".into(),
            rank: 1.0,
            enabled: true,
        },
    )
    .map_err(into_err)?;
    Ok(())
}

#[tauri::command]
pub fn report_correction(
    db: State<'_, AppStateHandle>,
    session_id: i64,
    before: String,
    after: String,
) -> Result<i64, String> {
    let conn = lock_db(&db)?;
    crate::learning::corrections::insert(
        &conn,
        &crate::learning::corrections::NewCorrection {
            session_id,
            before_text: before,
            after_text: after,
            detection_method: "manual".into(),
        },
    )
    .map_err(into_err)
}
