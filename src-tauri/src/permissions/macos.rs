//! macOS status-query FFI for the four onboarding permissions
//! (ADR 0061 / mb-mac-v1.4.6).
//!
//! Each function is a **silent preflight** — it reads the current TCC
//! grant without showing a prompt. The raw status codes are mapped into
//! the cross-platform [`PermissionState`] by the pure functions in
//! [`super`] (which carry the unit tests).
//!
//! Framework linking follows the raw-FFI discipline established in Wave
//! B2 (Carbon / accessibility-sys): we link the relevant Apple
//! frameworks directly rather than pulling a binding crate per API.

#![cfg(target_os = "macos")]

use objc2::{class, msg_send};
use objc2_foundation::NSString;

use super::{map_av_status, map_bool, map_iohid_access, PermissionState, PermissionStatuses};

// --- Microphone: AVFoundation -----------------------------------------------

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    /// `AVMediaTypeAudio` — the `NSString` media-type constant passed to
    /// `+[AVCaptureDevice authorizationStatusForMediaType:]`.
    static AVMediaTypeAudio: &'static NSString;
}

/// Microphone authorization via `AVCaptureDevice` (does NOT prompt).
fn microphone_state() -> PermissionState {
    // SAFETY: `AVCaptureDevice` is provided by AVFoundation (linked via
    // the `AVMediaTypeAudio` static below). `authorizationStatusForMediaType:`
    // is a class method that returns an `AVAuthorizationStatus`
    // (`NSInteger`) and performs no prompt or mutation.
    let raw: i64 = unsafe {
        let cls = class!(AVCaptureDevice);
        msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio]
    };
    map_av_status(raw)
}

// --- Input Monitoring: IOKit ------------------------------------------------

// `kIOHIDRequestTypeListenEvent` — we listen for key events in the
// global hotkey tap, so this is the request type that matters.
const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    /// `IOHIDCheckAccess` — returns an `IOHIDAccessType` for the given
    /// request type without prompting.
    fn IOHIDCheckAccess(request_type: u32) -> u32;
}

/// Input Monitoring grant via `IOHIDCheckAccess` (does NOT prompt).
fn input_monitoring_state() -> PermissionState {
    // SAFETY: `IOHIDCheckAccess` is a pure read of the current grant for
    // the given request type; it returns an `IOHIDAccessType` enum value.
    let raw = unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
    map_iohid_access(raw)
}

// --- Accessibility: ApplicationServices (via accessibility-sys) -------------

/// Accessibility trust via `AXIsProcessTrusted` (does NOT prompt — we
/// use the non-option variant precisely so it stays silent).
fn accessibility_state() -> PermissionState {
    // SAFETY: `AXIsProcessTrusted` reads the current trust state and
    // returns a `bool` (confirmed by Wave B2). No prompt, no mutation.
    let trusted = unsafe { accessibility_sys::AXIsProcessTrusted() };
    map_bool(trusted)
}

// --- Screen Recording: CoreGraphics -----------------------------------------

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// `CGPreflightScreenCaptureAccess` — returns whether this process
    /// currently has Screen Recording access, WITHOUT prompting (the
    /// `CGRequestScreenCaptureAccess` variant is the one that prompts).
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// Screen Recording grant via `CGPreflightScreenCaptureAccess`.
fn screen_recording_state() -> PermissionState {
    // SAFETY: preflight is a silent read returning a `bool`.
    let granted = unsafe { CGPreflightScreenCaptureAccess() };
    map_bool(granted)
}

/// Read all four permission states. Each read is independent and silent;
/// a failure mode would be a framework symbol going missing, which would
/// fail to *link*, not at runtime — so there is nothing to fall back on
/// here beyond the per-API mapping.
pub fn query_all() -> PermissionStatuses {
    PermissionStatuses {
        microphone: microphone_state(),
        input_monitoring: input_monitoring_state(),
        accessibility: accessibility_state(),
        screen_recording: screen_recording_state(),
    }
}
