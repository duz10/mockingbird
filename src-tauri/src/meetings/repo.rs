//! Read-side projections for the meeting subsystem.
//!
//! The write path is in [`super::persist`]; the read path is here.
//! Split because:
//!   1. The DTOs cross the Tauri IPC boundary and need `serde::Serialize`
//!      with a stable wire shape (`camelCase`), which the persist
//!      module's owned types deliberately don't carry (they're internal
//!      to the runtime).
//!   2. Persist is ~460 lines already; bundling the read helpers would
//!      push it past the 600-line cap.
//!
//! Wave 4.4 lands the projections (`MeetingDetail`, `MeetingSummary`,
//! `MeetingMatch`) and the read functions used by:
//!   - `meetings::export::render_markdown` (needs `MeetingDetail`)
//!   - `commands::meetings::list_meetings` / `get_meeting_detail` /
//!     `delete_meeting` / `search_meeting_transcripts`

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::capture::MeetingSource;
use super::persist::MeetingStatus;

/// Detail view of one meeting — what the `MeetingDetail.tsx` page renders.
///
/// Does NOT include the `raw_segments` stage rows (they're heavy
/// JSON-encoded `Vec<SttSegment>` payloads and the UI never displays
/// them directly; if a future panel needs them, add a separate
/// `load_meeting_segments(uuid, channel)` helper).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub uuid: String,
    pub title: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub status: MeetingStatus,
    pub error_message: Option<String>,
    pub source: MeetingSource,
    pub total_duration_ms: u64,
    pub mic_duration_ms: Option<u64>,
    pub sys_duration_ms: Option<u64>,
    pub formatter_version: String,
    pub whisper_model_id: String,
    /// `formatted` stage for channel='mic'.
    pub formatted_mic: Option<String>,
    /// `formatted` stage for channel='system'.
    pub formatted_sys: Option<String>,
    /// `formatted` stage for channel='merged' (only present when
    /// source = Both AND the merge step succeeded).
    pub formatted_merged: Option<String>,
}

/// History-list row — what `Meetings.tsx` renders one per row.
///
/// Intentionally narrow: no transcript bodies (the list view shouldn't
/// haul ~50 KB per row over IPC). Click-through loads the full detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    pub uuid: String,
    pub title: Option<String>,
    pub started_at: String,
    pub total_duration_ms: u64,
    pub status: MeetingStatus,
    pub source: MeetingSource,
}

/// One FTS5 hit returned by [`search_meetings`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingMatch {
    pub uuid: String,
    pub title: Option<String>,
    pub started_at: String,
    /// FTS5 `snippet()` output — has `<mark>...</mark>` around hits.
    pub snippet: String,
    /// Which channel ('mic' | 'system' | 'merged') matched.
    pub channel: String,
}

// --------------------------------------------------------------------
// Read functions
// --------------------------------------------------------------------

/// Load the full detail view of one meeting, joining the formatted
/// transcripts. Returns `None` if no `meeting_sessions` row has that
/// uuid.
pub fn load_meeting_detail(conn: &Connection, uuid: &str) -> AppResult<Option<MeetingDetail>> {
    // First the session row.
    let session_opt = conn
        .query_row(
            "SELECT id, uuid, title, started_at, ended_at, status, error_message, \
                    source, total_duration_ms, mic_duration_ms, sys_duration_ms, \
                    formatter_version, whisper_model_id \
             FROM meeting_sessions WHERE uuid = ?1",
            params![uuid],
            |row| {
                let id: i64 = row.get(0)?;
                Ok((
                    id,
                    SessionHeader {
                        uuid: row.get(1)?,
                        title: row.get(2)?,
                        started_at: row.get(3)?,
                        ended_at: row.get(4)?,
                        status_str: row.get(5)?,
                        error_message: row.get(6)?,
                        source_str: row.get(7)?,
                        total_duration_ms: row.get::<_, i64>(8)? as u64,
                        mic_duration_ms: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
                        sys_duration_ms: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                        formatter_version: row.get(11)?,
                        whisper_model_id: row.get(12)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(|e| AppError::MeetingCapture(format!("load_meeting_detail session row: {e}")))?;

    let Some((session_id, header)) = session_opt else {
        return Ok(None);
    };

    let status = MeetingStatus::from_db_str(&header.status_str).ok_or_else(|| {
        AppError::MeetingCapture(format!(
            "meeting_sessions.status has unknown value: {:?}",
            header.status_str
        ))
    })?;
    let source = MeetingSource::from_db_str(&header.source_str).ok_or_else(|| {
        AppError::MeetingCapture(format!(
            "meeting_sessions.source has unknown value: {:?}",
            header.source_str
        ))
    })?;

    // Then the three (or fewer) formatted transcript rows.
    let (formatted_mic, formatted_sys, formatted_merged) =
        load_formatted_channels(conn, session_id)?;

    Ok(Some(MeetingDetail {
        uuid: header.uuid,
        title: header.title,
        started_at: header.started_at,
        ended_at: header.ended_at,
        status,
        error_message: header.error_message,
        source,
        total_duration_ms: header.total_duration_ms,
        mic_duration_ms: header.mic_duration_ms,
        sys_duration_ms: header.sys_duration_ms,
        formatter_version: header.formatter_version,
        whisper_model_id: header.whisper_model_id,
        formatted_mic,
        formatted_sys,
        formatted_merged,
    }))
}

/// Convenience holder so the query closure can return one tuple
/// without unwieldy positional args. Internal to this module.
struct SessionHeader {
    uuid: String,
    title: Option<String>,
    started_at: String,
    ended_at: String,
    status_str: String,
    error_message: Option<String>,
    source_str: String,
    total_duration_ms: u64,
    mic_duration_ms: Option<u64>,
    sys_duration_ms: Option<u64>,
    formatter_version: String,
    whisper_model_id: String,
}

fn load_formatted_channels(
    conn: &Connection,
    session_id: i64,
) -> AppResult<(Option<String>, Option<String>, Option<String>)> {
    let mut stmt = conn
        .prepare(
            "SELECT channel, text FROM meeting_transcripts \
             WHERE meeting_session_id = ?1 AND stage = 'formatted'",
        )
        .map_err(|e| AppError::MeetingCapture(format!("prepare formatted load: {e}")))?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            let ch: String = row.get(0)?;
            let txt: String = row.get(1)?;
            Ok((ch, txt))
        })
        .map_err(|e| AppError::MeetingCapture(format!("query formatted rows: {e}")))?;

    let mut mic = None;
    let mut sys = None;
    let mut merged = None;
    for row in rows {
        let (ch, txt) =
            row.map_err(|e| AppError::MeetingCapture(format!("decode formatted row: {e}")))?;
        match ch.as_str() {
            "mic" => mic = Some(txt),
            "system" => sys = Some(txt),
            "merged" => merged = Some(txt),
            other => tracing::warn!(
                target: "meetings",
                channel = other,
                "unknown formatted channel in meeting_transcripts; ignoring"
            ),
        }
    }
    Ok((mic, sys, merged))
}

/// Most recent meetings first. `limit` caps the result vector;
/// `offset` lets the UI paginate (sticking to LIMIT/OFFSET for
/// simplicity — meeting history is bounded by `retention_days` and
/// unlikely to exceed a few hundred rows).
pub fn list_meetings(conn: &Connection, limit: i64, offset: i64) -> AppResult<Vec<MeetingSummary>> {
    let mut stmt = conn
        .prepare(
            "SELECT uuid, title, started_at, total_duration_ms, status, source \
             FROM meeting_sessions \
             ORDER BY started_at DESC \
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| AppError::MeetingCapture(format!("prepare list_meetings: {e}")))?;
    let rows = stmt
        .query_map(params![limit, offset], |row| {
            let uuid: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let started_at: String = row.get(2)?;
            let total_duration_ms: i64 = row.get(3)?;
            let status_str: String = row.get(4)?;
            let source_str: String = row.get(5)?;
            Ok((
                uuid,
                title,
                started_at,
                total_duration_ms,
                status_str,
                source_str,
            ))
        })
        .map_err(|e| AppError::MeetingCapture(format!("query list_meetings: {e}")))?;

    let mut out = Vec::new();
    for row in rows {
        let (uuid, title, started_at, total_duration_ms, status_str, source_str) =
            row.map_err(|e| AppError::MeetingCapture(format!("decode summary row: {e}")))?;
        let status = MeetingStatus::from_db_str(&status_str)
            .ok_or_else(|| AppError::MeetingCapture(format!("unknown status: {status_str:?}")))?;
        let source = MeetingSource::from_db_str(&source_str)
            .ok_or_else(|| AppError::MeetingCapture(format!("unknown source: {source_str:?}")))?;
        out.push(MeetingSummary {
            uuid,
            title,
            started_at,
            total_duration_ms: total_duration_ms as u64,
            status,
            source,
        });
    }
    Ok(out)
}

/// Hard-delete a meeting + cascade its transcript rows + FTS shadow.
/// `ON DELETE CASCADE` on `meeting_transcripts.meeting_session_id`
/// takes care of the rows; the FTS5 delete trigger handles the shadow.
///
/// Returns `Ok(false)` if no row had the uuid (idempotent — the UI's
/// "Delete" click on a stale list doesn't error).
pub fn delete_meeting(conn: &Connection, uuid: &str) -> AppResult<bool> {
    let rows_affected = conn
        .execute(
            "DELETE FROM meeting_sessions WHERE uuid = ?1",
            params![uuid],
        )
        .map_err(|e| AppError::MeetingCapture(format!("delete_meeting: {e}")))?;
    Ok(rows_affected > 0)
}

/// FTS5 search over formatted meeting transcripts. Returns one row
/// per matching transcript row (so a meeting whose mic AND merged
/// channels both match the query appears twice with channel-tagged
/// snippets). UI is responsible for de-duping at the meeting level
/// if it cares (today's design just lists hits).
pub fn search_meetings(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<MeetingMatch>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT s.uuid, s.title, s.started_at, t.channel, \
                    snippet(meeting_transcripts_fts, 0, '<mark>', '</mark>', '...', 12) \
             FROM meeting_transcripts_fts f \
             JOIN meeting_transcripts t ON t.id = f.rowid \
             JOIN meeting_sessions s ON s.id = t.meeting_session_id \
             WHERE meeting_transcripts_fts MATCH ?1 AND t.stage = 'formatted' \
             ORDER BY s.started_at DESC \
             LIMIT ?2",
        )
        .map_err(|e| AppError::MeetingCapture(format!("prepare search_meetings: {e}")))?;
    let rows = stmt
        .query_map(params![query, limit], |row| {
            Ok(MeetingMatch {
                uuid: row.get(0)?,
                title: row.get(1)?,
                started_at: row.get(2)?,
                channel: row.get(3)?,
                snippet: row.get(4)?,
            })
        })
        .map_err(|e| AppError::MeetingCapture(format!("query search_meetings: {e}")))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| AppError::MeetingCapture(format!("decode match row: {e}")))?);
    }
    Ok(out)
}

// Re-export `OptionalExtension` for `.optional()` on the query above.
// rusqlite ships it as a separate trait you have to opt into.
use rusqlite::OptionalExtension;

// ----------------------------------------------------------------
// Tests
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::apply_all;
    use crate::meetings::persist::{persist_meeting, MeetingPersistRequest};
    use crate::stt::SttSegment;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        apply_all(&conn).unwrap();
        conn
    }

    fn fixture_request(uuid: &str, source: MeetingSource) -> MeetingPersistRequest {
        MeetingPersistRequest {
            uuid: uuid.to_string(),
            title: Some(format!("Meeting {uuid}")),
            started_at: "2026-05-20T10:00:00Z".to_string(),
            ended_at: "2026-05-20T10:01:00Z".to_string(),
            status: MeetingStatus::Complete,
            error_message: None,
            source,
            total_duration_ms: 60_000,
            mic_duration_ms: if source.needs_mic() {
                Some(60_000)
            } else {
                None
            },
            sys_duration_ms: if source.needs_system() {
                Some(60_000)
            } else {
                None
            },
            hotkey_pressed: "RCtrl+M".to_string(),
            audio_blob_path: Some("/tmp/meeting.wav".to_string()),
            whisper_model_id: "whisper-large-v3-turbo-q5_0".to_string(),
            formatter_version: "mc-v1".to_string(),
            chunk_count_mic: if source.needs_mic() { Some(2) } else { None },
            chunk_count_sys: if source.needs_system() { Some(2) } else { None },
            stt_latency_ms: Some(450),
            formatter_latency_ms: Some(12),
            formatted_mic: if source.needs_mic() {
                Some(format!("mic body for {uuid}"))
            } else {
                None
            },
            formatted_sys: if source.needs_system() {
                Some(format!("sys body for {uuid}"))
            } else {
                None
            },
            formatted_merged: if source == MeetingSource::Both {
                Some(format!("**You:** hi\n\n**Other(s):** hello ({uuid})"))
            } else {
                None
            },
            segments_mic: if source.needs_mic() {
                Some(vec![SttSegment {
                    t0_ms: 0,
                    t1_ms: 1000,
                    text: "hi".into(),
                }])
            } else {
                None
            },
            segments_sys: if source.needs_system() {
                Some(vec![SttSegment {
                    t0_ms: 0,
                    t1_ms: 1000,
                    text: "hello".into(),
                }])
            } else {
                None
            },
        }
    }

    #[test]
    fn load_meeting_detail_round_trips_mic_only() {
        let conn = fresh_db();
        persist_meeting(&conn, &fixture_request("u-mic", MeetingSource::Mic)).unwrap();
        let detail = load_meeting_detail(&conn, "u-mic")
            .unwrap()
            .expect("present");
        assert_eq!(detail.uuid, "u-mic");
        assert_eq!(detail.source, MeetingSource::Mic);
        assert_eq!(detail.status, MeetingStatus::Complete);
        assert_eq!(detail.formatted_mic.as_deref(), Some("mic body for u-mic"));
        assert!(detail.formatted_sys.is_none());
        assert!(detail.formatted_merged.is_none());
    }

    #[test]
    fn load_meeting_detail_round_trips_both_channels() {
        let conn = fresh_db();
        persist_meeting(&conn, &fixture_request("u-both", MeetingSource::Both)).unwrap();
        let detail = load_meeting_detail(&conn, "u-both")
            .unwrap()
            .expect("present");
        assert!(detail.formatted_mic.is_some());
        assert!(detail.formatted_sys.is_some());
        assert!(detail.formatted_merged.is_some());
        assert_eq!(detail.source, MeetingSource::Both);
    }

    #[test]
    fn load_meeting_detail_returns_none_for_unknown_uuid() {
        let conn = fresh_db();
        assert!(load_meeting_detail(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn list_meetings_orders_started_at_desc() {
        let conn = fresh_db();
        let mut req_old = fixture_request("u-old", MeetingSource::Mic);
        req_old.started_at = "2026-01-01T10:00:00Z".into();
        let mut req_new = fixture_request("u-new", MeetingSource::Mic);
        req_new.started_at = "2026-12-31T10:00:00Z".into();
        persist_meeting(&conn, &req_old).unwrap();
        persist_meeting(&conn, &req_new).unwrap();

        let list = list_meetings(&conn, 10, 0).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].uuid, "u-new");
        assert_eq!(list[1].uuid, "u-old");
    }

    #[test]
    fn list_meetings_respects_limit_and_offset() {
        let conn = fresh_db();
        for i in 0..5 {
            let uuid = format!("u-{i:02}");
            let mut req = fixture_request(&uuid, MeetingSource::Mic);
            req.started_at = format!("2026-05-{:02}T10:00:00Z", 10 + i);
            persist_meeting(&conn, &req).unwrap();
        }
        let page1 = list_meetings(&conn, 2, 0).unwrap();
        let page2 = list_meetings(&conn, 2, 2).unwrap();
        let page3 = list_meetings(&conn, 2, 4).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 2);
        assert_eq!(page3.len(), 1);
        // No uuid appears twice across the pages.
        let mut all: Vec<_> = page1
            .iter()
            .chain(page2.iter())
            .chain(page3.iter())
            .map(|m| m.uuid.clone())
            .collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn delete_meeting_cascades_transcripts() {
        let conn = fresh_db();
        persist_meeting(&conn, &fixture_request("u-del", MeetingSource::Both)).unwrap();
        let n_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM meeting_transcripts", [], |r| r.get(0))
            .unwrap();
        assert!(n_before > 0);

        let removed = delete_meeting(&conn, "u-del").unwrap();
        assert!(removed, "delete should report a row was deleted");

        let n_sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM meeting_sessions", [], |r| r.get(0))
            .unwrap();
        let n_transcripts: i64 = conn
            .query_row("SELECT COUNT(*) FROM meeting_transcripts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_sessions, 0);
        assert_eq!(
            n_transcripts, 0,
            "ON DELETE CASCADE should drop transcripts"
        );
    }

    #[test]
    fn delete_meeting_returns_false_for_unknown_uuid() {
        let conn = fresh_db();
        assert!(!delete_meeting(&conn, "missing").unwrap());
    }

    #[test]
    fn search_meetings_finds_formatted_text() {
        let conn = fresh_db();
        let mut req = fixture_request("u-search", MeetingSource::Mic);
        req.formatted_mic = Some("the quick brown fox jumped over the lazy dog".into());
        persist_meeting(&conn, &req).unwrap();
        let hits = search_meetings(&conn, "quick", 10).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].uuid, "u-search");
        assert!(hits[0].snippet.contains("<mark>"));
        assert_eq!(hits[0].channel, "mic");
    }

    #[test]
    fn search_meetings_empty_query_returns_empty() {
        let conn = fresh_db();
        persist_meeting(&conn, &fixture_request("u-x", MeetingSource::Mic)).unwrap();
        assert!(search_meetings(&conn, "", 10).unwrap().is_empty());
        assert!(search_meetings(&conn, "   ", 10).unwrap().is_empty());
    }

    #[test]
    fn search_meetings_returns_empty_for_no_matches() {
        let conn = fresh_db();
        persist_meeting(&conn, &fixture_request("u-y", MeetingSource::Mic)).unwrap();
        let hits = search_meetings(&conn, "zzzz_no_such_token", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn meeting_status_serializes_as_lowercase() {
        let s = serde_json::to_string(&MeetingStatus::Partial).unwrap();
        assert_eq!(s, "\"partial\"");
        let parsed: MeetingStatus = serde_json::from_str("\"complete\"").unwrap();
        assert_eq!(parsed, MeetingStatus::Complete);
    }

    #[test]
    fn meeting_source_serializes_as_lowercase() {
        let s = serde_json::to_string(&MeetingSource::Both).unwrap();
        assert_eq!(s, "\"both\"");
        let parsed: MeetingSource = serde_json::from_str("\"mic\"").unwrap();
        assert_eq!(parsed, MeetingSource::Mic);
    }
}
