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

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::runtime::Bool;
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

/// How long we'll block waiting for the user to answer the TCC prompt
/// before falling back to a fresh silent read. The prompt has no OS
/// timeout, but we don't want a (theoretically impossible) wedged
/// completion handler to hang the Tauri command thread forever.
const MIC_PROMPT_WAIT: Duration = Duration::from_secs(120);

/// **Explicitly request** microphone access — the ONLY path that pops the
/// system TCC prompt (and thereby registers Mockingbird in the System
/// Settings -> Privacy -> Microphone list). The silent
/// [`microphone_state`] preflight never prompts, and macOS refuses to let
/// the user add an app to the Microphone pane manually — so an app that
/// never *requests* is unreachable. This is that request.
///
/// Behaviour by current status:
///   - `NotDetermined` -> shows the prompt, blocks until the user answers,
///     then returns the resulting `Granted`/`Denied` state.
///   - already decided (`Granted`/`Denied`/`Restricted`) -> no prompt is
///     shown (macOS won't re-prompt); returns the current state so the UI
///     can fall back to opening the Settings pane when denied.
pub fn request_microphone_access() -> PermissionState {
    let current = microphone_state();
    // macOS only shows the prompt from a NotDetermined state; once the
    // user has decided, `requestAccess` fires the handler immediately with
    // the existing grant and never re-prompts. Short-circuit so we don't
    // needlessly block, and so the UI gets the actionable state.
    if !matches!(current, PermissionState::NotDetermined) {
        tracing::info!(
            target: "permissions",
            ?current,
            "mic request: already decided; not prompting (macOS won't re-prompt) — UI falls back to Settings when denied"
        );
        return current;
    }

    let (tx, rx) = mpsc::channel::<bool>();
    tracing::info!(
        target: "permissions",
        "mic request: NotDetermined -> dispatching requestAccess to the MAIN thread to show the TCC prompt"
    );

    // mb-47k: the TCC prompt can ONLY be presented from the AppKit main
    // thread / main run-loop. Tauri command handlers run off the main
    // thread (async command thread-pool), so calling `requestAccess`
    // directly here resolves `denied` WITHOUT ever showing the prompt.
    // Hop onto the main dispatch queue; the completion handler still fires
    // on an arbitrary queue and wakes us over the channel. Only the
    // `Send` `tx` crosses the boundary — the non-`Send` `RcBlock` is built
    // and used entirely on the main thread inside the closure.
    DispatchQueue::main().exec_async(move || {
        let handler = RcBlock::new(move |granted: Bool| {
            let granted = granted.as_bool();
            tracing::info!(target: "permissions", granted, "mic request: completion handler fired");
            let _ = tx.send(granted);
        });

        // SAFETY: `+[AVCaptureDevice requestAccessForMediaType:completionHandler:]`
        // is a class method that (a) shows the TCC prompt when status is
        // NotDetermined and (b) invokes the passed block exactly once with
        // the resulting `BOOL`. AVFoundation copies (retains) the block
        // synchronously, so dropping our `RcBlock` handle at end of scope
        // is sound. `AVMediaTypeAudio` is the linked AVFoundation
        // media-type constant also used by the preflight above.
        unsafe {
            let cls = class!(AVCaptureDevice);
            let _: () = msg_send![
                cls,
                requestAccessForMediaType: AVMediaTypeAudio,
                completionHandler: &*handler,
            ];
        }
    });

    // Block (on this off-main command thread) until the user answers or
    // the safety timeout elapses; either way, return the fresh
    // authoritative status.
    match rx.recv_timeout(MIC_PROMPT_WAIT) {
        Ok(granted) => {
            tracing::info!(target: "permissions", granted, "mic request: user answered the prompt")
        }
        Err(_) => tracing::warn!(
            target: "permissions",
            timeout_secs = MIC_PROMPT_WAIT.as_secs(),
            "mic request: timed out waiting for the TCC answer; re-reading status"
        ),
    }
    let result = microphone_state();
    tracing::info!(target: "permissions", ?result, "mic request: final status after prompt");
    result
}

/// Fire the microphone TCC prompt **without blocking** — used on the
/// audio-capture path so the very first dictation/meeting naturally pops
/// the prompt (belt-and-suspenders alongside the user-initiated panel
/// button). No-op once the grant has been decided, so it never spams.
pub fn prompt_microphone_access_async() {
    if !matches!(microphone_state(), PermissionState::NotDetermined) {
        return;
    }
    tracing::info!(
        target: "permissions",
        "mic capture-path prompt: NotDetermined -> dispatching requestAccess to the MAIN thread"
    );
    // mb-47k: this fires from `CpalCapture::start` on the cpal audio
    // thread — NOT the main thread — so it has the same main-thread
    // requirement as `request_microphone_access`. Hop onto the main queue
    // so the prompt can actually display; we just don't block for the
    // answer here (the panel button is the blocking, user-initiated path).
    DispatchQueue::main().exec_async(move || {
        let handler = RcBlock::new(move |granted: Bool| {
            tracing::info!(
                target: "permissions",
                granted = granted.as_bool(),
                "mic capture-path prompt: answered"
            );
        });
        // SAFETY: identical contract to `request_microphone_access`; we
        // simply don't wait for the completion handler. AVFoundation
        // retains the block synchronously, so dropping our handle at end
        // of scope is sound.
        unsafe {
            let cls = class!(AVCaptureDevice);
            let _: () = msg_send![
                cls,
                requestAccessForMediaType: AVMediaTypeAudio,
                completionHandler: &*handler,
            ];
        }
    });
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
