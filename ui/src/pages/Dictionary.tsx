// Dictionary CRUD.
//
// Schema stays flat — one row per `(term, canonical)` pair — but the
// UI groups by `canonical` so a single conceptual "entry" (Huly with
// misspellings Hooli / Hooly / Huley) reads as one card with chips
// instead of four rows that look unrelated. Search filters before
// grouping, so typing "Hooli" surfaces the whole Huly card.
//
// Rows with `canonical IS NULL` are "proper noun" mode — the LLM is
// just being told the word exists. Each becomes a single-entry group
// keyed by its own term.
//
// No client-side virtualization. Real-world dictionaries cap out in
// the low hundreds; we'll graduate to react-window when someone shows
// up with 10k entries. YAGNI.
//
// History:
//   - mb-t75k surfaced the always-visible add panel + the from-
//     dictation modal.
//   - mb-9x33 (this iteration) groups rows by canonical and replaces
//     both add surfaces with the shared <DictionaryEntryForm>.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  Button,
  EmptyState,
  PageHeader,
} from "../components/primitives";
import { Dialog } from "../design/components/Dialog";
import {
  BookIcon,
  PencilIcon,
  PlusIcon,
  SearchIcon,
  TrashIcon,
} from "../design/Icon";
import { t } from "../i18n";
import { formatRelative, formatCount } from "../lib/format";
import { api } from "../lib/tauri";
import type { DictionaryEntry } from "../lib/types";

import {
  DictionaryEntryForm,
  EMPTY_FORM,
  type DictionaryFormState,
} from "./DictionaryEntryForm";
import styles from "./Dictionary.module.css";

/* ------------------------------------------------------------------ */
/* Grouping                                                            */
/* ------------------------------------------------------------------ */

/** A logical dictionary entry as the UI presents it. One canonical
 *  term, zero or more variant (misspelling) rows. */
interface Group {
  /** Stable key for React lists. */
  key: string;
  /** The user-visible canonical term. */
  canonical: string;
  /** True when every child row has `canonical IS NULL`. (Always one
   *  child in this case.) */
  isProperNoun: boolean;
  /** Underlying rows. For canonical-bearing groups every child shares
   *  the same `canonical`; for proper-noun groups there's exactly one
   *  child whose `canonical IS NULL`. */
  children: DictionaryEntry[];
  /** Sum of children's useCount. */
  useCount: number;
  /** Max of children's lastUsedAt (ISO), or null if none. */
  lastUsedAt: string | null;
  /** First non-null child appContext, or null. */
  appContext: string | null;
  /** "user" / "learned" / "import" if all children agree, else "mixed". */
  source: DictionaryEntry["source"] | "mixed";
}

function groupRows(rows: DictionaryEntry[]): Group[] {
  const byCanonical = new Map<string, DictionaryEntry[]>();
  const properNouns: DictionaryEntry[] = [];

  for (const row of rows) {
    if (row.canonical) {
      const list = byCanonical.get(row.canonical) ?? [];
      list.push(row);
      byCanonical.set(row.canonical, list);
    } else {
      properNouns.push(row);
    }
  }

  const groups: Group[] = [];

  for (const [canonical, children] of byCanonical) {
    groups.push(buildGroup(canonical, children, /*properNoun*/ false));
  }
  for (const row of properNouns) {
    groups.push(buildGroup(row.term, [row], /*properNoun*/ true));
  }

  // Sort by useCount desc, then canonical asc. Most-used floats top.
  groups.sort((a, b) => {
    if (b.useCount !== a.useCount) return b.useCount - a.useCount;
    return a.canonical.localeCompare(b.canonical);
  });

  return groups;
}

function buildGroup(
  canonical: string,
  children: DictionaryEntry[],
  isProperNoun: boolean,
): Group {
  const useCount = children.reduce((acc, r) => acc + r.useCount, 0);

  let lastUsedAt: string | null = null;
  for (const r of children) {
    if (r.lastUsedAt && (!lastUsedAt || r.lastUsedAt > lastUsedAt)) {
      lastUsedAt = r.lastUsedAt;
    }
  }

  const appContext = children.find((r) => r.appContext)?.appContext ?? null;

  const sources = new Set(children.map((r) => r.source));
  const source: Group["source"] =
    sources.size === 1 ? (children[0]?.source ?? "user") : "mixed";

  return {
    key: isProperNoun ? `pn:${children[0]?.id ?? canonical}` : `c:${canonical}`,
    canonical,
    isProperNoun,
    children,
    useCount,
    lastUsedAt,
    appContext,
    source,
  };
}

/* ------------------------------------------------------------------ */
/* Page                                                                */
/* ------------------------------------------------------------------ */

export function DictionaryPage() {
  const [rows, setRows] = useState<DictionaryEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [editingKey, setEditingKey] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const r = await api.list_dictionary();
    setRows(r);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Filter at the row level, then expand any partial group-match back
  // to all of its siblings. This way searching "Hooli" surfaces the
  // whole Huly group (canonical card + all three misspelling pills),
  // not just the one row whose term happened to match.
  const groups = useMemo(() => {
    if (!rows) return null;
    const q = query.trim().toLowerCase();
    if (!q) return groupRows(rows);

    const matchingCanonicals = new Set<string>();
    const matchingProperNounIds = new Set<number>();
    for (const r of rows) {
      const hit =
        r.term.toLowerCase().includes(q) ||
        (r.canonical?.toLowerCase().includes(q) ?? false) ||
        (r.appContext?.toLowerCase().includes(q) ?? false);
      if (!hit) continue;
      if (r.canonical) matchingCanonicals.add(r.canonical);
      else matchingProperNounIds.add(r.id);
    }

    const expanded = rows.filter((r) => {
      if (r.canonical) return matchingCanonicals.has(r.canonical);
      return matchingProperNounIds.has(r.id);
    });
    return groupRows(expanded);
  }, [rows, query]);

  const editingGroup = useMemo(() => {
    if (!editingKey || !groups) return null;
    return groups.find((g) => g.key === editingKey) ?? null;
  }, [editingKey, groups]);

  return (
    <>
      <PageHeader
        title={t("dictionary.title")}
        subtitle={t("dictionary.subtitle")}
      />

      <div className={styles.shell}>
        <AddTermPanel onAdded={refresh} />

        <div className={styles.toolbar}>
          <label className={styles.searchBox}>
            <span className={styles.searchIcon}>
              <SearchIcon size={16} />
            </span>
            <input
              className={styles.searchInput}
              type="search"
              placeholder={t("dictionary.search.placeholder")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              aria-label={t("dictionary.search.placeholder")}
            />
          </label>
        </div>

        {rows === null ? null : rows.length === 0 ? (
          <EmptyState
            icon={<BookIcon size={32} />}
            title={t("dictionary.empty")}
          />
        ) : (
          <div className={styles.groupGrid}>
            {(groups ?? []).map((group) => (
              <GroupCard
                key={group.key}
                group={group}
                onEdit={() => setEditingKey(group.key)}
                onDeleted={refresh}
              />
            ))}
          </div>
        )}
      </div>

      <EditEntryDialog
        group={editingGroup}
        open={editingGroup !== null}
        onClose={() => setEditingKey(null)}
        onSaved={refresh}
      />
    </>
  );
}

/* ------------------------------------------------------------------ */
/* AddTermPanel — inline form on the Dictionary page.                  */
/* ------------------------------------------------------------------ */

function AddTermPanel({ onAdded }: { onAdded: () => Promise<void> | void }) {
  const [form, setForm] = useState<DictionaryFormState>(EMPTY_FORM);
  const [busy, setBusy] = useState(false);

  const submit = useCallback(async () => {
    const canonical = form.canonical.trim();
    if (!canonical || busy) return;
    setBusy(true);
    try {
      await submitNewGroup({
        canonical,
        variants: form.variants,
        appContext: form.appContext.trim() || null,
      });
      setForm(EMPTY_FORM);
      await onAdded();
    } finally {
      setBusy(false);
    }
  }, [form, busy, onAdded]);

  return (
    <section className={styles.addPanel} aria-label={t("dictionary.add.heading")}>
      <h3 className={styles.addPanelTitle}>{t("dictionary.add.heading")}</h3>
      <DictionaryEntryForm value={form} onChange={setForm} />
      <div className={styles.addAction}>
        <Button
          variant="primary"
          onClick={() => void submit()}
          ariaLabel={t("dictionary.add")}
          disabled={busy || form.canonical.trim().length === 0}
        >
          <PlusIcon size={12} />
          {t("dictionary.add")}
        </Button>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* GroupCard — one logical entry as a card with optional chips.        */
/* ------------------------------------------------------------------ */

function GroupCard({
  group,
  onEdit,
  onDeleted,
}: {
  group: Group;
  onEdit: () => void;
  onDeleted: () => Promise<void> | void;
}) {
  const [busy, setBusy] = useState(false);

  async function handleDelete() {
    if (busy) return;
    if (!window.confirm(t("dictionary.group.confirmDelete"))) return;
    setBusy(true);
    try {
      await Promise.all(
        group.children.map((c) => api.delete_dictionary_entry(c.id)),
      );
      await onDeleted();
    } finally {
      setBusy(false);
    }
  }

  const sourceClass =
    group.source === "user"
      ? styles.sourceUser
      : group.source === "learned"
        ? styles.sourceLearned
        : group.source === "import"
          ? styles.sourceImport
          : styles.sourceMixed;

  const sourceLabel =
    group.source === "mixed" ? t("dictionary.group.mixedSource") : group.source;

  const usedText = t("dictionary.group.usedTotal").replace(
    "{count}",
    formatCount(group.useCount),
  );
  const lastUsedText = group.lastUsedAt
    ? t("dictionary.group.lastUsed").replace(
        "{when}",
        formatRelative(group.lastUsedAt),
      )
    : t("dictionary.group.neverUsed");

  return (
    <article className={styles.groupCard}>
      <header className={styles.groupHeader}>
        <div className={styles.groupTitleWrap}>
          <h3 className={styles.groupCanonical}>{group.canonical}</h3>
          {group.isProperNoun ? (
            <span className={styles.properNounTag}>
              {t("dictionary.group.properNoun")}
            </span>
          ) : null}
          <span className={`${styles.sourceBadge} ${sourceClass}`}>
            {sourceLabel}
          </span>
        </div>
        <div className={styles.groupActions}>
          <Button size="sm" onClick={onEdit} ariaLabel="Edit">
            <PencilIcon size={12} />
            Edit
          </Button>
          <Button
            size="sm"
            variant="danger"
            onClick={() => void handleDelete()}
            ariaLabel={t("common.delete")}
            disabled={busy}
          >
            <TrashIcon size={12} />
          </Button>
        </div>
      </header>

      {group.isProperNoun ? null : (
        <div className={styles.variantsRow}>
          <span className={styles.variantsLabel}>
            {t("dictionary.group.misspellings")}
          </span>
          <div className={styles.variantsList}>
            {group.children.map((c) => (
              <span key={c.id} className={styles.variantPill}>
                {c.term}
              </span>
            ))}
          </div>
        </div>
      )}

      <footer className={styles.groupFooter}>
        <span>{usedText}</span>
        <span aria-hidden> · </span>
        <span>{lastUsedText}</span>
        {group.appContext ? (
          <>
            <span aria-hidden> · </span>
            <span className={styles.appContext}>{group.appContext}</span>
          </>
        ) : null}
      </footer>
    </article>
  );
}

/* ------------------------------------------------------------------ */
/* EditEntryDialog — reconcile add/remove/rename against existing rows.*/
/* ------------------------------------------------------------------ */

function EditEntryDialog({
  group,
  open,
  onClose,
  onSaved,
}: {
  group: Group | null;
  open: boolean;
  onClose: () => void;
  onSaved: () => Promise<void> | void;
}) {
  const [form, setForm] = useState<DictionaryFormState>(EMPTY_FORM);
  const [busy, setBusy] = useState(false);

  // Re-seed the form whenever the dialog opens against a (different)
  // group. The Dialog component preserves the dialog DOM across
  // re-opens, so we have to reset state explicitly.
  useEffect(() => {
    if (open && group) {
      setForm({
        canonical: group.canonical,
        variants: group.isProperNoun
          ? []
          : group.children.map((c) => c.term),
        appContext: group.appContext ?? "",
      });
      setBusy(false);
    }
  }, [open, group]);

  async function handleSave() {
    if (!group || busy) return;
    const canonical = form.canonical.trim();
    if (!canonical) return;

    setBusy(true);
    try {
      await reconcileEdit(group, {
        canonical,
        variants: form.variants,
        appContext: form.appContext.trim() || null,
      });
      await onSaved();
      onClose();
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("dictionary.editDialog.title")}
      ariaLabel={t("dictionary.editDialog.title")}
      actions={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            variant="primary"
            onClick={() => void handleSave()}
            disabled={busy || form.canonical.trim().length === 0}
          >
            {t("common.save")}
          </Button>
        </>
      }
    >
      <DictionaryEntryForm
        value={form}
        onChange={setForm}
        autoFocusCanonical
      />
    </Dialog>
  );
}

/* ------------------------------------------------------------------ */
/* Write helpers                                                       */
/* ------------------------------------------------------------------ */

interface SubmitInput {
  canonical: string;
  variants: string[];
  appContext: string | null;
}

/** Insert a brand-new group. If `variants` is empty we insert one
 *  proper-noun row (`term = canonical, canonical = null`); otherwise
 *  N rows sharing `canonical`. */
async function submitNewGroup({
  canonical,
  variants,
  appContext,
}: SubmitInput): Promise<void> {
  if (variants.length === 0) {
    await api.upsert_dictionary_entry({
      term: canonical,
      canonical: null,
      source: "user",
      confidence: 1.0,
      appContext,
    });
    return;
  }
  await Promise.all(
    variants.map((variant) =>
      api.upsert_dictionary_entry({
        term: variant,
        canonical,
        source: "user",
        confidence: 1.0,
        appContext,
      }),
    ),
  );
}

/** Reconcile an edit against a group: delete removed variants, insert
 *  new ones, update `canonical` on rows that stayed when the user
 *  renamed the group. Preserves per-row `term`, `source`, `confidence`,
 *  `useCount`, `lastUsedAt` (the Rust side keeps the audit-tracked
 *  counters intact on canonical-only updates). */
async function reconcileEdit(
  group: Group,
  next: SubmitInput,
): Promise<void> {
  const ops: Promise<unknown>[] = [];
  const canonicalChanged = !group.isProperNoun && next.canonical !== group.canonical;
  const appContextChanged = next.appContext !== group.appContext;

  // Edge case: user emptied all variants on a previously-canonical
  // group. Collapse the entire group to proper-noun mode (delete all
  // children, insert one canonical=null row). The pinned `appContext`
  // rides along to the new row.
  if (!group.isProperNoun && next.variants.length === 0) {
    for (const child of group.children) {
      ops.push(api.delete_dictionary_entry(child.id));
    }
    ops.push(
      api.upsert_dictionary_entry({
        term: next.canonical,
        canonical: null,
        source: "user",
        confidence: 1.0,
        appContext: next.appContext,
      }),
    );
    await Promise.all(ops);
    return;
  }

  // Edge case: previously proper-noun, now has variants. Delete the
  // singleton row and insert N variant rows with shared canonical.
  if (group.isProperNoun && next.variants.length > 0) {
    for (const child of group.children) {
      ops.push(api.delete_dictionary_entry(child.id));
    }
    for (const variant of next.variants) {
      ops.push(
        api.upsert_dictionary_entry({
          term: variant,
          canonical: next.canonical,
          source: "user",
          confidence: 1.0,
          appContext: next.appContext,
        }),
      );
    }
    await Promise.all(ops);
    return;
  }

  // Proper-noun → proper-noun: just patch term + appContext on the
  // single child row.
  if (group.isProperNoun) {
    const only = group.children[0];
    if (!only) return;
    ops.push(
      api.upsert_dictionary_entry({
        id: only.id,
        term: next.canonical,
        canonical: null,
        source: only.source,
        confidence: only.confidence,
        appContext: next.appContext,
      }),
    );
    await Promise.all(ops);
    return;
  }

  // Canonical → canonical. Three buckets across the variant set:
  //   - stayed (in both old and new): no-op unless canonical/appContext
  //     changed, in which case patch them in place to preserve counters.
  //   - removed (in old, not in new): delete by id.
  //   - added  (in new, not in old): insert.
  const oldByTerm = new Map<string, DictionaryEntry>();
  for (const child of group.children) {
    oldByTerm.set(child.term.toLowerCase(), child);
  }
  const newLower = new Set(next.variants.map((v) => v.toLowerCase()));

  // Removed.
  for (const child of group.children) {
    if (!newLower.has(child.term.toLowerCase())) {
      ops.push(api.delete_dictionary_entry(child.id));
    }
  }

  for (const variant of next.variants) {
    const existing = oldByTerm.get(variant.toLowerCase());
    if (existing) {
      if (canonicalChanged || appContextChanged) {
        ops.push(
          api.upsert_dictionary_entry({
            id: existing.id,
            term: existing.term,
            canonical: next.canonical,
            source: existing.source,
            confidence: existing.confidence,
            appContext: next.appContext,
          }),
        );
      }
      // else: untouched. Don't churn useCount / lastUsedAt.
    } else {
      ops.push(
        api.upsert_dictionary_entry({
          term: variant,
          canonical: next.canonical,
          source: "user",
          confidence: 1.0,
          appContext: next.appContext,
        }),
      );
    }
  }

  await Promise.all(ops);
}
