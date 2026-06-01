//! History archive phase (ADR 0053 §D7, mb-i14b / 1E.4).
//!
//! Runs AFTER the seal + `mark_done` transaction in
//! [`super::filing::process_one`], so a failure here can't roll
//! back a successful filing. The caller logs + swallows the error
//! and `vault::history::reconcile_history` recovers on demand.
//!
//! Split out of `worker.rs` during Wave 1E.7 Part 2 (`mb-5lla`).
//! Behaviour is unchanged.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::sessions as db_sessions;
use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};
use crate::vault::history::{archive_session_history, HistoryArchiveInput};
use crate::vault::writer::CommitOutcome;

use super::transcripts::load_transcript_stage;

/// History archive step. Returns `Ok(())` even when the archive
/// was a no-op (idempotent re-call on a session whose sidecar
/// already exists). The distinction is logged via
/// `HistoryArchiveOutcome.archived` inside the helper; the worker
/// doesn't care to differentiate.
pub(super) fn maybe_archive_history(
    conn: &Arc<Mutex<Connection>>,
    session_id: i64,
    outcome: &CommitOutcome,
    captured_iso: &str,
) -> AppResult<()> {
    // Snapshot under a single short-lived lock: session metadata
    // (uuid, capture_kind, audio_blob_path) + transcripts (raw +
    // cleaned/final) + vault root.
    let (vault_root, session_uuid, capture_kind_db_str, audio_blob_path, raw_text, cleaned_text) = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in maybe_archive_history".into()))?;

        let vault_path: Option<String> = Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten();
        let vault_path_str = match vault_path {
            Some(s) if !s.trim().is_empty() => s,
            _ => {
                // Vault was unconfigured mid-run -- nothing to do.
                return Ok(());
            }
        };

        let session = db_sessions::get_by_id(&c, session_id)?.ok_or_else(|| {
            AppError::Other(format!(
                "maybe_archive_history: session id={session_id} disappeared mid-run"
            ))
        })?;

        let raw = load_transcript_stage(&c, session_id, "raw")?.unwrap_or_default();
        // Prefer the post-cleanup stage; fall back to cleaned, then
        // to empty (sessions whose cleanup pass never ran still
        // archive cleanly).
        let cleaned = load_transcript_stage(&c, session_id, "final")?
            .or(load_transcript_stage(&c, session_id, "cleaned")?)
            .unwrap_or_default();

        (
            std::path::PathBuf::from(vault_path_str),
            session.uuid,
            session.capture_kind.as_db_str().to_string(),
            session.audio_blob_path,
            raw,
            cleaned,
        )
    };

    // Derive the entry filename from the vault-relative POSIX-style
    // path the 1E.3 writer recorded. Last `/`-separated component is
    // the bare filename (always emitted with forward slashes, so the
    // split is host-OS independent).
    let entry_filename = outcome
        .vault_relative_path
        .rsplit('/')
        .next()
        .unwrap_or(&outcome.vault_relative_path)
        .to_string();

    let audio_path_buf = audio_blob_path.as_deref().map(std::path::PathBuf::from);

    let input = HistoryArchiveInput {
        session_id,
        session_uuid: &session_uuid,
        capture_kind: &capture_kind_db_str,
        captured_at: captured_iso,
        raw_transcript: &raw_text,
        cleaned_transcript: &cleaned_text,
        entry_id: &outcome.entry_id,
        entry_filename: &entry_filename,
        vault_file_hash: &outcome.file_hash,
        audio_blob_path: audio_path_buf.as_deref(),
    };

    let archived = archive_session_history(&input, &vault_root)?;
    tracing::info!(
        target: "kg::worker",
        session_id,
        json = %archived.json_path.display(),
        audio = ?archived.audio_path.as_ref().map(|p| p.display().to_string()),
        archived = archived.archived,
        "history archive step complete"
    );
    Ok(())
}
