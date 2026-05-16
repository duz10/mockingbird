//! Tauri command handlers — the IPC surface to the frontend.
//!
//! Phase 1 ships 3: `get_setting`, `set_setting`, `fts_smoke_test`.
//! Wave 5 may add more, Phase 5 (UI) will rely on these.
//!
//! All commands return `Result<T, String>` because typed errors don't
//! cross the IPC boundary cleanly. `into_command_err` does the
//! `AppError → String` conversion.

use std::sync::Mutex;

use tauri::State;

use crate::db::Database;
use crate::error::AppError;
use crate::settings::{model::SettingKey, Settings};

/// Managed state held by Tauri. `rusqlite::Connection` is `Send` but
/// not `Sync`, so we wrap the `Database` in a `Mutex` to satisfy
/// `tauri::State<T>`'s `Sync` requirement.
pub struct AppState {
    /// Database connection, behind a Mutex to provide `Sync`.
    pub db: Mutex<Database>,
}

impl AppState {
    /// Wrap an open `Database` in the managed-state shape.
    pub fn new(db: Database) -> Self {
        Self { db: Mutex::new(db) }
    }
}

pub(crate) fn into_command_err(e: AppError) -> String {
    e.to_string()
}

/// IPC: read a setting by key. Returns the stored JSON value, or the
/// key's default if unset / corrupted.
#[tauri::command]
pub async fn get_setting(
    state: State<'_, AppState>,
    key: String,
) -> Result<serde_json::Value, String> {
    let key = SettingKey::try_parse(&key).map_err(into_command_err)?;
    let guard = state
        .db
        .lock()
        .map_err(|e| format!("db lock poisoned: {e}"))?;
    let settings = Settings::new(&guard.conn);
    settings.get_raw(key).map_err(into_command_err)
}

/// IPC: write a setting by key. UPSERT semantics — overwrites if
/// present, inserts if missing.
#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let key = SettingKey::try_parse(&key).map_err(into_command_err)?;
    let guard = state
        .db
        .lock()
        .map_err(|e| format!("db lock poisoned: {e}"))?;
    let settings = Settings::new(&guard.conn);
    settings.set_raw(key, &value).map_err(into_command_err)
}

/// IPC: smoke-test the FTS5 wiring. Returns the count of transcript
/// hits for `query`. Used by the Wave-5 `fts5-smoke` judge.
#[tauri::command]
pub async fn fts_smoke_test(state: State<'_, AppState>, query: String) -> Result<usize, String> {
    let guard = state
        .db
        .lock()
        .map_err(|e| format!("db lock poisoned: {e}"))?;
    crate::db::search::smoke_test_count(&guard.conn, &query).map_err(into_command_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_command_err_renders_displayable() {
        let s = into_command_err(AppError::Other("boom".into()));
        assert!(s.contains("boom"), "got: {s}");
    }

    #[test]
    fn app_state_wraps_database() {
        let db = Database::open_in_memory().unwrap();
        let state = AppState::new(db);
        // Smoke: we can lock + access the connection.
        let guard = state.db.lock().unwrap();
        let one: i64 = guard.conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(one, 1);
    }
}
