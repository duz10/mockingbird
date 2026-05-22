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
   *  `{"app":..., "title":...}`; Wave 2 expands. */
  snapshotJson: string | null;
  createdAt: number;
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
