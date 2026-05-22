//! Command Center subsystem — orchestrator + public API.
//!
//! ADR 0037; phase doc `docs/phases/phase10.md` Wave 1A.
//!
//! The orchestrator is a thin shell over [`state::apply`]. The state
//! machine is pure; the orchestrator is the IO boundary that:
//!
//! - Owns the Tauri webview window labeled `command_center`.
//! - Owns the [`hotkey::CommandCenterHotkeyInstaller`] (the 4th LL
//!   keyboard hook in the app).
//! - Subscribes to `dictation:state` and `meeting:state` events to
//!   track which subsystem (if any) is currently recording.
//! - Dispatches mode picks: Meeting kicks off the existing
//!   [`crate::meetings::runtime::MeetingRuntimeShared::start_meeting`];
//!   Dictation is push-to-talk and has no programmatic start (the
//!   Dictation tile's UX is purely the "or just hold Right Alt" hint
//!   per ADR 0037 §4); Activity logs + no-ops until Wave 1B lands.
//!
//! ## Docs policy
//!
//! `missing_docs` is silenced module-wide here: every public surface
//! is described in either ADR 0037 or the doc comments on the
//! module-level prose blocks. Adding `///` lines to every variant /
//! field would be noise without adding signal — the ADR is the
//! contract.
//!
//! ## No recording logic here
//!
//! Per the brief: "No recording logic — purely a dispatcher to the
//! existing Dictation / Meeting / (Wave 1B) Activity runtimes." This
//! file MUST NOT grow audio / VAD / DB code. If you find yourself
//! reaching for `whisper-rs` or `rusqlite::Connection` in here, the
//! logic belongs in the subsystem you're trying to drive.

#![allow(missing_docs)]

pub mod hotkey;
pub mod state;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::overlay_conventions;
use crate::settings::{model::SettingKey, Settings};

pub use hotkey::{CcActivation, CcChordConfig, CommandCenterHotkeyInstaller};
pub use state::{apply, CcEffect, CcInput, CcState, CurrentSession, RecordingKind, Transition};

/// Tauri webview label declared in `tauri.conf.json`. Mirrors the
/// recording + meeting overlay pattern.
pub const COMMAND_CENTER_WINDOW_LABEL: &str = "command_center";

/// Event the UI subscribes to. Carries the current state of the
/// command center as a serialized payload.
pub const STATE_EVENT: &str = "command_center:state";

/// Owns the live Command Center. Cheap to clone (Arc-backed).
///
/// Construct one at app boot via [`Self::spawn`] (Windows-only —
/// the hotkey hook is Windows-only in v1; non-Windows installs return
/// a Hotkey-error which `spawn` demotes to "tray-only entry").
#[derive(Clone)]
pub struct CommandCenter {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<CcState>,
    /// Most recently started session, used to populate the SessionCard
    /// when the user opens the CC mid-recording. Updated by the
    /// `dictation:state` and `meeting:state` listeners; cleared when
    /// either subsystem reports `done`/`aborted`/`idle`.
    current_session: Mutex<Option<RecordingKind>>,
    /// True until the user first dismisses the CC via any path.
    /// Mirror of `SettingKey::CommandCenterSeenV1` (inverted) at boot.
    first_run: AtomicBool,
    app: AppHandle,
    /// Optional handle so the orchestrator can dispatch meeting
    /// start/stop without re-resolving managed state every call.
    /// Pulled from `app.try_state::<MeetingRuntimeShared>()` at
    /// spawn time; `None` on non-Windows + when meeting capture
    /// failed to install (we still want the CC to render the picker).
    #[cfg(target_os = "windows")]
    meeting: Mutex<Option<crate::meetings::runtime::MeetingRuntimeShared>>,
    /// Hotkey installer kept alive for the process lifetime. Drop
    /// tears down the hook + joins the thread.
    _hotkey: Option<CommandCenterHotkeyInstaller>,
}

/// Payload shape mirrored on the TS side (`ui/src/lib/command_center.ts`).
#[derive(Clone, Debug, Serialize)]
struct CcStatePayload<'a> {
    /// One of `"closed" | "modePicker" | "sessionCard" | "launching"`.
    state: &'a str,
    /// Welcome variant header band visible? Only true on first-run
    /// picker.
    #[serde(rename = "firstRun")]
    first_run: bool,
    /// Present when `state == "sessionCard" || "launching"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
}

impl CommandCenter {
    /// Spin up the Command Center.
    ///
    /// - Reads `CommandCenterChord` + `CommandCenterSeenV1` from
    ///   settings.
    /// - Installs the WH_KEYBOARD_LL hook (Windows). On failure, logs
    ///   a WARN and continues without the chord — the tray menu
    ///   entry still works.
    /// - Returns a clone-cheap handle the caller should `app.manage()`
    ///   so it stays alive for the process lifetime.
    pub fn spawn(app: AppHandle, settings_conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        // Read settings once at boot. Defaults are conservative.
        let (chord, first_run) = {
            // Mutex unpoisoned-at-boot — lib.rs::run() runs us before
            // any other thread touches the connection.
            let guard = settings_conn
                .lock()
                .expect("settings conn must be unpoisoned at boot");
            let s = Settings::new(&guard);
            let chord_str: String = s
                .get(SettingKey::CommandCenterChord)
                .unwrap_or_else(|_| "RightCtrl+Space".into());
            let seen: bool = s.get(SettingKey::CommandCenterSeenV1).unwrap_or(false);
            let chord = parse_chord(&chord_str).unwrap_or_else(|| {
                tracing::warn!(
                    target: "command_center",
                    raw = %chord_str,
                    "could not parse command_center_chord; falling back to default"
                );
                CcChordConfig::default()
            });
            (chord, !seen)
        };

        // Try to install the hotkey. Non-fatal if it fails (Windows
        // refused the hook; another global tool has a conflicting
        // hook; etc.).
        #[allow(unused_mut)]
        let mut installer: Option<CommandCenterHotkeyInstaller> = None;
        #[cfg(target_os = "windows")]
        {
            let (tx, rx) = std::sync::mpsc::channel::<CcActivation>();
            match CommandCenterHotkeyInstaller::install(chord, tx) {
                Ok(inst) => {
                    installer = Some(inst);
                    // Spin a tiny relay thread so chord fires drive the
                    // orchestrator without needing the hook thread to
                    // know about Tauri.
                    let cc_for_relay = app.clone();
                    std::thread::Builder::new()
                        .name("cc-relay".into())
                        .spawn(move || {
                            while let Ok(ev) = rx.recv() {
                                if let Some(cc) = cc_for_relay.try_state::<CommandCenter>() {
                                    tracing::debug!(
                                        target: "command_center",
                                        ts_ms = ev.ts_ms,
                                        "chord activation",
                                    );
                                    cc.open_via_chord();
                                } else {
                                    tracing::warn!(
                                        target: "command_center",
                                        "chord activation but no CommandCenter in app state; dropping",
                                    );
                                }
                            }
                        })
                        .ok();
                }
                Err(e) => {
                    tracing::warn!(
                        target: "command_center",
                        error = %e,
                        "command_center hotkey install failed; tray entry remains usable"
                    );
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            tracing::info!(
                target: "command_center",
                "command_center hotkey is Windows-only; tray entry is the cross-platform path"
            );
        }

        let inner = Inner {
            state: Mutex::new(CcState::Closed),
            current_session: Mutex::new(None),
            first_run: AtomicBool::new(first_run),
            app: app.clone(),
            #[cfg(target_os = "windows")]
            meeting: Mutex::new(None),
            _hotkey: installer,
        };
        let me = Self {
            inner: Arc::new(inner),
        };

        // Subscribe to subsystem state events so the SessionCard
        // population stays accurate.
        wire_session_listeners(&app, me.clone());

        // First-run auto-open is a deliberate side effect of spawn().
        // The brief: "Boot path: if false, call
        // command_center::open_via_first_run() after main-window init."
        // We do it here — lib.rs::run() calls spawn() AFTER the main
        // window is built.
        if first_run {
            tracing::info!(
                target: "command_center",
                "first-run — auto-opening Command Center with Welcome variant"
            );
            me.open_via_first_run();
        }

        me
    }

    /// Late-binding: attach the MeetingRuntimeShared so the orchestrator
    /// can dispatch meeting start/stop. Called by `lib.rs::run()`
    /// after the meeting runtime spawns.
    #[cfg(target_os = "windows")]
    pub fn attach_meeting_runtime(&self, mc: crate::meetings::runtime::MeetingRuntimeShared) {
        if let Ok(mut g) = self.inner.meeting.lock() {
            *g = Some(mc);
        }
    }

    // ---------------- Public API surface ----------------

    pub fn open_via_chord(&self) {
        self.drive(CcInput::Open);
    }
    pub fn open_via_tray(&self) {
        self.drive(CcInput::Open);
    }
    pub fn open_via_first_run(&self) {
        self.drive(CcInput::Open);
    }
    pub fn dismiss(&self) {
        self.drive(CcInput::Dismiss);
    }
    pub fn pick_mode(&self, kind: RecordingKind) {
        self.drive(CcInput::PickMode { kind });
    }
    pub fn stop_active_session(&self) {
        self.drive(CcInput::Stop);
    }

    /// Currently-observed state snapshot — useful for tests + the IPC
    /// `cc_get_state` command.
    pub fn snapshot(&self) -> CcState {
        *self
            .inner
            .state
            .lock()
            .expect("cc state mutex must be unpoisoned")
    }

    /// Read of the most-recently-started session, populated by the
    /// `dictation:state` / `meeting:state` listeners.
    pub fn current_session(&self) -> CurrentSession {
        self.inner
            .current_session
            .lock()
            .map(|g| *g)
            .unwrap_or(None)
    }

    /// Set the current-session snapshot (called by the state-event
    /// listeners).
    pub(crate) fn set_current_session(&self, kind: Option<RecordingKind>) {
        if let Ok(mut g) = self.inner.current_session.lock() {
            // If a session just ended for the kind we're displaying,
            // emit a SessionEnded input so the FSM can flip back to
            // the picker without a separate event-loop.
            let was = *g;
            *g = kind;
            drop(g);
            if let (Some(prior), None) = (was, kind) {
                // Subsystem reported end; drive SessionEnded to keep
                // the FSM honest.
                self.drive(CcInput::SessionEnded { kind: prior });
            }
        }
    }

    // ---------------- FSM drive + effects ----------------

    fn drive(&self, input: CcInput) {
        let (effect, next, first_run) = {
            let mut guard = self
                .inner
                .state
                .lock()
                .expect("cc state mutex must be unpoisoned");
            let session = self
                .inner
                .current_session
                .lock()
                .map(|g| *g)
                .unwrap_or(None);
            let first_run = self.inner.first_run.load(Ordering::Relaxed);
            let t = apply(*guard, input, session, first_run);
            *guard = t.next;
            (t.effect, t.next, first_run)
        };
        tracing::debug!(
            target: "command_center",
            ?input,
            ?next,
            ?effect,
            "fsm step"
        );
        self.run_effect(effect);
        // Always emit the latest state to the UI so React can re-render.
        self.emit_state(next, first_run);
        // First dismiss flips the "seen" setting. Cheap to do here
        // rather than in the FSM (which is pure).
        if matches!(input, CcInput::Dismiss) && first_run {
            self.inner.first_run.store(false, Ordering::Relaxed);
            self.persist_seen_flag();
        }
    }

    fn run_effect(&self, effect: CcEffect) {
        match effect {
            CcEffect::None => {}
            CcEffect::ShowWindow { .. } => self.show_window(),
            CcEffect::HideWindow => self.hide_window(),
            CcEffect::DispatchStart { kind } => self.dispatch_start(kind),
            CcEffect::DispatchStop { kind } => self.dispatch_stop(kind),
        }
    }

    fn show_window(&self) {
        let Some(w) = self
            .inner
            .app
            .get_webview_window(COMMAND_CENTER_WINDOW_LABEL)
        else {
            tracing::warn!(
                target: "command_center",
                label = COMMAND_CENTER_WINDOW_LABEL,
                "command center window not found; check tauri.conf.json"
            );
            return;
        };
        overlay_conventions::apply_noactivate_layered(&w);
        if let Err(e) = w.show() {
            tracing::warn!(target: "command_center", error = ?e, "show command center");
        }
        // Dictation pip should yield bottom-center while the CC is up.
        if let Some(rw) = self
            .inner
            .app
            .try_state::<crate::recording_window::RecordingWindow>()
        {
            rw.set_suppressed_for_command_center(true);
        }
    }

    fn hide_window(&self) {
        if let Some(w) = self
            .inner
            .app
            .get_webview_window(COMMAND_CENTER_WINDOW_LABEL)
        {
            if let Err(e) = w.hide() {
                tracing::warn!(target: "command_center", error = ?e, "hide command center");
            }
        }
        // Release the dictation-pip suppression flag.
        if let Some(rw) = self
            .inner
            .app
            .try_state::<crate::recording_window::RecordingWindow>()
        {
            rw.set_suppressed_for_command_center(false);
        }
    }

    fn dispatch_start(&self, kind: RecordingKind) {
        match kind {
            RecordingKind::Dictation => {
                // Push-to-talk has no programmatic start. The tile's
                // UX per ADR 0037 §4 is the "or just hold Right Alt"
                // hint; picking it dismisses the CC. We treat this as
                // immediate success so the FSM returns to picker on
                // the next interaction.
                tracing::info!(
                    target: "command_center",
                    "pick_mode(dictation) — dismissing; user holds Right Alt to start"
                );
                self.drive(CcInput::Dismiss);
            }
            RecordingKind::Meeting => self.dispatch_meeting_start(),
            RecordingKind::Activity => self.dispatch_activity_start(),
        }
    }

    #[cfg(target_os = "windows")]
    fn dispatch_meeting_start(&self) {
        // Resolve the most current MeetingRuntimeShared either from
        // our inner cache (preferred — set at spawn time via
        // `attach_meeting_runtime`) or from Tauri managed state as
        // a fallback. Either path produces a clone-cheap handle.
        let mc = {
            let cached = self.inner.meeting.lock().ok().and_then(|g| g.clone());
            cached.or_else(|| {
                self.inner
                    .app
                    .try_state::<crate::meetings::runtime::MeetingRuntimeShared>()
                    .map(|s| s.inner().clone())
            })
        };
        let Some(mc) = mc else {
            tracing::warn!(
                target: "command_center",
                "no meeting runtime registered; cannot start meeting from CC"
            );
            self.drive(CcInput::RuntimeReplied { success: false });
            return;
        };
        // Default source from settings; project LastChosenSource onto
        // the capture-side `MeetingSource` (the two enums live in
        // different modules on purpose).
        let source = {
            use crate::meetings::activation::LastChosenSource;
            use crate::meetings::capture::MeetingSource;
            match mc.config.default_source {
                LastChosenSource::Mic => MeetingSource::Mic,
                LastChosenSource::System => MeetingSource::System,
                LastChosenSource::Both => MeetingSource::Both,
            }
        };
        match mc.start_meeting(source) {
            Ok(uuid) => {
                tracing::info!(
                    target: "command_center",
                    %uuid,
                    "meeting start dispatched from CC"
                );
                self.set_current_session(Some(RecordingKind::Meeting));
                self.drive(CcInput::RuntimeReplied { success: true });
            }
            Err(e) => {
                tracing::warn!(
                    target: "command_center",
                    error = %e,
                    "meeting start failed from CC"
                );
                self.drive(CcInput::RuntimeReplied { success: false });
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn dispatch_meeting_start(&self) {
        tracing::info!(
            target: "command_center",
            "meeting start not available on this platform"
        );
        self.drive(CcInput::RuntimeReplied { success: false });
    }

    fn dispatch_stop(&self, kind: RecordingKind) {
        match kind {
            RecordingKind::Dictation => {
                // Mirror the recording-overlay's Esc-cancel: emit the
                // same event the overlay does, so the dictation
                // orchestrator transitions to Aborted gracefully.
                if let Err(e) = self.inner.app.emit("dictation:cancel", ()) {
                    tracing::warn!(target: "command_center", error = ?e, "emit dictation:cancel");
                }
            }
            RecordingKind::Meeting => self.dispatch_meeting_stop(),
            RecordingKind::Activity => self.dispatch_activity_stop(),
        }
    }

    #[cfg(target_os = "windows")]
    fn dispatch_meeting_stop(&self) {
        let mc = self
            .inner
            .meeting
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .or_else(|| {
                self.inner
                    .app
                    .try_state::<crate::meetings::runtime::MeetingRuntimeShared>()
                    .map(|s| s.inner().clone())
            });
        let Some(mc) = mc else {
            tracing::warn!(target: "command_center", "no meeting runtime to stop");
            return;
        };
        let in_flight = mc
            .in_flight
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|m| m.uuid.clone()));
        match in_flight {
            Some(uuid) => {
                if let Err(e) = mc.stop_meeting(&uuid) {
                    tracing::warn!(target: "command_center", error = %e, "stop meeting failed");
                }
                self.set_current_session(None);
            }
            None => {
                tracing::debug!(
                    target: "command_center",
                    "stop dispatched but no in-flight meeting"
                );
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn dispatch_meeting_stop(&self) {}

    /// Phase 10 Wave 1B — dispatch Activity tile pick. The runtime
    /// is registered as managed state at boot (lib.rs::run). We hit
    /// `start_with_audio()` (Phase 10 Wave 4: honors
    /// `ActivityAudioEnabled`), then drive the FSM forward with the
    /// result.
    fn dispatch_activity_start(&self) {
        let Some(rt) = self
            .inner
            .app
            .try_state::<crate::activity::ActivityCaptureRuntime>()
            .map(|s| s.inner().clone())
        else {
            tracing::warn!(
                target: "command_center",
                "no activity runtime registered; cannot start activity from CC"
            );
            self.drive(CcInput::RuntimeReplied { success: false });
            return;
        };
        // Wave 4: read the audio setting in its own short critical
        // section so the CC dispatch path doesn't hold the DB lock
        // across the FSM step. Best-effort: a setting-read failure
        // logs + falls back to audio-off (the safe default).
        let with_audio = self
            .inner
            .app
            .try_state::<crate::commands::AppStateHandle>()
            .and_then(|s| {
                s.db.lock().ok().map(|conn| {
                    crate::settings::Settings::new(&conn)
                        .get::<bool>(crate::settings::model::SettingKey::ActivityAudioEnabled)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        match rt.start_with_audio(with_audio) {
            Ok(()) => {
                let sid = rt.current_session_id();
                tracing::info!(
                    target: "command_center",
                    session_id = ?sid,
                    "activity start dispatched from CC"
                );
                self.set_current_session(Some(RecordingKind::Activity));
                self.drive(CcInput::RuntimeReplied { success: true });
            }
            Err(e) => {
                tracing::warn!(
                    target: "command_center",
                    error = %e,
                    "activity start failed from CC"
                );
                self.drive(CcInput::RuntimeReplied { success: false });
            }
        }
    }

    fn dispatch_activity_stop(&self) {
        let Some(rt) = self
            .inner
            .app
            .try_state::<crate::activity::ActivityCaptureRuntime>()
            .map(|s| s.inner().clone())
        else {
            tracing::warn!(target: "command_center", "no activity runtime to stop");
            return;
        };
        if let Err(e) = rt.stop() {
            tracing::warn!(target: "command_center", error = %e, "activity stop failed");
        }
        self.set_current_session(None);
    }

    fn emit_state(&self, state: CcState, first_run: bool) {
        let (label, kind) = match state {
            CcState::Closed => ("closed", None),
            CcState::ShowingModePicker { .. } => ("modePicker", None),
            CcState::ShowingSessionCard { kind } => ("sessionCard", Some(kind.as_str())),
            CcState::Launching { kind } => ("launching", Some(kind.as_str())),
        };
        let payload = CcStatePayload {
            state: label,
            first_run: first_run && matches!(state, CcState::ShowingModePicker { first_run: true }),
            kind,
        };
        if let Err(e) = self.inner.app.emit(STATE_EVENT, &payload) {
            tracing::debug!(
                target: "command_center",
                error = ?e,
                "emit command_center:state failed (UI not mounted yet?)"
            );
        }
    }

    fn persist_seen_flag(&self) {
        // Best-effort: a failure to persist just means the Welcome
        // banner will re-show next launch — mildly annoying, not
        // broken.
        let Some(state) = self.inner.app.try_state::<crate::commands::AppState>() else {
            return;
        };
        // Bind the Arc clone so the State<> guard can drop before we
        // try to lock. Drop-order pedantry: we extract the lock
        // result into an owned Option so the temporary Result's
        // borrow expires before the function returns (the trailing
        // semicolon is what the rustc note recommended). State<> is
        // a thin guard that doesn't impl Drop; the explicit drop is
        // a documentation marker for the borrow lifetime, not a
        // resource release.
        let conn_arc = state.db.clone();
        let _ = state; // explicit consume so the guard can release
        let locked = conn_arc.lock().ok();
        if let Some(guard) = locked {
            let s = Settings::new(&guard);
            if let Err(e) = s.set(SettingKey::CommandCenterSeenV1, &true) {
                tracing::warn!(
                    target: "command_center",
                    error = %e,
                    "persist command_center_seen_v1 failed"
                );
            }
        };
    }
}

// ---------------- Helpers ----------------

/// Parse a chord descriptor like `"RightCtrl+Space"` into a VK pair.
///
/// Accepts the same name-set as the meeting chord picker (via
/// [`crate::meetings::vk_names`]). The friendly aliases on the LHS
/// (`"RightCtrl"` vs. `"VK_RCONTROL"`) are translated here; everything
/// else is delegated.
fn parse_chord(s: &str) -> Option<CcChordConfig> {
    let (lhs, rhs) = s.split_once('+')?;
    let modifier_vk = vk_alias_or_name(lhs.trim())?;
    let main_vk = vk_alias_or_name(rhs.trim())?;
    Some(CcChordConfig {
        modifier_vk,
        main_vk,
    })
}

fn vk_alias_or_name(s: &str) -> Option<u32> {
    // Friendly aliases used in the chord setting string. Everything
    // else falls through to the VK_* name resolver.
    let resolved = match s.to_ascii_lowercase().as_str() {
        "rightctrl" | "rctrl" => Some("VK_RCONTROL"),
        "leftctrl" | "lctrl" => Some("VK_LCONTROL"),
        "rightalt" | "ralt" => Some("VK_RMENU"),
        "leftalt" | "lalt" => Some("VK_LMENU"),
        "rightshift" | "rshift" => Some("VK_RSHIFT"),
        "leftshift" | "lshift" => Some("VK_LSHIFT"),
        "rightwin" | "rwin" => Some("VK_RWIN"),
        "leftwin" | "lwin" => Some("VK_LWIN"),
        "space" => Some("VK_SPACE"),
        _ => None,
    };
    let name = resolved.unwrap_or(s);
    crate::meetings::vk_names::vk_name_to_code(name).ok()
}

fn wire_session_listeners<R: Runtime + 'static>(app: &AppHandle<R>, _cc: CommandCenter) {
    // Both the dictation overlay's `dictation:state` and the meeting
    // overlay's `meeting:state` events are best-effort signals; if we
    // miss one because Tauri hasn't fully booted the event bus, the
    // worst case is a stale SessionCard that the user can dismiss.
    //
    // We deliberately do NOT subscribe to these events from Rust in
    // Wave 1A. Tauri's `listen` API lives on the JS side; subscribing
    // from Rust would require wiring the event-loop runtime in, which
    // is a pile of incidental complexity for what the UI can already
    // observe directly (the CommandCenter.tsx component listens to
    // both event streams and calls `cc_update_session` via the IPC
    // command exposed in `commands/`).
    //
    // This function exists as the binding seam for Wave 1B when we
    // wire the activity-state subscription. For now it's a no-op +
    // a log line so the boot trace is honest about which signals are
    // active.
    let _ = app;
    tracing::debug!(
        target: "command_center",
        "session listeners: UI-driven (no Rust listen() in 1A)"
    );
}

// ---------------- Tauri command surface ----------------

/// IPC commands the UI invokes. Mounted in `commands::register`.
pub mod ipc {
    use super::{CommandCenter, RecordingKind};
    use serde::{Deserialize, Serialize};
    use tauri::{AppHandle, Manager, Runtime};

    /// What the UI tells us about the current session. Mirrors the
    /// shape on the TS side.
    #[derive(Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum SessionKindArg {
        Dictation,
        Meeting,
        Activity,
        None,
    }

    impl SessionKindArg {
        fn into_opt(self) -> Option<RecordingKind> {
            match self {
                Self::Dictation => Some(RecordingKind::Dictation),
                Self::Meeting => Some(RecordingKind::Meeting),
                Self::Activity => Some(RecordingKind::Activity),
                Self::None => None,
            }
        }
    }

    /// Shape returned by `cc_get_state`.
    #[derive(Serialize)]
    pub struct CcStateSnapshot {
        pub state: String,
        #[serde(rename = "firstRun")]
        pub first_run: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub kind: Option<String>,
    }

    fn must<R: Runtime>(app: &AppHandle<R>) -> Option<CommandCenter> {
        app.try_state::<CommandCenter>().map(|s| s.inner().clone())
    }

    #[tauri::command]
    pub fn cc_open_via_tray<R: Runtime>(app: AppHandle<R>) {
        if let Some(cc) = must(&app) {
            cc.open_via_tray();
        }
    }

    #[tauri::command]
    pub fn cc_dismiss<R: Runtime>(app: AppHandle<R>) {
        if let Some(cc) = must(&app) {
            cc.dismiss();
        }
    }

    #[tauri::command]
    pub fn cc_pick_mode<R: Runtime>(app: AppHandle<R>, kind: String) {
        let Some(cc) = must(&app) else {
            return;
        };
        let k = match kind.as_str() {
            "dictation" => RecordingKind::Dictation,
            "meeting" => RecordingKind::Meeting,
            "activity" => RecordingKind::Activity,
            other => {
                tracing::warn!(
                    target: "command_center",
                    kind = %other,
                    "cc_pick_mode: unknown kind"
                );
                return;
            }
        };
        cc.pick_mode(k);
    }

    #[tauri::command]
    pub fn cc_stop_active_session<R: Runtime>(app: AppHandle<R>) {
        if let Some(cc) = must(&app) {
            cc.stop_active_session();
        }
    }

    /// Called by the UI when it observes a `dictation:state` /
    /// `meeting:state` event indicating the live recording's kind
    /// changed. `kind == "none"` clears.
    #[tauri::command]
    pub fn cc_update_session<R: Runtime>(app: AppHandle<R>, kind: SessionKindArg) {
        if let Some(cc) = must(&app) {
            cc.set_current_session(kind.into_opt());
        }
    }

    #[tauri::command]
    pub fn cc_get_state<R: Runtime>(app: AppHandle<R>) -> CcStateSnapshot {
        let Some(cc) = must(&app) else {
            return CcStateSnapshot {
                state: "closed".into(),
                first_run: false,
                kind: None,
            };
        };
        let s = cc.snapshot();
        match s {
            super::CcState::Closed => CcStateSnapshot {
                state: "closed".into(),
                first_run: false,
                kind: None,
            },
            super::CcState::ShowingModePicker { first_run } => CcStateSnapshot {
                state: "modePicker".into(),
                first_run,
                kind: None,
            },
            super::CcState::ShowingSessionCard { kind } => CcStateSnapshot {
                state: "sessionCard".into(),
                first_run: false,
                kind: Some(kind.as_str().into()),
            },
            super::CcState::Launching { kind } => CcStateSnapshot {
                state: "launching".into(),
                first_run: false,
                kind: Some(kind.as_str().into()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chord_default() {
        let c = parse_chord("RightCtrl+Space").unwrap();
        assert_eq!(c.modifier_vk, 0xA3);
        assert_eq!(c.main_vk, 0x20);
    }

    #[test]
    fn parse_chord_lowercase_alias() {
        let c = parse_chord("rightctrl+space").unwrap();
        assert_eq!(c.modifier_vk, 0xA3);
        assert_eq!(c.main_vk, 0x20);
    }

    #[test]
    fn parse_chord_canonical_vk_names() {
        let c = parse_chord("VK_RCONTROL+VK_SPACE").unwrap();
        assert_eq!(c.modifier_vk, 0xA3);
        assert_eq!(c.main_vk, 0x20);
    }

    #[test]
    fn parse_chord_period_main_key() {
        let c = parse_chord("RightCtrl+VK_OEM_PERIOD").unwrap();
        assert_eq!(c.modifier_vk, 0xA3);
        // VK_OEM_PERIOD = 0xBE.
        assert_eq!(c.main_vk, 0xBE);
    }

    #[test]
    fn parse_chord_garbage_returns_none() {
        assert!(parse_chord("absolute nonsense").is_none());
        assert!(parse_chord("RightCtrl").is_none()); // no '+'
        assert!(parse_chord("Foo+Bar").is_none());
    }

    #[test]
    fn parse_chord_strips_whitespace() {
        let c = parse_chord("RightCtrl + Space").unwrap();
        assert_eq!(c.modifier_vk, 0xA3);
        assert_eq!(c.main_vk, 0x20);
    }
}
