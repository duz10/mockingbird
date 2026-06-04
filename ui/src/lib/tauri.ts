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
  DashboardSnapshot,
  EntityDetail,
  EntitySuggestion,
  EntrySummary,
  FailedFiling,
  InboxRuntimeStatus,
  InsightsSnapshot,
  KgBootstrapReport,
  KgHistoryReconcileReport,
  KgReconcileReport,
  KgSettings,
  SearchFilter,
  TagSuggestion,
  LearningRun,
  LlmPassPromptArg,
  LlmPassResult,
  MeetingSettingsSnapshot,
  ModeRow,
  QueueStatus,
  Vocabularies,
  SessionDetail,
  SessionImportSummary,
  SessionSummary,
  SettingsSnapshot,
  TagDetail,
  TranscriptSearchHit,
  VaultExportSummary,
  VaultPathCheck,
  VaultRuntimeStatus,
  VaultSettingsSnapshot,
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

/**
 * Opt-in IPC spy. When `window.__KG_IPC_SPY__` is set (by a
 * Playwright test in `addInitScript`), every `invoke()` call records
 * its command name. Used by `ui/tests/kg-graph-off-invariant.spec.ts`
 * (the Wave 1C.5 / `mb-f4gn` graph-off-UI invariant judge) to assert
 * that no `kg_*` IPC fires when `KgGraphEnabled=false`.
 *
 * Production cost: one `if (spy)` per IPC call. The hook itself is
 * a pure side-effect; it does NOT alter the value `invoke` returns.
 */
type IpcSpy = (command: string) => void;

declare global {
  interface Window {
    __MOCKINGBIRD_FIXTURES__?: FixtureMap;
    __KG_IPC_SPY__?: IpcSpy;
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
  // Opt-in spy hook (Wave 1C.5 / `mb-f4gn`). Fires before the
  // isTauri branch so both shell + preview test paths see it.
  if (typeof window !== "undefined" && window.__KG_IPC_SPY__) {
    try {
      window.__KG_IPC_SPY__(command);
    } catch {
      // Spy errors must not break the real IPC path.
    }
  }
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
  /** ADR 0047 §Wave 2.5 / mb-v2fa -- record an edit-equivalent
   *  action (today: the Dictations detail Copy button). The backend
   *  is conditional on the session being inside its 5-min injection
   *  window AND already armed at 1; legacy / never-injected /
   *  out-of-window calls are silent no-ops. Treat the returned
   *  promise as fire-and-forget at the call site -- a metric write
   *  failing must not break the copy flow. */
  dictation_mark_edit_observed: (sessionId: number) =>
    invoke<void>("dictation_mark_edit_observed", { sessionId }),

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

  // Phase 1C Wave 1C.1 — typed KG-settings IPC (ADR 0051).
  kg_settings_get_all: () => invoke<KgSettings>("kg_settings_get_all"),
  kg_settings_set: (key: string, value: unknown) =>
    invoke<void>("kg_settings_set", { key, value }),

  // Phase 1C Wave 1C.2 — failed-filings UX (ADR 0051 D1).
  /** Newest-first list of `state='failed'` rows from
   *  `kg_filing_queue`. The Rust side caps at 50 when `limit` is
   *  omitted; pass an explicit cap for tests / pagination if ever
   *  needed. Returns `[]` when the queue is clean. */
  kg_list_failed_filings: (limit?: number) =>
    invoke<FailedFiling[]>("kg_list_failed_filings", { limit }),
  /** Idempotently flip one `state='failed'` row back to `pending`
   *  (resets `attempt_count`, clears `last_error`). No-op on an
   *  already-pending row — single-click safe, no confirmation
   *  modal needed (D1 calls for optimistic UX). */
  kg_requeue_failed: (queueId: number) =>
    invoke<void>("kg_requeue_failed", { queueId }),
  /** Per-state counts + last successful filing's ISO timestamp.
   *  Cheap to compute (4 indexed SELECTs); the Settings tab calls
   *  this on mount + after each successful retry. NO polling. */
  kg_queue_status: () => invoke<QueueStatus>("kg_queue_status"),

  // Phase 1C Wave 1C.3 — Dictations retrieval (ADR 0051).
  /** Combinable retrieval search. Returns matching `entry_id`s
   *  (= `sessions.id`) by within-axis OR / across-axis AND. The UI
   *  short-circuits when the filter is fully empty and uses the
   *  existing `list_sessions` path instead (server-side
   *  `SearchFilter::is_empty()` semantics). */
  kg_search_entries: (filter: SearchFilter) =>
    invoke<number[]>("kg_search_entries", { filter }),
  /** Entity-chip autocomplete. `prefix=undefined` returns the global
   *  top entities ranked by mention count. Server caps at 50 when
   *  `limit` is omitted; we pass an explicit cap from the UI so the
   *  autocomplete list size is predictable. */
  kg_list_entities: (prefix?: string, limit?: number) =>
    invoke<EntitySuggestion[]>("kg_list_entities", { prefix, limit }),
  /** Tag-chip autocomplete — same shape as `kg_list_entities` over
   *  the open-vocab `tag_slug` axis. */
  kg_list_tags: (prefix?: string, limit?: number) =>
    invoke<TagSuggestion[]>("kg_list_tags", { prefix, limit }),
  /** Batched per-row chip + filing-state lookup. Single round-trip
   *  for the Dictations list page's KG strip. Per-row firing is a
   *  stop condition (kickoff brief) — always call once with the full
   *  list of visible session ids. Returns a record keyed by
   *  `entry_id` (JSON object keys are strings, hence the indexed
   *  shape; callers stringify the lookup key). */
  kg_entries_summary: (entryIds: number[]) =>
    invoke<Record<string, EntrySummary>>("kg_entries_summary", { entryIds }),

  // Phase 1C Wave 1C.4 — concept modal drill-down (ADR 0051 D4).
  /** Resolve one entity to its modal payload: header (canonical
   *  name + type + aliases), counters, and the most-recent N
   *  entries. `recentLimit` defaults to 50 server-side; omit unless
   *  a test wants to pin a smaller cap.
   *
   *  An unknown `entityId` is an **error** (Rust returns
   *  `"entity not found: <id>"`) — the modal surfaces it as a
   *  sonner toast and stays open in its loading state. */
  kg_entity_detail: (entityId: number, recentLimit?: number) =>
    invoke<EntityDetail>("kg_entity_detail", { entityId, recentLimit }),
  /** Resolve one tag slug to its modal payload. Keyed by
   *  `tagSlug: string` (not a numeric id) per the deviation in
   *  `commands/kg.rs::kg_tag_detail` — the canonical-vocab table
   *  is inert in 1B (LESSONS P11) so the slug IS the wire id.
   *
   *  An unknown slug is **not** an error — it yields zero counts +
   *  empty `recentEntries`. Opening the modal for a freshly-typed
   *  filter-bar slug before any dictation has been filed against it
   *  is a legitimate state. */
  kg_tag_detail: (tagSlug: string, recentLimit?: number) =>
    invoke<TagDetail>("kg_tag_detail", { tagSlug, recentLimit }),

  // Phase 1D Wave 1D.2 -- KG dashboard payload (ADR 0052 D2).
  /** Composite snapshot for the `/knowledge-graph` dashboard. One
   *  round-trip: counts + queue status + recent activity + flagged
   *  for review + upcoming due. **Graph-off contract:** when
   *  `KgGraphEnabled=false` the Rust side returns an all-zeros /
   *  all-empty shape WITHOUT reading the DB; the route guard
   *  short-circuits before calling here, so this is belt-and-braces
   *  for the bookmark-while-toggle-off case. */
  kg_dashboard_snapshot: () =>
    invoke<DashboardSnapshot>("kg_dashboard_snapshot"),

  /** Phase 1D Wave 1D.5 (`mb-navi`, ADR 0052) -- v1 controlled
   *  vocabularies (Layer 1 categories + Layer 2 entry types) for
   *  the Settings -> KG tab's read-only display. Static metadata
   *  derived from `kg::schema::{Category, EntryType}`; safe to call
   *  with `kgGraphEnabled=false` (no DB read, no graph state
   *  touched -- explicitly allowlisted by the graph-off invariant
   *  judge for this reason). */
  kg_vocabularies_get: () => invoke<Vocabularies>("kg_vocabularies_get"),
  /** Phase 1D Wave 1D.5 (`mb-navi`, ADR 0052) -- launch the
   *  configured Obsidian vault via the `obsidian://` URI scheme.
   *  Reads `VaultPath` from the Mobile Sync settings (single
   *  source of truth per ADR 0046). Errors when:
   *  - vault path is unconfigured (UI should pre-check + disable
   *    the button; this IPC validates as belt-and-braces);
   *  - Obsidian isn't installed / URL handler unregistered;
   *  - platform is not Windows (macOS/Linux are stubs until
   *    Phase 9). */
  kg_launch_obsidian: () => invoke<void>("kg_launch_obsidian"),

  /** Phase 1E Wave 1E.1 (`mb-e16d`, ADR 0053 D1) -- idempotently
   *  create the `<vault>/Knowledge Graph/{Inbox,Entries,History}/`
   *  subtree under the configured `VaultPath`. Fires on toggle-on
   *  from the Settings -> KG tab; also fired internally at app
   *  boot when both `KgGraphEnabled` and `VaultPath` are set (the
   *  boot path is Rust-side and does NOT go through this IPC).
   *
   *  Returns:
   *  - `"created"` if at least one subtree directory was missing
   *    and is now present;
   *  - `"alreadyExists"` if all four directories were already on
   *    disk (idempotent no-op).
   *
   *  Errors when the vault path is unset / empty / unwritable; the
   *  Settings KG tab pre-checks `vault_settings_get()` and disables
   *  the toggle-on path when unconfigured, so this is a belt-and-
   *  braces guard. */
  kg_subtree_bootstrap: () =>
    invoke<KgBootstrapReport>("kg_subtree_bootstrap"),

  /** Phase 1E hotfix to Wave 1E.3 (closes `mb-43xw`) -- on-demand
   *  reconcile of `<vault>/Knowledge Graph/Entries/` against the
   *  `sessions` table. Returns a [`KgReconcileReport`] with three
   *  counters: `missingFileCount`, `sealedCount`, `orphanFilesCount`.
   *  Read-only in v1 -- the report tells the operator what drift
   *  was detected; actual repair (file rewrite, row sealing,
   *  orphan cleanup) lands in later waves.
   *
   *  Errors when the KG toggle is off OR the vault path is unset /
   *  empty. The UI pre-checks both and disables the button + shows
   *  a tooltip; this IPC is the belt-and-braces guard. */
  kg_reconcile_vault: () =>
    invoke<KgReconcileReport>("kg_reconcile_vault"),

  /** Phase 1E hotfix sibling -- on-demand reconcile of the
   *  `History/` JSON sidecar archive. Symmetric to
   *  [`kg_reconcile_vault`]; same gating, same read-only posture. */
  kg_reconcile_history: () =>
    invoke<KgHistoryReconcileReport>("kg_reconcile_history"),

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

  /**
   * mb-1z0m (Round 3) -- fire-and-forget IPC-outcome mirror so JS-side
   * `console.warn` failures also land in `mockingbird.log`. Takes no
   * managed state on the Rust side, so it dispatches even when the
   * AppState slot is the broken thing we're diagnosing.
   *
   * Callers should NOT await this and should swallow its own errors --
   * losing the diagnostic line is strictly less bad than crashing
   * boot on a meta-diagnostic failure.
   */
  report_ipc_status: (label: string, ok: boolean, reason?: string) =>
    invoke<void>("report_ipc_status", { label, ok, reason: reason ?? null }),

  /** ADR 0045 — programmatic dictation start. Equivalent to pressing
   *  Right Alt (push-to-talk); the orchestrator runs the same code
   *  path. Returns once the synthetic event has been queued; the
   *  recording is observed via the `dictation:state` event stream
   *  (state flips to `"listening"` ~80 ms later). */
  dictation_start: () => invoke<void>("dictation_start"),
  /** ADR 0045 — programmatic dictation stop. Equivalent to releasing
   *  the PTT key (finalize, NOT cancel — Esc on the recording-pill
   *  overlay is still the cancel path). Idempotent: a no-op if the
   *  state machine isn't currently in a programmatic recording. */
  dictation_stop: () => invoke<void>("dictation_stop"),
  /** Phase 1D Wave 1D.3 (`mb-0gt6`, ADR 0052) -- start a KG audio
   *  note. Same orchestrator path as `dictation_start` but the
   *  resulting session is tagged `capture_kind='kg-note'`, so it
   *  fires KG filing AND appears in Dictations history (the
   *  intentional "audio-note" dual-write).
   *
   *  Stop is the same `dictation_stop` IPC -- only the start side
   *  carries the capture-kind tag; the orchestrator threads it
   *  through to the persist step. */
  dictation_start_kg_note: () => invoke<void>("dictation_start_kg_note"),
  /** Phase 1D Wave 1D.3 (`mb-0gt6`, ADR 0052) -- file a KG text
   *  note. Bypasses Whisper entirely: the text is the transcript.
   *  Inserts a session with `capture_kind='kg-note-text'` (which
   *  the history-list filter excludes), a raw+cleaned transcript
   *  row (equal contents), and enqueues for KG filing.
   *
   *  Returns the inserted `sessions.id` so the UI can correlate
   *  the subsequent dashboard refetch / filing-state pill without
   *  a follow-up lookup. */
  kg_ingest_text_note: (text: string) =>
    invoke<number>("kg_ingest_text_note", { text }),
  /** ADR 0046 §3.2 / mb-7vyz — desktop audio-file import.
   *  Opens a native file picker, decodes the selected file via
   *  symphonia, queues it onto the orchestrator's sibling headless-
   *  ingest channel, and resolves with the new session id +
   *  transcript preview when the pipeline completes.
   *
   *  Rejects with `"cancelled"` (literal string) if the user
   *  dismisses the picker — callers should suppress the error toast
   *  in that case. All other rejections carry a descriptive message
   *  suitable for showing verbatim. */
  dictation_import_file: () =>
    invoke<SessionImportSummary>("dictation_import_file"),

  // ADR 0046 Iter 2 / mb-vg3p — Mobile Sync (Obsidian Vault) preview.
  /** Read the four mobile-sync settings keys (`mobile_sync_enabled`,
   *  `vault_path`, `vault_sync_record_types`, `vault_retention_days`). */
  vault_settings_get: () =>
    invoke<VaultSettingsSnapshot>("vault_settings_get"),
  /** Write one of the four mobile-sync settings keys. The IPC also
   *  refreshes the runtime config + fires a backfill trigger, so
   *  flipping the toggle on with a valid path immediately populates
   *  the vault. */
  vault_settings_set: (key: string, value: unknown) =>
    invoke<void>("vault_settings_set", { key, value }),
  /** Manual reconciliation pass. Synchronous (blocks on the worker
   *  pool); the returned summary feeds the Export-now toast. */
  vault_export_now: () =>
    invoke<VaultExportSummary>("vault_export_now"),
  /** Native directory picker for the vault path. Returns `null` on
   *  cancel and the absolute path string on confirm. Exposed as a
   *  Rust IPC so the UI doesn't need `@tauri-apps/plugin-dialog`. */
  vault_pick_directory: () =>
    invoke<string | null>("vault_pick_directory"),

  // ADR 0046 Iter 4 / mb-vg3p — Mobile Sync settings-tab health card.
  /** Snapshot of the outbound projection runtime: running flag,
   *  manifest age, and last filesystem error if any. Polled every
   *  5 s while the Mobile Sync tab is visible. */
  vault_runtime_status: () =>
    invoke<VaultRuntimeStatus>("vault_runtime_status"),
  /** Snapshot of the inbox courier runtime: running flag, watch
   *  path, newest archived file timestamp, and the count of files
   *  currently sitting in `_failed/`. */
  inbox_runtime_status: () =>
    invoke<InboxRuntimeStatus>("inbox_runtime_status"),

  // ADR 0046 Iter 4 / mb-3xww — nested-vault detection wizard.
  /** Pre-flight a candidate vault path. Returns `{kind: "ok"}` when
   *  safe to use, or `{kind: "nestedVault", parentVault, suggestedSibling}`
   *  when the user picked a folder INSIDE an existing Obsidian vault.
   *  Pure inspection — does NOT mutate settings. */
  vault_check_path: (path: string) =>
    invoke<VaultPathCheck>("vault_check_path", { path }),
  /** Create a directory (and any missing parents) at `path`. Used by
   *  the "Use a sibling location instead" branch of the nested-vault
   *  wizard — the suggested sibling won't exist on disk yet. */
  vault_ensure_dir: (path: string) =>
    invoke<void>("vault_ensure_dir", { path }),
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
    case "vault_settings_get":
      return fixture(command, {
        mobileSyncEnabled: false,
        vaultPath: null,
        vaultSyncRecordTypes: "both",
        vaultRetentionDays: 30,
      } as VaultSettingsSnapshot) as T;
    case "vault_export_now":
      return fixture(command, {
        total: 4,
        changes: 2,
        archived: 0,
        skipped: false,
      } as VaultExportSummary) as T;
    case "vault_pick_directory":
      // Browser-preview stub: pretend the user picked a path.
      return fixture(command, "C:\\Users\\you\\mockingbird-vault") as T;
    case "vault_check_path":
      // Browser preview defaults to the happy path. Playwright
      // specs that want to exercise the nested-vault wizard set a
      // command fixture override explicitly.
      return fixture(command, { kind: "ok" } as VaultPathCheck) as T;
    case "vault_ensure_dir":
      return fixture(command, undefined) as T;
    case "kg_settings_get_all":
      return fixture(command, { kgGraphEnabled: false } as KgSettings) as T;
    case "kg_list_failed_filings":
      // Phase 1C Wave 1C.2 — default fixture is an empty list so
      // the "all good!" empty state renders in browser preview.
      // Playwright specs that want to see failed rows override via
      // `window.__MOCKINGBIRD_FIXTURES__`.
      return fixture(command, [] as FailedFiling[]) as T;
    case "kg_queue_status":
      // Matches the clean-queue fixture above: no work in flight,
      // no successful filings yet (consistent with kgGraphEnabled=
      // false default).
      return fixture(command, {
        pending: 0,
        processing: 0,
        failed: 0,
        done: 0,
        lastDoneIso: null,
      } as QueueStatus) as T;
    // Phase 1D Wave 1D.2 -- KG dashboard fixture. Default is the
    // empty-everything shape so the dashboard renders its empty
    // states out-of-the-box in browser preview (matches the
    // KgGraphEnabled=false default world). Playwright specs that
    // need populated bands override via __MOCKINGBIRD_FIXTURES__.
    case "kg_dashboard_snapshot":
      return fixture(command, {
        counts: {
          totalEntities: 0,
          entitiesByType: [],
          totalEntries: 0,
        },
        queueStatus: {
          pending: 0,
          processing: 0,
          failed: 0,
          done: 0,
          lastDoneIso: null,
        },
        recentActivity: [],
        flaggedForReview: [],
        upcomingDue: [],
      } as DashboardSnapshot) as T;
    // Phase 1D Wave 1D.5 (mb-navi, ADR 0052) -- vocabularies +
    // launch fixtures. Vocabularies returns the v1 taxonomy
    // verbatim (matches the Rust enum variants pinned by
    // `vocabularies_matches_schema_enums`). Launch is a void IPC;
    // the browser-preview fixture is a successful no-op so the
    // Settings tab can be exercised without a real Tauri shell.
    case "kg_vocabularies_get":
      return fixture(command, {
        categories: ["personal", "professional", "objective"],
        entryTypes: ["task", "research", "idea", "note", "reference"],
      } as Vocabularies) as T;
    case "kg_launch_obsidian":
      return fixture(command, undefined) as T;
    case "kg_subtree_bootstrap":
      // Phase 1E Wave 1E.1 (mb-e16d, ADR 0053 D1) -- browser-preview
      // default. "alreadyExists" is the steady-state shape (most
      // toggle-on flips after first activation hit the no-op path),
      // so design-mode renders see the no-op toast variant by
      // default. Playwright specs that want the freshly-created
      // variant override via __MOCKINGBIRD_FIXTURES__.
      return fixture(command, "alreadyExists" as KgBootstrapReport) as T;
    // Phase 1E hotfix -- reconcile fixtures. Both return the
    // zero-drift report by default (the common case once a vault
    // is fully synced). Playwright + vitest specs override via
    // `__MOCKINGBIRD_FIXTURES__` to exercise the
    // "missing/sealed/orphan" copy paths.
    case "kg_reconcile_vault":
      return fixture(command, {
        missingFileCount: 0,
        sealedCount: 0,
        orphanFilesCount: 0,
      } as KgReconcileReport) as T;
    case "kg_reconcile_history":
      return fixture(command, {
        missingSidecarCount: 0,
        orphanSidecarCount: 0,
      } as KgHistoryReconcileReport) as T;
    // Phase 1C Wave 1C.3 — Dictations retrieval fixtures.
    // Defaults match the kgGraphEnabled=false world: empty
    // autocomplete lists, empty entry-id sets, empty summary map.
    // Playwright + Wave 1C.3 vitest specs override via
    // `window.__MOCKINGBIRD_FIXTURES__`.
    case "kg_search_entries":
      return fixture(command, [] as number[]) as T;
    case "kg_list_entities":
      return fixture(command, [] as EntitySuggestion[]) as T;
    case "kg_list_tags":
      return fixture(command, [] as TagSuggestion[]) as T;
    case "kg_entries_summary":
      return fixture(command, {} as Record<string, EntrySummary>) as T;
    // Phase 1C Wave 1C.4 — concept modal fixtures. Defaults match
    // "no data yet" / open-vocab-slug-with-no-mentions states.
    // Playwright + vitest specs override via
    // `window.__MOCKINGBIRD_FIXTURES__` for non-empty cases.
    case "kg_entity_detail":
      return fixture(command, {
        entityId: 0,
        canonicalName: "",
        entityType: "object",
        aliases: [],
        mentionCount: 0,
        totalEntries: 0,
        recentEntries: [],
      } as EntityDetail) as T;
    case "kg_tag_detail":
      return fixture(command, {
        tagSlug: "",
        mentionCount: 0,
        totalEntries: 0,
        recentEntries: [],
      } as TagDetail) as T;
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
    case "report_ipc_status":
      // mb-1z0m (Round 3) -- fire-and-forget; nothing to fixture.
      return fixture(command, null) as T;
    case "dictation_run_llm_pass":
      return fixture(command, {
        id: "fixture-dictation-llm-pass-id",
        text:
          "**Summary** — this is a fixture LLM-pass output for the Dictations tab. Browser-preview only.",
        latencyMs: 1240,
      }) as T;
    case "dictation_mark_edit_observed":
      // Fire-and-forget metric write -- browser preview returns null.
      return fixture(command, null) as T;
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
    case "kg_settings_set":
    case "kg_requeue_failed":
    case "vault_settings_set":
    case "report_correction":
    case "trigger_learning_run":
    case "open_path":
    case "dictation_start":
    case "dictation_start_kg_note":
    case "dictation_stop":
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
    case "kg_ingest_text_note":
      // Phase 1D Wave 1D.3 (`mb-0gt6`) -- browser-preview stub.
      // Returns a synthetic session id so the design-mode "submit"
      // round-trips cleanly. Playwright specs can override via
      // `window.__MOCKINGBIRD_FIXTURES__`.
      return fixture(command, 99_999 as unknown as T);
    case "dictation_import_file":
      // ADR 0046 §3.2 / mb-7vyz — browser-preview stub. Real
      // shell-side response shape is `SessionImportSummary`; here
      // we return a deterministic synthetic so design work on the
      // Dictations toast keeps rendering.
      return fixture(command, {
        sessionId: 9999,
        source: "desktop-import",
        transcriptPreview: "(preview) Imported audio file — Lorem ipsum dolor sit amet...",
      } as SessionImportSummary) as T;
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
    editFreeSend: {
      lifetime: { injected: 179, editFree: 156, percentage: 156 / 179 },
      last30d: { injected: 42, editFree: 38, percentage: 38 / 42 },
    },
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
    // ADR 0045 + mb-tfyp — every 5th fixture row simulates an
    // in-app session so the IN_APP pill renders in Vite preview
    // / Playwright snapshots.
    startMode: i % 5 === 4 ? "in_app" : "ptt",
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
        startMode: "ptt",
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
