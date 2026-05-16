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
//! ## Pure-vs-OS split
//!
//! No OS surface yet, so this entire module is pure. The stub state
//! machine is tested without any Tauri runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::AppResult;

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
    /// Wave-4 stub: just flips the visibility atomic. The real
    /// implementation in Phase 5 will create / show the Tauri window
    /// here.
    pub fn show(&self) -> AppResult<()> {
        if !self.visible.swap(true, Ordering::SeqCst) {
            tracing::debug!("recording window: show (stub)");
        }
        Ok(())
    }

    /// Hide the recording window. Idempotent.
    pub fn hide(&self) -> AppResult<()> {
        if self.visible.swap(false, Ordering::SeqCst) {
            tracing::debug!("recording window: hide (stub)");
        }
        Ok(())
    }

    /// Current visibility — exposed for the tray status + tests.
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

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
