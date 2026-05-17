// Recording overlay — the pill that appears while RightAlt is held.
//
// Phase 0 contract:
//   - Renders a frameless transparent Tauri window content area.
//   - Subscribes to a `dictation:state` Tauri event for state.
//   - Renders a fake waveform while Listening (real RMS comes Phase 4).
//   - Esc emits `dictation:cancel` (orchestrator wires this up later).
//   - Honors `prefers-reduced-motion`.
//   - Renders in a plain browser too (Vite preview / Playwright):
//     defaults to a static "Listening" pill so designers see something.
//
// The waveform is a sine + noise oscillator, NOT real audio. Wiring
// real RMS would mean piping samples from the orchestrator's audio
// thread, which we don't have yet. The fake one shows the UX cleanly
// and gets replaced 1-to-1 in Phase 4.

import { useEffect, useMemo, useRef, useState } from "react";

import { isTauri } from "../lib/tauri";
import { XIcon } from "../design/Icon";
import { t } from "../i18n";
import styles from "./RecordingWindow.module.css";

const BAR_COUNT = 24;

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
  // Default to "listening" in dev so the overlay isn't a confused
  // blank box during design work. In Tauri the orchestrator will
  // emit an explicit state right after spawn.
  const [event, setEvent] = useState<DictationEvent>({
    state: isTauri() ? "idle" : "listening",
    modeSlug: "normal",
    modeLabel: "Normal",
  });

  // Subscribe to Tauri events. Dynamically imported so the recording
  // bundle stays import-free outside Tauri.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<DictationEvent>("dictation:state", (e) => {
        setEvent(e.payload);
      });
    })();
    return () => unlisten?.();
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

  const isActive = event.state === "listening";
  const showWaveform = event.state === "listening";

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
        <span className={styles.dot} aria-hidden="true" />

        {showWaveform ? (
          <Waveform active={isActive} />
        ) : (
          <span className={styles.barsStatic} aria-hidden="true" />
        )}

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

/**
 * Fake-but-pretty animated waveform. 24 vertical bars; each bar is a
 * phase-offset sine modulated by deterministic noise so the motion
 * reads as "audio" instead of "monotonic clock".
 *
 * Real audio RMS goes here once the orchestrator pipes samples
 * across (Phase 4 ticket). Until then this gives reviewers a sense
 * of the look without depending on Rust state.
 */
function Waveform({ active }: { active: boolean }) {
  const refs = useRef<(HTMLDivElement | null)[]>([]);
  const startedAt = useMemo(() => performance.now(), []);

  useEffect(() => {
    if (!active) return;
    // Respect the OS-level reduced-motion preference at the JS layer
    // too — CSS hides the bars but we shouldn't burn cycles ticking
    // an invisible canvas.
    if (
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
    ) {
      return;
    }
    let raf = 0;
    function tick(now: number) {
      const t = (now - startedAt) / 1000;
      for (let i = 0; i < BAR_COUNT; i++) {
        const el = refs.current[i];
        if (!el) continue;
        // Phase per bar + per-bar noise. The `% 7`-style noise keeps
        // the bars from marching in unison without needing Math.random
        // (which would re-trigger React renders if we put it in state).
        const phase = (i / BAR_COUNT) * Math.PI * 2;
        const noise = ((i * 13) % 7) / 7;
        const v = Math.abs(Math.sin(t * 4 + phase) * (0.6 + noise * 0.4));
        el.style.height = `${2 + v * 16}px`;
      }
      raf = requestAnimationFrame(tick);
    }
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [active, startedAt]);

  return (
    <div className={styles.bars} aria-hidden="true">
      {Array.from({ length: BAR_COUNT }, (_, i) => (
        <div
          key={i}
          ref={(el) => {
            refs.current[i] = el;
          }}
          className={`${styles.bar} ${active ? styles.barActive : ""}`}
        />
      ))}
    </div>
  );
}
