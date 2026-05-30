// Session list + search-hits list for the Dictations page.
//
// Extracted from Dictations.tsx in Phase 1C Wave 1C.3 (`mb-5ly5`)
// when the page grew the KG retrieval surface and the parent file
// pushed past the 600-LoC reviewability ceiling. Phase 1D Wave
// 1D.4 (`mb-6hm2`, ADR 0052 D5) **subtracts** the KG plumbing that
// briefly lived here (the `kgSummaries` map + `onConceptOpen`
// forwarders + per-row `DictationKgChips` mount). The KG chips and
// concept modal now live on the KG screen
// (`ui/src/routes/knowledge-graph/`); this file is back to the
// pre-1C shape -- a dumb list of session rows.
//
// Kept as a sibling component (rather than re-inlining into
// Dictations.tsx) because the file still earns its keep on
// reviewability grounds and we'll likely want to mount the same
// list on a future "All captures" surface or similar without
// re-extracting it. YAGNI says don't anticipate; cohesion says
// don't undo a clean extraction just because it temporarily got
// thinner.

import { EmptyState, Pill } from "../components/primitives";
import { SearchIcon } from "../design/Icon";
import { t } from "../i18n";
import {
  formatDuration,
  formatTimestamp,
  prettyAppName,
  truncate,
} from "../lib/format";
import type { SessionSummary, TranscriptSearchHit } from "../lib/types";

import styles from "./Dictations.module.css";

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
        {renderStatusPill(session)}
      </div>
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
              it -- it comes from our own SQL `snippet()` call
              wrapping `<mark>` around the match). Safe to render. */}
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
