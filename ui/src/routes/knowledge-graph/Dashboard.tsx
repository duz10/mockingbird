// Phase 1D Wave 1D.2 (`mb-j00j`, ADR 0052) -- read-only Knowledge
// Graph dashboard.
//
// The brief from the phase doc:
//   "The dashboard renders four sub-bands... 1. Counts (entity
//    totals + filed-entry totals). 2. Recent activity (last 10
//    filed entries with chip strips). 3. Queue state line. 4.
//    Flagged for review (== state='failed' in v1). Plus upcoming
//    due dates as an empty-state placeholder until Phase 1E."
//
// One IPC: `kg_dashboard_snapshot()` returns the whole payload in
// a single round-trip. The graph-off contract is honored on both
// sides -- the route's guard short-circuits before mounting this
// component, AND the Rust IPC returns an empty snapshot when the
// toggle is off (belt-and-braces for stale bookmarks). So this
// component can assume kgGraphEnabled === true.
//
// Bands are deliberately KEYBOARD-INERT this wave (1D.2). Row
// clicks on recent-activity items are NOT wired; Wave 1D.4
// relocates the concept modal here and wires it up. The chip
// strips reuse `DictationKgChips` in its inert-span mode (no
// `onConceptOpen` prop) so visually it matches the Dictations
// list but doesn't trip the modal.
//
// Composition: <PageHeader> + four <Card> bands + an "upcoming
// due" placeholder. Each band returns its own empty state when
// the underlying slice is empty -- with the live DB purged
// (kickoff context), the page should render entirely as empty
// states until the user runs a dictation post-toggle-flip. That
// is precisely the acceptance-gate condition.

import { useEffect, useState } from "react";

import {
  Card,
  EmptyState,
  PageHeader,
  Pill,
  Spinner,
} from "../../components/primitives";
import { t } from "../../i18n";
import { formatRelative, formatTimestamp } from "../../lib/format";
import { api } from "../../lib/tauri";
import type {
  DashboardSnapshot,
  EntityTypeCount,
  FailedFiling,
  QueueStatus,
  RecentActivity,
} from "../../lib/types";
import { DictationKgChips } from "../../pages/DictationKgChips";

import styles from "./Dashboard.module.css";

export function KnowledgeGraphDashboard() {
  const [snap, setSnap] = useState<DashboardSnapshot | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Fetch on mount. No polling, no event subscription -- mirrors
  // the SettingsKgFailedFilings pattern (Wave 1C.2): a refetch on
  // mount is enough for a read-only surface. A future wave can
  // promote to event-driven if filing-pipeline-finished events
  // become a thing.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const data = await api.kg_dashboard_snapshot();
        if (!cancelled) {
          setSnap(data);
          setLoadError(null);
        }
      } catch (err) {
        if (!cancelled) setLoadError(String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className={styles.page}>
      <PageHeader
        title={t("kg.dashboard.title")}
        subtitle={t("kg.dashboard.subtitle")}
      />
      {loadError ? (
        <div className={styles.errorBanner} role="alert">
          {t("kg.dashboard.loadError").replace("{error}", loadError)}
        </div>
      ) : null}
      {!snap ? (
        <Spinner label={t("kg.dashboard.title")} />
      ) : (
        <div className={styles.bands}>
          <CountsBand counts={snap.counts} />
          <QueueBand queue={snap.queueStatus} />
          <RecentActivityBand rows={snap.recentActivity} />
          <FlaggedBand rows={snap.flaggedForReview} />
          <UpcomingDueBand />
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Counts band -- totals + entities-by-type breakdown.                */
/* ------------------------------------------------------------------ */

function CountsBand({ counts }: { counts: DashboardSnapshot["counts"] }) {
  return (
    <Card title={t("kg.dashboard.counts.heading")}>
      <div className={styles.totals}>
        <Stat
          label={t("kg.dashboard.counts.totalEntities")}
          value={counts.totalEntities}
        />
        <Stat
          label={t("kg.dashboard.counts.totalEntries")}
          value={counts.totalEntries}
        />
      </div>
      <h3 className={styles.subHeading}>
        {t("kg.dashboard.counts.byType.heading")}
      </h3>
      {counts.entitiesByType.length === 0 ? (
        <p className={styles.empty} role="status">
          {t("kg.dashboard.counts.byType.empty")}
        </p>
      ) : (
        <ul
          className={styles.typeList}
          aria-label={t("kg.dashboard.counts.byType.heading")}
        >
          {counts.entitiesByType.map((row) => (
            <EntityTypeRow key={row.entityType} row={row} />
          ))}
        </ul>
      )}
    </Card>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className={styles.stat}>
      <span className={styles.statValue} aria-label={label}>
        {value}
      </span>
      <span className={styles.statLabel}>{label}</span>
    </div>
  );
}

function EntityTypeRow({ row }: { row: EntityTypeCount }) {
  return (
    <li className={styles.typeRow}>
      <span className={styles.typeName}>{row.entityType}</span>
      <span className={styles.typeCount} aria-label={`${row.count}`}>
        {row.count}
      </span>
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Queue band -- single status line + last-success timestamp.         */
/* ------------------------------------------------------------------ */

function QueueBand({ queue }: { queue: QueueStatus }) {
  const lastDone = queue.lastDoneIso
    ? formatTimestamp(queue.lastDoneIso)
    : t("kg.dashboard.queue.never");
  const line = t("kg.dashboard.queue.line")
    .replace("{pending}", String(queue.pending))
    .replace("{processing}", String(queue.processing))
    .replace("{failed}", String(queue.failed))
    .replace("{done}", String(queue.done));
  const lastDoneLine = t("kg.dashboard.queue.lastDone").replace(
    "{when}",
    lastDone,
  );
  return (
    <Card title={t("kg.dashboard.queue.heading")}>
      <p className={styles.queueLine} role="status">
        {line}
      </p>
      <p className={styles.queueSubLine}>{lastDoneLine}</p>
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Recent activity band -- last N done filings with chip strips.      */
/* ------------------------------------------------------------------ */

function RecentActivityBand({ rows }: { rows: RecentActivity[] }) {
  return (
    <Card title={t("kg.dashboard.recent.heading")}>
      {rows.length === 0 ? (
        <EmptyState title={t("kg.dashboard.recent.empty")} />
      ) : (
        <ul
          className={styles.recentList}
          aria-label={t("kg.dashboard.recent.heading")}
        >
          {rows.map((row) => (
            <RecentActivityRow key={row.entryId} row={row} />
          ))}
        </ul>
      )}
    </Card>
  );
}

function RecentActivityRow({ row }: { row: RecentActivity }) {
  // Reuse DictationKgChips in inert mode (no onConceptOpen) for
  // visual + ARIA parity with the Dictations list. The component
  // takes an EntrySummary, so we shape one here. All rows in
  // recent-activity are by-definition `state='done'` (server-side
  // filter), so filingState is fixed.
  const summary = {
    entities: row.entities,
    tags: row.tags,
    filingState: "done" as const,
  };
  return (
    <li className={styles.recentRow}>
      <div className={styles.recentTopRow}>
        <span className={styles.recentTitle}>{row.title || `#${row.entryId}`}</span>
        <span className={styles.recentWhen}>
          {formatRelative(row.capturedIso)}
        </span>
      </div>
      <DictationKgChips summary={summary} />
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Flagged-for-review band -- failed filings.                         */
/* ------------------------------------------------------------------ */

function FlaggedBand({ rows }: { rows: FailedFiling[] }) {
  return (
    <Card title={t("kg.dashboard.flagged.heading")}>
      {rows.length === 0 ? (
        <EmptyState title={t("kg.dashboard.flagged.empty")} />
      ) : (
        <ul
          className={styles.flaggedList}
          aria-label={t("kg.dashboard.flagged.heading")}
        >
          {rows.map((row) => (
            <li key={row.queueId} className={styles.flaggedRow}>
              <div className={styles.flaggedTopRow}>
                <span className={styles.recentTitle}>
                  {t("kg.dashboard.flagged.entry").replace(
                    "{entryId}",
                    String(row.entryId),
                  )}
                </span>
                <Pill tone="status-error">
                  {/* attempt count plus the most recent failure
                      time gives the user enough to triage without
                      opening the Settings -> KG retry surface.
                      Click-to-retry stays in Settings -> KG this
                      wave; relocating the retry button here is
                      Wave 1D.4's call. */}
                  {row.attemptCount === 1
                    ? t("kg.failed.attempts.one")
                    : t("kg.failed.attempts").replace(
                        "{count}",
                        String(row.attemptCount),
                      )}
                </Pill>
              </div>
              <span className={styles.recentWhen}>
                {t("kg.failed.failedAt").replace(
                  "{when}",
                  formatTimestamp(row.failedIso),
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Upcoming-due band -- 1D.2 placeholder; Phase 1E populates.         */
/* ------------------------------------------------------------------ */

function UpcomingDueBand() {
  // The server always returns an empty list in 1D.2 (see
  // `kg::dashboard::UpcomingDue` docstring). We render a deliberate
  // "coming later" empty-state so the band's spatial slot is
  // visible in the dashboard and Phase 1E's data fill is a pure
  // copy/state change rather than a layout change.
  return (
    <Card title={t("kg.dashboard.upcoming.heading")}>
      <EmptyState title={t("kg.dashboard.upcoming.empty")} />
    </Card>
  );
}
