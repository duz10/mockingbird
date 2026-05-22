// Wave-3 Blocks + Summary panel for the Activity detail view
// (ADR 0040). Sibling component to `Activity.tsx` so the parent
// page stays under the 600-line file limit. Owned by the
// activity-capture surface; no cross-feature deps.
//
// Layout: a small toolbar (Generate / Copy / Export) above a
// list of Block cards (label, time-range, abstract). Each card
// can be renamed / abstract-rewritten / deleted in place.

import { useCallback, useEffect, useMemo, useState } from "react";

import { Pill, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { activityApi } from "../lib/activity";
import type { ActivityBlockRow } from "../lib/activity";
import { formatDuration } from "../lib/format";

import styles from "./ActivityBlocks.module.css";

interface Props {
  sessionId: string;
  /** True iff the session has finished (we don't summarize while live). */
  canSummarize: boolean;
}

export function ActivityBlocksPanel({ sessionId, canSummarize }: Props) {
  const [blocks, setBlocks] = useState<ActivityBlockRow[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const refreshBlocks = useCallback(async () => {
    try {
      const rows = await activityApi.listBlocks(sessionId);
      setBlocks(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [sessionId]);

  useEffect(() => {
    refreshBlocks().catch(() => {
      // listBlocks errors land in the state; nothing else to do.
    });
  }, [refreshBlocks]);

  const onGenerate = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await activityApi.regenerateSummary(sessionId);
      await refreshBlocks();
      setToast(t("activity.blocks.toast.generated"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [sessionId, refreshBlocks]);

  const onCopy = useCallback(async () => {
    try {
      await activityApi.copyToClipboard(sessionId);
      setToast(t("activity.blocks.toast.copied"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [sessionId]);

  const onDeleteBlock = useCallback(
    async (blockId: string) => {
      try {
        await activityApi.block.delete(blockId);
        await refreshBlocks();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [refreshBlocks],
  );

  const onRenameBlock = useCallback(
    async (blockId: string, newLabel: string) => {
      const trimmed = newLabel.trim();
      try {
        await activityApi.block.rename(blockId, trimmed === "" ? null : trimmed);
        await refreshBlocks();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [refreshBlocks],
  );

  const onRewriteAbstract = useCallback(
    async (blockId: string, text: string) => {
      try {
        await activityApi.block.rewriteAbstract(blockId, text);
        await refreshBlocks();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [refreshBlocks],
  );

  // Auto-clear toast after 3s — small UX nicety, no library needed.
  useEffect(() => {
    if (!toast) return;
    const h = window.setTimeout(() => setToast(null), 3_000);
    return () => window.clearTimeout(h);
  }, [toast]);

  const hasBlocks = (blocks?.length ?? 0) > 0;

  return (
    <section className={styles.panel} aria-label={t("activity.blocks.heading")}>
      <header className={styles.head}>
        <h3 className={styles.heading}>{t("activity.blocks.heading")}</h3>
        <div className={styles.actions}>
          <button
            type="button"
            className={styles.primaryBtn}
            disabled={!canSummarize || busy}
            onClick={onGenerate}
          >
            {busy
              ? t("activity.blocks.generating")
              : hasBlocks
                ? t("activity.blocks.regenerate")
                : t("activity.blocks.generate")}
          </button>
          <button
            type="button"
            className={styles.secondaryBtn}
            disabled={!hasBlocks}
            onClick={onCopy}
          >
            {t("activity.blocks.copy")}
          </button>
        </div>
      </header>
      {toast && (
        <div className={styles.toast} role="status">
          {toast}
        </div>
      )}
      {error && (
        <div className={styles.error} role="alert">
          {error}
        </div>
      )}
      {!canSummarize && (
        <p className={styles.hint}>{t("activity.blocks.hint.live")}</p>
      )}
      {blocks === null ? (
        <div className={styles.spinnerWrap}>
          <Spinner />
        </div>
      ) : hasBlocks ? (
        <ol className={styles.blocks}>
          {blocks.map((b) => (
            <BlockCard
              key={b.id}
              block={b}
              onRename={onRenameBlock}
              onRewriteAbstract={onRewriteAbstract}
              onDelete={onDeleteBlock}
            />
          ))}
        </ol>
      ) : (
        <p className={styles.empty}>{t("activity.blocks.empty")}</p>
      )}
    </section>
  );
}

interface CardProps {
  block: ActivityBlockRow;
  onRename: (id: string, newLabel: string) => void;
  onRewriteAbstract: (id: string, text: string) => void;
  onDelete: (id: string) => void;
}

function BlockCard({
  block,
  onRename,
  onRewriteAbstract,
  onDelete,
}: CardProps) {
  const [editingLabel, setEditingLabel] = useState(false);
  const [editingAbstract, setEditingAbstract] = useState(false);
  const [labelDraft, setLabelDraft] = useState(block.label ?? "");
  const [absDraft, setAbsDraft] = useState(block.generatedAbstract ?? "");

  // Reset drafts if the parent refetches the block.
  useEffect(() => {
    setLabelDraft(block.label ?? "");
    setAbsDraft(block.generatedAbstract ?? "");
  }, [block.label, block.generatedAbstract]);

  const duration = useMemo(
    () => formatDuration(Math.max(0, block.endedAt - block.startedAt)),
    [block.startedAt, block.endedAt],
  );

  const display = block.label?.trim() || block.primaryApp;

  return (
    <li className={styles.card} data-edited={block.userEdited || undefined}>
      <header className={styles.cardHead}>
        {editingLabel ? (
          <div className={styles.inlineEditRow}>
            <input
              className={styles.input}
              value={labelDraft}
              onChange={(e) => setLabelDraft(e.target.value)}
              placeholder={block.primaryApp}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  onRename(block.id, labelDraft);
                  setEditingLabel(false);
                } else if (e.key === "Escape") {
                  setLabelDraft(block.label ?? "");
                  setEditingLabel(false);
                }
              }}
            />
            <button
              type="button"
              className={styles.tinyBtn}
              onClick={() => {
                onRename(block.id, labelDraft);
                setEditingLabel(false);
              }}
            >
              {t("activity.blocks.save")}
            </button>
            <button
              type="button"
              className={styles.tinyBtn}
              onClick={() => {
                setLabelDraft(block.label ?? "");
                setEditingLabel(false);
              }}
            >
              {t("activity.blocks.cancel")}
            </button>
          </div>
        ) : (
          <button
            type="button"
            className={styles.titleBtn}
            onClick={() => setEditingLabel(true)}
            title={t("activity.blocks.rename")}
          >
            <strong>{display}</strong>
            <span className={styles.subtitle}>{block.primaryTitle}</span>
          </button>
        )}
        <div className={styles.cardMeta}>
          <span>{duration}</span>
          {block.userEdited && (
            <Pill tone="status-info">{t("activity.blocks.edited")}</Pill>
          )}
        </div>
      </header>
      <div className={styles.cardBody}>
        {editingAbstract ? (
          <div className={styles.editAbstract}>
            <textarea
              className={styles.textarea}
              value={absDraft}
              onChange={(e) => setAbsDraft(e.target.value)}
              rows={3}
            />
            <div className={styles.inlineEditRow}>
              <button
                type="button"
                className={styles.tinyBtn}
                onClick={() => {
                  onRewriteAbstract(block.id, absDraft);
                  setEditingAbstract(false);
                }}
              >
                {t("activity.blocks.save")}
              </button>
              <button
                type="button"
                className={styles.tinyBtn}
                onClick={() => {
                  setAbsDraft(block.generatedAbstract ?? "");
                  setEditingAbstract(false);
                }}
              >
                {t("activity.blocks.cancel")}
              </button>
            </div>
          </div>
        ) : (
          <p
            className={styles.abstract}
            onClick={() => setEditingAbstract(true)}
            title={t("activity.blocks.rewrite")}
          >
            {block.generatedAbstract ?? (
              <em className={styles.empty}>{t("activity.blocks.noAbstract")}</em>
            )}
          </p>
        )}
      </div>
      <footer className={styles.cardFoot}>
        <button
          type="button"
          className={styles.dangerBtn}
          onClick={() => onDelete(block.id)}
        >
          {t("activity.blocks.delete")}
        </button>
      </footer>
    </li>
  );
}
