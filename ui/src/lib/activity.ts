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
};
