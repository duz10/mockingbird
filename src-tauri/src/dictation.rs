//! Dictation orchestrator — the glue that turns
//! [`crate::hotkey::state::StateAction`]s into a full
//! `audio → STT → cleanup → inject → persist` pipeline run.
//!
//! ## Architecture
//!
//! ```text
//!  State driver thread ──► Receiver<StateAction>
//!                                │
//!                                ▼
//!                     ┌─────────────────────────┐
//!                     │  DictationOrchestrator  │
//!                     │                         │
//!                     │  - audio: AudioCapture  │   (Phase 2)
//!                     │  - stt:   SpeechToText  │   (Phase 2)
//!                     │  - cleaner: Cleaner     │   (Phase 4)
//!                     │  - injector: Injector   │   (Phase 3 Wave 4)
//!                     │  - secure_guard         │   (Phase 3 Wave 2)
//!                     │  - window_ctx           │   (Phase 3 Wave 2)
//!                     │  - recording_window     │   (Phase 5 stub)
//!                     │  - db                   │   (Phase 1)
//!                     │  - fg_keydown: Option   │   ← captured on StartCapture
//!                     │  - user_overrides       │
//!                     │  - config               │
//!                     │                         │
//!                     │  run() loops on rx.iter │
//!                     └─────────────────────────┘
//! ```
//!
//! ## Pure-vs-OS split
//!
//! The complete pipeline is broken in two:
//!
//! 1. [`pipeline::Inputs`] / [`pipeline::run`] is the **pure decision
//!    layer**: given STT result + foreground snapshots + secure-input
//!    answer + cleaner output, decide what to inject (if anything)
//!    and what to persist. No OS calls, no traits, just `&str` +
//!    structs in/out. Trivially testable.
//!
//! 2. The orchestrator's `complete()` method is the **side-effect
//!    layer**: it asks the traits for their answers, hands them to
//!    [`pipeline::run`], then performs the resulting actions
//!    (`injector.inject`, `db::sessions::update_processing_complete`).
//!
//! This is the same pattern Wave 3 established for `hotkey::windows`
//! (pure `classify_keystroke` + thin OS shim).
//!
//! ## Submodules
//!
//! - [`runtime`]: Wave 4.5 spawn glue that wires the orchestrator
//!   into `lib.rs::run()` with the platform-default traits.

pub mod runtime;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rusqlite::Connection;

use crate::audio::vad::VoiceActivityDetector;
use crate::audio::{trim_speech, AudioCapture, TrimConfig};
use crate::cleanup::Cleaner;
use crate::db::sessions::{self, NewSession, ProcessingCompletion, SessionStatus};
use crate::db::transcripts;
use crate::error::{AppError, AppResult};
use crate::hotkey::state::StateAction;
use crate::hotkey::HotkeyEvent;
use crate::injection::secure_guard::SecureInputGuard;
use crate::injection::strategy::InjectionStrategy;
use crate::injection::{InjectionOutcome, Injector};
use crate::recording_window::RecordingWindow;
use crate::stt::{SpeechToText, TranscribeRequest};
use crate::window_context::{ForegroundWindow, WindowContext};

/// Provenance + mode config the orchestrator needs at session-insert
/// time.
///
/// All FK values must point at real rows. The orchestrator does NOT
/// resolve them dynamically — Wave 5+ will reload these when the user
/// switches modes; for Wave 4 they're set once at orchestrator
/// construction.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// `modes.id` for the active mode (Normal / Fragment / Verbose).
    pub mode_id: i64,
    /// `modes.slug` — passed to [`Cleaner::clean`] to select its prompt.
    pub mode_slug: String,
    /// Active `prompts.id`. Required by NewSession.
    pub prompt_id: i64,
    /// Active `dictionary_snapshots.id`. Required by NewSession.
    pub dictionary_snapshot_id: i64,
    /// Active `example_sets.id`. Required by NewSession.
    pub example_set_id: i64,
    /// Label written to `sessions.hotkey_pressed` (e.g. `"RightAlt"`).
    pub hotkey_label: String,
}

/// The owner struct. Construct via [`DictationOrchestrator::new`],
/// then call [`Self::run`] to enter the event loop.
pub struct DictationOrchestrator {
    audio: Box<dyn AudioCapture>,
    vad: Box<dyn VoiceActivityDetector>,
    stt: Box<dyn SpeechToText>,
    cleaner: Box<dyn Cleaner>,
    injector: Box<dyn Injector>,
    window_ctx: Box<dyn WindowContext>,
    secure_guard: Box<dyn SecureInputGuard>,
    recording_window: RecordingWindow,
    db: Arc<Mutex<Connection>>,
    config: OrchestratorConfig,
    user_overrides: HashMap<String, InjectionStrategy>,

    /// Feedback channel to the hotkey driver. After every
    /// `complete()` / `discard()` (including error paths) we emit
    /// `HotkeyEvent::PipelineComplete` so the §6.1 state machine can
    /// transition `Processing → Idle`. Without this signal the
    /// machine sticks in `Processing` after one hold and ignores
    /// every subsequent KeyDown.
    hotkey_tx: Sender<HotkeyEvent>,

    // Per-session transient state.
    state: SessionState,
}

/// All inputs `persist_complete` needs to write one fully-provenanced
/// session row + its `raw` / `cleaned` / `final` transcript stages.
///
/// Lives on its own struct (rather than 11 method params) so adding
/// new provenance fields in later waves doesn't ripple through the
/// call site as another positional argument. Borrowed lifetime `'a`
/// keeps text strings as `&str` slices — no clones on the hot path.
struct PersistCompleteParams<'a> {
    recording_ended_iso: String,
    /// Reserved for Wave 5 — currently written to `_` because
    /// `audio_duration_ms` is filled at the VAD-trim step.
    recording_duration_ms: i64,
    fg_keyup: &'a ForegroundWindow,
    outcome: InjectionOutcome,
    stt_latency_ms: Option<i64>,
    cleanup_latency_ms: Option<i64>,
    injection_latency_ms: Option<i64>,
    /// Raw STT output — IMMUTABLE per ADR 0010.
    raw_text: &'a str,
    /// Post-cleanup text. Equal to `raw_text` when the passthrough
    /// cleaner is used (Wave 4 default).
    cleaned_text: &'a str,
    /// What was actually injected. `None` when injection was aborted
    /// (secure input, user opt-out) → no `final` row.
    injected_text: Option<&'a str>,
    /// Cleanup model identifier for `transcripts.model_used`.
    cleanup_model: &'a str,
}

#[derive(Default)]
struct SessionState {
    /// `ForegroundWindow` snapshotted on `StartCapture`. Used by the
    /// focus-loss double-snapshot at `StopCapture` time.
    fg_keydown: Option<ForegroundWindow>,
    /// Wall-clock start, recorded on `StartCapture` for the DB row.
    started_at: Option<Instant>,
    /// ISO-8601 string for DB persistence.
    started_at_iso: Option<String>,
}

impl DictationOrchestrator {
    /// Construct an orchestrator from its dependencies.
    ///
    /// `user_overrides` is the per-app strategy table from settings
    /// (e.g. `{"chrome.exe": Paste, "1Password.exe": Abort}`). Empty
    /// map is acceptable — the [`crate::injection::strategy::BUILTIN_OVERRIDES`]
    /// table still applies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        audio: Box<dyn AudioCapture>,
        vad: Box<dyn VoiceActivityDetector>,
        stt: Box<dyn SpeechToText>,
        cleaner: Box<dyn Cleaner>,
        injector: Box<dyn Injector>,
        window_ctx: Box<dyn WindowContext>,
        secure_guard: Box<dyn SecureInputGuard>,
        recording_window: RecordingWindow,
        db: Arc<Mutex<Connection>>,
        config: OrchestratorConfig,
        user_overrides: HashMap<String, InjectionStrategy>,
        hotkey_tx: Sender<HotkeyEvent>,
    ) -> Self {
        Self {
            audio,
            vad,
            stt,
            cleaner,
            injector,
            window_ctx,
            secure_guard,
            recording_window,
            db,
            config,
            user_overrides,
            hotkey_tx,
            state: SessionState::default(),
        }
    }

    /// Send `PipelineComplete` to the state-machine driver.
    ///
    /// Called at the end of every terminal orchestrator action
    /// (`complete()`, `discard()`, and the persistence-helper error
    /// paths invoked from inside `complete()`). The state machine
    /// uses this to transition `Processing → Idle`; without it the
    /// machine sticks in `Processing` and silently ignores all
    /// further hotkey events per §6.1.
    ///
    /// Send failure is tracing-logged but not propagated: if the
    /// driver is gone, we're shutting down anyway and the orchestrator
    /// will see its own action channel close on the next iteration.
    fn signal_pipeline_complete(&self) {
        if let Err(e) = self.hotkey_tx.send(HotkeyEvent::PipelineComplete) {
            tracing::warn!(error = ?e, "could not signal PipelineComplete — driver gone?");
        }
    }

    /// Run the event loop. Returns when the channel closes (Driver
    /// thread exited).
    pub fn run(mut self, actions: Receiver<StateAction>) -> AppResult<()> {
        for action in actions.iter() {
            if let Err(e) = self.handle(action) {
                // Per ADR 0010 + the "never lose provenance" rule, a
                // pipeline error doesn't kill the orchestrator. Log
                // and continue.
                tracing::error!(error = ?e, "orchestrator action failed");
            }
        }
        Ok(())
    }

    fn handle(&mut self, action: StateAction) -> AppResult<()> {
        match action {
            StateAction::None => Ok(()),
            StateAction::StartCapture(_mode) => self.start_capture(),
            // Wave 4.8: every terminal action MUST emit
            // PipelineComplete back to the state machine, even on
            // pipeline error. Centralised here (not inside
            // complete()/discard()) so the signal can't get lost in
            // an early-return error path inside the helpers.
            StateAction::StopCapture => {
                let result = self.complete();
                self.signal_pipeline_complete();
                result
            }
            StateAction::DiscardAudio => {
                let result = self.discard();
                self.signal_pipeline_complete();
                result
            }
            StateAction::ShowConfirmCancel => {
                // Wave 5 surfaces the tray toast. For now: log.
                tracing::info!("confirm-cancel UI requested (Wave 5 will render)");
                Ok(())
            }
            StateAction::HideConfirmCancel => {
                tracing::debug!("confirm-cancel UI dismissed");
                Ok(())
            }
        }
    }

    fn start_capture(&mut self) -> AppResult<()> {
        self.state.fg_keydown = self.window_ctx.foreground().ok();
        self.state.started_at = Some(Instant::now());
        self.state.started_at_iso = Some(now_iso());
        self.audio.start()?;
        // show() handles both the OS-level window display + emitting
        // the initial `listening` state to the React overlay.
        self.recording_window.show(&self.config.mode_slug)?;
        tracing::info!(
            fg = ?self.state.fg_keydown.as_ref().map(|f| &f.process_name),
            "dictation: start_capture"
        );
        Ok(())
    }

    fn complete(&mut self) -> AppResult<()> {
        // Snapshot key-up state.
        let stop_at = Instant::now();
        let stop_iso = now_iso();
        self.audio.stop()?;
        // Window stays VISIBLE through the rest of the pipeline so
        // the user gets progress feedback (transcribing → cleaning →
        // pasting → done). It's hidden at the end of this fn, or by
        // the persist_failed_* helpers on early-return paths.
        let mode_slug = self.config.mode_slug.clone();

        // Drain the audio.
        let mut samples: Vec<i16> = Vec::new();
        self.audio.drain(&mut samples)?;
        let recording_duration_ms = self
            .state
            .started_at
            .map(|s| stop_at.duration_since(s).as_millis() as i64)
            .unwrap_or(0);

        tracing::debug!(
            target: "audio",
            drained_samples = samples.len(),
            recording_duration_ms,
            "complete(): drained audio from ring"
        );

        // Dump the post-resample buffer to a WAV file so a human
        // can listen to it on demand. Single-slot (overwrites every
        // session). Wave 5 will gate this behind a tray-menu "debug
        // capture" toggle; for now it's always-on (cheap).
        if let Err(e) = debug_dump_wav(&samples) {
            tracing::warn!(error = ?e, "debug WAV dump failed");
        }

        // Pull key-up snapshot. None == nothing focused (e.g. between
        // desktops) — treat as focus-changed abort.
        let fg_keyup = match self.window_ctx.foreground() {
            Ok(fg) => fg,
            Err(e) => {
                tracing::warn!(error = ?e, "no foreground at key-up; aborting");
                return self.persist_failed_no_foreground(stop_iso, recording_duration_ms);
            }
        };

        // Trim with VAD. Errors here fall back to the raw audio —
        // it's better to STT the whole clip than to abort.
        //
        // Reset Silero LSTM state at the start of every utterance.
        // Each dictation is a self-contained clip — there's no
        // streaming context to preserve, and state poisoning from
        // prior captures would bias the model.
        self.vad.reset();
        let trim_start = Instant::now();
        let trimmed: Vec<i16> = trim_speech(&samples, self.vad.as_mut(), &TrimConfig::default())
            .unwrap_or_else(|e| {
                tracing::warn!(error = ?e, "VAD trim failed; falling back to raw audio");
                samples.clone()
            });
        let _vad_ms = trim_start.elapsed().as_millis() as i64;

        tracing::debug!(
            target: "audio",
            raw_samples = samples.len(),
            trimmed_samples = trimmed.len(),
            vad_ms = _vad_ms,
            "complete(): VAD trim result"
        );

        // STT.
        self.recording_window
            .set_state(crate::recording_window::state::TRANSCRIBING, Some(&mode_slug));
        let stt_start = Instant::now();
        let stt_result = self.stt.transcribe(TranscribeRequest {
            audio: &trimmed,
            initial_prompt: None, // prompt-builder wiring is Phase 4
            force_cpu: false,
        });
        let stt_latency_ms = stt_start.elapsed().as_millis() as i64;
        let raw_text = match stt_result {
            Ok(t) => t.text,
            Err(e) => {
                return self.persist_failed_stt(stop_iso, recording_duration_ms, &fg_keyup, e)
            }
        };

        // Cleanup.
        self.recording_window
            .set_state(crate::recording_window::state::CLEANING, Some(&mode_slug));
        let cleanup_start = Instant::now();
        let cleaned_text = match self.cleaner.clean(&raw_text, &self.config.mode_slug) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = ?e, "cleaner failed; falling back to raw text");
                raw_text.clone()
            }
        };
        let cleanup_latency_ms = cleanup_start.elapsed().as_millis() as i64;

        // Secure-input check + focus-loss + strategy resolution.
        let is_secure = self.secure_guard.is_secure(&fg_keyup);
        let inputs = pipeline::Inputs {
            fg_keydown: self.state.fg_keydown.as_ref(),
            fg_keyup: &fg_keyup,
            is_secure,
            user_overrides: &self.user_overrides,
        };
        let decision = pipeline::decide(&inputs);

        // Inject (or skip per decision).
        self.recording_window
            .set_state(crate::recording_window::state::PASTING, Some(&mode_slug));
        let inject_start = Instant::now();
        let outcome = match decision {
            pipeline::Decision::Proceed(strategy) => {
                match self.injector.inject(&cleaned_text, strategy) {
                    Ok(o) => o,
                    Err(e) => {
                        tracing::warn!(error = ?e, "injector returned error");
                        InjectionOutcome::FailedSendInput
                    }
                }
            }
            pipeline::Decision::Abort(o) => o,
        };
        let injection_latency_ms = inject_start.elapsed().as_millis() as i64;

        // Persist. Whether the `final` transcript stage gets a row
        // depends on whether we actually injected something: an
        // aborted-secure or aborted-user-opt-out session has NO
        // final stage (nothing was injected), but raw + cleaned
        // must always be persisted (provenance > shortcuts).
        let injected_text = match outcome {
            InjectionOutcome::Ok | InjectionOutcome::OkClipboardNotRestored => {
                Some(cleaned_text.as_str())
            }
            _ => None,
        };
        self.persist_complete(PersistCompleteParams {
            recording_ended_iso: stop_iso,
            recording_duration_ms,
            fg_keyup: &fg_keyup,
            outcome,
            stt_latency_ms: Some(stt_latency_ms),
            cleanup_latency_ms: Some(cleanup_latency_ms),
            injection_latency_ms: Some(injection_latency_ms),
            raw_text: &raw_text,
            cleaned_text: &cleaned_text,
            injected_text,
            cleanup_model: self.cleaner.model_name(),
        })?;

        // Brief "done" flash before the window vanishes. The 200ms
        // sleep is on the dedicated dictation thread; the next
        // hotkey can't fire until the state machine receives
        // PipelineComplete (sent by the caller in handle()), so the
        // brief pause never delays the user.
        self.recording_window
            .set_state(crate::recording_window::state::DONE, Some(&mode_slug));
        std::thread::sleep(std::time::Duration::from_millis(200));
        self.recording_window.hide()?;

        // Reset transient state.
        self.state = SessionState::default();
        Ok(())
    }

    fn discard(&mut self) -> AppResult<()> {
        self.audio.stop()?;
        let mut samples: Vec<i16> = Vec::new();
        let _ = self.audio.drain(&mut samples); // discard
        self.recording_window.hide()?;
        tracing::info!("dictation: discard");
        self.state = SessionState::default();
        Ok(())
    }

    // ----- persistence helpers -----

    fn persist_complete(&self, p: PersistCompleteParams<'_>) -> AppResult<()> {
        let _ = p.recording_duration_ms; // Wave 5 fills `audio_duration_ms`.
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Other("orchestrator: db mutex poisoned".into()))?;
        let id = self.insert_session_row(&conn, &p.recording_ended_iso, p.fg_keyup)?;

        // Transcripts: ALWAYS write raw + cleaned (provenance > shortcuts).
        // Write final only if something was actually injected. Errors
        // are non-fatal — log + continue so the session row at least
        // gets its terminal status update.
        if let Err(e) = transcripts::insert_raw(&conn, id, p.raw_text) {
            tracing::warn!(error = ?e, session_id = id, "persist raw transcript failed");
        }
        if let Err(e) = transcripts::insert_cleaned(&conn, id, p.cleaned_text, p.cleanup_model) {
            tracing::warn!(error = ?e, session_id = id, "persist cleaned transcript failed");
        }
        if let Some(injected) = p.injected_text {
            if let Err(e) = transcripts::insert_final(&conn, id, injected, Some(p.cleanup_model)) {
                tracing::warn!(error = ?e, session_id = id, "persist final transcript failed");
            }
        }

        sessions::update_processing_complete(
            &conn,
            id,
            &ProcessingCompletion {
                completed_at: now_iso(),
                status: SessionStatus::Complete,
                stt_latency_ms: p.stt_latency_ms,
                cleanup_latency_ms: p.cleanup_latency_ms,
                injection_latency_ms: p.injection_latency_ms,
                injection_status: Some(p.outcome.as_db_str().to_string()),
            },
        )
    }

    fn persist_failed_stt(
        &self,
        recording_ended_iso: String,
        _recording_duration_ms: i64,
        fg_keyup: &ForegroundWindow,
        err: AppError,
    ) -> AppResult<()> {
        // Surface to the overlay BEFORE the DB write so the user sees
        // an error indicator immediately even if persistence stalls.
        let msg = format!("stt failed: {err}");
        self.recording_window.set_error(&msg);
        self.recording_window.hide()?;
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Other("orchestrator: db mutex poisoned".into()))?;
        let id = self.insert_session_row(&conn, &recording_ended_iso, fg_keyup)?;
        sessions::update_status_error(&conn, id, &msg)?;
        Ok(())
    }

    fn persist_failed_no_foreground(
        &self,
        recording_ended_iso: String,
        _recording_duration_ms: i64,
    ) -> AppResult<()> {
        self.recording_window
            .set_error("no foreground window at key-up");
        self.recording_window.hide()?;
        let conn = self
            .db
            .lock()
            .map_err(|_| AppError::Other("orchestrator: db mutex poisoned".into()))?;
        // No fg_keyup means we can't fill foreground_app — leave NULL.
        let id = self.insert_session_row_no_fg(&conn, &recording_ended_iso)?;
        sessions::update_status_error(&conn, id, "no foreground window at key-up")?;
        Ok(())
    }

    fn insert_session_row(
        &self,
        conn: &Connection,
        recording_ended_iso: &str,
        fg_keyup: &ForegroundWindow,
    ) -> AppResult<i64> {
        let started_at = self.state.started_at_iso.clone().unwrap_or_else(now_iso);
        let new = NewSession {
            uuid: new_uuid(),
            mode_id: self.config.mode_id,
            hotkey_pressed: self.config.hotkey_label.clone(),
            started_at,
            recording_ended_at: recording_ended_iso.to_string(),
            status: SessionStatus::Processing,
            foreground_app: Some(fg_keyup.process_name.clone()),
            foreground_window_title: Some(fg_keyup.title.clone()),
            audio_duration_ms: 0, // Wave 5 fills from VAD-trimmed length
            audio_blob_path: None,
            prompt_id: self.config.prompt_id,
            dictionary_snapshot_id: self.config.dictionary_snapshot_id,
            example_set_id: self.config.example_set_id,
        };
        sessions::insert(conn, &new)
    }

    fn insert_session_row_no_fg(
        &self,
        conn: &Connection,
        recording_ended_iso: &str,
    ) -> AppResult<i64> {
        let started_at = self.state.started_at_iso.clone().unwrap_or_else(now_iso);
        let new = NewSession {
            uuid: new_uuid(),
            mode_id: self.config.mode_id,
            hotkey_pressed: self.config.hotkey_label.clone(),
            started_at,
            recording_ended_at: recording_ended_iso.to_string(),
            status: SessionStatus::Processing,
            foreground_app: None,
            foreground_window_title: None,
            audio_duration_ms: 0,
            audio_blob_path: None,
            prompt_id: self.config.prompt_id,
            dictionary_snapshot_id: self.config.dictionary_snapshot_id,
            example_set_id: self.config.example_set_id,
        };
        sessions::insert(conn, &new)
    }
}

// --------------------------------------------------------------------
// Pure pipeline — testable without traits or OS calls
// --------------------------------------------------------------------

pub mod pipeline {
    //! Pure pipeline-decision functions.
    //!
    //! These run inside [`super::DictationOrchestrator::complete`]
    //! and are the "what should we do?" logic. The orchestrator's
    //! side-effect layer hands them values + acts on the decision.

    use std::collections::HashMap;

    use crate::injection::strategy::InjectionStrategy;
    use crate::injection::strategy_wiring::{decide_injection, InjectionDecision};
    use crate::injection::InjectionOutcome;
    use crate::window_context::ForegroundWindow;

    /// All inputs the decision layer needs.
    #[derive(Debug)]
    pub struct Inputs<'a> {
        /// Foreground at key-down, if we captured it (rare miss).
        pub fg_keydown: Option<&'a ForegroundWindow>,
        /// Foreground at key-up — never `None` here (caller already
        /// handled the no-foreground case).
        pub fg_keyup: &'a ForegroundWindow,
        /// Did the secure-input guard flag this target?
        pub is_secure: bool,
        /// User-defined per-app strategy overrides.
        pub user_overrides: &'a HashMap<String, InjectionStrategy>,
    }

    /// Resolved decision.
    #[derive(Debug, PartialEq, Eq)]
    pub enum Decision {
        /// Proceed with the given strategy.
        Proceed(InjectionStrategy),
        /// Abort with the given outcome (for DB persistence).
        Abort(InjectionOutcome),
    }

    /// Decide what to do. Pure.
    ///
    /// Precedence (top wins):
    ///   1. Secure-input on `fg_keyup` → `Abort(AbortedSecure)`.
    ///   2. Strategy resolution on `fg_keyup` → either `Proceed(...)`
    ///      or `Abort(AbortedUserOptOut)`.
    ///
    /// **No focus-change abort.** Per ADR 0020 (Wave 4.9), focus
    /// change between key-down and key-up is permissive: injection
    /// proceeds into whatever is focused at key-up, with full
    /// provenance recorded. The secure-input guard above runs on
    /// `fg_keyup` so password fields are still independently caught
    /// regardless of which app held focus at key-down.
    pub fn decide(inputs: &Inputs<'_>) -> Decision {
        if inputs.is_secure {
            return Decision::Abort(InjectionOutcome::AbortedSecure);
        }
        match decide_injection(inputs.fg_keydown, inputs.fg_keyup, inputs.user_overrides) {
            InjectionDecision::Proceed(s) => Decision::Proceed(s),
            InjectionDecision::AbortUserOptOut => {
                Decision::Abort(InjectionOutcome::AbortedUserOptOut)
            }
        }
    }
}

// --------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------

fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Dump the post-resample drained audio to a WAV file at
/// `%APPDATA%/com.dustin.mockingbird/last_capture.wav`.
///
/// Single-slot (overwrites every session). Useful for debugging
/// audio-pipeline issues — open the file in any audio player to
/// verify what we're handing to Silero + Whisper. Wave 5 will gate
/// behind a tray-menu toggle.
fn debug_dump_wav(samples: &[i16]) -> AppResult<()> {
    // Resolve %APPDATA%\com.dustin.mockingbird directly — this is
    // diagnostic-only code so we accept the cross-cutting env lookup
    // rather than threading an app_data_dir down through the
    // orchestrator's constructor (Wave 5 will gate this behind a CLI
    // flag and revisit the path-resolution story).
    let app_data =
        std::env::var("APPDATA").map_err(|e| AppError::Audio(format!("APPDATA env: {e}")))?;
    let dir = std::path::PathBuf::from(app_data).join("com.dustin.mockingbird");
    let path = dir.join("last_capture.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| AppError::Audio(format!("WAV create: {e}")))?;
    for &s in samples {
        writer
            .write_sample(s)
            .map_err(|e| AppError::Audio(format!("WAV write_sample: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| AppError::Audio(format!("WAV finalize: {e}")))?;
    tracing::info!(target: "audio", path = %path.display(), "debug WAV dumped");
    Ok(())
}

fn now_iso() -> String {
    // Phase 1 chose chrono-free time strings; format manually to keep
    // the dep surface minimal. Pattern: 2026-05-17T03:14:15Z.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_secs_as_iso(secs)
}

/// Format a Unix timestamp (seconds) as an ISO-8601 string in UTC.
/// Pure helper — testable without clock access.
pub fn format_secs_as_iso(secs: u64) -> String {
    // Naive but correct UTC seconds → broken-down time. Matches what
    // SQLite's `datetime('now')` produces.
    let (days, time_of_day) = (secs / 86_400, secs % 86_400);
    let (hours, rest) = (time_of_day / 3600, time_of_day % 3600);
    let (minutes, seconds) = (rest / 60, rest % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Howard Hinnant's days→Y/M/D algorithm (public domain). Used here
/// so we don't pull in chrono just for one ISO string.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::injection::InjectionOutcome;

    fn fg(name: &str) -> ForegroundWindow {
        ForegroundWindow {
            hwnd: 0x100,
            title: "Title".into(),
            process_name: name.into(),
            exe_path: None,
        }
    }

    // -----------------------------------------------------------------
    // pipeline::decide — exhaustive cases
    // -----------------------------------------------------------------

    #[test]
    fn happy_path_proceeds_with_paste() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Paste));
    }

    #[test]
    fn secure_input_aborts_even_if_focus_matches() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: true,
            user_overrides: &overrides,
        });
        assert_eq!(
            d,
            pipeline::Decision::Abort(InjectionOutcome::AbortedSecure)
        );
    }

    #[test]
    fn focus_change_proceeds_into_keyup_app_per_adr_0020() {
        // ADR 0020 permissive policy: user dictates into notepad,
        // alt-tabs to chrome before releasing the hotkey, text
        // lands in chrome. chrome.exe → Paste strategy.
        let prev = fg("notepad.exe");
        let now = fg("chrome.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Paste));
    }

    #[test]
    fn focus_change_into_secure_input_still_aborts() {
        // Secure-input guard runs on fg_keyup regardless of where
        // focus was at key-down — belt-and-suspenders for the
        // permissive focus-change policy.
        let prev = fg("notepad.exe");
        let now = fg("chrome.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: true,
            user_overrides: &overrides,
        });
        assert_eq!(
            d,
            pipeline::Decision::Abort(InjectionOutcome::AbortedSecure)
        );
    }

    #[test]
    fn user_opt_out_aborts_via_password_manager() {
        let prev = fg("1Password.exe");
        let now = fg("1Password.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(
            d,
            pipeline::Decision::Abort(InjectionOutcome::AbortedUserOptOut)
        );
    }

    #[test]
    fn terminal_uses_keystroke_strategy() {
        let prev = fg("WindowsTerminal.exe");
        let now = fg("WindowsTerminal.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Keystroke));
    }

    #[test]
    fn missing_keydown_does_not_block_injection() {
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: None,
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        // No fg_keydown means no focus-loss check; happy path.
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Paste));
    }

    #[test]
    fn user_override_to_keystroke_wins_over_builtin_paste() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let mut overrides = HashMap::new();
        overrides.insert("notepad.exe".into(), InjectionStrategy::Keystroke);
        let d = pipeline::decide(&pipeline::Inputs {
            fg_keydown: Some(&prev),
            fg_keyup: &now,
            is_secure: false,
            user_overrides: &overrides,
        });
        assert_eq!(d, pipeline::Decision::Proceed(InjectionStrategy::Keystroke));
    }

    // -----------------------------------------------------------------
    // format_secs_as_iso — pure date math
    // -----------------------------------------------------------------

    #[test]
    fn iso_at_unix_epoch_is_1970_01_01() {
        assert_eq!(format_secs_as_iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso_handles_known_timestamp() {
        // 2026-05-17T00:00:00Z = 1_778_976_000 seconds since epoch.
        // Derived from 2024-01-01 (known: 1_704_067_200) + 867 days
        // (731 across 2024-2025 + 136 days Jan 1 → May 17 in 2026).
        assert_eq!(format_secs_as_iso(1_778_976_000), "2026-05-17T00:00:00Z");
    }

    #[test]
    fn iso_handles_leap_year_feb_29() {
        // 2024-02-29T00:00:00Z = 1_709_164_800 seconds since epoch.
        assert_eq!(format_secs_as_iso(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn iso_handles_time_of_day() {
        // 1970-01-01T12:34:56Z = 45_296 seconds.
        assert_eq!(format_secs_as_iso(45_296), "1970-01-01T12:34:56Z");
    }
}
