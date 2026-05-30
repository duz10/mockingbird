// Session list + search-hits list for the Dictations page.
//
// Extracted from Dictations.tsx as part of Phase 1C Wave 1C.3
// (`mb-5ly5`) so the parent gets back under the 600-LoC ceiling
// AND so the per-row KG chip strip (`DictationKgChips`) has a
// natural mount point that doesn't bloat the page-level component.
//
// Behavior is byte-for-byte identical to the inline version
// through Wave 1C.2 -- this is a pure relocation -- with one
// additive concession to 1C.3: an optional `kgSummaries` map that,
// when present, renders the KG strip under each row.
//
// The parent decides whether to pass `kgSummaries` (only when
// `kgGraphEnabled === true` -- preserves the
// `kg-graph-off-ui-untouched` invariant pinned for Wave 1C.5's
// judge bundle).

import { EmptyState, Pill } from "../components/primitives";
import { SearchIcon } from "../design/Icon";
import { t } from "../i18n";
import {
  formatDuration,
  formatTimestamp,
  prettyAppName,
  truncate,
} from "../lib/format";
import type {
  ActiveConcept,
  EntrySummary,
  SessionSummary,
  TranscriptSearchHit,
} from "../lib/types";

import { DictationKgChips } from "./DictationKgChips";
import styles from "./Dictations.module.css";

/** Map of session id -> KG summary, returned in one batched call
 *  by `kgEntriesSummary`. Keys are stringified because JSON object
 *  keys can't be numbers on the wire. */
export type KgSummaryMap = Record<string, EntrySummary>;

/* ADR 0045 + mb-tfyp -- list-pill semantics.
 *
 * Priority order (highest wins):
 *   1. `startMode === 'in_app'` -> render `IN_APP` (neutral,
 *      glass-faint). In-app sessions don't have a target app, so
 *      the legacy abort heuristic doesn't apply and the row should
 *      never wear the red `ABORTED_FOCUS_CHANGED` pill -- even if
 *      the underlying injection_status string still happens to read
 *      "aborted" on legacy rows. The semantic is "captured a
 *      transcript, no injection by design."
 *   2. `injectionStatus === 'ok'` -> no pill (the happy path is
 *      silent).
 *   3. anything else -> red error pill with the verbatim status.
 */
function renderStatusPill(session: SessionSummary) {
  if (session.startMode === "in_app") {
    return (
      <>
        <span>·</span>
        <Pill tone="status-info">IN_APP</Pill>
      </>
    );
  }
  if (session.injectionStatus === "ok") return null;
  return (
    <>
      <span>·</span>
      <Pill tone="status-error">{session.injectionStatus}</Pill>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* SessionList -- canonical "all sessions" view.                      */
/* ------------------------------------------------------------------ */

export function SessionList({
  sessions,
  selectedId,
  onSelect,
  kgSummaries,
  onConceptOpen,
}: {
  sessions: SessionSummary[];
  selectedId: number | null;
  onSelect: (id: number) => void;
  /** Phase 1C Wave 1C.3 -- when present, render the KG chip strip
   *  under each row. The parent gates this on
   *  `kgGraphEnabled === true`. */
  kgSummaries?: KgSummaryMap;
  /** Phase 1C Wave 1C.4 -- forwarded into each row's chip strip.
   *  Optional; when omitted, chips render as inert spans (the
   *  Wave 1C.3 behaviour). */
  onConceptOpen?: (concept: ActiveConcept) => void;
}) {
  return (
    <div className={styles.list} role="listbox" aria-label="Sessions">
      {sessions.map((s) => (
        <SessionRow
          key={s.id}
          session={s}
          active={s.id === selectedId}
          onClick={() => onSelect(s.id)}
          kgSummary={kgSummaries?.[String(s.id)]}
          onConceptOpen={onConceptOpen}
        />
      ))}
    </div>
  );
}

function SessionRow({
  session,
  active,
  onClick,
  kgSummary,
  onConceptOpen,
}: {
  session: SessionSummary;
  active: boolean;
  onClick: () => void;
  kgSummary?: EntrySummary;
  onConceptOpen?: (concept: ActiveConcept) => void;
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
        {renderStatusPill(session)}
      </div>
      {/* DictationKgChips is null-returning when summary missing,
          empty, or filing-state silent (done/not_enqueued + no
          chips), so the row layout is unchanged when KG is off
          OR when the row has no KG content. */}
      <DictationKgChips summary={kgSummary} onConceptOpen={onConceptOpen} />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* SearchHitsList -- FTS5 view, swapped in when the query is set.     */
/* ------------------------------------------------------------------ */

export function SearchHitsList({
  hits,
  selectedId,
  onSelect,
  kgSummaries,
  onConceptOpen,
}: {
  hits: TranscriptSearchHit[];
  selectedId: number | null;
  onSelect: (id: number) => void;
  kgSummaries?: KgSummaryMap;
  onConceptOpen?: (concept: ActiveConcept) => void;
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
              it -- it comes from our own SQL `snippet()` call
              wrapping `<mark>` around the match). Safe to render. */}
          <div
            className={styles.rowText}
            dangerouslySetInnerHTML={{ __html: h.snippet }}
          />
          <div className={styles.rowMeta}>
            <span>stage: {h.stage}</span>
          </div>
          <DictationKgChips
            summary={kgSummaries?.[String(h.sessionId)]}
            onConceptOpen={onConceptOpen}
          />
        </div>
      ))}
    </div>
  );
}
