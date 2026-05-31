//! KG history archive — Phase 1E Wave 1E.4 (`mb-i14b`, ADR 0053 §D7).
//!
//! After the worker finishes the two-phase commit + queue seal for
//! a KG session (1E.3), this module writes a per-session JSON
//! sidecar at:
//!
//! ```text
//! <vault>/Knowledge Graph/History/<YYYY-MM>/<session-uuid>.json
//! ```
//!
//! and (for audio captures) moves the source recording into the
//! same month-bucket as `<session-uuid>.<ext>`. The sidecar + audio
//! pair are the spec §9 / §7.1 re-processing safety net: raw
//! transcript + processed audio kept untouched, regardless of what
//! happens to the projected entry on disk or in the DB.
//!
//! ## Why this is phase 4 (post-seal), not phase 2.5 (pre-seal)
//!
//! Same non-fatal-to-queue philosophy as 1E.3's vault projection.
//! Once the entry markdown is sealed onto `sessions.vault_path` +
//! `sessions.entry_id`, the system is in a durable, user-visible
//! state. The history archive is a strictly downstream artifact —
//! its failure should never roll back a successful filing. If the
//! archive fails, [`reconcile_history`] picks up the slack on demand
//! (1E.5 timer / IPC).
//!
//! ## Canonical JSON shape (ADR 0053 §D7 + Wave 1E.4 kickoff)
//!
//! Deterministic field order (struct declaration order under serde):
//!
//! ```json
//! {
//!   "session_uuid": "...",
//!   "session_id": 42,
//!   "capture_kind": "kg-note",
//!   "captured_at": "2026-06-15T14:32:01Z",
//!   "raw_transcript": "...",
//!   "cleaned_transcript": "...",
//!   "entry_id": "...",
//!   "entry_filename": "2026-06-15-buy-milk__abcd1234.md",
//!   "vault_file_hash": "<hex sha256>",
//!   "archive_version": 1
//! }
//! ```
//!
//! Rules: 2-space indent, LF line endings on every platform, no
//! trailing whitespace, exactly one trailing newline. Powered by
//! `serde_json::to_string_pretty` whose default `PrettyFormatter`
//! emits LF newlines + 2-space indent unconditionally; we append a
//! single `\n` at the end so the file always terminates with one
//! newline.
//!
//! ## Idempotency
//!
//! Calling [`archive_session_history`] when the JSON sidecar already
//! exists is a no-op — same shape as the 1E.3 commit re-call. The
//! audio file is only moved if (a) the source path exists AND (b) the
//! target path does not. This means a partial-failure mid-archive
//! converges on retry without overwriting completed work.
//!
//! ## Audio file move semantics
//!
//! The audio comes from `sessions.audio_blob_path` — the in-app
//! recording pipeline's working dir (usually under `%APPDATA%`).
//! That's almost certainly NOT on the same physical volume as the
//! vault, so a plain `fs::rename` will fail with EXDEV on Unix /
//! `ERROR_NOT_SAME_DEVICE` on Windows. We try rename first, then
//! fall back to copy-then-delete. This sidesteps the cross-device
//! footgun without paying the copy cost on the happy path where the
//! recording dir and vault happen to share a volume.

#![allow(missing_docs)]
// Module is gated behind the KG toggle + vault config in production;
// some helpers (reconcile_history) compile to `dead_code` until 1E.5
// + the IPC layer wire them up. Same pattern as `kg::worker` and
// `vault::writer`.
#![allow(dead_code)]

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::error::{AppError, AppResult};

use super::kg_layout::kg_subtree_paths;

/// Current sidecar schema version. Bumped only on a deliberate,
/// non-backward-compatible change to the JSON shape. The reverse-
/// watcher (1E.5) walks `History/` too and uses `archive_version`
/// plus the `.json` extension as discriminators ("this is a
/// history sidecar, not user content").
pub const ARCHIVE_VERSION: u32 = 1;

// ── Input + output shapes ─────────────────────────────────────────

/// Caller-supplied inputs for [`archive_session_history`]. Kept as
/// borrowed `&str` / `&Path` so the worker doesn't have to clone its
/// snapshot data into a fresh allocation just to call this.
#[derive(Debug, Clone)]
pub struct HistoryArchiveInput<'a> {
    /// `sessions.id` — useful for forensic correlation back to the
    /// DB row from a stray sidecar on disk.
    pub session_id: i64,
    /// `sessions.uuid`. Drives both the sidecar filename and the
    /// archived audio's filename.
    pub session_uuid: &'a str,
    /// Wire string of `sessions.capture_kind` (e.g. `"kg-note"`,
    /// `"kg-note-text"`). The worker only calls this for KG kinds;
    /// the value is recorded verbatim for downstream consumers.
    pub capture_kind: &'a str,
    /// RFC 3339 UTC timestamp from `sessions.started_at`. Drives
    /// the `History/<YYYY-MM>/` bucket — parse-failure here is a
    /// hard error since we can't pick a bucket without it.
    pub captured_at: &'a str,
    /// Verbatim Whisper output (`transcripts.stage='raw'`). For
    /// `kg-note-text` this is the user's typed input (the text-note
    /// path writes the same text into all three stages — see
    /// `kg::ingest_text`).
    pub raw_transcript: &'a str,
    /// Post-cleanup pipeline output (`transcripts.stage='cleaned'`
    /// or `'final'`). Empty string is acceptable for sessions where
    /// no cleanup stage exists yet.
    pub cleaned_transcript: &'a str,
    /// KG entry UUID (the `sessions.entry_id` set during the 1E.3
    /// seal). Always populated when this function is called.
    pub entry_id: &'a str,
    /// Just the filename (NOT the vault-relative path) of the
    /// projected markdown file. Lets a human or `rg` jump directly
    /// from the sidecar to the entry without splitting paths.
    pub entry_filename: &'a str,
    /// `sessions.vault_file_hash` — the lowercase hex SHA-256 of
    /// the markdown bytes. Useful for cross-checking that the entry
    /// on disk is the one this sidecar describes.
    pub vault_file_hash: &'a str,
    /// Working-dir path of the source audio recording. `None` for
    /// text-only captures (`kg-note-text`) and for any KG audio
    /// capture whose `sessions.audio_blob_path` was somehow NULL.
    pub audio_blob_path: Option<&'a Path>,
}

/// Outcome of a successful archive call. `archived = false` means
/// the sidecar already existed and the call was an idempotent
/// no-op (no files written, no audio moved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryArchiveOutcome {
    /// Absolute on-disk path of the JSON sidecar.
    pub json_path: PathBuf,
    /// Absolute on-disk path of the archived audio file, if any.
    /// `None` when the input had no audio OR the audio archive
    /// step was skipped (idempotent re-call where the target audio
    /// already exists).
    pub audio_path: Option<PathBuf>,
    /// `true` if this call produced new on-disk state. `false` if
    /// the JSON sidecar already existed (idempotent re-run).
    pub archived: bool,
}

/// Report of an on-demand reconcile pass over the history archive.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HistoryReconcileReport {
    /// Sessions that have a sealed entry projection
    /// (`sessions.entry_id` set) but no JSON sidecar at the
    /// expected `History/<YYYY-MM>/<uuid>.json` path. v1: counted
    /// plus logged; rewrite recovery is deferred to the IPC layer.
    /// (See `mb-43xw` for the reconcile-vault IPC sibling.)
    pub missing_sidecar_count: usize,
    /// JSON sidecars on disk that no `sessions` row claims via its
    /// uuid. Read-only count; nothing is deleted.
    pub orphan_sidecar_count: usize,
}

// ── Serialization (pure) ──────────────────────────────────────────

/// The on-disk JSON shape. Field order = struct declaration order =
/// canonical key order (`session_uuid` first, `archive_version`
/// last). `#[serde]` preserves declaration order when serializing a
/// struct, so this is the single source of truth for the contract.
#[derive(Debug, Serialize)]
struct SidecarRecord<'a> {
    session_uuid: &'a str,
    session_id: i64,
    capture_kind: &'a str,
    captured_at: &'a str,
    raw_transcript: &'a str,
    cleaned_transcript: &'a str,
    entry_id: &'a str,
    entry_filename: &'a str,
    vault_file_hash: &'a str,
    archive_version: u32,
}

/// Pure helper: serialize the sidecar to its canonical bytes.
///
/// Split out from [`archive_session_history`] so:
///   1. Tests can pin the canonical form without touching disk.
///   2. The 1E.5 reverse-watcher (when it eventually parses sidecars
///      to confirm "history blob, not user content") shares the
///      same canonical form definition.
///
/// The output is UTF-8, LF-terminated, with exactly one trailing
/// newline. `serde_json::to_string_pretty`'s default formatter emits
/// LF newlines + 2-space indent on every platform; we append the
/// final `\n` ourselves.
pub fn serialize_sidecar(input: &HistoryArchiveInput<'_>) -> AppResult<Vec<u8>> {
    let record = SidecarRecord {
        session_uuid: input.session_uuid,
        session_id: input.session_id,
        capture_kind: input.capture_kind,
        captured_at: input.captured_at,
        raw_transcript: input.raw_transcript,
        cleaned_transcript: input.cleaned_transcript,
        entry_id: input.entry_id,
        entry_filename: input.entry_filename,
        vault_file_hash: input.vault_file_hash,
        archive_version: ARCHIVE_VERSION,
    };
    let mut s = serde_json::to_string_pretty(&record).map_err(|e| {
        AppError::Vault(format!(
            "serialize_sidecar: serde_json failed for session {} -- {e}",
            input.session_uuid
        ))
    })?;
    s.push('\n');
    Ok(s.into_bytes())
}

/// Derive the `YYYY-MM` month-bucket directory name from a captured
/// timestamp. Strict — invalid input returns an error instead of
/// silently bucketing into the epoch month, because writing to the
/// wrong bucket would silently mis-file a sidecar where a human or
/// reconcile pass would never find it.
pub fn month_bucket(captured_at: &str) -> AppResult<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(captured_at).map_err(|e| {
        AppError::Vault(format!(
            "month_bucket: unparseable captured_at `{captured_at}` -- {e}"
        ))
    })?;
    let dt = dt.with_timezone(&chrono::Utc);
    Ok(format!(
        "{:04}-{:02}",
        dt.format("%Y").to_string().parse::<i32>().unwrap_or(1970),
        dt.format("%m").to_string().parse::<u32>().unwrap_or(1)
    ))
}

/// Compute the absolute JSON sidecar path for an input WITHOUT
/// touching disk. Mirrors [`super::writer::compute_artifact`]'s
/// pure-helper pattern.
pub fn sidecar_path_for(input: &HistoryArchiveInput<'_>, vault_root: &Path) -> AppResult<PathBuf> {
    let bucket = month_bucket(input.captured_at)?;
    let subtree = kg_subtree_paths(vault_root);
    Ok(subtree
        .history
        .join(&bucket)
        .join(format!("{}.json", input.session_uuid)))
}

/// Compute the absolute archived-audio path for an input, given the
/// source audio's extension. Returns `None` when the input has no
/// audio path attached (text-only captures).
pub fn audio_archive_path_for(
    input: &HistoryArchiveInput<'_>,
    vault_root: &Path,
) -> AppResult<Option<PathBuf>> {
    let Some(src) = input.audio_blob_path else {
        return Ok(None);
    };
    let bucket = month_bucket(input.captured_at)?;
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav")
        .to_string();
    let subtree = kg_subtree_paths(vault_root);
    Ok(Some(
        subtree
            .history
            .join(&bucket)
            .join(format!("{}.{ext}", input.session_uuid)),
    ))
}

// ── Public entry point ────────────────────────────────────────────

/// Write the JSON sidecar + (if present) move the source audio into
/// the month-bucket. Idempotent: a re-call on an already-archived
/// session is a no-op.
///
/// The caller (`kg::worker::process_one`) is responsible for the
/// KG-toggle / vault-configured gates BEFORE invoking this. Failure
/// here is non-fatal to the queue — the worker logs + carries on,
/// and [`reconcile_history`] eventually recovers.
pub fn archive_session_history(
    input: &HistoryArchiveInput<'_>,
    vault_root: &Path,
) -> AppResult<HistoryArchiveOutcome> {
    let json_path = sidecar_path_for(input, vault_root)?;

    // Idempotency check: if the sidecar already exists, we assume
    // both halves of the archive (json + audio) completed on a
    // prior run. The audio move is conditioned on (src exists AND
    // target missing) below, so even a half-completed prior run
    // converges safely on re-invocation.
    if json_path.exists() {
        tracing::debug!(
            target: "vault::history",
            session_uuid = %input.session_uuid,
            path = %json_path.display(),
            "history archive already present; skipping (idempotent no-op)"
        );
        return Ok(HistoryArchiveOutcome {
            json_path,
            audio_path: audio_archive_path_for(input, vault_root)?.filter(|p| p.exists()),
            archived: false,
        });
    }

    // Bucket dir on demand. `create_dir_all` is a no-op when the
    // dir already exists -- mirrors `bootstrap_kg_subtree`.
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::Vault(format!(
                "archive_session_history: failed to ensure bucket dir {} -- {e}",
                parent.display()
            ))
        })?;
    }

    // Step 1 -- JSON sidecar (atomic temp-sibling + rename, same
    // shape as `vault::writer::write_atomic`).
    let bytes = serialize_sidecar(input)?;
    write_atomic(&json_path, &bytes).map_err(|e| {
        AppError::Vault(format!(
            "archive_session_history: atomic write to {} failed -- {e}",
            json_path.display()
        ))
    })?;

    // Step 2 -- audio move (optional). Failure here is logged but
    // non-fatal -- the JSON sidecar is the safety-net primary; the
    // audio is the safety-net bonus. Losing the audio while
    // keeping the sidecar is strictly less bad than rolling back
    // the sidecar.
    let audio_target = audio_archive_path_for(input, vault_root)?;
    let final_audio = match (audio_target, input.audio_blob_path) {
        (Some(target), Some(src)) => {
            if !src.exists() {
                // Source vanished between session-finalize and
                // archive (user cleared their working dir, etc).
                // Common enough not to warn loudly.
                tracing::info!(
                    target: "vault::history",
                    session_uuid = %input.session_uuid,
                    src = %src.display(),
                    "audio source missing at archive time; skipping audio move"
                );
                None
            } else if target.exists() {
                // Idempotent: previous archive already moved it.
                Some(target)
            } else if let Err(e) = move_file(src, &target) {
                tracing::warn!(
                    target: "vault::history",
                    session_uuid = %input.session_uuid,
                    src = %src.display(),
                    dest = %target.display(),
                    error = %e,
                    "audio move failed; JSON sidecar still landed"
                );
                None
            } else {
                Some(target)
            }
        }
        _ => None,
    };

    tracing::info!(
        target: "vault::history",
        session_uuid = %input.session_uuid,
        session_id = input.session_id,
        json = %json_path.display(),
        audio = ?final_audio.as_ref().map(|p| p.display().to_string()),
        "history archive complete"
    );

    Ok(HistoryArchiveOutcome {
        json_path,
        audio_path: final_audio,
        archived: true,
    })
}

// ── Reconcile ─────────────────────────────────────────────────────

/// On-demand sweep that flags sealed-but-not-archived sessions and
/// orphan sidecars. Wave 1E.4 ships the SCAN; recovery (rewrite the
/// missing sidecar from the still-present DB rows) is deferred to
/// the IPC layer alongside `reconcile_vault` (see `mb-43xw`).
pub fn reconcile_history(
    conn: &Connection,
    vault_root: &Path,
) -> AppResult<HistoryReconcileReport> {
    let subtree = kg_subtree_paths(vault_root);
    let history_root = subtree.history;

    let mut report = HistoryReconcileReport::default();

    // (1) Sessions with sealed entry but no sidecar on disk.
    // We can't pin a session's bucket from DB state alone without
    // parsing `started_at` -- pull bucket-derivable timestamp +
    // uuid together so the scan stays one query.
    let mut stmt = conn.prepare(
        "SELECT uuid, started_at FROM sessions \
         WHERE entry_id IS NOT NULL AND vault_path IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        let uuid: String = r.get(0)?;
        let started_at: String = r.get(1)?;
        Ok((uuid, started_at))
    })?;
    let mut known_uuids: HashSet<String> = HashSet::new();
    for row in rows {
        let (uuid, started_at) = row?;
        known_uuids.insert(uuid.clone());
        let bucket = match month_bucket(&started_at) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    target: "vault::reconcile_history",
                    uuid = %uuid,
                    error = %e,
                    "skipping reconcile row -- unparseable started_at"
                );
                continue;
            }
        };
        let expected = history_root.join(&bucket).join(format!("{uuid}.json"));
        if !expected.exists() {
            report.missing_sidecar_count += 1;
            tracing::warn!(
                target: "vault::reconcile_history",
                uuid = %uuid,
                expected = %expected.display(),
                "session sealed but no history sidecar on disk"
            );
        }
    }

    // (2) Orphan sidecars: any `<uuid>.json` whose `<uuid>` is not in
    // the known set. Read-only; never deletes.
    if history_root.is_dir() {
        walk_history_sidecars(&history_root, &mut |path, uuid| {
            if !known_uuids.contains(uuid) {
                report.orphan_sidecar_count += 1;
                tracing::warn!(
                    target: "vault::reconcile_history",
                    path = %path.display(),
                    uuid = %uuid,
                    "orphan history sidecar (no session has this uuid)"
                );
            }
        });
    }

    tracing::info!(
        target: "vault::reconcile_history",
        missing = report.missing_sidecar_count,
        orphans = report.orphan_sidecar_count,
        "history reconcile pass complete"
    );

    Ok(report)
}

/// Walk every `<bucket>/<uuid>.json` under `history_root` and call
/// `cb` for each. Skips non-`.json` files + sub-directories that
/// aren't month-bucket-shaped. Errors during traversal are logged
/// and swallowed so a single bad file doesn't abort the whole sweep.
fn walk_history_sidecars(history_root: &Path, cb: &mut dyn FnMut(&Path, &str)) {
    let bucket_iter = match fs::read_dir(history_root) {
        Ok(it) => it,
        Err(e) => {
            tracing::warn!(
                target: "vault::reconcile_history",
                error = %e,
                "walk_history_sidecars: read_dir failed at history_root"
            );
            return;
        }
    };
    for bucket_entry in bucket_iter {
        let bucket_entry = match bucket_entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(target: "vault::reconcile_history", error = %e, "bucket iter");
                continue;
            }
        };
        let bucket_path = bucket_entry.path();
        if !bucket_path.is_dir() {
            continue;
        }
        let file_iter = match fs::read_dir(&bucket_path) {
            Ok(it) => it,
            Err(e) => {
                tracing::warn!(
                    target: "vault::reconcile_history",
                    bucket = %bucket_path.display(),
                    error = %e,
                    "bucket read_dir failed"
                );
                continue;
            }
        };
        for file_entry in file_iter {
            let file_entry = match file_entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = file_entry.path();
            if !is_sidecar_json(&path) {
                continue;
            }
            // `<uuid>.json` -> strip extension to recover uuid.
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            cb(&path, stem);
        }
    }
}

fn is_sidecar_json(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json")
}

// ── Private filesystem helpers ────────────────────────────────────

/// Atomic file write: temp-sibling + rename. Same shape as
/// `vault::writer::write_atomic` but kept private to this module so
/// the two writers can evolve independently if 1E.5 introduces
/// reverse-watcher-specific requirements on one of them.
fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".mb-tmp");
    let tmp_path = PathBuf::from(tmp);
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target)?;
    Ok(())
}

/// Cross-filesystem-safe file move: try `rename` first (atomic,
/// O(1)) and fall back to copy + remove on any failure. The
/// in-app recording dir + the user's vault are usually on different
/// volumes, so the rename almost always falls through to copy.
fn move_file(src: &Path, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_rename_err) => {
            // Copy then delete. Atomic-enough for our purposes: a
            // crash between copy-success + delete-success leaves the
            // archived audio in place + the source intact, which a
            // future archive call can detect (target exists -> no-op).
            fs::copy(src, dest)?;
            fs::remove_file(src)?;
            Ok(())
        }
    }
}

// ──────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
