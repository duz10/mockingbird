// Recording overlay — the pill that appears while RightAlt is held.
//
// Phase 0 contract:
//   - Renders a frameless transparent Tauri window content area.
//   - Subscribes to a `dictation:state` Tauri event for state.
//   - The MockingbirdMark logo carries the live state (oscillating
//     ellipses while recording, collapsed idle, settle on done) per
//     Design Language v1 §08.
//   - Esc emits `dictation:cancel` (orchestrator wires this up later).
//   - Honors `prefers-reduced-motion` (mark animations zero out).
//   - Renders in a plain browser too (Vite preview / Playwright):
//     defaults to a static "Listening" pill so designers see something.

import { useEffect, useState } from "react";

import { api, isTauri } from "../lib/tauri";
import { XIcon } from "../design/Icon";
import { MockingbirdMark, type MarkState } from "../design/components/MockingbirdMark";
import { t } from "../i18n";
import type { StartMode } from "../lib/types";
import styles from "./RecordingWindow.module.css";

/** All states the orchestrator can drive the overlay through. */
type DictationState =
  | "idle"
  | "listening"
  | "transcribing"
  | "cleaning"
  | "pasting"
  | "done"
  | "aborted";

interface DictationEvent {
  state: DictationState;
  modeSlug?: string;
  modeLabel?: string;
  /** ADR 0045 + mb-tfyp — present on every emit while a session
   *  is in flight. `"in_app"` unlocks the inline Stop button below;
   *  `"ptt"` keeps the legacy pill behavior (no Stop, the user
   *  releases Right Alt to finalize). */
  startMode?: StartMode;
}

/** Status-label lookup. Centralized so i18n keys stay in one place. */
function statusLabel(state: DictationState): string {
  switch (state) {
    case "listening":    return t("recording.state.listening");
    case "transcribing": return t("recording.state.transcribing");
    case "cleaning":     return t("recording.state.cleaning");
    case "pasting":      return t("recording.state.pasting");
    case "done":         return t("recording.state.done");
    case "aborted":      return t("recording.state.aborted");
    case "idle":         return t("recording.state.idle");
  }
}

/**
 * Map dictation pipeline state → MockingbirdMark visual state.
 * Per Design Language v1 §08 (live state in motion): collapsed circles
 * when waiting, oscillating ellipses while recording, settle on done.
 */
function dictationStateToMarkState(state: DictationState): MarkState {
  switch (state) {
    case "idle":         return "idle";
    case "listening":    return "active";
    case "transcribing": return "active";
    case "cleaning":     return "active";
    case "pasting":      return "active";
    case "done":         return "static";
    case "aborted":      return "exit";
  }
}

/** CSS Module classname per state — keeps the mapping explicit. */
function stateClass(state: DictationState): string {
  return {
    idle:         styles.stateIdle!,
    listening:    styles.stateListening!,
    transcribing: styles.stateTranscribing!,
    cleaning:     styles.stateCleaning!,
    pasting:      styles.statePasting!,
    done:         styles.stateDone!,
    aborted:      styles.stateAborted!,
  }[state];
}

/** Tailwind-token style mode → color var lookup. */
function modeColorVar(slug: string | undefined): string {
  if (!slug) return "var(--mode-normal)";
  return `var(--mode-${slug}, var(--mode-normal))`;
}

export function RecordingWindow() {
  // Start in "listening" state so the overlay renders the pretty pill
  // immediately on first show — there's a cold-start race where the
  // orchestrator's first `dictation:state` emit can fire BEFORE this
  // component has mounted its listener. The Rust side now re-emits
  // at 50/200/500ms to win the race; this default makes the visual
  // correct even if all three emits somehow miss.
  const [event, setEvent] = useState<DictationEvent>({
    state: "listening",
    modeSlug: "normal",
    modeLabel: "Normal",
  });
  /** Debounce flag for the Stop button so a rage-click doesn't fire
   *  two `dictation_stop` IPCs in <80ms (the FSM no-ops the second,
   *  but we save the round-trip + log line). */
  const [stopping, setStopping] = useState(false);

  // Subscribe to Tauri events. Dynamically imported so the recording
  // bundle stays import-free outside Tauri.
  //
  // We MERGE the incoming payload into the existing state rather than
  // replacing it — mid-pipeline events like `transcribing` only carry
  // the new state, no modeSlug/modeLabel, so a naive replace would
  // blank out the mode badge between listening → transcribing.
  //
  // **Watchdog**: also kick a 60-second timer on every state event.
  // If it ever fires (no state update for 60s = Rust orchestrator is
  // hung OR the process has crashed), hide ourselves so the user
  // isn't staring at a frozen pill forever. Saw this happen in Phase
  // 5 smoketest when an Ollama HTTP call hung past the 30s timeout
  // and the parent process crashed before the cleanup-fallback path
  // could run.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let watchdog: number | undefined;

    const kickWatchdog = () => {
      if (watchdog !== undefined) window.clearTimeout(watchdog);
      watchdog = window.setTimeout(() => {
        // No state update for 60s — assume the Rust side is dead and
        // self-destruct. Use Tauri's window API directly because IPC
        // to a crashed process won't return.
        void (async () => {
          // eslint-disable-next-line no-console
          console.warn(
            "[recording] no state event for 60s; assuming orchestrator dead, hiding self",
          );
          try {
            const { getCurrentWindow } = await import("@tauri-apps/api/window");
            await getCurrentWindow().hide();
          } catch (err) {
            // eslint-disable-next-line no-console
            console.warn("[recording] watchdog hide failed", err);
          }
        })();
      }, 60_000);
    };

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<DictationEvent>("dictation:state", (e) => {
        setEvent((prev) => ({ ...prev, ...e.payload }));
        kickWatchdog();
      });
      kickWatchdog(); // Arm on initial mount too.
    })();
    return () => {
      unlisten?.();
      if (watchdog !== undefined) window.clearTimeout(watchdog);
    };
  }, []);

  // Esc → cancel the active dictation. Hook is global so the overlay
  // doesn't need to be focused (it usually isn't — focus stays in the
  // target app while the user dictates).
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      if (!isTauri()) return;
      // Fire-and-forget; the orchestrator will (eventually) listen.
      void (async () => {
        const { emit } = await import("@tauri-apps/api/event");
        await emit("dictation:cancel");
      })();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className={styles.shell}>
      <div
        className={`${styles.pill} ${stateClass(event.state)}`}
        role="status"
        aria-live="polite"
        // The whole pill is the drag surface; opt-out items below
        // declare `app-region: no-drag` so clicks register.
        data-tauri-drag-region
      >
        {/* The logo itself carries live state. Per the Design
            Language v1 doc §11: "the logo carries the live state —
            collapsed circles when waiting, oscillating ellipses
            while recording." */}
        <MockingbirdMark
          state={dictationStateToMarkState(event.state)}
          size={28}
          // Use the idle muted gradient when the mark is in idle
          // state — matches the design doc §11 first pill state.
          gradient={
            event.state === "idle" ? ["#8A7A6E", "#6B5A4E"] : undefined
          }
          title={statusLabel(event.state)}
        />

        <span className={styles.statusText}>{statusLabel(event.state)}</span>

        {event.modeLabel && (
          <span
            className={styles.modeBadge}
            style={
              { ["--mode-color" as string]: modeColorVar(event.modeSlug) } as React.CSSProperties
            }
          >
            {event.modeLabel}
          </span>
        )}

        {/* ADR 0045 + mb-sowc — Stop button is only meaningful for
            in-app sessions. PTT sessions finalize by releasing Right
            Alt; an inline Stop would either be a no-op (the FSM
            ignores `dictation_stop` for a PTT-held session because
            the synthetic KeyUp uses a different VK) or worse,
            confusing UI. Mirror the meeting overlay's `Stop / Cancel`
            side-by-side pattern: Stop is primary (danger-tinted),
            Cancel/X is the secondary discard. */}
        {event.startMode === "in_app" && (
          <button
            type="button"
            className={styles.stop}
            aria-label={t("recording.action.stop")}
            disabled={stopping}
            onClick={() => {
              if (stopping) return;
              setStopping(true);
              void (async () => {
                try {
                  await api.dictation_stop();
                } catch (err) {
                  // eslint-disable-next-line no-console
                  console.warn("[recording] dictation_stop failed", err);
                } finally {
                  // Hide-on-done is driven by the orchestrator's
                  // `idle` emit; this just unlocks the button if
                  // the user opens another session via PTT later.
                  window.setTimeout(() => setStopping(false), 200);
                }
              })();
            }}
          >
            {stopping ? "…" : t("recording.action.stop")}
          </button>
        )}

        <button
          type="button"
          className={styles.cancel}
          aria-label={t("recording.action.cancel")}
          onClick={() => {
            if (!isTauri()) return;
            void (async () => {
              const { emit } = await import("@tauri-apps/api/event");
              await emit("dictation:cancel");
            })();
          }}
        >
          <XIcon width={14} height={14} />
        </button>
      </div>
    </div>
  );
}

