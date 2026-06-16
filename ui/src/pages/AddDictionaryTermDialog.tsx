// Modal for adding a dictionary term while viewing a dictation.
// Triggered from DictationsDetailPane "Add term to dictionary"
// action (mb-t75k). Pre-fills `appContext` from the dictation's
// foregroundApp so the entry auto-scopes to whichever app the user
// was dictating into; user can clear that field for a global scope.
//
// Wraps the design-system Dialog primitive (native <dialog>, ESC +
// focus management for free) so we don't grow a bespoke modal shell.

import { useEffect, useState } from "react";

import { Button } from "../components/primitives";
import { Dialog } from "../design/components/Dialog";
import { PlusIcon } from "../design/Icon";
import { t } from "../i18n";
import { api } from "../lib/tauri";

import styles from "./AddDictionaryTermDialog.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
  /** Auto-fill for the appContext field (typically the dictation's
   *  foregroundApp). User can clear it for a global-scope entry. */
  initialAppContext?: string | null;
  /** Fired after a successful upsert so callers can toast / refresh. */
  onAdded?: () => void;
}

export function AddDictionaryTermDialog({
  open,
  onClose,
  initialAppContext,
  onAdded,
}: Props) {
  const [term, setTerm] = useState("");
  const [canonical, setCanonical] = useState("");
  const [appContext, setAppContext] = useState(initialAppContext ?? "");
  const [busy, setBusy] = useState(false);

  // Reset form state every time the dialog opens so a previous
  // half-typed entry doesn't ghost back in. The initialAppContext
  // is re-applied here for the same reason — the dictation might
  // have changed between opens.
  useEffect(() => {
    if (open) {
      setTerm("");
      setCanonical("");
      setAppContext(initialAppContext ?? "");
      setBusy(false);
    }
  }, [open, initialAppContext]);

  async function handleSubmit() {
    const trimmed = term.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    try {
      await api.upsert_dictionary_entry({
        term: trimmed,
        canonical: canonical.trim() || null,
        source: "user",
        confidence: 1.0,
        appContext: appContext.trim() || null,
      });
      onAdded?.();
      onClose();
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("dictionary.addDialog.title")}
      ariaLabel={t("dictionary.addDialog.title")}
      actions={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            variant="primary"
            onClick={() => void handleSubmit()}
            disabled={busy || term.trim().length === 0}
          >
            <PlusIcon size={12} />
            {t("dictionary.add")}
          </Button>
        </>
      }
    >
      <form
        className={styles.form}
        onSubmit={(e) => {
          e.preventDefault();
          void handleSubmit();
        }}
      >
        <label className={styles.field}>
          <span className={styles.label}>{t("dictionary.column.term")}</span>
          <input
            className={styles.input}
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            placeholder={t("dictionary.add.placeholder.term")}
            autoFocus
            aria-label={t("dictionary.column.term")}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.label}>{t("dictionary.column.canonical")}</span>
          <input
            className={styles.input}
            value={canonical}
            onChange={(e) => setCanonical(e.target.value)}
            placeholder={t("dictionary.add.placeholder.canonical")}
            aria-label={t("dictionary.column.canonical")}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.label}>{t("dictionary.column.appContext")}</span>
          <input
            className={styles.input}
            value={appContext}
            onChange={(e) => setAppContext(e.target.value)}
            placeholder={t("dictionary.add.placeholder.appContext")}
            aria-label={t("dictionary.column.appContext")}
          />
        </label>
        <p className={styles.hint}>{t("dictionary.add.hint")}</p>
      </form>
    </Dialog>
  );
}
