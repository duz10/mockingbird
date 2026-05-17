// Dictionary CRUD. Add-row inline at the top, list below with edit
// in place (term + canonical only — source/confidence are managed by
// the learning loop) and delete via trash icon.
//
// No client-side virtualization yet. Real-world dictionaries cap out
// in the low hundreds; we'll graduate to react-window when someone
// shows up with 10k entries. YAGNI.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  Button,
  EmptyState,
  PageHeader,
} from "../components/primitives";
import { BookIcon, CheckIcon, PlusIcon, SearchIcon, TrashIcon, XIcon } from "../design/Icon";
import { t } from "../i18n";
import { formatRelative, formatCount } from "../lib/format";
import { api } from "../lib/tauri";
import type { DictionaryEntry } from "../lib/types";

import styles from "./Dictionary.module.css";

export function DictionaryPage() {
  const [rows, setRows] = useState<DictionaryEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [editId, setEditId] = useState<number | null>(null);
  const [draft, setDraft] = useState<{ term: string; canonical: string; appContext: string }>(
    { term: "", canonical: "", appContext: "" },
  );

  const refresh = useCallback(async () => {
    const r = await api.list_dictionary();
    setRows(r);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const filtered = useMemo(() => {
    if (!rows) return null;
    const q = query.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter(
      (r) =>
        r.term.toLowerCase().includes(q) ||
        (r.canonical?.toLowerCase().includes(q) ?? false) ||
        (r.appContext?.toLowerCase().includes(q) ?? false),
    );
  }, [rows, query]);

  const handleAdd = useCallback(async () => {
    const term = draft.term.trim();
    if (!term) return;
    await api.upsert_dictionary_entry({
      term,
      canonical: draft.canonical.trim() || null,
      source: "user",
      confidence: 1.0,
      appContext: draft.appContext.trim() || null,
    });
    setDraft({ term: "", canonical: "", appContext: "" });
    await refresh();
  }, [draft, refresh]);

  const handleSaveEdit = useCallback(
    async (row: DictionaryEntry, patch: Partial<DictionaryEntry>) => {
      // The TS helper accepts an optional `id`. When present, the
      // Rust side UPDATE-s in place; otherwise INSERT.
      await api.upsert_dictionary_entry({
        id: row.id,
        term: patch.term ?? row.term,
        canonical: patch.canonical ?? row.canonical,
        source: row.source,
        confidence: row.confidence,
        appContext: patch.appContext ?? row.appContext,
      });
      setEditId(null);
      await refresh();
    },
    [refresh],
  );

  const handleDelete = useCallback(
    async (id: number) => {
      if (!window.confirm(t("dictionary.column.term") + " — " + t("common.confirm") + "?"))
        return;
      await api.delete_dictionary_entry(id);
      await refresh();
    },
    [refresh],
  );

  return (
    <>
      <PageHeader
        title={t("dictionary.title")}
        subtitle={t("dictionary.subtitle")}
      />

      <div className={styles.shell}>
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
          <div className={styles.tableWrap}>
            <table className={styles.table}>
              <thead>
                <tr>
                  <th>{t("dictionary.column.term")}</th>
                  <th>{t("dictionary.column.canonical")}</th>
                  <th>{t("dictionary.column.source")}</th>
                  <th>{t("dictionary.column.appContext")}</th>
                  <th>{t("dictionary.column.useCount")}</th>
                  <th>{t("dictionary.column.lastUsed")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                <tr className={styles.addRow}>
                  <td>
                    <input
                      className={styles.inlineInput}
                      value={draft.term}
                      onChange={(e) =>
                        setDraft({ ...draft, term: e.target.value })
                      }
                      placeholder="new term"
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleAdd();
                      }}
                      aria-label="New term"
                    />
                  </td>
                  <td>
                    <input
                      className={styles.inlineInput}
                      value={draft.canonical}
                      onChange={(e) =>
                        setDraft({ ...draft, canonical: e.target.value })
                      }
                      placeholder="canonical form (optional)"
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleAdd();
                      }}
                      aria-label="Canonical form"
                    />
                  </td>
                  <td>
                    <span className={`${styles.sourceBadge} ${styles.sourceUser}`}>
                      user
                    </span>
                  </td>
                  <td>
                    <input
                      className={styles.inlineInput}
                      value={draft.appContext}
                      onChange={(e) =>
                        setDraft({ ...draft, appContext: e.target.value })
                      }
                      placeholder="app context (optional)"
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleAdd();
                      }}
                      aria-label="App context"
                    />
                  </td>
                  <td colSpan={2} />
                  <td>
                    <div className={styles.cellActions}>
                      <Button
                        size="sm"
                        variant="primary"
                        onClick={handleAdd}
                        ariaLabel={t("dictionary.add")}
                      >
                        <PlusIcon size={12} />
                        {t("dictionary.add")}
                      </Button>
                    </div>
                  </td>
                </tr>
                {(filtered ?? []).map((row) => (
                  <DictRow
                    key={row.id}
                    row={row}
                    isEditing={editId === row.id}
                    onEdit={() => setEditId(row.id)}
                    onCancelEdit={() => setEditId(null)}
                    onSave={(patch) => void handleSaveEdit(row, patch)}
                    onDelete={() => void handleDelete(row.id)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </>
  );
}

function DictRow({
  row,
  isEditing,
  onEdit,
  onCancelEdit,
  onSave,
  onDelete,
}: {
  row: DictionaryEntry;
  isEditing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onSave: (patch: Partial<DictionaryEntry>) => void;
  onDelete: () => void;
}) {
  const [term, setTerm] = useState(row.term);
  const [canonical, setCanonical] = useState(row.canonical ?? "");
  const [appContext, setAppContext] = useState(row.appContext ?? "");

  // Sync draft when the row changes (e.g., re-fetched).
  useEffect(() => {
    setTerm(row.term);
    setCanonical(row.canonical ?? "");
    setAppContext(row.appContext ?? "");
  }, [row.id, row.term, row.canonical, row.appContext]);

  const sourceClass =
    row.source === "user"
      ? styles.sourceUser
      : row.source === "learned"
        ? styles.sourceLearned
        : styles.sourceImport;

  return (
    <tr>
      <td className={styles.cellMono}>
        {isEditing ? (
          <input
            className={styles.inlineInput}
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            aria-label="Term"
          />
        ) : (
          row.term
        )}
      </td>
      <td className={styles.cellMono}>
        {isEditing ? (
          <input
            className={styles.inlineInput}
            value={canonical}
            onChange={(e) => setCanonical(e.target.value)}
            aria-label="Canonical"
          />
        ) : (
          row.canonical ?? "—"
        )}
      </td>
      <td>
        <span className={`${styles.sourceBadge} ${sourceClass}`}>
          {row.source}
        </span>
        {row.source !== "user" ? (
          <span
            className={styles.confidenceBar}
            title={`confidence ${(row.confidence * 100).toFixed(0)}%`}
          >
            <span
              className={styles.confidenceFill}
              style={{ width: `${Math.round(row.confidence * 100)}%` }}
            />
          </span>
        ) : null}
      </td>
      <td className={styles.cellMono}>
        {isEditing ? (
          <input
            className={styles.inlineInput}
            value={appContext}
            onChange={(e) => setAppContext(e.target.value)}
            aria-label="App context"
          />
        ) : (
          row.appContext ?? "—"
        )}
      </td>
      <td className={styles.cellCount}>{formatCount(row.useCount)}</td>
      <td className={styles.cellCount}>
        {row.lastUsedAt ? formatRelative(row.lastUsedAt) : "—"}
      </td>
      <td>
        <div className={styles.cellActions}>
          {isEditing ? (
            <>
              <Button
                size="sm"
                variant="primary"
                onClick={() =>
                  onSave({
                    term,
                    canonical: canonical || null,
                    appContext: appContext || null,
                  })
                }
                ariaLabel={t("common.save")}
              >
                <CheckIcon size={12} />
              </Button>
              <Button
                size="sm"
                onClick={onCancelEdit}
                ariaLabel={t("common.cancel")}
              >
                <XIcon size={12} />
              </Button>
            </>
          ) : (
            <>
              <Button size="sm" onClick={onEdit} ariaLabel="Edit">
                Edit
              </Button>
              <Button
                size="sm"
                variant="danger"
                onClick={onDelete}
                ariaLabel={t("common.delete")}
              >
                <TrashIcon size={12} />
              </Button>
            </>
          )}
        </div>
      </td>
    </tr>
  );
}
