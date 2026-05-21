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
  MeetingTickEvent,
} from "../lib/types";

/** Sentinel returned by the Rust emitter when a channel has produced
 *  no data yet (vs. "silence"). Mirrors `DBFS_NO_DATA` in
 *  `src-tauri/src/meetings/levels.rs`. */
const DBFS_NO_DATA = 0;
/** Bottom of the dBFS scale we render. Mirrors `DBFS_FLOOR` in
 *  `levels.rs`. -100 dBFS renders as a fully-dim bar. */
const DBFS_FLOOR = -100;

/** Map a dBFS reading in `[-100, 0]` to a `0..1` fill ratio for the
 *  VU bar. The no-data sentinel collapses to `0` (flat bar). */
export function dbfsToFill(db: number): number {
  if (db === DBFS_NO_DATA) return 0;
  if (db <= DBFS_FLOOR) return 0;
  if (db >= 0) return 1;
  // Linear in dB. Future iteration could switch to log-perceptual
  // mapping but the eye reads linear dB just fine for a 100-pixel bar.
  return (db - DBFS_FLOOR) / -DBFS_FLOOR;
}

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
  // ADR 0032 / mb-nig — live per-channel dBFS from `meeting:tick`.
  // Initialised to the no-data sentinel so first render shows flat
  // bars instead of "silence" (which would be misleading before any
  // drain has happened).
  const [micDb, setMicDb] = useState<number>(DBFS_NO_DATA);
  const [sysDb, setSysDb] = useState<number>(DBFS_NO_DATA);
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

      // ADR 0032 / mb-nig: live audio levels for the VU bars.
      unlisteners.push(
        await listen<MeetingTickEvent>("meeting:tick", (e) => {
          if (cancelled) return;
          setMicDb(e.payload.micDb);
          setSysDb(e.payload.sysDb);
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
          {/* ADR 0032 / mb-nig: live VU bars. Hidden from the a11y
              tree (purely decorative — the screen reader gets the
              elapsed time + state from the role=status region). */}
          <VuBars micDb={micDb} sysDb={sysDb} />
          <button
            type="button"
            className={`${styles.btn} ${styles.btnStop}`}
            onClick={handleStop}
            disabled={busy !== null}
            aria-label={t("meetingOverlay.stop")}
          >
            {busy === "stopping" ? "…" : t("meetingOverlay.stop")}
          </button>
          {/* mb-fc1 hotfix: dismiss the pill WITHOUT stopping the
              recording. The chord re-summons it, the Meetings page
              also shows it via the started-event listener. Distinct
              icon (× sized 14px) so the user can't confuse it with
              the Stop button (red, labelled). */}
          <button
            type="button"
            className={styles.btnCancel}
            onClick={handleCancel}
            aria-label={t("meetingOverlay.dismiss")}
            title={t("meetingOverlay.dismiss")}
          >
            <XIcon size={14} />
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

/** Twin VU bars (mic + sys). Pure presentational; no IPC. Rendered
 *  inside the recording-mode pill. ADR 0032 / mb-nig. */
function VuBars({ micDb, sysDb }: { micDb: number; sysDb: number }) {
  const micFill = dbfsToFill(micDb);
  const sysFill = dbfsToFill(sysDb);
  return (
    <span className={styles.vuBars} aria-hidden="true">
      <span className={styles.vuBar} title={`mic ${formatDb(micDb)}`}>
        <span
          className={styles.vuFill}
          style={{ transform: `scaleX(${micFill})` }}
        />
      </span>
      <span className={styles.vuBar} title={`sys ${formatDb(sysDb)}`}>
        <span
          className={styles.vuFill}
          style={{ transform: `scaleX(${sysFill})` }}
        />
      </span>
    </span>
  );
}

function formatDb(db: number): string {
  if (db === DBFS_NO_DATA) return "—";
  return `${db.toFixed(0)} dB`;
}
