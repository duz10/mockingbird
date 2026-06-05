//! Inbox courier processor (Wave 3.2) — ADR 0046 Iter 3 / mb-txmy.
//!
//! Downstream of [`super::watcher`]: consumes
//! [`super::watcher::StableInboxFile`] events emitted once a candidate
//! has cleared two consecutive size-stability reads, validates them,
//! decodes the audio, queues a [`HeadlessIngestRequest`] over the
//! existing ADR 0046 §3.2 channel, awaits the reply, then archives
//! the courier file on success or quarantines it on failure.
//!
//! ## Why the architecture mirrors `dictation_import_file`
//!
//! The desktop "+ Audio file" IPC handler is the sibling producer
//! into the same [`HeadlessIngestSender`]. It decodes, queues, and
//! awaits — exactly the same flow we run here, just driven from a
//! file-system event instead of a user picker. Reusing the channel
//! means the orchestrator's whisper-rs/Silero/Cleaner deps are
//! constructed exactly once (per ADR 0046 §3.2 rationale) and
//! shared across every entry path.
//!
//! ## Single-in-flight + serial processing
//!
//! The orchestrator transcribes one request at a time anyway — but
//! we still gate the courier with a [`Mutex`] so we don't even
//! enqueue request #2 until request #1 has come back. Two reasons:
//!
//! - Decode is CPU-heavy; running two `symphonia` passes in
//!   parallel would just thrash the cache for no throughput win.
//! - We move the source file post-success. If we enqueued two
//!   requests against the same path (e.g. a debouncer hiccup
//!   replayed an old `StableInboxFile`), the second move would
//!   race with the first.
//!
//! ## Archive layout
//!
//! Successful imports rename atomically to
//! `<vault>/inbox/_archive/<YYYY-MM-DD>/<original-filename>`.
//! The date sub-directory keeps the archive browsable AND avoids
//! filename collisions over time — `Memo.m4a` recorded on two
//! different days lands in two separate folders without clobbering.
//!
//! Failures rename to `<vault>/inbox/_failed/<original-filename>`
//! (no date subdir; failures are rare and the user will be
//! investigating each one manually). If a failed filename
//! collides, a `-<n>` suffix is appended before the extension —
//! we never overwrite a quarantined file.
//!
//! ## What this module does NOT do
//!
//! - No conflict-file quarantine (`(Conflict YYYY-MM-DD …)` pattern,
//!   ADR §6) — Iter 4 hardening matrix.
//! - No dedup ledger (sha256-keyed `vault_inbox_ledger` table,
//!   ADR §6) — Iter 4 hardening matrix.
//! - No placeholder-note write before ingest (`status: processing`
//!   frontmatter, ADR §7) — Iter 4 UX matrix.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crossbeam_channel::Receiver;

use super::watcher::StableInboxFile;
use crate::audio::decode::decode_to_pcm16_mono_16k;
use crate::dictation::ingest::IngestProvenance;
use crate::dictation::ingest_channel::{HeadlessIngestRequest, HeadlessIngestSender};
use crate::dictation::ingest_progress::{self, IngestProgressBus, IngestProgressEvent};
use crate::error::{AppError, AppResult};

// --------------------------------------------------------------------
// Tunables
// --------------------------------------------------------------------

/// Hard upper bound on a single courier file's size. The iOS
/// Shortcut spec (ADR 0046 §8) settles on Low (32 kbps AAC mono),
/// giving ~5 MB for ~20 min of audio; anything north of 50 MB is
/// either a misconfigured Shortcut, a manual drop of the wrong
/// file, or a malicious payload. Capping early keeps the decoder
/// from chewing memory + CPU on noise.
const MAX_SIZE_BYTES: u64 = 50 * 1024 * 1024;

/// Same allowlist as the watcher's [`super::watcher`] — duplicated
/// here as a defense-in-depth check. The watcher emits only
/// allowlisted extensions, but a future caller of the courier
/// (e.g. a manual "re-process this file" UI) wouldn't go through
/// the watcher.
const EXTENSION_ALLOWLIST: &[&str] = &["m4a", "wav", "mp3"];

// --------------------------------------------------------------------
// Errors
// --------------------------------------------------------------------

/// Reasons a courier file fails before / during ingest. Mapped to
/// a string in the `_failed/` sidecar (when Iter 4 adds it) and the
/// tracing log line emitted when the file moves to quarantine.
#[derive(Debug)]
pub enum CourierFailure {
    /// Extension not on the allowlist (watcher should have caught
    /// this, but defense-in-depth).
    UnsupportedExtension(String),
    /// File size 0 / missing on disk by the time the courier ran.
    Empty,
    /// File exceeds [`MAX_SIZE_BYTES`].
    TooLarge(u64),
    /// `symphonia` couldn't decode the bytes into PCM.
    DecodeFailed(String),
    /// Headless ingest itself returned an error (Whisper / Cleaner
    /// / DB write — the full chain).
    IngestFailed(String),
    /// The orchestrator channel was closed when we tried to enqueue
    /// (dictation runtime not started, or it crashed).
    OrchestratorUnavailable,
    /// The reply channel was dropped without a reply — orchestrator
    /// crash mid-request.
    OrchestratorDroppedReply,
}

impl std::fmt::Display for CourierFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedExtension(e) => write!(f, "unsupported extension: {e:?}"),
            Self::Empty => write!(f, "file is empty (0 bytes)"),
            Self::TooLarge(n) => write!(f, "file exceeds {MAX_SIZE_BYTES} bytes (got {n})"),
            Self::DecodeFailed(s) => write!(f, "decode failed: {s}"),
            Self::IngestFailed(s) => write!(f, "ingest failed: {s}"),
            Self::OrchestratorUnavailable => write!(f, "orchestrator unavailable"),
            Self::OrchestratorDroppedReply => write!(f, "orchestrator dropped reply channel"),
        }
    }
}

// --------------------------------------------------------------------
// Public surface
// --------------------------------------------------------------------

/// One-shot outcome of [`Courier::process_one`]. Exposed primarily
/// for tests + observability; production callers can ignore the
/// value because the courier already routes the file to the right
/// zone before returning.
#[derive(Debug)]
pub enum CourierOutcome {
    /// Ingest succeeded; file moved to `_archive/YYYY-MM-DD/`.
    Archived {
        /// `sessions.id` of the newly written row.
        session_id: i64,
        /// Destination path the courier file was renamed to.
        archive_to: PathBuf,
    },
    /// Ingest succeeded; source file deleted because
    /// `KeepAudioBlobs` is off.
    Deleted {
        /// `sessions.id` of the newly written row.
        session_id: i64,
    },
    /// Ingest failed for some reason; file moved to `_failed/`.
    Quarantined {
        /// Why the ingest failed (validation / decode / orchestrator).
        reason: CourierFailure,
        /// Destination path the courier file was renamed to.
        failed_to: PathBuf,
    },
}

/// Handle for stopping the courier worker thread.
pub struct CourierHandle {
    shutdown_tx: crossbeam_channel::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CourierHandle {
    /// Signal shutdown + join. Mirror of [`super::watcher::InboxWatcherHandle::stop`].
    pub fn stop(mut self) -> AppResult<()> {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| AppError::Other("inbox courier thread panicked".into()))?;
        }
        Ok(())
    }
}

impl Drop for CourierHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The courier processor itself.
///
/// One [`Courier`] services one watcher's worth of
/// [`StableInboxFile`] events. The [`Mutex`] guards the
/// single-in-flight invariant.
pub struct Courier {
    inbox_path: PathBuf,
    input_rx: Receiver<StableInboxFile>,
    headless_ingest_tx: HeadlessIngestSender,
    in_flight: Arc<Mutex<()>>,
    /// ADR 0046 Iter 4 — mirror of `SettingKey::KeepAudioBlobs`.
    /// When true (default), successful ingests move the courier to
    /// `_archive/<YYYY-MM-DD>/`. When false, the source audio is
    /// DELETED after a successful ingest. Changing the setting
    /// restarts the runtime, so this is effectively immutable
    /// per courier instance.
    keep_audio_blobs: bool,
    /// ADR 0046 Iter 4 / mb-q1xt — best-effort progress emitter so
    /// the desktop UI's import-progress overlay lights up for files
    /// arriving via mobile sync, not just for the IPC-driven
    /// `+ Audio file` path. Defaults to noop when constructed
    /// without a real bus (tests / pre-Tauri-setup phase).
    progress: Arc<dyn IngestProgressBus>,
}

impl Courier {
    /// Build a courier rooted at `<vault>/inbox/`. The caller owns
    /// the [`Receiver`] half from the watcher's emit channel and
    /// a clone of the orchestrator's [`HeadlessIngestSender`].
    pub fn new(
        inbox_path: PathBuf,
        input_rx: Receiver<StableInboxFile>,
        headless_ingest_tx: HeadlessIngestSender,
        keep_audio_blobs: bool,
        progress: Arc<dyn IngestProgressBus>,
    ) -> Self {
        Self {
            inbox_path,
            input_rx,
            headless_ingest_tx,
            in_flight: Arc::new(Mutex::new(())),
            keep_audio_blobs,
            progress,
        }
    }

    /// Spawn the worker thread. Returns a handle for shutdown.
    pub fn start(self) -> AppResult<CourierHandle> {
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let keep_audio_blobs = self.keep_audio_blobs;
        let progress = Arc::clone(&self.progress);
        let thread = std::thread::Builder::new()
            .name("mockingbird-inbox-courier".into())
            .spawn(move || {
                courier_loop(
                    &self.inbox_path,
                    &self.input_rx,
                    &self.headless_ingest_tx,
                    &self.in_flight,
                    keep_audio_blobs,
                    &*progress,
                    &shutdown_rx,
                );
                tracing::info!(target: "inbox::courier", "worker exiting");
            })
            .map_err(|e| AppError::Other(format!("inbox courier: spawn thread: {e}")))?;
        tracing::info!(
            target: "inbox::courier",
            keep_audio_blobs,
            "courier started"
        );
        Ok(CourierHandle {
            shutdown_tx,
            thread: Some(thread),
        })
    }
}

// --------------------------------------------------------------------
// Worker loop
// --------------------------------------------------------------------

fn courier_loop(
    inbox_path: &Path,
    input_rx: &Receiver<StableInboxFile>,
    headless_ingest_tx: &HeadlessIngestSender,
    in_flight: &Arc<Mutex<()>>,
    keep_audio_blobs: bool,
    progress: &dyn IngestProgressBus,
    shutdown_rx: &crossbeam_channel::Receiver<()>,
) {
    use crossbeam_channel::{select, RecvError};
    loop {
        select! {
            recv(shutdown_rx) -> _ => {
                tracing::debug!(target: "inbox::courier", "shutdown requested");
                break;
            }
            recv(input_rx) -> msg => {
                match msg {
                    Ok(file) => {
                        let _guard = match in_flight.lock() {
                            Ok(g) => g,
                            Err(p) => {
                                tracing::error!(
                                    target: "inbox::courier",
                                    "in_flight mutex poisoned; recovering"
                                );
                                p.into_inner()
                            }
                        };
                        let outcome = process_one(
                            inbox_path,
                            &file,
                            headless_ingest_tx,
                            &ProductionFileOps,
                            keep_audio_blobs,
                            progress,
                        );
                        log_outcome(&file, &outcome);
                    }
                    Err(RecvError) => {
                        tracing::warn!(target: "inbox::courier", "input channel disconnected");
                        break;
                    }
                }
            }
        }
    }
}

fn log_outcome(file: &StableInboxFile, outcome: &CourierOutcome) {
    match outcome {
        CourierOutcome::Archived {
            session_id,
            archive_to,
        } => {
            tracing::info!(
                target: "inbox::courier",
                src = %file.path.display(),
                session_id = session_id,
                dst = %archive_to.display(),
                "ingest ok; archived"
            );
        }
        CourierOutcome::Deleted { session_id } => {
            tracing::info!(
                target: "inbox::courier",
                src = %file.path.display(),
                session_id = session_id,
                "ingest ok; source deleted (KeepAudioBlobs=off)"
            );
        }
        CourierOutcome::Quarantined { reason, failed_to } => {
            tracing::warn!(
                target: "inbox::courier",
                src = %file.path.display(),
                reason = %reason,
                dst = %failed_to.display(),
                "ingest failed; quarantined"
            );
        }
    }
}

// --------------------------------------------------------------------
// Filesystem injection point (lets tests stub out real disk I/O)
// --------------------------------------------------------------------

/// Filesystem operations the courier performs after deciding the
/// outcome. Behind a trait so tests can substitute an in-memory
/// double rather than touching real disk — keeps the hot logic
/// (validate → decode → ingest → route) fully testable in
/// pure-Rust mode (LESSONS P2 throwaway-crate friendly).
pub(crate) trait FileOps {
    /// Atomic rename if `src` and `dst` are on the same volume.
    /// Falls back to copy + delete on cross-volume `rename`
    /// failure. `dst`'s parent directory is created if missing.
    fn move_file(&self, src: &Path, dst: &Path) -> AppResult<()>;

    /// Read metadata; returns `(size, exists)`.
    fn metadata_size(&self, path: &Path) -> AppResult<u64>;

    /// Decode an audio file into 16 kHz mono PCM. Production uses
    /// [`decode_to_pcm16_mono_16k`]; tests stub.
    fn decode(&self, path: &Path) -> AppResult<Vec<i16>>;

    /// Current UTC ISO-8601 timestamp. Threaded so tests get
    /// deterministic provenance.
    fn now_iso(&self) -> String;

    /// Delete a file. Used when `KeepAudioBlobs` is off and a
    /// successful ingest's source audio should be removed instead
    /// of archived.
    fn delete_file(&self, path: &Path) -> AppResult<()>;
}

pub(crate) struct ProductionFileOps;

impl FileOps for ProductionFileOps {
    fn move_file(&self, src: &Path, dst: &Path) -> AppResult<()> {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Vault(format!(
                    "courier: create parent {} for archive: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        match std::fs::rename(src, dst) {
            Ok(()) => Ok(()),
            Err(rename_err) => {
                // Cross-volume edge case — `rename` returns
                // ERROR_NOT_SAME_DEVICE on Windows. Fall back
                // to copy + delete. Vault + inbox almost always
                // share a volume but a user who mounts the vault
                // on a different drive shouldn't be silently
                // stuck.
                tracing::warn!(
                    target: "inbox::courier",
                    error = %rename_err,
                    "rename failed; falling back to copy+delete"
                );
                std::fs::copy(src, dst).map_err(|e| {
                    AppError::Vault(format!(
                        "courier: fallback copy {}→{} failed: {}",
                        src.display(),
                        dst.display(),
                        e
                    ))
                })?;
                std::fs::remove_file(src).map_err(|e| {
                    AppError::Vault(format!(
                        "courier: fallback remove {} failed: {}",
                        src.display(),
                        e
                    ))
                })?;
                Ok(())
            }
        }
    }

    fn metadata_size(&self, path: &Path) -> AppResult<u64> {
        std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| AppError::Vault(format!("courier: stat {}: {}", path.display(), e)))
    }

    fn decode(&self, path: &Path) -> AppResult<Vec<i16>> {
        decode_to_pcm16_mono_16k(path)
    }

    fn now_iso(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }

    fn delete_file(&self, path: &Path) -> AppResult<()> {
        std::fs::remove_file(path)
            .map_err(|e| AppError::Vault(format!("courier: remove {}: {}", path.display(), e)))
    }
}

// --------------------------------------------------------------------
// The actual work — `process_one`
// --------------------------------------------------------------------

/// Process exactly one [`StableInboxFile`]. Always returns a
/// [`CourierOutcome`] (the file is always either archived or
/// quarantined; we never leave it in-place because the watcher
/// would just re-detect it on the next stability tick).
pub(crate) fn process_one(
    inbox_path: &Path,
    file: &StableInboxFile,
    headless_ingest_tx: &HeadlessIngestSender,
    fs: &dyn FileOps,
    keep_audio_blobs: bool,
    progress: &dyn IngestProgressBus,
) -> CourierOutcome {
    // Resolve filename up front so it can label every progress
    // event (the failure paths below all need it too).
    let original_filename = file
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("inbox-courier")
        .to_string();

    // 1. Validate. Order matters: cheap checks first. Validation
    //    failures emit a single `failed` event -- no `decoding`
    //    happened, so we don't need to clear an in-flight overlay.
    if let Err(failure) = validate(&file.path, fs) {
        progress.emit(IngestProgressEvent::failed(
            ingest_progress::source::MOBILE_INBOX,
            &original_filename,
            failure.to_string(),
        ));
        return quarantine(inbox_path, file, failure, fs);
    }

    // 2. Decode. CPU-heavy; run synchronously on the courier
    //    thread (we're already off the IPC executor).
    progress.emit(IngestProgressEvent::staged(
        ingest_progress::stage::DECODING,
        ingest_progress::source::MOBILE_INBOX,
        &original_filename,
    ));
    let samples = match fs.decode(&file.path) {
        Ok(s) => s,
        Err(e) => {
            let err = e.to_string();
            progress.emit(IngestProgressEvent::failed(
                ingest_progress::source::MOBILE_INBOX,
                &original_filename,
                format!("decode failed: {err}"),
            ));
            return quarantine(inbox_path, file, CourierFailure::DecodeFailed(err), fs);
        }
    };
    tracing::info!(
        target: "inbox::courier",
        path = %file.path.display(),
        samples = samples.len(),
        approx_seconds = samples.len() as f64 / 16_000.0,
        "decoded"
    );

    // 3. Build provenance + bounded(1) reply channel. Emit
    //    `transcribing` BEFORE the send -- whisper + cleanup run
    //    opaquely on the orchestrator's thread, so this single
    //    label covers the entire crunch from the UI's POV.
    progress.emit(IngestProgressEvent::staged(
        ingest_progress::stage::TRANSCRIBING,
        ingest_progress::source::MOBILE_INBOX,
        &original_filename,
    ));
    let provenance = IngestProvenance::mobile_inbox(original_filename.clone(), fs.now_iso());

    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    if headless_ingest_tx
        .send(HeadlessIngestRequest {
            samples,
            provenance,
            reply_tx,
        })
        .is_err()
    {
        progress.emit(IngestProgressEvent::failed(
            ingest_progress::source::MOBILE_INBOX,
            &original_filename,
            "orchestrator unavailable".to_string(),
        ));
        return quarantine(
            inbox_path,
            file,
            CourierFailure::OrchestratorUnavailable,
            fs,
        );
    }

    // 4. Block waiting for the orchestrator. `recv()` is fine
    //    here — this is the courier thread, not the async
    //    executor.
    let session_id = match reply_rx.recv() {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => {
            let err = e.to_string();
            progress.emit(IngestProgressEvent::failed(
                ingest_progress::source::MOBILE_INBOX,
                &original_filename,
                format!("ingest failed: {err}"),
            ));
            return quarantine(inbox_path, file, CourierFailure::IngestFailed(err), fs);
        }
        Err(_) => {
            progress.emit(IngestProgressEvent::failed(
                ingest_progress::source::MOBILE_INBOX,
                &original_filename,
                "orchestrator dropped reply channel".to_string(),
            ));
            return quarantine(
                inbox_path,
                file,
                CourierFailure::OrchestratorDroppedReply,
                fs,
            );
        }
    };
    // 4b. Terminal `done` emit. Fires regardless of
    //     archive-vs-delete branch below (the session row is
    //     already committed at this point).
    progress.emit(IngestProgressEvent::done(
        ingest_progress::source::MOBILE_INBOX,
        &original_filename,
        session_id,
    ));

    // 5. Archive OR delete on success, depending on the user's
    //    `KeepAudioBlobs` toggle. Either way the sessions row is
    //    already committed — we never re-quarantine here, because
    //    that would leave a dangling DB row pointing at a
    //    quarantined file.
    if keep_audio_blobs {
        let archive_to = archive_destination(inbox_path, &original_filename, fs);
        if let Err(e) = fs.move_file(&file.path, &archive_to) {
            tracing::error!(
                target: "inbox::courier",
                session_id,
                src = %file.path.display(),
                dst = %archive_to.display(),
                error = %e,
                "INGESTED but archive move failed; manual cleanup needed"
            );
        }
        CourierOutcome::Archived {
            session_id,
            archive_to,
        }
    } else {
        if let Err(e) = fs.delete_file(&file.path) {
            tracing::error!(
                target: "inbox::courier",
                session_id,
                src = %file.path.display(),
                error = %e,
                "INGESTED but source delete failed (KeepAudioBlobs=off); manual cleanup needed"
            );
        }
        CourierOutcome::Deleted { session_id }
    }
}

fn validate(path: &Path, fs: &dyn FileOps) -> Result<(), CourierFailure> {
    // Extension allowlist (defense in depth — watcher already
    // checks).
    let ext_lower = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext_lower.as_deref() {
        Some(ext) if EXTENSION_ALLOWLIST.contains(&ext) => {}
        other => {
            return Err(CourierFailure::UnsupportedExtension(
                other.unwrap_or("<none>").to_string(),
            ));
        }
    }

    // Size bounds — re-read here rather than trusting the
    // watcher's `StableInboxFile.size`, because the courier
    // doesn't run instantly after the watcher emits and the file
    // could conceivably have been replaced in the interim.
    let size = fs
        .metadata_size(path)
        .map_err(|e| CourierFailure::DecodeFailed(format!("stat before decode: {e}")))?;
    if size == 0 {
        return Err(CourierFailure::Empty);
    }
    if size > MAX_SIZE_BYTES {
        return Err(CourierFailure::TooLarge(size));
    }
    Ok(())
}

fn quarantine(
    inbox_path: &Path,
    file: &StableInboxFile,
    failure: CourierFailure,
    fs: &dyn FileOps,
) -> CourierOutcome {
    let filename = file
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("inbox-courier")
        .to_string();
    let dst = unique_failed_path(inbox_path, &filename);
    if let Err(e) = fs.move_file(&file.path, &dst) {
        tracing::error!(
            target: "inbox::courier",
            src = %file.path.display(),
            dst = %dst.display(),
            error = %e,
            "QUARANTINE move failed; file remains in inbox/"
        );
    }
    CourierOutcome::Quarantined {
        reason: failure,
        failed_to: dst,
    }
}

/// Compute `<inbox>/_archive/<YYYY-MM-DD>/<original-filename>`.
fn archive_destination(inbox_path: &Path, filename: &str, fs: &dyn FileOps) -> PathBuf {
    // Pull the date prefix off `now_iso()` — it returns RFC 3339,
    // so the first 10 characters are the YYYY-MM-DD slice. Cheaper
    // and less fragile than re-formatting via `chrono::Local::now`.
    let date_subdir = fs.now_iso().chars().take(10).collect::<String>();
    inbox_path.join("_archive").join(date_subdir).join(filename)
}

/// Compute `<inbox>/_failed/<filename>`, appending `-1`, `-2`, ... to
/// the stem if a collision exists so we never overwrite an
/// earlier quarantined file.
fn unique_failed_path(inbox_path: &Path, filename: &str) -> PathBuf {
    let failed_dir = inbox_path.join("_failed");
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
    // Astronomically unreachable, but the type system demands a
    // return — fall back to the original (overwriting is still
    // better than panic).
    initial
}

fn split_stem_ext(filename: &str) -> (&str, Option<&str>) {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (filename, None),
    }
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! All tests use the [`FileOps`] trait to stub real disk I/O,
    //! plus a hand-rolled orchestrator stand-in that consumes the
    //! channel and replies synchronously. The full courier flow is
    //! exercised end-to-end without ever touching a real audio file
    //! or the real orchestrator (no whisper-rs, no CUDA, no ort).
    //!
    //! Live-fire via the throwaway-crate recipe (LESSONS P2) —
    //! these mirror straight across.

    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::SystemTime;

    /// In-memory file-ops double. Tracks every `move_file` / `delete_file`
    /// call so tests can assert routing.
    struct FakeFs {
        size_of: StdMutex<std::collections::HashMap<PathBuf, u64>>,
        decode_result: StdMutex<AppResult<Vec<i16>>>,
        moves: StdMutex<Vec<(PathBuf, PathBuf)>>,
        deletes: StdMutex<Vec<PathBuf>>,
        now_iso: String,
    }

    impl FakeFs {
        fn new(size: u64, decode_ok: bool, now_iso: &str) -> Self {
            let mut sizes = std::collections::HashMap::new();
            // Default size — tests can override per-path before
            // calling process_one.
            sizes.insert(PathBuf::from("/inbox/foo.m4a"), size);
            Self {
                size_of: StdMutex::new(sizes),
                decode_result: StdMutex::new(if decode_ok {
                    Ok(vec![0i16; 16_000])
                } else {
                    Err(AppError::Audio("synthetic decode failure".into()))
                }),
                moves: StdMutex::new(Vec::new()),
                deletes: StdMutex::new(Vec::new()),
                now_iso: now_iso.to_string(),
            }
        }
    }

    impl FileOps for FakeFs {
        fn move_file(&self, src: &Path, dst: &Path) -> AppResult<()> {
            self.moves
                .lock()
                .unwrap()
                .push((src.to_path_buf(), dst.to_path_buf()));
            Ok(())
        }
        fn metadata_size(&self, path: &Path) -> AppResult<u64> {
            self.size_of
                .lock()
                .unwrap()
                .get(path)
                .copied()
                .ok_or_else(|| AppError::Vault(format!("stat missing {}", path.display())))
        }
        fn decode(&self, _path: &Path) -> AppResult<Vec<i16>> {
            // Replace the inner result so each call gets it once.
            // (Tests only call decode once per process_one anyway.)
            std::mem::replace(
                &mut *self.decode_result.lock().unwrap(),
                Err(AppError::Audio("test fixture exhausted".into())),
            )
        }
        fn now_iso(&self) -> String {
            self.now_iso.clone()
        }
        fn delete_file(&self, path: &Path) -> AppResult<()> {
            self.deletes.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }
    }

    fn synthetic_stable(path: &str, size: u64) -> StableInboxFile {
        StableInboxFile {
            path: PathBuf::from(path),
            size,
            observed_at: SystemTime::now(),
        }
    }

    /// Spawn a thread that consumes one HeadlessIngestRequest and
    /// replies with `reply`. Returns the sender (which the courier
    /// uses to enqueue) — caller drops it to drop the channel.
    fn stub_orchestrator(reply: AppResult<i64>) -> HeadlessIngestSender {
        let (tx, rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
        std::thread::spawn(move || {
            if let Ok(req) = rx.recv() {
                let _ = req.reply_tx.send(reply);
            }
        });
        tx
    }

    #[test]
    fn success_routes_to_archive_with_date_subdir() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Memo.m4a", 12_345);
        let fs = FakeFs::new(12_345, true, "2026-05-27T18:00:00Z");
        // Pre-seed the size map for the actual path.
        fs.size_of.lock().unwrap().insert(file.path.clone(), 12_345);
        let tx = stub_orchestrator(Ok(42));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );

        match outcome {
            CourierOutcome::Archived {
                session_id,
                archive_to,
            } => {
                assert_eq!(session_id, 42);
                assert_eq!(
                    archive_to,
                    PathBuf::from("/vault/inbox/_archive/2026-05-27/Memo.m4a")
                );
            }
            other => panic!("expected Archived, got {other:?}"),
        }
        let moves = fs.moves.lock().unwrap();
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].0, file.path);
        assert_eq!(
            moves[0].1,
            PathBuf::from("/vault/inbox/_archive/2026-05-27/Memo.m4a")
        );
    }

    /// ADR 0046 Iter 4 — with `KeepAudioBlobs=false`, the source
    /// audio is deleted instead of archived. The sessions row is
    /// still committed (the courier outcome carries the id).
    #[test]
    fn keep_audio_blobs_off_deletes_source_after_success() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Memo.m4a", 12_345);
        let fs = FakeFs::new(12_345, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 12_345);
        let tx = stub_orchestrator(Ok(7));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            /*keep_audio_blobs=*/ false,
            &ingest_progress::NoopIngestProgressBus,
        );

        match outcome {
            CourierOutcome::Deleted { session_id } => assert_eq!(session_id, 7),
            other => panic!("expected Deleted, got {other:?}"),
        }
        // Source path was deleted, never moved.
        let deletes = fs.deletes.lock().unwrap();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0], file.path);
        let moves = fs.moves.lock().unwrap();
        assert!(
            moves.is_empty(),
            "expected no archive move with KeepAudioBlobs=off, got {moves:?}"
        );
    }

    #[test]
    fn ingest_failure_routes_to_failed() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Memo.m4a", 12_345);
        let fs = FakeFs::new(12_345, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 12_345);
        let tx = stub_orchestrator(Err(AppError::Stt("synthetic whisper crash".into())));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );

        match outcome {
            CourierOutcome::Quarantined { reason, failed_to } => {
                assert!(matches!(reason, CourierFailure::IngestFailed(_)));
                assert_eq!(failed_to, PathBuf::from("/vault/inbox/_failed/Memo.m4a"));
            }
            other => panic!("expected Quarantined, got {other:?}"),
        }
    }

    #[test]
    fn zero_byte_file_quarantines_without_decoding() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Empty.m4a", 0);
        let fs = FakeFs::new(0, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 0);
        let tx = stub_orchestrator(Ok(0));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );
        match outcome {
            CourierOutcome::Quarantined { reason, failed_to } => {
                assert!(matches!(reason, CourierFailure::Empty));
                assert_eq!(failed_to, PathBuf::from("/vault/inbox/_failed/Empty.m4a"));
            }
            other => panic!("expected Quarantined, got {other:?}"),
        }
    }

    #[test]
    fn oversized_file_quarantines_without_decoding() {
        let inbox = PathBuf::from("/vault/inbox");
        let big = MAX_SIZE_BYTES + 1;
        let file = synthetic_stable("/vault/inbox/Huge.m4a", big);
        let fs = FakeFs::new(big, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), big);
        let tx = stub_orchestrator(Ok(0));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );
        assert!(matches!(
            outcome,
            CourierOutcome::Quarantined {
                reason: CourierFailure::TooLarge(_),
                ..
            }
        ));
    }

    #[test]
    fn wrong_extension_quarantines() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/note.txt", 100);
        let fs = FakeFs::new(100, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 100);
        let tx = stub_orchestrator(Ok(0));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );
        assert!(matches!(
            outcome,
            CourierOutcome::Quarantined {
                reason: CourierFailure::UnsupportedExtension(_),
                ..
            }
        ));
    }

    #[test]
    fn decode_failure_quarantines() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Garbage.m4a", 100);
        let fs = FakeFs::new(100, false, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 100);
        let tx = stub_orchestrator(Ok(0));

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );
        assert!(matches!(
            outcome,
            CourierOutcome::Quarantined {
                reason: CourierFailure::DecodeFailed(_),
                ..
            }
        ));
    }

    #[test]
    fn orchestrator_unavailable_quarantines() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Memo.m4a", 100);
        let fs = FakeFs::new(100, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 100);
        // Closed channel — drop the receiver immediately so send
        // returns Err.
        let (tx, rx) = crossbeam_channel::unbounded::<HeadlessIngestRequest>();
        drop(rx);

        let outcome = process_one(
            &inbox,
            &file,
            &tx,
            &fs,
            true,
            &ingest_progress::NoopIngestProgressBus,
        );
        assert!(matches!(
            outcome,
            CourierOutcome::Quarantined {
                reason: CourierFailure::OrchestratorUnavailable,
                ..
            }
        ));
    }

    /// Recorded progress-bus emit: (stage, source, session_id, error).
    /// Tuple form keeps the test-local capturing bus throwaway-crate
    /// friendly (LESSONS P2) without needing the real event type.
    type RecordedEmit = (&'static str, &'static str, Option<i64>, Option<String>);

    /// In-test progress bus that records every emit in order.
    /// Mirrors the capturing bus in `dictation/ingest_progress.rs`'s
    /// own tests; kept local here so this file stays
    /// throwaway-crate friendly (LESSONS P2).
    #[derive(Default)]
    struct RecordingBus {
        events: StdMutex<Vec<RecordedEmit>>,
    }

    impl IngestProgressBus for RecordingBus {
        fn emit(&self, event: IngestProgressEvent) {
            self.events.lock().unwrap().push((
                event.stage,
                event.source,
                event.session_id,
                event.error,
            ));
        }
    }

    #[test]
    fn success_emits_decoding_then_transcribing_then_done_on_inbox_source() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Memo.m4a", 12_345);
        let fs = FakeFs::new(12_345, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 12_345);
        let tx = stub_orchestrator(Ok(7));
        let bus = RecordingBus::default();

        let _ = process_one(&inbox, &file, &tx, &fs, true, &bus);

        let events = bus.events.lock().unwrap();
        let stages: Vec<&str> = events.iter().map(|(s, _, _, _)| *s).collect();
        assert_eq!(
            stages,
            vec!["decoding", "transcribing", "done"],
            "got: {events:?}"
        );
        // Every event must carry the mobile-inbox source label.
        for (_, src, _, _) in events.iter() {
            assert_eq!(*src, "mobile-inbox");
        }
        // Only the terminal `done` emit carries session_id.
        assert_eq!(events[0].2, None);
        assert_eq!(events[1].2, None);
        assert_eq!(events[2].2, Some(7));
    }

    #[test]
    fn decode_failure_emits_decoding_then_failed_with_error() {
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Garbage.m4a", 100);
        let fs = FakeFs::new(100, false, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 100);
        let tx = stub_orchestrator(Ok(0));
        let bus = RecordingBus::default();

        let _ = process_one(&inbox, &file, &tx, &fs, true, &bus);

        let events = bus.events.lock().unwrap();
        let stages: Vec<&str> = events.iter().map(|(s, _, _, _)| *s).collect();
        assert_eq!(stages, vec!["decoding", "failed"], "got: {events:?}");
        // The `failed` emit MUST carry the error string so the
        // overlay can quote it. Don't pin the exact message --
        // just check it's non-empty.
        let err = events[1].3.as_ref().expect("failed must carry error");
        assert!(!err.is_empty());
    }

    #[test]
    fn validation_failure_emits_only_failed_no_decoding() {
        // Zero-byte file fails validation BEFORE the decode bracket --
        // the user never saw a "decoding" toast, so we shouldn't
        // emit one just to clear it.
        let inbox = PathBuf::from("/vault/inbox");
        let file = synthetic_stable("/vault/inbox/Empty.m4a", 0);
        let fs = FakeFs::new(0, true, "2026-05-27T18:00:00Z");
        fs.size_of.lock().unwrap().insert(file.path.clone(), 0);
        let tx = stub_orchestrator(Ok(0));
        let bus = RecordingBus::default();

        let _ = process_one(&inbox, &file, &tx, &fs, true, &bus);

        let events = bus.events.lock().unwrap();
        let stages: Vec<&str> = events.iter().map(|(s, _, _, _)| *s).collect();
        assert_eq!(stages, vec!["failed"], "got: {events:?}");
    }

    #[test]
    fn unique_failed_path_appends_suffix_on_collision() {
        // Without an existing file the initial path is used as-is.
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path();
        let failed_dir = inbox.join("_failed");
        std::fs::create_dir_all(&failed_dir).unwrap();

        let first = unique_failed_path(inbox, "foo.m4a");
        assert_eq!(first, failed_dir.join("foo.m4a"));

        // Seed a collision; expect -1 suffix.
        std::fs::write(&first, b"x").unwrap();
        let second = unique_failed_path(inbox, "foo.m4a");
        assert_eq!(second, failed_dir.join("foo-1.m4a"));

        // Seed another collision; expect -2 suffix.
        std::fs::write(&second, b"x").unwrap();
        let third = unique_failed_path(inbox, "foo.m4a");
        assert_eq!(third, failed_dir.join("foo-2.m4a"));
    }

    #[test]
    fn split_stem_ext_handles_dotfiles_and_no_extension() {
        assert_eq!(split_stem_ext("foo.m4a"), ("foo", Some("m4a")));
        assert_eq!(split_stem_ext("foo"), ("foo", None));
        // Leading-dot files (`.gitignore`) are treated as no-ext —
        // splitting `.gitignore` would give ("", "gitignore"),
        // which we reject.
        assert_eq!(split_stem_ext(".gitignore"), (".gitignore", None));
    }
}
