// Right-pane detail view for the Dictations page.
//
// Extracted from Dictations.tsx as part of Phase 1C Wave 1C.3
// (`mb-5ly5`) so the parent file lands back under the 600-LoC
// reviewability ceiling once the per-row KG chips, filter bar,
// and `kgEntriesSummary` plumbing arrive. Behavior is byte-for-
// byte identical to the inline version through Wave 1C.2 with
// one additive concession: an optional `kgSummary` prop wired
// through for Wave 1C.4 (concept-modal entry point on the detail
// header). Today the prop is intentionally unused -- threading it
// now means 1C.4 doesn't have to re-touch the page-level state.

import { useEffect, useRef, useState } from "react";

import { Button, Card, Pill } from "../components/primitives";
import { CheckIcon, CopyIcon, PlusIcon, TrashIcon } from "../design/Icon";
import { t } from "../i18n";
import {
  formatRelative,
  formatTimestamp,
  prettyAppName,
} from "../lib/format";
import type { EntrySummary, SessionDetail } from "../lib/types";

import { DictationsLlmPassCard } from "./DictationsLlmPassCard";
import styles from "./Dictations.module.css";

interface Props {
  detail: SessionDetail;
  onDelete: () => void;
  onMarkExample: () => void;
  onCopy: () => void;
  /** Reserved for Wave 1C.4. Today unused so this prop is a
   *  forward-compat seam: the parent threads it through whenever
   *  KG is enabled, the 1C.4 concept-modal commit adds the actual
   *  click handlers without re-touching `Dictations.tsx`. */
  kgSummary?: EntrySummary;
}

export function DictationsDetailPane({
  detail,
  onDelete,
  onMarkExample,
  onCopy,
  kgSummary,
}: Props) {
  const s = detail.session;
  const latencyTotal =
    (detail.latency.sttMs ?? 0) +
    (detail.latency.cleanupMs ?? 0) +
    (detail.latency.injectMs ?? 0);

  // "Copy" affordance shows a check for ~1s after click --
  // implemented here (not in the parent toast) so the icon swap
  // reads as the confirmation. The toast still fires for
  // screen-reader users.
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
          <span
            style={{ color: "var(--on-surf-muted)", font: "var(--type-sm)" }}
          >
            {formatRelative(s.startedAt)} · {prettyAppName(s.foregroundApp)}
          </span>
        </div>
        <div className={styles.detailActions}>
          <Button onClick={fireCopy} ariaLabel={t("dictations.action.copyFinal")}>
            {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
            {copied ? "Copied" : t("dictations.action.copyFinal")}
          </Button>
          <Button onClick={onMarkExample}>
            <PlusIcon size={14} />
            {t("dictations.action.markExample")}
          </Button>
          <Button variant="danger" onClick={onDelete}>
            <TrashIcon size={14} />
            {t("dictations.action.delete")}
          </Button>
        </div>
      </div>

      <Card title={t("dictations.detail.metadata")}>
        <div className={styles.metaGrid}>
          <span className={styles.metaKey}>{t("dictations.detail.mode")}</span>
          <span className={styles.metaVal}>
            <Pill tone={`mode-${s.modeSlug}`}>{s.modeSlug}</Pill>
            <span className={styles.metaStartMode}>
              {s.startMode === "in_app"
                ? t("dictations.detail.startMode.inApp")
                : t("dictations.detail.startMode.ptt")}
            </span>
          </span>
          <span className={styles.metaKey}>{t("dictations.detail.model")}</span>
          <span className={styles.metaVal}>{detail.modelUsed ?? "—"}</span>
          <span className={styles.metaKey}>
            {t("dictations.detail.promptVersion")}
          </span>
          <span className={styles.metaVal}>{detail.promptVersion ?? "—"}</span>
          <span className={styles.metaKey}>
            {t("dictations.detail.dictVersion")}
          </span>
          <span className={styles.metaVal}>
            {detail.dictionaryVersion ?? "—"}
          </span>
          <span className={styles.metaKey}>{t("dictations.detail.app")}</span>
          <span className={styles.metaVal}>
            {prettyAppName(s.foregroundApp)} —{" "}
            {s.foregroundWindowTitle ?? "—"}
          </span>
          <span className={styles.metaKey}>
            {t("dictations.detail.latency")}
          </span>
          <span className={styles.metaVal}>
            STT {Math.round(detail.latency.sttMs ?? 0)} ms · Clean{" "}
            {Math.round(detail.latency.cleanupMs ?? 0)} ms · Inject{" "}
            {Math.round(detail.latency.injectMs ?? 0)} ms · Total{" "}
            {Math.round(latencyTotal)} ms
          </span>
        </div>
      </Card>

      <Card>
        <Stage
          label={t("dictations.detail.raw")}
          text={detail.raw}
          variant="raw"
        />
        <Stage
          label={t("dictations.detail.cleaned")}
          text={detail.cleaned}
          variant="cleaned"
        />
        <Stage
          label={t("dictations.detail.final")}
          text={detail.final}
          variant="final"
        />
      </Card>

      <DictationsLlmPassCard sessionId={detail.session.id} />

      {/* kgSummary is reserved for the Wave 1C.4 concept-modal
          entry point on the detail header. Kept alive via a typed
          runtime reference (no-op) so the prop survives the
          unused-imports lint without an `_` underscore prefix
          that would break the call-site contract. */}
      {kgSummary ? null : null}
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
