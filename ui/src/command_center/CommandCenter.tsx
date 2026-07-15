// Command Center overlay React component (Phase 10 Wave 1A, ADR 0037).
//
// Three rendering shapes per the spec:
//
//   - Welcome variant (first_run + modePicker): same picker but with
//     a "Welcome to Mockingbird" header band above the tiles.
//   - Mode picker: three tiles — Dictation, Meeting, Activity. The
//     Activity tile is rendered disabled until Wave 1B lands its
//     runtime (the state-machine wires the pick path; the runtime
//     just isn't there yet — clicks would dispatch and fail).
//   - SessionCard: live recording — kind label + elapsed time +
//     Stop button.
//
// Esc + outside-click dismiss. Chord re-press triggers Rust-side
// open (no-op when already open; the FSM debounces).
//
// This component owns no recording logic. It's a thin reactive view
// over the Rust orchestrator's state.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  COMMAND_CENTER_STATE_EVENT,
  dismissCommandCenter,
  getCommandCenterState,
  pickCommandCenterMode,
  stopActiveCommandCenterSession,
} from "../lib/command_center";
import { api } from "../lib/tauri";
import type {
  CcStateSnapshot,
  RecordingKind,
} from "../lib/command_center";

import styles from "./CommandCenter.module.css";

/**
 * The three modes the user can pick. As of Wave 1B (ADR 0036) all
 * three are wired to real runtimes; Activity launches the titles-
 * only sampler (richer UIA capture lands in Wave 2).
 */
const MODES: ReadonlyArray<{
  kind: RecordingKind;
  title: string;
  hint: string;
  disabled?: boolean;
}> = [
  {
    kind: "dictation",
    title: "Dictation",
    hint: "or just hold Right Alt",
  },
  {
    kind: "meeting",
    title: "Meeting",
    hint: "transcribe a call or conversation",
  },
  {
    kind: "activity",
    title: "Activity",
    hint: "log what you worked on",
  },
];

export function CommandCenter(): JSX.Element | null {
  const [snap, setSnap] = useState<CcStateSnapshot>({
    state: "closed",
    firstRun: false,
  });
  // Tick the elapsed-time display on the SessionCard. We don't need
  // absolute accuracy here — it's a soft UI counter, not an audit
  // log. The actual session timestamps come from the recording
  // subsystem.
  const sessionStartedAt = useRef<number | null>(null);
  const [elapsedMs, setElapsedMs] = useState(0);

  // macOS-port (v1 honest-surface) — Activity capture is Windows-only
  // (macOS ships a no-op StubSampler), so the Activity tile is disabled
  // on macOS to stop a user starting a session that would capture
  // nothing. The Command Center runs in its own webview, so it can't
  // read the main window's app store; resolve the host OS locally.
  // Defaults to `false` (Windows-parity) until the probe resolves and
  // on any failure, so a failed probe never disables a Windows tile.
  const [isMac, setIsMac] = useState(false);
  useEffect(() => {
    void api
      .host_os()
      .then((os) => setIsMac(os === "macos"))
      .catch(() => {});
  }, []);

  // Snapshot on mount + subscribe to live updates.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      try {
        const initial = await getCommandCenterState();
        if (!cancelled) setSnap(initial);
      } catch {
        // Boot-race: backend not ready yet. The first state event
        // will land us in the right place.
      }
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<CcStateSnapshot>(
          COMMAND_CENTER_STATE_EVENT,
          (e) => {
            if (cancelled) return;
            setSnap(e.payload);
          },
        );
      } catch {
        // Non-Tauri (test) environment: leave the snapshot at its
        // initial value.
      }
    })();

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // Reset the elapsed timer when a SessionCard appears; clear it
  // when we leave that state.
  useEffect(() => {
    if (snap.state === "sessionCard") {
      if (sessionStartedAt.current == null) {
        sessionStartedAt.current = Date.now();
        setElapsedMs(0);
      }
      const id = window.setInterval(() => {
        if (sessionStartedAt.current != null) {
          setElapsedMs(Date.now() - sessionStartedAt.current);
        }
      }, 250);
      return () => window.clearInterval(id);
    }
    sessionStartedAt.current = null;
    setElapsedMs(0);
    return undefined;
  }, [snap.state]);

  // Esc dismisses regardless of which sub-view we're showing. The
  // SessionCard's Stop button is a separate handler (Esc must not
  // stop a recording).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        void dismissCommandCenter();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Outside-click dismiss. The overlay is a full-window webview at
  // bottom-center; clicking the body BUT outside the card panel
  // dismisses.
  const cardRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const onPointer = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (target && cardRef.current && !cardRef.current.contains(target)) {
        void dismissCommandCenter();
      }
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, []);

  const onPickMode = useCallback(async (kind: RecordingKind) => {
    try {
      await pickCommandCenterMode(kind);
    } catch (err) {
      // Surface a soft log; the state machine emits a Launching ->
      // ModePicker transition on its own when the runtime refuses.
      // We don't want a toast layer here in 1A.
      console.warn("cc: pick mode failed", err);
    }
  }, []);

  const onStop = useCallback(async () => {
    try {
      await stopActiveCommandCenterSession();
    } catch (err) {
      console.warn("cc: stop failed", err);
    }
  }, []);

  if (snap.state === "closed") return null;

  return (
    <div className={styles.root} aria-modal data-state={snap.state}>
      <div className={styles.card} ref={cardRef} role="dialog">
        {snap.firstRun && snap.state === "modePicker" && (
          <>
            <header className={styles.welcomeBand}>
              <h2 className={styles.welcomeTitle}>Welcome to Mockingbird</h2>
              <p className={styles.welcomeBody}>
                Press <kbd>Right&nbsp;Ctrl</kbd> + <kbd>Space</kbd> any time to
                pop this card back up.
              </p>
            </header>
            <button
              type="button"
              className={styles.closeButton}
              aria-label="Close welcome card"
              onClick={() => void dismissCommandCenter()}
            >
              <svg
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.75"
                strokeLinecap="round"
                aria-hidden="true"
                focusable="false"
              >
                <path d="M4 4 L12 12 M12 4 L4 12" />
              </svg>
            </button>
          </>
        )}

        {(snap.state === "modePicker" || snap.state === "launching") && (
          <ModePicker
            launchingKind={
              snap.state === "launching" ? snap.kind ?? null : null
            }
            onPick={onPickMode}
            isMac={isMac}
          />
        )}

        {snap.state === "sessionCard" && (
          <SessionCard kind={snap.kind ?? "dictation"} elapsedMs={elapsedMs} onStop={onStop} />
        )}
      </div>
    </div>
  );
}

interface ModePickerProps {
  launchingKind: RecordingKind | null;
  onPick: (kind: RecordingKind) => void;
  /** macOS-port — when true, the Activity tile is disabled + relabelled
   *  "coming soon on macOS" (its backend is Windows-only). */
  isMac: boolean;
}

function ModePicker({ launchingKind, onPick, isMac }: ModePickerProps): JSX.Element {
  return (
    <div className={styles.modePicker} role="group" aria-label="Pick a recording mode">
      {MODES.map((m) => {
        const isLaunchingThis = launchingKind === m.kind;
        // Activity is Coming soon on macOS: disable the tile so it can't
        // launch a capture-nothing session.
        const comingSoon = isMac && m.kind === "activity";
        const disabled =
          (m.disabled ?? false) || comingSoon || launchingKind != null;
        const hint = comingSoon
          ? "coming soon on macOS"
          : isLaunchingThis
            ? "starting…"
            : m.hint;
        return (
          <button
            key={m.kind}
            type="button"
            className={styles.modeTile}
            data-kind={m.kind}
            data-launching={isLaunchingThis ? "true" : undefined}
            data-coming-soon={comingSoon ? "true" : undefined}
            disabled={disabled}
            onClick={() => onPick(m.kind)}
          >
            <span className={styles.modeTileTitle}>{m.title}</span>
            <span className={styles.modeTileHint}>{hint}</span>
          </button>
        );
      })}
    </div>
  );
}

interface SessionCardProps {
  kind: RecordingKind;
  elapsedMs: number;
  onStop: () => void;
}

function SessionCard({ kind, elapsedMs, onStop }: SessionCardProps): JSX.Element {
  const label = useMemo(() => {
    switch (kind) {
      case "dictation":
        return "Dictation";
      case "meeting":
        return "Meeting";
      case "activity":
        return "Activity";
    }
  }, [kind]);
  return (
    <div className={styles.sessionCard}>
      <div className={styles.sessionMeta}>
        <span className={styles.sessionKind}>{label}</span>
        <span className={styles.sessionElapsed} aria-live="polite">
          {formatElapsed(elapsedMs)}
        </span>
      </div>
      <button type="button" className={styles.stopButton} onClick={onStop}>
        Stop
      </button>
    </div>
  );
}

/**
 * Format a millisecond duration as `m:ss` (or `h:mm:ss` past an hour).
 * Pure function — exported for the unit test next door.
 */
export function formatElapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "0:00";
  const totalSecs = Math.floor(ms / 1000);
  const secs = totalSecs % 60;
  const mins = Math.floor(totalSecs / 60) % 60;
  const hours = Math.floor(totalSecs / 3600);
  const pad2 = (n: number): string => (n < 10 ? `0${n}` : String(n));
  if (hours > 0) return `${hours}:${pad2(mins)}:${pad2(secs)}`;
  return `${mins}:${pad2(secs)}`;
}
