// Meeting activation overlay.
//
// Shows when the user fires the meeting chord (Right Ctrl + M).
// Three states:
//   1. CHOOSE       — source picker + Start + Cancel.
//   2. RECORDING    — live elapsed-time chip + Stop button.
//   3. (closed)     — the window hides itself; no UI rendered.
//
// Transitions are driven by the same Tauri events the main-window
// Meetings page uses:
//   * `meeting:state`         — phase changes (started / done / …)
//   * `meeting:overlay-open`  — Rust opens the overlay (Wave 5 will
//                                wire this from the activation hook;
//                                in Wave 4 we still react if it fires).
//
// Esc cancels (hides the overlay; no IPC unless a recording is in
// flight). Click-outside is NOT bound — the overlay is always-on-top
// and non-focused, so click-outside lands in the underlying app and
// would be confusing.
//
// Outside Tauri (browser preview / Playwright) the overlay renders
// in its CHOOSE state with the default fixture probe so designers
// can see the layout without spinning up Rust.

import { useCallback, useEffect, useRef, useState } from "react";

import { MicIcon, XIcon } from "../design/Icon";
import { t } from "../i18n";
import { meetings } from "../lib/meetings";
import { isTauri } from "../lib/tauri";
import type {
  MeetingSourceKind,
  MeetingSourceProbe,
  MeetingStateEvent,
} from "../lib/types";

import styles from "./MeetingOverlay.module.css";

type OverlayMode = "choose" | "recording";

export function MeetingOverlay() {
  const [mode, setMode] = useState<OverlayMode>("choose");
  const [probe, setProbe] = useState<MeetingSourceProbe | null>(null);
  const [source, setSource] = useState<MeetingSourceKind>("mic");
  const [recordingUuid, setRecordingUuid] = useState<string | null>(null);
  const [elapsedSec, setElapsedSec] = useState(0);
  const [busy, setBusy] = useState<"starting" | "stopping" | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const recordStartRef = useRef<number | null>(null);
  // a11y: the overlay opens as a non-focused window (focus:false in
  // tauri.conf.json so it doesn't steal focus from the user's typing
  // target). The window CAN'T receive OS focus on open, but it CAN
  // route a programmatic focus() inside the webview — which is what
  // a screen reader / keyboard-only user needs to start tabbing through
  // the controls. Focus the source <select> on mount; the overlay is
  // tiny enough that the Start button is two tab-stops away.
  const sourceSelectRef = useRef<HTMLSelectElement | null>(null);

  /* -------- initial probe ----------------------------------------- */

  useEffect(() => {
    void (async () => {
      try {
        const p = await meetings.probeSources();
        setProbe(p);
        if (!p.micAvailable && p.systemAvailable) setSource("system");
      } catch {
        setProbe({ micAvailable: true, systemAvailable: false });
      }
    })();
  }, []);

  /* -------- a11y: focus source picker on CHOOSE mount ------------- */

  useEffect(() => {
    if (mode !== "choose") return;
    // Defer one tick so the ref attaches after React flushes the
    // CHOOSE-mode tree. Without this, the ref can be null on the
    // first render when transitioning from RECORDING → CHOOSE (the
    // overlay window reopens to record another meeting).
    const handle = window.setTimeout(() => {
      sourceSelectRef.current?.focus();
    }, 0);
    return () => window.clearTimeout(handle);
  }, [mode]);

  /* -------- Tauri event subscriptions ----------------------------- */

  useEffect(() => {
    if (!isTauri()) return;
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");

      unlisteners.push(
        await listen<MeetingStateEvent>("meeting:state", (e) => {
          if (cancelled) return;
          const s = e.payload.state;
          if (s === "started" && e.payload.uuid) {
            setRecordingUuid(e.payload.uuid);
            setMode("recording");
            setBusy(null);
            recordStartRef.current = Date.now();
          } else if (s === "done" || s === "error" || s === "interrupted") {
            setRecordingUuid(null);
            setBusy(null);
            recordStartRef.current = null;
            setElapsedSec(0);
            // Auto-hide after a successful finish; on error we stay
            // visible so the user can see the failure message.
            if (s === "done") {
              void hideOverlay();
            } else {
              setErrorMsg(`Recording ${s}`);
            }
          }
        }),
      );

      // Wave 5 will wire the activation hook to emit this when the
      // chord fires. Until then it's a no-op subscriber — the overlay
      // window stays hidden until something shows it.
      unlisteners.push(
        await listen("meeting:overlay-open", () => {
          if (cancelled) return;
          setMode("choose");
          setErrorMsg(null);
        }),
      );
    })();

    return () => {
      cancelled = true;
      for (const f of unlisteners) f();
    };
  }, []);

  /* -------- elapsed-time ticker ----------------------------------- */

  useEffect(() => {
    if (recordingUuid === null) return;
    const handle = window.setInterval(() => {
      const t0 = recordStartRef.current;
      if (t0 !== null) setElapsedSec(Math.floor((Date.now() - t0) / 1000));
    }, 1000);
    return () => window.clearInterval(handle);
  }, [recordingUuid]);

  /* -------- Esc-to-cancel ----------------------------------------- */

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      // If a recording is in flight we DON'T auto-stop — Stop should
      // be a deliberate click. Esc just hides the overlay; the live
      // record-bar in the main window remains the source of truth.
      void hideOverlay();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  /* -------- handlers ---------------------------------------------- */

  const handleStart = useCallback(async () => {
    if (busy) return;
    setBusy("starting");
    setErrorMsg(null);
    try {
      const { uuid } = await meetings.start(source);
      // The `meeting:state=started` event will normally fire and
      // flip us into RECORDING mode; do a defensive optimistic
      // update too in case the event misses.
      setRecordingUuid(uuid);
      recordStartRef.current = Date.now();
      setMode("recording");
    } catch (err) {
      setErrorMsg(String(err));
      setBusy(null);
    }
  }, [busy, source]);

  const handleStop = useCallback(async () => {
    if (!recordingUuid || busy) return;
    setBusy("stopping");
    try {
      await meetings.stop(recordingUuid);
      // `meeting:state=done` will fire from Rust and clean up + hide.
    } catch (err) {
      setErrorMsg(String(err));
      setBusy(null);
    }
  }, [recordingUuid, busy]);

  const handleCancel = useCallback(() => {
    void hideOverlay();
  }, []);

  /* -------- render ------------------------------------------------ */

  if (mode === "recording") {
    return (
      <div className={styles.shell}>
        <div className={styles.pill} role="status" aria-live="polite" data-tauri-drag-region>
          <span className={styles.pulseDot} aria-hidden />
          <span className={styles.label}>{t("meetingOverlay.recording")}</span>
          <span className={styles.elapsed}>{formatStopwatch(elapsedSec)}</span>
          <button
            type="button"
            className={`${styles.btn} ${styles.btnStop}`}
            onClick={handleStop}
            disabled={busy !== null}
            aria-label={t("meetingOverlay.stop")}
          >
            {busy === "stopping" ? "…" : t("meetingOverlay.stop")}
          </button>
        </div>
      </div>
    );
  }

  // CHOOSE mode.
  return (
    <div className={styles.shell}>
      <div className={styles.pill} role="dialog" aria-label={t("meetingOverlay.title")} data-tauri-drag-region>
        <MicIcon size={18} />
        <span className={styles.title}>{t("meetingOverlay.title")}</span>

        <select
          ref={sourceSelectRef}
          className={styles.select}
          value={source}
          onChange={(e) => setSource(e.target.value as MeetingSourceKind)}
          aria-label={t("meetings.source.label")}
          disabled={busy !== null}
        >
          <option value="mic" disabled={probe ? !probe.micAvailable : false}>
            {t("meetings.source.mic")}
          </option>
          <option
            value="system"
            disabled={probe ? !probe.systemAvailable : false}
          >
            {t("meetings.source.system")}
          </option>
          <option
            value="both"
            disabled={probe ? !(probe.micAvailable && probe.systemAvailable) : false}
          >
            {t("meetings.source.both")}
          </option>
        </select>

        <button
          type="button"
          className={`${styles.btn} ${styles.btnStart}`}
          onClick={handleStart}
          disabled={busy !== null}
          aria-label={t("meetingOverlay.start")}
        >
          {busy === "starting" ? "…" : t("meetingOverlay.start")}
        </button>

        <button
          type="button"
          className={styles.btnCancel}
          onClick={handleCancel}
          aria-label={t("meetingOverlay.cancel")}
        >
          <XIcon size={14} />
        </button>
      </div>

      {errorMsg ? (
        <div className={styles.errorRow} role="alert">
          {errorMsg}
        </div>
      ) : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Helpers                                                             */
/* ------------------------------------------------------------------ */

async function hideOverlay(): Promise<void> {
  if (!isTauri()) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().hide();
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[meeting-overlay] hide failed", err);
  }
}

/** `m:ss` formatter, duplicated locally to keep the overlay bundle
 *  minimal (avoids pulling in `lib/format.ts` and its `date-fns`
 *  dependency). 6 lines isn't worth the layering. */
function formatStopwatch(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}
