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

    // ----- Phase MC — meeting capture settings (ADR 0026 lateral epic). -----
    // The full Section MC.5 contract documents these. Defaults reflect
    // the Right Ctrl + M chord choice (ADR 0027) and the 30 s chunk +
    // 2 s overlap chunked-Whisper design (ADR 0029).
    //
    /// VK name string for the meeting hotkey modifier. Default
    /// `"VK_RCONTROL"`. Allowed set: `RCtrl, LCtrl, RAlt, LAlt, RShift,
    /// LShift, RWin, LWin`. Settings UI clamps the picker.
    /// Conflict probe (Wave 3) rejects if it equals the dictation VK.
    MeetingHotkeyModifier,
    /// VK name string for the meeting hotkey main key. Default
    /// `"VK_OEM_PERIOD"` (the `.>` key) as of the mb-fc1 hotfix —
    /// the original `"VK_M"` default collided with Microsoft 365
    /// Copilot on Windows 11. ADR 0019 fallback ladder steps to
    /// `"VK_F13"` then `"VK_F14"` then user-pick if the chosen key is
    /// claimed by another global hook.
    MeetingHotkeyKey,
    /// Default source preselected in the meeting overlay. One of
    /// `"mic" | "system" | "both"`. Default `"mic"`.
    MeetingDefaultSource,
    /// Hard cap on meeting duration in seconds. Default 14_400 (4 h).
    /// Clamped to `[60, 21600]` by the settings facade (Wave 4).
    MeetingMaxDurationSeconds,
    /// Whether the deterministic formatter strips fillers
    /// ("um", "uh", "you know", …) by default. Default `true`.
    MeetingFillerStripEnabled,
    /// Gap in ms between segments that triggers a paragraph break.
    /// Default 2_000. Clamped to `[500, 10_000]` by the settings facade.
    MeetingParagraphGapMs,
    /// Per-feature retention override for meeting audio blobs.
    /// Defaults to the global `AudioRetentionDays` (30) if absent on
    /// the settings facade's read path. Stored separately so a power
    /// user can keep meetings 90 days while dictation stays at 30.
    MeetingAudioRetentionDays,
    /// Whether the LLM-pass UI affordance is available on the meeting
    /// detail page. Default `true`. Disabling hides the panel; does
    /// NOT remove the prompt markdown files from the bundle.
    MeetingLlmPassEnabled,
    /// UI state — last source the user picked in the overlay. Echoed
    /// back as the preselected option on next open. One of
    /// `"mic" | "system" | "both"`. Default `"mic"`.
    MeetingLastSelectedSource,
    /// Speaker label for the mic channel in merged-view exports.
    /// Default `"You"`.
    MeetingSpeakerLabelMic,
    /// Speaker label for the system-loopback channel in merged-view
    /// exports. Default `"Other(s)"`.
    MeetingSpeakerLabelSys,
    /// Whether the meeting hotkey is currently paused (mirror of the
    /// tray-menu "Pause Meeting Hotkey" toggle). Default `false`.
    /// Persisted so the choice survives app restart. Read once at
    /// runtime spawn + written by `MeetingCaptureRuntime::
    /// set_meeting_hotkey_paused`.
    MeetingHotkeyPaused,
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
            // Phase MC.
            Self::MeetingHotkeyModifier => "meeting_hotkey_modifier",
            Self::MeetingHotkeyKey => "meeting_hotkey_key",
            Self::MeetingDefaultSource => "meeting_default_source",
            Self::MeetingMaxDurationSeconds => "meeting_max_duration_seconds",
            Self::MeetingFillerStripEnabled => "meeting_filler_strip_enabled",
            Self::MeetingParagraphGapMs => "meeting_paragraph_gap_ms",
            Self::MeetingAudioRetentionDays => "meeting_audio_retention_days",
            Self::MeetingLlmPassEnabled => "meeting_llm_pass_enabled",
            Self::MeetingLastSelectedSource => "meeting_last_selected_source",
            Self::MeetingSpeakerLabelMic => "meeting_speaker_label_mic",
            Self::MeetingSpeakerLabelSys => "meeting_speaker_label_sys",
            Self::MeetingHotkeyPaused => "meeting_hotkey_paused",
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
            // Phase MC.
            "meeting_hotkey_modifier" => Ok(Self::MeetingHotkeyModifier),
            "meeting_hotkey_key" => Ok(Self::MeetingHotkeyKey),
            "meeting_default_source" => Ok(Self::MeetingDefaultSource),
            "meeting_max_duration_seconds" => Ok(Self::MeetingMaxDurationSeconds),
            "meeting_filler_strip_enabled" => Ok(Self::MeetingFillerStripEnabled),
            "meeting_paragraph_gap_ms" => Ok(Self::MeetingParagraphGapMs),
            "meeting_audio_retention_days" => Ok(Self::MeetingAudioRetentionDays),
            "meeting_llm_pass_enabled" => Ok(Self::MeetingLlmPassEnabled),
            "meeting_last_selected_source" => Ok(Self::MeetingLastSelectedSource),
            "meeting_speaker_label_mic" => Ok(Self::MeetingSpeakerLabelMic),
            "meeting_speaker_label_sys" => Ok(Self::MeetingSpeakerLabelSys),
            "meeting_hotkey_paused" => Ok(Self::MeetingHotkeyPaused),
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
            // Phase MC.
            Self::MeetingHotkeyModifier => serde_json::json!("VK_RCONTROL"),
            // mb-fc1 hotfix: was "VK_M". Right Ctrl + Period is the
            // post-Copilot-collision default. See ADR 0033 (chord
            // collision hotfix).
            Self::MeetingHotkeyKey => serde_json::json!("VK_OEM_PERIOD"),
            Self::MeetingDefaultSource => serde_json::json!("mic"),
            Self::MeetingMaxDurationSeconds => serde_json::json!(14_400),
            Self::MeetingFillerStripEnabled => serde_json::json!(true),
            Self::MeetingParagraphGapMs => serde_json::json!(2_000),
            // Null means "inherit AudioRetentionDays". Settings facade
            // resolves the fallback on read; storing null here keeps
            // the per-feature override explicit.
            Self::MeetingAudioRetentionDays => serde_json::json!(null),
            Self::MeetingLlmPassEnabled => serde_json::json!(true),
            Self::MeetingLastSelectedSource => serde_json::json!("mic"),
            Self::MeetingSpeakerLabelMic => serde_json::json!("You"),
            Self::MeetingSpeakerLabelSys => serde_json::json!("Other(s)"),
            Self::MeetingHotkeyPaused => serde_json::json!(false),
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
            // Phase MC.
            Self::MeetingHotkeyModifier,
            Self::MeetingHotkeyKey,
            Self::MeetingDefaultSource,
            Self::MeetingMaxDurationSeconds,
            Self::MeetingFillerStripEnabled,
            Self::MeetingParagraphGapMs,
            Self::MeetingAudioRetentionDays,
            Self::MeetingLlmPassEnabled,
            Self::MeetingLastSelectedSource,
            Self::MeetingSpeakerLabelMic,
            Self::MeetingSpeakerLabelSys,
            Self::MeetingHotkeyPaused,
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

    /// Phase MC — the meeting setting defaults must match the contract
    /// documented in `docs/phases/phase-meeting-capture.md` Section
    /// MC.5. Drift here means the Wave 3 conflict probe and the Wave 5
    /// settings UI will silently disagree with the plan.
    #[test]
    fn meeting_settings_defaults_match_section_mc_5() {
        use serde_json::json;
        assert_eq!(
            SettingKey::MeetingHotkeyModifier.default_value(),
            json!("VK_RCONTROL")
        );
        assert_eq!(
            SettingKey::MeetingHotkeyKey.default_value(),
            json!("VK_OEM_PERIOD")
        );
        assert_eq!(
            SettingKey::MeetingDefaultSource.default_value(),
            json!("mic")
        );
        assert_eq!(
            SettingKey::MeetingMaxDurationSeconds.default_value(),
            json!(14_400)
        );
        assert_eq!(
            SettingKey::MeetingFillerStripEnabled.default_value(),
            json!(true)
        );
        assert_eq!(
            SettingKey::MeetingParagraphGapMs.default_value(),
            json!(2_000)
        );
        assert!(SettingKey::MeetingAudioRetentionDays
            .default_value()
            .is_null());
        assert_eq!(
            SettingKey::MeetingLlmPassEnabled.default_value(),
            json!(true)
        );
        assert_eq!(
            SettingKey::MeetingLastSelectedSource.default_value(),
            json!("mic")
        );
        assert_eq!(
            SettingKey::MeetingSpeakerLabelMic.default_value(),
            json!("You")
        );
        assert_eq!(
            SettingKey::MeetingSpeakerLabelSys.default_value(),
            json!("Other(s)")
        );
    }

    /// `SettingKey::all()` must enumerate every variant. Coupled to
    /// the variant count so adding a key without extending `all()`
    /// fails the test. Bump the expected count when you add a key.
    #[test]
    fn all_enumerates_every_variant() {
        // 8 original + 11 Phase MC + 1 Phase MC W5 (MeetingHotkeyPaused) = 20.
        assert_eq!(SettingKey::all().len(), 20);
    }
}
