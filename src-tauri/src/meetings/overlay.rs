//! Meeting overlay window owner.
//!
//! Mirrors `recording_window.rs` (the dictation overlay) but is its
//! own file — recording_window.rs is sealed (binding rule). The
//! meeting overlay window is declared in `tauri.conf.json` as
//! `"meeting_overlay"` (Wave 4) and rendered from
//! `ui/src/meeting_overlay.tsx` (Wave 4).
//!
//! This module owns the Tauri `AppHandle` for the overlay and the
//! show/hide + event-emit helpers.
//!
//! Wave 1 scaffold — empty struct + `todo!()` show/hide stubs.
//! Wave 4 ships the impl.

use crate::error::AppResult;

/// Owner of the `meeting_overlay` Tauri window. One per
/// `MeetingCaptureRuntime`.
#[derive(Debug, Default)]
pub struct MeetingOverlay {
    _private: (),
}

impl MeetingOverlay {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn show(&self) -> AppResult<()> {
        todo!("Wave 4: show the meeting_overlay window + emit ready event")
    }

    pub fn hide(&self) -> AppResult<()> {
        todo!("Wave 4: hide the meeting_overlay window")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_smoke() {
        let _ = MeetingOverlay::new();
    }
}
