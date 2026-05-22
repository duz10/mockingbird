// Activity page — sessions list + chronological event timeline.
//
// Phase 10 Wave 1B (ADR 0036). Two-pane layout mirroring Meetings:
//   left  — list of `activity_sessions` rows, newest first
//   right — selected session's event timeline, chronological
//
// Why a single page instead of `/activity` + `/activity/:id`:
//   * The user clicks a row to inspect events without losing list
//     context (same affordance Meetings + Dictations both ship).
//   * Adding a second routed page for the detail view would replicate
//     the list-fetch logic (DRY).
//
// Both `/activity` and `/activity/:id` resolve to this component.
// Selection is driven by the URL param; falling back to the
// most-recent session when none is selected. This matches the
// Meetings page pattern exactly.
//
// Wave 1B intentionally has NO live event subscription: the
// CommandCenter is the start/stop surface and re-mounting the page
// (or clicking the refresh button) is sufficient. Wave 2+ can wire
// an `activity:state` event when there's reason to.

import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { EmptyState, PageHeader, Pill, Spinner } from "../components/primitives";
import { ActivityIcon } from "../design/Icon";
import { t } from "../i18n";
import { activityApi } from "../lib/activity";
import type {
  ActivityEventRow,
  ActivitySessionDetail,
  ActivitySessionRow,
  ActivitySessionStatus,
} from "../lib/activity";
import { formatDuration, formatRelative } from "../lib/format";

import styles from "./Activity.module.css";

const LIST_LIMIT = 100;

/** How the status pill in the row + detail header renders. Tones
 *  map to the existing token names (see `design/tokens.css`). */
function statusToneAndLabel(
  s: ActivitySessionStatus,
): { tone: string; label: string } {
  switch (s) {
    case "in_progress":
      return { tone: "status-info", label: t("activity.status.inProgress") };
    case "completed":
      return { tone: "status-ok", label: t("activity.status.completed") };
    case "partial":
      return { tone: "status-error", label: t("activity.status.partial") };
    case "crashed_recovered":
      return { tone: "status-error", label: t("activity.status.crashedRecovered") };
  }
}

/** Human-readable kind label for a row in the event timeline. */
function eventKindLabel(kind: string): string {
  switch (kind) {
    case "app_switch":
      return t("activity.event.appSwitch");
    case "context_snapshot":
      return t("activity.event.contextSnapshot");
    case "paused":
      return t("activity.event.paused");
    case "resumed":
      return t("activity.event.resumed");
    case "idle_start":
      return t("activity.event.idleStart");
    case "idle_end":
      return t("activity.event.idleEnd");
    case "layer_error":
      return t("activity.event.layerError");
    default:
      return kind;
  }
}

/**
 * Compute a session duration label.
 *
 * For `in_progress` rows we substitute "now" so the timer keeps
 * advancing while the user is reading — without setting up an
 * interval here, the value is just the gap between `startedAt` and
 * the moment this function runs (which is on every re-render,
 * which is good enough for a passive list page).
 */
function durationOf(row: ActivitySessionRow): string {
  const end = row.endedAt ?? Date.now();
  return formatDuration(Math.max(0, end - row.startedAt));
}

export function ActivityPage() {
  const { id: paramId } = useParams<{ id?: string }>();
  const navigate = useNavigate();

  const [rows, setRows] = useState<ActivitySessionRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [detail, setDetail] = useState<ActivitySessionDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const selectedId = useMemo(() => {
    if (paramId) return paramId;
    return rows && rows.length > 0 ? rows[0]!.id : null;
  }, [paramId, rows]);

  // Load list on mount + when we expect the data may have changed
  // (after delete).
  const refresh = useCallback(async () => {
    try {
      const list = await activityApi.listSessions(LIST_LIMIT);
      setRows(list);
      setError(null);
    } catch (e) {
      setError(String(e));
      setRows([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Load detail when selection changes.
  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    setDetailLoading(true);
    let cancelled = false;
    void (async () => {
      try {
        const d = await activityApi.getSessionDetail(selectedId);
        if (!cancelled) setDetail(d);
      } catch (e) {
        if (!cancelled) {
          setDetail(null);
          setError(String(e));
        }
      } finally {
        if (!cancelled) setDetailLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const onSelect = useCallback(
    (id: string) => {
      navigate(`/activity/${id}`);
    },
    [navigate],
  );

  const onDelete = useCallback(
    async (id: string) => {
      // eslint-disable-next-line no-alert
      if (!window.confirm(t("activity.detail.deleteConfirm"))) return;
      try {
        await activityApi.deleteSession(id);
        // Drop the selection if we just nuked it.
        if (paramId === id) navigate("/activity");
        await refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [paramId, navigate, refresh],
  );

  if (rows === null) {
    return (
      <>
        <PageHeader title={t("activity.title")} subtitle={t("activity.subtitle")} />
        <Spinner />
      </>
    );
  }

  if (rows.length === 0) {
    return (
      <>
        <PageHeader title={t("activity.title")} subtitle={t("activity.subtitle")} />
        <EmptyState
          icon={<ActivityIcon size={32} />}
          title={t("activity.empty.title")}
          subtitle={t("activity.empty.body")}
        />
      </>
    );
  }

  return (
    <>
      <PageHeader title={t("activity.title")} subtitle={t("activity.subtitle")} />
      {error && <div className={styles.error}>{error}</div>}
      <div className={styles.shell}>
        <aside className={styles.listPane} aria-label="Activity sessions">
          <ul className={styles.list}>
            {rows.map((row) => {
              const { tone, label } = statusToneAndLabel(row.status);
              const isSelected = row.id === selectedId;
              return (
                <li key={row.id}>
                  <button
                    type="button"
                    className={
                      isSelected ? `${styles.row} ${styles.rowActive}` : styles.row
                    }
                    onClick={() => onSelect(row.id)}
                  >
                    <div className={styles.rowHead}>
                      <span className={styles.rowDate}>
                        {formatRelative(new Date(row.startedAt).toISOString())}
                      </span>
                      <Pill tone={tone}>{label}</Pill>
                    </div>
                    <div className={styles.rowMeta}>
                      <span>
                        {t("activity.list.duration")}: {durationOf(row)}
                      </span>
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>
        </aside>
        <section className={styles.detailPane} aria-label="Selected session detail">
          {detailLoading ? (
            <Spinner />
          ) : detail ? (
            <ActivityDetailView detail={detail} onDelete={onDelete} />
          ) : (
            <EmptyState
              icon={<ActivityIcon size={32} />}
              title={t("activity.detail.empty")}
            />
          )}
        </section>
      </div>
    </>
  );
}

interface DetailViewProps {
  detail: ActivitySessionDetail;
  onDelete: (id: string) => void;
}

function ActivityDetailView({ detail, onDelete }: DetailViewProps) {
  const { session, events } = detail;
  const { tone, label } = statusToneAndLabel(session.status);
  return (
    <div className={styles.detail}>
      <header className={styles.detailHead}>
        <div>
          <h2 className={styles.detailTitle}>
            {formatRelative(new Date(session.startedAt).toISOString())}
          </h2>
          <div className={styles.detailSub}>
            <Pill tone={tone}>{label}</Pill>
            <span>
              {t("activity.list.duration")}: {durationOf(session)}
            </span>
            <span>
              {t("activity.list.events")}: {events.length}
            </span>
          </div>
        </div>
        <button
          type="button"
          className={styles.deleteBtn}
          onClick={() => onDelete(session.id)}
        >
          {t("activity.detail.delete")}
        </button>
      </header>

      {events.length === 0 ? (
        <EmptyState
          title={t("activity.detail.empty")}
          icon={<ActivityIcon size={32} />}
        />
      ) : (
        <ol className={styles.timeline}>
          {events.map((e) => (
            <ActivityEventRowView key={e.id} event={e} startedAt={session.startedAt} />
          ))}
        </ol>
      )}
    </div>
  );
}

interface EventRowProps {
  event: ActivityEventRow;
  startedAt: number;
}

function ActivityEventRowView({ event, startedAt }: EventRowProps) {
  const elapsedMs = Math.max(0, event.ts - startedAt);
  const elapsed = formatDuration(elapsedMs);
  const kindLabel = eventKindLabel(event.kind);
  const title = event.windowTitle?.trim() || t("activity.event.untitled");
  const app = event.appName;
  return (
    <li className={styles.event} data-kind={event.kind}>
      <span className={styles.eventTime}>{elapsed}</span>
      <span className={styles.eventKind}>{kindLabel}</span>
      <span className={styles.eventBody}>
        {app && <strong className={styles.eventApp}>{app}</strong>}
        {app && <span aria-hidden> · </span>}
        <span className={styles.eventTitle}>{title}</span>
      </span>
    </li>
  );
}
