//! Settings key/value bridge.
//!
//! The DB has a flat `settings (key, value)` table — strings only.
//! This module typifies a known set of keys + provides safe
//! defaults so the UI never sees a missing field.

use rusqlite::Connection;
use tauri::State;

use crate::commands::types::SettingsSnapshot;
use crate::commands::{into_err, lock_db, AppStateHandle};

const KEY_THEME: &str = "ui.theme";
const KEY_SOUND: &str = "ui.sound_enabled";
const KEY_AUTOSTART: &str = "ui.autostart";
const KEY_REDUCED_MOTION: &str = "ui.reduced_motion";
const KEY_RETENTION_DAYS: &str = "history.retention_days";
const KEY_AUDIO_RETENTION: &str = "history.audio_retention";
const KEY_LEARNING_ENABLED: &str = "learning.enabled";
const KEY_CLAUDE_CONFIGURED: &str = "secrets.claude_key_configured";

fn get_string(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .ok()
}

fn get_bool(conn: &Connection, key: &str, default: bool) -> bool {
    get_string(conn, key)
        .map(|s| matches!(s.as_str(), "1" | "true" | "yes"))
        .unwrap_or(default)
}

fn get_i64(conn: &Connection, key: &str, default: i64) -> i64 {
    get_string(conn, key)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(default)
}

#[tauri::command]
pub fn get_settings(db: State<'_, AppStateHandle>) -> Result<SettingsSnapshot, String> {
    let conn = lock_db(&db)?;
    Ok(SettingsSnapshot {
        theme: get_string(&conn, KEY_THEME).unwrap_or_else(|| "system".into()),
        sound_enabled: get_bool(&conn, KEY_SOUND, false),
        autostart: get_bool(&conn, KEY_AUTOSTART, false),
        reduced_motion: get_bool(&conn, KEY_REDUCED_MOTION, false),
        retention_days: get_i64(&conn, KEY_RETENTION_DAYS, 180),
        audio_retention: get_bool(&conn, KEY_AUDIO_RETENTION, false),
        learning_enabled: get_bool(&conn, KEY_LEARNING_ENABLED, true),
        claude_key_configured: get_bool(&conn, KEY_CLAUDE_CONFIGURED, false),
    })
}

#[tauri::command]
pub fn update_setting(
    db: State<'_, AppStateHandle>,
    key: String,
    value: String,
) -> Result<(), String> {
    // Allowlist — refuse keys we don't manage. Prevents the UI from
    // ever writing arbitrary settings via dev tools.
    const ALLOWED: &[&str] = &[
        KEY_THEME,
        KEY_SOUND,
        KEY_AUTOSTART,
        KEY_REDUCED_MOTION,
        KEY_RETENTION_DAYS,
        KEY_AUDIO_RETENTION,
        KEY_LEARNING_ENABLED,
        KEY_CLAUDE_CONFIGURED,
    ];
    if !ALLOWED.contains(&key.as_str()) {
        return Err(format!("unknown settings key: {key}"));
    }
    let conn = lock_db(&db)?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [&key, &value],
    )
    .map_err(into_err)?;
    Ok(())
}
