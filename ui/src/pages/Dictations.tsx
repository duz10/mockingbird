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
// Phase 1C Wave 1C.3 / ADR 0051 (`mb-5ly5`) -- Knowledge Graph
// retrieval surfaces. When `KgGraphEnabled === true`:
//   * `DictationsFilterBar` mounts above the search input with
//     entity + tag multi-select chips.
//   * Per-row `DictationKgChips` strip renders under each session
//     row in the list (top-5 entity chips + top-5 tag chips + a
//     filing-state pill when not silent).
//   * When any chip is selected, the visible session list is
//     intersected with `kgSearchEntries(filter)`. The free-text
//     search box ALSO threads into the filter's `query` field
//     (UNION-style: result list = entity/tag-name hits OR the
//     existing FTS hits) per Wave brief D6.
//   * Per-row chip + filing-state data is fetched in ONE batched
//     `kgEntriesSummary(visibleIds)` call (per-row firing is a
//     stop condition).
// When `KgGraphEnabled === false`, every KG surface stays hidden
// -- behavior is identical to the pre-1C Dictations page, which
// is the `kg-graph-off-ui-untouched` invariant pinned for the
// 1C.5 judge bundle.
//
// Sub-component layout:
//   * `DictationsFilterBar`        -- KG retrieval filter chips
//   * `DictationsList` /
//     `SessionList`+`SearchHitsList` -- list-pane rendering
//   * `DictationsLlmPassCard`      -- on-demand LLM pass card

import { useCallback, useEffect, useMemo, useState } from "react";
import { toast as sonnerToast } from "sonner";

import {
  Button,
  EmptyState,
  PageHeader,
  Spinner,
} from "../components/primitives";
import { DictationRecordButton } from "./DictationRecordButton";
import { HistoryIcon, PlusIcon, SearchIcon } from "../design/Icon";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api, isTauri } from "../lib/tauri";
import type {
  SearchFilter,
  SessionDetail,
  SessionSummary,
  TranscriptSearchHit,
} from "../lib/types";

import { DictationsDetailPane } from "./DictationsDetailPane";
import {
  DictationsFilterBar,
  type SelectedEntity,
} from "./DictationsFilterBar";
import {
  SearchHitsList,
  SessionList,
  type KgSummaryMap,
} from "./DictationsList";
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

  // ── Phase 1C Wave 1C.3 KG state ────────────────────────────────
  //
  // The toggle is read once on mount. The Wave 1C.1 worker promotion
  // means a flip in Settings takes effect within ~5s for the
  // BACKEND, but the UI surface is decided at mount and stays
  // stable for the lifetime of the Dictations page (it'd be
  // jarring to have the filter bar appear / disappear under the
  // user mid-session). A future wave can promote to a live store
  // subscription if real users hit the seam.
  const [kgEnabled, setKgEnabled] = useState(false);
  const [filterEntities, setFilterEntities] = useState<SelectedEntity[]>([]);
  const [filterTags, setFilterTags] = useState<string[]>([]);
  // Result of `kg_search_entries(filter)`. `null` = no filter
  // active (use the base list); array (possibly empty) = filter
  // active.
  const [kgMatchIds, setKgMatchIds] = useState<Set<number> | null>(null);
  const [kgSummaries, setKgSummaries] = useState<KgSummaryMap>({});
  // Refresh nonce for the summary effect -- bumped on tab focus
  // (D8 refresh strategy: no polling, no Tauri events).
  const [summaryNonce, setSummaryNonce] = useState(0);

  const setSelectedSession = useAppStore((s) => s.setSelectedSession);

  // Initial KG-enabled fetch. Independent of the list fetch so a
  // slow KG settings read can't block the page render.
  useEffect(() => {
    void (async () => {
      try {
        const s = await api.kg_settings_get_all();
        setKgEnabled(s.kgGraphEnabled);
      } catch {
        // Silent fall-through: if the IPC fails we treat KG as
        // disabled. Matches the `kg-graph-off-ui-untouched`
        // invariant default.
        setKgEnabled(false);
      }
    })();
  }, []);

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

  // Debounced FTS5 search. Unchanged from pre-1C behaviour.
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

  // ── KG filter resolution ──────────────────────────────────────
  //
  // Build the wire filter from local state. When KG is off OR all
  // axes are empty (no chips AND no query) we set `kgMatchIds` to
  // null and short-circuit -- saves a no-op IPC and matches the
  // server's `SearchFilter::is_empty()` semantics.
  //
  // D6 (search-box extension): when KG is ON, the existing query
  // string ALSO goes into the filter's `query` axis. The result
  // list is therefore "entity/tag-name hits OR FTS hits" -- the
  // server-side filter contributes one set; the FTS path
  // contributes the other; the final visible list is the union
  // computed below in `leftPaneContent`.
  const filterIsActive =
    kgEnabled &&
    (filterEntities.length > 0 ||
      filterTags.length > 0 ||
      query.trim().length > 0);

  useEffect(() => {
    if (!filterIsActive) {
      setKgMatchIds(null);
      return;
    }
    const filter: SearchFilter = {
      entities: filterEntities.map((e) => e.entityId),
      tags: filterTags,
      query: query.trim().length > 0 ? query.trim() : undefined,
    };
    // 200 ms debounce -- chip-add fires synchronously but a fast
    // typist hitting the search box benefits from the same window
    // the FTS path uses.
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const ids = await api.kg_search_entries(filter);
          setKgMatchIds(new Set(ids));
        } catch (err) {
          // Surface as a sonner toast (per kickoff brief: reuse the
          // app-wide Toaster mounted in main.tsx instead of
          // reinventing inline state). Treat the failure as
          // "empty match set" so the empty-state CTA gives the
          // user a way out.
          setKgMatchIds(new Set());
          sonnerToast.error(
            t("kg.filter.loadError").replace("{error}", String(err)),
          );
        }
      })();
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [
    filterIsActive,
    filterEntities,
    filterTags,
    query,
  ]);

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

  // ── Visible-id resolution & batched summary fetch ─────────────
  //
  // The visible set is whatever the list pane will render below.
  // We compute it here (once) and feed both into the rendered
  // sublist AND into the summary effect.
  //
  // When KG is OFF: identical to pre-1C -- whichever of
  // `searchHits` / `sessions` is the active source. `visibleSessions`
  // / `visibleHits` are derived references; the rendering path
  // below switches on `searchHits` non-null exactly as before.
  //
  // When KG is ON + filter active: intersect the active source
  // with `kgMatchIds`. When KG is ON but filter empty: identical
  // to the off path (no IPC fired).
  const visibleSessions = useMemo(() => {
    if (!sessions) return null;
    if (kgEnabled && kgMatchIds !== null) {
      return sessions.filter((s) => kgMatchIds.has(s.id));
    }
    return sessions;
  }, [sessions, kgEnabled, kgMatchIds]);

  const visibleHits = useMemo(() => {
    if (!searchHits) return null;
    if (kgEnabled && kgMatchIds !== null) {
      return searchHits.filter((h) => kgMatchIds.has(h.sessionId));
    }
    return searchHits;
  }, [searchHits, kgEnabled, kgMatchIds]);

  // Batched per-row chip + filing-state fetch. Single IPC for the
  // entire visible list -- per-row firing is the kickoff stop
  // condition. Refreshes on filter change (implicit via deps) and
  // on tab focus (`summaryNonce` bump below).
  useEffect(() => {
    if (!kgEnabled) {
      setKgSummaries({});
      return;
    }
    const ids = visibleHits
      ? Array.from(new Set(visibleHits.map((h) => h.sessionId)))
      : visibleSessions
        ? visibleSessions.map((s) => s.id)
        : [];
    if (ids.length === 0) {
      setKgSummaries({});
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const map = await api.kg_entries_summary(ids);
        if (!cancelled) setKgSummaries(map);
      } catch {
        if (!cancelled) setKgSummaries({});
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [kgEnabled, visibleSessions, visibleHits, summaryNonce]);

  // D8 refresh-on-focus. No polling, no Tauri event subscription
  // this wave -- the wave brief explicitly forbids both. We bump
  // the summary nonce so the batched-fetch effect re-runs with
  // the same id set.
  useEffect(() => {
    if (!kgEnabled) return;
    const onFocus = () => setSummaryNonce((n) => n + 1);
    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        setSummaryNonce((n) => n + 1);
      }
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [kgEnabled]);

  // Clear-all helper for the FilterBar's "Clear filters" button.
  // Resets every axis (chips + query) so the empty-state CTA
  // brings the user back to the base list in one click.
  const clearAllFilters = useCallback(() => {
    setFilterEntities([]);
    setFilterTags([]);
    setQuery("");
  }, []);

  // Decide what to render in the list pane. Mirrors the pre-1C
  // priority order with the new filter-active empty state taking
  // precedence over the FTS-search "No results" empty state.
  const leftPaneContent = useMemo(() => {
    if (visibleSessions === null) return <Spinner />;
    if (sessions !== null && sessions.length === 0) {
      // True base-state empty: no sessions exist at all. The
      // historical empty state (not a filter-driven one).
      return (
        <EmptyState
          icon={<HistoryIcon size={28} />}
          title={t("dictations.empty.title")}
          subtitle={t("dictations.empty.subtitle")}
        />
      );
    }
    // Filter-active empty state. Distinct from the "no sessions"
    // state above so the CTA differs ("Clear filters" vs "press
    // Right Alt to dictate").
    const anyActive =
      kgEnabled &&
      (filterEntities.length > 0 || filterTags.length > 0);
    const filterActiveNoMatches =
      anyActive &&
      ((visibleHits !== null && visibleHits.length === 0) ||
        (visibleHits === null && visibleSessions.length === 0));
    if (filterActiveNoMatches) {
      return (
        <EmptyState
          icon={<SearchIcon size={28} />}
          title={t("kg.filter.empty.title")}
          subtitle={t("kg.filter.empty.subtitle")}
          action={
            <Button
              variant="ghost"
              size="sm"
              onClick={clearAllFilters}
              ariaLabel={t("kg.filter.clear")}
            >
              {t("kg.filter.clear")}
            </Button>
          }
        />
      );
    }
    if (visibleHits !== null) {
      return (
        <SearchHitsList
          hits={visibleHits}
          selectedId={selectedId}
          onSelect={setSelectedId}
          kgSummaries={kgEnabled ? kgSummaries : undefined}
        />
      );
    }
    return (
      <SessionList
        sessions={visibleSessions}
        selectedId={selectedId}
        onSelect={setSelectedId}
        kgSummaries={kgEnabled ? kgSummaries : undefined}
      />
    );
  }, [
    sessions,
    visibleSessions,
    visibleHits,
    selectedId,
    kgEnabled,
    kgSummaries,
    filterEntities.length,
    filterTags.length,
    clearAllFilters,
  ]);

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

          {/* Phase 1C Wave 1C.3 -- KG filter bar. Gated on the
              activation toggle (kg-graph-off-ui-untouched invariant). */}
          {kgEnabled ? (
            <DictationsFilterBar
              entities={filterEntities}
              tags={filterTags}
              onEntitiesChange={setFilterEntities}
              onTagsChange={setFilterTags}
              onClearAll={clearAllFilters}
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
              kgSummary={
                kgEnabled ? kgSummaries[String(detail.session.id)] : undefined
              }
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


