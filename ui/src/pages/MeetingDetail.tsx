// Detail pane for the Meetings page.
//
// Renders the selected meeting's metadata + tabbed transcript view +
// LLM-pass panel. Pure presentational component — all data and
// callbacks come from the parent. Extracted out of `Meetings.tsx`
// to keep both files under the 600-line cap (AGENTS.md house rule).
//
// Why not its own routed page: the parent two-pane layout keeps the
// record-bar visible while the user reads transcripts, which is more
// useful than a dedicated route. See the comment block at the top of
// `Meetings.tsx` for the rationale.

import { useEffect, useState } from "react";

import { Button, Card, Pill } from "../components/primitives";
import { CopyIcon, DownloadIcon, TrashIcon } from "../design/Icon";
import { t } from "../i18n";
import { formatDuration, formatTimestamp, truncate } from "../lib/format";
import type {
  BuiltInPromptId,
  LlmPassResult,
  MeetingDetail as MeetingDetailType,
  MeetingSourceKind,
} from "../lib/types";

import styles from "./Meetings.module.css";

/* ------------------------------------------------------------------ */
/* Shared types (also imported by Meetings.tsx)                        */
/* ------------------------------------------------------------------ */

export type TranscriptTab = "merged" | "mic" | "system";

export interface LlmPassUiState {
  /** Which prompt is currently selected in the dropdown. */
  promptId: BuiltInPromptId | "custom";
  /** Custom body, only used when `promptId === "custom"`. */
  customBody: string;
  /** Last successful run for this meeting, if any. */
  result: LlmPassResult | null;
  /** True while the IPC call is in flight. */
  running: boolean;
  /** Whether to attach the LLM-pass text on Copy/Export. */
  includeInExport: boolean;
}

export const FRESH_LLM: LlmPassUiState = {
  promptId: "summary",
  customBody: "",
  result: null,
  running: false,
  includeInExport: true,
};

/* ------------------------------------------------------------------ */
/* DetailView component                                                */
/* ------------------------------------------------------------------ */

interface DetailViewProps {
  detail: MeetingDetailType;
  llm: LlmPassUiState;
  onLlmChange: (patch: Partial<LlmPassUiState>) => void;
  onRunLlmPass: () => void;
  onCopy: () => void;
  onExport: () => void;
  onDelete: () => void;
}

export function MeetingDetailView({
  detail,
  llm,
  onLlmChange,
  onRunLlmPass,
  onCopy,
  onExport,
  onDelete,
}: DetailViewProps) {
  // Default tab: prefer merged when both channels present; else
  // whichever single channel has content; else mic as a placeholder.
  const defaultTab: TranscriptTab =
    detail.formattedMerged != null
      ? "merged"
      : detail.formattedMic != null
        ? "mic"
        : detail.formattedSys != null
          ? "system"
          : "mic";
  const [tab, setTab] = useState<TranscriptTab>(defaultTab);

  // Re-default the tab when the user navigates to a different meeting.
  useEffect(() => {
    setTab(defaultTab);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail.uuid]);

  const title = detail.title?.trim() || t("meetings.detail.untitled");
  const transcript =
    tab === "merged"
      ? detail.formattedMerged
      : tab === "mic"
        ? detail.formattedMic
        : detail.formattedSys;

  return (
    <>
      <header className={styles.detailHeader}>
        <div>
          <h2 className={styles.detailTitle}>{truncate(title, 80)}</h2>
          <div className={styles.rowMeta}>
            <span>{formatTimestamp(detail.startedAt)}</span>
            <span>·</span>
            <span>{formatDuration(detail.totalDurationMs)}</span>
            <Pill tone={`mode-${sourceTone(detail.source)}`}>{detail.source}</Pill>
            {detail.status !== "complete" ? (
              <Pill tone="status-error">{statusLabel(detail.status)}</Pill>
            ) : (
              <Pill tone="status-ok">{statusLabel(detail.status)}</Pill>
            )}
          </div>
        </div>
        <div className={styles.detailActions}>
          <Button onClick={onCopy} ariaLabel={t("meetings.detail.action.copy")}>
            <CopyIcon size={14} /> {t("meetings.detail.action.copy")}
          </Button>
          <Button
            onClick={onExport}
            ariaLabel={t("meetings.detail.action.export")}
          >
            <DownloadIcon size={14} /> {t("meetings.detail.action.export")}
          </Button>
          <Button
            variant="danger"
            onClick={onDelete}
            ariaLabel={t("meetings.detail.action.delete")}
          >
            <TrashIcon size={14} /> {t("meetings.detail.action.delete")}
          </Button>
        </div>
      </header>

      <Card ariaLabel="Transcript">
        <div className={styles.tabs} role="tablist">
          <TranscriptTabButton
            label={t("meetings.detail.tab.merged")}
            active={tab === "merged"}
            disabled={detail.formattedMerged == null}
            onClick={() => setTab("merged")}
          />
          <TranscriptTabButton
            label={t("meetings.detail.tab.mic")}
            active={tab === "mic"}
            disabled={detail.formattedMic == null}
            onClick={() => setTab("mic")}
          />
          <TranscriptTabButton
            label={t("meetings.detail.tab.system")}
            active={tab === "system"}
            disabled={detail.formattedSys == null}
            onClick={() => setTab("system")}
          />
        </div>
        <div
          className={styles.transcript}
          role="tabpanel"
          aria-label={`Transcript: ${tab}`}
        >
          {transcript ?? t("meetings.detail.noTranscript")}
        </div>
      </Card>

      <Card title="Metadata">
        <div className={styles.metaGrid}>
          <span className={styles.metaKey}>
            {t("meetings.detail.duration")}
          </span>
          <span className={styles.metaVal}>
            {formatDuration(detail.totalDurationMs)}
          </span>
          <span className={styles.metaKey}>{t("meetings.detail.source")}</span>
          <span className={styles.metaVal}>{detail.source}</span>
          <span className={styles.metaKey}>{t("meetings.detail.status")}</span>
          <span className={styles.metaVal}>{statusLabel(detail.status)}</span>
          <span className={styles.metaKey}>{t("meetings.detail.model")}</span>
          <span className={styles.metaVal}>{detail.whisperModelId}</span>
          <span className={styles.metaKey}>
            {t("meetings.detail.formatter")}
          </span>
          <span className={styles.metaVal}>{detail.formatterVersion}</span>
          <span className={styles.metaKey}>{t("meetings.detail.uuid")}</span>
          <span className={styles.metaVal}>{detail.uuid}</span>
        </div>
      </Card>

      <LlmPassPanel state={llm} onChange={onLlmChange} onRun={onRunLlmPass} />
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Internal sub-components                                             */
/* ------------------------------------------------------------------ */

function TranscriptTabButton({
  label,
  active,
  disabled,
  onClick,
}: {
  label: string;
  active: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      disabled={disabled}
      className={`${styles.tab} ${active ? styles.tabActive : ""}`}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function LlmPassPanel({
  state,
  onChange,
  onRun,
}: {
  state: LlmPassUiState;
  onChange: (patch: Partial<LlmPassUiState>) => void;
  onRun: () => void;
}) {
  return (
    <div className={styles.llmPanel} aria-label="LLM pass">
      <div className={styles.llmRow}>
        <select
          className={styles.llmPromptSelect}
          value={state.promptId}
          onChange={(e) =>
            onChange({ promptId: e.target.value as typeof state.promptId })
          }
          aria-label="Prompt"
        >
          <option value="summary">{t("meetings.llm.prompt.summary")}</option>
          <option value="action_items">
            {t("meetings.llm.prompt.action_items")}
          </option>
          <option value="cleaner_punctuation">
            {t("meetings.llm.prompt.cleaner_punctuation")}
          </option>
          <option value="custom">{t("meetings.llm.prompt.custom")}</option>
        </select>
        <Button
          variant="primary"
          onClick={onRun}
          disabled={state.running}
          ariaLabel={t("meetings.detail.action.runLlmPass")}
        >
          {state.running
            ? t("meetings.llm.running")
            : t("meetings.detail.action.runLlmPass")}
        </Button>
      </div>
      {state.promptId === "custom" ? (
        <textarea
          className={styles.llmCustomInput}
          rows={3}
          placeholder="Type your prompt body…"
          value={state.customBody}
          onChange={(e) => onChange({ customBody: e.target.value })}
          aria-label="Custom prompt body"
        />
      ) : null}

      {state.result ? (
        <>
          <div className={styles.llmIncludeRow}>
            <label>
              <input
                type="checkbox"
                checked={state.includeInExport}
                onChange={(e) => onChange({ includeInExport: e.target.checked })}
              />{" "}
              {t("meetings.llm.includeInExport")} (
              {t("meetings.llm.latency").replace(
                "{ms}",
                String(state.result.latencyMs),
              )}
              )
            </label>
          </div>
          <div className={styles.llmOutput}>{state.result.text}</div>
        </>
      ) : state.running ? (
        <div className={styles.llmOutput}>{t("meetings.llm.running")}</div>
      ) : (
        <div className={styles.llmOutput}>{t("meetings.llm.notRun")}</div>
      )}

      <div className={styles.llmInvariant}>{t("meetings.llm.invariant")}</div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Shared helpers — exported because Meetings.tsx's list rows also    */
/* render source pills + status labels.                                */
/* ------------------------------------------------------------------ */

/** Map a meeting source to one of the design-system mode tokens for
 *  the pill background. Arbitrary-but-consistent mapping gives each
 *  pill a distinct hue without minting new tokens. */
export function sourceTone(source: MeetingSourceKind): string {
  switch (source) {
    case "mic":
      return "casual";
    case "both":
      return "normal";
    case "system":
      return "formal";
  }
}

export function statusLabel(status: MeetingDetailType["status"]): string {
  return t(`meetings.status.${status}`);
}
