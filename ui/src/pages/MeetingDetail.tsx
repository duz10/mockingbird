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

import { useEffect, useRef, useState } from "react";

import { Button, Card, Pill } from "../components/primitives";
import { LlmRunButton } from "../components/LlmRunButton";
import {
  CheckIcon,
  CopyIcon,
  DownloadIcon,
  PencilIcon,
  TrashIcon,
  XIcon,
} from "../design/Icon";
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
  /** Persist a new title for this meeting. Pass `null` to clear back
   *  to the auto-derived / fallback title. Parent is responsible for
   *  refreshing the detail + list after the IPC resolves. */
  onRename: (next: string | null) => Promise<void> | void;
}

export function MeetingDetailView({
  detail,
  llm,
  onLlmChange,
  onRunLlmPass,
  onCopy,
  onExport,
  onDelete,
  onRename,
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

  // Rename UI state. Local-only — the persisted title lives on
  // `detail.title` and refreshes via the parent after onRename.
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(detail.title ?? "");
  // Reset the draft when the user navigates to a different meeting
  // (otherwise the old draft would leak into the new meeting's edit).
  useEffect(() => {
    setEditing(false);
    setDraft(detail.title ?? "");
  }, [detail.uuid, detail.title]);

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
          <TitleArea
            title={title}
            editing={editing}
            draft={draft}
            onDraftChange={setDraft}
            onStartEdit={() => {
              setDraft(detail.title ?? "");
              setEditing(true);
            }}
            onCancel={() => {
              setDraft(detail.title ?? "");
              setEditing(false);
            }}
            onSave={async () => {
              const trimmed = draft.trim();
              const next = trimmed.length === 0 ? null : trimmed;
              // Skip the IPC if nothing changed.
              if ((next ?? null) === (detail.title ?? null)) {
                setEditing(false);
                return;
              }
              await onRename(next);
              setEditing(false);
            }}
          />
          <div className={styles.rowMeta}>
            <span>{formatTimestamp(detail.startedAt)}</span>
            <span>·</span>
            <span>{formatDuration(detail.totalDurationMs)}</span>
            <SourcePills source={detail.source} />
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

/* ------------------------------------------------------------------ */
/* Ephemeral-output notice (ADR 0032 / mb-rm7)                         */
/* ------------------------------------------------------------------ */

/** Persisted across sessions. Once dismissed the notice never renders
 *  again for this user. Keyed under the meetings namespace so future
 *  per-user UX flags can share the prefix without colliding. */
const LLM_EPHEMERAL_ACK_KEY = "mockingbird.meetings.llmEphemeralAck";

/** Read+write hook for the one-time "LLM output isn't saved" notice.
 *  Returns `[acknowledged, acknowledge]`. `acknowledged === true`
 *  means the user has clicked "Got it" before — never show the notice
 *  again. SSR/JSDOM-safe: falls back to `acknowledged = true` if
 *  `localStorage` is unavailable so we never block-render in tests. */
function useEphemeralAck(): [boolean, () => void] {
  const initial = readEphemeralAck();
  const [ack, setAck] = useState(initial);
  const acknowledge = () => {
    try {
      window.localStorage.setItem(LLM_EPHEMERAL_ACK_KEY, "1");
    } catch {
      // localStorage disabled / quota — best-effort.
    }
    setAck(true);
  };
  return [ack, acknowledge];
}

function readEphemeralAck(): boolean {
  try {
    return window.localStorage.getItem(LLM_EPHEMERAL_ACK_KEY) === "1";
  } catch {
    return true; // pretend ack'd; never block the panel in odd envs
  }
}

/** Exported for vitest. Resets the localStorage flag so the next
 *  mount renders the notice again. Not surfaced in any UI. */
export function __resetEphemeralAckForTest(): void {
  try {
    window.localStorage.removeItem(LLM_EPHEMERAL_ACK_KEY);
  } catch {
    /* noop */
  }
}

function LlmEphemeralNotice({ onDismiss }: { onDismiss: () => void }) {
  return (
    <aside className={styles.llmEphemeralNotice} role="note">
      <span>{t("meetings.llm.ephemeralNotice.body")}</span>
      <button
        type="button"
        className={styles.llmEphemeralDismiss}
        onClick={onDismiss}
      >
        {t("meetings.llm.ephemeralNotice.dismiss")}
      </button>
    </aside>
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
  const [ack, acknowledge] = useEphemeralAck();
  // mb-l8ey: wrap in <Card> so this panel reads as a glass-soft
  // sibling to the Dictations LLM pass panel. The inner .llmPanel
  // div remains as the flex-column layout container but no longer
  // owns a background of its own (Card owns the surface now).
  return (
    <Card title={t("meetings.llm.title")} ariaLabel="LLM pass">
      <div className={styles.llmPanel}>
      <div className={styles.llmRow}>
        <label className={styles.llmPromptLabel}>
          {t("meetings.llm.prompt.label")}
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
        </label>
        <LlmRunButton
          onClick={onRun}
          running={state.running}
          idleLabel={t("meetings.llm.run")}
          runningLabel={t("meetings.llm.running")}
          ariaLabel={t("meetings.detail.action.runLlmPass")}
        />
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

      {state.result && !ack ? (
        <LlmEphemeralNotice onDismiss={acknowledge} />
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
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Shared helpers — exported because Meetings.tsx's list rows also    */
/* render source pills + status labels.                                */
/* ------------------------------------------------------------------ */

/** Map a meeting source to one of the design-system mode tokens for
 *  the pill background. Arbitrary-but-consistent mapping gives each
 *  pill a distinct hue without minting new tokens.
 *
 *  Per-channel hues (used by [`SourcePills`]) stay stable across
 *  "mic" / "both" — the Mic pill is always green, the System pill
 *  is always indigo — so a user scanning the list reads color as
 *  channel, not as "single vs combined". */
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

/* ------------------------------------------------------------------ */
/* SourcePills — one pill per active channel.                          */
/*                                                                    */
/* `mic`     → [Mic]                                                  */
/* `system`  → [System]                                               */
/* `both`    → [Mic] [System]                                         */
/*                                                                    */
/* Per-channel colors are fixed (Mic = casual / green, System =       */
/* formal / indigo) so the list reads consistently across mixed-      */
/* source recordings. Replaces the old single-pill "both" label,      */
/* which was ambiguous without context (mb-z5y user feedback).        */
/* ------------------------------------------------------------------ */

interface SourcePillsProps {
  source: MeetingSourceKind;
}

export function SourcePills({ source }: SourcePillsProps) {
  const hasMic = source === "mic" || source === "both";
  const hasSys = source === "system" || source === "both";
  return (
    <>
      {hasMic ? (
        <Pill tone="mode-casual">{t("meetings.source.pill.mic")}</Pill>
      ) : null}
      {hasSys ? (
        <Pill tone="mode-formal">{t("meetings.source.pill.system")}</Pill>
      ) : null}
    </>
  );
}

/* ------------------------------------------------------------------ */
/* TitleArea — view + edit modes for the meeting title.                */
/* ------------------------------------------------------------------ */

interface TitleAreaProps {
  title: string;
  editing: boolean;
  draft: string;
  onDraftChange: (next: string) => void;
  onStartEdit: () => void;
  onCancel: () => void;
  onSave: () => void | Promise<void>;
}

function TitleArea({
  title,
  editing,
  draft,
  onDraftChange,
  onStartEdit,
  onCancel,
  onSave,
}: TitleAreaProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Auto-focus + select-all on entering edit mode, so the user can
  // just type a new title without first clearing the old one. Uses
  // an effect (not autoFocus prop) so the focus is re-applied if the
  // same TitleArea component instance is toggled in/out of edit mode.
  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  if (!editing) {
    return (
      <div className={styles.detailTitleRow}>
        <h2 className={styles.detailTitle}>{truncate(title, 80)}</h2>
        <button
          type="button"
          className={styles.titleEditBtn}
          onClick={onStartEdit}
          aria-label={t("meetings.detail.action.rename")}
          title={t("meetings.detail.action.rename")}
        >
          <PencilIcon size={14} />
        </button>
      </div>
    );
  }

  return (
    <div className={styles.detailTitleRow}>
      <form
        className={styles.titleEditForm}
        onSubmit={(e) => {
          e.preventDefault();
          void onSave();
        }}
      >
        <input
          ref={inputRef}
          type="text"
          className={styles.titleEditInput}
          value={draft}
          onChange={(e) => onDraftChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              onCancel();
            }
          }}
          placeholder={t("meetings.detail.rename.placeholder")}
          maxLength={200}
          aria-label={t("meetings.detail.action.rename")}
        />
        <button
          type="submit"
          className={styles.titleEditBtn}
          aria-label={t("meetings.detail.rename.save")}
          title={t("meetings.detail.rename.save")}
        >
          <CheckIcon size={14} />
        </button>
        <button
          type="button"
          className={styles.titleEditBtn}
          onClick={onCancel}
          aria-label={t("meetings.detail.rename.cancel")}
          title={t("meetings.detail.rename.cancel")}
        >
          <XIcon size={14} />
        </button>
        <span className={styles.titleEditHint}>
          {t("meetings.detail.rename.hint")}
        </span>
      </form>
    </div>
  );
}
