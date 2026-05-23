// Dictations page — list + detail in a two-pane layout. Shows the
// history of past dictation sessions (Right-Alt push-to-talk).
// Sibling to the Meetings page; *not* the same thing as meeting
// recordings.
//
// **Renamed from "History" 2026-05-21.** Internal Rust event names
// (`history:session-saved`) are left alone — sealed Phase 4 code,
// and the wire-name has no user impact. The i18n keys, route, and
// component names all moved to `dictations.*` / `/dictations` /
// `DictationsPage` so the new naming is consistent from the source
// down.
//
// Why no virtual scrolling: the session list is bounded (we'll add a
// "Load more" affordance when the user hits the bottom). Real users
// hit 40-200 sessions per week; a virtualized list would be a YAGNI
// dep until someone shows up with 50k rows. If/when that happens,
// promote the .list scroll container to react-window without changing
// any other prop.
//
// Search uses the FTS5 endpoint when the query is non-empty (debounced
// 200 ms). Empty query => normal `list_sessions` paged from offset 0.
// Selecting a hit jumps to that session's detail view.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  Button,
  Card,
  EmptyState,
  PageHeader,
  Pill,
  Spinner,
} from "../components/primitives";
import { LlmRunButton } from "../components/LlmRunButton";
import {
  CheckIcon,
  CopyIcon,
  HistoryIcon,
  PlusIcon,
  SearchIcon,
  TrashIcon,
} from "../design/Icon";
import { t } from "../i18n";
import {
  formatDuration,
  formatRelative,
  formatTimestamp,
  prettyAppName,
  truncate,
} from "../lib/format";
import { useAppStore } from "../lib/store";
import { api, isTauri } from "../lib/tauri";
import type { SessionDetail, SessionSummary, TranscriptSearchHit } from "../lib/types";

import styles from "./Dictations.module.css";

const PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 200;

export function DictationsPage() {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [searchHits, setSearchHits] = useState<TranscriptSearchHit[] | null>(null);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const setSelectedSession = useAppStore((s) => s.setSelectedSession);

  // Initial list fetch.
  useEffect(() => {
    void (async () => {
      const list = await api.list_sessions(PAGE_SIZE, 0);
      setSessions(list);
      if (list[0] && selectedId === null) setSelectedId(list[0].id);
    })();
    // selectedId intentionally omitted — we only auto-pick on first load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Live-refresh: the Rust orchestrator emits `history:session-saved`
  // after each new row (Complete or Error) is committed to the DB.
  // We refetch the list silently — preserving the user's current
  // `selectedId` so they aren't yanked away from an old session
  // they're reading. The new row simply appears at the top.
  //
  // Deliberately does NOT auto-select the new row, because the user
  // may be triaging an older one. If they want the freshly-dictated
  // session, the list's top entry is one click away.
  //
  // Dynamic import keeps `@tauri-apps/api/event` out of the
  // fixture/preview bundle (same pattern RecordingWindow uses).
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{ sessionId: number }>(
        "history:session-saved",
        () => {
          // Don't refetch on a stale listener if the component
          // unmounted between event fire and async resolution.
          if (cancelled) return;
          void (async () => {
            const list = await api.list_sessions(PAGE_SIZE, 0);
            if (cancelled) return;
            setSessions(list);
            // If nothing was selected yet (e.g. very first dictation
            // since app launch with empty history), pick the new row.
            // Otherwise keep the current selection untouched.
            setSelectedId((prev) => prev ?? list[0]?.id ?? null);
          })();
        },
      );
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Debounced search.
  useEffect(() => {
    if (query.trim().length === 0) {
      setSearchHits(null);
      return;
    }
    const handle = window.setTimeout(() => {
      void (async () => {
        const hits = await api.search_transcripts(query.trim(), PAGE_SIZE);
        setSearchHits(hits);
      })();
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [query]);

  // Fetch detail when selection changes.
  useEffect(() => {
    if (selectedId === null) {
      setDetail(null);
      setSelectedSession(null);
      return;
    }
    void (async () => {
      const d = await api.get_session_detail(selectedId);
      setDetail(d);
      setSelectedSession(d);
    })();
  }, [selectedId, setSelectedSession]);

  // Reusable toast helper. Auto-clears after 1.6s.
  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 1600);
  }, []);

  // Action handlers — close over the current detail. They re-resolve
  // session list after destructive ops so the UI stays consistent.
  const handleDelete = useCallback(async () => {
    if (!detail) return;
    if (!window.confirm(t("dictations.action.delete") + "?")) return;
    await api.delete_session(detail.session.id);
    const list = await api.list_sessions(PAGE_SIZE, 0);
    setSessions(list);
    setSelectedId(list[0]?.id ?? null);
    showToast("Deleted");
  }, [detail, showToast]);

  const handleMark = useCallback(async () => {
    if (!detail) return;
    await api.mark_session_as_example(detail.session.id);
    showToast("Marked as example");
  }, [detail, showToast]);

  const handleCopy = useCallback(async () => {
    if (!detail) return;
    await navigator.clipboard.writeText(detail.final || detail.cleaned || detail.raw);
    showToast(t("dictations.copied"));
  }, [detail, showToast]);

  // Decide what to render in the list pane.
  const leftPaneContent = useMemo(() => {
    if (sessions === null) return <Spinner />;
    if (sessions.length === 0) {
      return (
        <EmptyState
          icon={<HistoryIcon size={28} />}
          title={t("dictations.empty.title")}
          subtitle={t("dictations.empty.subtitle")}
        />
      );
    }
    if (searchHits !== null) {
      return (
        <SearchHitsList
          hits={searchHits}
          selectedId={selectedId}
          onSelect={setSelectedId}
        />
      );
    }
    return (
      <SessionList
        sessions={sessions}
        selectedId={selectedId}
        onSelect={setSelectedId}
      />
    );
  }, [sessions, searchHits, selectedId]);

  return (
    <>
      <PageHeader title={t("dictations.title")} />

      <div className={styles.shell}>
        <div className={styles.leftPane}>
          <SearchInput value={query} onChange={setQuery} />
          {leftPaneContent}
        </div>

        <div className={styles.rightPane}>
          {detail ? (
            <DetailView
              detail={detail}
              onDelete={handleDelete}
              onMarkExample={handleMark}
              onCopy={handleCopy}
            />
          ) : sessions && sessions.length === 0 ? null : (
            <Spinner />
          )}
        </div>
      </div>

      {toast ? <div className={styles.toast}>{toast}</div> : null}
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Search input                                                         */
/* ------------------------------------------------------------------ */

function SearchInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className={styles.searchBox}>
      <span className={styles.searchIcon}>
        <SearchIcon size={16} />
      </span>
      <input
        type="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={t("dictations.search.placeholder")}
        className={styles.searchInput}
        aria-label={t("dictations.search.placeholder")}
      />
    </label>
  );
}

/* ------------------------------------------------------------------ */
/* List variants                                                        */
/* ------------------------------------------------------------------ */

function SessionList({
  sessions,
  selectedId,
  onSelect,
}: {
  sessions: SessionSummary[];
  selectedId: number | null;
  onSelect: (id: number) => void;
}) {
  return (
    <div className={styles.list} role="listbox" aria-label="Sessions">
      {sessions.map((s) => (
        <SessionRow
          key={s.id}
          session={s}
          active={s.id === selectedId}
          onClick={() => onSelect(s.id)}
        />
      ))}
    </div>
  );
}

function SessionRow({
  session,
  active,
  onClick,
}: {
  session: SessionSummary;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <div
      className={`${styles.row} ${active ? styles.rowActive : ""}`}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      role="option"
      aria-selected={active}
      tabIndex={0}
    >
      <div className={styles.rowHeader}>
        <Pill tone={`mode-${session.modeSlug}`}>{session.modeSlug}</Pill>
        <span className={styles.rowTime}>
          {formatTimestamp(session.startedAt)}
        </span>
      </div>
      <div className={styles.rowText}>{truncate(session.finalText, 160)}</div>
      <div className={styles.rowMeta}>
        <span>{prettyAppName(session.foregroundApp)}</span>
        <span>·</span>
        <span>{formatDuration(session.durationMs)}</span>
        {session.injectionStatus !== "ok" ? (
          <>
            <span>·</span>
            <Pill tone="status-error">{session.injectionStatus}</Pill>
          </>
        ) : null}
      </div>
    </div>
  );
}

function SearchHitsList({
  hits,
  selectedId,
  onSelect,
}: {
  hits: TranscriptSearchHit[];
  selectedId: number | null;
  onSelect: (id: number) => void;
}) {
  if (hits.length === 0) {
    return (
      <EmptyState
        icon={<SearchIcon size={28} />}
        title={t("dictations.search.noResults")}
      />
    );
  }
  return (
    <div className={styles.list} role="listbox" aria-label="Search results">
      {hits.map((h) => (
        <div
          key={`${h.sessionId}-${h.stage}`}
          className={`${styles.row} ${h.sessionId === selectedId ? styles.rowActive : ""}`}
          onClick={() => onSelect(h.sessionId)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onSelect(h.sessionId);
            }
          }}
          role="option"
          aria-selected={h.sessionId === selectedId}
          tabIndex={0}
        >
          <div className={styles.rowHeader}>
            <Pill tone={`mode-${h.modeSlug}`}>{h.modeSlug}</Pill>
            <span className={styles.rowTime}>
              {formatTimestamp(h.startedAt)}
            </span>
          </div>
          {/* FTS5 snippet is server-trusted (no user HTML can reach
              it — it comes from our own SQL `snippet()` call wrapping
              `<mark>` around the match). Safe to render. */}
          <div
            className={styles.rowText}
            dangerouslySetInnerHTML={{ __html: h.snippet }}
          />
          <div className={styles.rowMeta}>
            <span>stage: {h.stage}</span>
          </div>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Detail view                                                          */
/* ------------------------------------------------------------------ */

function DetailView({
  detail,
  onDelete,
  onMarkExample,
  onCopy,
}: {
  detail: SessionDetail;
  onDelete: () => void;
  onMarkExample: () => void;
  onCopy: () => void;
}) {
  const s = detail.session;
  const latencyTotal =
    (detail.latency.sttMs ?? 0) +
    (detail.latency.cleanupMs ?? 0) +
    (detail.latency.injectMs ?? 0);

  // "Copy" affordance shows a check for ~1s after click — implemented
  // here (not in the parent toast) so the icon swap reads as the
  // confirmation. The toast still fires for screen-reader users.
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number | null>(null);
  function fireCopy() {
    onCopy();
    setCopied(true);
    if (copyTimer.current) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopied(false), 1000);
  }
  useEffect(() => {
    return () => {
      if (copyTimer.current) window.clearTimeout(copyTimer.current);
    };
  }, []);

  return (
    <>
      <div className={styles.detailHeader}>
        <div className={styles.detailMeta}>
          <h2 style={{ margin: 0, font: "var(--type-lg)", fontWeight: 600 }}>
            {formatTimestamp(s.startedAt)}
          </h2>
          <span style={{ color: "var(--on-surf-muted)", font: "var(--type-sm)" }}>
            {formatRelative(s.startedAt)} · {prettyAppName(s.foregroundApp)}
          </span>
        </div>
        <div className={styles.detailActions}>
          <Button onClick={fireCopy} ariaLabel={t("dictations.action.copyFinal")}>
            {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
            {copied ? "Copied" : t("dictations.action.copyFinal")}
          </Button>
          <Button onClick={onMarkExample}>
            <PlusIcon size={14} />
            {t("dictations.action.markExample")}
          </Button>
          <Button variant="danger" onClick={onDelete}>
            <TrashIcon size={14} />
            {t("dictations.action.delete")}
          </Button>
        </div>
      </div>

      <Card title={t("dictations.detail.metadata")}>
        <div className={styles.metaGrid}>
          <span className={styles.metaKey}>{t("dictations.detail.mode")}</span>
          <span className={styles.metaVal}>
            <Pill tone={`mode-${s.modeSlug}`}>{s.modeSlug}</Pill>
          </span>
          <span className={styles.metaKey}>{t("dictations.detail.model")}</span>
          <span className={styles.metaVal}>{detail.modelUsed ?? "—"}</span>
          <span className={styles.metaKey}>
            {t("dictations.detail.promptVersion")}
          </span>
          <span className={styles.metaVal}>{detail.promptVersion ?? "—"}</span>
          <span className={styles.metaKey}>
            {t("dictations.detail.dictVersion")}
          </span>
          <span className={styles.metaVal}>
            {detail.dictionaryVersion ?? "—"}
          </span>
          <span className={styles.metaKey}>{t("dictations.detail.app")}</span>
          <span className={styles.metaVal}>
            {prettyAppName(s.foregroundApp)} —{" "}
            {s.foregroundWindowTitle ?? "—"}
          </span>
          <span className={styles.metaKey}>{t("dictations.detail.latency")}</span>
          <span className={styles.metaVal}>
            STT {Math.round(detail.latency.sttMs ?? 0)} ms · Clean{" "}
            {Math.round(detail.latency.cleanupMs ?? 0)} ms · Inject{" "}
            {Math.round(detail.latency.injectMs ?? 0)} ms · Total{" "}
            {Math.round(latencyTotal)} ms
          </span>
        </div>
      </Card>

      <Card>
        <Stage label={t("dictations.detail.raw")} text={detail.raw} variant="raw" />
        <Stage
          label={t("dictations.detail.cleaned")}
          text={detail.cleaned}
          variant="cleaned"
        />
        <Stage
          label={t("dictations.detail.final")}
          text={detail.final}
          variant="final"
        />
      </Card>

      <LlmPassCard sessionId={detail.session.id} />
    </>
  );
}

/* ------------------------------------------------------------------ */
/* LLM-pass card                                                       */
/*                                                                    */
/* Optional, off the critical path. Calls the dictation LLM-pass IPC  */
/* with a built-in prompt and renders the result. Nothing is          */
/* persisted — same invariant as the meeting LLM pass.                */
/* ------------------------------------------------------------------ */

type DictLlmPrompt = "summary" | "action_items" | "cleaner_punctuation";

const LLM_PROMPT_OPTIONS: Array<{ id: DictLlmPrompt; labelKey: string }> = [
  { id: "summary", labelKey: "meetings.llm.prompt.summary" },
  { id: "action_items", labelKey: "meetings.llm.prompt.action_items" },
  {
    id: "cleaner_punctuation",
    labelKey: "meetings.llm.prompt.cleaner_punctuation",
  },
];

/* ------------------------------------------------------------------ */
/* Minimal LLM-output renderer.                                       */
/*                                                                    */
/* Our action_items / summary / cleaner_punctuation prompts emit at   */
/* most: paragraphs and `- ` bullet lists. We render exactly those    */
/* two shapes — no react-markdown dep, no nested-list support, no     */
/* inline emphasis parsing. YAGNI: if a future prompt ever needs      */
/* tables or headings, swap in a real markdown lib then. Until then   */
/* this stays a ~40-line pure transform.                              */
/*                                                                    */
/* The Copy button copies `result.text` (the source markdown), so     */
/* paste-into-other-apps still gets dash bullets, not stripped text.  */
/* ------------------------------------------------------------------ */

function LlmMarkdownView({ text }: { text: string }) {
  // Split the input into "blocks" separated by one-or-more blank
  // lines. Each block is either a bullet list (every line starts
  // with `- ` or `* `) or a paragraph.
  const blocks = text
    .split(/\n\s*\n/)
    .map((b) => b.trim())
    .filter(Boolean);

  return (
    <div className={styles.llmResultMd}>
      {blocks.map((block, i) => {
        const lines = block.split("\n").map((l) => l.trim()).filter(Boolean);
        const isBulletList =
          lines.length > 0 &&
          lines.every((l) => l.startsWith("- ") || l.startsWith("* "));
        if (isBulletList) {
          return (
            <ul key={i} className={styles.llmResultList}>
              {lines.map((l, j) => (
                // Strip the leading `- ` / `* ` marker — the <li>
                // bullet supplies the visual marker. Keep it simple:
                // no inline-formatting parsing.
                <li key={j}>{l.slice(2)}</li>
              ))}
            </ul>
          );
        }
        return (
          <p key={i} className={styles.llmResultPara}>
            {block}
          </p>
        );
      })}
    </div>
  );
}

function LlmPassCard({ sessionId }: { sessionId: number }) {
  const [prompt, setPrompt] = useState<DictLlmPrompt>("summary");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<{ text: string; latencyMs: number } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number | null>(null);

  // Clear local state when the user navigates to a different
  // session. The LLM pass is per-session; carrying over a previous
  // result into a new session would be misleading.
  useEffect(() => {
    setResult(null);
    setError(null);
    setRunning(false);
    setCopied(false);
  }, [sessionId]);

  useEffect(() => {
    return () => {
      if (copyTimer.current) window.clearTimeout(copyTimer.current);
    };
  }, []);

  async function runPass() {
    setRunning(true);
    setError(null);
    try {
      const r = await api.dictation_run_llm_pass(sessionId, prompt);
      setResult({ text: r.text, latencyMs: r.latencyMs });
    } catch (e) {
      // Surface the backend error message verbatim — it's already a
      // human-readable string from `commands::into_err`.
      setError(String(e));
    } finally {
      setRunning(false);
    }
  }

  async function copyOutput() {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.text);
    } catch {
      // Fall back silently — the user can still select + copy by
      // hand. Not worth a toast for a 1-in-1000 permission edge.
      return;
    }
    setCopied(true);
    if (copyTimer.current) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopied(false), 1200);
  }

  return (
    <Card title={t("dictations.llm.title")}>
      <div className={styles.llmControls}>
        <label className={styles.llmPromptLabel}>
          {t("dictations.llm.prompt.label")}
          <select
            className={styles.llmPromptSelect}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value as DictLlmPrompt)}
            disabled={running}
          >
            {LLM_PROMPT_OPTIONS.map((opt) => (
              <option key={opt.id} value={opt.id}>
                {t(opt.labelKey)}
              </option>
            ))}
          </select>
        </label>
        <LlmRunButton
          onClick={runPass}
          running={running}
          idleLabel={t("dictations.llm.run")}
          runningLabel={t("dictations.llm.running")}
        />
      </div>

      {error ? (
        <div className={styles.llmError} role="alert">
          {t("dictations.llm.error").replace("{message}", error)}
        </div>
      ) : null}

      {result ? (
        <div className={styles.llmResult}>
          <div className={styles.llmResultMeta}>
            <span>
              {t("dictations.llm.latency").replace(
                "{ms}",
                String(result.latencyMs),
              )}
            </span>
            <Button onClick={copyOutput} ariaLabel={t("dictations.llm.copy")}>
              {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
              {copied ? t("dictations.llm.copied") : t("dictations.llm.copy")}
            </Button>
          </div>
          <LlmMarkdownView text={result.text} />
        </div>
      ) : !running && !error ? (
        <p className={styles.llmHelp}>{t("dictations.llm.notRun")}</p>
      ) : null}
    </Card>
  );
}

function Stage({
  label,
  text,
  variant,
}: {
  label: string;
  text: string;
  variant: "raw" | "cleaned" | "final";
}) {
  if (!text) return null;
  const cls =
    variant === "raw"
      ? `${styles.stageText} ${styles.raw}`
      : variant === "final"
        ? `${styles.stageText} ${styles.final}`
        : styles.stageText;
  return (
    <div className={styles.stage}>
      <div className={styles.stageLabel}>
        <span className={styles.stageLabelText}>{label}</span>
      </div>
      <pre className={cls}>{text}</pre>
    </div>
  );
}
