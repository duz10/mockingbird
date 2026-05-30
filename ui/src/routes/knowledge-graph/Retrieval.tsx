// Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) -- Retrieval band on
// the Knowledge Graph dashboard.
//
// Origin: this is the relocated home for the filter chips + per-row
// entity/tag/filing-state strip that Phase 1C Wave 1C.3 (`mb-5ly5`)
// shipped on the Dictations page. With the KG screen now first-
// class (Wave 1D.2) and the KG-specific capture surface alongside
// (Wave 1D.3), KG retrieval belongs here too. The Dictations page
// goes back to the pre-1C shape (history list + FTS); cross-surface
// concerns about "where's the chips?" are answered by the same
// answer "wherever the KG screen is".
//
// What this band does
// -------------------
// Owns:
//   - Filter state (entity + tag multi-select; the free-text axis
//     stays on Dictations' FTS box because the KG screen has no
//     equivalent transcript search and the chip-typeahead covers
//     concept-name search natively).
//   - The `kg_search_entries(filter)` resolution into a visible-id
//     set, debounced 200 ms.
//   - The batched `kg_entries_summary(visibleIds)` fetch that feeds
//     per-row `EntryChips`.
// Receives (from the dashboard parent):
//   - `onConceptOpen` -- called when a chip is clicked. The
//     dashboard owns the single `ConceptModal` instance, so chip
//     clicks bubble up rather than each band mounting its own.
//
// What this band does NOT do
// --------------------------
// Row clicks are visually selectable (keyboard focus, row
// highlight) but do NOT navigate anywhere this wave. The
// historical "click a row to open it in the detail pane" only
// made sense on Dictations because that page has a right-hand
// detail pane; the KG screen does not. Wiring a row click to a
// `/dictations#session=<id>` deep link is a worthwhile follow-up
// (filed as a separate bead) but isn't in 1D.4's scope -- the
// kickoff explicitly limits this wave to "no UI logic change,
// just relocation".

import { useCallback, useEffect, useMemo, useState } from "react";
import { toast as sonnerToast } from "sonner";

import { Button, Card, EmptyState, Spinner } from "../../components/primitives";
import { SearchIcon } from "../../design/Icon";
import { t } from "../../i18n";
import { api } from "../../lib/tauri";
import type {
  ActiveConcept,
  EntrySummary,
  SearchFilter,
  SessionSummary,
} from "../../lib/types";

import { EntryChips } from "./EntryChips";
import { KgFilterBar, type SelectedEntity } from "./FilterBar";
import styles from "./Retrieval.module.css";

const PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 200;

/** Map of session id -> KG summary, returned in one batched call
 *  by `kgEntriesSummary`. Keys are stringified because JSON object
 *  keys can't be numbers on the wire. */
type KgSummaryMap = Record<string, EntrySummary>;

interface Props {
  /** Bubbled up from chip clicks; the dashboard owns the single
   *  ConceptModal instance. */
  onConceptOpen: (concept: ActiveConcept) => void;
}

export function Retrieval({ onConceptOpen }: Props) {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [filterEntities, setFilterEntities] = useState<SelectedEntity[]>([]);
  const [filterTags, setFilterTags] = useState<string[]>([]);
  // `null` = filter inactive (use base list); Set (possibly empty)
  // = filter active.
  const [kgMatchIds, setKgMatchIds] = useState<Set<number> | null>(null);
  const [kgSummaries, setKgSummaries] = useState<KgSummaryMap>({});
  // Selection is visual-only this wave (see file header note). We
  // keep it so keyboard users still get focus/active row feedback.
  const [selectedId, setSelectedId] = useState<number | null>(null);

  // Initial base-list fetch. Same `list_sessions` IPC the Dictations
  // page uses -- the visible set is `sessions ∩ kgMatchIds` when a
  // filter is active. We don't pull from `kg_dashboard_snapshot`
  // here because that IPC caps at the recent-activity slice; the
  // retrieval band needs the full page-1 list to intersect against.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const list = await api.list_sessions(PAGE_SIZE, 0);
      if (!cancelled) setSessions(list);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // ── KG filter resolution ──────────────────────────────────────
  //
  // Build the wire filter from local state. When all axes are
  // empty we set `kgMatchIds` to null and short-circuit -- saves
  // a no-op IPC and matches the server's `SearchFilter::is_empty()`
  // semantics.
  const filterIsActive = filterEntities.length > 0 || filterTags.length > 0;

  useEffect(() => {
    if (!filterIsActive) {
      setKgMatchIds(null);
      return;
    }
    const filter: SearchFilter = {
      entities: filterEntities.map((e) => e.entityId),
      tags: filterTags,
    };
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const ids = await api.kg_search_entries(filter);
          setKgMatchIds(new Set(ids));
        } catch (err) {
          // Surface as a sonner toast (reuse the app-wide Toaster
          // mounted in main.tsx). Treat the failure as "empty
          // match set" so the empty-state CTA gives the user a
          // way out.
          setKgMatchIds(new Set());
          sonnerToast.error(
            t("kg.filter.loadError").replace("{error}", String(err)),
          );
        }
      })();
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [filterIsActive, filterEntities, filterTags]);

  const visibleSessions = useMemo(() => {
    if (!sessions) return null;
    if (kgMatchIds !== null) {
      return sessions.filter((s) => kgMatchIds.has(s.id));
    }
    return sessions;
  }, [sessions, kgMatchIds]);

  // Batched per-row chip + filing-state fetch. Single IPC for the
  // entire visible list.
  useEffect(() => {
    if (!visibleSessions) {
      setKgSummaries({});
      return;
    }
    const ids = visibleSessions.map((s) => s.id);
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
  }, [visibleSessions]);

  const clearAllFilters = useCallback(() => {
    setFilterEntities([]);
    setFilterTags([]);
  }, []);

  return (
    <Card title={t("kg.retrieval.heading")}>
      <KgFilterBar
        entities={filterEntities}
        tags={filterTags}
        onEntitiesChange={setFilterEntities}
        onTagsChange={setFilterTags}
        onClearAll={clearAllFilters}
      />
      <RetrievalList
        visibleSessions={visibleSessions}
        sessions={sessions}
        filterIsActive={filterIsActive}
        kgSummaries={kgSummaries}
        selectedId={selectedId}
        onSelect={setSelectedId}
        onConceptOpen={onConceptOpen}
        clearAllFilters={clearAllFilters}
      />
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* List sub-component                                                 */
/* ------------------------------------------------------------------ */

interface RetrievalListProps {
  visibleSessions: SessionSummary[] | null;
  sessions: SessionSummary[] | null;
  filterIsActive: boolean;
  kgSummaries: KgSummaryMap;
  selectedId: number | null;
  onSelect: (id: number) => void;
  onConceptOpen: (concept: ActiveConcept) => void;
  clearAllFilters: () => void;
}

function RetrievalList({
  visibleSessions,
  sessions,
  filterIsActive,
  kgSummaries,
  selectedId,
  onSelect,
  onConceptOpen,
  clearAllFilters,
}: RetrievalListProps) {
  if (visibleSessions === null) return <Spinner />;
  if (sessions !== null && sessions.length === 0) {
    // No sessions filed yet at all. Distinct copy from the
    // filter-active empty state below.
    return (
      <EmptyState
        icon={<SearchIcon size={28} />}
        title={t("kg.retrieval.empty.title")}
        subtitle={t("kg.retrieval.empty.subtitle")}
      />
    );
  }
  if (filterIsActive && visibleSessions.length === 0) {
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
  if (!filterIsActive) {
    // No filter active -- the band's call-to-action is the chips
    // themselves; the full base list is on the Dictations page.
    // Showing 50 unfiltered rows here would just duplicate the
    // recent-activity band below.
    return (
      <p className={styles.idle} role="status">
        {t("kg.retrieval.idle")}
      </p>
    );
  }
  return (
    <div className={styles.list} role="listbox" aria-label={t("kg.retrieval.heading")}>
      {visibleSessions.map((s) => (
        <RetrievalRow
          key={s.id}
          session={s}
          active={s.id === selectedId}
          onClick={() => onSelect(s.id)}
          summary={kgSummaries[String(s.id)]}
          onConceptOpen={onConceptOpen}
        />
      ))}
    </div>
  );
}

function RetrievalRow({
  session,
  active,
  onClick,
  summary,
  onConceptOpen,
}: {
  session: SessionSummary;
  active: boolean;
  onClick: () => void;
  summary?: EntrySummary;
  onConceptOpen: (concept: ActiveConcept) => void;
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
        <span className={styles.rowTitle}>
          {truncatePreview(session.finalText) || `#${session.id}`}
        </span>
      </div>
      <EntryChips summary={summary} onConceptOpen={onConceptOpen} />
    </div>
  );
}

// Tiny inline preview helper. We don't pull in `truncate` from
// lib/format because we want a more aggressive cap here (the rows
// are denser than on Dictations -- there's no second column).
function truncatePreview(text: string): string {
  const MAX = 140;
  if (text.length <= MAX) return text;
  return text.slice(0, MAX - 1) + "\u2026";
}
