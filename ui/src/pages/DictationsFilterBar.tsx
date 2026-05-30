// Filter bar for the Dictations page (Phase 1C Wave 1C.3 / ADR
// 0051 D1 / bead `mb-5ly5`). Two independent multi-select chip
// groups, one per retrieval axis the wave brief blesses:
//
//   * Entities -- typed identifiers (entity_id: number). The
//     selected chip displays the canonicalName + entityType from
//     the suggestion that added it; the wire payload is just the
//     id. Autocomplete via `kgListEntities(prefix)`.
//   * Tags     -- open-vocab tag_slug strings (no synthesised id;
//     1B schema reality). Autocomplete via `kgListTags(prefix)`.
//
// The category axis from PHASE-0-5-REPORT.md §7 is intentionally
// DROPPED this wave (data not persisted in 1B; future bead
// `mb-oji5` per the kickoff).
//
// The parent (Dictations.tsx) owns the filter state and the
// IPC-call effect; this component is a controlled view + a
// debounced autocomplete shell. We get the parent's onChange + the
// current selections in, render the chips + a popover-style
// suggestions list, and call back when the user adds/removes a
// chip. The parent then triggers `kgSearchEntries(filter)` via
// its own effect (mirrors the existing search-debounce pattern in
// Dictations.tsx).
//
// Accessibility: each input is a labelled <combobox>-style search,
// the selected chips have remove-button affordances with descriptive
// ARIA labels, and the suggestion list uses role="listbox" so
// screen readers announce it as the combobox popup it functionally
// is.

import { useCallback, useEffect, useId, useState } from "react";

import { Button } from "../components/primitives";
import { t } from "../i18n";
import { api } from "../lib/tauri";
import type { EntitySuggestion, TagSuggestion } from "../lib/types";

import styles from "./DictationsFilterBar.module.css";

// Wave brief specifies a 200 ms debounce on prefix queries -- short
// enough to feel responsive while typing, long enough to drop the
// fast-typist's intermediate strokes off the network.
const DEBOUNCE_MS = 200;

// Cap autocomplete results client-side too so a misconfigured
// server (or a future migration that lifts the default-50 cap)
// can't blow up the popover layout. The Rust default is already
// 50; this matches.
const AUTOCOMPLETE_LIMIT = 50;

/** Currently-selected entity. We keep canonicalName + entityType
 *  alongside the id so the chip can render without re-querying the
 *  server when the user navigates away and back. The wire payload
 *  to `kgSearchEntries` only uses `entityId`. */
export interface SelectedEntity {
  entityId: number;
  canonicalName: string;
  entityType: string;
}

interface Props {
  /** Currently-selected entity chips. */
  entities: SelectedEntity[];
  /** Currently-selected tag slugs. */
  tags: string[];
  /** Called when the user adds OR removes an entity. Receives the
   *  new full list. */
  onEntitiesChange: (next: SelectedEntity[]) => void;
  /** Called when the user adds OR removes a tag. Receives the new
   *  full list. */
  onTagsChange: (next: string[]) => void;
  /** Convenience callback for the "Clear filters" link. Parent is
   *  responsible for ALSO clearing its query state if applicable
   *  (the search input lives outside this component). */
  onClearAll: () => void;
}

export function DictationsFilterBar({
  entities,
  tags,
  onEntitiesChange,
  onTagsChange,
  onClearAll,
}: Props) {
  const hasAny = entities.length > 0 || tags.length > 0;
  return (
    <div className={styles.bar} role="group" aria-label={t("kg.filter.heading")}>
      <div className={styles.header}>
        <span className={styles.heading}>{t("kg.filter.heading")}</span>
        {hasAny ? (
          <Button
            variant="ghost"
            size="sm"
            onClick={onClearAll}
            ariaLabel={t("kg.filter.clear")}
          >
            {t("kg.filter.clear")}
          </Button>
        ) : null}
      </div>

      <EntityPicker entities={entities} onChange={onEntitiesChange} />
      <TagPicker tags={tags} onChange={onTagsChange} />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* EntityPicker -- combobox over kg_list_entities.                    */
/* ------------------------------------------------------------------ */

function EntityPicker({
  entities,
  onChange,
}: {
  entities: SelectedEntity[];
  onChange: (next: SelectedEntity[]) => void;
}) {
  const [query, setQuery] = useState("");
  const [suggestions, setSuggestions] = useState<EntitySuggestion[]>([]);
  const [open, setOpen] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const inputId = useId();
  const listboxId = useId();

  // Build the set of already-selected ids once per render so the
  // suggestion filter is O(1) per row. Re-suggesting an
  // already-selected entity would be confusing UX.
  const selectedIds = new Set(entities.map((e) => e.entityId));

  // Debounced prefix query. Empty prefix returns the global top
  // (server-side ranking), so the popover is informative the
  // moment the user focuses the input.
  useEffect(() => {
    if (!open) return;
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const rows = await api.kg_list_entities(
            query.trim() || undefined,
            AUTOCOMPLETE_LIMIT,
          );
          setSuggestions(rows);
          setLoadError(null);
        } catch (err) {
          setLoadError(String(err));
          setSuggestions([]);
        }
      })();
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [query, open]);

  const addEntity = useCallback(
    (s: EntitySuggestion) => {
      if (selectedIds.has(s.entityId)) return;
      onChange([
        ...entities,
        {
          entityId: s.entityId,
          canonicalName: s.canonicalName,
          entityType: s.entityType,
        },
      ]);
      setQuery("");
    },
    // selectedIds is derived from `entities`; depending on it
    // would force a new function identity every render. The
    // closure already captures the current `entities` so this is
    // safe.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [entities, onChange],
  );

  const removeEntity = useCallback(
    (entityId: number) => {
      onChange(entities.filter((e) => e.entityId !== entityId));
    },
    [entities, onChange],
  );

  const filteredSuggestions = suggestions.filter(
    (s) => !selectedIds.has(s.entityId),
  );

  return (
    <div className={styles.picker}>
      <label className={styles.pickerLabel} htmlFor={inputId}>
        {t("kg.filter.entities.label")}
      </label>

      <div className={styles.chipsRow}>
        {entities.map((e) => (
          <span key={e.entityId} className={styles.selectedChip}>
            <span className={styles.selectedChipText} title={e.entityType}>
              {e.canonicalName}
            </span>
            <button
              type="button"
              className={styles.removeBtn}
              onClick={() => removeEntity(e.entityId)}
              aria-label={t("kg.filter.entities.remove").replace(
                "{name}",
                e.canonicalName,
              )}
            >
              ×
            </button>
          </span>
        ))}

        <input
          id={inputId}
          type="search"
          className={styles.input}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onFocus={() => setOpen(true)}
          onBlur={() => {
            // Defer close so a click on a suggestion lands before
            // the popover dismounts. 120 ms is the smallest delay
            // that survives every browser's click ordering without
            // feeling sticky on a real user's tab-away.
            window.setTimeout(() => setOpen(false), 120);
          }}
          placeholder={t("kg.filter.entities.placeholder")}
          aria-label={t("kg.filter.entities.aria")}
          role="combobox"
          aria-expanded={open}
          aria-controls={listboxId}
          aria-autocomplete="list"
        />
      </div>

      {open ? (
        <ul
          id={listboxId}
          role="listbox"
          aria-label={t("kg.filter.suggestions.aria")}
          className={styles.popover}
        >
          {loadError ? (
            <li className={styles.popoverError} role="alert">
              {t("kg.filter.loadError").replace("{error}", loadError)}
            </li>
          ) : filteredSuggestions.length === 0 ? (
            <li className={styles.popoverEmpty}>
              {t("kg.filter.suggestions.empty")}
            </li>
          ) : (
            filteredSuggestions.map((s) => (
              <li
                key={s.entityId}
                role="option"
                aria-selected={false}
                className={styles.popoverRow}
                // Use onMouseDown so the click registers BEFORE the
                // input's onBlur closes the popover -- the
                // onClick-then-onBlur race otherwise dismisses the
                // popover and never adds the chip.
                onMouseDown={(e) => {
                  e.preventDefault();
                  addEntity(s);
                }}
              >
                <span className={styles.popoverRowName}>{s.canonicalName}</span>
                <span className={styles.popoverRowMeta}>
                  {s.entityType} · {s.mentionCount}
                </span>
              </li>
            ))
          )}
        </ul>
      ) : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* TagPicker -- combobox over kg_list_tags.                           */
/* Structurally identical to EntityPicker but over the open-vocab     */
/* tag_slug axis. Kept as a sibling rather than a parameterised       */
/* generic because the two pickers' UX will diverge in 1C.4 (concept  */
/* modal entry point on entities only) and the duplication today      */
/* makes that easy. ~60 lines is well under the cost of a clever      */
/* abstraction.                                                       */
/* ------------------------------------------------------------------ */

function TagPicker({
  tags,
  onChange,
}: {
  tags: string[];
  onChange: (next: string[]) => void;
}) {
  const [query, setQuery] = useState("");
  const [suggestions, setSuggestions] = useState<TagSuggestion[]>([]);
  const [open, setOpen] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const inputId = useId();
  const listboxId = useId();

  const selectedSlugs = new Set(tags);

  useEffect(() => {
    if (!open) return;
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const rows = await api.kg_list_tags(
            query.trim() || undefined,
            AUTOCOMPLETE_LIMIT,
          );
          setSuggestions(rows);
          setLoadError(null);
        } catch (err) {
          setLoadError(String(err));
          setSuggestions([]);
        }
      })();
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [query, open]);

  const addTag = useCallback(
    (slug: string) => {
      if (selectedSlugs.has(slug)) return;
      onChange([...tags, slug]);
      setQuery("");
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tags, onChange],
  );

  const removeTag = useCallback(
    (slug: string) => {
      onChange(tags.filter((s) => s !== slug));
    },
    [tags, onChange],
  );

  const filteredSuggestions = suggestions.filter(
    (s) => !selectedSlugs.has(s.tagSlug),
  );

  return (
    <div className={styles.picker}>
      <label className={styles.pickerLabel} htmlFor={inputId}>
        {t("kg.filter.tags.label")}
      </label>

      <div className={styles.chipsRow}>
        {tags.map((slug) => (
          <span key={slug} className={styles.selectedChip}>
            <span className={styles.selectedChipText}>
              {t("kg.chip.tagPrefix")}
              {slug}
            </span>
            <button
              type="button"
              className={styles.removeBtn}
              onClick={() => removeTag(slug)}
              aria-label={t("kg.filter.tags.remove").replace("{slug}", slug)}
            >
              ×
            </button>
          </span>
        ))}

        <input
          id={inputId}
          type="search"
          className={styles.input}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onFocus={() => setOpen(true)}
          onBlur={() => {
            window.setTimeout(() => setOpen(false), 120);
          }}
          placeholder={t("kg.filter.tags.placeholder")}
          aria-label={t("kg.filter.tags.aria")}
          role="combobox"
          aria-expanded={open}
          aria-controls={listboxId}
          aria-autocomplete="list"
        />
      </div>

      {open ? (
        <ul
          id={listboxId}
          role="listbox"
          aria-label={t("kg.filter.suggestions.aria")}
          className={styles.popover}
        >
          {loadError ? (
            <li className={styles.popoverError} role="alert">
              {t("kg.filter.loadError").replace("{error}", loadError)}
            </li>
          ) : filteredSuggestions.length === 0 ? (
            <li className={styles.popoverEmpty}>
              {t("kg.filter.suggestions.empty")}
            </li>
          ) : (
            filteredSuggestions.map((s) => (
              <li
                key={s.tagSlug}
                role="option"
                aria-selected={false}
                className={styles.popoverRow}
                onMouseDown={(e) => {
                  e.preventDefault();
                  addTag(s.tagSlug);
                }}
              >
                <span className={styles.popoverRowName}>
                  {t("kg.chip.tagPrefix")}
                  {s.tagSlug}
                </span>
                <span className={styles.popoverRowMeta}>{s.mentionCount}</span>
              </li>
            ))
          )}
        </ul>
      ) : null}
    </div>
  );
}
