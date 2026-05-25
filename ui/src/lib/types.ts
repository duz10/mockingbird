// Shared types between the UI and the Rust Tauri command layer.
//
// These must stay in 1:1 sync with the Rust `serde` representations in
// `src-tauri/src/commands/types.rs`. If you add a field on one side
// without the other, the Tauri invoke deserialization will throw at
// runtime and the corresponding Playwright spec will fail loudly.

export type ThemeChoice = "system" | "light" | "dark";
/** Canonical DB string of an injection outcome.
 *
 * Mirrors `InjectionOutcome::as_db_str` on the Rust side. New variants:
 *   - `"in_app"` (ADR 0045 + mb-tfyp) — programmatic in-app dictation,
 *     no inject attempted by design (not a failure).
 *
 * Kept as a string union (not narrow literals) because legacy rows can
 * carry pre-Phase-1 values like `"unknown"` and the UI must render
 * something useful regardless. */
export type InjectionStatus = string;

/** ADR 0045 + mb-tfyp — which path started a dictation session.
 *  Persisted as `sessions.start_mode` (migration 017). */
export type StartMode = "ptt" | "in_app";

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
  /** All-time totals across dictation + meetings. */
  lifetime: {
    dictationWords: number;
    dictationSessions: number;
    dictationRecordingMs: number;
    meetingsCount: number;
    meetingsTotalMs: number;
  };
  /** Longest consecutive-day dictation streak ever. */
  longestStreakDays: number;
  /** Daily activity for the last 365 days, oldest first. */
  heatmap365d: { date: string; sessions: number; words: number }[];
  /** Session-start counts per hour-of-day (0..=23), last 90 days. */
  peakHours: number[];
  /** Top 8 dictionary terms by use_count. */
  topDictTerms: { term: string; useCount: number }[];
  /** Top 8 most-corrected raw tokens, last 90 days. */
  topCorrections: { before: string; count: number }[];
  /** Average WPM over the last 30 days. `null` when no qualifying samples. */
  wpm: { avgWpm: number | null; samples: number };
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
  /** ADR 0045 + mb-tfyp. Drives the list-pill semantics: in-app
   *  sessions render `IN_APP` (neutral) instead of leaking the
   *  ABORTED_FOCUS_CHANGED legacy heuristic which doesn't apply
   *  when there was no target app. Defaults to `"ptt"` for
   *  pre-migration-017 rows. */
  startMode: StartMode;
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

/** ADR 0046 §3.2 / mb-7vyz — response from the `dictation_import_file`
 *  IPC. Surfaced to the Dictations page toast on success; not
 *  persisted in the UI store (the SessionsEventBus event will
 *  trigger a list refetch and the row picks itself up there). */
export interface SessionImportSummary {
  /** New `sessions.id` row id, for optional scroll-to-row. */
  sessionId: number;
  /** Always `"desktop-import"` for the + Audio file path; the Iter 3
   *  inbox watcher persists `"mobile-inbox"` through the same
   *  channel. Kept as a free-form string union for forward-compat. */
  source: string;
  /** First ~120 chars of the cleaned (or raw, on fallback) transcript
   *  — already truncated server-side with a trailing ellipsis. */
  transcriptPreview: string;
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

/* ------------------------------------------------------------------ */
/* Meeting capture — Phase MC. Wire shapes mirror Rust serde derives  */
/* in `src-tauri/src/meetings/` and `src-tauri/src/commands/meetings.rs`. */
/* ------------------------------------------------------------------ */

/** Persisted status of a meeting row (matches `MeetingStatus::as_db_str`). */
export type MeetingStatus =
  | "complete"
  | "partial"
  | "demoted"
  | "interrupted"
  | "failed";

/** Which audio source(s) the meeting captured. */
export type MeetingSourceKind = "mic" | "system" | "both";

/** Result of `meeting_probe_sources`. Available BEFORE a meeting starts. */
export interface MeetingSourceProbe {
  micAvailable: boolean;
  systemAvailable: boolean;
}

/** History-list row. Narrow on purpose — no transcript body. */
export interface MeetingSummary {
  uuid: string;
  title: string | null;
  startedAt: string;
  totalDurationMs: number;
  status: MeetingStatus;
  source: MeetingSourceKind;
}

/**
 * Detail view of one meeting. Mirror of `meetings::repo::MeetingDetail`.
 * Any of the formatted-* fields can be null depending on `source` +
 * `status` (e.g. system-only meeting has `formattedMic == null`).
 */
export interface MeetingDetail {
  uuid: string;
  title: string | null;
  startedAt: string;
  endedAt: string;
  status: MeetingStatus;
  errorMessage: string | null;
  source: MeetingSourceKind;
  totalDurationMs: number;
  micDurationMs: number | null;
  sysDurationMs: number | null;
  formatterVersion: string;
  whisperModelId: string;
  formattedMic: string | null;
  formattedSys: string | null;
  formattedMerged: string | null;
}

/** One FTS5 hit returned by `search_meeting_transcripts`. */
export interface MeetingMatch {
  uuid: string;
  title: string | null;
  startedAt: string;
  /** FTS5 snippet — has `<mark>...</mark>` around hits. */
  snippet: string;
  /** Which channel matched: `"mic"` | `"system"` | `"merged"`. */
  channel: string;
}

/** Built-in LLM-pass prompts. Custom prompts ride as `{ custom: body }`.
 *
 * NOTE: this union spans every built-in any caller might pass. The
 * dictation handler accepts the full set; the meetings handler
 * recognises summary / action_items / cleaner_punctuation only and
 * rejects unknown names. UI pickers gate per-context.
 */
export type BuiltInPromptId =
  | "summary"
  | "action_items"
  | "cleaner_punctuation"
  | "compress";

/** Wire shape for `meeting_run_llm_pass.promptId`. */
export type LlmPassPromptArg = BuiltInPromptId | { custom: string };

/** Result of one LLM-pass run. Output is NOT persisted to DB. */
export interface LlmPassResult {
  /** Client-side handle — pass back in `include_llm_pass` for export. */
  id: string;
  text: string;
  latencyMs: number;
}

/** Payload of the `meeting:state` event emitted by the Rust runtime. */
export interface MeetingStateEvent {
  /** `"started" | "transcribing" | "formatting" | "done" | "error" | "warn-already-running" | …` */
  state: string;
  uuid: string | null;
  /** DB-form source string (`"mic" | "system" | "both"`), nullable on warn events. */
  source: MeetingSourceKind | null;
}

/** Payload of `meetings:session-saved` — fires after the DB row commits. */
export interface MeetingSessionSavedEvent {
  uuid: string;
  sessionRowid: number;
}

/** Phase MC Wave 5 — typed snapshot of the meeting-side `SettingKey`
 *  registry. Mirrors `MeetingSettingsSnapshot` in
 *  `src-tauri/src/commands/settings.rs`. Read via
 *  `api.meeting_settings_get_all()`; individual fields are written
 *  via `api.meeting_settings_set(key, value)` (key = the db string
 *  per `SettingKey::as_str`). `hotkeyPaused` is read-only here —
 *  changes go through `meetings.setPaused()` so the activation
 *  channel gets the PauseToggle event too. */
export interface MeetingSettingsSnapshot {
  hotkeyModifier: string;
  hotkeyKey: string;
  defaultSource: "mic" | "system" | "both";
  maxDurationSeconds: number;
  fillerStripEnabled: boolean;
  paragraphGapMs: number;
  audioRetentionDays: number | null;
  llmPassEnabled: boolean;
  speakerLabelMic: string;
  speakerLabelSys: string;
  hotkeyPaused: boolean;
}

/** Payload of `meeting:progress` — fires while the long-form STT
 *  driver walks chunks. One event per chunk per channel; `chunksTotal`
 *  is `null` until the capture-side `Receiver` closes (= no more
 *  chunks coming), then becomes a fixed number. */
export interface MeetingProgressEvent {
  channel: "mic" | "system";
  chunksDone: number;
  chunksTotal: number | null;
}

/** Payload of `meeting:tick` — fires every ~250ms while a meeting
 *  is in flight. ADR 0032 / mb-nig. `micDb` / `sysDb` are dBFS in
 *  `[-100, 0]`; the value `0.0` (exactly) is the "no data yet"
 *  sentinel for an inactive or as-yet-undrained channel — the UI
 *  should render that as a flat bar rather than "silence". */
export interface MeetingTickEvent {
  uuid: string;
  elapsedMs: number;
  micDb: number;
  sysDb: number;
}

/** ADR 0046 Iter 2 / mb-vg3p — typed snapshot of every Mobile-Sync
 *  settings key. Read via `api.vault_settings_get()`; individual
 *  fields written via `api.vault_settings_set(key, value)` where
 *  `key` matches the Rust-side `SettingKey::as_str()` value
 *  (snake_case — see `dbKeyForVaultSetting` in `SettingsMobileSyncTab`).
 *  The set IPC also fires a backfill trigger so a freshly-enabled
 *  vault populates without an explicit Export-now click.
 *
 *  Iter 4 / mb-vg3p extends the snapshot with the four remaining
 *  ADR 0046 keys: `vaultSyncBackend`, `syncTierByteCap`,
 *  `keepAudioBlobs`, `vaultDebugKeepCouriers`. */
export type VaultSyncBackend =
  | "obsidian-sync-standard"
  | "obsidian-sync-plus"
  | "manual";

export interface VaultSettingsSnapshot {
  mobileSyncEnabled: boolean;
  vaultPath: string | null;
  vaultSyncRecordTypes: "dictation" | "meeting" | "both";
  vaultRetentionDays: number;
  vaultSyncBackend: VaultSyncBackend;
  /** Per-file size warning threshold in bytes. */
  syncTierByteCap: number;
  /** When true, the inbox courier moves processed files to
   *  `_archive/` instead of deleting them. */
  keepAudioBlobs: boolean;
  /** Developer-only retention of intermediate courier files. */
  vaultDebugKeepCouriers: boolean;
}

/** ADR 0046 Iter 4 / mb-vg3p — outbound projection runtime status.
 *  Polled every 5s while the Mobile Sync settings tab is mounted.
 *  Cheap to compute (reads config + stats `manifest.json`). */
export interface VaultRuntimeStatus {
  running: boolean;
  manifestAgeMs: number | null;
  manifestModifiedIso: string | null;
  lastError: string | null;
}

/** ADR 0046 Iter 4 / mb-vg3p — inbox courier runtime status.
 *  Polled alongside `VaultRuntimeStatus`. `failedCount` drives the
 *  "open `inbox/_failed/`" link when greater than zero. */
export interface InboxRuntimeStatus {
  running: boolean;
  watchPath: string | null;
  lastArchivedIso: string | null;
  failedCount: number;
  lastError: string | null;
}

/** Result of a manual `vault_export_now` IPC call. `skipped` is true
 *  when the runtime short-circuited because sync was disabled or no
 *  vault path was configured -- the UI renders a different toast in
 *  that case. Otherwise `changes` / `archived` / `total` count the
 *  actual reconciliation work. */
export interface VaultExportSummary {
  total: number;
  changes: number;
  archived: number;
  skipped: boolean;
}

/** ADR 0046 Iter 4 / mb-3xww — pre-flight check for a candidate
 *  vault path. The Settings UI runs this BEFORE persisting
 *  `VaultPath` so it can surface a guided dialog when the user
 *  pointed at a folder INSIDE an existing Obsidian vault (both
 *  vaults would then race to own `.obsidian/`, which is one of
 *  the original Iter 2 smoke-test traps).
 *
 *  Wire-format mirror of the Rust `commands::vault::VaultPathCheck`
 *  enum, serialized with `tag: "kind"`. */
export type VaultPathCheck =
  | { kind: "ok" }
  | {
      kind: "nestedVault";
      parentVault: string;
      suggestedSibling: string | null;
    };
