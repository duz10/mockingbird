//! Serde-shaped DTOs returned from Tauri commands.
//!
//! These MUST stay in 1:1 sync with `ui/src/lib/types.ts`. Add a
//! field on one side, add it on the other in the same commit, or
//! the UI's `invoke` deserialize will panic at runtime + the
//! Playwright spec will surface it.
//!
//! camelCase serialization is enforced — JS likes it, and tracking
//! a single rename rule is easier than two.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsSnapshot {
    pub today: InsightsToday,
    pub streak_days: i64,
    pub sparkline7d: Vec<i64>,
    pub mode_mix: Vec<ModeMixEntry>,
    pub top_apps: Vec<TopAppEntry>,
    pub latency: LatencyBreakdown,
    pub learning: LearningSummary,
    /// All-time totals across dictation + meetings.
    pub lifetime: LifetimeTotals,
    /// Longest consecutive-day streak ever recorded (≥ current).
    pub longest_streak_days: i64,
    /// Daily activity for the last 365 days, oldest first. Used by
    /// the GitHub-style contribution heatmap on the Usage tab.
    pub heatmap365d: Vec<HeatmapDay>,
    /// Session-start counts per hour-of-day (0..=23), last 90 days.
    /// Index 0 = midnight–1am, index 23 = 11pm–midnight.
    pub peak_hours: Vec<i64>,
    /// Most-used dictionary terms by `use_count`. Top 8.
    pub top_dict_terms: Vec<DictTermEntry>,
    /// Most-corrected raw tokens (from the corrections table). Top 8.
    pub top_corrections: Vec<CorrectionEntry>,
    /// Words-per-minute stats over the last 30 days.
    pub wpm: WpmStats,
    /// ADR 0047 §Wave 2.5 / mb-v2fa -- edit-free-send rate, the
    /// inverse-of-success metric that proves whether the rest of
    /// the cleanup-pipeline refinement worked. See
    /// `EditFreeSendStats`.
    pub edit_free_send: EditFreeSendStats,
}

/// ADR 0047 §Wave 2.5 / mb-v2fa -- aggregation of
/// `sessions.edit_free_within_5min` over two time windows.
///
/// The metric is binary per session: an injected session either
/// did or did not have an edit-equivalent action within 5 min of
/// injection. Sessions that never injected (in-app, abort, headless
/// ingest) are excluded from both numerator AND denominator -- they
/// have no `edit_free_within_5min` value to aggregate over.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFreeSendStats {
    /// All-time bucket.
    pub lifetime: EditFreeBucket,
    /// Last 30 days, anchored to `processing_completed_at`.
    pub last30d: EditFreeBucket,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFreeBucket {
    /// Total injected sessions in the window (denominator).
    pub injected: i64,
    /// Injected sessions that survived the 5-min observation
    /// window without an edit-equivalent action (numerator).
    pub edit_free: i64,
    /// `edit_free / injected`, 0..=1. `None` when injected = 0
    /// (avoid divide-by-zero; UI renders as "—" instead).
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifetimeTotals {
    pub dictation_words: i64,
    pub dictation_sessions: i64,
    pub dictation_recording_ms: i64,
    pub meetings_count: i64,
    pub meetings_total_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapDay {
    /// `YYYY-MM-DD` (local date).
    pub date: String,
    pub sessions: i64,
    pub words: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictTermEntry {
    pub term: String,
    pub use_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionEntry {
    pub before: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpmStats {
    /// Average WPM over sessions in the last 30 days. `None` if
    /// there's no qualifying sample.
    pub avg_wpm: Option<f64>,
    /// Number of sessions that contributed to the average. Helps the
    /// UI render "based on N sessions" honestly.
    pub samples: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsToday {
    pub words: i64,
    pub sessions: i64,
    pub recording_ms: i64,
    pub time_saved_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeMixEntry {
    pub slug: String,
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopAppEntry {
    pub app: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyBreakdown {
    pub stt_ms: Option<f64>,
    pub cleanup_ms: Option<f64>,
    pub inject_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningSummary {
    pub last_run_at: Option<String>,
    pub committed_streak: i64,
    pub last_rolled_back: bool,
    pub recent_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: i64,
    pub uuid: String,
    pub mode_slug: String,
    pub started_at: String,
    pub duration_ms: i64,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub final_text: String,
    pub injection_status: String,
    /// ADR 0045 + mb-tfyp — `"ptt"` or `"in_app"`. Distinguishes
    /// hotkey-triggered sessions from programmatic-start ones so
    /// the UI list-pill can render IN_APP instead of the
    /// (semantically wrong) ABORTED_FOCUS_CHANGED for in-app rows.
    pub start_mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: SessionSummary,
    pub raw: String,
    pub cleaned: String,
    pub r#final: String,
    pub model_used: Option<String>,
    pub prompt_version: Option<String>,
    pub dictionary_version: Option<i64>,
    pub latency: LatencyBreakdown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSearchHit {
    pub session_id: i64,
    pub stage: String,
    pub snippet: String,
    pub started_at: String,
    pub mode_slug: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryDto {
    pub id: i64,
    pub term: String,
    pub canonical: Option<String>,
    pub source: String,
    pub confidence: f64,
    pub app_context: Option<String>,
    pub use_count: i64,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeDto {
    pub slug: String,
    pub label: String,
    pub enabled: bool,
    pub model_id: String,
    pub provider: String,
    pub temperature: f64,
    pub max_tokens: i64,
    pub hotkey: String,
    pub prompt_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub theme: String,
    pub sound_enabled: bool,
    pub autostart: bool,
    pub reduced_motion: bool,
    pub retention_days: i64,
    pub audio_retention: bool,
    pub learning_enabled: bool,
    pub claude_key_configured: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningRunDto {
    pub id: i64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub sessions_analyzed: Option<i64>,
    pub corrections_classified: Option<i64>,
    pub examples_added: Option<i64>,
    pub examples_removed: Option<i64>,
    pub dictionary_terms_added: Option<i64>,
    pub rolled_back: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub data_dir: String,
    pub logs_dir: String,
    pub models_dir: String,
}
