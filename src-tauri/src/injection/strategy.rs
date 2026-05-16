//! Per-app injection-strategy resolution (ADR 0016).
//!
//! Wave 2 adds the `phf::Map` of builtin overrides + the `resolve()`
//! function that merges builtins with user-supplied overrides. Wave 4
//! wires this into the orchestrator with a real `ForegroundWindow`
//! probe + the focus-loss double-snapshot (bd `mb-3yn`).
//!
//! ## Resolution order (ADR 0016 §3)
//!
//! 1. User overrides (`settings.injection.app_overrides`) — explicit
//!    user choice wins. Lookup key is lowercased process basename.
//! 2. Builtin override table ([`BUILTIN_OVERRIDES`]) — apps known to
//!    misbehave with paste (terminals → Keystroke) or to be unsafe to
//!    inject into at all (password managers, kernel anti-cheat → Abort).
//! 3. Default — [`InjectionStrategy::Paste`].
//!
//! ## Why `phf`
//!
//! Static compile-time perfect hash table. Lookup is `O(1)` with no
//! runtime initialisation, no heap, and the data is `.rodata`-resident
//! so it costs no per-process allocation. The set will grow over time;
//! `HashMap::new()` at module load would be silly for ~12 entries that
//! never change.

use std::collections::HashMap;

use phf::phf_map;
use serde::{Deserialize, Serialize};

/// Tier of injection mechanism resolved for a foreground app.
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

/// Built-in per-app overrides (ADR 0016 §4).
///
/// Lookup key is the **lowercased** process basename (e.g.
/// `"windowsterminal.exe"`). Categories:
///
/// - **Terminals & shells (Keystroke):** consoles handle Ctrl+V
///   inconsistently across host (conhost vs Windows Terminal) and
///   shell (cmd vs PowerShell vs WSL). SendInput is the lowest common
///   denominator. ADR 0016 §4.1.
/// - **Password managers (Abort):** never autofill secrets via our
///   pipeline. The user pasting their own password is a feature; us
///   doing it for them via a transcribed audio command is a
///   security disaster. ADR 0017 + ADR 0016 §4.2.
/// - **Kernel anti-cheat (Abort):** Vanguard/EAC/BattlEye treat
///   synthetic input as cheating; SendInput from a non-game process
///   can trigger bans. Abort entirely. ADR 0016 §4.3.
pub static BUILTIN_OVERRIDES: phf::Map<&'static str, InjectionStrategy> = phf_map! {
    // --- Terminals & shells ---
    "windowsterminal.exe" => InjectionStrategy::Keystroke,
    "cmd.exe"             => InjectionStrategy::Keystroke,
    "powershell.exe"      => InjectionStrategy::Keystroke,
    "pwsh.exe"            => InjectionStrategy::Keystroke,
    "conhost.exe"         => InjectionStrategy::Keystroke,
    "wt.exe"              => InjectionStrategy::Keystroke,

    // --- Password managers ---
    "1password.exe"       => InjectionStrategy::Abort,
    "keepass.exe"         => InjectionStrategy::Abort,
    "keepassxc.exe"       => InjectionStrategy::Abort,
    "bitwarden.exe"       => InjectionStrategy::Abort,

    // --- Kernel-level anti-cheat ---
    "vanguard.exe"        => InjectionStrategy::Abort,
    "easyanticheat.exe"   => InjectionStrategy::Abort,
    "beservice.exe"       => InjectionStrategy::Abort,
};

/// Resolve the injection strategy for the foreground app.
///
/// `process_name` is the executable basename as reported by
/// `WindowContext::foreground()` (e.g. `"WindowsTerminal.exe"`). The
/// comparison is case-insensitive — Windows path lookups are
/// case-folded by the OS, and ADR 0016 codifies lowercase-on-lookup so
/// upstream casing drift never silently changes routing.
///
/// `user_overrides` should already have lowercased keys (the settings
/// loader does this at parse time, ADR 0016 §3.4).
pub fn resolve(
    process_name: &str,
    user_overrides: &HashMap<String, InjectionStrategy>,
) -> InjectionStrategy {
    let key = process_name.to_ascii_lowercase();
    if let Some(s) = user_overrides.get(&key) {
        return *s;
    }
    if let Some(s) = BUILTIN_OVERRIDES.get(key.as_str()) {
        return *s;
    }
    InjectionStrategy::Paste
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> HashMap<String, InjectionStrategy> {
        HashMap::new()
    }

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

    #[test]
    fn unknown_app_falls_back_to_paste() {
        assert_eq!(resolve("randomapp.exe", &empty()), InjectionStrategy::Paste);
    }

    #[test]
    fn terminal_resolves_to_keystroke() {
        assert_eq!(
            resolve("WindowsTerminal.exe", &empty()),
            InjectionStrategy::Keystroke
        );
        assert_eq!(resolve("cmd.exe", &empty()), InjectionStrategy::Keystroke);
        assert_eq!(
            resolve("powershell.exe", &empty()),
            InjectionStrategy::Keystroke
        );
        assert_eq!(resolve("pwsh.exe", &empty()), InjectionStrategy::Keystroke);
    }

    #[test]
    fn password_manager_resolves_to_abort() {
        assert_eq!(resolve("1Password.exe", &empty()), InjectionStrategy::Abort);
        assert_eq!(resolve("KeePassXC.exe", &empty()), InjectionStrategy::Abort);
        assert_eq!(resolve("Bitwarden.exe", &empty()), InjectionStrategy::Abort);
    }

    #[test]
    fn anticheat_resolves_to_abort() {
        assert_eq!(resolve("Vanguard.exe", &empty()), InjectionStrategy::Abort);
        assert_eq!(
            resolve("EasyAntiCheat.exe", &empty()),
            InjectionStrategy::Abort
        );
        assert_eq!(resolve("BEService.exe", &empty()), InjectionStrategy::Abort);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        // Windows reports basenames inconsistently across APIs
        // (GetModuleBaseNameW preserves case; some shell APIs
        // lowercase). The resolver MUST be insensitive.
        for spelling in [
            "WINDOWSTERMINAL.EXE",
            "windowsterminal.exe",
            "WindowsTerminal.exe",
            "wINdowsTERMinal.EXe",
        ] {
            assert_eq!(
                resolve(spelling, &empty()),
                InjectionStrategy::Keystroke,
                "case-insensitivity failed for spelling: {spelling}"
            );
        }
    }

    #[test]
    fn user_override_wins_over_builtin() {
        // User decides cmd.exe should paste — respect it.
        let mut user = HashMap::new();
        user.insert("cmd.exe".to_string(), InjectionStrategy::Paste);
        assert_eq!(resolve("cmd.exe", &user), InjectionStrategy::Paste);
    }

    #[test]
    fn user_override_works_for_unknown_app() {
        // User flags a custom app for keystroke injection.
        let mut user = HashMap::new();
        user.insert("myrareeditor.exe".to_string(), InjectionStrategy::Keystroke);
        assert_eq!(
            resolve("myrareeditor.exe", &user),
            InjectionStrategy::Keystroke
        );
    }

    #[test]
    fn user_override_can_promote_to_abort() {
        // User flags a normal app as Abort — say they really don't
        // want voice input near their notes app for privacy reasons.
        let mut user = HashMap::new();
        user.insert("notes.exe".to_string(), InjectionStrategy::Abort);
        assert_eq!(resolve("notes.exe", &user), InjectionStrategy::Abort);
    }

    #[test]
    fn builtin_table_contains_no_paste_entries() {
        // Paste is the default — putting a paste entry in the builtin
        // table is dead weight. This test guards against accidental
        // additions during table maintenance.
        for (key, strategy) in BUILTIN_OVERRIDES.entries() {
            assert_ne!(
                *strategy,
                InjectionStrategy::Paste,
                "builtin override {key} maps to Paste — remove it; that's the default"
            );
        }
    }

    #[test]
    fn empty_process_name_falls_back_to_paste() {
        // Edge: WindowContext could plausibly return "" if
        // K32GetModuleBaseNameW fails. Resolver should not panic.
        assert_eq!(resolve("", &empty()), InjectionStrategy::Paste);
    }
}
