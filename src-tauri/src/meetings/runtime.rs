//! Meeting capture runtime — lifecycle owner.
//!
//! `MeetingCaptureRuntime` is the long-lived object held in Tauri's
//! `manage(...)` registry. It owns:
//!   - the meetings message-pump thread (via [`MeetingHotkeyInstaller`])
//!     that installs the second `WH_KEYBOARD_LL` hook;
//!   - a dedicated activation thread that drives the
//!     [`Activation`] state machine off the hotkey channel and
//!     translates `MeetingToggle` actions into start/stop calls;
//!   - the in-flight meeting bag (`Arc<Mutex<Option<InFlightMeeting>>>`)
//!     — at most one meeting is active at a time;
//!   - the LLM-pass response cache (IDs → result text) — populated by
//!     the `meeting_run_llm_pass` IPC command, drained at copy-export
//!     time so the LLM output can land in the rendered Markdown
//!     **without ever being persisted to the DB**
//!     (judge `mc-no-llm-in-critical-path`).
//!
//! ## Critical-path invariant
//!
//! The recording-to-canonical-transcript path is
//! `meeting_stop → TwinStreamCapture::stop → long-form-stt join →
//! formatter::format → merge_two_channels → persist_meeting`. The
//! lifecycle methods live in [`super::lifecycle`]; that module's
//! docs restate the binding "no LLM in this path" invariant. The
//! optional LLM pass is reached only via the `meeting_run_llm_pass`
//! IPC command.
//!
//! Wave 4 §4.2 of `phase-mc-wave4-brief.md` is the binding spec.
//!
//! ## File-split note
//!
//! Wave 4 split this module: the struct definitions + activation
//! thread + `Drop` impl live here; the
//! `start_meeting`/`stop_meeting`/`finalize_in_flight_as_interrupted`
//! methods live in [`super::lifecycle`] to keep both files under the
//! 600-line cap mandated by AGENTS.md.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use rusqlite::Connection;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::meetings::activation::{
    Activation, ActivationAction, ActivationEvent, LastChosenSource,
};
use crate::meetings::capture::{MeetingSource, TwinStreamCapture};
use crate::meetings::hotkey_installer::{ChordConfig, MeetingHotkeyInstaller};
use crate::meetings::long_form_stt::LongFormOutput;

// --------------------------------------------------------------------
// Public types
// --------------------------------------------------------------------

/// Configuration captured at runtime spawn. Read once from the
/// `settings` table; the runtime does NOT re-read on every toggle
/// (settings changes take effect on app restart, mirroring the
/// dictation runtime's behaviour).
#[derive(Debug, Clone)]
pub struct MeetingRuntimeConfig {
    pub modifier_vk: u32,
    pub main_vk: u32,
    pub max_duration_seconds: u32,
    pub default_source: LastChosenSource,
    /// Sticky base dir for chunk wavs; a per-meeting subdir is
    /// created under this. Typically `<app_data>/meetings/`.
    pub chunk_base_dir: PathBuf,
    /// Whisper model id for provenance on persisted rows.
    pub whisper_model_id: String,
    /// Formatter version tag for provenance (e.g. `"mc-v1"`).
    pub formatter_version: String,
    /// Hotkey description for the `meeting_sessions.hotkey_pressed`
    /// provenance column (e.g. `"RCtrl+M"`).
    pub hotkey_label: String,
}

impl MeetingRuntimeConfig {
    /// Best-effort defaults — used by tests + as the fallback when
    /// settings haven't been populated yet.
    pub fn defaults_with(chunk_base_dir: PathBuf) -> Self {
        Self {
            modifier_vk: 0xA3, // VK_RCONTROL
            main_vk: 0x4D,     // 'M'
            max_duration_seconds: 14_400,
            default_source: LastChosenSource::Mic,
            chunk_base_dir,
            whisper_model_id: "whisper-large-v3-turbo-q5_0".to_string(),
            formatter_version: "mc-v1".to_string(),
            hotkey_label: "RCtrl+M".to_string(),
        }
    }
}

/// Shared inner-state bag — cheaply clonable into the activation
/// thread + the IPC command handlers. The non-clonable hot-key
/// installer + thread handles stay on the outer
/// [`MeetingCaptureRuntime`].
#[derive(Clone)]
pub struct MeetingRuntimeShared {
    pub(crate) shared_conn: Arc<Mutex<Connection>>,
    pub(crate) config: MeetingRuntimeConfig,
    pub(crate) app_handle: AppHandle,
    pub(crate) in_flight: Arc<Mutex<Option<InFlightMeeting>>>,
    pub(crate) llm_pass_cache: Arc<Mutex<HashMap<String, String>>>,
}

/// One live meeting. Held under `Mutex<Option<…>>` so `start` /
/// `stop` can take exclusive access while still allowing read-only
/// observers (e.g. an IPC `meeting_status` query, if added later)
/// to peek via `.lock()`.
///
/// Fields are `pub(crate)` so the lifecycle module can destructure
/// without going through accessors.
pub(crate) struct InFlightMeeting {
    pub uuid: String,
    pub started_at_iso: String,
    pub started_at_instant: Instant,
    pub source: MeetingSource,
    pub capture: TwinStreamCapture,
    pub long_form_thread: JoinHandle<AppResult<LongFormOutput>>,
    pub chunk_dir: PathBuf,
}

/// Long-lived owner of the meeting-capture subsystem.
///
/// Spawned once at app startup from `lib.rs::run`'s `.setup(...)`
/// callback. Drop tears down the meetings hotkey + activation
/// thread + any in-flight capture (best-effort, with the in-flight
/// session persisted as `MeetingStatus::Interrupted`).
pub struct MeetingCaptureRuntime {
    shared: MeetingRuntimeShared,
    hotkey: Option<MeetingHotkeyInstaller>,
    activation_thread: Option<JoinHandle<()>>,
    /// Held purely so dropping it signals the activation loop to
    /// exit (the loop selects between the hotkey rx and this stop
    /// rx; both being disconnected ends the loop).
    activation_stop_tx: Option<Sender<()>>,
}

impl MeetingCaptureRuntime {
    /// Wire the hotkey + spawn the activation thread.
    pub fn spawn(
        shared_conn: Arc<Mutex<Connection>>,
        config: MeetingRuntimeConfig,
        app_handle: AppHandle,
    ) -> AppResult<Self> {
        let shared = MeetingRuntimeShared {
            shared_conn,
            config: config.clone(),
            app_handle,
            in_flight: Arc::new(Mutex::new(None)),
            llm_pass_cache: Arc::new(Mutex::new(HashMap::new())),
        };

        let (act_tx, act_rx) = mpsc::channel::<ActivationEvent>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let chord = ChordConfig {
            modifier_vk: config.modifier_vk,
            main_vk: config.main_vk,
        };
        let hotkey = MeetingHotkeyInstaller::install(chord, act_tx)
            .map_err(|e| AppError::MeetingCapture(format!("install meeting hotkey: {e}")))?;

        let shared_for_thread = shared.clone();
        let activation_thread = thread::Builder::new()
            .name("mockingbird-meeting-activation".into())
            .spawn(move || activation_loop(shared_for_thread, act_rx, stop_rx))
            .map_err(|e| AppError::MeetingCapture(format!("spawn activation thread: {e}")))?;

        Ok(Self {
            shared,
            hotkey: Some(hotkey),
            activation_thread: Some(activation_thread),
            activation_stop_tx: Some(stop_tx),
        })
    }

    /// Cheap snapshot of the shared bag — for the IPC command layer
    /// to operate on the runtime without holding the outer reference.
    pub fn shared(&self) -> MeetingRuntimeShared {
        self.shared.clone()
    }

    /// Idempotent. If already in-flight, returns the existing uuid
    /// and emits a `warn-already-running` overlay state.
    pub fn start_meeting(&self, source: MeetingSource) -> AppResult<String> {
        self.shared.start_meeting(source)
    }

    pub fn stop_meeting(&self, uuid: &str) -> AppResult<()> {
        self.shared.stop_meeting(uuid)
    }

    pub fn llm_pass_cache(&self) -> Arc<Mutex<HashMap<String, String>>> {
        self.shared.llm_pass_cache.clone()
    }
}

impl Drop for MeetingCaptureRuntime {
    fn drop(&mut self) {
        // Best-effort tear-down. Errors are tracing-logged; Drop must
        // not panic. Order matters: stop the hotkey first so no new
        // activation events arrive, then signal + join the activation
        // thread, then finalize any in-flight meeting.
        if let Some(hk) = self.hotkey.take() {
            if let Err(e) = hk.stop() {
                tracing::warn!(target: "meetings", error = %e, "meeting hotkey stop on Drop");
            }
        }
        drop(self.activation_stop_tx.take());
        if let Some(jh) = self.activation_thread.take() {
            if let Err(e) = jh.join() {
                tracing::warn!(
                    target: "meetings",
                    "meeting activation thread panicked: {e:?}"
                );
            }
        }
        // Finalize any in-flight meeting as Interrupted. Don't block
        // forever — `capture.stop()` already has its own internal
        // timeouts via the chunker thread polling.
        if let Err(e) = self.shared.finalize_in_flight_as_interrupted() {
            tracing::warn!(
                target: "meetings",
                error = %e,
                "finalize-on-drop failed; meeting status may not be persisted"
            );
        }
    }
}

// --------------------------------------------------------------------
// Activation loop — runs on its own thread; calls into
// [`super::lifecycle`]'s start/stop methods on each `MeetingToggle`.
// --------------------------------------------------------------------

/// Drive the chord state machine until the activation channel
/// disconnects (hotkey stopped) or the stop signal fires. On each
/// `MeetingToggle`, consult `in_flight` to decide start-vs-stop.
fn activation_loop(
    shared: MeetingRuntimeShared,
    act_rx: mpsc::Receiver<ActivationEvent>,
    stop_rx: mpsc::Receiver<()>,
) {
    let mut activation = Activation::new(shared.config.default_source);
    loop {
        // Disconnect on either channel ends the loop. We poll the
        // stop_rx with try_recv on each iteration and block on the
        // act_rx — the hotkey installer's drop closes act_rx, so we
        // unblock cleanly on shutdown.
        if matches!(
            stop_rx.try_recv(),
            Ok(()) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            tracing::debug!(target: "meetings", "activation loop: stop signal");
            break;
        }
        let event = match act_rx.recv() {
            Ok(e) => e,
            Err(_) => {
                tracing::debug!(target: "meetings", "activation loop: hotkey channel closed");
                break;
            }
        };
        let action = match activation.on_event(event) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(target: "meetings", error = %e, "activation on_event error");
                ActivationAction::Noop
            }
        };
        if let ActivationAction::MeetingToggle { source } = action {
            handle_toggle(&shared, source);
        }
    }
}

fn handle_toggle(shared: &MeetingRuntimeShared, source: LastChosenSource) {
    let in_flight_uuid = {
        let guard = shared.in_flight.lock().expect("in_flight mutex poisoned");
        guard.as_ref().map(|m| m.uuid.clone())
    };
    if let Some(uuid) = in_flight_uuid {
        if let Err(e) = shared.stop_meeting(&uuid) {
            tracing::warn!(target: "meetings", error = %e, "stop_meeting from toggle");
        }
    } else {
        let mc_source = match source {
            LastChosenSource::Mic => MeetingSource::Mic,
            LastChosenSource::System => MeetingSource::System,
            LastChosenSource::Both => MeetingSource::Both,
        };
        if let Err(e) = shared.start_meeting(mc_source) {
            tracing::warn!(target: "meetings", error = %e, "start_meeting from toggle");
        }
    }
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke: defaults-with constructor round-trips its argument.
    #[test]
    fn defaults_with_sets_chunk_base_dir() {
        let p = PathBuf::from("/tmp/meetings-test");
        let cfg = MeetingRuntimeConfig::defaults_with(p.clone());
        assert_eq!(cfg.chunk_base_dir, p);
        assert_eq!(cfg.modifier_vk, 0xA3);
        assert_eq!(cfg.main_vk, 0x4D);
        assert_eq!(cfg.main_vk, b'M' as u32);
    }

    #[test]
    fn defaults_with_uses_canonical_provenance_strings() {
        let cfg = MeetingRuntimeConfig::defaults_with(PathBuf::from("."));
        assert_eq!(cfg.formatter_version, "mc-v1");
        assert_eq!(cfg.hotkey_label, "RCtrl+M");
        assert!(cfg.whisper_model_id.contains("whisper"));
    }

    // The full spawn-then-drop test requires `tauri::test::mock_app()`
    // and a live keyboard hook (real OS hook = unreliable in headless
    // CI). Gated `#[ignore]` per `phase-mc-wave4-brief.md` §4.2's
    // explicit escape hatch: "If `tauri::test::mock_app()` isn't
    // ergonomic enough … gate these `#[ignore]` and document why".
    //
    // The pure-logic surface (activation state machine, formatter,
    // chunker, merge, lifecycle helpers) is fully covered by ~80
    // tests across the sibling modules. Runtime glue is exercised
    // end-to-end during the Wave 4 hands-on QA matrix (mb-pdv.18).
    #[test]
    #[ignore]
    #[cfg(target_os = "windows")]
    fn runtime_spawn_then_drop_is_clean() {
        // tauri::test::mock_app() doesn't satisfy `Emitter::emit` for
        // our event payloads; production code path exercised via the
        // QA matrix.
    }

    #[test]
    #[ignore]
    #[cfg(target_os = "windows")]
    fn runtime_drop_during_capture_marks_interrupted() {}
}
