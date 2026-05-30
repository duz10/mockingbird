//! KG text-note ingest path — Wave 1D.3 (`mb-0gt6`, ADR 0052).
//!
//! Sibling of [`crate::dictation::ingest`] (the file-import / mobile-
//! inbox path). Both write into `sessions` + `transcripts` with full
//! provenance; the difference is **what** the raw text came from:
//!
//! - `dictation::ingest`     → PCM → VAD → Whisper → cleanup → DB
//! - `kg::ingest_text`       → user-typed string → DB (no audio, no
//!   STT, no cleanup LLM pass)
//!
//! After the DB write the path branches on the `KgGraphEnabled`
//! setting:
//!
//! - graph **off** → row exists in `sessions` with `capture_kind =
//!   'kg-note-text'` but the KG queue stays empty. If the user later
//!   flips the toggle on, a Phase 1E backfill will pick the row up.
//! - graph **on**  → the same `enqueue_for_filing` call the
//!   dictation-tail source-gate uses fires immediately.
//!
//! ## Divergence from ADR 0052 §D3
//!
//! ADR 0052 §D3 originally proposed routing text notes around the
//! `sessions` table via a synthetic `entry_id` in `kg_filing_queue`.
//! Wave 1D.3 (this module) supersedes that: the simpler design reuses
//! the existing `sessions` + `transcripts` + `enqueue_for_filing`
//! plumbing, with the Dictations history page filtering by
//! `capture_kind != 'kg-note-text'` to keep text notes out of the
//! dictation view. Rationale: every other surface (provenance ladder,
//! parity probe, vault export-job source-filter, KG worker) already
//! reads from `sessions.id`, so a parallel write path would force
//! either a wider `enqueue_for_filing` API or a join through a new
//! discriminator column. The carry-forward note in [`CaptureKind`]
//! and the Wave 1D.6 seal will pick this up as a formal note on
//! ADR 0052.
//!
//! [`CaptureKind`]: crate::db::sessions::CaptureKind
//!
//! ## Why no shared `run_from_text` extraction
//!
//! Phase doc §"Refactor seam" anticipated extracting
//! `kg::pipeline::run_from_text` as a shared entry point for both
//! audio and text. Inspection of [`crate::kg::pipeline::run_pipeline`] shows
//! it already takes `&str` (the dictation transcript text) — there
//! is no audio-shaped entry point to factor out. The audio path
//! writes its text into `transcripts` (raw / cleaned / final), then
//! the KG worker reads that text back and calls `run_pipeline`. The
//! text-note path produces the same `transcripts.text` row from a
//! different source (user input vs. Whisper output); from the KG
//! worker's perspective the two paths are already indistinguishable.
//! Saved ~100 LoC of unnecessary plumbing; logged in LESSONS.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::sessions::{
    self, CaptureKind, NewSession, ProcessingCompletion, SessionSource, SessionStatus, StartMode,
};
use crate::db::transcripts;
use crate::dictation::events::SessionsEventBus;
use crate::dictation::{format_secs_as_iso, resolve_active_mode_from_conn, OrchestratorConfig};
use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};

/// Sentinel string used for `sessions.hotkey_pressed` on text-note
/// rows. There is no key press involved; we still need a non-NULL
/// value (the column is NOT NULL with no default) and a stable
/// sentinel makes log + UI filtering trivial. Mirrors the
/// `file-import:*` convention from [`crate::dictation::ingest`].
const TEXT_NOTE_HOTKEY_SENTINEL: &str = "kg-text-note";

/// Cleanup model identifier stored on the cleaned/final transcripts.
/// Text notes have no cleanup pass — the user's exact input is what
/// gets filed. The sentinel mirrors `LlmCleaner::model_name` for the
/// passthrough case (an empty / "passthrough" string would falsely
/// imply an LLM was consulted).
const TEXT_NOTE_CLEANUP_MODEL: &str = "kg-text-note (no cleanup)";

/// Ingest one user-typed KG text note.
///
/// **Atomic**: holds the DB mutex for the whole insert + enqueue
/// sequence so the new session row and its filing-queue entry (if
/// the graph is on) land in lock-step. Returns the inserted
/// `sessions.id` on success.
///
/// Errors are propagated up to the IPC handler: this path doesn't
/// have a "persist an error row" fallback like
/// [`crate::dictation::ingest::headless_ingest`] does, because the
/// user is in front of the screen and a toast on failure is the
/// right UX (vs. a silent-loss-shaped row in the history).
///
/// `events` fires `history:session-saved` after the lock drops so
/// the React side can refetch — the Dictations page filters this
/// row out (text notes are KG-only at the UI surface), but the KG
/// dashboard's "recent activity" band watches the same event and
/// will re-render to include the new entry.
pub fn ingest_text_note(
    db: &Arc<Mutex<Connection>>,
    events: &dyn SessionsEventBus,
    config: &OrchestratorConfig,
    text: &str,
) -> AppResult<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other(
            "kg::ingest_text_note: text is empty after trim".into(),
        ));
    }
    let started_at_iso = now_iso_utc();

    let conn = db
        .lock()
        .map_err(|_| AppError::Other("kg::ingest_text_note: db mutex poisoned".into()))?;

    // Pin the active mode the same way dictation does. Text notes
    // record provenance against whatever mode is active at file
    // time — the user can later see "this text note was filed
    // while in <mode>".
    let resolved =
        resolve_active_mode_from_conn(&conn, config.mode_id, &config.mode_slug, config.prompt_id);

    let session_id = persist_text_note_row(
        &conn,
        &TextNoteRow {
            started_at_iso: &started_at_iso,
            text: trimmed,
            resolved_mode_id: resolved.mode_id,
            resolved_prompt_id: resolved.prompt_id,
            dictionary_snapshot_id: config.dictionary_snapshot_id,
            example_set_id: config.example_set_id,
        },
    )?;

    // Source gate equivalent. The dictation tail goes through
    // `try_enqueue_for_kg_filing` (outcome + source + toggle); for
    // text notes the source gate is satisfied by definition (this
    // function is only called from the KG-screen text-note IPC),
    // there is no injection outcome to gate on, and the toggle is
    // the only remaining check. Same `Settings::get` + early-return
    // pattern as the dictation tail's toggle gate.
    let kg_enabled = Settings::new(&conn)
        .get::<bool>(SettingKey::KgGraphEnabled)
        .unwrap_or(false);
    if kg_enabled {
        if let Err(e) = super::enqueue_for_filing(&conn, session_id, &started_at_iso) {
            // Same non-regressing posture as the dictation tail: a
            // KG enqueue failure must not roll back the session
            // row. The user's text exists in the durable store; the
            // queue can be retried via Phase 1E backfill or the
            // Settings "requeue failed" surface.
            tracing::warn!(
                error = ?e,
                session_id,
                "kg::ingest_text_note: enqueue failed; row persisted, queue empty (kg-graph-failure-non-regressing)"
            );
        }
    } else {
        tracing::debug!(
            session_id,
            "kg::ingest_text_note: graph off; row persisted without enqueue"
        );
    }

    drop(conn);
    events.emit_session_saved(session_id);
    tracing::info!(
        session_id,
        chars = trimmed.chars().count(),
        kg_enabled,
        "kg::ingest_text_note: success"
    );
    Ok(session_id)
}

/// Inputs the persistence helper needs. Mirrors the
/// `IngestPersistParams` pattern in `dictation::ingest`: one struct
/// per persistence operation so future provenance additions don't
/// ripple through the call site as another positional argument.
struct TextNoteRow<'a> {
    started_at_iso: &'a str,
    text: &'a str,
    resolved_mode_id: i64,
    resolved_prompt_id: i64,
    dictionary_snapshot_id: i64,
    example_set_id: i64,
}

/// Insert the session row + the three transcript stages.
///
/// Why all three stages: the `kg_filing_queue` worker (and every
/// future read-side surface) reads the `cleaned` or `final` stage
/// via `transcripts::get_stage`. Writing all three keeps the text
/// note's row indistinguishable in shape from a dictation row at
/// the storage layer — fewer special-case branches downstream.
/// `model_used` on cleaned/final is the sentinel above so an audit
/// query can identify "no LLM touched this text" rows precisely.
fn persist_text_note_row(conn: &Connection, p: &TextNoteRow<'_>) -> AppResult<i64> {
    let new = NewSession {
        uuid: new_uuid(),
        mode_id: p.resolved_mode_id,
        hotkey_pressed: TEXT_NOTE_HOTKEY_SENTINEL.to_string(),
        started_at: p.started_at_iso.to_string(),
        recording_ended_at: p.started_at_iso.to_string(),
        status: SessionStatus::Processing,
        foreground_app: None,
        foreground_window_title: None,
        // Text notes have no audio. Persist 0 (the column is NOT
        // NULL) — a UI filter on `audio_duration_ms = 0` is a clean
        // heuristic for "non-audio session" if anything ever needs
        // one beyond the explicit `capture_kind` discriminator.
        audio_duration_ms: 0,
        audio_blob_path: None,
        prompt_id: p.resolved_prompt_id,
        dictionary_snapshot_id: p.dictionary_snapshot_id,
        example_set_id: p.example_set_id,
        // KG-screen text input is in-app by definition (no key
        // press; the IPC is fired from a textarea submit).
        start_mode: StartMode::InApp,
        // Source is the local desktop UI; the file-import / mobile-
        // inbox variants are reserved for audio paths and would
        // mis-tag this row for the vault export-job filter.
        source: SessionSource::Desktop,
        capture_kind: CaptureKind::KgNoteText,
    };
    let id = sessions::insert(conn, &new)?;

    // Same trio of writes the dictation tail does. We tolerate
    // individual transcript writes failing — the session row is
    // the durable record; a failed stage write logs and continues
    // so the status update still lands and the row stays usable
    // by everything that doesn't need the missing stage.
    if let Err(e) = transcripts::insert_raw(conn, id, p.text) {
        tracing::warn!(error = ?e, session_id = id, "kg::ingest_text_note: raw transcript write failed");
    }
    if let Err(e) = transcripts::insert_cleaned(conn, id, p.text, TEXT_NOTE_CLEANUP_MODEL) {
        tracing::warn!(error = ?e, session_id = id, "kg::ingest_text_note: cleaned transcript write failed");
    }
    if let Err(e) = transcripts::insert_final(conn, id, p.text, Some(TEXT_NOTE_CLEANUP_MODEL)) {
        tracing::warn!(error = ?e, session_id = id, "kg::ingest_text_note: final transcript write failed");
    }

    sessions::update_processing_complete(
        conn,
        id,
        &ProcessingCompletion {
            completed_at: now_iso_utc(),
            status: SessionStatus::Complete,
            // No latency to report — the path is synchronous from
            // the user's keypress and stage timings would be
            // misleading noise in the Insights aggregation.
            stt_latency_ms: None,
            cleanup_latency_ms: None,
            injection_latency_ms: None,
            // No injection happened; matches the dictation
            // headless-ingest convention for "no inject" rows.
            injection_status: None,
        },
    )?;
    Ok(id)
}

fn now_iso_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_secs_as_iso(secs)
}

fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    //! Unit coverage for the text-note ingest path. Lives here so the
    //! cargo-test-launch failure on this box (LESSONS P2) doesn't
    //! block them — these are pure-Rust, no whisper-rs / ort / cuda
    //! deps, so the throwaway-crate fallback works.

    use std::sync::{Arc, Mutex};

    use rusqlite::Connection;

    use super::*;
    use crate::db::migrations::apply_all;
    use crate::db::transcripts::{self as tx, Stage};
    use crate::dictation::events::SessionsEventBus;
    use crate::dictation::runtime::bootstrap_provenance_rows;
    use crate::settings::{model::SettingKey, Settings};

    fn fresh_db() -> Arc<Mutex<Connection>> {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_all(&mut conn).unwrap();
        bootstrap_provenance_rows(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn fresh_config(db: &Arc<Mutex<Connection>>) -> OrchestratorConfig {
        let conn = db.lock().unwrap();
        let (dict_id, example_id) = bootstrap_provenance_rows(&conn).unwrap();
        // Seed a minimal mode + prompt row so resolve_* has something
        // to find. Mirror the bootstrap fixture the runtime uses.
        let prompt_id: i64 = conn
            .query_row("SELECT id FROM prompts ORDER BY id LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap_or(1);
        let (mode_id, slug): (i64, String) = conn
            .query_row("SELECT id, slug FROM modes ORDER BY id LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        OrchestratorConfig {
            mode_id,
            mode_slug: slug,
            prompt_id,
            dictionary_snapshot_id: dict_id,
            example_set_id: example_id,
            hotkey_label: "test".into(),
        }
    }

    /// Test double for the events bus. Counts emit calls so we can
    /// assert the IPC handler will fire the React refetch.
    #[derive(Default)]
    struct CountingBus {
        emitted: std::sync::Mutex<Vec<i64>>,
    }
    impl SessionsEventBus for CountingBus {
        fn emit_session_saved(&self, id: i64) {
            self.emitted.lock().unwrap().push(id);
        }
    }

    fn queue_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM kg_filing_queue", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn empty_text_is_rejected_before_any_db_write() {
        let db = fresh_db();
        let cfg = fresh_config(&db);
        let bus = CountingBus::default();
        let err = ingest_text_note(&db, &bus, &cfg, "   \n  ").unwrap_err();
        assert!(err.to_string().contains("empty"));
        let conn = db.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "empty text must not insert a row");
        assert_eq!(queue_count(&conn), 0);
        assert!(bus.emitted.lock().unwrap().is_empty());
    }

    #[test]
    fn graph_off_persists_row_but_does_not_enqueue() {
        let db = fresh_db();
        let cfg = fresh_config(&db);
        let bus = CountingBus::default();
        // KgGraphEnabled defaults to false; assert explicitly.
        {
            let conn = db.lock().unwrap();
            let enabled: bool = Settings::new(&conn)
                .get::<bool>(SettingKey::KgGraphEnabled)
                .unwrap();
            assert!(!enabled, "default toggle must be off (ADR 0050)");
        }
        let id = ingest_text_note(&db, &bus, &cfg, "hello text note").unwrap();
        let conn = db.lock().unwrap();
        let kind: String = conn
            .query_row(
                "SELECT capture_kind FROM sessions WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "kg-note-text");
        assert_eq!(
            queue_count(&conn),
            0,
            "graph off must skip enqueue (toggle-gate)"
        );
        assert_eq!(bus.emitted.lock().unwrap().as_slice(), &[id]);
    }

    #[test]
    fn graph_on_persists_row_and_enqueues() {
        let db = fresh_db();
        let cfg = fresh_config(&db);
        let bus = CountingBus::default();
        {
            let conn = db.lock().unwrap();
            Settings::new(&conn)
                .set_raw(SettingKey::KgGraphEnabled, &serde_json::Value::Bool(true))
                .unwrap();
        }
        let id = ingest_text_note(&db, &bus, &cfg, "filed text note").unwrap();
        let conn = db.lock().unwrap();
        assert_eq!(queue_count(&conn), 1);
        let queued_entry: i64 = conn
            .query_row(
                "SELECT entry_id FROM kg_filing_queue WHERE state = 'pending'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            queued_entry, id,
            "the queued entry_id must equal the inserted sessions.id"
        );
    }

    #[test]
    fn all_three_transcript_stages_are_written_with_user_text() {
        let db = fresh_db();
        let cfg = fresh_config(&db);
        let bus = CountingBus::default();
        let user_text = "lunch with Alice next Tuesday";
        let id = ingest_text_note(&db, &bus, &cfg, user_text).unwrap();
        let conn = db.lock().unwrap();
        for stage in [Stage::Raw, Stage::Cleaned, Stage::Final] {
            let got = tx::get_stage(&conn, id, stage)
                .unwrap()
                .unwrap_or_else(|| panic!("stage {stage:?} missing for text-note session {id}"));
            assert_eq!(got.text, user_text);
        }
    }

    #[test]
    fn whitespace_is_trimmed_before_write() {
        let db = fresh_db();
        let cfg = fresh_config(&db);
        let bus = CountingBus::default();
        let id = ingest_text_note(&db, &bus, &cfg, "  padded text  \n").unwrap();
        let conn = db.lock().unwrap();
        let raw = tx::get_stage(&conn, id, Stage::Raw).unwrap().unwrap();
        assert_eq!(raw.text, "padded text");
    }
}
