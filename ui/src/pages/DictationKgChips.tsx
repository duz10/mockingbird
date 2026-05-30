// Per-row Knowledge Graph strip for the Dictations list.
//
// Phase 1C Wave 1C.3 / ADR 0051 D1 (per-row entity/tag display) +
// D8 (filing-state pill). Bead `mb-5ly5`.
//
// Composition: top-5 entity chips (server-ordered DESC by mention
// rank) + "+N more" overflow + same shape for tags + a single
// filing-state pill. The whole strip stays *visually inert* this
// wave -- chips are spans, not buttons. Click-to-add-to-filter
// graduates to interactive in Wave 1C.4 (concept modal,
// `mb-sx6p`) per the kickoff risk list.
//
// The parent (`DictationsList.tsx`) is the only caller and gates
// rendering on `kgGraphEnabled === true`. When the summary is
// missing for a row (e.g. legacy never-filed session pre-Phase-1B)
// we render NOTHING -- empty strips would add visual noise without
// signal. The component returns `null` in that case so the row
// height stays unchanged.

import type { MouseEvent } from "react";

import { t } from "../i18n";
import type { ActiveConcept, EntrySummary, FilingState } from "../lib/types";

import styles from "./DictationKgChips.module.css";

// Server orders entities + tags DESC by mention rank; we keep the
// top N inline and surface the overflow via a "+N more" hint.
// 5 is the kickoff's prescribed cap -- small enough to fit in a
// narrow left-pane row (~300px), large enough to be informative.
const TOP_N = 5;

interface Props {
  /** Batched per-row payload from `kg_entries_summary`. `undefined`
   *  when the row isn't in the summary map (legacy / never-filed
   *  session) -- we render `null` in that case. */
  summary: EntrySummary | undefined;
  /** Phase 1C Wave 1C.4 / mb-sx6p -- dispatched when a chip is
   *  clicked. Optional so existing call sites without modal
   *  plumbing (none today, but future surfaces e.g. MeetingDetail)
   *  can opt out and keep the chips visually inert. When omitted,
   *  chips render as inert spans (Wave 1C.3 behaviour). When
   *  present, chips render as native <button>s with
   *  click-to-open-concept + stopPropagation so the row's
   *  select-on-click doesn't fire underneath. */
  onConceptOpen?: (concept: ActiveConcept) => void;
}

export function DictationKgChips({ summary, onConceptOpen }: Props) {
  // Legacy / never-filed row: render nothing so the row height
  // doesn't shift between summary-present and summary-absent
  // states. A future wave may surface a "not yet indexed" pill;
  // 1C.3 deliberately keeps the off-corpus path silent.
  if (!summary) return null;

  // No-data short-circuit: zero entities, zero tags, no
  // user-visible pill needed (filingState === "done" or
  // "not_enqueued"). Skip the whole strip so empty rows stay
  // visually clean.
  const hasEntities = summary.entities.length > 0;
  const hasTags = summary.tags.length > 0;
  const pillText = filingPillText(summary.filingState);
  if (!hasEntities && !hasTags && !pillText) return null;

  const entitiesShown = summary.entities.slice(0, TOP_N);
  const entitiesOverflow = Math.max(
    0,
    summary.entities.length - entitiesShown.length,
  );
  const tagsShown = summary.tags.slice(0, TOP_N);
  const tagsOverflow = Math.max(0, summary.tags.length - tagsShown.length);

  return (
    <div className={styles.strip} aria-label={t("kg.filter.heading")}>
      {hasEntities ? (
        <div className={styles.group}>
          {entitiesShown.map((e) =>
            onConceptOpen ? (
              <button
                key={`e-${e.entityId}`}
                type="button"
                className={styles.entity}
                aria-label={t("kg.chip.entityOpenAria").replace(
                  "{name}",
                  e.canonicalName,
                )}
                title={e.entityType}
                onClick={(ev: MouseEvent<HTMLButtonElement>) => {
                  // stopPropagation so the parent row's
                  // onClick (select-this-session) doesn't fire
                  // under the chip click. Mirrors the kickoff's
                  // explicit guidance for 1C.4.
                  ev.stopPropagation();
                  onConceptOpen({
                    kind: "entity",
                    entityId: e.entityId,
                  });
                }}
                onKeyDown={(ev) => {
                  // The row swallows space + enter to trigger
                  // select-this-session; stop those keys before
                  // they bubble so the chip's native button
                  // activation wins inside its own focus scope.
                  if (ev.key === "Enter" || ev.key === " ") {
                    ev.stopPropagation();
                  }
                }}
              >
                {e.canonicalName}
              </button>
            ) : (
              <span
                key={`e-${e.entityId}`}
                className={styles.entity}
                aria-label={t("kg.chip.entityAria").replace(
                  "{name}",
                  e.canonicalName,
                )}
                // Use native title= for the entity type so a hover
                // surfaces person/place/thing without a custom
                // tooltip primitive. Matches the failed-filings
                // pattern in SettingsKgFailedFilings.
                title={e.entityType}
              >
                {e.canonicalName}
              </span>
            ),
          )}
          {entitiesOverflow > 0 ? (
            <span className={styles.overflow}>
              {t("kg.chip.more").replace("{count}", String(entitiesOverflow))}
            </span>
          ) : null}
        </div>
      ) : null}

      {hasTags ? (
        <div className={styles.group}>
          {tagsShown.map((tag) =>
            onConceptOpen ? (
              <button
                key={`t-${tag.tagSlug}`}
                type="button"
                className={styles.tag}
                aria-label={t("kg.chip.tagOpenAria").replace(
                  "{slug}",
                  tag.tagSlug,
                )}
                onClick={(ev: MouseEvent<HTMLButtonElement>) => {
                  ev.stopPropagation();
                  onConceptOpen({
                    kind: "tag",
                    tagSlug: tag.tagSlug,
                  });
                }}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter" || ev.key === " ") {
                    ev.stopPropagation();
                  }
                }}
              >
                {t("kg.chip.tagPrefix")}
                {tag.tagSlug}
              </button>
            ) : (
              <span
                key={`t-${tag.tagSlug}`}
                className={styles.tag}
                aria-label={t("kg.chip.tagAria").replace(
                  "{slug}",
                  tag.tagSlug,
                )}
              >
                {t("kg.chip.tagPrefix")}
                {tag.tagSlug}
              </span>
            ),
          )}
          {tagsOverflow > 0 ? (
            <span className={styles.overflow}>
              {t("kg.chip.more").replace("{count}", String(tagsOverflow))}
            </span>
          ) : null}
        </div>
      ) : null}

      {pillText ? (
        <FilingPill state={summary.filingState} text={pillText} />
      ) : null}
    </div>
  );
}

/** Mapping table for filing-state -> user-visible pill text.
 *  Returns `null` for `done` and `not_enqueued` (both render no
 *  pill per the kickoff brief). Centralised here so the FilingPill
 *  component and the parent's short-circuit see the same answer. */
function filingPillText(state: FilingState): string | null {
  switch (state) {
    case "pending":
      return t("kg.chip.filing.pending");
    case "processing":
      return t("kg.chip.filing.processing");
    case "failed":
      return t("kg.chip.filing.failed");
    case "done":
    case "not_enqueued":
      return null;
  }
}

// Centralised mapping table for filing-state -> presentation props
// of the FilingPill. Returning a literal object keeps the three
// fields strictly typed without TS's let-assignment narrowing
// caveats (which trip when the switch contains an early-return
// default branch).
function filingPillProps(
  state: FilingState,
): {
  ariaKey: string;
  // `string | undefined` because tsconfig has
  // `noUncheckedIndexedAccess: true` and CSS-module property
  // lookups widen to optional. React's className accepts
  // `undefined` (renders as no class), so this is safe to pass
  // straight through.
  toneClass: string | undefined;
  title?: string;
} | null {
  switch (state) {
    case "pending":
      return {
        ariaKey: "kg.chip.filing.pending.aria",
        toneClass: styles.pillPending,
      };
    case "processing":
      return {
        ariaKey: "kg.chip.filing.processing.aria",
        toneClass: styles.pillProcessing,
      };
    case "failed":
      return {
        ariaKey: "kg.chip.filing.failed.aria",
        toneClass: styles.pillFailed,
        title: t("kg.chip.filing.failed.tooltip"),
      };
    case "done":
    case "not_enqueued":
      // Unreachable: callers filter these via `filingPillText`
      // before mounting the pill. Returning null keeps the
      // exhaustive switch honest for future variants.
      return null;
  }
}

function FilingPill({ state, text }: { state: FilingState; text: string }) {
  const props = filingPillProps(state);
  if (!props) return null;
  return (
    <span
      className={`${styles.pill} ${props.toneClass}`}
      role="status"
      aria-label={t(props.ariaKey)}
      title={props.title}
    >
      {text}
    </span>
  );
}
