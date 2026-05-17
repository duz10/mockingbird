// History page — list + detail in a two-pane layout.
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
import { api } from "../lib/tauri";
import type { SessionDetail, SessionSummary, TranscriptSearchHit } from "../lib/types";

import styles from "./History.module.css";

const PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 200;

export function HistoryPage() {
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
    if (!window.confirm(t("history.action.delete") + "?")) return;
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
    showToast(t("history.copied"));
  }, [detail, showToast]);

  // Decide what to render in the list pane.
  const leftPaneContent = useMemo(() => {
    if (sessions === null) return <Spinner />;
    if (sessions.length === 0) {
      return (
        <EmptyState
          icon={<HistoryIcon size={28} />}
          title={t("history.empty.title")}
          subtitle={t("history.empty.subtitle")}
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
      <PageHeader title={t("history.title")} />

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
        placeholder={t("history.search.placeholder")}
        className={styles.searchInput}
        aria-label={t("history.search.placeholder")}
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
        title={t("history.search.noResults")}
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
          <Button onClick={fireCopy} ariaLabel={t("history.action.copyFinal")}>
            {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
            {copied ? "Copied" : t("history.action.copyFinal")}
          </Button>
          <Button onClick={onMarkExample}>
            <PlusIcon size={14} />
            {t("history.action.markExample")}
          </Button>
          <Button variant="danger" onClick={onDelete}>
            <TrashIcon size={14} />
            {t("history.action.delete")}
          </Button>
        </div>
      </div>

      <Card title={t("history.detail.metadata")}>
        <div className={styles.metaGrid}>
          <span className={styles.metaKey}>{t("history.detail.mode")}</span>
          <span className={styles.metaVal}>
            <Pill tone={`mode-${s.modeSlug}`}>{s.modeSlug}</Pill>
          </span>
          <span className={styles.metaKey}>{t("history.detail.model")}</span>
          <span className={styles.metaVal}>{detail.modelUsed ?? "—"}</span>
          <span className={styles.metaKey}>
            {t("history.detail.promptVersion")}
          </span>
          <span className={styles.metaVal}>{detail.promptVersion ?? "—"}</span>
          <span className={styles.metaKey}>
            {t("history.detail.dictVersion")}
          </span>
          <span className={styles.metaVal}>
            {detail.dictionaryVersion ?? "—"}
          </span>
          <span className={styles.metaKey}>{t("history.detail.app")}</span>
          <span className={styles.metaVal}>
            {prettyAppName(s.foregroundApp)} —{" "}
            {s.foregroundWindowTitle ?? "—"}
          </span>
          <span className={styles.metaKey}>{t("history.detail.latency")}</span>
          <span className={styles.metaVal}>
            STT {Math.round(detail.latency.sttMs ?? 0)} ms · Clean{" "}
            {Math.round(detail.latency.cleanupMs ?? 0)} ms · Inject{" "}
            {Math.round(detail.latency.injectMs ?? 0)} ms · Total{" "}
            {Math.round(latencyTotal)} ms
          </span>
        </div>
      </Card>

      <Card>
        <Stage label={t("history.detail.raw")} text={detail.raw} variant="raw" />
        <Stage
          label={t("history.detail.cleaned")}
          text={detail.cleaned}
          variant="cleaned"
        />
        <Stage
          label={t("history.detail.final")}
          text={detail.final}
          variant="final"
        />
      </Card>
    </>
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
