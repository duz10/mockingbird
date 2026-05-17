//! Phase 1 typed-setting bridge — narrowed-scope variant of the
//! `commands/settings.rs` UI bridge.
//!
//! The two coexist deliberately:
//!
//! - **legacy** (`get_setting` / `set_setting`) — typed setting keys,
//!   JSON-value payloads, used by tests + the older Phase 1 contract.
//! - **settings** (`get_settings` / `update_setting`) — flat
//!   string→string, used by the UI's Settings panel.
//!
//! Naming is intentionally close-but-distinct (`get_setting` vs.
//! `get_settings`) so Tauri's globally-unique handler registry
//! accepts both. Removing the legacy commands would orphan the
//! Phase 1 typed `Settings` model + its tests; not worth the cleanup
//! cost right now.

use tauri::State;

use crate::error::AppError;
use crate::settings::{model::SettingKey, Settings};

use super::AppStateHandle;

pub(crate) fn into_command_err(e: AppError) -> String {
    e.to_string()
}

/// IPC: read a typed setting by key. Returns the stored JSON value,
/// or the key's default if unset / corrupted.
#[tauri::command]
pub async fn get_setting(
    state: State<'_, AppStateHandle>,
    key: String,
) -> Result<serde_json::Value, String> {
    let key = SettingKey::try_parse(&key).map_err(into_command_err)?;
    let guard = state
        .db
        .lock()
        .map_err(|e| format!("db lock poisoned: {e}"))?;
    let settings = Settings::new(&guard);
    settings.get_raw(key).map_err(into_command_err)
}

/// IPC: write a typed setting by key. UPSERT semantics.
#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppStateHandle>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let key = SettingKey::try_parse(&key).map_err(into_command_err)?;
    let guard = state
        .db
        .lock()
        .map_err(|e| format!("db lock poisoned: {e}"))?;
    let settings = Settings::new(&guard);
    settings.set_raw(key, &value).map_err(into_command_err)
}

/// IPC: smoke-test FTS5. Returns the count of transcript hits for
/// `query`. Used by the Wave-5 `fts5-smoke` judge.
#[tauri::command]
pub async fn fts_smoke_test(
    state: State<'_, AppStateHandle>,
    query: String,
) -> Result<usize, String> {
    let guard = state
        .db
        .lock()
        .map_err(|e| format!("db lock poisoned: {e}"))?;
    crate::db::search::smoke_test_count(&guard, &query).map_err(into_command_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_command_err_renders_displayable() {
        let s = into_command_err(AppError::Other("boom".into()));
        assert!(s.contains("boom"), "got: {s}");
    }
}
