// Modal for adding a dictionary entry while viewing a dictation.
// Triggered from DictationsDetailPane "Add term to dictionary"
// action. Pre-fills `appContext` from the dictation's foregroundApp
// so the entry auto-scopes to whichever app the user was dictating
// into; the user can clear that field for a global scope.
//
// Built on the design-system Dialog primitive (native <dialog>, ESC +
// focus management for free) and the shared <DictionaryEntryForm> so
// it stays visually + structurally aligned with the inline panel on
// the Dictionary page (mb-9x33). Submit fires N parallel
// `upsert_dictionary_entry` calls — one per variant, or one
// proper-noun row when no variants were entered.

import { useEffect, useState } from "react";

import { Button } from "../components/primitives";
import { Dialog } from "../design/components/Dialog";
import { PlusIcon } from "../design/Icon";
import { t } from "../i18n";
import { api } from "../lib/tauri";

import {
  DictionaryEntryForm,
  EMPTY_FORM,
  type DictionaryFormState,
} from "./DictionaryEntryForm";

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
  const [form, setForm] = useState<DictionaryFormState>(EMPTY_FORM);
  const [busy, setBusy] = useState(false);

  // Reset every time the dialog opens so a previous half-typed entry
  // doesn't ghost back in. The initialAppContext is re-applied here
  // for the same reason — the dictation might have changed between
  // opens.
  useEffect(() => {
    if (open) {
      setForm({
        canonical: "",
        variants: [],
        appContext: initialAppContext ?? "",
      });
      setBusy(false);
    }
  }, [open, initialAppContext]);

  async function handleSubmit() {
    const canonical = form.canonical.trim();
    if (!canonical || busy) return;
    setBusy(true);
    try {
      const appContext = form.appContext.trim() || null;
      if (form.variants.length === 0) {
        await api.upsert_dictionary_entry({
          term: canonical,
          canonical: null,
          source: "user",
          confidence: 1.0,
          appContext,
        });
      } else {
        await Promise.all(
          form.variants.map((variant) =>
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
            disabled={busy || form.canonical.trim().length === 0}
          >
            <PlusIcon size={12} />
            {t("dictionary.add")}
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
