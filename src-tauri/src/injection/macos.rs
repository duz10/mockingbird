//! macOS implementation of [`super::Injector`] (ADR 0058, mb-mac-v1.4.2).
//!
//! The macOS analogue of `windows.rs`. Three strategies (per
//! [`InjectionStrategy`]):
//!
//! - **Paste**: [`paste::with_saved_clipboard`] saves the pasteboard,
//!   writes the payload, synthesizes **Cmd+V** via
//!   `CGEventCreateKeyboardEvent` posted to the HID event tap, then
//!   restores the pasteboard. All pasteboard writes live in `paste.rs`
//!   per PLAN §12 #17 — `macos.rs` supplies ONLY the keypress.
//! - **Keystroke**: types the text directly via
//!   `CGEventKeyboardSetUnicodeString` (no clipboard) — the macOS twin
//!   of the Windows `KEYEVENTF_UNICODE` path. Used for apps the
//!   strategy table flags as paste-hostile.
//! - **Abort**: [`InjectionOutcome::AbortedUserOptOut`]. No OS calls.
//!
//! ## Secure input (coupling with Leaf 2, ADR 0059)
//!
//! The injector itself does **not** consult the secure-input guard —
//! exactly like `windows.rs`. Parity with the Windows design: the
//! orchestrator (`dictation.rs`) owns the `is_secure` → abort → toast
//! decision (`Decision::Abort(AbortedSecure)`). [`inject_secure_guarded`]
//! is the macOS coupling seam the future runtime-wiring leaf calls — it
//! consults the guard BEFORE any clipboard write or keypost and returns
//! [`InjectionOutcome::AbortedSecure`] for a focused password field.
//!
//! ## Accessibility permission
//!
//! Posting synthetic events requires the **Accessibility** grant. When
//! it is not granted the OS silently swallows the keypost (no error is
//! returned), so we cannot detect failure at the call site — we log a
//! clear warning and proceed. The clipboard save/restore still runs
//! without the grant; only the keypost is gated. The real keypost is
//! exercised in the permission-gated `mac-p3-dictation-e2e`.

#![cfg(target_os = "macos")]
#![allow(dead_code)] // Runtime wiring for macOS lands in a later leaf.

use super::paste::{self, PasteOutcome};
use super::secure_guard::SecureInputGuard;
use super::{InjectionOutcome, InjectionStrategy, Injector};
use crate::error::{AppError, AppResult};
use crate::window_context::ForegroundWindow;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// macOS virtual keycode for the `V` key (`kVK_ANSI_V`). Stable across
/// every macOS release — part of the ANSI keyboard layout constants.
const KVK_ANSI_V: u16 = 0x09;

/// CoreGraphics-based injector. Stateless — every `inject()` call is
/// independent (matches the Windows `SendInputInjector`).
#[derive(Default)]
pub struct MacInjector;

impl MacInjector {
    /// Construct an injector. No OS resources are acquired.
    pub fn new() -> AppResult<Self> {
        Ok(Self)
    }
}

impl Injector for MacInjector {
    fn inject(&self, text: &str, strategy: InjectionStrategy) -> AppResult<InjectionOutcome> {
        match strategy {
            InjectionStrategy::Abort => Ok(InjectionOutcome::AbortedUserOptOut),
            InjectionStrategy::Paste => paste_path(text),
            InjectionStrategy::Keystroke => keystroke_path(text),
        }
    }
}

/// macOS coupling of Leaf 1 (paste) + Leaf 2 (secure guard).
///
/// Consults `guard` BEFORE any clipboard write or keypost; returns
/// [`InjectionOutcome::AbortedSecure`] without touching the pasteboard
/// when a secure field is focused. This is the macOS analogue of the
/// Windows orchestrator's pre-inject `is_secure` check in `dictation.rs`
/// (`Decision::Abort(AbortedSecure)`), which is what fires the user
/// toast + persists the raw transcript. The future macOS
/// runtime-wiring leaf calls this instead of [`Injector::inject`]
/// directly, so the "never silently inject into a password field"
/// principle holds on macOS.
pub fn inject_secure_guarded(
    injector: &dyn Injector,
    guard: &dyn SecureInputGuard,
    fg: &ForegroundWindow,
    text: &str,
    strategy: InjectionStrategy,
) -> AppResult<InjectionOutcome> {
    if guard.is_secure(fg) {
        tracing::info!(
            "macOS secure-input field focused; aborting injection \
             (no clipboard write, no keypost)"
        );
        return Ok(InjectionOutcome::AbortedSecure);
    }
    injector.inject(text, strategy)
}

// --------------------------------------------------------------------
// Strategy paths
// --------------------------------------------------------------------

fn paste_path(text: &str) -> AppResult<InjectionOutcome> {
    warn_if_accessibility_missing("Cmd+V keypost");
    let outcome = paste::with_saved_clipboard(text, post_cmd_v)?;
    Ok(match outcome {
        PasteOutcome::Ok => InjectionOutcome::Ok,
        PasteOutcome::OkClipboardNotRestored => InjectionOutcome::OkClipboardNotRestored,
    })
}

fn keystroke_path(text: &str) -> AppResult<InjectionOutcome> {
    if text.is_empty() {
        return Ok(InjectionOutcome::Ok); // empty text is trivially "delivered"
    }
    warn_if_accessibility_missing("unicode keystroke");
    type_unicode_string(text)?;
    Ok(InjectionOutcome::Ok)
}

// --------------------------------------------------------------------
// OS shims — CoreGraphics event synthesis
// --------------------------------------------------------------------

/// Synthesize a Cmd+V key-down then key-up, posted to the HID event tap.
fn post_cmd_v() -> AppResult<()> {
    let source = new_event_source()?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), KVK_ANSI_V, true)
        .map_err(|()| AppError::Injection("CGEvent Cmd+V key-down create failed".into()))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    let key_up = CGEvent::new_keyboard_event(source, KVK_ANSI_V, false)
        .map_err(|()| AppError::Injection("CGEvent Cmd+V key-up create failed".into()))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Type `text` directly via `CGEventKeyboardSetUnicodeString`.
///
/// keycode `0` + a Unicode string payload is the documented way to
/// inject arbitrary text without per-key keymap translation — the
/// macOS analogue of the Windows `KEYEVENTF_UNICODE` per-char loop.
fn type_unicode_string(text: &str) -> AppResult<()> {
    let source = new_event_source()?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), 0, true)
        .map_err(|()| AppError::Injection("CGEvent keystroke key-down create failed".into()))?;
    key_down.set_string(text);
    key_down.post(CGEventTapLocation::HID);

    let key_up = CGEvent::new_keyboard_event(source, 0, false)
        .map_err(|()| AppError::Injection("CGEvent keystroke key-up create failed".into()))?;
    key_up.set_string(text);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

fn new_event_source() -> AppResult<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| AppError::Injection("CGEventSource::new(HIDSystemState) failed".into()))
}

// --------------------------------------------------------------------
// Accessibility permission
// --------------------------------------------------------------------

/// Whether this process currently holds the Accessibility (AX) grant.
///
/// Required for `CGEventPost` to actually reach the focused app. A
/// side-effect-free system query.
fn accessibility_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` has no preconditions and no
    // out-params; it reads the process's current TCC state.
    unsafe { accessibility_sys::AXIsProcessTrusted() }
}

/// Emit a one-line warning when the Accessibility grant is missing, so
/// the keypost-silently-ignored failure mode is visible in logs and to
/// the Wave C onboarding flow / e2e.
fn warn_if_accessibility_missing(what: &str) {
    if !accessibility_trusted() {
        tracing::warn!(
            "macOS Accessibility permission not granted; the synthesized {what} will be \
             ignored by the OS (the clipboard save/restore still runs). Grant under \
             System Settings → Privacy & Security → Accessibility."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_succeeds() {
        assert!(MacInjector::new().is_ok());
    }

    #[test]
    fn abort_strategy_yields_user_opt_out_with_no_os_calls() {
        let injector = MacInjector::new().expect("construct");
        let outcome = injector
            .inject("anything", InjectionStrategy::Abort)
            .expect("Abort never returns Err");
        assert_eq!(outcome, InjectionOutcome::AbortedUserOptOut);
    }

    #[test]
    fn empty_keystroke_is_trivially_delivered_without_os_calls() {
        let injector = MacInjector::new().expect("construct");
        let outcome = injector
            .inject("", InjectionStrategy::Keystroke)
            .expect("empty keystroke is a no-op");
        assert_eq!(outcome, InjectionOutcome::Ok);
    }
}
