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
use std::sync::atomic::{AtomicBool, Ordering};
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
use crate::settings::{model::SettingKey, Settings};

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
    ///
    /// **mb-fc1 hotfix:** the chord default flipped from
    /// `RCtrl + M` to `RCtrl + .` (`VK_OEM_PERIOD`) because the
    /// original `M` collided with Microsoft 365 Copilot on Windows 11.
    pub fn defaults_with(chunk_base_dir: PathBuf) -> Self {
        Self {
            modifier_vk: 0xA3, // VK_RCONTROL
            main_vk: 0xBE,     // VK_OEM_PERIOD
            max_duration_seconds: 14_400,
            default_source: LastChosenSource::Mic,
            chunk_base_dir,
            whisper_model_id: "whisper-large-v3-turbo-q5_0".to_string(),
            formatter_version: "mc-v1".to_string(),
            hotkey_label: "RCtrl+.".to_string(),
        }
    }

    /// Hydrate from the live settings DB. Reads
    /// `MeetingHotkeyModifier` + `MeetingHotkeyKey` +
    /// `MeetingMaxDurationSeconds` + `MeetingDefaultSource` and
    /// overlays them onto [`defaults_with`].
    ///
    /// **Failure model:** any single-setting parse error logs a
    /// `tracing::warn!` and falls back to the documented default for
    /// that key. The runtime spawn must NOT fail just because the
    /// user hand-edited the DB into an inconsistent state — better to
    /// boot with the safe default and surface the corruption in the
    /// settings UI than to refuse to start.
    ///
    /// Audit-trail context (mb-fc1, 2026-05-23): before this method
    /// existed, `defaults_with` was called unconditionally and the
    /// settings DB rows for the chord were dead writes — the
    /// Settings → Meetings tab picker has been shipped since Wave 5
    /// but never had teeth.
    pub fn from_settings(conn: &Connection, chunk_base_dir: PathBuf) -> Self {
        use super::vk_names::vk_name_to_code;
        // One-shot data hotfix: pre-mb-fc1 installs persisted
        // `"VK_M"` as the chord default. That key collides with the
        // Microsoft 365 Copilot global chord on Windows 11. If the
        // stored value is the literal old default *and* this DB has
        // never been migrated, upgrade it once to the new safe
        // default. Users who genuinely picked `VK_M` after the hotfix
        // ships are protected: we set the marker row regardless, so
        // we never re-migrate the same DB twice. If they re-pick
        // `VK_M` post-hotfix, the marker is already present and we
        // leave their choice alone.
        upgrade_legacy_chord_default_once(conn);

        let mut cfg = Self::defaults_with(chunk_base_dir);
        let s = Settings::new(conn);

        match s.get::<String>(SettingKey::MeetingHotkeyModifier) {
            Ok(name) => match vk_name_to_code(&name) {
                Ok(code) => cfg.modifier_vk = code,
                Err(e) => tracing::warn!(
                    target: "meetings",
                    error = %e,
                    name = %name,
                    "MeetingHotkeyModifier parse failed; using default"
                ),
            },
            Err(e) => tracing::warn!(
                target: "meetings",
                error = %e,
                "MeetingHotkeyModifier read failed; using default"
            ),
        }
        match s.get::<String>(SettingKey::MeetingHotkeyKey) {
            Ok(name) => match vk_name_to_code(&name) {
                Ok(code) => cfg.main_vk = code,
                Err(e) => tracing::warn!(
                    target: "meetings",
                    error = %e,
                    name = %name,
                    "MeetingHotkeyKey parse failed; using default"
                ),
            },
            Err(e) => tracing::warn!(
                target: "meetings",
                error = %e,
                "MeetingHotkeyKey read failed; using default"
            ),
        }
        if let Ok(max) = s.get::<i64>(SettingKey::MeetingMaxDurationSeconds) {
            // Mirror the server-side clamp documented in the settings
            // facade: [60, 21600]. The settings layer is supposed to
            // enforce this on write, but a hand-edit could land an
            // out-of-range value here. Clamp defensively.
            cfg.max_duration_seconds = max.clamp(60, 21_600) as u32;
        }
        if let Ok(src) = s.get::<String>(SettingKey::MeetingDefaultSource) {
            cfg.default_source = match src.as_str() {
                "mic" => LastChosenSource::Mic,
                "system" => LastChosenSource::System,
                "both" => LastChosenSource::Both,
                other => {
                    tracing::warn!(
                        target: "meetings",
                        value = %other,
                        "MeetingDefaultSource unknown; using Mic"
                    );
                    LastChosenSource::Mic
                }
            };
        }
        // Refresh the human-readable hotkey label from the resolved
        // codes so logs + provenance rows match the live chord.
        cfg.hotkey_label = format_hotkey_label(cfg.modifier_vk, cfg.main_vk);
        cfg
    }
}

/// Render a `MOD+KEY` label string from a pair of VK codes. Used
/// for `tracing` logs and the `meeting_sessions.hotkey_pressed`
/// provenance column. Pure: no DB, no allocation surprises.
pub fn format_hotkey_label(modifier_vk: u32, main_vk: u32) -> String {
    use super::vk_names::vk_code_to_name;
    let m = vk_code_to_name(modifier_vk)
        .map(short_label)
        .unwrap_or_else(|| format!("VK_{modifier_vk:#X}"));
    let k = vk_code_to_name(main_vk)
        .map(short_label)
        .unwrap_or_else(|| format!("VK_{main_vk:#X}"));
    format!("{m}+{k}")
}

/// Marker key written to the raw `settings` table once we've done the
/// mb-fc1 chord-default upgrade. Lives outside the typed
/// [`SettingKey`] enum on purpose: it's bookkeeping, not user-facing
/// configuration, and we don't want it appearing in settings UIs or
/// reset-to-defaults flows.
const CHORD_HOTFIX_MARKER_KEY: &str = "_internal_mc_chord_copilot_hotfix_v1";

/// One-shot bookkeeping: upgrade `meeting_hotkey_key = "VK_M"` to
/// `"VK_OEM_PERIOD"` on DBs that pre-date the mb-fc1 hotfix.
///
/// Behaviour matrix:
///
/// | marker present | row value      | action                          |
/// |----------------|----------------|---------------------------------|
/// | yes            | (any)          | no-op                           |
/// | no             | absent         | write marker, leave key absent  |
/// | no             | `"VK_M"`       | overwrite to `"VK_OEM_PERIOD"`, write marker |
/// | no             | anything else  | write marker, leave row alone   |
///
/// All DB ops are best-effort: a failure here just means we'll try
/// again next launch. We don't want a stale settings row to brick
/// meeting capture entirely — that's worse than the original bug.
pub(crate) fn upgrade_legacy_chord_default_once(conn: &Connection) {
    let already_done: rusqlite::Result<i64> = conn.query_row(
        "SELECT 1 FROM settings WHERE key = ?1",
        rusqlite::params![CHORD_HOTFIX_MARKER_KEY],
        |r| r.get(0),
    );
    if already_done.is_ok() {
        return;
    }

    // If the meeting_hotkey_key row exists *and* equals the old
    // default JSON literal, upgrade it. The settings facade JSON-
    // encodes strings, so "VK_M" lands as the literal four-byte
    // string `"VK_M"` (including the surrounding quotes).
    let stored: rusqlite::Result<String> = conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params!["meeting_hotkey_key"],
        |r| r.get(0),
    );
    if let Ok(v) = stored {
        if v == "\"VK_M\"" {
            let _ = conn.execute(
                "UPDATE settings SET value = ?1 WHERE key = ?2",
                rusqlite::params!["\"VK_OEM_PERIOD\"", "meeting_hotkey_key"],
            );
            tracing::info!(
                target: "meetings",
                "upgraded legacy meeting chord default VK_M → VK_OEM_PERIOD (mb-fc1)"
            );
        }
    }

    // Write the marker regardless so we don't keep checking on every
    // launch.
    let _ = conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![CHORD_HOTFIX_MARKER_KEY, "\"applied\""],
    );
}

fn short_label(vk_name: &str) -> String {
    // Pretty-print the common VK names for log readability. Anything
    // not in this table falls through to the raw `VK_*` string —
    // perfectly fine for a log line.
    match vk_name {
        "VK_RCONTROL" => "RCtrl".into(),
        "VK_LCONTROL" => "LCtrl".into(),
        "VK_RMENU" => "RAlt".into(),
        "VK_LMENU" => "LAlt".into(),
        "VK_RSHIFT" => "RShift".into(),
        "VK_LSHIFT" => "LShift".into(),
        "VK_RWIN" => "RWin".into(),
        "VK_LWIN" => "LWin".into(),
        "VK_OEM_PERIOD" => ".".into(),
        "VK_OEM_COMMA" => ",".into(),
        "VK_OEM_1" => ";".into(),
        "VK_OEM_2" => "/".into(),
        "VK_OEM_3" => "`".into(),
        "VK_OEM_5" => "\\".into(),
        "VK_OEM_6" => "]".into(),
        "VK_OEM_MINUS" => "-".into(),
        "VK_OEM_PLUS" => "=".into(),
        "VK_SPACE" => "Space".into(),
        other => {
            // Strip the VK_ prefix for single-letter / F-key / digit
            // cases so we get "M", "F13", "1" instead of "VK_M" etc.
            other.strip_prefix("VK_").unwrap_or(other).into()
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
    /// ADR 0032 / mb-nig: tick-emitter thread that publishes
    /// `meeting:tick { elapsedMs, micDb, sysDb }` to the overlay
    /// every ~250ms. Cleared by `finalize_meeting` (sets `running`
    /// to false, then `join()`s).
    pub tick_running: Arc<std::sync::atomic::AtomicBool>,
    pub tick_thread: Option<JoinHandle<()>>,
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
    /// A clone of the activation event sender, retained so external
    /// callers (the tray menu, the settings IPC) can inject
    /// [`ActivationEvent::PauseToggle`] events into the same channel
    /// the OS hook feeds. Mirrors `hotkey::pause::Pause` for dictation.
    act_tx: Sender<ActivationEvent>,
    /// Fast-read cache of the paused state. The Tauri command layer
    /// reads this in the IPC `meeting_is_paused` path and the tray
    /// menu-build path; settings DB is the source of truth across
    /// restarts but the in-memory cell is the source of truth
    /// within a single run.
    is_paused: Arc<AtomicBool>,
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
        // Clone the sender BEFORE handing it to the installer so we
        // keep an injection path for PauseToggle events.
        let act_tx_for_injection = act_tx.clone();
        let hotkey = MeetingHotkeyInstaller::install(chord, act_tx)
            .map_err(|e| AppError::MeetingCapture(format!("install meeting hotkey: {e}")))?;

        let shared_for_thread = shared.clone();
        let activation_thread = thread::Builder::new()
            .name("mockingbird-meeting-activation".into())
            .spawn(move || activation_loop(shared_for_thread, act_rx, stop_rx))
            .map_err(|e| AppError::MeetingCapture(format!("spawn activation thread: {e}")))?;

        // Hydrate the paused-state cache from the settings DB. If the
        // setting was true at last shutdown, inject a PauseToggle so
        // the activation thread starts in the paused state. Errors
        // here are non-fatal: a setting-read failure shouldn't block
        // app startup — the hotkey just defaults to unpaused.
        let is_paused = Arc::new(AtomicBool::new(false));
        match Self::read_paused_setting(&shared.shared_conn) {
            Ok(true) => {
                is_paused.store(true, Ordering::SeqCst);
                // Best-effort: the receiver is wired up above, so this
                // send queues immediately into the activation loop.
                let _ = act_tx_for_injection.send(ActivationEvent::PauseToggle { paused: true });
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(
                target: "meetings",
                error = %e,
                "failed to read MeetingHotkeyPaused on spawn; defaulting to unpaused"
            ),
        }

        Ok(Self {
            shared,
            hotkey: Some(hotkey),
            activation_thread: Some(activation_thread),
            activation_stop_tx: Some(stop_tx),
            act_tx: act_tx_for_injection,
            is_paused,
        })
    }

    /// Idempotent paused-toggle. Updates the in-memory cache, persists
    /// to settings, and injects an `ActivationEvent::PauseToggle` so
    /// the activation state machine moves to/from its paused state.
    ///
    /// Errors only on settings-write failure (the in-memory + channel
    /// updates are infallible). On error, the in-memory state IS
    /// already updated — the caller's view-of-the-world matches the
    /// runtime even if the settings DB write failed.
    pub fn set_meeting_hotkey_paused(&self, paused: bool) -> AppResult<()> {
        self.is_paused.store(paused, Ordering::SeqCst);
        // Send first so the activation thread reacts ASAP; a settings
        // write failure shouldn't delay the toggle the user just
        // clicked. If the channel is closed (Drop in flight) the send
        // errors and we swallow — nothing for us to do.
        let _ = self.act_tx.send(ActivationEvent::PauseToggle { paused });
        let conn = self
            .shared
            .shared_conn
            .lock()
            .map_err(|_| AppError::MeetingCapture("shared_conn mutex poisoned".into()))?;
        Settings::new(&conn).set(SettingKey::MeetingHotkeyPaused, &paused)
    }

    /// Lock-free read of the in-memory paused cache. The tray
    /// menu-build path + the IPC `meeting_is_paused` command both go
    /// through this.
    pub fn is_meeting_hotkey_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    fn read_paused_setting(shared_conn: &Arc<Mutex<Connection>>) -> AppResult<bool> {
        let conn = shared_conn
            .lock()
            .map_err(|_| AppError::MeetingCapture("shared_conn mutex poisoned".into()))?;
        Settings::new(&conn).get::<bool>(SettingKey::MeetingHotkeyPaused)
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

    pub fn cancel_meeting(&self, uuid: &str) -> AppResult<()> {
        self.shared.cancel_meeting(uuid)
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
        // Push-to-stop — second chord-press during an active meeting
        // ends it. We do NOT show the overlay here; the lifecycle's
        // `done` event will let the React side self-hide whatever it
        // was showing.
        if let Err(e) = shared.stop_meeting(&uuid) {
            tracing::warn!(target: "meetings", error = %e, "stop_meeting from toggle");
        }
        return;
    }

    // Push-to-start — show the overlay window first so the user can
    // confirm / change the source picker, then the React side fires
    // `meeting_start` when the user clicks the big Start button. We
    // do NOT auto-start the capture here; that would surprise the
    // user when the wrong source is preselected.
    //
    // Wave 5 deviation from §5.6 of the wave brief: the brief
    // proposed showing the overlay + auto-starting. The auto-start
    // is what makes the chord a "toggle" rather than a "confirm-
    // first" affordance — picking it is a UX call. We go with
    // overlay-first for two reasons: (a) the source picker is the
    // primary value-add of having an overlay at all, and (b) it
    // matches the main-window record bar's flow (pick → start). If
    // user testing shows the extra click is annoying we can add a
    // "skip picker, use last source" setting in Wave 6 polish.
    let _ = source; // last-source becomes the picker's preselect via
                    // the existing LastChosenSource hydration path.
    if !crate::meetings::overlay::show_overlay(&shared.app_handle) {
        // Overlay missing — fall back to direct start so the chord
        // still does something. Source is whatever the activation
        // state machine last remembered.
        tracing::warn!(
            target: "meetings",
            "meeting overlay window missing; falling back to direct start"
        );
        let mc_source = match source {
            LastChosenSource::Mic => MeetingSource::Mic,
            LastChosenSource::System => MeetingSource::System,
            LastChosenSource::Both => MeetingSource::Both,
        };
        if let Err(e) = shared.start_meeting(mc_source) {
            tracing::warn!(target: "meetings", error = %e, "start_meeting fallback from toggle");
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
        // mb-fc1 hotfix: main_vk default flipped from VK_M (0x4D) to
        // VK_OEM_PERIOD (0xBE) due to Microsoft 365 Copilot chord
        // collision on Windows 11.
        assert_eq!(cfg.main_vk, 0xBE);
    }

    #[test]
    fn defaults_with_uses_canonical_provenance_strings() {
        let cfg = MeetingRuntimeConfig::defaults_with(PathBuf::from("."));
        assert_eq!(cfg.formatter_version, "mc-v1");
        // Provenance label tracks the new chord default. Existing
        // sessions in older DBs still carry "RCtrl+M" — that's fine,
        // it's historical provenance, immutable by Principle 1.
        assert_eq!(cfg.hotkey_label, "RCtrl+.");
        assert!(cfg.whisper_model_id.contains("whisper"));
    }

    #[test]
    fn format_hotkey_label_pretty_prints_common_chords() {
        assert_eq!(format_hotkey_label(0xA3, 0xBE), "RCtrl+.");
        assert_eq!(format_hotkey_label(0xA3, 0x4D), "RCtrl+M");
        assert_eq!(format_hotkey_label(0xA5, 0x7C), "RAlt+F13");
        assert_eq!(format_hotkey_label(0xA2, 0xBC), "LCtrl+,");
    }

    #[test]
    fn format_hotkey_label_falls_back_to_hex_for_unknowns() {
        // 0x99 isn't in the supported VK table — we still emit
        // something useful for the log line rather than panicking.
        let s = format_hotkey_label(0xA3, 0x99);
        assert!(s.starts_with("RCtrl+"), "got: {s}");
        assert!(s.contains("99") || s.contains("0x99"), "got: {s}");
    }

    #[test]
    fn from_settings_returns_defaults_on_fresh_db() {
        // No settings rows written yet ⇒ every read falls through to
        // the documented default, and `from_settings` should match
        // `defaults_with` exactly.
        let db = crate::db::Database::open_in_memory().expect("open db");
        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        let baseline = MeetingRuntimeConfig::defaults_with(PathBuf::from("."));
        assert_eq!(cfg.modifier_vk, baseline.modifier_vk);
        assert_eq!(cfg.main_vk, baseline.main_vk);
        assert_eq!(cfg.hotkey_label, baseline.hotkey_label);
    }

    #[test]
    fn from_settings_picks_up_user_customised_chord() {
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        s.set(SettingKey::MeetingHotkeyModifier, &"VK_RMENU".to_string())
            .unwrap();
        s.set(SettingKey::MeetingHotkeyKey, &"VK_F13".to_string())
            .unwrap();
        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        assert_eq!(cfg.modifier_vk, 0xA5); // RAlt
        assert_eq!(cfg.main_vk, 0x7C); // F13
        assert_eq!(cfg.hotkey_label, "RAlt+F13");
    }

    #[test]
    fn from_settings_falls_back_to_default_on_bad_vk_name() {
        // Hand-edited DB rot — the parser fails, we fall back rather
        // than refusing to boot the meeting subsystem.
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        s.set(
            SettingKey::MeetingHotkeyKey,
            &"VK_NOT_A_REAL_KEY".to_string(),
        )
        .unwrap();
        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        // Default main_vk still wins.
        assert_eq!(cfg.main_vk, 0xBE);
    }

    #[test]
    fn from_settings_clamps_max_duration_to_documented_range() {
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        // Use set_raw so we bypass any client-side clamp and prove the
        // server-side defence is in place.
        s.set_raw(
            SettingKey::MeetingMaxDurationSeconds,
            &serde_json::json!(99_999_999_i64),
        )
        .unwrap();
        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        assert_eq!(cfg.max_duration_seconds, 21_600);
    }

    #[test]
    fn legacy_vk_m_chord_is_upgraded_once_on_first_load() {
        // Simulate the pre-mb-fc1 state: the meeting_hotkey_key row
        // holds the old default `"VK_M"`.
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        s.set(SettingKey::MeetingHotkeyKey, &"VK_M".to_string())
            .unwrap();

        // First load: should upgrade to VK_OEM_PERIOD.
        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        assert_eq!(cfg.main_vk, 0xBE);
        let post: String = s.get(SettingKey::MeetingHotkeyKey).unwrap();
        assert_eq!(post, "VK_OEM_PERIOD");
    }

    #[test]
    fn legacy_chord_upgrade_is_idempotent_and_respects_user_re_pick() {
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        s.set(SettingKey::MeetingHotkeyKey, &"VK_M".to_string())
            .unwrap();

        // First load triggers the migration.
        let _ = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));

        // User deliberately re-picks VK_M after the migration. The
        // marker row is already present, so we leave their choice
        // alone on the next load.
        s.set(SettingKey::MeetingHotkeyKey, &"VK_M".to_string())
            .unwrap();
        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        assert_eq!(cfg.main_vk, 0x4D, "second load must respect user re-pick");
        let post: String = s.get(SettingKey::MeetingHotkeyKey).unwrap();
        assert_eq!(post, "VK_M");
    }

    #[test]
    fn legacy_chord_upgrade_leaves_custom_user_keys_alone() {
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        s.set(SettingKey::MeetingHotkeyKey, &"VK_F13".to_string())
            .unwrap();

        let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
        assert_eq!(cfg.main_vk, 0x7C);
        let post: String = s.get(SettingKey::MeetingHotkeyKey).unwrap();
        assert_eq!(post, "VK_F13");
    }

    #[test]
    fn from_settings_maps_default_source_strings() {
        let db = crate::db::Database::open_in_memory().expect("open db");
        let s = Settings::new(&db.conn);
        for (raw, expected) in [
            ("mic", LastChosenSource::Mic),
            ("system", LastChosenSource::System),
            ("both", LastChosenSource::Both),
        ] {
            s.set(SettingKey::MeetingDefaultSource, &raw.to_string())
                .unwrap();
            let cfg = MeetingRuntimeConfig::from_settings(&db.conn, PathBuf::from("."));
            assert_eq!(cfg.default_source, expected, "source={raw}");
        }
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
