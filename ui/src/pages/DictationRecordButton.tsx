// Start/Stop button for the Dictations page (ADR 0045 programmatic
// dictation start). Lives next to `MeetingRecordBar.tsx` so the two
// page-bound record controls sit beside each other.
//
// Self-contained: owns its subscription to the `dictation:state`
// event so the parent doesn't need to know about the event stream.
// Outside Tauri (Vite preview / Playwright) the button still renders
// — the state stays `idle` and clicks no-op via the tauri.ts fixture
// stubs.

import { useEffect, useState } from "react";

import { Button } from "../components/primitives";
import { MicIcon } from "../design/Icon";
import { t } from "../i18n";
import { api, isTauri } from "../lib/tauri";

import styles from "./Dictations.module.css";

/** Match `RecordingWindow.tsx`'s union — kept private so each consumer
 *  doesn't accidentally drift from the orchestrator's source of truth.
 *  Promote to `lib/types.ts` if a third consumer shows up. */
type DictationState =
  | "idle"
  | "listening"
  | "transcribing"
  | "cleaning"
  | "pasting"
  | "done"
  | "aborted";

interface DictationStateEvent {
  state: DictationState;
}

/** "Active" = the orchestrator is currently doing something on behalf
 *  of a session. Used to decide between the Start and Stop affordance. */
const ACTIVE_STATES: ReadonlySet<DictationState> = new Set([
  "listening",
  "transcribing",
  "cleaning",
  "pasting",
]);

export function DictationRecordButton() {
  const [state, setState] = useState<DictationState>("idle");
  const [pending, setPending] = useState(false);

  // Subscribe to the same `dictation:state` event the recording-pill
  // overlay listens to. Whoever fires first wins; we just shadow the
  // latest state on this page. Idempotent — listen() returns a cleanup.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<DictationStateEvent>("dictation:state", (e) => {
        setState(e.payload.state);
      });
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  const active = ACTIVE_STATES.has(state);
  const label = active
    ? t("dictations.record.stop")
    : t("dictations.record.start");

  async function handleClick() {
    if (pending) return;
    setPending(true);
    try {
      if (active) {
        await api.dictation_stop();
      } else {
        await api.dictation_start();
      }
    } catch (err) {
      // The orchestrator logs the real error server-side; surface a
      // minimal hint here so the user knows the click registered but
      // something refused it. Toast wiring is the parent's job; we
      // log to the console so the dev sees it.
      // eslint-disable-next-line no-console
      console.warn("[dictations] start/stop failed", err);
    } finally {
      // Brief debounce — the next state event will arrive within ~80
      // ms (KeyDown → PendingHold → Recording threshold), at which
      // point the button re-renders with the new label. The pending
      // flag just prevents a double-click in that window.
      window.setTimeout(() => setPending(false), 200);
    }
  }

  return (
    <div className={styles.recordRow}>
      <Button
        variant={active ? "danger" : "primary"}
        onClick={handleClick}
        disabled={pending}
        ariaLabel={label}
      >
        {active ? (
          <span className={styles.recordDot} aria-hidden />
        ) : (
          <MicIcon size={16} />
        )}
        {label}
      </Button>
    </div>
  );
}
