//! Recording-window owner.
//!
//! The recording window is a small **non-activating** Tauri webview
//! (label `"recording"`, configured in `tauri.conf.json` with
//! `focus: false`, `decorations: false`, `transparent: true`,
//! `alwaysOnTop: true`, `skipTaskbar: true`). The orchestrator shows
//! it during `StateAction::StartCapture` and hides it on
//! `StopCapture` / `DiscardAudio`. Mid-pipeline it emits
//! `dictation:state` events so the React overlay (`recording.html`)
//! can move from "listening" to "transcribing" to "pasting" to "done".
//!
//! ## Contract
//!
//! - **Idempotent** show / hide / set_state. Orchestrator never has to
//!   track its own visibility.
//! - **App-handle is optional**. Pure unit tests construct without
//!   one; production wiring in `lib.rs::run()` calls
//!   [`Self::set_app_handle`] right after the runtime is spawned. This
//!   keeps the orchestrator constructible in tests with no Tauri
//!   runtime present.
//! - **Non-activating**. We `.show()` the window but never `.set_focus()`,
//!   and the config locks `focus: false`. If a future change ever
//!   focuses this window, our SendInput will land in **our own** webview
//!   instead of the user's app — catastrophic ADR 0016 §7 failure.
//! - **Best-effort I/O**. Tauri errors (window gone, runtime not yet
//!   ready) are logged and swallowed — feedback is convenience, never
//!   correctness, so a missing UI must not stall the dictation pipeline.
//!
//! ## Audible beep (interim)
//!
//! While the visual indicator was unwired, `show()`/`hide()` emitted
//! short kernel32 `Beep`s as audible feedback. They're still here
//! gated on `--feature audible-beeps`; the default build is silent
//! now that the overlay is real.
//!
//! ## Cross-platform
//!
//! The state machine + emit logic is portable; the visual webview is
//! Tauri-provided on every platform. The `Beep` helper is
//! Windows-only and no-ops elsewhere.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::sessions::StartMode;
use crate::error::AppResult;

/// Webview label declared in `tauri.conf.json`. Kept here as a const
/// so the wiring stays in one file.
const RECORDING_WINDOW_LABEL: &str = "recording";

/// Event name the recording overlay subscribes to. Mirrored in the
/// TypeScript side at `ui/src/lib/tauri.ts`.
const STATE_EVENT: &str = "dictation:state";

/// Event fired *after* a session row has been committed to the
/// database (Complete or Error). The main UI's History page listens
/// for this to refresh its list live, without a manual reload.
///
/// Decoupled from `dictation:state` deliberately: the state event
/// is the UI state machine (idle → listening → … → done/error) and
/// it sometimes fires BEFORE the DB write (e.g. failed-STT path
/// surfaces the error indicator first so the user sees feedback even
/// if persistence stalls). "Data changed" is a separate concern; it
/// must only fire AFTER the row exists, otherwise the History
/// refetch would race and miss the new entry.
const SESSION_SAVED_EVENT: &str = "history:session-saved";

/// Pipeline states surfaced to the overlay. Strings (not an enum
/// discriminator) so adding a new state on the Rust side doesn't
/// silently change the TypeScript shape.
pub mod state {
    /// User is holding the hotkey; mic is open.
    pub const LISTENING: &str = "listening";
    /// Whisper is running on the captured audio.
    pub const TRANSCRIBING: &str = "transcribing";
    /// LLM cleanup pass.
    pub const CLEANING: &str = "cleaning";
    /// SendInput / clipboard paste in progress.
    pub const PASTING: &str = "pasting";
    /// Successfully injected — shown briefly before fading to idle.
    pub const DONE: &str = "done";
    /// Pipeline failed (with an error message in payload).
    pub const ERROR: &str = "error";
    /// Quiet state — window hidden, no activity.
    pub const IDLE: &str = "idle";
    /// User cancelled mid-flight.
    pub const ABORTED: &str = "aborted";
}

/// Payload shape for the `history:session-saved` event. Mirrored
/// in TypeScript on the History page's listener. The id is enough
/// for the frontend to optimistically refetch and select if desired;
/// kept deliberately small (no row body) since the page already has
/// `get_session_detail(id)` for the full payload.
#[derive(Clone, Debug, Serialize)]
struct SessionSavedPayload {
    #[serde(rename = "sessionId")]
    session_id: i64,
}

/// Payload shape for the `dictation:state` event. Matches
/// `DictationStateEvent` on the TypeScript side.
#[derive(Clone, Debug, Serialize)]
struct StateEventPayload<'a> {
    state: &'a str,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modeSlug")]
    mode_slug: Option<&'a str>,
    /// Human-readable mode name ("Normal", "Verbose", …). Without
    /// this the React overlay's mode badge stays hidden because the
    /// component renders `event.modeLabel && <Badge>…</Badge>`.
    #[serde(skip_serializing_if = "Option::is_none", rename = "modeLabel")]
    mode_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
    /// ADR 0045 + mb-tfyp — `"ptt"` or `"in_app"`. Carried on EVERY
    /// emit during a session (not just `listening`) so a listener
    /// that subscribes after the initial `listening` fires (e.g. the
    /// Dictations page's `<DictationRecordButton>` re-mounting on
    /// tab switch) still sees the value. Omitted when there is no
    /// active session (idle / pre-first-show).
    #[serde(skip_serializing_if = "Option::is_none", rename = "startMode")]
    start_mode: Option<&'a str>,
}

/// Internal encoding of the optional `StartMode` for the
/// `AtomicU8` slot. SeqCst loads return a `u8`; this struct just
/// names the discriminants in one place.
mod start_mode_slot {
    pub const NONE: u8 = 0;
    pub const PTT: u8 = 1;
    pub const IN_APP: u8 = 2;
}

/// Title-case a mode slug ("normal" → "Normal"). Cheap, no allocator
/// in the common case of an empty input. Kept private — the React
/// overlay does its own label lookup for i18n, this is the fallback.
fn label_from_slug(slug: &str) -> String {
    let mut chars = slug.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Owner of the recording-window state.
///
/// Cheap to clone (`Arc`s under the hood). The orchestrator hands
/// clones to every thread that needs to surface state — they all
/// share the same visibility flag and the same `AppHandle`.
#[derive(Clone, Default)]
pub struct RecordingWindow {
    visible: Arc<AtomicBool>,
    /// Set lazily from `lib.rs::run()` after the Tauri app is built.
    /// `Option` so unit tests can construct a window without spinning
    /// up a full Tauri runtime.
    app: Arc<Mutex<Option<AppHandle>>>,
    /// ADR 0045 + mb-tfyp — current session's `StartMode`, included
    /// in every `dictation:state` emit so the React overlay can
    /// render the pill-overlay Stop button conditionally without
    /// fetching extra state. AtomicU8 over Mutex<Option<StartMode>>
    /// to keep `emit()` lock-free (emits happen from multiple
    /// threads — the cold-start re-emit burst spawns its own).
    /// `0 = unset`, `1 = ptt`, `2 = in_app`. See `start_mode_slot`.
    start_mode: Arc<AtomicU8>,
    /// Phase 10 Wave 1A (ADR 0037 §5 — surgical touch authorized).
    /// When the Command Center window is up, the dictation pip and
    /// the CC would collide at bottom-center. The CC sets this flag
    /// to `true` while it's visible; `show()` honors it by skipping
    /// the actual webview `show()` while still emitting state events
    /// (so any other listener — e.g. tests, future overlays — keeps
    /// getting the pipeline-state stream).
    ///
    /// **Suppresses visibility only.** The dictation pipeline itself
    /// (audio capture, STT, cleanup, injection) is unaffected. This
    /// is a UI-collision avoidance flag, not a state-machine flag.
    suppressed_for_command_center: Arc<AtomicBool>,
}

impl RecordingWindow {
    /// Construct a new window owner. The Tauri handle is plugged in
    /// later via [`Self::set_app_handle`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Phase 10 Wave 1A. Flip the suppression flag. When `true`, the
    /// dictation pip's [`Self::show`] still emits the `listening`
    /// state event but skips the actual webview show, leaving the
    /// Command Center window's bottom-center slot uncontested. The
    /// Command Center orchestrator flips this on when its window
    /// opens and off when it closes.
    pub fn set_suppressed_for_command_center(&self, suppressed: bool) {
        let prior = self
            .suppressed_for_command_center
            .swap(suppressed, Ordering::SeqCst);
        if prior != suppressed {
            tracing::debug!(
                target: "recording_window",
                suppressed,
                "command-center suppression flag toggled"
            );
            // If suppression is being lifted while we believe we're
            // visible, re-issue the show() so the pip actually appears
            // (the previous show() while suppressed was a no-op).
            if !suppressed && self.visible.load(Ordering::SeqCst) {
                self.with_window(|w| {
                    let _ = w.show();
                });
            }
            // Conversely, if suppression is being engaged while the
            // pip is currently visible, hide the webview but DO NOT
            // touch `visible` — the dictation pipeline still thinks
            // it's recording, and when suppression lifts we need to
            // restore the visual.
            if suppressed && self.visible.load(Ordering::SeqCst) {
                self.with_window(|w| {
                    let _ = w.hide();
                });
            }
        }
    }

    /// Current value of the suppression flag. Exposed for tests +
    /// any future tray status surface that wants to display "pip
    /// suppressed by Command Center".
    pub fn is_suppressed_for_command_center(&self) -> bool {
        self.suppressed_for_command_center.load(Ordering::SeqCst)
    }

    /// ADR 0045 + mb-tfyp — record the start_mode for the session
    /// the orchestrator is about to begin. Subsequent `emit()`s
    /// (listening, transcribing, cleaning, pasting, done) carry
    /// this value in the payload. Called from
    /// `DictationOrchestrator::start_capture` BEFORE `show()`.
    ///
    /// `hide()` resets to `unset` so an unrelated `error` /
    /// `aborted` emit between sessions doesn't leak the previous
    /// session's mode.
    pub fn set_start_mode(&self, mode: StartMode) {
        let slot = match mode {
            StartMode::Ptt => start_mode_slot::PTT,
            StartMode::InApp => start_mode_slot::IN_APP,
        };
        self.start_mode.store(slot, Ordering::SeqCst);
    }

    /// Reset start_mode to "unset". Internal: called from `hide()`.
    fn clear_start_mode(&self) {
        self.start_mode
            .store(start_mode_slot::NONE, Ordering::SeqCst);
    }

    /// Snapshot the current start_mode as the wire string, if any.
    fn current_start_mode_str(&self) -> Option<&'static str> {
        match self.start_mode.load(Ordering::SeqCst) {
            start_mode_slot::PTT => Some("ptt"),
            start_mode_slot::IN_APP => Some("in_app"),
            _ => None,
        }
    }

    /// Wire up the Tauri app handle. Idempotent — last writer wins.
    /// Called once from `lib.rs::run()` setup.
    pub fn set_app_handle(&self, app: AppHandle) {
        // The Mutex is held briefly only on (rare) handle replacement;
        // reads on the hot path use a separate try_lock-equivalent.
        if let Ok(mut g) = self.app.lock() {
            *g = Some(app);
        }
    }

    /// Show the recording window and emit a `listening` state event.
    /// Idempotent — calling `show()` while visible just re-emits the
    /// state (cheap, and lets the UI recover if it missed the first).
    ///
    /// **Emit-after-show race**: the first `w.show()` is a cold start
    /// for the webview (WebView2 process spawn + React mount + event
    /// listener registration — 50–500ms). A single emit fires while
    /// React is still mounting and the listener never sees it. We
    /// counter this with a tiny burst of re-emits at 50ms / 200ms /
    /// 500ms after the initial fire. Cost: 3 cheap event sends per
    /// dictation. Benefit: the pretty pill actually appears.
    pub fn show(&self, mode_slug: &str) -> AppResult<()> {
        let was_hidden = !self.visible.swap(true, Ordering::SeqCst);
        // Phase 10 Wave 1A: while the Command Center occupies the
        // bottom-center slot, skip the actual webview show. The
        // state-event stream still fires — the pipeline is recording,
        // we're just hiding the pip until the CC closes.
        let suppressed = self.suppressed_for_command_center.load(Ordering::SeqCst);
        if was_hidden {
            tracing::info!(
                mode = %mode_slug,
                suppressed,
                "🎙 recording window: SHOW"
            );
            beep_best_effort(BEEP_START_HZ, BEEP_DURATION_MS);
            if !suppressed {
                self.with_window(|w| {
                    let _ = w.show();
                });
            }
        }
        self.emit(state::LISTENING, Some(mode_slug), None);

        // Re-emit burst to win the cold-start race on first show.
        if was_hidden {
            let me = self.clone();
            let slug = mode_slug.to_string();
            std::thread::spawn(move || {
                for delay_ms in [50u64, 200, 500] {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    // Bail if the window was hidden in the meantime
                    // (very short dictations).
                    if !me.visible.load(Ordering::SeqCst) {
                        return;
                    }
                    me.emit(state::LISTENING, Some(&slug), None);
                }
            });
        }

        Ok(())
    }

    /// Hide the recording window and emit an `idle` state event.
    /// Idempotent.
    pub fn hide(&self) -> AppResult<()> {
        let was_visible = self.visible.swap(false, Ordering::SeqCst);
        if was_visible {
            tracing::info!("🎙 recording window: HIDE");
            beep_best_effort(BEEP_STOP_HZ, BEEP_DURATION_MS);
            self.with_window(|w| {
                let _ = w.hide();
            });
        }
        // Emit IDLE WITH the trailing start_mode (so the overlay
        // sees the session it was tracking end cleanly), then clear
        // the slot so the next session starts clean.
        self.emit(state::IDLE, None, None);
        self.clear_start_mode();
        Ok(())
    }

    /// Emit a mid-pipeline state without touching visibility. Used
    /// during `complete()` to push `transcribing` / `cleaning` /
    /// `pasting` / `done` to the overlay.
    pub fn set_state(&self, state: &str, mode_slug: Option<&str>) {
        self.emit(state, mode_slug, None);
    }

    /// Emit an error state. The overlay surfaces a brief toast then
    /// returns to idle.
    pub fn set_error(&self, msg: &str) {
        self.emit(state::ERROR, None, Some(msg));
    }

    /// Signal that a session row has just landed in the database.
    /// Fires `history:session-saved` with the new row id so the
    /// History page (and anything else interested) can refetch.
    /// Best-effort like every other emit — logged + swallowed on
    /// failure so the dictation pipeline never blocks on UI hiccups.
    pub fn emit_session_saved(&self, session_id: i64) {
        let Ok(guard) = self.app.lock() else { return };
        let Some(app) = guard.as_ref() else {
            tracing::trace!(
                session_id,
                "skip emit_session_saved: no app handle (tests / not booted)"
            );
            return;
        };
        let payload = SessionSavedPayload { session_id };
        if let Err(e) = app.emit(SESSION_SAVED_EVENT, &payload) {
            tracing::debug!(
                error = ?e,
                session_id,
                "failed to emit history:session-saved"
            );
        }
    }

    /// Current visibility — exposed for the tray status + tests.
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    /// Internal: run a closure with the recording webview if available.
    /// Logs + swallows the "window gone" case so the pipeline never
    /// stalls on UI hiccups.
    fn with_window<F: FnOnce(tauri::WebviewWindow)>(&self, f: F) {
        let Ok(guard) = self.app.lock() else { return };
        let Some(app) = guard.as_ref() else { return };
        match app.get_webview_window(RECORDING_WINDOW_LABEL) {
            Some(w) => f(w),
            None => {
                tracing::warn!(
                    label = RECORDING_WINDOW_LABEL,
                    "recording webview not found — running headless?"
                );
            }
        }
    }

    /// Internal: emit a `dictation:state` event. Errors are logged
    /// at debug — they're harmless in tests + headless contexts.
    fn emit(&self, state: &str, mode_slug: Option<&str>, error: Option<&str>) {
        let Ok(guard) = self.app.lock() else { return };
        let Some(app) = guard.as_ref() else {
            tracing::trace!(state, "skip emit: no app handle (tests / not booted)");
            return;
        };
        let mode_label = mode_slug.map(label_from_slug);
        let payload = StateEventPayload {
            state,
            mode_slug,
            mode_label,
            error,
            start_mode: self.current_start_mode_str(),
        };
        if let Err(e) = app.emit(STATE_EVENT, &payload) {
            tracing::debug!(error = ?e, state, "failed to emit dictation:state");
        }
    }
}

// ---------------------------------------------------------------------
// Audible beep — Windows-only kernel32 binding. Optional now that we
// have a real overlay; gated on a cargo feature for users who liked it.
// ---------------------------------------------------------------------

const BEEP_START_HZ: u32 = 800;
const BEEP_STOP_HZ: u32 = 400;
const BEEP_DURATION_MS: u32 = 60;

/// Best-effort audible beep. Errors are swallowed silently —
/// feedback is convenience, not correctness, so a missing audio
/// output (no speakers, headless CI) must never break dictation.
///
/// Gated on the `audible-beeps` cargo feature so the default build
/// is silent now that the visual overlay is real.
#[cfg(all(target_os = "windows", feature = "audible-beeps"))]
fn beep_best_effort(freq_hz: u32, duration_ms: u32) {
    // SAFETY: `Beep` is a stable kernel32 export with no callback or
    // pointer arguments; freq/duration are by-value u32s. The function
    // returns BOOL — we ignore the result. Linking against
    // `kernel32.dll` is guaranteed on all Windows targets.
    extern "system" {
        fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
    }
    unsafe {
        let _ = Beep(freq_hz, duration_ms);
    }
}

/// No-op on non-Windows targets or when the feature is disabled.
#[cfg(not(all(target_os = "windows", feature = "audible-beeps")))]
fn beep_best_effort(_freq_hz: u32, _duration_ms: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_is_hidden() {
        let w = RecordingWindow::new();
        assert!(!w.is_visible());
    }

    #[test]
    fn show_makes_visible() {
        let w = RecordingWindow::new();
        w.show("normal").unwrap();
        assert!(w.is_visible());
    }

    #[test]
    fn hide_after_show_makes_hidden() {
        let w = RecordingWindow::new();
        w.show("normal").unwrap();
        w.hide().unwrap();
        assert!(!w.is_visible());
    }

    #[test]
    fn show_is_idempotent() {
        let w = RecordingWindow::new();
        w.show("normal").unwrap();
        w.show("normal").unwrap();
        assert!(w.is_visible());
    }

    #[test]
    fn hide_is_idempotent() {
        let w = RecordingWindow::new();
        w.hide().unwrap();
        w.hide().unwrap();
        assert!(!w.is_visible());
    }

    #[test]
    fn clones_share_state() {
        // The orchestrator hands a clone to the tray; both must see
        // the same visibility flag.
        let w = RecordingWindow::new();
        let w2 = w.clone();
        w.show("normal").unwrap();
        assert!(w2.is_visible());
        w2.hide().unwrap();
        assert!(!w.is_visible());
    }

    #[test]
    fn set_state_does_not_change_visibility() {
        let w = RecordingWindow::new();
        w.show("normal").unwrap();
        w.set_state(state::TRANSCRIBING, Some("normal"));
        assert!(w.is_visible());
    }

    // -------- Phase 10 Wave 1A — command-center suppression --------

    #[test]
    fn suppression_flag_defaults_to_false() {
        let w = RecordingWindow::new();
        assert!(!w.is_suppressed_for_command_center());
    }

    #[test]
    fn suppression_flag_round_trips() {
        let w = RecordingWindow::new();
        w.set_suppressed_for_command_center(true);
        assert!(w.is_suppressed_for_command_center());
        w.set_suppressed_for_command_center(false);
        assert!(!w.is_suppressed_for_command_center());
    }

    #[test]
    fn suppression_does_not_change_visibility_state() {
        // Setting the flag is purely about the webview show; the
        // `visible` field tracks the dictation pipeline's belief
        // about whether it's recording and must not flip just because
        // the CC happens to be up.
        let w = RecordingWindow::new();
        w.show("normal").unwrap();
        assert!(w.is_visible());
        w.set_suppressed_for_command_center(true);
        assert!(w.is_visible(), "visible flag must be unaffected");
    }

    #[test]
    fn show_while_suppressed_still_flips_visible_flag() {
        // The pipeline calls show() at StartCapture regardless of
        // whether the CC is up. We honor the call (flip `visible`,
        // emit the event burst) but skip the webview show. When the
        // CC closes, the suppression-clear path re-issues the show().
        let w = RecordingWindow::new();
        w.set_suppressed_for_command_center(true);
        w.show("normal").unwrap();
        assert!(w.is_visible());
        assert!(w.is_suppressed_for_command_center());
    }

    #[test]
    fn suppression_clones_share_state() {
        let w = RecordingWindow::new();
        let w2 = w.clone();
        w.set_suppressed_for_command_center(true);
        assert!(w2.is_suppressed_for_command_center());
    }
}
