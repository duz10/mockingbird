//! Strategy + focus-loss decision wiring.
//!
//! `strategy.rs` is the pure resolver (process name → strategy). This
//! file is the **orchestrator-side glue** that combines:
//!
//! - The key-down [`ForegroundWindow`] snapshot (stashed by the
//!   orchestrator when [`super::super::hotkey::state::StateAction::StartCapture`]
//!   fires).
//! - The key-up [`ForegroundWindow`] snapshot (taken right before
//!   the injection decision).
//! - The user's per-app strategy overrides (settings.toml).
//!
//! …into a single decision: do we paste, keystroke, or abort —
//! and if abort, with which [`InjectionOutcome`]?
//!
//! ## Focus-loss double-snapshot (ADR 0016 §7)
//!
//! If the foreground process changed between key-down and key-up,
//! we MUST NOT inject into the new app — they didn't ask for the
//! transcript. The raw audio + transcript still persists for
//! provenance, but `injection_status = aborted_focus_changed`.
//!
//! Comparison is on `process_name` only — not HWND, not title.
//! Reasons:
//! - HWND changes whenever a window is recreated (browser tab reload).
//! - Title changes mid-session (VSCode dirty-marker, file rename).
//! - process_name is stable for the user's identification of the app.
//!
//! ## Pure decision
//!
//! [`decide_injection`] is pure: same inputs → same output.
//! Unit-testable with synthesised [`ForegroundWindow`]s + override
//! maps; no OS calls.

use std::collections::HashMap;

use super::strategy::{resolve, InjectionStrategy};
use super::InjectionOutcome;
use crate::window_context::ForegroundWindow;

/// The outcome of [`decide_injection`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectionDecision {
    /// Proceed with the given strategy.
    Proceed(InjectionStrategy),
    /// Skip injection; persist as `aborted_focus_changed` (user
    /// alt-tabbed mid-session).
    AbortFocusChanged,
    /// Skip injection; persist as `aborted_user_opt_out` (strategy
    /// resolved to [`InjectionStrategy::Abort`] — password manager,
    /// anti-cheat, etc.).
    AbortUserOptOut,
}

impl InjectionDecision {
    /// Convenience: map directly to the [`InjectionOutcome`] the
    /// orchestrator will persist when this decision is the FINAL
    /// outcome (no further work). For [`Self::Proceed`] this returns
    /// `None` since the actual injection still has to happen and the
    /// injector reports its own outcome.
    pub fn final_outcome(&self) -> Option<InjectionOutcome> {
        match self {
            Self::Proceed(_) => None,
            Self::AbortFocusChanged => Some(InjectionOutcome::AbortedFocusChanged),
            Self::AbortUserOptOut => Some(InjectionOutcome::AbortedUserOptOut),
        }
    }
}

/// Decide what to do given the two foreground snapshots and the user's
/// per-app strategy overrides.
///
/// `fg_keydown` may be `None` if the orchestrator missed the snapshot
/// (transient null-foreground state at key-down — rare). In that
/// case we conservatively allow the injection — the user clearly
/// intended SOMETHING; missing provenance shouldn't block their work.
///
/// `fg_keyup` is required — if there's no foreground at key-up,
/// there's nothing to inject into and the caller should `AbortFocusChanged`.
pub fn decide_injection(
    fg_keydown: Option<&ForegroundWindow>,
    fg_keyup: &ForegroundWindow,
    user_overrides: &HashMap<String, InjectionStrategy>,
) -> InjectionDecision {
    // 1. Focus-loss check.
    if let Some(prev) = fg_keydown {
        if !same_process(prev, fg_keyup) {
            return InjectionDecision::AbortFocusChanged;
        }
    }
    // 2. Strategy resolution.
    match resolve(&fg_keyup.process_name, user_overrides) {
        InjectionStrategy::Abort => InjectionDecision::AbortUserOptOut,
        other @ (InjectionStrategy::Paste | InjectionStrategy::Keystroke) => {
            InjectionDecision::Proceed(other)
        }
    }
}

/// Compare two foreground snapshots for "same app" semantics.
///
/// Case-insensitive on `process_name` — Win32 process names come in
/// a mix of casings depending on how the EXE was launched
/// (`Code.exe` vs `code.exe`).
pub fn same_process(a: &ForegroundWindow, b: &ForegroundWindow) -> bool {
    a.process_name.eq_ignore_ascii_case(&b.process_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fg(process_name: &str) -> ForegroundWindow {
        ForegroundWindow {
            hwnd: 0x100,
            title: "Some Title".into(),
            process_name: process_name.into(),
            exe_path: Some(format!(r"C:\path\{process_name}")),
        }
    }

    #[test]
    fn proceed_when_same_app_and_strategy_is_paste() {
        let prev = fg("notepad.exe");
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        assert_eq!(
            decide_injection(Some(&prev), &now, &overrides),
            InjectionDecision::Proceed(InjectionStrategy::Paste)
        );
    }

    #[test]
    fn proceed_with_keystroke_for_terminal() {
        let now = fg("WindowsTerminal.exe");
        let overrides = HashMap::new();
        assert_eq!(
            decide_injection(Some(&now.clone()), &now, &overrides),
            InjectionDecision::Proceed(InjectionStrategy::Keystroke)
        );
    }

    #[test]
    fn abort_user_opt_out_for_password_manager() {
        let now = fg("1Password.exe");
        let overrides = HashMap::new();
        let d = decide_injection(Some(&now.clone()), &now, &overrides);
        assert_eq!(d, InjectionDecision::AbortUserOptOut);
        assert_eq!(d.final_outcome(), Some(InjectionOutcome::AbortedUserOptOut));
    }

    #[test]
    fn abort_focus_changed_when_process_differs() {
        let prev = fg("notepad.exe");
        let now = fg("chrome.exe");
        let overrides = HashMap::new();
        let d = decide_injection(Some(&prev), &now, &overrides);
        assert_eq!(d, InjectionDecision::AbortFocusChanged);
        assert_eq!(
            d.final_outcome(),
            Some(InjectionOutcome::AbortedFocusChanged)
        );
    }

    #[test]
    fn case_insensitive_same_process_match() {
        // Code.exe vs code.exe is "same app".
        let prev = fg("Code.exe");
        let now = fg("code.exe");
        let overrides = HashMap::new();
        assert_eq!(
            decide_injection(Some(&prev), &now, &overrides),
            InjectionDecision::Proceed(InjectionStrategy::Paste)
        );
    }

    #[test]
    fn missing_keydown_snapshot_falls_through_to_strategy() {
        // No focus-loss check possible; strategy resolution still runs.
        let now = fg("notepad.exe");
        let overrides = HashMap::new();
        assert_eq!(
            decide_injection(None, &now, &overrides),
            InjectionDecision::Proceed(InjectionStrategy::Paste)
        );
    }

    #[test]
    fn user_override_wins_over_builtin() {
        // User has configured notepad.exe as Keystroke.
        let now = fg("notepad.exe");
        let mut overrides = HashMap::new();
        overrides.insert("notepad.exe".into(), InjectionStrategy::Keystroke);
        assert_eq!(
            decide_injection(Some(&now.clone()), &now, &overrides),
            InjectionDecision::Proceed(InjectionStrategy::Keystroke)
        );
    }

    #[test]
    fn user_override_to_abort_blocks_injection_into_otherwise_normal_app() {
        // User has marked Notepad as Abort (don't trust me to dictate
        // into Notepad).
        let now = fg("notepad.exe");
        let mut overrides = HashMap::new();
        overrides.insert("notepad.exe".into(), InjectionStrategy::Abort);
        assert_eq!(
            decide_injection(Some(&now.clone()), &now, &overrides),
            InjectionDecision::AbortUserOptOut
        );
    }

    #[test]
    fn final_outcome_returns_none_for_proceed() {
        // Proceed doesn't have a final outcome — the injector reports it.
        let d = InjectionDecision::Proceed(InjectionStrategy::Paste);
        assert_eq!(d.final_outcome(), None);
    }

    #[test]
    fn same_process_helper_is_case_insensitive() {
        let a = fg("Chrome.exe");
        let b = fg("chrome.EXE");
        assert!(same_process(&a, &b));
    }

    #[test]
    fn same_process_distinguishes_different_apps() {
        let a = fg("chrome.exe");
        let b = fg("firefox.exe");
        assert!(!same_process(&a, &b));
    }
}
