//! Per-app injection-strategy resolution (ADR 0016).
//!
//! **Skeleton in Wave 1.** The `phf::Map` static table + user-override
//! merge logic land in Wave 2 (bd `mb-7xs`). Wave 4 extends with
//! foreground-process wiring + focus-loss double-snapshot (bd
//! `mb-3yn`).
//!
//! ## Type contract
//!
//! [`InjectionStrategy`] is defined here so callers (including the
//! orchestrator in Wave 4) can name it without depending on the full
//! resolution machinery. The variant set is sealed — adding a fourth
//! tier requires an ADR.

use serde::{Deserialize, Serialize};

/// Tier of injection mechanism resolved for a foreground app.
///
/// Resolution is performed by `resolve(...)` (Wave 2) against the
/// built-in `phf` table merged with user overrides from
/// `settings.injection.app_overrides`.
///
/// `Paste` is `#[default]` per ADR 0016 (Tier 0 — works in ~95% of
/// apps; unknown apps fall back to it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InjectionStrategy {
    /// Clipboard paste via Ctrl+V (default, ADR 0007 / 0016).
    #[default]
    Paste,
    /// Per-character `SendInput` with `KEYEVENTF_UNICODE`.
    Keystroke,
    /// Do not inject; abort with `injection_status = aborted_user_opt_out`.
    Abort,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_strategy_is_paste() {
        // PLAN §3 Tier 0 — ADR 0007 — ADR 0016.
        assert_eq!(InjectionStrategy::default(), InjectionStrategy::Paste);
    }

    #[test]
    fn strategy_serializes_lowercase() {
        // Settings file uses lowercase strings ("paste", "keystroke",
        // "abort"). Test guards against accidental case drift.
        let json = serde_json::to_string(&InjectionStrategy::Keystroke).unwrap();
        assert_eq!(json, "\"keystroke\"");
        let parsed: InjectionStrategy = serde_json::from_str("\"abort\"").unwrap();
        assert_eq!(parsed, InjectionStrategy::Abort);
    }
}
