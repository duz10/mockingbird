//! Recording-window owner (stub for Wave 4).
//!
//! The recording window is a small non-activating Tauri window that
//! the orchestrator shows during `StateAction::StartCapture` and hides
//! on `StopCapture` / `DiscardAudio`. It surfaces a recording
//! indicator (level meter, elapsed timer, cancel button) — the React
//! UI inside the window lands in Phase 5.
//!
//! ## Wave 4 scope
//!
//! - **Stable Rust API**: [`RecordingWindow::show`] / [`Self::hide`] /
//!   [`Self::is_visible`].
//! - **Idempotent**: `show()` while visible is a no-op; `hide()` while
//!   hidden is a no-op. The orchestrator doesn't track visibility.
//! - **Stubbed visuals**: no actual Tauri window yet. State machine
//!   only — implementation lands in Phase 5 with the React side.
//! - **Non-activating contract**: any future impl MUST use Tauri's
//!   `focused(false)` + `WS_EX_NOACTIVATE` so the recording window
//!   never steals foreground (which would cause our injection to
//!   land in our OWN window — catastrophic ADR 0016 §7 failure mode).
//!
//! ## Wave 4.7 — interim audible feedback
//!
//! Until Phase 5 ships the real visual indicator, `show()` / `hide()`
//! emit short `Beep`s (800 Hz on start, 400 Hz on stop) so the user
//! has *some* confirmation that the hotkey was detected. Phase 5 may
//! keep or drop these — they're a temporary UX scaffold, not a
//! permanent contract. Beep is best-effort (errors logged, ignored).
//!
//! ## Pure-vs-OS split
//!
//! The state machine is pure. The audible-beep helper is `cfg`-gated
//! to Windows and no-ops elsewhere (Phase 9 macOS / Linux can fill in
//! platform-native equivalents).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::AppResult;

/// Frequency (Hz) of the "recording started" beep.
const BEEP_START_HZ: u32 = 800;
/// Frequency (Hz) of the "recording stopped" beep — lower so the
/// user can tell start vs stop by ear.
const BEEP_STOP_HZ: u32 = 400;
/// Beep duration (ms). Long enough to hear, short enough not to feel
/// laggy. `Beep` is synchronous, so this directly delays the orchestrator
/// — keep it small.
const BEEP_DURATION_MS: u32 = 60;

/// Owner of the recording-window state.
///
/// Cloneable — the orchestrator hands clones to the state-driver
/// thread (show/hide) and to the tray thread (it may want to know
/// "is the recording window currently up?" for status display).
#[derive(Clone, Default)]
pub struct RecordingWindow {
    visible: Arc<AtomicBool>,
}

impl RecordingWindow {
    /// Construct a new window owner with `visible = false`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the recording window. Idempotent.
    ///
    /// Wave-4 stub: flips the visibility atomic and (on Windows)
    /// emits a short start-beep for interim audible feedback. Phase 5
    /// will replace the beep with the real Tauri window.
    pub fn show(&self) -> AppResult<()> {
        if !self.visible.swap(true, Ordering::SeqCst) {
            tracing::info!("🎙 recording window: SHOW (start)");
            beep_best_effort(BEEP_START_HZ, BEEP_DURATION_MS);
        }
        Ok(())
    }

    /// Hide the recording window. Idempotent.
    pub fn hide(&self) -> AppResult<()> {
        if self.visible.swap(false, Ordering::SeqCst) {
            tracing::info!("🎙 recording window: HIDE (stop)");
            beep_best_effort(BEEP_STOP_HZ, BEEP_DURATION_MS);
        }
        Ok(())
    }

    /// Current visibility — exposed for the tray status + tests.
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

/// Best-effort audible beep. Errors are swallowed silently —
/// feedback is convenience, not correctness, so a missing audio
/// output (no speakers, headless CI) must never break dictation.
///
/// `Beep` is in kernel32. The `windows` 0.56 crate doesn't expose it
/// (it's a pre-Windows-NT relic), so we declare the FFI inline. The
/// signature has been frozen since Windows 95 — safe to bind by hand.
#[cfg(target_os = "windows")]
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

/// No-op on non-Windows targets. Phase 9 will provide platform-native
/// equivalents (macOS `NSBeep`, Linux `XBell` / PipeWire ping).
#[cfg(not(target_os = "windows"))]
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
        w.show().unwrap();
        assert!(w.is_visible());
    }

    #[test]
    fn hide_after_show_makes_hidden() {
        let w = RecordingWindow::new();
        w.show().unwrap();
        w.hide().unwrap();
        assert!(!w.is_visible());
    }

    #[test]
    fn show_is_idempotent() {
        let w = RecordingWindow::new();
        w.show().unwrap();
        w.show().unwrap();
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
        w.show().unwrap();
        assert!(w2.is_visible());
        w2.hide().unwrap();
        assert!(!w.is_visible());
    }
}
