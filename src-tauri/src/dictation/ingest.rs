//! Headless dictation ingest -- the audio-from-file entry point.
//!
//! ## What this is
//!
//! A pure-Rust function that takes pre-decoded 16 kHz mono i16 PCM
//! and runs it through the SAME VAD -> STT -> Cleanup pipeline the
//! live PTT path uses, then persists one [`sessions`] row plus the
//! `raw` and `cleaned` transcript stages. No injection. No
//! clipboard. No recording-window overlay. No foreground-app
//! capture.
//!
//! ## Where the audio comes from
//!
//! Two production callers, both passing samples produced by
//! [`crate::audio::decode::decode_to_pcm16_mono_16k`]:
//!
//! 1. Iter 1 -- "+ Audio file" desktop import button (mb-7vyz).
//!    User clicks the button, file picker opens, decoder runs,
//!    calls `headless_ingest` with
//!    `IngestProvenance::desktop_import(...)`.
//! 2. Iter 3 -- mobile inbox courier (mb-txmy). A vault-watcher
//!    notices a new .m4a delivered by the iOS Shortcut, decodes
//!    it, calls `headless_ingest` with
//!    `IngestProvenance::mobile_inbox(...)`.
//!
//! ## What it deliberately SKIPS (vs.
//! `DictationOrchestrator::complete`)
//!
//! Every one of these has either no analogue or no meaning in the
//! file-import flow, per ADR 0046 Section 3:
//!
//! - Foreground capture -- there is no target app. `foreground_app`
//!   is `None` in the persisted row.
//! - Focus-drift check -- same reason.
//! - Secure-input guard -- nothing is being injected.
//! - Injection -- the user wanted a transcript in the History
//!   list, not a paste into the focused app. No
//!   `transcripts(stage='final')` row is ever written from this
//!   path.
//! - Pill overlay state transitions -- no overlay window exists
//!   for a headless ingest. Progress, if any, is the IPC-handler's
//!   responsibility.
//! - Hotkey FSM `PipelineComplete` signal -- the FSM is not
//!   involved.
//!
//! ## What it KEEPS identical to PTT
//!
//! - VAD trim (with LSTM reset, same `TrimConfig::default()`).
//! - STT via `SpeechToText::transcribe` (same `TranscribeRequest`).
//! - Cleanup via `Cleaner::clean(raw, mode_slug)` with the SAME
//!   per-mode prompt selection used by PTT.
//! - Mode resolution via
//!   [`crate::dictation::resolve_active_mode_from_db`] (shared
//!   free function -- the user's active mode at ingest time wins,
//!   same way PTT respects the Modes page selector).
//! - Provenance: `prompt_id`, `dictionary_snapshot_id`,
//!   `example_set_id` all populated from the provided
//!   `OrchestratorConfig` defaults identical to PTT bootstrap.
//! - The `SessionsEventBus` emit so the React Dictations page
//!   refetches and the new row pops in immediately.
//!
//! ## What it owns
//!
//! Nothing. All deps come in via [`IngestDeps`]. This is
//! deliberate: the dependency graph is the caller's
//! responsibility, which keeps this module trivially testable with
//! mocks (the `tests` module at the bottom uses mockall-shaped
//! doubles).
//!
//! ## Failure modes
//!
//! - STT failure -> session row persisted with `status='error'`.
//!   Parallel to `DictationOrchestrator::persist_failed_stt`. The
//!   user sees the row appear with an error message; we do not
//!   lose the provenance.
//! - VAD failure -> fall back to raw (untrimmed) audio. Parallel
//!   to `complete()`'s behavior -- it is better to STT the whole
//!   clip than to abort.
//! - Cleanup failure -> cleaned text falls back to raw text. Same
//!   as `complete()`.
//! - DB write failure -> propagated as `AppError`. The caller is
//!   the IPC handler, which surfaces a toast.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::audio::vad::VoiceActivityDetector;
use crate::audio::{trim_speech, TrimConfig};
use crate::cleanup::Cleaner;
use crate::db::sessions::{
    self, NewSession, ProcessingCompletion, SessionSource, SessionStatus, StartMode,
};
use crate::db::transcripts;
use crate::dictation::events::SessionsEventBus;
use crate::dictation::{resolve_active_mode_from_db, OrchestratorConfig};
use crate::error::{AppError, AppResult};
use crate::stt::{SpeechToText, TranscribeRequest};

/// Where a headless-ingested session originated.
///
/// Threaded into `sessions.source` so the Iter 2 export job (and
/// the React UI) can distinguish desktop file-imports from iOS
/// courier imports. PTT sessions never go through this path --
/// they are always `SessionSource::Desktop`, written by the
/// orchestrator directly.
#[derive(Debug, Clone)]
pub struct IngestProvenance {
    /// `SessionSource::DesktopImport` for the + Audio file button,
    /// `SessionSource::MobileInbox` for the iOS Shortcut courier.
    /// `SessionSource::Desktop` is rejected at call time -- PTT is
    /// not a valid source for this code path.
    pub source: SessionSource,

    /// Original filename as the user provided it (or the courier
    /// dropped it). Used for the success toast plus future
    /// Dictations-page filename column. Not persisted yet (the
    /// sessions table has no dedicated column); Iter 2's export
    /// job will pull this from the audit log if needed.
    pub original_filename: String,

    /// ISO-8601 timestamp the file landed on disk / was selected.
    /// Used as `sessions.started_at` so the row sorts naturally in
    /// the Dictations list. For + Audio file this is "now"; for
    /// the courier it is the file's mtime at pickup.
    pub received_at_iso: String,
}

impl IngestProvenance {
    /// Sugar for the + Audio file button (Iter 1).
    pub fn desktop_import(original_filename: String, received_at_iso: String) -> Self {
        Self {
            source: SessionSource::DesktopImport,
            original_filename,
            received_at_iso,
        }
    }

    /// Sugar for the iOS Shortcut courier (Iter 3).
    pub fn mobile_inbox(original_filename: String, received_at_iso: String) -> Self {
        Self {
            source: SessionSource::MobileInbox,
            original_filename,
            received_at_iso,
        }
    }
}

/// Borrowed dependency bundle for one `headless_ingest` call.
///
/// All five live on the caller's stack -- typically the IPC
/// handler, which constructs them right before calling. Each
/// `&mut` / `&dyn` is one-shot: the function returns, the borrows
/// release, the caller is free to drop or reuse.
///
/// Why a borrowed struct vs. eight positional `fn` params:
///   - Future provenance / config additions do not ripple through
///     every call site.
///   - Tests can build a fresh `IngestDeps` per case without
///     re-stating field names in a constructor.
pub struct IngestDeps<'a> {
    /// Voice-activity detector. Mutable because Silero VAD carries
    /// LSTM hidden state we reset at the top of every ingest.
    pub vad: &'a mut dyn VoiceActivityDetector,

    /// Speech-to-text. Same trait the orchestrator uses; the impl
    /// is caller's choice (Whisper-rs in production, mock in
    /// tests). Mutable because `SpeechToText::transcribe` takes
    /// `&mut self` -- the Whisper-rs impl carries an internal
    /// state cache across calls.
    pub stt: &'a mut dyn SpeechToText,

    /// Cleanup-pass (LLM or passthrough). Mutable because the LLM
    /// impl holds an HTTP client + in-flight state across calls.
    pub cleaner: &'a mut dyn Cleaner,

    /// Database connection pool the orchestrator already holds. We
    /// take `&Arc<Mutex<...>>` (not `&Connection`) so this function
    /// locks once around the entire persist sequence -- matching
    /// the orchestrator's behavior in `persist_complete`.
    pub db: &'a Arc<Mutex<Connection>>,

    /// UI event channel -- fires `history:session-saved` with the
    /// new row id so the React Dictations page refetches.
    pub events: &'a dyn SessionsEventBus,

    /// Bootstrap-time defaults: dictionary snapshot, example set,
    /// fallback mode/prompt. The mode actually used is resolved at
    /// ingest time from the active-mode setting, falling back to
    /// these values -- same precedence PTT uses.
    pub config: &'a OrchestratorConfig,
}

/// All persistence-shaped inputs for one ingest success path.
/// Built once inside `headless_ingest` and passed to the persist
/// helper; keeping it in a struct avoids the multi-positional-arg
/// anti-pattern AND gives later Iters a single place to thread
/// additional provenance fields without re-touching every call
/// site.
struct IngestPersistParams<'a> {
    started_at_iso: &'a str,
    recording_ended_iso: String,
    audio_duration_ms: i64,
    raw_text: &'a str,
    cleaned_text: &'a str,
    cleanup_model: &'a str,
    stt_latency_ms: i64,
    cleanup_latency_ms: i64,
    provenance: &'a IngestProvenance,
    resolved_mode_id: i64,
    resolved_prompt_id: i64,
    /// From `OrchestratorConfig`. PTT pulls these from the same
    /// config field at session-insert time -- we mirror that
    /// exactly so FK invariants hold.
    dictionary_snapshot_id: i64,
    example_set_id: i64,
}

/// All inputs for the STT-failure persistence branch. Bundled to
/// keep the helper signature honest (rather than the previous
/// many-positional anti-pattern) and to leave room to add fields
/// without re-touching every call site.
struct IngestErrorParams<'a> {
    started_at_iso: &'a str,
    audio_duration_ms: i64,
    provenance: &'a IngestProvenance,
    resolved_mode_id: i64,
    resolved_prompt_id: i64,
    dictionary_snapshot_id: i64,
    example_set_id: i64,
    error_message: &'a str,
}

/// Headless ingest -- run pre-decoded PCM through VAD/STT/Cleanup
/// and persist as a `sessions` row + raw/cleaned transcripts.
///
/// Returns the inserted session row id on success.
///
/// `samples` must already be 16 kHz mono i16 PCM -- caller's
/// responsibility (use
/// [`crate::audio::decode::decode_to_pcm16_mono_16k`]).
///
/// Behavior contract: see this module's doc comment.
pub fn headless_ingest(
    deps: IngestDeps<'_>,
    samples: Vec<i16>,
    provenance: IngestProvenance,
) -> AppResult<i64> {
    // Guard: PTT is not a valid source for this path. Catching
    // this early prevents a category of "headless ingest with
    // desktop source" bugs that would corrupt the source-filter
    // for the Iter 2 export job.
    if provenance.source == SessionSource::Desktop {
        return Err(AppError::Other(
            "headless_ingest: SessionSource::Desktop is reserved for the PTT path".into(),
        ));
    }

    let started_at_iso = provenance.received_at_iso.clone();
    let audio_duration_ms = samples_to_duration_ms(samples.len());

    tracing::info!(
        source = provenance.source.as_db_str(),
        filename = %provenance.original_filename,
        samples = samples.len(),
        audio_duration_ms,
        "headless_ingest: begin"
    );

    // Pin the active mode the same way `start_capture` does --
    // user's current selection wins, with the config fields as the
    // fallback.
    let resolved = resolve_active_mode_from_db(
        deps.db,
        deps.config.mode_id,
        &deps.config.mode_slug,
        deps.config.prompt_id,
    );
    let mode_slug = resolved.slug.clone();

    // VAD trim. Reset LSTM at the start of every utterance -- each
    // file is a self-contained clip with no streaming context to
    // preserve. Errors fall back to raw audio (same as PTT).
    deps.vad.reset();
    let trimmed: Vec<i16> =
        trim_speech(&samples, deps.vad, &TrimConfig::default()).unwrap_or_else(|e| {
            tracing::warn!(error = ?e, "headless_ingest: VAD trim failed; using raw audio");
            samples.clone()
        });
    tracing::debug!(
        raw_samples = samples.len(),
        trimmed_samples = trimmed.len(),
        "headless_ingest: VAD trim done"
    );

    // STT.
    let stt_start = std::time::Instant::now();
    let stt_result = deps.stt.transcribe(TranscribeRequest {
        audio: &trimmed,
        initial_prompt: None,
        force_cpu: false,
    });
    let stt_latency_ms = stt_start.elapsed().as_millis() as i64;

    let raw_text = match stt_result {
        Ok(t) => t.text,
        Err(e) => {
            // Persist an error-status row so the user sees the
            // attempted ingest in their History list -- silent loss
            // is the worst failure mode for this path.
            let msg = format!("stt failed: {e}");
            tracing::warn!(error = ?e, "headless_ingest: STT failed; persisting error row");
            return persist_ingest_error(
                deps.db,
                deps.events,
                IngestErrorParams {
                    started_at_iso: &started_at_iso,
                    audio_duration_ms,
                    provenance: &provenance,
                    resolved_mode_id: resolved.mode_id,
                    resolved_prompt_id: resolved.prompt_id,
                    dictionary_snapshot_id: deps.config.dictionary_snapshot_id,
                    example_set_id: deps.config.example_set_id,
                    error_message: &msg,
                },
            );
        }
    };

    // Cleanup. Same passthrough/LLM dispatch as PTT.
    let cleanup_start = std::time::Instant::now();
    let cleaned_text = match deps.cleaner.clean(&raw_text, &mode_slug) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                error = ?e,
                "headless_ingest: cleaner failed; falling back to raw text"
            );
            raw_text.clone()
        }
    };
    let cleanup_latency_ms = cleanup_start.elapsed().as_millis() as i64;

    // Persist success.
    let recording_ended_iso = now_iso_utc();
    persist_ingest(
        deps.db,
        deps.events,
        IngestPersistParams {
            started_at_iso: &started_at_iso,
            recording_ended_iso,
            audio_duration_ms,
            raw_text: &raw_text,
            cleaned_text: &cleaned_text,
            cleanup_model: deps.cleaner.model_name(),
            stt_latency_ms,
            cleanup_latency_ms,
            provenance: &provenance,
            resolved_mode_id: resolved.mode_id,
            resolved_prompt_id: resolved.prompt_id,
            dictionary_snapshot_id: deps.config.dictionary_snapshot_id,
            example_set_id: deps.config.example_set_id,
        },
    )
}

/// Success-path persistence. Inserts the session row, raw + cleaned
/// transcripts, and updates the row to `complete` status. Fires the
/// `SessionsEventBus` emit after dropping the DB lock so the React
/// refetch can re-acquire it without contention (same pattern as
/// `DictationOrchestrator::persist_complete`).
fn persist_ingest(
    db: &Arc<Mutex<Connection>>,
    events: &dyn SessionsEventBus,
    p: IngestPersistParams<'_>,
) -> AppResult<i64> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Other("headless_ingest: db mutex poisoned".into()))?;

    let new = NewSession {
        uuid: new_uuid(),
        mode_id: p.resolved_mode_id,
        // sessions.hotkey_pressed is NOT NULL with no default; for
        // headless ingest there is no key press. Persist a stable
        // sentinel so the column has a defensible value AND so a
        // UI filter / log search can identify the file-import
        // lineage without joining on source. The sentinel reads
        // naturally in the Dictations page row tooltip.
        hotkey_pressed: format!("file-import:{}", p.provenance.source.as_db_str()),
        started_at: p.started_at_iso.to_string(),
        recording_ended_at: p.recording_ended_iso.clone(),
        status: SessionStatus::Processing,
        // No target app for a headless ingest.
        foreground_app: None,
        foreground_window_title: None,
        audio_duration_ms: p.audio_duration_ms,
        audio_blob_path: None,
        prompt_id: p.resolved_prompt_id,
        dictionary_snapshot_id: p.dictionary_snapshot_id,
        example_set_id: p.example_set_id,
        // Headless ingest is not a PTT session; not an in-app live
        // session either. Reuse `InApp` because that is the only
        // non-PTT enum we have, and the UX semantics line up (no
        // foreground paste happened).
        start_mode: StartMode::InApp,
        source: p.provenance.source,
    };

    let id = sessions::insert(&conn, &new)?;

    // Provenance writes. Failures are non-fatal -- the session row
    // is the durable record; transcript write failure logs + moves
    // on so the status update still lands.
    if let Err(e) = transcripts::insert_raw(&conn, id, p.raw_text) {
        tracing::warn!(error = ?e, session_id = id, "persist raw transcript failed");
    }
    if let Err(e) = transcripts::insert_cleaned(&conn, id, p.cleaned_text, p.cleanup_model) {
        tracing::warn!(error = ?e, session_id = id, "persist cleaned transcript failed");
    }
    // NO final stage -- nothing was injected. This is the
    // load-bearing distinction from the PTT path.

    sessions::update_processing_complete(
        &conn,
        id,
        &ProcessingCompletion {
            completed_at: now_iso_utc(),
            status: SessionStatus::Complete,
            stt_latency_ms: Some(p.stt_latency_ms),
            cleanup_latency_ms: Some(p.cleanup_latency_ms),
            injection_latency_ms: None,
            // No injection -- leave the column NULL so downstream
            // filters can identify headless ingests by
            // `injection_status IS NULL` without needing to join
            // on source.
            injection_status: None,
        },
    )?;

    drop(conn);
    events.emit_session_saved(id);
    tracing::info!(
        session_id = id,
        source = p.provenance.source.as_db_str(),
        stt_latency_ms = p.stt_latency_ms,
        cleanup_latency_ms = p.cleanup_latency_ms,
        "headless_ingest: success"
    );
    Ok(id)
}

/// STT-failure persistence -- mirrors `persist_failed_stt` from
/// the orchestrator. The session row lands so the user sees the
/// attempted ingest; the error message is on the row. Same
/// dictionary / example FK threading as the success path so the
/// row passes referential checks identically.
fn persist_ingest_error(
    db: &Arc<Mutex<Connection>>,
    events: &dyn SessionsEventBus,
    p: IngestErrorParams<'_>,
) -> AppResult<i64> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Other("headless_ingest: db mutex poisoned".into()))?;

    let new = NewSession {
        uuid: new_uuid(),
        mode_id: p.resolved_mode_id,
        hotkey_pressed: format!("file-import:{}", p.provenance.source.as_db_str()),
        started_at: p.started_at_iso.to_string(),
        recording_ended_at: now_iso_utc(),
        status: SessionStatus::Processing,
        foreground_app: None,
        foreground_window_title: None,
        audio_duration_ms: p.audio_duration_ms,
        audio_blob_path: None,
        prompt_id: p.resolved_prompt_id,
        dictionary_snapshot_id: p.dictionary_snapshot_id,
        example_set_id: p.example_set_id,
        start_mode: StartMode::InApp,
        source: p.provenance.source,
    };

    let id = sessions::insert(&conn, &new)?;
    sessions::update_status_error(&conn, id, p.error_message)?;
    drop(conn);
    events.emit_session_saved(id);
    Ok(id)
}

/// Convert 16 kHz mono i16 sample count to milliseconds.
fn samples_to_duration_ms(sample_count: usize) -> i64 {
    // 16 000 samples / sec -> 1 ms = 16 samples.
    (sample_count as i64) * 1000 / 16_000
}

/// Mint a fresh UUID v4 -- same shape as `dictation::new_uuid`,
/// duplicated here so this module has no pub(crate) dep on the
/// parent's private helper. Keeps the module's surface area honest.
fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// ISO-8601 UTC timestamp with millisecond precision. Matches the
/// shape produced by `dictation::now_iso` so DB rows from PTT and
/// headless ingest sort identically.
fn now_iso_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    //! NOTE: these tests are NOT runnable on this Windows box via
    //! `cargo test --release` due to the
    //! `STATUS_ENTRYPOINT_NOT_FOUND` issue documented in LESSONS
    //! 2026-05-17 (mb-0n8c). They DO compile via
    //! `cargo test --release --no-run`, which is the sanctioned
    //! gate. The pure-Rust subset of these tests is exercisable
    //! via the throwaway-crate recipe (LESSONS 2026-05-17).

    use super::*;

    #[test]
    fn samples_to_duration_ms_basics() {
        assert_eq!(samples_to_duration_ms(0), 0);
        assert_eq!(samples_to_duration_ms(16), 1);
        assert_eq!(samples_to_duration_ms(16_000), 1000);
        assert_eq!(samples_to_duration_ms(8_000), 500);
    }

    #[test]
    fn ingest_provenance_constructors_set_source() {
        let dimp = IngestProvenance::desktop_import("a.m4a".into(), "2026-05-25T00:00:00Z".into());
        assert_eq!(dimp.source, SessionSource::DesktopImport);
        let mob = IngestProvenance::mobile_inbox("b.m4a".into(), "2026-05-25T00:00:00Z".into());
        assert_eq!(mob.source, SessionSource::MobileInbox);
    }

    #[test]
    fn new_uuid_unique() {
        let a = new_uuid();
        let b = new_uuid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36); // canonical 8-4-4-4-12 form
    }
}
