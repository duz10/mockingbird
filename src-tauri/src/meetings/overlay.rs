//! Meeting overlay window helpers.
//!
//! Mirrors `recording_window.rs` (the dictation overlay) but is its
//! own file — recording_window.rs is sealed (binding rule). The
//! meeting overlay window is declared in `tauri.conf.json` as
//! `"meeting_overlay"` (Wave 4) and rendered from
//! `ui/src/meeting_overlay.tsx` (Wave 4).
//!
//! ## Wave 5 — wiring
//!
//! Wave 1 stubbed an empty `MeetingOverlay` struct with `todo!()`
//! show/hide. Wave 4 declared the actual Tauri window with
//! `visible: false`. Wave 5 (this file) ships the show/hide helpers
//! that get called from the activation-thread `handle_toggle` path
//! in [`super::runtime`].
//!
//! These are free functions rather than methods on a struct so the
//! activation loop doesn't have to thread an extra object around;
//! the only state they need is the `AppHandle` (already on
//! [`super::runtime::MeetingRuntimeShared`]).

use tauri::{AppHandle, Emitter, Manager};

/// Label of the meeting overlay webview, matches the declaration in
/// `tauri.conf.json`. Kept as a `pub const` so callers in tests +
/// future modules don't have to re-type the string.
pub const MEETING_OVERLAY_LABEL: &str = "meeting_overlay";

/// Show the overlay window and emit `meeting:overlay-open` so the
/// React side flips into CHOOSE mode. Best-effort: a missing window
/// (e.g. the user removed the declaration from `tauri.conf.json`)
/// logs a warning rather than panicking — the chord still drives
/// the meeting via `meeting:state` events from the lifecycle path,
/// so a missing overlay is degraded UX but not a broken app.
///
/// Returns `true` when the overlay window was found + show() was
/// dispatched; `false` when the window is missing (callers can fall
/// back to direct start if they like).
pub fn show_overlay(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(MEETING_OVERLAY_LABEL) else {
        tracing::warn!(
            label = MEETING_OVERLAY_LABEL,
            "meeting overlay window not found; tauri.conf.json missing the declaration?"
        );
        return false;
    };
    if let Err(e) = window.show() {
        tracing::warn!(error = ?e, "show meeting overlay");
        return false;
    }
    // Emit the open event so the React side flips to CHOOSE mode.
    // The window is declared with `focus: false` so showing it
    // shouldn't steal focus from whatever the user is typing into
    // (Zoom chat, Teams notes, etc.).
    if let Err(e) = app.emit(MEETING_OVERLAY_OPEN_EVENT, ()) {
        tracing::warn!(error = ?e, "emit meeting:overlay-open");
    }
    true
}

/// Show the overlay WITHOUT emitting the CHOOSE-mode event. Used
/// when the meeting has already started via a direct-start path
/// (e.g. the main-window "Start Recording" button) and the React
/// side will receive the recording state via the normal
/// `meeting:state` event stream from the lifecycle layer.
///
/// Without this, the chord path's `show_overlay` would emit
/// `meeting:overlay-open`, flipping the overlay into CHOOSE mode
/// even though we're already past CHOOSE. That'd cause a flicker
/// (CHOOSE → recording) the user would notice.
///
/// Returns `true` when the overlay window was found + show() was
/// dispatched; `false` when the window is missing.
pub fn force_show_for_recording(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(MEETING_OVERLAY_LABEL) else {
        tracing::warn!(
            label = MEETING_OVERLAY_LABEL,
            "meeting overlay window not found on direct-start path"
        );
        return false;
    };
    if let Err(e) = window.show() {
        tracing::warn!(error = ?e, "force-show meeting overlay");
        return false;
    }
    true
}

/// Hide the overlay window (no event — the React side self-clears
/// state on `meeting:state == "done"`).
pub fn hide_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MEETING_OVERLAY_LABEL) else {
        return;
    };
    if let Err(e) = window.hide() {
        tracing::warn!(error = ?e, "hide meeting overlay");
    }
}

/// Event name emitted when the chord requests the overlay. The
/// React side listens for this and flips into CHOOSE mode. Kept as
/// a constant so the TS side + tests can reference the same string.
pub const MEETING_OVERLAY_OPEN_EVENT: &str = "meeting:overlay-open";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_matches_tauri_conf_json() {
        // If this drifts the overlay window won't be found by label.
        assert_eq!(MEETING_OVERLAY_LABEL, "meeting_overlay");
    }

    #[test]
    fn event_name_matches_ui_subscription() {
        // The React side (ui/src/meeting_overlay/MeetingOverlay.tsx)
        // listens for this exact event name. Drift here = silent
        // overlay-doesn't-open bug.
        assert_eq!(MEETING_OVERLAY_OPEN_EVENT, "meeting:overlay-open");
    }
}
