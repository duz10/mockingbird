#![allow(missing_docs)] // Enum variants are documented in docs/SETTINGS.md.

//! Typed setting keys — the registry of every known setting in the app.
//!
//! Adding a new setting:
//!   1. Add a variant to [`SettingKey`].
//!   2. Add the mapping in `as_str` and `try_parse`.
//!   3. Add a default in `default_value`.
//!   4. Document it in `docs/SETTINGS.md` (Wave 5 task).
//!
//! Values are stored in the `settings` table as JSON-encoded TEXT
//! (via the `Settings` facade in `super`).

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKey {
    AutostartEnabled,
    LogLevel,
    Theme,
    ReducedMotion,
    SoundFeedback,
    /// Reference (not the secret itself) to the Windows Credential
    /// Manager entry that holds the Claude API key. Phase 4 wires the
    /// real lookup; Phase 1 just stores/reads the string.
    ClaudeApiKeyRef,
    AudioRetentionDays,
    LearningEnabled,
}

impl SettingKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutostartEnabled => "autostart_enabled",
            Self::LogLevel => "log_level",
            Self::Theme => "theme",
            Self::ReducedMotion => "reduced_motion",
            Self::SoundFeedback => "sound_feedback",
            Self::ClaudeApiKeyRef => "claude_api_key_ref",
            Self::AudioRetentionDays => "audio_retention_days",
            Self::LearningEnabled => "learning_enabled",
        }
    }

    pub fn try_parse(s: &str) -> AppResult<Self> {
        match s {
            "autostart_enabled" => Ok(Self::AutostartEnabled),
            "log_level" => Ok(Self::LogLevel),
            "theme" => Ok(Self::Theme),
            "reduced_motion" => Ok(Self::ReducedMotion),
            "sound_feedback" => Ok(Self::SoundFeedback),
            "claude_api_key_ref" => Ok(Self::ClaudeApiKeyRef),
            "audio_retention_days" => Ok(Self::AudioRetentionDays),
            "learning_enabled" => Ok(Self::LearningEnabled),
            other => Err(AppError::Other(format!("unknown setting key: {other:?}"))),
        }
    }

    pub fn default_value(self) -> serde_json::Value {
        match self {
            Self::AutostartEnabled => serde_json::json!(false),
            Self::LogLevel => serde_json::json!("info"),
            Self::Theme => serde_json::json!("system"),
            Self::ReducedMotion => serde_json::json!(false),
            Self::SoundFeedback => serde_json::json!(true),
            Self::ClaudeApiKeyRef => serde_json::json!(null),
            Self::AudioRetentionDays => serde_json::json!(30),
            Self::LearningEnabled => serde_json::json!(true),
        }
    }

    /// All known keys. Useful for the settings UI (Phase 5) and for
    /// tests that iterate over them.
    pub fn all() -> &'static [SettingKey] {
        &[
            Self::AutostartEnabled,
            Self::LogLevel,
            Self::Theme,
            Self::ReducedMotion,
            Self::SoundFeedback,
            Self::ClaudeApiKeyRef,
            Self::AudioRetentionDays,
            Self::LearningEnabled,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips_via_as_str_and_try_parse() {
        for &k in SettingKey::all() {
            let s = k.as_str();
            let parsed = SettingKey::try_parse(s).unwrap();
            assert_eq!(parsed, k);
        }
    }

    #[test]
    fn try_parse_rejects_unknown_keys() {
        assert!(SettingKey::try_parse("not_a_real_key").is_err());
        assert!(SettingKey::try_parse("").is_err());
        assert!(SettingKey::try_parse("THEME").is_err()); // case-sensitive
    }

    #[test]
    fn every_key_has_a_default_value() {
        for &k in SettingKey::all() {
            let v = k.default_value();
            // All defaults must serialize; smoke check.
            let _ = v.to_string();
        }
    }

    #[test]
    fn defaults_match_documented_types() {
        assert!(SettingKey::AutostartEnabled.default_value().is_boolean());
        assert!(SettingKey::LogLevel.default_value().is_string());
        assert!(SettingKey::Theme.default_value().is_string());
        assert!(SettingKey::AudioRetentionDays.default_value().is_number());
        assert!(SettingKey::ClaudeApiKeyRef.default_value().is_null());
    }
}
