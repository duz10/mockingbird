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
//! ## Wave 1C.2 additions (`mb-9ufg`, ADR 0051)
//!
//! - [`kg_list_failed_filings`] — paginated read of `state='failed'`
//!   rows for the Settings KG tab's failed-filings list.
//! - [`kg_requeue_failed`] — flips a failed row back to `pending`,
//!   resets `attempt_count`, clears `last_error`. **Idempotent** on
//!   already-pending rows (J3 invariant for ADR 0051's Wave 1C.5
//!   judge bundle); a double-click on Retry is a no-op, not an error.
//! - [`kg_queue_status`] — per-state counts + `last_done_iso` for
//!   the "Filing status" line above the failed-filings list.
//!
//! ## Why not extend `commands/settings.rs`?
//!
//! ADR 0051's wave plan calls out `src-tauri/src/commands/kg.rs` as
//! a new file (scopes nicely to KG; keeps `settings.rs` from sprawling).
//! Future KG-side IPC for filter candidates (1C.3) and concept
//! lookups (1C.4) lands in this same module.

use serde::Serialize;
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::kg::store::queue::{self, FailedFiling, QueueStatus};
use crate::settings::{model::SettingKey, Settings};

/// Default cap on [`kg_list_failed_filings`] when the UI omits the
/// `limit` argument. Matches D1 in the Wave 1C.2 binding parameters.
const DEFAULT_FAILED_FILINGS_LIMIT: u32 = 50;

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

/// List rows currently `state='failed'` in `kg_filing_queue`,
/// newest-first by enqueue time. `limit` defaults to
/// [`DEFAULT_FAILED_FILINGS_LIMIT`] when omitted.
///
/// The returned struct is the IPC DTO directly
/// ([`FailedFiling`] in `kg::store::queue`), serialized to camelCase
/// for the JS side. Wave 1C.2 / ADR 0051 D1.
#[tauri::command]
pub fn kg_list_failed_filings(
    db: State<'_, AppStateHandle>,
    limit: Option<u32>,
) -> Result<Vec<FailedFiling>, String> {
    let cap = limit.unwrap_or(DEFAULT_FAILED_FILINGS_LIMIT);
    let conn = lock_db(&db)?;
    queue::list_failed(&conn, cap).map_err(into_err)
}

/// Flip a `state='failed'` row back to `pending` for another shot.
/// Resets `attempt_count=0`, clears `last_error`. **Idempotent**:
/// calling on an already-pending (or missing) row returns `Ok(())`
/// without error -- this is the J3 invariant pinned for ADR 0051's
/// Wave 1C.5 judge bundle. Wave 1C.2 / ADR 0051 D1.
#[tauri::command]
pub fn kg_requeue_failed(db: State<'_, AppStateHandle>, queue_id: i64) -> Result<(), String> {
    let conn = lock_db(&db)?;
    queue::requeue_failed(&conn, queue_id).map_err(into_err)
}

/// Per-state queue counts + the most recent successful filing's
/// timestamp. Drives the "Filing status" line above the failed-
/// filings list. Wave 1C.2 / ADR 0051 D1.
#[tauri::command]
pub fn kg_queue_status(db: State<'_, AppStateHandle>) -> Result<QueueStatus, String> {
    let conn = lock_db(&db)?;
    queue::queue_status(&conn).map_err(into_err)
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
    //
    // Wave 1C.2: the store-layer surface
    // (`list_failed` / `requeue_failed` / `queue_status`) is
    // exhaustively tested in `kg::store::queue::tests` -- IPC
    // command wrappers are 3-line `lock_db` + `map_err` proxies.
    // The wire-shape camelCase contract is tested in queue.rs's
    // `dtos_serialize_camel_case` test.

    #[test]
    fn default_failed_filings_limit_matches_brief() {
        // Brief D1: "defaults limit=50". Pinning the constant here so
        // a typo in the default surfaces as a unit-test failure rather
        // than a quiet UX regression where the UI shows the wrong
        // number of rows.
        assert_eq!(DEFAULT_FAILED_FILINGS_LIMIT, 50);
    }
}
