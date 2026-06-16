// DictionaryEntryForm — the labeled inputs shared by the inline
// "Add term" panel on the Dictionary page and the "Add term from
// dictation" modal. Pulled out so the two surfaces can't drift; the
// owning surface decides framing (panel vs dialog) and submission
// (single add vs reconcile-edit).
//
// The form is intentionally controlled. The parent owns state and
// drives every keystroke — keeps the dialog's reset-on-open ritual
// and the inline panel's reset-after-submit dance both trivially
// expressible without two copies of the same logic.

import { ChipInput } from "../components/ChipInput";
import { t } from "../i18n";

import styles from "./AddDictionaryTermDialog.module.css";

export interface DictionaryFormState {
  canonical: string;
  variants: string[];
  appContext: string;
}

interface Props {
  value: DictionaryFormState;
  onChange: (next: DictionaryFormState) => void;
  /** Forwarded to the canonical input — handy for autofocus inside dialogs. */
  autoFocusCanonical?: boolean;
}

export function DictionaryEntryForm({
  value,
  onChange,
  autoFocusCanonical,
}: Props) {
  return (
    <div className={styles.form}>
      <label className={styles.field}>
        <span className={styles.label}>{t("dictionary.form.canonical")}</span>
        <input
          className={styles.input}
          value={value.canonical}
          onChange={(e) => onChange({ ...value, canonical: e.target.value })}
          placeholder={t("dictionary.form.placeholder.canonical")}
          autoFocus={autoFocusCanonical}
          aria-label={t("dictionary.form.canonical")}
        />
      </label>
      <label className={styles.field}>
        <span className={styles.label}>{t("dictionary.form.variants")}</span>
        <ChipInput
          value={value.variants}
          onChange={(next) => onChange({ ...value, variants: next })}
          placeholder={t("dictionary.form.placeholder.variants")}
          ariaLabel={t("dictionary.form.variants")}
        />
      </label>
      <label className={styles.field}>
        <span className={styles.label}>{t("dictionary.column.appContext")}</span>
        <input
          className={styles.input}
          value={value.appContext}
          onChange={(e) => onChange({ ...value, appContext: e.target.value })}
          placeholder={t("dictionary.add.placeholder.appContext")}
          aria-label={t("dictionary.column.appContext")}
        />
      </label>
      <p className={styles.hint}>{t("dictionary.form.hint")}</p>
    </div>
  );
}

export const EMPTY_FORM: DictionaryFormState = {
  canonical: "",
  variants: [],
  appContext: "",
};
