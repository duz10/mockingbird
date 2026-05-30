// Phase 1D Wave 1D.3 (`mb-0gt6`, ADR 0052) -- KG dashboard capture
// surface. Two side-by-side inputs:
//
//   1. Audio note  -> `dictation_start_kg_note` / `dictation_stop`.
//      Reuses the dictation orchestrator (Whisper + cleanup) so the
//      transcript quality matches every other dictation. The session
//      lands with `capture_kind='kg-note'`, which (a) appears in the
//      Dictations history (intentional dual-write), and (b) fires
//      the KG filing queue via the source-gated dictation-tail hook
//      from Wave 1D.1.
//
//   2. Text note   -> `kg_ingest_text_note(text)`.
//      Bypasses Whisper entirely; the typed string IS the transcript.
//      Session lands with `capture_kind='kg-note-text'`, which the
//      `list_sessions` filter excludes from the Dictations history
//      page. The row still exists for provenance + parity-probe
//      coverage, but the user only ever sees it surface in the KG
//      dashboard bands.
//
// Both flows funnel into the SAME 3-gate cascade introduced in 1D.1:
// `kg_outcome_for(capture_kind)` returns `Enqueue` for kg-note* iff
// `KgGraphEnabled` is `true`. The route-level guard upstream already
// gates this component on `kgGraphEnabled === true`, so the user
// never sees these controls while the toggle is off.
//
// Status feedback reuses the in-flight LESSONS pattern from
// SettingsKgFailedFilings: a single `status` discriminated union
// (idle / busy / success / error) drives a `role="status"` line.
// The audio path additionally listens on the existing
// `history:session-saved` event (the same bus the Dictations page
// uses) to flip from "Transcribing..." -> "Filed" without polling.

import { useEffect, useRef, useState } from "react";

import { Button, Card } from "../../components/primitives";
import { t } from "../../i18n";
import { api, isTauri } from "../../lib/tauri";

import styles from "./CaptureBand.module.css";

/* ------------------------------------------------------------------ */
/* Status state                                                        */
/* ------------------------------------------------------------------ */

/** Status discriminated union per capture lane. */
type LaneStatus =
  | { kind: "idle" }
  | { kind: "busy"; label: string }
  | { kind: "success"; label: string }
  | { kind: "error"; label: string };

const IDLE: LaneStatus = { kind: "idle" };

interface CaptureBandProps {
  /** Called whenever a capture (audio or text) successfully files,
   *  so the parent dashboard can refetch its snapshot and the
   *  user sees the new row in the recent-activity band without
   *  reload. Optional: the audio path also emits the global
   *  `history:session-saved` event, which other listeners (e.g.
   *  the Dictations page) react to. */
  onFiled?: () => void;
}

export function CaptureBand({ onFiled }: CaptureBandProps) {
  return (
    <Card title={t("kg.dashboard.capture.heading")}>
      <p className={styles.subtitle}>{t("kg.dashboard.capture.subtitle")}</p>
      <div className={styles.lanes}>
        <AudioNoteLane onFiled={onFiled} />
        <TextNoteLane onFiled={onFiled} />
      </div>
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Audio note lane                                                     */
/* ------------------------------------------------------------------ */

/** Phases the local UI tracks for the audio note. The orchestrator
 *  is the source of truth (PTT FSM); this is just the button's
 *  view-model so we know whether to label the button "Start" or
 *  "Stop" and what to put in the status line. */
type AudioPhase = "idle" | "recording" | "processing";

function AudioNoteLane({ onFiled }: { onFiled?: () => void }) {
  const [phase, setPhase] = useState<AudioPhase>("idle");
  const [status, setStatus] = useState<LaneStatus>(IDLE);
  // Guard against stale event handlers updating state after unmount;
  // dynamic-import of `@tauri-apps/api/event` makes the lifecycle
  // mildly racy.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Subscribe to `history:session-saved` so we can flip the button
  // back to "idle" (and set the success label) once the row commits.
  // We deliberately listen to the SAME global event the Dictations
  // page uses -- there's no per-lane filter needed because the lane
  // only goes into `processing` after WE clicked Start; any saved
  // session that arrives while we're processing is by definition
  // ours (PTT is single-session).
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{ sessionId: number }>(
        "history:session-saved",
        (e) => {
          if (cancelled || !mountedRef.current) return;
          setPhase((current) => {
            if (current !== "processing") return current;
            const id = e.payload?.sessionId ?? 0;
            setStatus({
              kind: "success",
              label: t("kg.dashboard.capture.audio.filed").replace(
                "{sessionId}",
                String(id),
              ),
            });
            onFiled?.();
            return "idle";
          });
        },
      );
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onFiled]);

  const handleStart = async () => {
    setStatus({
      kind: "busy",
      label: t("kg.dashboard.capture.audio.recording"),
    });
    setPhase("recording");
    try {
      await api.dictation_start_kg_note();
    } catch (err) {
      if (!mountedRef.current) return;
      setPhase("idle");
      setStatus({
        kind: "error",
        label: t("kg.dashboard.capture.audio.failed").replace(
          "{error}",
          String(err),
        ),
      });
    }
  };

  const handleStop = async () => {
    setStatus({
      kind: "busy",
      label: t("kg.dashboard.capture.audio.processing"),
    });
    setPhase("processing");
    try {
      await api.dictation_stop();
    } catch (err) {
      if (!mountedRef.current) return;
      setPhase("idle");
      setStatus({
        kind: "error",
        label: t("kg.dashboard.capture.audio.failed").replace(
          "{error}",
          String(err),
        ),
      });
    }
  };

  const isRecording = phase === "recording";
  const isBusy = phase !== "idle";
  return (
    <section
      className={styles.lane}
      aria-label={t("kg.dashboard.capture.audio.start")}
    >
      <Button
        variant={isRecording ? "danger" : "primary"}
        onClick={isRecording ? handleStop : handleStart}
        disabled={phase === "processing"}
        ariaLabel={
          isRecording
            ? t("kg.dashboard.capture.audio.stop")
            : t("kg.dashboard.capture.audio.start")
        }
      >
        {isRecording
          ? t("kg.dashboard.capture.audio.stop")
          : t("kg.dashboard.capture.audio.start")}
      </Button>
      <StatusLine status={status} isBusy={isBusy} />
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Text note lane                                                      */
/* ------------------------------------------------------------------ */

function TextNoteLane({ onFiled }: { onFiled?: () => void }) {
  const [text, setText] = useState("");
  const [status, setStatus] = useState<LaneStatus>(IDLE);
  const [submitting, setSubmitting] = useState(false);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const handleSubmit = async () => {
    const trimmed = text.trim();
    if (trimmed.length === 0) {
      setStatus({
        kind: "error",
        label: t("kg.dashboard.capture.text.empty"),
      });
      return;
    }
    setSubmitting(true);
    setStatus({
      kind: "busy",
      label: t("kg.dashboard.capture.text.submitting"),
    });
    try {
      const id = await api.kg_ingest_text_note(trimmed);
      if (!mountedRef.current) return;
      setText("");
      setStatus({
        kind: "success",
        label: t("kg.dashboard.capture.text.filed").replace(
          "{sessionId}",
          String(id),
        ),
      });
      onFiled?.();
    } catch (err) {
      if (!mountedRef.current) return;
      setStatus({
        kind: "error",
        label: t("kg.dashboard.capture.text.failed").replace(
          "{error}",
          String(err),
        ),
      });
    } finally {
      if (mountedRef.current) setSubmitting(false);
    }
  };

  return (
    <section
      className={styles.lane}
      aria-label={t("kg.dashboard.capture.text.label")}
    >
      <label className={styles.textLabel} htmlFor="kg-text-note-input">
        {t("kg.dashboard.capture.text.label")}
      </label>
      <textarea
        id="kg-text-note-input"
        className={styles.textInput}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={t("kg.dashboard.capture.text.placeholder")}
        rows={3}
        disabled={submitting}
        aria-label={t("kg.dashboard.capture.text.label")}
      />
      <Button
        variant="primary"
        onClick={() => void handleSubmit()}
        disabled={submitting || text.trim().length === 0}
        ariaLabel={t("kg.dashboard.capture.text.submit")}
      >
        {submitting
          ? t("kg.dashboard.capture.text.submitting")
          : t("kg.dashboard.capture.text.submit")}
      </Button>
      <StatusLine status={status} isBusy={submitting} />
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Shared status line                                                  */
/* ------------------------------------------------------------------ */

function StatusLine({
  status,
  isBusy,
}: {
  status: LaneStatus;
  isBusy: boolean;
}) {
  if (status.kind === "idle") {
    // Reserve the vertical space so the lane doesn't jump when the
    // first status arrives. The non-breaking-space keeps the box
    // measurable for tests without rendering visible glyphs.
    return (
      <p className={styles.status} role="status" aria-live="polite">
        {"\u00a0"}
      </p>
    );
  }
  const cls =
    status.kind === "error"
      ? styles.statusError
      : status.kind === "success"
        ? styles.statusSuccess
        : styles.statusBusy;
  return (
    <p
      className={`${styles.status} ${cls}`}
      role="status"
      aria-live={isBusy ? "polite" : "polite"}
      aria-busy={isBusy ? true : undefined}
    >
      {status.label}
    </p>
  );
}
