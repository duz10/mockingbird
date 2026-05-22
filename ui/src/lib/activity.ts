// Activity-capture IPC + types (Phase 10 Wave 1B, ADR 0036).
//
// Mirrors `src-tauri/src/activity/persist.rs` and
// `src-tauri/src/commands/activity.rs`. Anything that changes here
// must change in lockstep on the Rust side.

import { invoke } from "./tauri";

/** Persisted session status. Matches `SessionStatus::as_db_str`. */
export type ActivitySessionStatus =
  | "in_progress"
  | "completed"
  | "partial"
  | "crashed_recovered";

/** Lifecycle snapshot returned by `activity_runtime_snapshot`. */
export type ActivityLifecycle = "idle" | "active" | "paused" | "stopped";

export interface ActivitySessionRow {
  id: string;
  startedAt: number;
  endedAt: number | null;
  status: ActivitySessionStatus;
  audioEnabled: boolean;
  screenshotEnabled: boolean;
  label: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface ActivityEventRow {
  id: string;
  sessionId: string;
  ts: number;
  kind: string;
  appName: string | null;
  windowTitle: string | null;
  /** Free-form JSON string. Wave 1B emits null or
   *  `{"app":..., "title":...}`; Wave 2 emits a v2-schema payload with
   *  monitor + UIA fields (see {@link UiaSnapshotV2}). */
  snapshotJson: string | null;
  createdAt: number;
}

/**
 * Wave 2 `context_snapshot` payload shape (matches
 * `src-tauri/src/activity/uia/payload.rs::ProbeResult` after
 * `serde(rename_all = "camelCase")`). The `schema` discriminator
 * is checked by {@link parseSnapshotJson}; older rows missing the
 * discriminator return `null` from the parser and the UI falls
 * back to the flat app/title rendering.
 */
export interface UiaSnapshotV2 {
  schema: "v2";
  app: string;
  title: string;
  monitor: MonitorInfo | null;
  focusedField: FocusedField | null;
  visibleTextFragments: string[];
  controlSummary: ControlSummary;
  passwordFieldActive: boolean;
  status: ProbeStatus;
}

export interface MonitorInfo {
  name: string;
  isPrimary: boolean;
  bounds: { left: number; top: number; right: number; bottom: number };
  dpiScale: number | null;
}

export interface FocusedField {
  name: string;
  controlType: string;
  value: string;
}

export interface ControlSummary {
  editCount: number;
  buttonCount: number;
  documentCount: number;
  linkCount: number;
  textCount: number;
  otherCount: number;
  elementsVisited: number;
}

export type ProbeStatus =
  | { kind: "ok" }
  | { kind: "no_payload" }
  | { kind: "failed"; reason: string };

/**
 * Safe-parse the `snapshot_json` column. Returns the typed v2 payload
 * when the schema discriminator matches, otherwise null (caller
 * renders the flat fallback). Never throws.
 */
export function parseSnapshotJson(
  raw: string | null,
): UiaSnapshotV2 | null {
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as unknown;
    if (
      typeof v === "object" &&
      v !== null &&
      (v as { schema?: unknown }).schema === "v2"
    ) {
      // SAFETY: we have asserted the schema discriminator. Mockingbird
      // is the only writer; we trust the rest of the shape downstream.
      return v as UiaSnapshotV2;
    }
  } catch {
    // fall through
  }
  return null;
}

export interface ActivitySessionDetail {
  session: ActivitySessionRow;
  events: ActivityEventRow[];
}

/**
 * One persisted Block row — the unit the Wave-3 abstractor pipeline
 * produces. Mirrors `activity::blocks_persist::ActivityBlockRow`.
 */
export interface ActivityBlockRow {
  id: string;
  sessionId: string;
  startedAt: number;
  endedAt: number;
  primaryApp: string;
  /** User-set label. Null until the user renames via `block.rename`. */
  label: string | null;
  primaryTitle: string;
  /** LLM-or-template summary. Null only if the abstractor emitted nothing. */
  generatedAbstract: string | null;
  /** True iff the user has touched label or generatedAbstract. */
  userEdited: boolean;
  /** JSON array of source `activity_events.id` rows (Principle 2). */
  sourceEventIds: string;
  /** Prompt-set fingerprint at write time. */
  promptVersionSha: string;
  createdAt: number;
  updatedAt: number;
}

export interface ActivityRuntimeSnapshot {
  lifecycle: ActivityLifecycle;
  currentSessionId: string | null;
}

/**
 * Thin IPC layer. Every wrapper goes through `invoke`, which the
 * shared `tauri.ts` shim routes either to the real Rust handler or
 * to its fixture map for browser-preview / unit tests.
 */
export const activityApi = {
  start: (): Promise<string | null> =>
    invoke<string | null>("activity_start"),
  pause: (): Promise<void> => invoke("activity_pause"),
  resume: (): Promise<void> => invoke("activity_resume"),
  stop: (): Promise<void> => invoke("activity_stop"),
  runtimeSnapshot: (): Promise<ActivityRuntimeSnapshot> =>
    invoke<ActivityRuntimeSnapshot>("activity_runtime_snapshot"),
  listSessions: (limit: number): Promise<ActivitySessionRow[]> =>
    invoke<ActivitySessionRow[]>("activity_list_sessions", { limit }),
  getSessionDetail: (
    sessionId: string,
  ): Promise<ActivitySessionDetail | null> =>
    invoke<ActivitySessionDetail | null>("activity_get_session_detail", {
      sessionId,
    }),
  deleteSession: (sessionId: string): Promise<void> =>
    invoke("activity_delete_session", { sessionId }),

  // -----------------------------------------------------------------
  // Wave 3 — LLM block summarization + Block CRUD + export (ADR 0040).
  // -----------------------------------------------------------------

  /**
   * Run the full Wave-3 pipeline against a session: normalize events,
   * group into Blocks, abstract each, assemble Markdown, write to
   * `summary_markdown`. Returns the resulting Markdown.
   * User-edited Block rows are preserved across re-runs.
   */
  regenerateSummary: (sessionId: string): Promise<string> =>
    invoke<string>("activity_regenerate_summary", { sessionId }),

  /** List the Blocks for one session, chronologically. */
  listBlocks: (sessionId: string): Promise<ActivityBlockRow[]> =>
    invoke<ActivityBlockRow[]>("activity_list_blocks", { sessionId }),

  /** Render the work-report Markdown variant on demand. */
  renderWorkReport: (sessionId: string): Promise<string> =>
    invoke<string>("activity_render_work_report", { sessionId }),

  /** Write the stored `summary_markdown` to a destination path. */
  exportMarkdown: (sessionId: string, destPath: string): Promise<void> =>
    invoke("activity_export_markdown", { sessionId, destPath }),

  /** Copy the stored `summary_markdown` to the system clipboard. */
  copyToClipboard: (sessionId: string): Promise<void> =>
    invoke("activity_copy_to_clipboard", { sessionId }),

  block: {
    /** Set a Block's user-facing label. Pass null to clear. */
    rename: (blockId: string, newLabel: string | null): Promise<void> =>
      invoke("activity_block_rename", { blockId, newLabel }),
    /** Overwrite a Block's generated_abstract with user text. */
    rewriteAbstract: (blockId: string, text: string): Promise<void> =>
      invoke("activity_block_rewrite_abstract", { blockId, text }),
    /** Delete one Block. */
    delete: (blockId: string): Promise<void> =>
      invoke("activity_block_delete", { blockId }),
    /** Merge `sourceIds` into `targetId`. */
    merge: (targetId: string, sourceIds: string[]): Promise<void> =>
      invoke("activity_block_merge", { targetId, sourceIds }),
    /** Split a Block at `splitAtMs`. Returns the new (right-half) Block id. */
    split: (blockId: string, splitAtMs: number): Promise<string> =>
      invoke<string>("activity_block_split", { blockId, splitAtMs }),
  },

  // -----------------------------------------------------------------
  // Wave 5 — Hardening (ADR 0042 retention, 0043 exclusion, 0044 PDF).
  // -----------------------------------------------------------------

  exclusion: {
    /** List ALL rules (built-ins + user-created, enabled or not). */
    list: (): Promise<ExclusionRule[]> =>
      invoke<ExclusionRule[]>("activity_exclusion_list"),
    /** Validate a `(kind, pattern)` without persisting. Returns void on
     *  success; rejects with a string error on invalid input. */
    validate: (kind: ExclusionRuleKind, pattern: string): Promise<void> =>
      invoke("activity_exclusion_validate", { kind, pattern }),
    /** Upsert a user-created rule. `id = null` → INSERT. Built-in rule
     *  shapes are immutable; use {@link setEnabled} for built-ins. */
    upsert: (
      id: string | null,
      kind: ExclusionRuleKind,
      pattern: string,
      enabled: boolean,
      note: string | null,
    ): Promise<string> =>
      invoke<string>("activity_exclusion_upsert", {
        id,
        kind,
        pattern,
        enabled,
        note,
      }),
    /** Toggle the `enabled` flag on any rule (built-in or user). */
    setEnabled: (id: string, enabled: boolean): Promise<void> =>
      invoke("activity_exclusion_set_enabled", { id, enabled }),
    /** Delete a user-created rule. Built-ins reject with an error. */
    delete: (id: string): Promise<void> =>
      invoke("activity_exclusion_delete", { id }),
  },

  retention: {
    /** Read current TTLs + last-sweep timestamp. */
    get: (): Promise<RetentionPolicy> =>
      invoke<RetentionPolicy>("activity_retention_get"),
    /** Persist the three TTL knobs. `0` = forever. */
    set: (
      eventsDays: number,
      segmentsDays: number,
      blocksDays: number,
    ): Promise<void> =>
      invoke("activity_retention_set", {
        eventsDays,
        segmentsDays,
        blocksDays,
      }),
    /** Trigger the sweep immediately and return row-count summary. */
    sweepNow: (): Promise<RetentionSweepResult> =>
      invoke<RetentionSweepResult>("activity_retention_sweep_now"),
  },

  /** Render the per-session PDF to `destPath`. `mode` selects layout. */
  exportPdf: (
    sessionId: string,
    destPath: string,
    mode: ActivityPdfMode,
  ): Promise<void> =>
    invoke("activity_export_pdf", { sessionId, destPath, mode }),
};

// -------------------------------------------------------------------
// Wave 5 — Hardening DTOs.
// -------------------------------------------------------------------

/** Exclusion-rule kinds (ADR 0043 §Rule kinds). */
export type ExclusionRuleKind = "app_glob" | "title_regex" | "system";

export interface ExclusionRule {
  id: string;
  kind: ExclusionRuleKind;
  pattern: string;
  enabled: boolean;
  isBuiltin: boolean;
  note: string | null;
  createdAt: number;
  updatedAt: number;
}

/** Retention policy (ADR 0042). All `*Days` fields: `0` = forever. */
export interface RetentionPolicy {
  eventsDays: number;
  segmentsDays: number;
  blocksDays: number;
  /** Unix epoch ms of the last successful sweep. `0` = never. */
  lastSweepMs: number;
}

export interface RetentionSweepResult {
  eventsDeleted: number;
  segmentsDeleted: number;
  blocksDeleted: number;
  blocksMarkedPurged: number;
  ranAtMs: number;
}

export type ActivityPdfMode = "full" | "work_report";
