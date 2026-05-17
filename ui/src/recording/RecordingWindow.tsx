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
import { useAppStore } from "../lib/store";
import { XIcon } from "../design/Icon";
import { MockingbirdMark, type MarkState } from "../design/components/MockingbirdMark";
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

  const isActive = event.state === "listening";
  const showWaveform = event.state === "listening";
  const designVersion = useAppStore((s) => s.designVersion);
  const useV2 = designVersion === "v2";

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
        {useV2 ? (
          // v2 — the logo itself carries live state. Per the Design
          // Language v1 doc §11: "the logo carries the live state —
          // collapsed circles when waiting, oscillating ellipses
          // while recording." Replaces the dot + waveform combo.
          <MockingbirdMark
            state={dictationStateToMarkState(event.state)}
            size={28}
            // Use the idle muted gradient when the mark is in idle
            // state — matches the Design Language v1 doc §11 first
            // pill state ("Idle · tap to start").
            gradient={
              event.state === "idle" ? ["#8A7A6E", "#6B5A4E"] : undefined
            }
            title={statusLabel(event.state)}
          />
        ) : (
          <>
            <span className={styles.dot} aria-hidden="true" />
            {showWaveform ? (
              <Waveform active={isActive} />
            ) : (
              <span className={styles.barsStatic} aria-hidden="true" />
            )}
          </>
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
