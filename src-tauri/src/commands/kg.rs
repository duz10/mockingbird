//! KG settings IPC — Phase 1C Wave 1C.1 (`mb-ucmx`, ADR 0051).
//!
//! Surface for the Settings UI's KG tab. Mirrors the
//! [`meeting_settings_get_all`] / [`meeting_settings_set`] shape from
//! `commands/settings.rs` so the UI's IPC binding layer can follow
//! the same allowlist + typed-snapshot conventions.
//!
//! [`meeting_settings_get_all`]: super::settings::meeting_settings_get_all
//! [`meeting_settings_set`]: super::settings::meeting_settings_set
//!
//! ## Shape
//!
//! - [`kg_settings_get_all`] returns a typed [`KgSettingsSnapshot`].
//!   v1 carries one field (`kg_graph_enabled`); the struct is
//!   forward-compatible — Phase 1C.3+ KG settings (filter defaults,
//!   per-mode opt-in, etc.) land here without an IPC-shape break.
//! - [`kg_settings_set`] is a key/value write gated by
//!   [`is_kg_setting_allowed_for_ui`]. v1 allows only
//!   `kg_graph_enabled`; new keys are an explicit edit to the
//!   allowlist (catches typos + accidental dictation-side writes).
//!
//! ## Why not extend `commands/settings.rs`?
//!
//! ADR 0051's wave plan calls out `src-tauri/src/commands/kg.rs` as
//! a new file (scopes nicely to KG; keeps `settings.rs` from sprawling).
//! Future KG-side IPC for failed-filings (1C.2), filter candidates
//! (1C.3), and concept lookups (1C.4) lands in this same module.

use serde::Serialize;
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::settings::{model::SettingKey, Settings};

/// Typed snapshot of every KG-side setting the UI reads.
///
/// One field today; the struct is the forward-compat boundary so
/// later 1C waves (and 1D backfill) can add fields without
/// breaking the IPC contract.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KgSettingsSnapshot {
    /// Master KG opt-in (per ADR 0050; default `false`). When `false`
    /// the dictation tail does not enqueue and the filing worker
    /// sleeps without dequeuing (Wave 1C.1 boot-vs-poll promotion).
    pub kg_graph_enabled: bool,
}

#[tauri::command]
pub fn kg_settings_get_all(db: State<'_, AppStateHandle>) -> Result<KgSettingsSnapshot, String> {
    let conn = lock_db(&db)?;
    let s = Settings::new(&conn);
    Ok(KgSettingsSnapshot {
        kg_graph_enabled: s
            .get::<bool>(SettingKey::KgGraphEnabled)
            .map_err(into_err)?,
    })
}

#[tauri::command]
pub fn kg_settings_set(
    db: State<'_, AppStateHandle>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let setting_key = SettingKey::try_parse(&key).map_err(into_err)?;
    if !is_kg_setting_allowed_for_ui(setting_key) {
        return Err(format!(
            "setting key {key:?} cannot be written via kg_settings_set"
        ));
    }
    let conn = lock_db(&db)?;
    Settings::new(&conn)
        .set_raw(setting_key, &value)
        .map_err(into_err)
}

/// Allowlist for the UI-side KG settings writer.
///
/// v1 allows only `KgGraphEnabled`. Adding a new KG setting that
/// the UI should be able to flip is a one-line edit here AND in
/// the typed [`KgSettingsSnapshot`] above — both edits intentionally
/// land together so the surface stays in sync.
fn is_kg_setting_allowed_for_ui(k: SettingKey) -> bool {
    matches!(k, SettingKey::KgGraphEnabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_kg_graph_enabled() {
        assert!(is_kg_setting_allowed_for_ui(SettingKey::KgGraphEnabled));
    }

    #[test]
    fn allowlist_rejects_unrelated_keys() {
        // Dictation-side / meeting-side keys must not be writable
        // through the KG IPC — that's a typo guard, mirrors the
        // meeting_settings allowlist test's posture.
        assert!(!is_kg_setting_allowed_for_ui(SettingKey::Theme));
        assert!(!is_kg_setting_allowed_for_ui(SettingKey::LearningEnabled));
        assert!(!is_kg_setting_allowed_for_ui(SettingKey::MeetingHotkeyKey));
        assert!(!is_kg_setting_allowed_for_ui(
            SettingKey::CommandCenterChord
        ));
    }

    // End-to-end snapshot / set / set-allowlist tests live in the
    // throwaway-crate runner ((LESSONS P2 — `cargo test --release`
    // is broken on this box). The pure-Rust unit tests above cover
    // the allowlist contract; the wired-in-Tauri-State tests would
    // require a managed-state harness that's heavier than the
    // surface warrants and is exercised through the existing
    // `kg_graph_off_invariant` probe end-to-end.
}
