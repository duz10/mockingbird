// Thin shim over @tauri-apps/api so the UI runs in three contexts:
//
//   1. Inside the real Tauri shell  → calls invoke() for real.
//   2. Vite preview (`npm run preview`) → returns fixtures.
//   3. Playwright tests              → fixtures, plus per-test overrides.
//
// Why this exists: Tauri's webview injects `__TAURI_INTERNALS__` at
// boot. When that's absent we know we're in a plain browser context;
// returning fixtures lets the UI render fully without a Rust process,
// which means component-level Playwright specs can hit `npm run preview`
// directly. The same module is used in unit tests via jsdom.
//
// Tauri command naming: see `src-tauri/src/commands/mod.rs`.
// Every command name here must match a `#[tauri::command]` over there.

import type {
  ActiveMode,
  DictionaryEntry,
  InsightsSnapshot,
  LearningRun,
  LlmPassPromptArg,
  LlmPassResult,
  MeetingSettingsSnapshot,
  ModeRow,
  SessionDetail,
  SessionSummary,
  SettingsSnapshot,
  TranscriptSearchHit,
} from "./types";

/** True when we're running inside an actual Tauri shell. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Per-command fixture overrides — settable from Playwright specs via
 * `window.__MOCKINGBIRD_FIXTURES__`. Lets a test stage "what if the
 * dictionary has 1,000 entries" without spinning up Rust.
 */
type FixtureMap = Partial<Record<string, unknown>>;
declare global {
  interface Window {
    __MOCKINGBIRD_FIXTURES__?: FixtureMap;
  }
}

function fixture<T>(command: string, fallback: T): T {
  const overrides =
    typeof window !== "undefined" ? window.__MOCKINGBIRD_FIXTURES__ : undefined;
  if (overrides && command in overrides) {
    return overrides[command] as T;
  }
  return fallback;
}

/**
 * Generic invoke wrapper. In Tauri context delegates to the real
 * invoke; outside Tauri returns a fixture (or throws for write
 * commands that have no fixture).
 */
export async function invoke<T>(command: string, args?: object): Promise<T> {
  if (isTauri()) {
    const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
    return tauriInvoke<T>(command, args as Record<string, unknown>);
  }
  return fixtureFor<T>(command, args);
}

/* ------------------------------------------------------------------ */
/* Typed wrappers — every command the UI calls goes through one of    */
/* these. Adding a new command? Add it both here and as a fixture     */
/* in `fixtureFor` below. The TS compiler will catch UI callers if    */
/* you forget.                                                        */
/* ------------------------------------------------------------------ */

export const api = {
  // Insights
  insights_snapshot: () => invoke<InsightsSnapshot>("insights_snapshot"),

  // Sessions / history
  list_sessions: (limit: number, offset: number) =>
    invoke<SessionSummary[]>("list_sessions", { limit, offset }),
  get_session_detail: (id: number) =>
    invoke<SessionDetail>("get_session_detail", { id }),
  search_transcripts: (query: string, limit: number) =>
    invoke<TranscriptSearchHit[]>("search_transcripts", { query, limit }),
  delete_session: (id: number) => invoke<void>("delete_session", { id }),
  mark_session_as_example: (id: number) =>
    invoke<void>("mark_session_as_example", { id }),
  report_correction: (sessionId: number, before: string, after: string) =>
    invoke<number>("report_correction", { sessionId, before, after }),
  /** Run the optional dictation LLM-pass over a session's transcript.
   *  Returns the text directly; nothing is cached server-side (see
   *  `dictation_run_llm_pass` in commands/sessions.rs for the
   *  rationale — no export pipeline = no need for a cache id). */
  dictation_run_llm_pass: (
    sessionId: number,
    promptId: LlmPassPromptArg,
    modelId?: string,
  ) =>
    invoke<LlmPassResult>("dictation_run_llm_pass", {
      sessionId,
      promptId,
      modelId,
    }),

  // Dictionary
  list_dictionary: () => invoke<DictionaryEntry[]>("list_dictionary"),
  // upsert: when `id` is set the Rust side updates the existing row;
  // when omitted it inserts a new one. Either way the returned number
  // is the row id.
  upsert_dictionary_entry: (
    entry: Omit<DictionaryEntry, "id" | "createdAt" | "useCount" | "lastUsedAt"> & {
      id?: number;
    },
  ) => invoke<number>("upsert_dictionary_entry", { entry }),
  delete_dictionary_entry: (id: number) =>
    invoke<void>("delete_dictionary_entry", { id }),

  // Modes
  list_modes: () => invoke<ModeRow[]>("list_modes"),
  update_mode: (slug: string, patch: Partial<ModeRow>) =>
    invoke<void>("update_mode", { slug, patch }),
  get_active_mode: () => invoke<ActiveMode>("get_active_mode"),
  set_active_mode: (slug: string) => invoke<void>("set_active_mode", { slug }),

  // Settings
  get_settings: () => invoke<SettingsSnapshot>("get_settings"),
  update_setting: (key: string, value: string) =>
    invoke<void>("update_setting", { key, value }),

  /**
   * Phase 10 Wave 1A/1B — read/write the typed `SettingKey` registry
   * (`settings/model.rs`). Distinct from the legacy `update_setting`
   * flat bag above. The chord-rebind row reads `command_center_chord`
   * via this path; the legacy-meeting-chord toggle writes
   * `legacy_meeting_chord_enabled` here.
   */
  legacy_get_setting: (key: string) =>
    invoke<unknown>("get_setting", { key }),
  legacy_set_setting: (key: string, value: unknown) =>
    invoke<void>("set_setting", { key, value }),

  // Phase MC Wave 5 — typed meeting-settings IPC.
  meeting_settings_get_all: () =>
    invoke<MeetingSettingsSnapshot>("meeting_settings_get_all"),
  meeting_settings_set: (key: string, value: unknown) =>
    invoke<void>("meeting_settings_set", { key, value }),

  // Learning loop
  list_learning_runs: (limit: number) =>
    invoke<LearningRun[]>("list_learning_runs", { limit }),
  trigger_learning_run: () => invoke<number>("trigger_learning_run"),

  // System
  open_path: (path: string) => invoke<void>("open_path", { path }),
  app_paths: () =>
    invoke<{ dataDir: string; logsDir: string; modelsDir: string }>(
      "app_paths",
    ),
  /**
   * Local Ollama `/api/tags`. Returns the installed model names
   * (e.g. `["qwen2.5:7b-instruct-q4_K_M", "gemma2:2b"]`) for the
   * Modes editor's model dropdown. Returns `[]` when Ollama is
   * unreachable so the UI falls back to free-text input.
   */
  list_installed_models: () =>
    invoke<string[]>("list_installed_models"),
};

/* ------------------------------------------------------------------ */
/* Fixtures — small but representative. Edit here for design work in  */
/* the browser. NOT used inside the Tauri shell.                      */
/* ------------------------------------------------------------------ */

function fixtureFor<T>(command: string, args?: object): T {
  switch (command) {
    case "insights_snapshot":
      return fixture(command, FIXTURES.insights) as T;
    case "list_sessions":
      return fixture(command, FIXTURES.sessions) as T;
    case "get_session_detail": {
      const id = (args as { id: number } | undefined)?.id ?? 1;
      const detail = FIXTURES.sessionDetails.find((s) => s.session.id === id);
      return fixture(command, detail ?? FIXTURES.sessionDetails[0]!) as T;
    }
    case "search_transcripts":
      return fixture(command, FIXTURES.searchHits) as T;
    case "list_dictionary":
      return fixture(command, FIXTURES.dictionary) as T;
    case "list_modes":
      return fixture(command, FIXTURES.modes) as T;
    case "get_active_mode":
      return fixture(command, FIXTURES.activeMode) as T;
    case "get_settings":
      return fixture(command, FIXTURES.settings) as T;
    case "meeting_settings_get_all":
      return fixture(command, {
        hotkeyModifier: "VK_RCONTROL",
        hotkeyKey: "VK_M",
        defaultSource: "mic",
        maxDurationSeconds: 14_400,
        fillerStripEnabled: true,
        paragraphGapMs: 2000,
        audioRetentionDays: null,
        llmPassEnabled: true,
        speakerLabelMic: "You",
        speakerLabelSys: "Other(s)",
        hotkeyPaused: false,
      } as MeetingSettingsSnapshot) as T;
    case "list_learning_runs":
      return fixture(command, FIXTURES.learningRuns) as T;
    case "app_paths":
      return fixture(command, {
        dataDir: "C:\\Users\\you\\AppData\\Roaming\\Mockingbird",
        logsDir: "C:\\Users\\you\\AppData\\Roaming\\Mockingbird\\logs",
        modelsDir: "C:\\Users\\you\\mockingbird_models",
      }) as T;
    case "list_installed_models":
      // Mirror the user's actual setup so the storybook-style
      // browser preview looks right.
      return fixture(command, [
        "qwen2.5:7b-instruct-q4_K_M",
        "qwen2.5:3b-instruct-q4_K_M",
        "gemma2:2b-instruct-q4_K_M",
      ]) as T;
    case "dictation_run_llm_pass":
      return fixture(command, {
        id: "fixture-dictation-llm-pass-id",
        text:
          "**Summary** — this is a fixture LLM-pass output for the Dictations tab. Browser-preview only.",
        latencyMs: 1240,
      }) as T;
    // Phase 10 Wave 1B — Activity capture fixtures.
    case "activity_list_sessions":
      return fixture(command, [
        {
          id: "fixture-activity-session-1",
          startedAt: Date.now() - 45 * 60_000,
          endedAt: Date.now() - 15 * 60_000,
          status: "completed",
          audioEnabled: false,
          screenshotEnabled: false,
          label: null,
          createdAt: Date.now() - 45 * 60_000,
          updatedAt: Date.now() - 15 * 60_000,
        },
      ]) as T;
    case "activity_get_session_detail": {
      const sid =
        (args as { sessionId?: string } | undefined)?.sessionId ??
        "fixture-activity-session-1";
      return fixture(command, {
        session: {
          id: sid,
          startedAt: Date.now() - 45 * 60_000,
          endedAt: Date.now() - 15 * 60_000,
          status: "completed",
          audioEnabled: false,
          screenshotEnabled: false,
          label: null,
          createdAt: Date.now() - 45 * 60_000,
          updatedAt: Date.now() - 15 * 60_000,
        },
        events: [
          {
            id: "fixture-evt-1",
            sessionId: sid,
            ts: Date.now() - 44 * 60_000,
            kind: "app_switch",
            appName: "Code.exe",
            windowTitle: "activity.rs — mockingbird",
            snapshotJson: null,
            createdAt: Date.now() - 44 * 60_000,
          },
          {
            id: "fixture-evt-2",
            sessionId: sid,
            ts: Date.now() - 30 * 60_000,
            kind: "app_switch",
            appName: "chrome.exe",
            windowTitle: "Activity Capture Plan — Notion",
            snapshotJson: null,
            createdAt: Date.now() - 30 * 60_000,
          },
        ],
      }) as T;
    }
    case "activity_runtime_snapshot":
      return fixture(command, {
        lifecycle: "idle",
        currentSessionId: null,
      }) as T;
    case "activity_start":
      return fixture(command, null as unknown as T);
    // Phase 10 Wave 3 — Summarization + Block CRUD fixtures (ADR 0040).
    case "activity_list_blocks":
      return fixture(command, [
        {
          id: "fixture-blk-1",
          sessionId: "fixture-activity-session-1",
          startedAt: Date.now() - 44 * 60_000,
          endedAt: Date.now() - 30 * 60_000,
          primaryApp: "Code.exe",
          label: null,
          primaryTitle: "activity.rs \u2014 mockingbird",
          generatedAbstract:
            "The user edited the activity-capture module in their Rust IDE.",
          userEdited: false,
          sourceEventIds: '["fixture-evt-1"]',
          promptVersionSha: "abstract_v1-deadbeef",
          createdAt: Date.now() - 30 * 60_000,
          updatedAt: Date.now() - 30 * 60_000,
        },
        {
          id: "fixture-blk-2",
          sessionId: "fixture-activity-session-1",
          startedAt: Date.now() - 30 * 60_000,
          endedAt: Date.now() - 15 * 60_000,
          primaryApp: "chrome.exe",
          label: null,
          primaryTitle: "Activity Capture Plan \u2014 Notion",
          generatedAbstract:
            "The user reviewed the activity-capture plan document in Notion.",
          userEdited: false,
          sourceEventIds: '["fixture-evt-2"]',
          promptVersionSha: "abstract_v1-deadbeef",
          createdAt: Date.now() - 15 * 60_000,
          updatedAt: Date.now() - 15 * 60_000,
        },
      ]) as T;
    case "activity_regenerate_summary":
    case "activity_render_work_report":
      return fixture(
        command,
        "# Activity \u2014 fixture\n\n_Markdown summary fixture for browser preview._" as unknown as T,
      );
    case "activity_block_split":
      return fixture(command, "fixture-blk-new" as unknown as T);
    // Phase 10 Wave 5 — Hardening fixtures (ADR 0042/0043/0044).
    case "activity_exclusion_list":
      return fixture(command, [
        {
          id: "builtin-1password",
          kind: "app_glob",
          pattern: "1Password*",
          enabled: true,
          isBuiltin: true,
          note: "1Password credentials manager",
          createdAt: Date.now() - 86_400_000,
          updatedAt: Date.now() - 86_400_000,
        },
        {
          id: "builtin-secure-input",
          kind: "system",
          pattern: "password_field_active",
          enabled: true,
          isBuiltin: true,
          note: "Drop snapshot when UIA reports an active password field",
          createdAt: Date.now() - 86_400_000,
          updatedAt: Date.now() - 86_400_000,
        },
      ]) as T;
    case "activity_exclusion_upsert":
      return fixture(command, "fixture-rule-new" as unknown as T);
    case "activity_retention_get":
      return fixture(command, {
        eventsDays: 0,
        segmentsDays: 0,
        blocksDays: 0,
        lastSweepMs: 0,
      }) as T;
    case "activity_retention_sweep_now":
      return fixture(command, {
        eventsDeleted: 0,
        segmentsDeleted: 0,
        blocksDeleted: 0,
        blocksMarkedPurged: 0,
        ranAtMs: Date.now(),
      }) as T;
    case "get_setting": {
      // Typed-settings read. Browser-preview only — the real values
      // come from the typed-registry IPC inside the Tauri shell.
      const key = (args as { key?: string } | undefined)?.key;
      if (key === "command_center_chord") return fixture(command, "RightCtrl+Space" as unknown as T);
      if (key === "legacy_meeting_chord_enabled") return fixture(command, false as unknown as T);
      return fixture(command, null as unknown as T);
    }
    case "set_setting":
      return fixture(command, undefined as unknown as T);
    // Phase 10 Wave 1A — Command Center fixtures (mb-n455).
    // Default to the "welcome" shape (firstRun=true + modePicker) so
    // visiting `/command_center.html` in browser preview shows the
    // picker without an override. qa-kitten flips this via
    // `window.__MOCKINGBIRD_FIXTURES__.cc_get_state` to screenshot
    // the modePicker (firstRun=false) and sessionCard states.
    case "cc_get_state":
      return fixture(command, {
        state: "modePicker",
        firstRun: true,
      } as unknown as T);
    // CC writes are all fire-and-forget from the UI's perspective;
    // the real backend emits a state event back. In browser mode we
    // no-op so clicks don't throw.
    case "cc_open_via_tray":
    case "cc_dismiss":
    case "cc_pick_mode":
    case "cc_stop_active_session":
    case "cc_update_session":
      return fixture(command, undefined as unknown as T);
    // mb-xnn7 forensic beacon — slated for removal in MC v1.3. Until
    // it's gone, give it a no-op fixture so the meeting overlay's
    // browser-preview entry doesn't spew unhandled rejections.
    case "meeting_debug_listener_ping":
      return fixture(command, undefined as unknown as T);
    // Write commands — no-op in fixture mode.
    case "delete_session":
    case "mark_session_as_example":
    case "upsert_dictionary_entry":
    case "delete_dictionary_entry":
    case "update_mode":
    case "set_active_mode":
    case "update_setting":
    case "meeting_settings_set":
    case "report_correction":
    case "trigger_learning_run":
    case "open_path":
    case "activity_pause":
    case "activity_resume":
    case "activity_stop":
    case "activity_delete_session":
    case "activity_block_rename":
    case "activity_block_rewrite_abstract":
    case "activity_block_delete":
    case "activity_block_merge":
    case "activity_export_markdown":
    case "activity_copy_to_clipboard":
    case "activity_exclusion_validate":
    case "activity_exclusion_set_enabled":
    case "activity_exclusion_delete":
    case "activity_retention_set":
    case "activity_export_pdf":
      return fixture(command, undefined as unknown as T);
    default:
      throw new Error(`fixtureFor: no fixture for command "${command}"`);
  }
}

// Exported so tests can import & mutate via `window.__MOCKINGBIRD_FIXTURES__`.
export const FIXTURES: {
  insights: InsightsSnapshot;
  sessions: SessionSummary[];
  sessionDetails: SessionDetail[];
  searchHits: TranscriptSearchHit[];
  dictionary: DictionaryEntry[];
  modes: ModeRow[];
  activeMode: ActiveMode;
  settings: SettingsSnapshot;
  learningRuns: LearningRun[];
} = {
  activeMode: { slug: "normal" },
  insights: {
    today: {
      words: 1842,
      sessions: 23,
      recordingMs: 11 * 60_000 + 24_000,
      timeSavedMs: 38 * 60_000,
    },
    streakDays: 14,
    sparkline7d: [320, 410, 220, 580, 1100, 760, 1842],
    modeMix: [
      { slug: "casual", label: "Casual", count: 9 },
      { slug: "normal", label: "Normal", count: 11 },
      { slug: "formal", label: "Formal", count: 3 },
    ],
    topApps: [
      { app: "slack.exe", count: 9 },
      { app: "Code.exe", count: 6 },
      { app: "chrome.exe", count: 4 },
      { app: "notepad.exe", count: 2 },
      { app: "outlook.exe", count: 2 },
    ],
    latency: {
      sttMs: 612,
      cleanupMs: 481,
      injectMs: 38,
    },
    learning: {
      lastRunAt: "2026-05-17T02:00:00Z",
      committedStreak: 6,
      lastRolledBack: false,
      recentTerms: ["kubectl", "Mockingbird", "Tauri", "OKLCH", "WisprFlow"],
    },
    lifetime: {
      dictationWords: 24_066,
      dictationSessions: 412,
      dictationRecordingMs: 6 * 3_600_000 + 17 * 60_000,
      meetingsCount: 18,
      meetingsTotalMs: 12 * 3_600_000 + 42 * 60_000,
    },
    longestStreakDays: 27,
    // Generate a plausibly-irregular 365-day heatmap: some weeks busy,
    // some sparse, weekend dips. Deterministic so fixture screenshots
    // are stable across reruns.
    heatmap365d: Array.from({ length: 365 }, (_, i) => {
      const daysAgo = 364 - i;
      const date = new Date(Date.now() - daysAgo * 86_400_000)
        .toISOString()
        .slice(0, 10);
      // Pseudo-random but deterministic: hash the index into a small
      // count. Weekends get reduced activity.
      const day = new Date(date).getDay();
      const isWeekend = day === 0 || day === 6;
      const seed = ((i * 1103515245 + 12345) >>> 0) % 11;
      const sessions = isWeekend ? Math.max(0, seed - 7) : seed;
      return { date, sessions, words: sessions * 78 };
    }),
    peakHours: [
      0, 0, 0, 0, 0, 0, 1, 4, 12, 28, 41, 33,
      22, 31, 38, 29, 24, 15, 9, 5, 3, 2, 1, 0,
    ],
    topDictTerms: [
      { term: "kubectl", useCount: 47 },
      { term: "Mockingbird", useCount: 36 },
      { term: "Tauri", useCount: 28 },
      { term: "OKLCH", useCount: 19 },
      { term: "qwen2.5", useCount: 14 },
      { term: "rusqlite", useCount: 11 },
      { term: "Ollama", useCount: 9 },
      { term: "WisprFlow", useCount: 6 },
    ],
    topCorrections: [
      { before: "missy", count: 8 },
      { before: "dustin", count: 6 },
      { before: "react", count: 5 },
      { before: "k8s", count: 4 },
      { before: "github", count: 3 },
      { before: "oh-em-gee", count: 2 },
    ],
    wpm: { avgWpm: 142.7, samples: 184 },
  },
  sessions: Array.from({ length: 40 }, (_, i) => ({
    id: 100 + i,
    uuid: `00000000-0000-0000-0000-${(100 + i).toString().padStart(12, "0")}`,
    modeSlug: ["casual", "normal", "formal"][i % 3]!,
    startedAt: new Date(Date.now() - i * 1000 * 60 * 17).toISOString(),
    durationMs: 4_000 + (i % 6) * 2_000,
    foregroundApp: ["slack.exe", "Code.exe", "chrome.exe", "notepad.exe"][i % 4]!,
    foregroundWindowTitle:
      ["#engineering — Slack", "main.rs — mockingbird", "Inbox — Gmail", "Untitled - Notepad"][i % 4]!,
    finalText:
      i === 0
        ? "Hey team, just pushed the Phase 4 LLM cleanup work. Provider abstraction works for both Ollama and Claude; tests are green."
        : i === 1
          ? "Add a `--verbose` flag to the build script so CI can see what step is hanging."
          : `Sample dictation #${i} — the quick brown fox jumps over the lazy dog.`,
    injectionStatus: i % 8 === 7 ? "aborted" : "ok",
  })),
  sessionDetails: [
    {
      session: {
        id: 100,
        uuid: "00000000-0000-0000-0000-000000000100",
        modeSlug: "normal",
        startedAt: new Date().toISOString(),
        durationMs: 6_400,
        foregroundApp: "slack.exe",
        foregroundWindowTitle: "#engineering — Slack",
        finalText:
          "Hey team, just pushed the Phase 4 LLM cleanup work. Provider abstraction works for both Ollama and Claude; tests are green.",
        injectionStatus: "ok",
      },
      raw: "hey team just pushed the phase four LLM cleanup work provider abstraction works for both olama and claude tests are green",
      cleaned:
        "Hey team, just pushed the Phase 4 LLM cleanup work. Provider abstraction works for both Ollama and Claude; tests are green.",
      final:
        "Hey team, just pushed the Phase 4 LLM cleanup work. Provider abstraction works for both Ollama and Claude; tests are green.",
      modelUsed: "qwen2.5:3b-instruct-q4_K_M",
      promptVersion: "normal@v1",
      dictionaryVersion: 7,
      latency: { sttMs: 612, cleanupMs: 481, injectMs: 38 },
    },
  ],
  searchHits: [],
  dictionary: [
    {
      id: 1,
      term: "Mockingbird",
      canonical: "Mockingbird",
      source: "user",
      confidence: 1.0,
      appContext: null,
      useCount: 42,
      lastUsedAt: new Date().toISOString(),
      createdAt: "2026-01-01T00:00:00Z",
    },
    {
      id: 2,
      term: "kubectl",
      canonical: "kubectl",
      source: "learned",
      confidence: 0.7,
      appContext: "terminal",
      useCount: 18,
      lastUsedAt: new Date(Date.now() - 86400_000).toISOString(),
      createdAt: "2026-03-12T00:00:00Z",
    },
    {
      id: 3,
      term: "OKLCH",
      canonical: "OKLCH",
      source: "user",
      confidence: 1.0,
      appContext: null,
      useCount: 4,
      lastUsedAt: new Date(Date.now() - 86400_000 * 3).toISOString(),
      createdAt: "2026-04-22T00:00:00Z",
    },
  ],
  modes: [
    // Browser-preview fixture mirrors the post-Wave-2 mode set
    // (migration 008). Updating these every migration is friction;
    // worth it because designers see the same shape as the shipped
    // app.
    {
      slug: "casual",
      label: "Casual",
      enabled: true,
      modelId: "qwen2.5:3b-instruct-q4_K_M",
      provider: "ollama",
      temperature: 0.4,
      maxTokens: 1024,
      hotkey: "Ctrl+Win+C",
      promptVersion: "v1",
    },
    {
      slug: "normal",
      label: "Normal",
      enabled: true,
      modelId: "qwen2.5:7b-instruct-q4_K_M",
      provider: "ollama",
      temperature: 0.1,
      maxTokens: 2048,
      hotkey: "Ctrl+Win",
      promptVersion: "v4",
    },
    {
      slug: "formal",
      label: "Formal",
      enabled: true,
      modelId: "qwen2.5:7b-instruct-q4_K_M",
      provider: "ollama",
      temperature: 0.1,
      maxTokens: 4096,
      hotkey: "Ctrl+Win+F",
      promptVersion: "v1",
    },
    {
      slug: "rewrite",
      label: "Rewrite",
      enabled: false,
      modelId: "qwen2.5:3b-instruct-q4_K_M",
      provider: "ollama",
      temperature: 0.4,
      maxTokens: 2048,
      hotkey: "Ctrl+Win+R",
      promptVersion: "v1",
    },
    {
      slug: "expand",
      label: "Expand",
      enabled: false,
      modelId: "qwen2.5:3b-instruct-q4_K_M",
      provider: "ollama",
      temperature: 0.5,
      maxTokens: 4096,
      hotkey: "Ctrl+Win+E",
      promptVersion: "v1",
    },
    {
      slug: "summarize",
      label: "Summarize",
      enabled: false,
      modelId: "qwen2.5:3b-instruct-q4_K_M",
      provider: "ollama",
      temperature: 0.3,
      maxTokens: 2048,
      hotkey: "Ctrl+Win+S",
      promptVersion: "v1",
    },
  ],
  settings: {
    theme: "system",
    soundEnabled: false,
    autostart: false,
    reducedMotion: false,
    retentionDays: 180,
    audioRetention: false,
    learningEnabled: true,
    claudeKeyConfigured: false,
  },
  learningRuns: [
    {
      id: 12,
      startedAt: "2026-05-17T02:00:00Z",
      completedAt: "2026-05-17T02:00:09Z",
      sessionsAnalyzed: 23,
      correctionsClassified: 8,
      examplesAdded: 3,
      examplesRemoved: 0,
      dictionaryTermsAdded: 2,
      rolledBack: false,
      notes: "committed",
    },
    {
      id: 11,
      startedAt: "2026-05-16T02:00:00Z",
      completedAt: "2026-05-16T02:00:11Z",
      sessionsAnalyzed: 31,
      correctionsClassified: 12,
      examplesAdded: 4,
      examplesRemoved: 1,
      dictionaryTermsAdded: 1,
      rolledBack: false,
      notes: "committed",
    },
    {
      id: 10,
      startedAt: "2026-05-15T02:00:00Z",
      completedAt: "2026-05-15T02:00:07Z",
      sessionsAnalyzed: 18,
      correctionsClassified: 5,
      examplesAdded: 2,
      examplesRemoved: 0,
      dictionaryTermsAdded: 0,
      rolledBack: true,
      notes: "regression: before=0.04 after=0.06; rolled back",
    },
  ],
};
