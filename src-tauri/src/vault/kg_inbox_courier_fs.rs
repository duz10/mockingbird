//! Filesystem injection point + validation / quarantine helpers
//! for [`crate::vault::kg_inbox_courier`].
//!
//! Split out of `kg_inbox_courier.rs` to keep the parent under the
//! 600-line cap. Everything here is `pub(crate)`-scoped and only
//! re-exported via the parent module's call sites -- nothing in
//! this file is part of the public IPC surface.
//!
//! ## What's here
//!
//! - The [`KgFileOps`] trait + its production implementation
//!   [`ProductionKgFileOps`] (decode + stat + move + clock). Tests
//!   stub the trait directly to skip real disk I/O + symphonia
//!   (LESSONS P2 throwaway-crate friendly).
//! - [`validate`] -- extension allowlist + size bounds (mirrors
//!   [`crate::inbox::courier`]'s identical-named helper; we
//!   duplicate rather than re-export because the inbox courier's
//!   helper is private and reaching across a sibling crate-level
//!   `pub(crate)` boundary for a 20-LoC pure function is more
//!   coupling than it's worth).
//! - [`quarantine`] -- failure routing into `<KG Inbox>/_failed/`.
//! - [`unique_failed_path`] + [`split_stem_ext`] -- collision-safe
//!   destination computation.
//! - [`already_ingested`] -- the idempotency probe against
//!   `sessions.audio_blob_path` (crash-recovery guard described in
//!   the parent module's flow doc).
//!
//! ## What's NOT here
//!
//! - `process_one` and `courier_loop` -- they live in
//!   `kg_inbox_courier.rs` because they ARE the courier's core
//!   policy (the trait + helpers are the mechanism).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::kg_inbox_courier::{
    KgCourierFailure, KgCourierOutcome, EXTENSION_ALLOWLIST, FAILED_DIR_NAME, MAX_SIZE_BYTES,
};
use crate::audio::decode::decode_to_pcm16_mono_16k;
use crate::db::sessions;
use crate::error::{AppError, AppResult};
use crate::inbox::watcher::StableInboxFile;

// --------------------------------------------------------------------
// Trait + production implementation
// --------------------------------------------------------------------

/// Filesystem operations the KG-Inbox courier performs.
///
/// Narrower than [`crate::inbox::courier::FileOps`]: we don't need
/// `delete_file` (no `KeepAudioBlobs` toggle) and we don't archive
/// via `move_file` to a date subdir (the worker handles archive).
/// What remains is the failure-quarantine move + size + decode +
/// clock injection.
pub(crate) trait KgFileOps {
    /// Atomic rename with cross-volume copy+delete fallback. `dst`'s
    /// parent is created if missing.
    fn move_file(&self, src: &Path, dst: &Path) -> AppResult<()>;

    /// File size at probe time (re-read; the watcher's
    /// `StableInboxFile.size` could be stale).
    fn metadata_size(&self, path: &Path) -> AppResult<u64>;

    /// Decode an audio file into 16 kHz mono PCM.
    fn decode(&self, path: &Path) -> AppResult<Vec<i16>>;

    /// Current UTC ISO-8601 timestamp.
    fn now_iso(&self) -> String;
}

pub(crate) struct ProductionKgFileOps;

impl KgFileOps for ProductionKgFileOps {
    fn move_file(&self, src: &Path, dst: &Path) -> AppResult<()> {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Vault(format!(
                    "kg-inbox courier: create parent {} for quarantine: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        match std::fs::rename(src, dst) {
            Ok(()) => Ok(()),
            Err(rename_err) => {
                tracing::warn!(
                    target: "kg_inbox::courier",
                    error = %rename_err,
                    "rename failed; falling back to copy+delete"
                );
                std::fs::copy(src, dst).map_err(|e| {
                    AppError::Vault(format!(
                        "kg-inbox courier: fallback copy {}->{} failed: {}",
                        src.display(),
                        dst.display(),
                        e
                    ))
                })?;
                std::fs::remove_file(src).map_err(|e| {
                    AppError::Vault(format!(
                        "kg-inbox courier: fallback remove {} failed: {}",
                        src.display(),
                        e
                    ))
                })?;
                Ok(())
            }
        }
    }

    fn metadata_size(&self, path: &Path) -> AppResult<u64> {
        std::fs::metadata(path).map(|m| m.len()).map_err(|e| {
            AppError::Vault(format!("kg-inbox courier: stat {}: {}", path.display(), e))
        })
    }

    fn decode(&self, path: &Path) -> AppResult<Vec<i16>> {
        decode_to_pcm16_mono_16k(path)
    }

    fn now_iso(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }
}

// --------------------------------------------------------------------
// Helpers used by `process_one`
// --------------------------------------------------------------------

/// Phase 1E Wave 1E.6 (`mb-i46v`) idempotency probe. Returns the
/// existing `sessions.id` when a row already references `path` via
/// `audio_blob_path` -- meaning a previous run already wrote the
/// session but hasn't (yet) seen its phase-4 archive complete. The
/// caller then short-circuits ingest and lets the worker eventually
/// rename the file out of `Knowledge Graph/Inbox/`.
pub(crate) fn already_ingested(db: &Arc<Mutex<Connection>>, path: &Path) -> AppResult<Option<i64>> {
    let key = path.to_string_lossy().into_owned();
    let conn = db
        .lock()
        .map_err(|_| AppError::Other("kg-inbox courier: db mutex poisoned".into()))?;
    Ok(sessions::find_by_audio_blob_path(&conn, &key)?.map(|s| s.id))
}

pub(crate) fn validate(path: &Path, fs: &dyn KgFileOps) -> Result<(), KgCourierFailure> {
    let ext_lower = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext_lower.as_deref() {
        Some(ext) if EXTENSION_ALLOWLIST.contains(&ext) => {}
        other => {
            return Err(KgCourierFailure::UnsupportedExtension(
                other.unwrap_or("<none>").to_string(),
            ));
        }
    }

    let size = fs
        .metadata_size(path)
        .map_err(|e| KgCourierFailure::DecodeFailed(format!("stat before decode: {e}")))?;
    if size == 0 {
        return Err(KgCourierFailure::Empty);
    }
    if size > MAX_SIZE_BYTES {
        return Err(KgCourierFailure::TooLarge(size));
    }
    Ok(())
}

pub(crate) fn quarantine(
    kg_inbox_path: &Path,
    file: &StableInboxFile,
    failure: KgCourierFailure,
    fs: &dyn KgFileOps,
) -> KgCourierOutcome {
    let filename = file
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("kg-inbox-courier")
        .to_string();
    let dst = unique_failed_path(kg_inbox_path, &filename);
    if let Err(e) = fs.move_file(&file.path, &dst) {
        tracing::error!(
            target: "kg_inbox::courier",
            src = %file.path.display(),
            dst = %dst.display(),
            error = %e,
            "QUARANTINE move failed; file remains in KG Inbox/"
        );
    }
    KgCourierOutcome::Quarantined {
        reason: failure,
        failed_to: dst,
    }
}

/// Compute `<KG Inbox>/_failed/<filename>`, appending `-<n>` to the
/// stem on collision so we never overwrite a previously-quarantined
/// file. Mirror of the inbox courier's helper of the same name --
/// duplicated rather than `pub`-exported because reaching across
/// the sibling module's `pub(crate)` boundary for a 20-LoC pure
/// function is more coupling than it's worth.
pub(crate) fn unique_failed_path(kg_inbox_path: &Path, filename: &str) -> PathBuf {
    let failed_dir = kg_inbox_path.join(FAILED_DIR_NAME);
    let initial = failed_dir.join(filename);
    if !initial.exists() {
        return initial;
    }
    let (stem, ext) = split_stem_ext(filename);
    for n in 1..=u32::MAX {
        let candidate = match ext {
            Some(e) => failed_dir.join(format!("{stem}-{n}.{e}")),
            None => failed_dir.join(format!("{stem}-{n}")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    // Unreachable in practice; defensive fallback rather than panic.
    initial
}

pub(crate) fn split_stem_ext(filename: &str) -> (&str, Option<&str>) {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (filename, None),
    }
}
