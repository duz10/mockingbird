// macOS-only Modes "Model" control + low-RAM warning (ADR 0066).
//
// WHY this exists: on a memory-constrained Mac the RAM-aware layer
// (ADR 0064) substitutes the modes-table parity model (a 7B) for a
// smaller one (a 3B) at dictation time. The legacy free-text Model field
// showed the CONFIGURED 7B — i.e. it lied about what actually runs. This
// component shows the EFFECTIVE model, lets the user pin a specific model
// (a persisted override that bypasses the heuristic), and revert to Auto.
//
// Gated to macOS by the caller (isMac). Non-macOS keeps the legacy
// free-text input verbatim → Windows byte-identical. The override layer
// is itself macOS-scoped (the bug is Mac-only).

import { useCallback } from "react";

import { t } from "../i18n";
import { api } from "../lib/tauri";
import type { EffectiveModel } from "../lib/types";

import styles from "./Modes.module.css";

/** Sentinel `<option>` value for "Auto (RAM-aware)" — no user pin. */
const AUTO_VALUE = "__auto__";

interface MacModelControlProps {
  slug: string;
  /** Locally-installed Ollama tags for the dropdown options. */
  installedModels: string[];
  /** The effective-model payload for this mode (configured/effective/
   *  override/budget/reachability). */
  effective: EffectiveModel;
  /** Re-fetch the effective model after an override change. */
  onChanged: () => void;
}

/**
 * macOS Model picker: a dropdown of installed Ollama models plus an
 * "Auto (RAM-aware)" option (the default). Auto renders the effective
 * model inline so it never misrepresents what runs; a specific pick is a
 * persisted override with a revert affordance.
 */
export function MacModelControl({
  slug,
  installedModels,
  effective,
  onChanged,
}: MacModelControlProps) {
  const isAuto = effective.overrideModel === null;

  const handleSelect = useCallback(
    async (value: string) => {
      if (value === AUTO_VALUE) {
        await api.clear_mode_model_override(slug);
      } else {
        await api.set_mode_model_override(slug, value);
      }
      onChanged();
    },
    [slug, onChanged],
  );

  // Build the option set: Auto first, then installed models, plus the
  // current override if it isn't in the installed list (so a pinned-but-
  // since-removed model still displays + stays selectable).
  const options = [...installedModels];
  if (effective.overrideModel && !options.includes(effective.overrideModel)) {
    options.push(effective.overrideModel);
  }

  const budgetSuffix =
    effective.budgetGb !== null ? ` (${effective.budgetGb} GB)` : "";

  return (
    <div className={styles.field}>
      <label className={styles.fieldLabel} htmlFor={`${slug}-model`}>
        {t("modes.field.model")}
      </label>
      <select
        id={`${slug}-model`}
        className={styles.select}
        value={isAuto ? AUTO_VALUE : effective.overrideModel ?? AUTO_VALUE}
        onChange={(e) => void handleSelect(e.target.value)}
      >
        <option value={AUTO_VALUE}>{t("modes.field.model.auto")}</option>
        {options.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </select>

      {/* Truthful effective-model line. In Auto we spell out what the
          RAM-aware layer actually picks; pinned shows the pin is exact. */}
      {isAuto ? (
        <span className={styles.effectiveHint}>
          {t("modes.field.model.usingPrefix")} {effective.effective}
          {budgetSuffix}
        </span>
      ) : (
        <div className={styles.effectiveRow}>
          <span className={styles.effectiveHint}>
            {t("modes.field.model.pinned")}
          </span>
          <button
            type="button"
            className={styles.revertBtn}
            onClick={() => void handleSelect(AUTO_VALUE)}
          >
            {t("modes.field.model.revert")}
          </button>
        </div>
      )}

      {!effective.ollamaReachable ? (
        <span className={styles.ollamaNote}>
          {t("modes.field.model.ollamaDown")}
        </span>
      ) : null}
    </div>
  );
}

interface MacRamWarningProps {
  /** Detected unified-memory budget in whole GiB, or null if unknown. */
  budgetGb: number | null;
}

/** Hardcoded RAM threshold (GiB) below which Normal's rich formatting is
 *  not reliably achievable (the 7B doesn't fit; a lighter model runs). */
const LOW_RAM_THRESHOLD_GB = 16;

/**
 * <16 GB explainer for the Modes section (macOS). Non-alarming: it tells
 * the user Normal's richer formatting is tuned for 16 GB+ Macs (full 7B)
 * and recommends Casual on this machine. Renders nothing at/above the
 * threshold or when the budget is unknown.
 */
export function MacRamWarning({ budgetGb }: MacRamWarningProps) {
  if (budgetGb === null || budgetGb >= LOW_RAM_THRESHOLD_GB) return null;
  return (
    <div className={styles.ramWarning} role="note">
      <strong className={styles.ramWarningTitle}>
        {t("modes.lowRam.title")}
      </strong>
      <p className={styles.ramWarningBody}>{t("modes.lowRam.body")}</p>
    </div>
  );
}
