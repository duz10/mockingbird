//! macOS first-launch permissions onboarding (ADR 0061 / mb-mac-v1.4.6).
//!
//! Mockingbird needs four macOS privacy grants to do full dictation +
//! meeting capture. None of them can be granted programmatically —
//! macOS requires the user to flip them in **System Settings → Privacy
//! & Security**. This module:
//!
//!   1. Models the four permissions + their grant states ([`Permission`],
//!      [`PermissionState`]).
//!   2. Builds the `x-apple.systempreferences:` deep-link URL that jumps
//!      the user straight to the right pane ([`Permission::settings_pane_url`]).
//!   3. Maps each platform API's raw status code into the shared
//!      [`PermissionState`] (the pure, unit-tested mapping functions).
//!   4. (macOS only, [`mod macos`]) performs the actual silent preflight
//!      status reads.
//!
//! ## The four permissions and their status-query APIs
//!
//! | Permission        | Query API                                   | Needed for           |
//! |-------------------|---------------------------------------------|----------------------|
//! | Microphone        | `AVCaptureDevice.authorizationStatus`       | dictation + meetings |
//! | Input Monitoring  | `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)` | the global hotkey tap |
//! | Accessibility     | `AXIsProcessTrusted()`                       | Cmd+V paste + secure-input guard |
//! | Screen Recording  | `CGPreflightScreenCaptureAccess()`          | Phase 4 meeting system-audio |
//!
//! All four queries are **silent preflights** — they read the current
//! grant without triggering a TCC prompt (the `request…` variants prompt;
//! we deliberately don't call those here, so polling on focus is free of
//! popup spam).
//!
//! The cross-platform logic here is fully testable; the macOS FFI is in
//! [`macos`] and the Tauri command surface lives in
//! `crate::commands::permissions`.

use serde::Serialize;

#[cfg(target_os = "macos")]
pub mod macos;

/// One of the four macOS privacy permissions Mockingbird surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    /// Microphone — dictation + meeting audio capture.
    Microphone,
    /// Input Monitoring — the global hotkey `CGEventTap` (Wave B1).
    InputMonitoring,
    /// Accessibility — the synthesized Cmd+V keypost + per-field secure
    /// input detection (Wave B2).
    Accessibility,
    /// Screen Recording — Phase 4 ScreenCaptureKit system-audio capture.
    /// Surfaced now for completeness even though dictation doesn't use it.
    ScreenRecording,
}

/// Current grant state of a permission.
///
/// `NotDetermined` means the user hasn't decided yet (no prompt shown);
/// `Denied` means an explicit deny; `Restricted` means policy/MDM lockout
/// (microphone only). For the boolean APIs (Accessibility, Screen
/// Recording) macOS can't distinguish "not yet decided" from "denied", so
/// a non-granted result maps to `NotDetermined` (see [`map_bool`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    /// Granted — the feature will work.
    Granted,
    /// Explicitly denied by the user.
    Denied,
    /// Not yet decided (no TCC prompt answered).
    NotDetermined,
    /// Locked out by policy / parental controls / MDM (mic only).
    Restricted,
    /// This platform doesn't have this permission concept (non-macOS).
    Unsupported,
}

/// All four permission states in one payload for the UI to poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatuses {
    /// Microphone grant state.
    pub microphone: PermissionState,
    /// Input Monitoring grant state (the global hotkey tap).
    pub input_monitoring: PermissionState,
    /// Accessibility grant state (Cmd+V paste + secure-input guard).
    pub accessibility: PermissionState,
    /// Screen Recording grant state (Phase 4 system-audio capture).
    pub screen_recording: PermissionState,
}

impl PermissionStatuses {
    /// All-`Unsupported` — the non-macOS answer.
    pub const fn unsupported() -> Self {
        Self {
            microphone: PermissionState::Unsupported,
            input_monitoring: PermissionState::Unsupported,
            accessibility: PermissionState::Unsupported,
            screen_recording: PermissionState::Unsupported,
        }
    }
}

impl Permission {
    /// Stable string key the UI sends back to `mac_open_settings_pane`.
    /// Matches the serde `camelCase` rendering so the round-trip is
    /// symmetric.
    pub const fn key(self) -> &'static str {
        match self {
            Permission::Microphone => "microphone",
            Permission::InputMonitoring => "inputMonitoring",
            Permission::Accessibility => "accessibility",
            Permission::ScreenRecording => "screenRecording",
        }
    }

    /// Parse a UI key back into a [`Permission`].
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "microphone" => Some(Permission::Microphone),
            "inputMonitoring" => Some(Permission::InputMonitoring),
            "accessibility" => Some(Permission::Accessibility),
            "screenRecording" => Some(Permission::ScreenRecording),
            _ => None,
        }
    }

    /// The `x-apple.systempreferences:` deep link that opens System
    /// Settings directly to this permission's pane.
    ///
    /// The `Privacy_*` anchors are the documented stable identifiers for
    /// the Privacy & Security panes (`Privacy_ListenEvent` is the
    /// not-obvious one — it's the *Input Monitoring* pane, not a literal
    /// "ListenEvent" UI string).
    pub const fn settings_pane_url(self) -> &'static str {
        match self {
            Permission::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            Permission::InputMonitoring => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            Permission::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Permission::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        }
    }
}

/// Map an `AVAuthorizationStatus` raw value to a [`PermissionState`].
///
/// Apple's enum: `NotDetermined = 0`, `Restricted = 1`, `Denied = 2`,
/// `Authorized = 3`. Anything unexpected falls back to `NotDetermined`
/// (the safe "prompt the user" state).
pub fn map_av_status(raw: i64) -> PermissionState {
    match raw {
        0 => PermissionState::NotDetermined,
        1 => PermissionState::Restricted,
        2 => PermissionState::Denied,
        3 => PermissionState::Granted,
        _ => PermissionState::NotDetermined,
    }
}

/// Map an `IOHIDAccessType` raw value to a [`PermissionState`].
///
/// `kIOHIDAccessTypeGranted = 0`, `kIOHIDAccessTypeDenied = 1`,
/// `kIOHIDAccessTypeUnknown = 2` (not yet decided). Unknown → it hasn't
/// been determined, so it maps to `NotDetermined`.
pub fn map_iohid_access(raw: u32) -> PermissionState {
    match raw {
        0 => PermissionState::Granted,
        1 => PermissionState::Denied,
        2 => PermissionState::NotDetermined,
        _ => PermissionState::NotDetermined,
    }
}

/// Map a boolean preflight (`AXIsProcessTrusted` /
/// `CGPreflightScreenCaptureAccess`) to a [`PermissionState`].
///
/// These APIs only answer "granted?" — they can't distinguish a fresh
/// install (never asked) from an explicit deny. A non-granted result is
/// therefore reported as `NotDetermined`: the honest, actionable state
/// ("open Settings and grant it"). Documented in ADR 0061.
pub fn map_bool(granted: bool) -> PermissionState {
    if granted {
        PermissionState::Granted
    } else {
        PermissionState::NotDetermined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_roundtrips_for_every_permission() {
        for p in [
            Permission::Microphone,
            Permission::InputMonitoring,
            Permission::Accessibility,
            Permission::ScreenRecording,
        ] {
            assert_eq!(Permission::from_key(p.key()), Some(p));
        }
    }

    #[test]
    fn from_key_rejects_garbage() {
        assert_eq!(Permission::from_key("camera"), None);
        assert_eq!(Permission::from_key(""), None);
        assert_eq!(Permission::from_key("Microphone"), None); // case-sensitive
    }

    #[test]
    fn deep_links_target_the_right_panes() {
        assert!(Permission::Microphone
            .settings_pane_url()
            .ends_with("Privacy_Microphone"));
        // The non-obvious one: Input Monitoring's anchor is ListenEvent.
        assert!(Permission::InputMonitoring
            .settings_pane_url()
            .ends_with("Privacy_ListenEvent"));
        assert!(Permission::Accessibility
            .settings_pane_url()
            .ends_with("Privacy_Accessibility"));
        assert!(Permission::ScreenRecording
            .settings_pane_url()
            .ends_with("Privacy_ScreenCapture"));
    }

    #[test]
    fn deep_links_use_the_systempreferences_scheme() {
        for p in [
            Permission::Microphone,
            Permission::InputMonitoring,
            Permission::Accessibility,
            Permission::ScreenRecording,
        ] {
            assert!(p
                .settings_pane_url()
                .starts_with("x-apple.systempreferences:com.apple.preference.security?Privacy_"));
        }
    }

    #[test]
    fn av_status_mapping_covers_apples_enum() {
        assert_eq!(map_av_status(0), PermissionState::NotDetermined);
        assert_eq!(map_av_status(1), PermissionState::Restricted);
        assert_eq!(map_av_status(2), PermissionState::Denied);
        assert_eq!(map_av_status(3), PermissionState::Granted);
        // Defensive: an out-of-range value is treated as "ask again".
        assert_eq!(map_av_status(99), PermissionState::NotDetermined);
    }

    #[test]
    fn iohid_access_mapping() {
        assert_eq!(map_iohid_access(0), PermissionState::Granted);
        assert_eq!(map_iohid_access(1), PermissionState::Denied);
        assert_eq!(map_iohid_access(2), PermissionState::NotDetermined);
        assert_eq!(map_iohid_access(7), PermissionState::NotDetermined);
    }

    #[test]
    fn bool_mapping_treats_non_granted_as_not_determined() {
        assert_eq!(map_bool(true), PermissionState::Granted);
        assert_eq!(map_bool(false), PermissionState::NotDetermined);
    }

    #[test]
    fn unsupported_payload_is_all_unsupported() {
        let s = PermissionStatuses::unsupported();
        assert_eq!(s.microphone, PermissionState::Unsupported);
        assert_eq!(s.input_monitoring, PermissionState::Unsupported);
        assert_eq!(s.accessibility, PermissionState::Unsupported);
        assert_eq!(s.screen_recording, PermissionState::Unsupported);
    }
}
