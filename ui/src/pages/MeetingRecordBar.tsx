// Record bar for the Meetings page.
//
// Pure presentational component — owns no IPC, just renders the
// source picker + the start/stop primary button and surfaces a few
// live indicators (pulsing dot + elapsed stopwatch + phase label).
// All state lives in the parent `MeetingsPage` so the live event
// stream wiring stays in one place.
//
// Extracted out of `Meetings.tsx` to keep that file under the
// 600-line cap. Lives in `pages/` rather than `components/` because
// the component is meaning-loaded for one specific page; promoting it
// to a generic primitive would be premature (YAGNI).

import { MicIcon } from "../design/Icon";
import { Pill } from "../components/primitives";
import { t } from "../i18n";
import type { MeetingSourceKind, MeetingSourceProbe } from "../lib/types";

import styles from "./Meetings.module.css";

export interface MeetingRecordBarProps {
  probe: MeetingSourceProbe | null;
  source: MeetingSourceKind;
  onSourceChange: (s: MeetingSourceKind) => void;
  recordingUuid: string | null;
  recordingPhase: string | null;
  startingOrStopping: "starting" | "stopping" | null;
  elapsedSec: number;
  /** Per-channel chunk progress from `meeting:progress`. Optional;
   *  when present + the phase is `"transcribing"`, render an inline
   *  "transcribing N/M" chip. */
  progress?: {
    mic?: { done: number; total: number | null };
    system?: { done: number; total: number | null };
  };
  onStart: () => void;
  onStop: () => void;
}

export function MeetingRecordBar({
  probe,
  source,
  onSourceChange,
  recordingUuid,
  recordingPhase,
  startingOrStopping,
  elapsedSec,
  progress,
  onStart,
  onStop,
}: MeetingRecordBarProps) {
  const recording = recordingUuid !== null;
  const buttonLabel = recording
    ? startingOrStopping === "stopping"
      ? t("meetings.stopping")
      : t("meetings.stop")
    : startingOrStopping === "starting"
      ? t("meetings.starting")
      : t("meetings.start");

  const isAvailable = (s: MeetingSourceKind): boolean => {
    if (!probe) return true; // Unknown probe → don't disable yet.
    if (s === "mic") return probe.micAvailable;
    if (s === "system") return probe.systemAvailable;
    return probe.micAvailable && probe.systemAvailable;
  };

  return (
    <div
      className={styles.recordBar}
      role="group"
      aria-label={t("meetings.recording")}
    >
      <div className={styles.recordRow}>
        <select
          className={styles.sourceSelect}
          value={source}
          onChange={(e) => onSourceChange(e.target.value as MeetingSourceKind)}
          disabled={recording}
          aria-label={t("meetings.source.label")}
        >
          <option value="mic" disabled={!isAvailable("mic")}>
            {t("meetings.source.mic")}
            {probe && !probe.micAvailable
              ? ` — ${t("meetings.source.unavailable")}`
              : ""}
          </option>
          <option value="system" disabled={!isAvailable("system")}>
            {t("meetings.source.system")}
            {probe && !probe.systemAvailable
              ? ` — ${t("meetings.source.unavailable")}`
              : ""}
          </option>
          <option value="both" disabled={!isAvailable("both")}>
            {t("meetings.source.both")}
          </option>
        </select>

        <button
          type="button"
          className={`${styles.recordButton} ${
            recording ? styles.recordButtonStop : ""
          }`}
          onClick={recording ? onStop : onStart}
          disabled={startingOrStopping !== null}
          aria-label={buttonLabel}
        >
          {recording ? (
            <span className={styles.recordDot} aria-hidden />
          ) : (
            <MicIcon size={16} />
          )}
          {buttonLabel}
        </button>
      </div>

      {recording ? (
        <div className={styles.recordStatus} aria-live="polite">
          <Pill tone="status-error">{t("meetings.recording")}</Pill>
          <span>{formatStopwatch(elapsedSec)}</span>
          {recordingPhase && recordingPhase !== "started" ? (
            <span>· {recordingPhase}</span>
          ) : null}
          {recordingPhase === "transcribing" && progress
            ? renderProgressChip(progress)
            : null}
        </div>
      ) : null}
    </div>
  );
}

/** Pure helper for the progress chip’s `"done/total"` text.
 *
 *  Per the master plan §Wave 5: format `"N/M"`, where `M` is `"?"`
 *  while ANY channel's `chunksTotal` is null (capture-side sender
 *  still open) and a concrete number once every channel reports its
 *  total. When both channels are active we sum across them so the
 *  chip stays compact.
 *
 *  Returns `null` when no channel has reported yet — the parent
 *  uses that to decide whether to render the chip at all.
 *
 *  Exported so the vitest harness can hit the math directly without
 *  having to spin up the React component (we don't have @testing-
 *  library installed yet — deferred to Wave 5.5 a11y / Wave 6). */
export function summarizeProgress(progress: {
  mic?: { done: number; total: number | null };
  system?: { done: number; total: number | null };
}): { done: number; total: string } | null {
  const channels = [progress.mic, progress.system].filter(
    (c): c is { done: number; total: number | null } => c !== undefined,
  );
  if (channels.length === 0) return null;

  const done = channels.reduce((sum, c) => sum + c.done, 0);
  const anyUnknown = channels.some((c) => c.total === null);
  const total = anyUnknown
    ? "?"
    : channels.reduce((sum, c) => sum + (c.total ?? 0), 0).toString();
  return { done, total };
}

function renderProgressChip(progress: {
  mic?: { done: number; total: number | null };
  system?: { done: number; total: number | null };
}): JSX.Element | null {
  const summary = summarizeProgress(progress);
  if (!summary) return null;
  return (
    <span>
      · transcribing {summary.done}/{summary.total}
    </span>
  );
}

/** Local `m:ss` formatter — duplicating `lib/format.formatStopwatch`
 *  would be a layering violation if that shape changes; the meeting
 *  ticker is a different unit (`elapsedSec` is integer-seconds, not
 *  fractional). Keep it scoped. */
function formatStopwatch(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}
