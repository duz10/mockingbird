//! macOS implementation of [`super::Injector`] (ADR 0058, mb-mac-v1.4.2).
//!
//! The macOS analogue of `windows.rs`. Two live strategies (per
//! [`InjectionStrategy`]) plus a retained-but-off-path clipboard path:
//!
//! - **Keystroke (macOS DEFAULT, ADR 0069)**: types the text directly
//!   via `CGEventKeyboardSetUnicodeString` (no clipboard) — the macOS
//!   twin of the Windows `KEYEVENTF_UNICODE` path. On macOS BOTH the
//!   `Paste` and `Keystroke` strategies route here, because Cmd+V is a
//!   pasteboard *read* the `changeCount` guard is structurally blind to
//!   (a read never bumps `changeCount`), so clipboard-paste races the
//!   target's read and pastes the user's STALE clipboard on the release
//!   `.app` (mb-yxs / mb-22y — two failed timing attempts before this).
//!   Synthesized keystrokes need no clipboard at all: no save/restore,
//!   no race, and the user's clipboard is never disturbed.
//! - **Abort**: [`InjectionOutcome::AbortedUserOptOut`]. No OS calls.
//! - **Paste (retained, OFF the default path)**:
//!   [`paste::with_saved_clipboard`] saves the pasteboard, writes the
//!   payload, synthesizes **Cmd+V** via `CGEventCreateKeyboardEvent`
//!   posted to the HID event tap, then restores the pasteboard. All
//!   pasteboard writes live in `paste.rs` per PLAN §12 #17. Kept for
//!   reference / a possible future opt-in; NOT reached by the default
//!   macOS dictation path (see [`MacInjector::inject`]).
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

use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// macOS virtual keycode for the `V` key (`kVK_ANSI_V`). Stable across
/// every macOS release — part of the ANSI keyboard layout constants.
const KVK_ANSI_V: u16 = 0x09;

/// Max UTF-16 code units per `CGEventKeyboardSetUnicodeString` event.
///
/// The API silently truncates long payloads (the practical ceiling is
/// ~20 UTF-16 units per event), so a full transcript posted in a single
/// event would lose everything past the first ~20 chars. We chunk well
/// under that ceiling and post one event per chunk.
const MAX_UTF16_UNITS_PER_EVENT: usize = 16;

/// Delay between posted chunks so fast-consuming apps (terminals,
/// Electron surfaces) don't drop characters when several unicode events
/// land in the same run-loop turn. Conservative — a typical ~60-char
/// transcript is ~4 chunks, adding ~12ms total. Tune up (not down) if a
/// specific app ever drops characters.
const INTER_CHUNK_DELAY: Duration = Duration::from_millis(4);

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
            // ADR 0069: on macOS BOTH Paste and Keystroke route through
            // synthesized keystrokes. The clipboard-paste path
            // (`paste_path` / `post_cmd_v`) is retained for reference /
            // future opt-in but is deliberately OFF the default
            // dictation path — Cmd+V is a pasteboard *read* the
            // `changeCount` guard can't observe, so it races the target
            // and pastes the user's stale clipboard on the release
            // `.app` (mb-yxs / mb-22y). Keystrokes need no clipboard.
            InjectionStrategy::Paste | InjectionStrategy::Keystroke => keystroke_path(text),
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
///
/// The payload is split into [`MAX_UTF16_UNITS_PER_EVENT`]-sized chunks
/// ([`chunk_utf16`]) because a single event silently truncates past
/// ~20 UTF-16 units. Each chunk is one key-down/key-up pair, paced by
/// [`INTER_CHUNK_DELAY`] so fast apps don't drop characters.
fn type_unicode_string(text: &str) -> AppResult<()> {
    let chunks = chunk_utf16(text, MAX_UTF16_UNITS_PER_EVENT);
    let last = chunks.len().saturating_sub(1);
    for (i, chunk) in chunks.iter().enumerate() {
        post_unicode_chunk(chunk)?;
        if i != last {
            std::thread::sleep(INTER_CHUNK_DELAY);
        }
    }
    Ok(())
}

/// Post a single chunk (<= [`MAX_UTF16_UNITS_PER_EVENT`] UTF-16 units)
/// as one key-down/key-up pair via `CGEventKeyboardSetUnicodeString`.
fn post_unicode_chunk(chunk: &str) -> AppResult<()> {
    let source = new_event_source()?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), 0, true)
        .map_err(|()| AppError::Injection("CGEvent keystroke key-down create failed".into()))?;
    key_down.set_string(chunk);
    key_down.post(CGEventTapLocation::HID);

    let key_up = CGEvent::new_keyboard_event(source, 0, false)
        .map_err(|()| AppError::Injection("CGEvent keystroke key-up create failed".into()))?;
    key_up.set_string(chunk);
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Split `text` into chunks each at most `max_units` UTF-16 code units,
/// never splitting a `char` (so a surrogate pair stays intact within a
/// single event). Pure + unit-testable — no OS calls.
///
/// A `char` is at most 2 UTF-16 units, so with `max_units >= 2` every
/// char fits; the only way a chunk exceeds `max_units` is the trivial
/// `max_units < 2` case, which callers never use.
fn chunk_utf16(text: &str, max_units: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_units = 0usize;
    for ch in text.chars() {
        let units = ch.len_utf16();
        if current_units + units > max_units && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        current.push(ch);
        current_units += units;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
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

    #[test]
    fn empty_paste_routes_to_keystroke_and_is_a_noop() {
        // ADR 0069: macOS routes the Paste strategy through the
        // keystroke path. For empty text this returns Ok WITHOUT
        // touching the clipboard (the old paste_path would have run a
        // full save/restore on the pasteboard even for empty text).
        let injector = MacInjector::new().expect("construct");
        let outcome = injector
            .inject("", InjectionStrategy::Paste)
            .expect("empty paste is a no-op on macOS keystroke path");
        assert_eq!(outcome, InjectionOutcome::Ok);
    }

    #[test]
    fn chunk_utf16_empty_yields_no_chunks() {
        assert!(chunk_utf16("", MAX_UTF16_UNITS_PER_EVENT).is_empty());
    }

    #[test]
    fn chunk_utf16_short_string_is_single_chunk() {
        let chunks = chunk_utf16("hello", MAX_UTF16_UNITS_PER_EVENT);
        assert_eq!(chunks, vec!["hello".to_string()]);
    }

    #[test]
    fn chunk_utf16_splits_on_the_unit_boundary() {
        // 40 ASCII chars, max 16 units/chunk -> 16 + 16 + 8.
        let text = "a".repeat(40);
        let chunks = chunk_utf16(&text, 16);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 16);
        assert_eq!(chunks[1].len(), 16);
        assert_eq!(chunks[2].len(), 8);
        // Lossless: concatenation reconstructs the original exactly.
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_utf16_never_splits_a_surrogate_pair() {
        // Each emoji is 2 UTF-16 units. With max=3, only one emoji fits
        // per chunk (2 units; a second would be 4 > 3), so we get one
        // chunk per emoji and no chunk ever exceeds max_units.
        let text = "\u{1F600}\u{1F601}\u{1F602}"; // 3 emoji, 6 UTF-16 units
        let chunks = chunk_utf16(text, 3);
        assert_eq!(chunks.len(), 3);
        for c in &chunks {
            let units: usize = c.chars().map(char::len_utf16).sum();
            assert!(
                units <= 3,
                "chunk exceeded max_units: {c:?} ({units} units)"
            );
            // Each chunk is a whole, valid char (no split surrogate).
            assert_eq!(c.chars().count(), 1);
        }
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_utf16_mixed_ascii_and_wide_is_lossless() {
        let text = "caf\u{00E9} \u{1F44D} na\u{00EF}ve r\u{00E9}sum\u{00E9}";
        let chunks = chunk_utf16(text, MAX_UTF16_UNITS_PER_EVENT);
        assert_eq!(chunks.concat(), text, "chunking must be lossless");
        for c in &chunks {
            let units: usize = c.chars().map(char::len_utf16).sum();
            assert!(units <= MAX_UTF16_UNITS_PER_EVENT);
        }
    }
}
