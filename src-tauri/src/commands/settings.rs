//! Settings key/value bridge.
//!
//! The DB has a flat `settings (key, value)` table — strings only.
//! This module typifies a known set of keys + provides safe
//! defaults so the UI never sees a missing field.

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::commands::types::SettingsSnapshot;
use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::settings::{model::SettingKey, Settings};

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

// --------------------------------------------------------------------
// Phase MC Wave 5 — typed meeting-settings IPC.
//
// The legacy `get_settings` / `update_setting` pair above is hardcoded
// to a fixed `SettingsSnapshot` shape. Meeting settings landed in a
// separate typed registry (`settings::model::SettingKey`) with
// JSON-encoded values, so they need their own IPC path.
//
// Contract:
//   `meeting_settings_get_all() -> MeetingSettingsSnapshot`
//   `meeting_settings_set(key: String, value: serde_json::Value)`
//
// The `set` command is allowlisted to the `Meeting*` SettingKey
// variants so the UI can't write through to dictation-side keys
// via a typo. The pause-toggle flow goes through the dedicated
// `meeting_set_paused` command (it needs to inject an activation
// event in addition to writing the setting).
// --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSettingsSnapshot {
    pub hotkey_modifier: String,
    pub hotkey_key: String,
    pub default_source: String,
    pub max_duration_seconds: i64,
    pub filler_strip_enabled: bool,
    pub paragraph_gap_ms: i64,
    pub audio_retention_days: Option<i64>,
    pub llm_pass_enabled: bool,
    pub speaker_label_mic: String,
    pub speaker_label_sys: String,
    pub hotkey_paused: bool,
}

#[tauri::command]
pub fn meeting_settings_get_all(
    db: State<'_, AppStateHandle>,
) -> Result<MeetingSettingsSnapshot, String> {
    let conn = lock_db(&db)?;
    let s = Settings::new(&conn);
    // Every read defaults via `Settings::get` (which falls through to
    // `SettingKey::default_value` on a missing row), so a fresh DB
    // hydrates cleanly.
    Ok(MeetingSettingsSnapshot {
        hotkey_modifier: s
            .get::<String>(SettingKey::MeetingHotkeyModifier)
            .map_err(into_err)?,
        hotkey_key: s
            .get::<String>(SettingKey::MeetingHotkeyKey)
            .map_err(into_err)?,
        default_source: s
            .get::<String>(SettingKey::MeetingDefaultSource)
            .map_err(into_err)?,
        max_duration_seconds: s
            .get::<i64>(SettingKey::MeetingMaxDurationSeconds)
            .map_err(into_err)?,
        filler_strip_enabled: s
            .get::<bool>(SettingKey::MeetingFillerStripEnabled)
            .map_err(into_err)?,
        paragraph_gap_ms: s
            .get::<i64>(SettingKey::MeetingParagraphGapMs)
            .map_err(into_err)?,
        audio_retention_days: s
            .get::<Option<i64>>(SettingKey::MeetingAudioRetentionDays)
            .map_err(into_err)?,
        llm_pass_enabled: s
            .get::<bool>(SettingKey::MeetingLlmPassEnabled)
            .map_err(into_err)?,
        speaker_label_mic: s
            .get::<String>(SettingKey::MeetingSpeakerLabelMic)
            .map_err(into_err)?,
        speaker_label_sys: s
            .get::<String>(SettingKey::MeetingSpeakerLabelSys)
            .map_err(into_err)?,
        hotkey_paused: s
            .get::<bool>(SettingKey::MeetingHotkeyPaused)
            .map_err(into_err)?,
    })
}

#[tauri::command]
pub fn meeting_settings_set(
    db: State<'_, AppStateHandle>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let setting_key = SettingKey::try_parse(&key).map_err(into_err)?;
    if !is_meeting_setting_allowed_for_ui(setting_key) {
        return Err(format!(
            "setting key {key:?} cannot be written via meeting_settings_set"
        ));
    }
    let conn = lock_db(&db)?;
    Settings::new(&conn)
        .set_raw(setting_key, &value)
        .map_err(into_err)
}

/// Allowlist: only Meeting* keys can be written via the meeting
/// settings IPC. `MeetingHotkeyPaused` is excluded — it has a
/// dedicated `meeting_set_paused` command (needs to inject the
/// activation event as well as persist).
fn is_meeting_setting_allowed_for_ui(k: SettingKey) -> bool {
    matches!(
        k,
        SettingKey::MeetingHotkeyModifier
            | SettingKey::MeetingHotkeyKey
            | SettingKey::MeetingDefaultSource
            | SettingKey::MeetingMaxDurationSeconds
            | SettingKey::MeetingFillerStripEnabled
            | SettingKey::MeetingParagraphGapMs
            | SettingKey::MeetingAudioRetentionDays
            | SettingKey::MeetingLlmPassEnabled
            | SettingKey::MeetingSpeakerLabelMic
            | SettingKey::MeetingSpeakerLabelSys // MeetingLastSelectedSource: runtime-managed; MeetingHotkeyPaused: dedicated command
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_writable_meeting_keys() {
        for k in [
            SettingKey::MeetingHotkeyModifier,
            SettingKey::MeetingHotkeyKey,
            SettingKey::MeetingDefaultSource,
            SettingKey::MeetingMaxDurationSeconds,
            SettingKey::MeetingFillerStripEnabled,
            SettingKey::MeetingParagraphGapMs,
            SettingKey::MeetingAudioRetentionDays,
            SettingKey::MeetingLlmPassEnabled,
            SettingKey::MeetingSpeakerLabelMic,
            SettingKey::MeetingSpeakerLabelSys,
        ] {
            assert!(
                is_meeting_setting_allowed_for_ui(k),
                "key {k:?} should be writable via meeting_settings_set"
            );
        }
    }

    #[test]
    fn allowlist_rejects_dictation_and_paused_keys() {
        // Dictation key — not writable through meeting IPC.
        assert!(!is_meeting_setting_allowed_for_ui(SettingKey::Theme));
        assert!(!is_meeting_setting_allowed_for_ui(
            SettingKey::AudioRetentionDays
        ));
        // MeetingHotkeyPaused has its own command — reject here so we
        // can't accidentally toggle pause without injecting the
        // activation event.
        assert!(!is_meeting_setting_allowed_for_ui(
            SettingKey::MeetingHotkeyPaused
        ));
        // MeetingLastSelectedSource is runtime-managed.
        assert!(!is_meeting_setting_allowed_for_ui(
            SettingKey::MeetingLastSelectedSource
        ));
    }
}
