//! KG-Inbox courier (Phase 1E Wave 1E.6 / `mb-i46v`, ADR 0053
//! Section "KG-Inbox courier").
//!
//! Sibling of [`crate::inbox::courier`]. Watches
//! `<vault>/Knowledge Graph/Inbox/` for audio files dropped via
//! the iOS Shortcut (mobile sync) OR by a desktop drag-and-drop,
//! and pipes them through the existing
//! [`crate::dictation::ingest::headless_ingest`] with the new
//! [`crate::dictation::ingest::IngestProvenance::mobile_inbox_kg_note`]
//! provenance. The source-gate (ADR 0052 Section D1) -- now also
//! wired through the headless ingest tail (Wave 1E.6) -- routes
//! the resulting session into `kg_filing_queue`, and the KG worker
//! takes it from there (file -> Markdown -> INDEX / LOG / Tags ->
//! `Knowledge Graph/History/<YYYY-MM>/`).
//!
//! ## Why a sibling module, not a re-use of the existing courier
//!
//! The ADR 0046 inbox courier and this KG-Inbox courier share their
//! file-detection scaffolding (the [`crate::inbox::watcher`] stability
//! state machine, the [`crate::inbox::watcher::StableInboxFile`] event
//! shape, and the validation primitives) but diverge in three ways
//! that make a single fused module less cohesive, not more:
//!
//! - **Provenance.** ADR 0046 uses
//!   [`crate::dictation::ingest::IngestProvenance::mobile_inbox`]
//!   (`capture_kind = Dictation`); we use
//!   [`crate::dictation::ingest::IngestProvenance::mobile_inbox_kg_note`]
//!   (`capture_kind = KgNote`, audio path threaded through).
//! - **Post-success disposition.** ADR 0046 moves the source file
//!   to `<vault>/inbox/_archive/<YYYY-MM-DD>/<filename>` directly
//!   from the courier. We do NOT move -- the KG worker's phase-4
//!   archive ([`crate::vault::history::archive_session_history`])
//!   renames the file into `Knowledge Graph/History/<YYYY-MM>/<uuid>.<ext>`
//!   so the canonical filename is the session UUID, not the
//!   user-or-Shortcut-provided original. The worker drives this
//!   off `sessions.audio_blob_path`, which we set on insert via
//!   the new `mobile_inbox_kg_note` provenance.
//! - **Failure disposition.** ADR 0046 quarantines to
//!   `<vault>/inbox/_failed/<filename>`. We quarantine to
//!   `<vault>/Knowledge Graph/Inbox/_failed/<filename>` so the user
//!   sees the failure where they dropped the file -- Obsidian's
//!   file explorer surfaces both folders, and a failed KG note in
//!   the ADR 0046 `_failed/` zone would be hopelessly misleading.
//!
//! Per AGENTS.md "smallest container that fits the work": a focused
//! ~400-line sibling module is more cohesive than retrofitting a
//! disposition-strategy abstraction across both code paths for a
//! one-shot reuse. YAGNI wins.
//!
//! ## Idempotency contract
//!
//! Files survive the round-trip through `headless_ingest` and are
//! moved only when the worker's phase-4 archive runs. Between
//! these two points the file sits in `Knowledge Graph/Inbox/` --
//! which means a crash mid-flight, app restart, or initial-scan
//! re-emit could otherwise produce a duplicate session row.
//! Guarded by [`already_ingested`]: before calling
//! `headless_ingest`, we query `sessions` for any row with
//! `audio_blob_path = <this file's path>`. A hit means a previous
//! attempt already wrote the session; we skip the ingest and let
//! the worker phase-4 archive on its next tick.
//!
//! For "same content, different filename, dropped twice" the
//! courier currently produces two session rows (no SHA-256 ledger).
//! That gap mirrors the existing ADR 0046 inbox; ADR 0046 Iter 4
//! `mb-qxrm` covers the shared `vault_inbox_ledger` table that
//! would close it. Surfaced in LESSONS as a known limitation.
//!
//! ## What this module does NOT do
//!
//! - No SHA-256 dedup (Iter 4 hardening; shared with ADR 0046).
//! - No conflict-file quarantine (`(Conflict YYYY-MM-DD ...)`).
//! - No placeholder-note write (the KG worker writes the entry
//!   markdown directly).
//! - No archive move (the worker does it via phase-4).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crossbeam_channel::Receiver;
use rusqlite::Connection;

use super::kg_inbox_courier_fs::{
    already_ingested, quarantine, validate, KgFileOps, ProductionKgFileOps,
};
use crate::dictation::ingest::IngestProvenance;
use crate::dictation::ingest_channel::{HeadlessIngestRequest, HeadlessIngestSender};
use crate::dictation::ingest_progress::{self, IngestProgressBus, IngestProgressEvent};
use crate::error::{AppError, AppResult};
use crate::inbox::watcher::StableInboxFile;

// --------------------------------------------------------------------
// Tunables (mirror of `inbox::courier` -- we want identical caps so a
// file rejected by one path would be rejected by the other; surprises
// would only confuse users).
// --------------------------------------------------------------------

/// Hard upper bound on a single courier file's size. Same 50 MB cap
/// as [`crate::inbox::courier`] -- a longer Voice Memo than that is
/// almost certainly a misconfigured Shortcut or a wrong drop.
/// `pub(super)` so the sibling `kg_inbox_courier_fs` module shares
/// the cap without re-declaring it.
pub(super) const MAX_SIZE_BYTES: u64 = 50 * 1024 * 1024;

/// Extension allowlist. Same as [`crate::inbox::courier`] -- m4a is
/// the iOS Shortcut default, wav/mp3 are common drag-and-drop
/// shapes.
pub(super) const EXTENSION_ALLOWLIST: &[&str] = &["m4a", "wav", "mp3"];

/// Subdirectory under `Knowledge Graph/Inbox/` where files that
/// fail validation / decode / ingest land for manual triage.
/// Filtered out by the watcher's
/// [`crate::inbox::watcher::should_consider_path`] (the `_failed`
/// path-segment exclusion applies regardless of which inbox the
/// watcher is rooted at), so quarantined files don't loop.
pub(super) const FAILED_DIR_NAME: &str = "_failed";

// --------------------------------------------------------------------
// Errors
// --------------------------------------------------------------------

/// Reasons a KG-Inbox file fails before / during ingest.
#[derive(Debug)]
pub enum KgCourierFailure {
    /// Extension not on the allowlist.
    UnsupportedExtension(String),
    /// File is empty (0 bytes) or missing at probe time.
    Empty,
    /// File exceeds [`MAX_SIZE_BYTES`].
    TooLarge(u64),
    /// `symphonia` couldn't decode the bytes into PCM.
    DecodeFailed(String),
    /// Headless ingest returned an error.
    IngestFailed(String),
    /// The orchestrator channel was closed.
    OrchestratorUnavailable,
    /// The reply channel was dropped without a reply.
    OrchestratorDroppedReply,
}

impl std::fmt::Display for KgCourierFailure {
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

/// One-shot outcome of [`process_one`]. Distinguished from the
/// ADR 0046 [`crate::inbox::courier::CourierOutcome`] because the
/// success path here does NOT move the file -- the worker phase-4
/// archive handles that asynchronously.
#[derive(Debug)]
pub enum KgCourierOutcome {
    /// Ingest succeeded (or was idempotently skipped because a
    /// session already references this path). The file stays in
    /// `Knowledge Graph/Inbox/` until the worker phase-4 archive
    /// runs.
    Ingested {
        /// `sessions.id` of the newly-written (or already-existing,
        /// for the idempotent-skip path) row.
        session_id: i64,
        /// `true` when this call short-circuited because a session
        /// row already pointed at the source path (crash-recovery
        /// re-emit). Useful for tests; production code only logs.
        idempotent_skip: bool,
    },
    /// Ingest failed; file moved to `_failed/`.
    Quarantined {
        /// Why the ingest failed.
        reason: KgCourierFailure,
        /// Destination path the file was renamed to.
        failed_to: PathBuf,
    },
}

/// Handle for stopping the KG-Inbox courier worker thread.
pub struct KgInboxCourierHandle {
    shutdown_tx: crossbeam_channel::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl KgInboxCourierHandle {
    /// Signal shutdown + join. Mirror of
    /// [`crate::inbox::courier::CourierHandle::stop`].
    pub fn stop(mut self) -> AppResult<()> {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| AppError::Other("kg-inbox courier thread panicked".into()))?;
        }
        Ok(())
    }
}

impl Drop for KgInboxCourierHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The courier processor itself.
pub struct KgInboxCourier {
    kg_inbox_path: PathBuf,
    input_rx: Receiver<StableInboxFile>,
    headless_ingest_tx: HeadlessIngestSender,
    in_flight: Arc<Mutex<()>>,
    /// Shared DB handle for the idempotency lookup
    /// ([`sessions::find_by_audio_blob_path`]).
    db: Arc<Mutex<Connection>>,
    progress: Arc<dyn IngestProgressBus>,
}

impl KgInboxCourier {
    /// Build a courier rooted at `<vault>/Knowledge Graph/Inbox/`.
    pub fn new(
        kg_inbox_path: PathBuf,
        input_rx: Receiver<StableInboxFile>,
        headless_ingest_tx: HeadlessIngestSender,
        db: Arc<Mutex<Connection>>,
        progress: Arc<dyn IngestProgressBus>,
    ) -> Self {
        Self {
            kg_inbox_path,
            input_rx,
            headless_ingest_tx,
            in_flight: Arc::new(Mutex::new(())),
            db,
            progress,
        }
    }

    /// Spawn the worker thread.
    pub fn start(self) -> AppResult<KgInboxCourierHandle> {
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let progress = Arc::clone(&self.progress);
        let thread = std::thread::Builder::new()
            .name("mockingbird-kg-inbox-courier".into())
            .spawn(move || {
                courier_loop(
                    &self.kg_inbox_path,
                    &self.input_rx,
                    &self.headless_ingest_tx,
                    &self.in_flight,
                    &self.db,
                    &*progress,
                    &shutdown_rx,
                );
                tracing::info!(target: "kg_inbox::courier", "worker exiting");
            })
            .map_err(|e| AppError::Other(format!("kg-inbox courier: spawn thread: {e}")))?;
        tracing::info!(
            target: "kg_inbox::courier",
            "courier started"
        );
        Ok(KgInboxCourierHandle {
            shutdown_tx,
            thread: Some(thread),
        })
    }
}

// --------------------------------------------------------------------
// Worker loop
// --------------------------------------------------------------------

#[allow(clippy::too_many_arguments)] // each arg is independent + named
fn courier_loop(
    kg_inbox_path: &Path,
    input_rx: &Receiver<StableInboxFile>,
    headless_ingest_tx: &HeadlessIngestSender,
    in_flight: &Arc<Mutex<()>>,
    db: &Arc<Mutex<Connection>>,
    progress: &dyn IngestProgressBus,
    shutdown_rx: &crossbeam_channel::Receiver<()>,
) {
    use crossbeam_channel::{select, RecvError};
    loop {
        select! {
            recv(shutdown_rx) -> _ => {
                tracing::debug!(target: "kg_inbox::courier", "shutdown requested");
                break;
            }
            recv(input_rx) -> msg => {
                match msg {
                    Ok(file) => {
                        let _guard = match in_flight.lock() {
                            Ok(g) => g,
                            Err(p) => {
                                tracing::error!(
                                    target: "kg_inbox::courier",
                                    "in_flight mutex poisoned; recovering"
                                );
                                p.into_inner()
                            }
                        };
                        let outcome = process_one(
                            kg_inbox_path,
                            &file,
                            headless_ingest_tx,
                            &ProductionKgFileOps,
                            db,
                            progress,
                        );
                        log_outcome(&file, &outcome);
                    }
                    Err(RecvError) => {
                        tracing::warn!(target: "kg_inbox::courier", "input channel disconnected");
                        break;
                    }
                }
            }
        }
    }
}

fn log_outcome(file: &StableInboxFile, outcome: &KgCourierOutcome) {
    match outcome {
        KgCourierOutcome::Ingested {
            session_id,
            idempotent_skip,
        } => {
            tracing::info!(
                target: "kg_inbox::courier",
                src = %file.path.display(),
                session_id = session_id,
                idempotent_skip = idempotent_skip,
                "KG ingest ok; worker phase-4 will archive"
            );
        }
        KgCourierOutcome::Quarantined { reason, failed_to } => {
            tracing::warn!(
                target: "kg_inbox::courier",
                src = %file.path.display(),
                reason = %reason,
                dst = %failed_to.display(),
                "KG ingest failed; quarantined"
            );
        }
    }
}

// --------------------------------------------------------------------
// The actual work -- `process_one`
// --------------------------------------------------------------------

/// Process exactly one [`StableInboxFile`] from
/// `Knowledge Graph/Inbox/`.
///
/// Flow:
///
/// 1. Validate (extension allowlist, size cap, non-empty).
/// 2. Idempotency check: if a `sessions` row already references
///    this exact path via `audio_blob_path`, short-circuit and
///    let the worker phase-4 archive on its next tick.
/// 3. Decode to PCM.
/// 4. Build [`IngestProvenance::mobile_inbox_kg_note`] with the
///    courier source path threaded through.
/// 5. Enqueue [`HeadlessIngestRequest`]; await reply.
/// 6. Return [`KgCourierOutcome::Ingested`] -- crucially, NO move.
///    The worker's phase-4 archive moves the file to
///    `Knowledge Graph/History/<YYYY-MM>/<uuid>.<ext>`.
pub(crate) fn process_one(
    kg_inbox_path: &Path,
    file: &StableInboxFile,
    headless_ingest_tx: &HeadlessIngestSender,
    fs: &dyn KgFileOps,
    db: &Arc<Mutex<Connection>>,
    progress: &dyn IngestProgressBus,
) -> KgCourierOutcome {
    let original_filename = file
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("kg-inbox-courier")
        .to_string();

    // 1. Validate. Cheap checks first.
    if let Err(failure) = validate(&file.path, fs) {
        progress.emit(IngestProgressEvent::failed(
            ingest_progress::source::KG_INBOX,
            &original_filename,
            failure.to_string(),
        ));
        return quarantine(kg_inbox_path, file, failure, fs);
    }

    // 2. Idempotency check -- crash-recovery guard. If the previous
    //    run inserted a session but didn't get to phase-4 archive,
    //    on restart we'd otherwise re-ingest. The DB-by-path probe
    //    catches the live case (file still in Inbox/, session row
    //    points at it).
    match already_ingested(db, &file.path) {
        Ok(Some(existing_id)) => {
            tracing::info!(
                target: "kg_inbox::courier",
                src = %file.path.display(),
                session_id = existing_id,
                "idempotent skip: session already references this path; awaiting worker phase-4 archive"
            );
            return KgCourierOutcome::Ingested {
                session_id: existing_id,
                idempotent_skip: true,
            };
        }
        Ok(None) => { /* fresh file; fall through */ }
        Err(e) => {
            // DB read failure is not load-bearing for correctness
            // (the worst case is a duplicate session, recoverable
            // by user); log + proceed.
            tracing::warn!(
                target: "kg_inbox::courier",
                src = %file.path.display(),
                error = ?e,
                "idempotency DB probe failed; proceeding with ingest"
            );
        }
    }

    // 3. Decode.
    progress.emit(IngestProgressEvent::staged(
        ingest_progress::stage::DECODING,
        ingest_progress::source::KG_INBOX,
        &original_filename,
    ));
    let samples = match fs.decode(&file.path) {
        Ok(s) => s,
        Err(e) => {
            let err = e.to_string();
            progress.emit(IngestProgressEvent::failed(
                ingest_progress::source::KG_INBOX,
                &original_filename,
                format!("decode failed: {err}"),
            ));
            return quarantine(kg_inbox_path, file, KgCourierFailure::DecodeFailed(err), fs);
        }
    };
    tracing::info!(
        target: "kg_inbox::courier",
        path = %file.path.display(),
        samples = samples.len(),
        approx_seconds = samples.len() as f64 / 16_000.0,
        "decoded"
    );

    // 4. Build provenance. `mobile_inbox_kg_note` pins
    //    `capture_kind = KgNote` (source-gate enqueue) AND threads
    //    the courier path through as `audio_blob_path` so the
    //    worker's phase-4 archive has something to rename.
    progress.emit(IngestProgressEvent::staged(
        ingest_progress::stage::TRANSCRIBING,
        ingest_progress::source::KG_INBOX,
        &original_filename,
    ));
    let provenance = IngestProvenance::mobile_inbox_kg_note(
        original_filename.clone(),
        fs.now_iso(),
        file.path.clone(),
    );

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
            ingest_progress::source::KG_INBOX,
            &original_filename,
            "orchestrator unavailable".to_string(),
        ));
        return quarantine(
            kg_inbox_path,
            file,
            KgCourierFailure::OrchestratorUnavailable,
            fs,
        );
    }

    // 5. Block waiting for orchestrator.
    let session_id = match reply_rx.recv() {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => {
            let err = e.to_string();
            progress.emit(IngestProgressEvent::failed(
                ingest_progress::source::KG_INBOX,
                &original_filename,
                format!("ingest failed: {err}"),
            ));
            return quarantine(kg_inbox_path, file, KgCourierFailure::IngestFailed(err), fs);
        }
        Err(_) => {
            progress.emit(IngestProgressEvent::failed(
                ingest_progress::source::KG_INBOX,
                &original_filename,
                "orchestrator dropped reply channel".to_string(),
            ));
            return quarantine(
                kg_inbox_path,
                file,
                KgCourierFailure::OrchestratorDroppedReply,
                fs,
            );
        }
    };

    progress.emit(IngestProgressEvent::done(
        ingest_progress::source::KG_INBOX,
        &original_filename,
        session_id,
    ));

    // 6. NO move. The worker phase-4 archive does it. The file
    //    sits in `Knowledge Graph/Inbox/` until the worker drains
    //    `kg_filing_queue` and runs `archive_session_history`,
    //    which renames it to `Knowledge Graph/History/<YYYY-MM>/`.
    KgCourierOutcome::Ingested {
        session_id,
        idempotent_skip: false,
    }
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
#[path = "kg_inbox_courier_tests.rs"]
mod tests;
