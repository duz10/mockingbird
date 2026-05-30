// Concept modal — entity + tag drill-down sheet.
//
// Phase 1C Wave 1C.4 / ADR 0051 D4 (`mb-sx6p`). Composes the
// design-system `Dialog` primitive, which uses the native <dialog>
// element under the hood and gets us focus trap, Escape-to-close,
// backdrop-click-to-close, and aria-modal semantics for free
// (verified by structural read of `Dialog.tsx` per Wave 1C.3
// risk-flag-1). We DON'T re-implement any of that here.
//
// Two modes, dispatched off the `concept` prop discriminator:
//   * `{ kind: "entity", entityId }`  -> `kgEntityDetail`
//   * `{ kind: "tag",    tagSlug   }` -> `kgTagDetail`
//
// The parent (`Dictations.tsx`) owns the `activeConcept` state
// and mounts a single instance of this component at the page
// level. The modal stays mounted with `open=false` between
// invocations so the next open doesn't flash a stale-then-cleared
// frame.
//
// `tagSlug` (not `tagId`) keys the tag mode — see the Rust-side
// deviation note in `commands/kg.rs::kg_tag_detail` + LESSONS P11.
// Open-vocab semantics: an unknown slug returns zero counts and
// empty `recentEntries` (not an error) — useful when the user
// opens the modal for a freshly-typed filter-bar slug before any
// dictation has been filed against it. The entity mode is
// stricter: an unknown id IS an error and surfaces a sonner toast.

import { useEffect, useState } from "react";
import { toast as sonnerToast } from "sonner";

import { Button, Spinner } from "../components/primitives";
import { Dialog } from "../design/components/Dialog";
import { t } from "../i18n";
import { formatTimestamp, truncate } from "../lib/format";
import { api } from "../lib/tauri";
import type {
  ActiveConcept,
  EntityDetail,
  EntryRef,
  TagDetail,
} from "../lib/types";

import styles from "./ConceptModal.module.css";

// Cap for canonical-name truncation in the title. The Rust side
// caps `EntryRef.title` at 80 (in mod.rs ENTRY_TITLE_MAX_CHARS) but
// imposes no cap on `EntityDetail.canonicalName` itself; we apply a
// modest UI-side cap so an extreme outlier doesn't blow out the
// modal frame. The native `title=` attribute still surfaces the
// full string on hover.
const TITLE_TRUNCATE = 80;

// Recent-entries fetch cap. The IPC defaults to 50 server-side
// when omitted; we pass undefined to inherit. The Rust comment is
// the source of truth -- keep this in sync.
const RECENT_LIMIT: number | undefined = undefined;

interface Props {
  concept: ActiveConcept | null;
  onClose: () => void;
  /** Fired when the user clicks a row in `recentEntries`. The parent
   *  closes the modal (via `onClose`) AND scrolls/selects that
   *  session in the detail pane. We split the two callbacks so the
   *  parent can sequence them; an alternative would be one
   *  `onSelectAndClose(id)` API, but the current shape keeps each
   *  responsibility addressable on its own line in the parent. */
  onSelectEntry: (entryId: number) => void;
}

// Internal load state. `null` data + non-null error => error state.
// `null` data + null error => loading.
interface ModalState {
  entity: EntityDetail | null;
  tag: TagDetail | null;
  error: string | null;
}

const EMPTY_STATE: ModalState = { entity: null, tag: null, error: null };

export function ConceptModal({ concept, onClose, onSelectEntry }: Props) {
  // Render-state cache. Cleared on every open (effect below).
  const [state, setState] = useState<ModalState>(EMPTY_STATE);

  // Fetch on every (re-)open. The `concept` ref identity changes
  // when the parent dispatches a new open call, so the effect
  // fires per-open without a separate "isOpen" prop. Cancellation
  // guards against a fast open->close->open race.
  useEffect(() => {
    if (!concept) {
      // Modal is closed -- wipe the cache so the next open
      // doesn't flash stale content. The `open` prop on Dialog
      // is null-derived below, so the user won't see this empty
      // state during the close animation.
      setState(EMPTY_STATE);
      return;
    }
    let cancelled = false;
    setState(EMPTY_STATE);
    void (async () => {
      try {
        if (concept.kind === "entity") {
          const d = await api.kg_entity_detail(concept.entityId, RECENT_LIMIT);
          if (!cancelled) setState({ entity: d, tag: null, error: null });
        } else {
          const d = await api.kg_tag_detail(concept.tagSlug, RECENT_LIMIT);
          if (!cancelled) setState({ entity: null, tag: d, error: null });
        }
      } catch (err) {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        setState({
          entity: null,
          tag: null,
          error: message,
        });
        // Toast the error so the user gets a tangible cue even if
        // they dismiss the modal before reading the inline error
        // block. Matches the Wave 1C.3 filter-load-error pattern.
        sonnerToast.error(
          t("kg.concept.loadError").replace("{error}", message),
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [concept]);

  // Resolved view-model. Either entity or tag is non-null in the
  // loaded state; both null = loading or error.
  const isOpen = concept !== null;
  const title = resolveTitle(concept, state);
  const ariaLabel = title || t("kg.concept.loading");

  return (
    <Dialog
      open={isOpen}
      onClose={onClose}
      ariaLabel={ariaLabel}
      actions={
        <Button
          variant="ghost"
          size="sm"
          onClick={onClose}
          ariaLabel={t("kg.concept.close.aria")}
        >
          {t("kg.concept.close")}
        </Button>
      }
    >
      <div className={styles.shell}>
        {/* Header. Title is the concept name (truncated, with a
            full-text title= tooltip). Entity mode adds a small
            type badge under the title; tag mode renders the slug
            with the `#` prefix from i18n. */}
        <ConceptHeader concept={concept} state={state} title={title} />

        {/* Body. Three exclusive states: error block, loading
            spinner, or loaded payload. */}
        {state.error ? (
          <div className={styles.errorRow} role="alert">
            {t("kg.concept.loadError").replace("{error}", state.error)}
          </div>
        ) : !state.entity && !state.tag ? (
          <div className={styles.loadingRow}>
            <Spinner label={t("kg.concept.loading")} />
          </div>
        ) : (
          <ConceptBody
            state={state}
            onSelectEntry={(id) => {
              // Close-then-select so the parent's per-row scroll
              // happens AFTER the dialog unmounts (avoids a
              // jarring focus-restore-mid-scroll).
              onClose();
              onSelectEntry(id);
            }}
          />
        )}
      </div>
    </Dialog>
  );
}

/* ------------------------------------------------------------------ */
/* Header                                                              */
/* ------------------------------------------------------------------ */

function ConceptHeader({
  concept,
  state,
  title,
}: {
  concept: ActiveConcept | null;
  state: ModalState;
  title: string;
}) {
  // Entity-type badge is only meaningful in entity mode; the
  // tag-mode header just shows the `#slug` title.
  const entityType =
    concept?.kind === "entity" && state.entity ? state.entity.entityType : null;
  // Title fallback while loading: in entity mode we show nothing
  // (the title needs the canonical_name we don't have yet); in tag
  // mode we already have the slug from the discriminator. Using
  // `title || ""` collapses to the empty string, which the CSS
  // ellipsis handles cleanly.
  return (
    <div className={styles.header}>
      <h2 className={styles.title} title={title || undefined}>
        {title}
      </h2>
      {entityType ? (
        <span className={styles.entityTypeBadge} aria-label={t("kg.concept.entityType")}>
          {entityType}
        </span>
      ) : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Body                                                                */
/* ------------------------------------------------------------------ */

function ConceptBody({
  state,
  onSelectEntry,
}: {
  state: ModalState;
  onSelectEntry: (entryId: number) => void;
}) {
  // Narrow to a single shape so the rest of this fn can ignore
  // the entity/tag discriminator. Both EntityDetail and TagDetail
  // share the (mentionCount, totalEntries, recentEntries) trio;
  // only `aliases` is entity-specific.
  const loaded: {
    mentionCount: number;
    totalEntries: number;
    recentEntries: EntryRef[];
    aliases: string[];
    isEntity: boolean;
  } | null = state.entity
    ? {
        mentionCount: state.entity.mentionCount,
        totalEntries: state.entity.totalEntries,
        recentEntries: state.entity.recentEntries,
        aliases: state.entity.aliases,
        isEntity: true,
      }
    : state.tag
      ? {
          mentionCount: state.tag.mentionCount,
          totalEntries: state.tag.totalEntries,
          recentEntries: state.tag.recentEntries,
          aliases: [],
          isEntity: false,
        }
      : null;

  // Caller only mounts this component when state has loaded
  // payload (see the parent's conditional render). The null
  // branch here is defensive -- it should be unreachable.
  if (!loaded) return null;
  const { mentionCount, totalEntries, recentEntries, aliases, isEntity } = loaded;

  return (
    <>
      <div className={styles.counters}>
        <span>{formatMentionCount(mentionCount)}</span>
        <span>{formatTotalEntries(totalEntries)}</span>
      </div>

      {aliases.length > 0 ? (
        <div className={styles.aliasesGroup}>
          <span className={styles.aliasesLabel}>{t("kg.concept.aliases")}</span>
          <div className={styles.aliasesList}>
            {aliases.map((a) => (
              <span key={a} className={styles.aliasChip}>
                {a}
              </span>
            ))}
          </div>
        </div>
      ) : null}

      <div className={styles.recentSection}>
        <h3 className={styles.recentHeading}>
          {t("kg.concept.recentHeading")}
        </h3>
        {recentEntries.length === 0 ? (
          <span className={styles.recentEmpty}>
            {isEntity
              ? t("kg.concept.recentEmpty")
              : t("kg.concept.tagPlaceholder")}
          </span>
        ) : (
          <div className={styles.recentList} role="list">
            {recentEntries.map((row) => (
              <RecentRow
                key={row.entryId}
                row={row}
                onClick={() => onSelectEntry(row.entryId)}
              />
            ))}
          </div>
        )}
      </div>

      <div className={styles.footer}>
        <span className={styles.footerStat}>
          {formatTotalEntries(totalEntries)}
        </span>
      </div>
    </>
  );
}

function RecentRow({
  row,
  onClick,
}: {
  row: EntryRef;
  onClick: () => void;
}) {
  // The row is a button so it inherits the global focus-visible
  // outline (design/global.css's `button:focus-visible` rule);
  // we don't add a custom outline override here.
  return (
    <button
      type="button"
      className={styles.recentRow}
      onClick={onClick}
      aria-label={t("kg.concept.entryAria").replace("{title}", row.title)}
      role="listitem"
    >
      <span className={styles.recentRowTitle} title={row.title}>
        {truncate(row.title, 96)}
      </span>
      <span className={styles.recentRowMeta}>
        {formatTimestamp(row.capturedIso)}
      </span>
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* View-model helpers                                                  */
/* ------------------------------------------------------------------ */

function resolveTitle(
  concept: ActiveConcept | null,
  state: ModalState,
): string {
  if (!concept) return "";
  if (concept.kind === "entity") {
    if (!state.entity) return "";
    return truncate(
      t("kg.concept.entityTitle").replace(
        "{name}",
        state.entity.canonicalName,
      ),
      TITLE_TRUNCATE,
    );
  }
  // Tag mode: we already have the slug from the discriminator,
  // so render it immediately (even before the IPC resolves). The
  // counters update once the IPC lands.
  return truncate(
    t("kg.concept.tagTitle").replace("{slug}", concept.tagSlug),
    TITLE_TRUNCATE,
  );
}

function formatMentionCount(n: number): string {
  return n === 1
    ? t("kg.concept.mentions.one")
    : t("kg.concept.mentions").replace("{count}", String(n));
}

function formatTotalEntries(n: number): string {
  return n === 1
    ? t("kg.concept.totalEntries.one")
    : t("kg.concept.totalEntries").replace("{count}", String(n));
}
