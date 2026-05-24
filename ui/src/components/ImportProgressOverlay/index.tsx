// Import-progress overlay (ADR 0046 Iter 4 / mb-q1xt).
//
// Surfaces the `+ Audio file` IPC + inbox-courier ingest pipeline as a
// small in-shell pill in the bottom-right corner of the main window.
// Listens to the Rust `ingest_progress` Tauri event and walks through
// decoding → transcribing → done / failed.
//
// Why an in-shell overlay and NOT the separate `recording` Tauri
// webview? The recording window is owned by `recording_window.rs`
// and tightly coupled to the dictation orchestrator's state (it
// shows/hides on `StartCapture` / `StopCapture`). Repurposing it
// from the main window would require IPCs to drive show/hide from
// the React side — much larger blast radius than this self-contained
// overlay needs. The kickoff explicitly OK'd a "quick wrap or sibling
// component" when the original RecordingWindow is too coupled.

import { useEffect, useMemo, useState } from "react";

import { isTauri } from "../../lib/tauri";
import { t } from "../../i18n";

import styles from "./styles.module.css";

/** Wire-format stage labels emitted by the Rust side. Matches
 *  `dictation::ingest_progress::stage`. */
export type ImportStage =
  | "decoding"
  | "transcribing"
  | "cleaning"
  | "done"
  | "failed";

/** Wire-format source labels. Matches `dictation::ingest_progress::source`. */
export type ImportSource = "desktop-import" | "mobile-inbox";

/** Payload mirrored from `IngestProgressEvent` on the Rust side. */
export interface IngestProgressPayload {
  stage: ImportStage;
  source: ImportSource;
  originalFilename: string;
  sessionId?: number;
  error?: string;
}

/** Internal view state for the overlay. `null` means hidden. */
interface OverlayState {
  payload: IngestProgressPayload;
  /** Bumped whenever a fresh event arrives — used to drive the
   *  auto-dismiss timer on `done` without restarting it for every
   *  unrelated re-render. */
  receivedAt: number;
}

/**
 * Reducer-style transition: given the previous state and a fresh
 * payload, return the new state.
 *
 * Pulled out as a pure function so the unit tests can exercise the
 * state machine without rendering the component or pumping Tauri
 * events through it.
 */
export function reduceProgress(
  _prev: OverlayState | null,
  payload: IngestProgressPayload,
): OverlayState {
  return { payload, receivedAt: Date.now() };
}

/** How long the `done` ✓ stays on screen before auto-dismissing. */
const DONE_DISMISS_MS = 1500;

function stageLabel(stage: ImportStage): string {
  return t(`import.stage.${stage}`);
}

function sourceLabel(source: ImportSource): string {
  return t(`import.source.${source}`);
}

export function ImportProgressOverlay() {
  const [state, setState] = useState<OverlayState | null>(null);

  // Subscribe to the `ingest_progress` event. Dynamically imported
  // so a non-Tauri preview (Vite / Playwright) doesn't blow up.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<IngestProgressPayload>(
        "ingest_progress",
        (e) => {
          setState((prev) => reduceProgress(prev, e.payload));
        },
      );
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Auto-dismiss on `done` after DONE_DISMISS_MS. Re-runs whenever
  // a fresh state arrives — if another import lands during the
  // dismiss window, the new payload's receivedAt resets the timer.
  useEffect(() => {
    if (!state) return;
    if (state.payload.stage !== "done") return;
    const timer = window.setTimeout(() => {
      // Only clear if the state we're closing OVER is still the same
      // one we scheduled the timer for. A new event would have bumped
      // receivedAt and re-run this effect with a fresh closure.
      setState((cur) =>
        cur && cur.receivedAt === state.receivedAt ? null : cur,
      );
    }, DONE_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [state]);

  // Derive the visual data. `useMemo` so we don't recompute on every
  // unrelated parent re-render.
  const view = useMemo(() => {
    if (!state) return null;
    const { payload } = state;
    const isTerminal =
      payload.stage === "done" || payload.stage === "failed";
    return {
      payload,
      isTerminal,
      label: stageLabel(payload.stage),
      source: sourceLabel(payload.source),
      stateClass: styles[`stage_${payload.stage}`] ?? "",
    };
  }, [state]);

  if (!view) return null;

  return (
    <div
      className={`${styles.shell} ${view.stateClass}`}
      role="status"
      aria-live="polite"
      data-testid="import-progress-overlay"
      data-stage={view.payload.stage}
    >
      <div className={styles.pulse} aria-hidden />
      <div className={styles.body}>
        <div className={styles.label}>{view.label}</div>
        <div className={styles.detail}>
          <span className={styles.source}>{view.source}</span>
          <span className={styles.filename} title={view.payload.originalFilename}>
            {view.payload.originalFilename}
          </span>
        </div>
        {view.payload.stage === "failed" && view.payload.error && (
          <div className={styles.error} role="alert">
            {view.payload.error}
          </div>
        )}
      </div>
      {view.isTerminal && (
        <button
          type="button"
          className={styles.dismiss}
          aria-label={t("import.dismiss")}
          onClick={() => setState(null)}
        >
          ×
        </button>
      )}
    </div>
  );
}
