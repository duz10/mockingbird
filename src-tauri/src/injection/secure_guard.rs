//! Secure-input detection (ADR 0017, PLAN §12 #18 binding).
//!
//! **Stub in Wave 1.** The Windows impl (three OR-combined signals:
//! `GetGUIThreadInfo(GUI_SECUREINPUT)` + class-name allowlist +
//! `ES_PASSWORD` style) lands in Wave 2 (bd `mb-tye`).
//!
//! The orchestrator in Wave 4 calls `is_secure(...)` **before** any
//! clipboard mutation or `SendInput` call. There is no path through
//! the orchestrator that reaches paste without the guard returning
//! `false` first — enforced by code structure, not by trust.

use crate::window_context::ForegroundWindow;

/// Detects whether the foreground window represents a "secure input"
/// surface where injection MUST abort.
///
/// Trait shape is locked in Wave 1; the production Windows impl
/// lands in Wave 2. Tests will substitute mock implementations.
pub trait SecureInputGuard: Send + Sync {
    /// Returns `true` if any signal indicates a secure field is
    /// focused. On `true` the orchestrator aborts injection,
    /// persists the raw transcript with `injection_status =
    /// aborted_secure`, and emits a tray toast.
    fn is_secure(&self, fg: &ForegroundWindow) -> bool;
}

/// Conservative stub guard for Wave 1 — never reports secure.
///
/// Replaced in Wave 2 by `WinSecureInputGuard`. Using it in
/// production would defeat the §12 #18 binding — the Wave 4
/// orchestrator must hold a `WinSecureInputGuard`, not this.
pub struct NeverSecureGuard;

impl SecureInputGuard for NeverSecureGuard {
    fn is_secure(&self, _fg: &ForegroundWindow) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_context::ForegroundWindow;

    #[test]
    fn never_secure_guard_returns_false() {
        let fg = ForegroundWindow {
            hwnd: 0,
            title: "Test".into(),
            process_name: "test.exe".into(),
            exe_path: None,
        };
        assert!(!NeverSecureGuard.is_secure(&fg));
    }
}
