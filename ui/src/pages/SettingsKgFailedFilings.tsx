// Phase 1C Wave 1C.2 — failed-filings + queue-status UX, extracted
// from SettingsKgTab.tsx so each file stays under the 400-LoC
// reviewability threshold (per the wave brief; the ui-author's
// 600-LoC ceiling is the hard stop).
//
// Authoring scope per ADR 0051 § "UI sealed-surface authorization"
// (Wave 1C.2 / bead mb-9ufg). The parent SettingsKgTab is the only
// caller and gates rendering on `kgGraphEnabled === true` so this
// component never queries the DB before the user has opted in.
//
// Wire contract (all camelCase post-Tauri-serde):
//   * `kg_queue_status() -> { pending, processing, failed, lastDoneIso }`
//   * `kg_list_failed_filings(limit?) -> FailedFiling[]`  (default cap 50, newest-first)
//   * `kg_requeue_failed(queueId) -> void`               (idempotent)
//
// Refresh strategy: on-mount + on retry-success. NO polling, NO Tauri
// events — the wave brief explicitly forbids both. A future wave can
// promote to event-driven if the user's workflow demands sub-second
// freshness; today the cost/benefit doesn't justify the plumbing.

import { useCallback, useEffect, useState } from "react";

import { Button, Card, Pill, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { formatTimestamp, truncate } from "../lib/format";
import { api } from "../lib/tauri";
import type { FailedFiling, QueueStatus } from "../lib/types";

import styles from "./Settings.module.css";

// Per ADR 0051 D1: inline-truncate the error text past this length;
// the full message stays accessible via the native title= tooltip.
const ERROR_TRUNCATE = 200;

// Transient toast lifetime. Long enough to read, short enough not
// to linger — mirrors the post-Copy toast on Dictations detail.
const TOAST_MS = 2500;

interface Toast {
  kind: "ok" | "err";
  text: string;
}

/** Public surface — the parent renders this iff KgGraphEnabled is on.
 *  All state lives inside; the parent doesn't need to know about
 *  queue rows. Mount/unmount cleanly tracks the toggle. */
export function SettingsKgFailedFilings() {
  const [status, setStatus] = useState<QueueStatus | null>(null);
  const [failed, setFailed] = useState<FailedFiling[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  // queueId currently being retried — used to disable the button
  // mid-flight so a fast double-click doesn't fire two IPC calls.
  // The Rust side IS idempotent (J3 invariant) but the optimistic
  // UX still benefits from one round-trip per click.
  const [retrying, setRetrying] = useState<number | null>(null);

  // Centralised refresh. Called on mount and after each successful
  // retry. Both IPCs run in parallel so the UI flips together.
  const refreshQueue = useCallback(async () => {
    try {
      const [s, f] = await Promise.all([
        api.kg_queue_status(),
        api.kg_list_failed_filings(),
      ]);
      setStatus(s);
      setFailed(f);
      setLoadError(null);
    } catch (err) {
      setLoadError(String(err));
    }
  }, []);

  useEffect(() => {
    void refreshQueue();
  }, [refreshQueue]);

  // Toast auto-dismiss. Keyed off the toast object identity so
  // successive retries reset the clock.
  useEffect(() => {
    if (!toast) return;
    const handle = window.setTimeout(() => setToast(null), TOAST_MS);
    return () => window.clearTimeout(handle);
  }, [toast]);

  const onRetry = useCallback(
    async (queueId: number) => {
      setRetrying(queueId);
      try {
        await api.kg_requeue_failed(queueId);
        setToast({ kind: "ok", text: t("kg.failed.retry.success") });
        await refreshQueue();
      } catch (err) {
        setToast({
          kind: "err",
          text: t("kg.failed.retry.error").replace("{error}", String(err)),
        });
      } finally {
        setRetrying(null);
      }
    },
    [refreshQueue],
  );

  return (
    <Card title={t("kg.status.title")}>
      {loadError ? (
        <div className={styles.errorBanner} role="alert">
          {t("kg.status.loadError").replace("{error}", loadError)}
        </div>
      ) : null}

      <StatusLine status={status} />
      <FailedFilingsList
        rows={failed}
        retrying={retrying}
        onRetry={onRetry}
      />

      {/* Transient toast — inline near the list so it sits in the
          user's gaze right after they click Retry. role differs by
          kind so screen readers announce errors as alerts. */}
      {toast ? (
        <div
          role={toast.kind === "err" ? "alert" : "status"}
          aria-live="polite"
          style={{
            marginTop: "var(--s-2)",
            background: "var(--surf-1)",
            borderLeft:
              toast.kind === "err"
                ? "3px solid var(--status-error)"
                : "3px solid var(--mode-normal)",
            borderRadius: "var(--r-2)",
            padding: "var(--s-2) var(--s-3)",
            font: "var(--type-sm)",
            color: "var(--on-surf)",
          }}
        >
          {toast.text}
        </div>
      ) : null}
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* StatusLine — single-line summary above the failed-filings list.   */
/* ------------------------------------------------------------------ */

function StatusLine({ status }: { status: QueueStatus | null }) {
  if (!status) {
    // Render a neutral placeholder so layout doesn't jump when the
    // data lands; the parent's error banner covers the failure case.
    return (
      <p
        style={{
          margin: 0,
          font: "var(--type-sm)",
          color: "var(--on-surf-muted)",
        }}
      >
        —
      </p>
    );
  }
  const lastDone = status.lastDoneIso
    ? formatTimestamp(status.lastDoneIso)
    : t("kg.status.never");
  const text = t("kg.status.line")
    .replace("{pending}", String(status.pending))
    .replace("{processing}", String(status.processing))
    .replace("{failed}", String(status.failed))
    .replace("{lastDone}", lastDone);
  return (
    <p
      style={{
        margin: 0,
        font: "var(--type-sm)",
        color: "var(--on-surf-muted)",
      }}
    >
      {text}
    </p>
  );
}

/* ------------------------------------------------------------------ */
/* FailedFilingsList — one row per failed kg_filing_queue entry.     */
/* ------------------------------------------------------------------ */

interface FailedFilingsListProps {
  rows: FailedFiling[] | null;
  retrying: number | null;
  onRetry: (queueId: number) => void;
}

function FailedFilingsList({
  rows,
  retrying,
  onRetry,
}: FailedFilingsListProps) {
  if (rows === null) {
    // Initial fetch in flight.
    return <Spinner label="Loading failed filings" />;
  }
  if (rows.length === 0) {
    return (
      <p
        role="status"
        style={{
          margin: 0,
          padding: "var(--s-2) 0",
          font: "var(--type-sm)",
          color: "var(--on-surf-muted)",
        }}
      >
        {t("kg.failed.empty")}
      </p>
    );
  }
  return (
    <ul
      aria-label={t("kg.failed.heading")}
      style={{
        listStyle: "none",
        margin: 0,
        padding: 0,
        display: "flex",
        flexDirection: "column",
        gap: "var(--s-2)",
      }}
    >
      {rows.map((row) => (
        <FailedFilingRow
          key={row.queueId}
          row={row}
          isRetrying={retrying === row.queueId}
          onRetry={onRetry}
        />
      ))}
    </ul>
  );
}

function FailedFilingRow({
  row,
  isRetrying,
  onRetry,
}: {
  row: FailedFiling;
  isRetrying: boolean;
  onRetry: (queueId: number) => void;
}) {
  const truncated = truncate(row.lastError, ERROR_TRUNCATE);
  const showTooltip = row.lastError.length > ERROR_TRUNCATE;
  const attemptsLabel =
    row.attemptCount === 1
      ? t("kg.failed.attempts.one")
      : t("kg.failed.attempts").replace("{count}", String(row.attemptCount));
  return (
    <li
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--s-1)",
        padding: "var(--s-2) var(--s-3)",
        background: "var(--surf-1)",
        border: "1px solid var(--border)",
        borderRadius: "var(--r-2)",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          gap: "var(--s-2)",
          flexWrap: "wrap",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--s-2)",
            flexWrap: "wrap",
          }}
        >
          <span
            style={{
              font: "var(--type-sm)",
              fontWeight: 600,
              color: "var(--on-surf)",
            }}
          >
            {t("kg.failed.entry").replace("{entryId}", String(row.entryId))}
          </span>
          <Pill tone="status-error">{attemptsLabel}</Pill>
          <span
            style={{
              font: "var(--type-xs)",
              color: "var(--on-surf-muted)",
            }}
          >
            {t("kg.failed.failedAt").replace(
              "{when}",
              formatTimestamp(row.failedIso),
            )}
          </span>
        </div>
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
      <div
        // Native title= surfaces the full text on hover; the brief
        // explicitly OKs this in lieu of a custom tooltip primitive.
        title={showTooltip ? row.lastError : undefined}
        style={{
          font: "var(--type-xs)",
          color: "var(--on-surf-muted)",
          fontFamily: "var(--font-mono)",
          wordBreak: "break-word",
        }}
      >
        {truncated}
      </div>
    </li>
  );
}
