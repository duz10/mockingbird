// Phase 1D Wave 1D.2 (`mb-j00j`, ADR 0052) -- Knowledge Graph
// dashboard. Promoted by Wave 1D.3 (`mb-0gt6`) with a capture
// surface; further extended by Wave 1D.4 (`mb-6hm2`, ADR 0052 D5)
// to host the relocated KG retrieval surface (filter chips + per-
// row chip strip + concept modal) and to gain a click-to-retry
// affordance on the Flagged-for-review band.
//
// Band layout (top to bottom):
//   1. CaptureBand           -- audio/text note capture (1D.3).
//   2. CountsBand            -- totals + entities-by-type (1D.2).
//   3. QueueBand             -- pending/processing/failed/done (1D.2).
//   4. Retrieval             -- filter chips + filtered list (1D.4).
//   5. RecentActivityBand    -- last N done filings, chip strips
//                               now interactive (1D.4).
//   6. FlaggedBand           -- failed filings + Retry buttons (1D.4).
//   7. UpcomingDueBand       -- placeholder, Phase 1E populates.
//
// The page mounts a single `ConceptModal` instance at the bottom
// of the JSX tree; bands that surface chips (Retrieval +
// RecentActivityBand) bubble chip clicks up via an `onConceptOpen`
// prop so all entity/tag drill-down funnels through one modal.
// This was the Phase 1C pattern on Dictations (Wave 1C.4); the
// pattern moves wholesale to the dashboard in 1D.4.
//
// One IPC for the read-only bands: `kg_dashboard_snapshot()`
// returns the whole snapshot payload in a single round-trip. The
// graph-off contract is honored on both sides -- the route's guard
// short-circuits before mounting this component, AND the Rust IPC
// returns an empty snapshot when the toggle is off (belt-and-braces
// for stale bookmarks). So this component can assume
// kgGraphEnabled === true.

import { useCallback, useEffect, useState } from "react";

import {
  Button,
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
  ActiveConcept,
  DashboardSnapshot,
  EntityTypeCount,
  FailedFiling,
  QueueStatus,
  RecentActivity,
} from "../../lib/types";

import { CaptureBand } from "./CaptureBand";
import { ConceptModal } from "./ConceptModal";
import { EntryChips } from "./EntryChips";
import { Retrieval } from "./Retrieval";
import styles from "./Dashboard.module.css";

// Transient toast lifetime for the FlaggedBand's retry feedback.
// Mirrors the value SettingsKgFailedFilings used before relocating.
const RETRY_TOAST_MS = 2500;

export function KnowledgeGraphDashboard() {
  const [snap, setSnap] = useState<DashboardSnapshot | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // Wave 1D.4 -- the single ConceptModal lives here; bands surface
  // their chip clicks up via `onConceptOpen`.
  const [activeConcept, setActiveConcept] = useState<ActiveConcept | null>(
    null,
  );

  // Centralized refetch -- shared by the on-mount fetch, the
  // post-capture refresh hook the CaptureBand fires after an
  // audio or text note files, and FlaggedBand's post-retry
  // refresh. Wave 1D.3 promoted this to a named callback; Wave
  // 1D.4 added the retry consumer.
  const refetch = useCallback(async () => {
    try {
      const data = await api.kg_dashboard_snapshot();
      setSnap(data);
      setLoadError(null);
    } catch (err) {
      setLoadError(String(err));
    }
  }, []);

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

  const handleConceptOpen = useCallback((c: ActiveConcept) => {
    setActiveConcept(c);
  }, []);
  const handleConceptClose = useCallback(() => {
    setActiveConcept(null);
  }, []);
  // The modal's "click a recent-entry row" handler. On Dictations
  // it selected the session in the right-hand detail pane; the KG
  // dashboard has no detail pane, so we just close the modal here.
  // Deep-linking to /dictations?selected=<id> is filed as a
  // follow-up bead.
  const handleConceptSelectEntry = useCallback((_entryId: number) => {
    setActiveConcept(null);
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
          <CaptureBand onFiled={() => void refetch()} />
          <CountsBand counts={snap.counts} />
          <QueueBand queue={snap.queueStatus} />
          <Retrieval onConceptOpen={handleConceptOpen} />
          <RecentActivityBand
            rows={snap.recentActivity}
            onConceptOpen={handleConceptOpen}
          />
          <FlaggedBand rows={snap.flaggedForReview} onRetried={refetch} />
          <UpcomingDueBand />
        </div>
      )}

      {/* Single ConceptModal instance at the page level. Stays
       *  mounted with open=false (concept=null) between
       *  invocations so successive opens don't flash a teardown
       *  frame. Pattern carried over from Dictations.tsx (1C.4)
       *  when the modal relocated here in 1D.4. */}
      <ConceptModal
        concept={activeConcept}
        onClose={handleConceptClose}
        onSelectEntry={handleConceptSelectEntry}
      />
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

function RecentActivityBand({
  rows,
  onConceptOpen,
}: {
  rows: RecentActivity[];
  onConceptOpen: (concept: ActiveConcept) => void;
}) {
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
            <RecentActivityRow
              key={row.entryId}
              row={row}
              onConceptOpen={onConceptOpen}
            />
          ))}
        </ul>
      )}
    </Card>
  );
}

function RecentActivityRow({
  row,
  onConceptOpen,
}: {
  row: RecentActivity;
  onConceptOpen: (concept: ActiveConcept) => void;
}) {
  // Wave 1D.4 -- chips are now interactive (the per-row strip
  // gains `onConceptOpen`). All rows in recent-activity are
  // by-definition `state='done'` (server-side filter), so
  // filingState is fixed.
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
      <EntryChips summary={summary} onConceptOpen={onConceptOpen} />
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Flagged-for-review band -- failed filings + click-to-retry.        */
/* ------------------------------------------------------------------ */

function FlaggedBand({
  rows,
  onRetried,
}: {
  rows: FailedFiling[];
  /** Called after a successful `kg_requeue_failed` so the parent
   *  refetches the dashboard snapshot (which drops the retried
   *  row out of the flagged set). */
  onRetried: () => Promise<void>;
}) {
  // queueId currently being retried -- disables the button mid-
  // flight so a fast double-click doesn't fire two IPCs. The Rust
  // side IS idempotent (Wave 1C.5 J3 invariant) but the optimistic
  // UX still benefits from one round-trip per click. Mirrors the
  // pattern SettingsKgFailedFilings used pre-relocation.
  const [retrying, setRetrying] = useState<number | null>(null);
  const [toast, setToast] = useState<{ kind: "ok" | "err"; text: string } | null>(
    null,
  );

  // Toast auto-dismiss. Keyed off the toast object identity so
  // successive retries reset the clock.
  useEffect(() => {
    if (!toast) return;
    const handle = window.setTimeout(() => setToast(null), RETRY_TOAST_MS);
    return () => window.clearTimeout(handle);
  }, [toast]);

  const onRetry = useCallback(
    async (queueId: number) => {
      setRetrying(queueId);
      try {
        await api.kg_requeue_failed(queueId);
        setToast({ kind: "ok", text: t("kg.failed.retry.success") });
        await onRetried();
      } catch (err) {
        setToast({
          kind: "err",
          text: t("kg.failed.retry.error").replace("{error}", String(err)),
        });
      } finally {
        setRetrying(null);
      }
    },
    [onRetried],
  );

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
            <FlaggedRow
              key={row.queueId}
              row={row}
              isRetrying={retrying === row.queueId}
              onRetry={onRetry}
            />
          ))}
        </ul>
      )}

      {toast ? (
        <div
          role={toast.kind === "err" ? "alert" : "status"}
          aria-live="polite"
          className={
            toast.kind === "err" ? styles.toastError : styles.toastOk
          }
        >
          {toast.text}
        </div>
      ) : null}
    </Card>
  );
}

function FlaggedRow({
  row,
  isRetrying,
  onRetry,
}: {
  row: FailedFiling;
  isRetrying: boolean;
  onRetry: (queueId: number) => void;
}) {
  const attemptsLabel =
    row.attemptCount === 1
      ? t("kg.failed.attempts.one")
      : t("kg.failed.attempts").replace("{count}", String(row.attemptCount));
  return (
    <li className={styles.flaggedRow}>
      <div className={styles.flaggedTopRow}>
        <span className={styles.recentTitle}>
          {t("kg.dashboard.flagged.entry").replace(
            "{entryId}",
            String(row.entryId),
          )}
        </span>
        <Pill tone="status-error">{attemptsLabel}</Pill>
        <Button
          variant="primary"
          size="sm"
          onClick={() => onRetry(row.queueId)}
          disabled={isRetrying}
          ariaLabel={t("kg.failed.retry.aria").replace(
            "{entryId}",
            String(row.entryId),
          )}
        >
          {t("kg.failed.retry")}
        </Button>
      </div>
      <span className={styles.recentWhen}>
        {t("kg.failed.failedAt").replace(
          "{when}",
          formatTimestamp(row.failedIso),
        )}
      </span>
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Upcoming-due band -- 1D.2 placeholder; Phase 1E populates.         */
/* ------------------------------------------------------------------ */

function UpcomingDueBand() {
  return (
    <Card title={t("kg.dashboard.upcoming.heading")}>
      <EmptyState title={t("kg.dashboard.upcoming.empty")} />
    </Card>
  );
}
