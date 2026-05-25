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

    // ----- Phase 10 Wave 1A — Unified Recording Command Center (ADR 0037). -----
    //
    // Three additive keys. No new migration; values land in the
    // existing `settings` table via the `Settings` facade.
    //
    /// String chord descriptor for the Command Center hotkey.
    /// Default `"RightCtrl+Space"`. Same parser as the meeting chord
    /// picker (a `MODIFIER+KEY` pair where both sides are VK-name
    /// strings — see [`crate::meetings::vk_names`]). Used by
    /// `command_center::hotkey::install`.
    CommandCenterChord,
    /// One-shot "user has seen the Welcome variant of the Command
    /// Center" flag. Default `false`; flips to `true` the first time
    /// the Command Center is dismissed via any path (Esc, mode pick,
    /// tray close). Boot path checks it to decide whether to call
    /// `open_via_first_run()`.
    CommandCenterSeenV1,
    /// Whether the legacy `Right Ctrl + .` meeting chord is still
    /// installed. Default `false` for new users. Existing users have
    /// this auto-promoted to `true` by the one-shot migration in
    /// `meetings/runtime.rs` (pattern verbatim from ADR 0033). When
    /// `false`, `meetings/hotkey_installer.rs` skips the direct-chord
    /// install; the user reaches Meeting capture via the Command
    /// Center mode picker instead.
    LegacyMeetingChordEnabled,

    // ----- Phase 10 Wave 4 — Activity Layer 2 (audio). -----
    //
    /// Whether Activity Capture sessions also record mic + system
    /// audio for per-Block transcription (ADR 0041 / Wave 4). Default
    /// `false` (privacy by default — Principle 4 spirit). When `true`,
    /// each session reuses the Meeting Capture twin-stream + chunked-
    /// Whisper pipeline; transcript segments land in
    /// `activity_transcript_segments` and the audio-aware abstractor
    /// prompt is used at summarization time.
    ActivityAudioEnabled,

    // ----- Phase 10 Wave 5 — Activity retention TTL (ADR 0042). -----
    //
    // All defaults are `0 = forever` (privacy-by-default-but-don't-
    // purge-without-permission — the user must opt in to TTL purges).
    //
    /// TTL in days for `activity_events`. `0` = forever. The retention
    /// sweep (run at boot + periodically) DELETEs events older than
    /// `now - ttl_days * 86_400_000` ms. Blocks whose events are
    /// purged get `raw_events_purged_at` stamped (ADR 0042 §Sweep
    /// order) but survive themselves.
    ActivityRetentionEventsDays,
    /// TTL in days for `activity_transcript_segments`. `0` = forever.
    /// Separate axis from events because some users want long-form
    /// audio transcripts purged faster than the visual timeline.
    ActivityRetentionSegmentsDays,
    /// TTL in days for `activity_blocks` themselves (the derived
    /// summary layer). `0` = forever. When non-zero, Blocks whose
    /// `started_at` is older than the cutoff are deleted entirely.
    /// This is independent of the `raw_events_purged_at` breadcrumb —
    /// that only fires when raw events under a Block age out while the
    /// Block is still inside its own retention window.
    ActivityRetentionBlocksDays,
    /// Unix epoch ms of the last successful retention sweep. `0` =
    /// never. Read by the boot sweep to decide whether to run, and by
    /// the Settings UI to display "last sweep: 2026-05-26 14:02".
    ActivityRetentionLastSweepMs,

    // ----- ADR 0046 Iter 2 — Mobile Sync via Obsidian vault. -----
    //
    // Eight keys total; Iter 2 wires the first four behaviorally, the
    // remaining four are enum-stubbed so the Iter 4 Mobile tab can
    // render with the full settings registry already present. No
    // migration: values land in the existing `settings` table via the
    // `Settings` facade. Defaults are deliberately OFF / unset --
    // mobile sync is opt-in, per Principle 4 + ADR §10.
    //
    /// Master toggle. When `true` AND `VaultPath` validates, every
    /// dictation / meeting commit triggers an export reconciliation
    /// pass and the Iter 3 inbox watcher is armed. Default `false`.
    /// Flipping ON also fires an initial backfill (every in-scope
    /// historical record gets projected).
    MobileSyncEnabled,
    /// Absolute path to the Obsidian vault root the user wants
    /// Mockingbird to write into (e.g.
    /// `C:\Users\dboyd\mockingbird-vault`). Validated by
    /// `vault::layout::VaultLayout::validate` before being honored.
    /// `null` until the user picks one. Default `null`.
    VaultPath,
    /// Which record types to project. One of `"dictation" |
    /// "meeting" | "both"`. Default `"both"`. Narrowing this triggers
    /// the reconciliation engine to ARCHIVE (not delete) any
    /// currently-projected records that are now out of scope -- ADR
    /// §5 / §13 "history/_archive/" zone.
    VaultSyncRecordTypes,
    /// Retention window in days. Records whose `started_at` is older
    /// than `now - retention_days * 86_400_000` ms are NOT projected
    /// (and are archived if they were previously projected). Default
    /// 30. `0` reserved for "forever" but the Settings UI clamps to
    /// `[1, 3650]` to discourage unbounded vault growth.
    VaultRetentionDays,

    // ----- Iter 4 stubs (declared now for enum stability; no
    // behavior in Iter 2). The Mobile tab in Iter 4 will be the
    // first reader. -----
    //
    /// Which sync backend the user has chosen for the vault. One of
    /// `"obsidian-sync" | "icloud-drive" | "onedrive" | "google-drive"
    /// | "dropbox" | "syncthing" | "other"`. Used by the Mobile tab
    /// in Iter 4 to surface the right help copy + the tier byte cap.
    /// Default `"obsidian-sync"` (the recommended option).
    VaultSyncBackend,
    /// Per-tier byte cap for the chosen backend, in bytes. The
    /// Mobile tab pre-fills from a built-in table (Obsidian Sync
    /// Standard = 4 MB, etc.); the user can override. The Iter 3
    /// audio-blob courier path checks this before queueing.
    /// `0` = unlimited (mostly for self-hosted Syncthing).
    /// Default `4_000_000` (Obsidian Sync Standard tier).
    SyncTierByteCap,
    /// Debug toggle: when `true`, the inbox watcher leaves successful
    /// couriers in `<vault>/inbox/_keep/` instead of deleting them.
    /// Default `false`. Power-user setting; never surfaced in the
    /// regular Settings UI.
    VaultDebugKeepCouriers,
    /// Whether the audio-blob courier (Iter 3+) keeps raw audio files
    /// in the vault under `<vault>/audio-blobs/`. Off by default
    /// because audio is large and most sync tiers charge per byte.
    /// Default `false`.
    KeepAudioBlobs,

    // ----- ADR 0047 Wave 1.2 — cleanup shrink-fallback dial. -----
    //
    /// Fraction-of-words threshold below which the dictation LLM
    /// cleanup pass is treated as having dropped content and falls
    /// back to the deterministic preprocessor output (ADR 0047
    /// §Wave 1.2). Default `0.65` — anything shorter than 65% of the
    /// pre-text word count, with no preprocessor-detected self-
    /// corrections to explain the loss, is the wrong answer.
    ///
    /// Lower the value to suppress the guard (more permissive);
    /// raise it to be more conservative. `0.0` disables.
    LlmShrinkFallbackThreshold,

    // ----- ADR 0047 Wave 2.2 — LLM-skip-on-short-utterance dial. -----
    //
    /// Word-count ceiling under which short, non-listy utterances
    /// skip the LLM cleanup pass entirely and return the deterministic
    /// preprocessor's output directly (ADR 0047 §Wave 2.2). Default
    /// `12` — the empirically observed inflection point between
    /// "one-liner that the preprocessor already handled cleanly" and
    /// "long enough that an LLM pass might add value".
    ///
    /// The skip is also gated on `pre.notes.looks_listy() == false`;
    /// any utterance with list signals always reaches the LLM so the
    /// list can be rendered. `0` disables the skip (always run LLM).
    LlmSkipWordThreshold,
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
            // Phase 10 Wave 1A.
            Self::CommandCenterChord => "command_center_chord",
            Self::CommandCenterSeenV1 => "command_center_seen_v1",
            Self::LegacyMeetingChordEnabled => "legacy_meeting_chord_enabled",
            // Phase 10 Wave 4.
            Self::ActivityAudioEnabled => "activity_audio_enabled",
            // Phase 10 Wave 5.
            Self::ActivityRetentionEventsDays => "activity_retention_events_days",
            Self::ActivityRetentionSegmentsDays => "activity_retention_segments_days",
            Self::ActivityRetentionBlocksDays => "activity_retention_blocks_days",
            Self::ActivityRetentionLastSweepMs => "activity_retention_last_sweep_ms",
            // ADR 0046 Iter 2.
            Self::MobileSyncEnabled => "mobile_sync_enabled",
            Self::VaultPath => "vault_path",
            Self::VaultSyncRecordTypes => "vault_sync_record_types",
            Self::VaultRetentionDays => "vault_retention_days",
            Self::VaultSyncBackend => "vault_sync_backend",
            Self::SyncTierByteCap => "sync_tier_byte_cap",
            Self::VaultDebugKeepCouriers => "vault_debug_keep_couriers",
            Self::KeepAudioBlobs => "keep_audio_blobs",
            // ADR 0047 Wave 1.2.
            Self::LlmShrinkFallbackThreshold => "llm_shrink_fallback_threshold",
            // ADR 0047 Wave 2.2.
            Self::LlmSkipWordThreshold => "llm_skip_word_threshold",
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
            // Phase 10 Wave 1A.
            "command_center_chord" => Ok(Self::CommandCenterChord),
            "command_center_seen_v1" => Ok(Self::CommandCenterSeenV1),
            "legacy_meeting_chord_enabled" => Ok(Self::LegacyMeetingChordEnabled),
            // Phase 10 Wave 4.
            "activity_audio_enabled" => Ok(Self::ActivityAudioEnabled),
            // Phase 10 Wave 5.
            "activity_retention_events_days" => Ok(Self::ActivityRetentionEventsDays),
            "activity_retention_segments_days" => Ok(Self::ActivityRetentionSegmentsDays),
            "activity_retention_blocks_days" => Ok(Self::ActivityRetentionBlocksDays),
            "activity_retention_last_sweep_ms" => Ok(Self::ActivityRetentionLastSweepMs),
            // ADR 0046 Iter 2.
            "mobile_sync_enabled" => Ok(Self::MobileSyncEnabled),
            "vault_path" => Ok(Self::VaultPath),
            "vault_sync_record_types" => Ok(Self::VaultSyncRecordTypes),
            "vault_retention_days" => Ok(Self::VaultRetentionDays),
            "vault_sync_backend" => Ok(Self::VaultSyncBackend),
            "sync_tier_byte_cap" => Ok(Self::SyncTierByteCap),
            "vault_debug_keep_couriers" => Ok(Self::VaultDebugKeepCouriers),
            "keep_audio_blobs" => Ok(Self::KeepAudioBlobs),
            // ADR 0047 Wave 1.2.
            "llm_shrink_fallback_threshold" => Ok(Self::LlmShrinkFallbackThreshold),
            // ADR 0047 Wave 2.2.
            "llm_skip_word_threshold" => Ok(Self::LlmSkipWordThreshold),
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
            // Phase 10 Wave 1A. Default chord per ADR 0037 §Q1; if it
            // conflicts with another global hook at boot, the conflict
            // probe logs a WARN and leaves the user to re-pick via
            // Settings (existing meeting-chord pattern).
            Self::CommandCenterChord => serde_json::json!("RightCtrl+Space"),
            Self::CommandCenterSeenV1 => serde_json::json!(false),
            // Default OFF for new users. The one-shot migration in
            // `meetings/runtime.rs` flips this to `true` on first boot
            // for any profile that has a non-default `meeting_hotkey_*`
            // value (i.e. an existing user who has been driving the
            // direct chord pre-CC).
            Self::LegacyMeetingChordEnabled => serde_json::json!(false),
            // Phase 10 Wave 4. Privacy by default — Principle 4 spirit.
            // The user opts in via Settings; the Command Center reads
            // this value at start time.
            Self::ActivityAudioEnabled => serde_json::json!(false),
            // Phase 10 Wave 5. All TTLs default to 0 = forever (user
            // opt-in). LastSweepMs starts at 0 = never.
            Self::ActivityRetentionEventsDays => serde_json::json!(0),
            Self::ActivityRetentionSegmentsDays => serde_json::json!(0),
            Self::ActivityRetentionBlocksDays => serde_json::json!(0),
            Self::ActivityRetentionLastSweepMs => serde_json::json!(0),
            // ADR 0046 Iter 2. All defaults are opt-in / OFF.
            Self::MobileSyncEnabled => serde_json::json!(false),
            Self::VaultPath => serde_json::json!(null),
            Self::VaultSyncRecordTypes => serde_json::json!("both"),
            Self::VaultRetentionDays => serde_json::json!(30),
            // Iter 4 — surfaced via the Mobile Sync settings tab.
            // `vault_sync_backend` selects the Obsidian Sync tier so
            // the byte-cap warning + iOS Shortcut tier hints make
            // sense; default is the most common (Standard) plan.
            Self::VaultSyncBackend => serde_json::json!("obsidian-sync-standard"),
            // 5 MB matches Obsidian Sync Standard's per-file ceiling.
            // Plus tier raises this; the UI flips the default when
            // the user switches `VaultSyncBackend` but the persisted
            // value is whatever the user last set.
            Self::SyncTierByteCap => serde_json::json!(5_000_000),
            // Dev-only toggle; OFF means the courier's normal
            // archive/clean behaviour runs.
            Self::VaultDebugKeepCouriers => serde_json::json!(false),
            // ON by default per ADR 0046 Iter 4: a user enabling
            // mobile sync probably wants their audio retained
            // alongside the transcript so they can re-transcribe
            // after a model upgrade. Turning OFF deletes the source
            // audio after a successful ingest to save space.
            Self::KeepAudioBlobs => serde_json::json!(true),
            // ADR 0047 Wave 1.2 — 0.65 chosen as the inflection point
            // between "plausibly-aggressive cleanup" and "content loss".
            // Stored as a setting so we can tune without code change
            // (ADR §Risk #2).
            Self::LlmShrinkFallbackThreshold => serde_json::json!(0.65),
            // ADR 0047 Wave 2.2 — 12 words is the empirical inflection
            // point between "preprocessor already handled this" and
            // "long enough for an LLM pass to matter". Stored as a
            // setting so the threshold is tunable without code change.
            Self::LlmSkipWordThreshold => serde_json::json!(12),
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
            // Phase 10 Wave 1A.
            Self::CommandCenterChord,
            Self::CommandCenterSeenV1,
            Self::LegacyMeetingChordEnabled,
            // Phase 10 Wave 4.
            Self::ActivityAudioEnabled,
            // Phase 10 Wave 5.
            Self::ActivityRetentionEventsDays,
            Self::ActivityRetentionSegmentsDays,
            Self::ActivityRetentionBlocksDays,
            Self::ActivityRetentionLastSweepMs,
            // ADR 0046 Iter 2.
            Self::MobileSyncEnabled,
            Self::VaultPath,
            Self::VaultSyncRecordTypes,
            Self::VaultRetentionDays,
            Self::VaultSyncBackend,
            Self::SyncTierByteCap,
            Self::VaultDebugKeepCouriers,
            Self::KeepAudioBlobs,
            // ADR 0047 Wave 1.2.
            Self::LlmShrinkFallbackThreshold,
            // ADR 0047 Wave 2.2.
            Self::LlmSkipWordThreshold,
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
        // 8 original + 11 Phase MC + 1 Phase MC W5 (MeetingHotkeyPaused)
        //   + 3 Phase 10 W1A (CommandCenter* + LegacyMeetingChordEnabled)
        //   + 1 Phase 10 W4 (ActivityAudioEnabled)
        //   + 4 Phase 10 W5 (ActivityRetention* x 4)
        //   + 8 ADR 0046 Iter 2 (MobileSync/Vault* x 8)
        //   + 1 ADR 0047 Wave 1.2 (LlmShrinkFallbackThreshold)
        //   + 1 ADR 0047 Wave 2.2 (LlmSkipWordThreshold) = 38.
        assert_eq!(SettingKey::all().len(), 38);
    }

    /// ADR 0046 Iter 2 defaults must match §10 ("opt-in by default";
    /// every behavioral key starts OFF or unset so a user who hasn't
    /// touched the Mobile Sync tab sees zero outbound writes to disk).
    #[test]
    fn mobile_sync_defaults_match_adr_0046() {
        use serde_json::json;
        assert_eq!(SettingKey::MobileSyncEnabled.default_value(), json!(false));
        assert!(SettingKey::VaultPath.default_value().is_null());
        assert_eq!(
            SettingKey::VaultSyncRecordTypes.default_value(),
            json!("both")
        );
        assert_eq!(SettingKey::VaultRetentionDays.default_value(), json!(30));
        assert_eq!(
            SettingKey::VaultSyncBackend.default_value(),
            json!("obsidian-sync-standard")
        );
        assert_eq!(
            SettingKey::SyncTierByteCap.default_value(),
            json!(5_000_000)
        );
        assert_eq!(
            SettingKey::VaultDebugKeepCouriers.default_value(),
            json!(false)
        );
        // Iter 4: KeepAudioBlobs flipped to default-ON so enabling
        // mobile sync retains source audio for future re-transcribes;
        // the user opts INTO deletion via the Advanced toggle.
        assert_eq!(SettingKey::KeepAudioBlobs.default_value(), json!(true));
    }

    /// Phase 10 Wave 1A defaults must match ADR 0037 §Decision items
    /// Q1, Q4, Q5. Drift here breaks the conflict probe + first-run
    /// auto-open + the legacy-chord migration.
    #[test]
    fn command_center_defaults_match_adr_0037() {
        use serde_json::json;
        assert_eq!(
            SettingKey::CommandCenterChord.default_value(),
            json!("RightCtrl+Space")
        );
        assert_eq!(
            SettingKey::CommandCenterSeenV1.default_value(),
            json!(false)
        );
        assert_eq!(
            SettingKey::LegacyMeetingChordEnabled.default_value(),
            json!(false)
        );
    }
}
