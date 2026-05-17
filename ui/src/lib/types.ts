// Shared types between the UI and the Rust Tauri command layer.
//
// These must stay in 1:1 sync with the Rust `serde` representations in
// `src-tauri/src/commands/types.rs`. If you add a field on one side
// without the other, the Tauri invoke deserialization will throw at
// runtime and the corresponding Playwright spec will fail loudly.

export type ThemeChoice = "system" | "light" | "dark";
export type InjectionStatus = "ok" | "aborted" | "secure_input" | "focus_changed";

export interface InsightsSnapshot {
  today: {
    words: number;
    sessions: number;
    recordingMs: number;
    timeSavedMs: number;
  };
  /** Consecutive days with at least one successful dictation. */
  streakDays: number;
  /** Word count for each of the last 7 days, oldest first. */
  sparkline7d: number[];
  modeMix: { slug: string; label: string; count: number }[];
  topApps: { app: string; count: number }[];
  latency: {
    sttMs: number;
    cleanupMs: number;
    injectMs: number;
  };
  learning: {
    lastRunAt: string | null;
    committedStreak: number;
    lastRolledBack: boolean;
    recentTerms: string[];
  };
}

export interface SessionSummary {
  id: number;
  uuid: string;
  modeSlug: string;
  startedAt: string;
  durationMs: number;
  foregroundApp: string | null;
  foregroundWindowTitle: string | null;
  finalText: string;
  injectionStatus: InjectionStatus;
}

export interface SessionDetail {
  session: SessionSummary;
  raw: string;
  cleaned: string;
  final: string;
  modelUsed: string | null;
  promptVersion: string | null;
  dictionaryVersion: number | null;
  latency: {
    sttMs: number | null;
    cleanupMs: number | null;
    injectMs: number | null;
  };
}

export interface TranscriptSearchHit {
  sessionId: number;
  stage: "raw" | "cleaned" | "final";
  /** FTS5 snippet with `<mark>...</mark>` highlights. */
  snippet: string;
  startedAt: string;
  modeSlug: string;
}

export interface DictionaryEntry {
  id: number;
  term: string;
  canonical: string | null;
  source: "user" | "learned" | "import";
  confidence: number;
  appContext: string | null;
  useCount: number;
  lastUsedAt: string | null;
  createdAt: string;
}

export interface ModeRow {
  slug: string;
  label: string;
  enabled: boolean;
  modelId: string;
  provider: "ollama" | "claude";
  temperature: number;
  maxTokens: number;
  hotkey: string;
  promptVersion: string;
}

/**
 * Currently-selected transcription mode. The orchestrator resolves
 * this fresh at the start of every dictation, so `set_active_mode`
 * takes effect on the NEXT Right-Alt hold without any restart.
 *
 * AI command modes (rewrite/expand/summarize) are NOT eligible to be
 * set as active — they act on existing text via their own hotkeys.
 */
export interface ActiveMode {
  /** Slug of the active transcription mode (e.g. `"normal"`). */
  slug: string;
}

/**
 * The fixed allowlist of mode slugs that can be set as active. Kept
 * in sync with `TRANSCRIPTION_SLUGS` in
 * `src-tauri/src/commands/active_mode.rs`. The Modes UI uses this to
 * decide which mode cards get the "Set active" radio treatment vs.
 * the legacy enable/disable + hotkey treatment.
 */
/**
 * Updated for Wave 2 of ADR 0022 (migration 008). The pre-Wave-2
 * trio (`normal` / `verbose` / `fragment`) was replaced with the
 * three focused modes below. `verbose` and `fragment` rows still
 * exist in the database (soft-disabled) so historical session rows
 * resolve correctly, but they're not pickable as the active mode.
 */
export const TRANSCRIPTION_SLUGS = ["casual", "normal", "formal"] as const;
export type TranscriptionSlug = (typeof TRANSCRIPTION_SLUGS)[number];

export function isTranscriptionSlug(slug: string): slug is TranscriptionSlug {
  return (TRANSCRIPTION_SLUGS as readonly string[]).includes(slug);
}

export interface SettingsSnapshot {
  theme: ThemeChoice;
  soundEnabled: boolean;
  autostart: boolean;
  reducedMotion: boolean;
  retentionDays: number;
  audioRetention: boolean;
  learningEnabled: boolean;
  claudeKeyConfigured: boolean;
}

export interface LearningRun {
  id: number;
  startedAt: string;
  completedAt: string | null;
  sessionsAnalyzed: number | null;
  correctionsClassified: number | null;
  examplesAdded: number | null;
  examplesRemoved: number | null;
  dictionaryTermsAdded: number | null;
  rolledBack: boolean;
  notes: string | null;
}

/* ------------------------------------------------------------------ */
/* Recording overlay state — pushed from Rust via Tauri events.       */
/* ------------------------------------------------------------------ */

export type RecordingPhase =
  | "idle"
  | "recording"
  | "processing"
  | "ok"
  | "error";

export interface RecordingState {
  phase: RecordingPhase;
  modeSlug: string;
  modeLabel: string;
  /** Linear 0..1 RMS level — sampled by Rust ~60 Hz. */
  audioLevel: number;
  /** Wall-clock seconds since RECORDING entered. */
  elapsedSec: number;
  /** Optional message — typically only set during error phase. */
  message?: string;
}
