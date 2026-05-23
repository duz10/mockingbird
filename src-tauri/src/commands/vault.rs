//! Vault-related Tauri commands -- ADR 0046 Iter 2 / mb-lvzw.
//!
//! Two surfaces:
//!
//! - **Settings IPC** (`vault_settings_get` / `vault_settings_set`) --
//!   typed get/set for the four `Vault*` / `MobileSync*` keys. Same
//!   shape as `meeting_settings_*`; lives here rather than in
//!   `settings.rs` so the vault subsystem owns its own UI surface
//!   end-to-end. Every `set` ALSO refreshes the runtime config and
//!   fires a backfill trigger so the user's settings flip takes
//!   effect immediately.
//!
//! - **Manual reconciliation** (`vault_export_now`) -- synchronous
//!   one-shot pass for the Settings UI "Export now" button. Returns
//!   a small summary the toast renders verbatim.
//!
//! All commands honor the runtime's coalesce / blocking-lock
//! semantics so spamming the button can't pile up worker threads or
//! corrupt the manifest.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::settings::{model::SettingKey, Settings};
use crate::vault::export_job::{ReconciliationSummary, VaultRuntime};

// --------------------------------------------------------------------
// Settings get/set
// --------------------------------------------------------------------

/// Snapshot of every UI-visible vault / mobile-sync setting.
///
/// Lowercase JSON field names (camelCase via `rename_all`) align with
/// the rest of the typed settings IPC contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettingsSnapshot {
    pub mobile_sync_enabled: bool,
    pub vault_path: Option<String>,
    pub vault_sync_record_types: String,
    pub vault_retention_days: i64,
}

#[tauri::command]
pub fn vault_settings_get(db: State<'_, AppStateHandle>) -> Result<VaultSettingsSnapshot, String> {
    let conn = lock_db(&db)?;
    let s = Settings::new(&conn);
    Ok(VaultSettingsSnapshot {
        mobile_sync_enabled: s
            .get::<bool>(SettingKey::MobileSyncEnabled)
            .map_err(into_err)?,
        vault_path: s
            .get::<Option<String>>(SettingKey::VaultPath)
            .map_err(into_err)?,
        vault_sync_record_types: s
            .get::<String>(SettingKey::VaultSyncRecordTypes)
            .map_err(into_err)?,
        vault_retention_days: s
            .get::<i64>(SettingKey::VaultRetentionDays)
            .map_err(into_err)?,
    })
}

/// Write one of the four vault settings keys + refresh runtime + fire
/// a backfill trigger. The trigger is gated by the new config: if
/// `MobileSyncEnabled` is still false or the path is unset, the
/// trigger is a no-op (same gate as the post-commit hooks).
#[tauri::command]
pub fn vault_settings_set(
    db: State<'_, AppStateHandle>,
    vault: State<'_, Arc<VaultRuntime>>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let setting_key = SettingKey::try_parse(&key).map_err(into_err)?;
    if !is_vault_setting_allowed_for_ui(setting_key) {
        return Err(format!(
            "setting key {key:?} cannot be written via vault_settings_set"
        ));
    }
    // Drop the lock before refreshing the runtime config (which
    // re-acquires it). Avoids a self-deadlock and keeps the
    // critical section minimal.
    {
        let conn = lock_db(&db)?;
        Settings::new(&conn)
            .set_raw(setting_key, &value)
            .map_err(into_err)?;
    }
    let db_arc = Arc::clone(&db.inner().db);
    vault.refresh_config(&db_arc).map_err(into_err)?;
    // Fire-and-forget backfill so a freshly-enabled vault populates
    // without the user needing to click "Export now". Coalesces if
    // a worker is already running.
    vault.trigger(db_arc);
    Ok(())
}

fn is_vault_setting_allowed_for_ui(k: SettingKey) -> bool {
    matches!(
        k,
        SettingKey::MobileSyncEnabled
            | SettingKey::VaultPath
            | SettingKey::VaultSyncRecordTypes
            | SettingKey::VaultRetentionDays
    )
}

// --------------------------------------------------------------------
// Manual export trigger
// --------------------------------------------------------------------

/// What the "Export now" button sees back. Same shape as
/// [`ReconciliationSummary`] but with camelCase field names for the
/// React side. Skipped runs surface as a single boolean rather than
/// pretending zero records were exported, so the UI can render
/// "Sync is disabled" vs. "No new records" distinctly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultExportSummary {
    pub total: usize,
    pub changes: usize,
    pub archived: usize,
    pub skipped: bool,
}

impl From<ReconciliationSummary> for VaultExportSummary {
    fn from(s: ReconciliationSummary) -> Self {
        Self {
            total: s.total,
            changes: s.changes,
            archived: s.archived,
            skipped: s.skipped,
        }
    }
}

#[tauri::command]
pub async fn vault_export_now(
    db: State<'_, AppStateHandle>,
    vault: State<'_, Arc<VaultRuntime>>,
) -> Result<VaultExportSummary, String> {
    // Snapshot Arcs before the async boundary -- `State` isn't Send.
    let db_arc = Arc::clone(&db.inner().db);
    let vault_arc = Arc::clone(&vault);
    // Run on the blocking pool so we don't park Tauri's async
    // executor while a large reconciliation pass crunches.
    tokio::task::spawn_blocking(move || vault_arc.run_once_blocking(&db_arc))
        .await
        .map_err(|e| format!("export task panicked: {e}"))?
        .map(VaultExportSummary::from)
        .map_err(into_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_the_four_vault_keys() {
        for k in [
            SettingKey::MobileSyncEnabled,
            SettingKey::VaultPath,
            SettingKey::VaultSyncRecordTypes,
            SettingKey::VaultRetentionDays,
        ] {
            assert!(is_vault_setting_allowed_for_ui(k), "{k:?}");
        }
    }

    #[test]
    fn allowlist_rejects_non_vault_keys() {
        assert!(!is_vault_setting_allowed_for_ui(SettingKey::Theme));
        assert!(!is_vault_setting_allowed_for_ui(
            SettingKey::MeetingHotkeyModifier
        ));
    }

    #[test]
    fn summary_conversion_preserves_skipped_marker() {
        let s = ReconciliationSummary {
            total: 0,
            changes: 0,
            archived: 0,
            skipped: true,
        };
        let v: VaultExportSummary = s.into();
        assert!(v.skipped);
    }
}
