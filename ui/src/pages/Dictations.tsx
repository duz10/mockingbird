// Dictations page -- list + detail in a two-pane layout. Shows the
// history of past dictation sessions (Right-Alt push-to-talk).
// Sibling to the Meetings page; *not* the same thing as meeting
// recordings.
//
// **Renamed from "History" 2026-05-21.** Internal Rust event names
// (`history:session-saved`) are left alone -- sealed Phase 4 code,
// and the wire-name has no user impact. The i18n keys, route, and
// component names all moved to `dictations.*` / `/dictations` /
// `DictationsPage` so the new naming is consistent from the source
// down.
//
// Why no virtual scrolling: the session list is bounded (we'll add
// a "Load more" affordance when the user hits the bottom). Real
// users hit 40-200 sessions per week; a virtualized list would be
// a YAGNI dep until someone shows up with 50k rows. If/when that
// happens, promote the .list scroll container to react-window
// without changing any other prop.
//
// Search uses the FTS5 endpoint when the query is non-empty
// (debounced 200 ms). Empty query => normal `list_sessions` paged
// from offset 0. Selecting a hit jumps to that session's detail
// view.
//
// **Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) -- KG surface
// removed.** Phase 1C Wave 1C.3/1C.4 grew this page a Knowledge
// Graph filter bar, per-row chip strip, and concept-modal entry
// points. With the dedicated `/knowledge-graph` route now first-
// class, the entire KG retrieval UX lives there
// (`ui/src/routes/knowledge-graph/Retrieval.tsx` + the dashboard
// Flagged-for-review band). This page is back to its pre-1C shape:
// history list + FTS search + detail pane. The
// `kg-graph-off-ui-untouched` invariant is now trivially honored
// here at the page level -- this file has zero `kg_*` IPC calls,
// zero KG-aware state, and zero KG component imports.

import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import {
  Button,
  EmptyState,
  PageHeader,
  Spinner,
} from "../components/primitives";
import { CleanupPassthroughBanner } from "../components/CleanupStatus";
import { DictationRecordButton } from "./DictationRecordButton";
import { HistoryIcon, PlusIcon, SearchIcon } from "../design/Icon";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api, isTauri } from "../lib/tauri";
import type {
  SessionDetail,
  SessionSummary,
  TranscriptSearchHit,
} from "../lib/types";

import { DictationsDetailPane } from "./DictationsDetailPane";
import { SearchHitsList, SessionList } from "./DictationsList";
import styles from "./Dictations.module.css";

const PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 200;

export function DictationsPage() {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [searchHits, setSearchHits] = useState<TranscriptSearchHit[] | null>(
    null,
  );
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [isImporting, setIsImporting] = useState(false);

  const setSelectedSession = useAppStore((s) => s.setSelectedSession);
  const isMac = useAppStore((s) => s.isMac);
  const navigate = useNavigate();

  // Initial list fetch.
  useEffect(() => {
    void (async () => {
      const list = await api.list_sessions(PAGE_SIZE, 0);
      setSessions(list);
      if (list[0] && selectedId === null) setSelectedId(list[0].id);
    })();
    // selectedId intentionally omitted -- we only auto-pick on
    // first load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Live-refresh: the Rust orchestrator emits
  // `history:session-saved` after each new row (Complete or Error)
  // is committed to the DB. We refetch the list silently --
  // preserving the user's current `selectedId` so they aren't
  // yanked away from an old session they're reading. The new row
  // simply appears at the top.
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
          if (cancelled) return;
          void (async () => {
            const list = await api.list_sessions(PAGE_SIZE, 0);
            if (cancelled) return;
            setSessions(list);
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

  // Debounced FTS5 search.
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

  // Action handlers -- close over the current detail. They
  // re-resolve session list after destructive ops so the UI stays
  // consistent.
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
    await navigator.clipboard.writeText(
      detail.final || detail.cleaned || detail.raw,
    );
    // ADR 0047 §Wave 2.5 / mb-v2fa -- manual Copy is an
    // edit-equivalent action.
    void api
      .dictation_mark_edit_observed(detail.session.id)
      .catch(() => {
        // Silent: a metric write failing must not block the copy.
      });
    showToast(t("dictations.copied"));
  }, [detail, showToast]);

  // ADR 0046 §3.2 / mb-7vyz -- desktop audio-file import.
  const handleImport = useCallback(async () => {
    setIsImporting(true);
    try {
      const summary = await api.dictation_import_file();
      const list = await api.list_sessions(PAGE_SIZE, 0);
      setSessions(list);
      setSelectedId(summary.sessionId);
      const preview = summary.transcriptPreview.trim();
      showToast(
        preview.length > 0 ? `Imported: ${preview}` : "Imported audio file",
      );
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg !== "cancelled") {
        showToast(`Import failed: ${msg}`);
      }
    } finally {
      setIsImporting(false);
    }
  }, [showToast]);

  // Decide what to render in the list pane. Mirrors the pre-1C
  // priority order: spinner during initial load, empty-state when
  // there are no sessions at all, search-hits view when the query
  // is non-empty, otherwise the canonical session list.
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
      <PageHeader
        title={t("dictations.title")}
        actions={
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void handleImport()}
            disabled={isImporting}
            ariaLabel="Import audio file"
            title="Import an audio file as a dictation."
          >
            <PlusIcon size={14} />
            {isImporting ? "Importing…" : "Audio file"}
          </Button>
        }
      />

      <div className={styles.shell}>
        <div className={styles.leftPane}>
          {/* ADR 0045: programmatic start/stop, sibling to Right-Alt
              PTT. Self-contained -- owns its own dictation:state
              subscription. */}
          <DictationRecordButton />

          {/* macOS-only: nag once (dismissible) when the CURRENT cleanup
              state is passthrough so raw dictations aren't a mystery.
              isMac-gated → Windows byte-identical. */}
          {isMac ? (
            <CleanupPassthroughBanner
              onSetup={() => navigate("/settings", { state: { tab: "models" } })}
            />
          ) : null}

          <SearchInput value={query} onChange={setQuery} />
          {leftPaneContent}
        </div>

        <div className={styles.rightPane}>
          {detail ? (
            <DictationsDetailPane
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
/* Search input                                                       */
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
