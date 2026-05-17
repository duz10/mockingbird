#![allow(missing_docs)] // Trait + factory; method-level docs are the API.

//! Text injection — types the cleaned transcript into the focused app.
//!
//! Cross-platform via the [`Injector`] trait. Windows impl uses
//! `SendInput` (Ctrl+V for paste strategy; `KEYEVENTF_UNICODE` for
//! keystroke fallback); macOS / Linux are `todo!()` stubs per PLAN
//! §12 #15.
//!
//! ## Strategy resolution
//!
//! [`strategy::resolve`] maps a foreground process name to one of
//! [`InjectionStrategy::Paste`] / [`InjectionStrategy::Keystroke`] /
//! [`InjectionStrategy::Abort`]. Defaults to `Paste`. See ADR 0016
//! for the built-in table + the user-override convention.
//!
//! ## Clipboard discipline
//!
//! `injection/paste.rs` is the **only** file in this workspace that
//! is permitted to call `SetClipboardData`. The save/restore protocol
//! lives there (ADR 0018). The shell-side hook
//! `scripts/hooks/warn-bare-clipboard-set.py` flags violations.
//!
//! ## Secure input
//!
//! `secure_guard::SecureInputGuard::is_secure(&fg)` MUST return
//! `false` before any injection path opens the clipboard or calls
//! `SendInput`. PLAN §12 #18 is binding. ADR 0017 documents the
//! detection signals.

pub mod paste;
pub mod secure_guard;
pub mod strategy;
pub mod strategy_wiring;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(target_os = "windows"))]
use crate::error::AppError;
use crate::error::AppResult;

pub use strategy::InjectionStrategy;

/// Outcome of a single injection attempt — surfaced for the DB
/// `injection_status` column (PLAN provenance principle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionOutcome {
    /// Text reached the focused app; clipboard restored cleanly.
    Ok,
    /// Text reached the focused app; clipboard restore was skipped
    /// because another app wrote the clipboard mid-paste.
    OkClipboardNotRestored,
    /// Secure-input field detected — nothing was pasted, no
    /// clipboard write occurred. Raw transcript still persisted.
    AbortedSecure,
    /// Per-app strategy table or user override said "do not paste
    /// into this app" (password managers, anti-cheat).
    AbortedUserOptOut,
    /// Focus changed between key-down and key-up — **legacy** variant.
    ///
    /// Per ADR 0020 (Wave 4.9) the default pipeline no longer emits
    /// this outcome: focus change is permissive and injection
    /// proceeds into the key-up app. The variant + DB string
    /// `"aborted_focus_changed"` are retained because pre-4.9
    /// session rows in users' databases use it, and the schema's
    /// CHECK constraint still lists it. A future opt-in "strict"
    /// focus mode could re-emit it.
    AbortedFocusChanged,
    /// Clipboard was locked by another process for >3 retries.
    FailedClipboardLocked,
    /// `SendInput` returned 0 events written (anti-cheat block, etc).
    FailedSendInput,
}

impl InjectionOutcome {
    /// Map to the canonical string written to `sessions.injection_status`.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::OkClipboardNotRestored => "ok_clipboard_not_restored",
            Self::AbortedSecure => "aborted_secure",
            Self::AbortedUserOptOut => "aborted_user_opt_out",
            Self::AbortedFocusChanged => "aborted_focus_changed",
            Self::FailedClipboardLocked => "failed_clipboard_locked",
            Self::FailedSendInput => "failed_send_input",
        }
    }
}

/// Inject the given text into the currently-focused application.
///
/// **Caller responsibilities (in this order):**
/// 1. Resolve the strategy via [`strategy::resolve`].
/// 2. Confirm `SecureInputGuard::is_secure(&fg)` is `false`.
/// 3. Call [`Injector::inject`].
///
/// The trait does **not** enforce step 2 — callers must. This is
/// intentional: the orchestrator owns the abort decision (and the
/// associated DB write + tray toast), not the injector.
pub trait Injector: Send {
    /// Inject `text` using the given strategy.
    fn inject(&self, text: &str, strategy: InjectionStrategy) -> AppResult<InjectionOutcome>;
}

/// Construct the platform-default injector.
pub fn make_default_injector() -> AppResult<Box<dyn Injector>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::SendInputInjector::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Injection(
            "injector not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_db_strings_are_stable() {
        // These strings end up in the sessions.injection_status column
        // and become part of the persisted provenance — changing them
        // is a schema break. This test guards against accidental
        // rewordings.
        assert_eq!(InjectionOutcome::Ok.as_db_str(), "ok");
        assert_eq!(
            InjectionOutcome::AbortedSecure.as_db_str(),
            "aborted_secure"
        );
        assert_eq!(
            InjectionOutcome::AbortedFocusChanged.as_db_str(),
            "aborted_focus_changed"
        );
        assert_eq!(
            InjectionOutcome::FailedClipboardLocked.as_db_str(),
            "failed_clipboard_locked"
        );
    }
}
