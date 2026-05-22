// Phase 10 Wave 1A deferral landed in Wave 1B: the
// `command_center_chord` row in the Settings → General panel.
//
// Extracted into its own file rather than appended to Settings.tsx
// because Settings.tsx was already past the 600-line cap when Wave 1B
// landed (715 lines on disk). The chord row reads + writes via the
// same `api.update_setting(key, value)` round-trip every other
// General-panel row uses — there's no special wiring on the Rust
// side beyond `SettingKey::CommandCenterChord`, which already has
// default + setter coverage in `settings/model.rs`.
//
// Tests: none — the round-trip is exercised end-to-end via the
// Command Center boot path (`command_center::parse_chord` reads the
// setting at app start, falls back to the default if the parse
// fails).

import { useEffect, useState } from "react";

import { t } from "../i18n";
import { api } from "../lib/tauri";

import styles from "./Settings.module.css";

const SETTING_KEY = "command_center_chord";
const DEFAULT_CHORD = "RightCtrl+Space";

/**
 * Free-text chord picker. Validation is intentionally minimal here —
 * the Rust side parses the string and falls back to the default if
 * it can't, so a typo never bricks the app. We just refuse to submit
 * an empty string.
 */
export function CommandCenterChordRow() {
  const [value, setValue] = useState<string>(DEFAULT_CHORD);
  const [draft, setDraft] = useState<string>(DEFAULT_CHORD);
  const [saving, setSaving] = useState(false);

  // Load the persisted value once on mount. We don't subscribe to a
  // changefeed — chord-rebind is rare; a stale-after-external-write
  // case is acceptably tiny.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        // The legacy single-setting bridge (`get_setting`) handles
        // arbitrary keys + returns a string. Wave 1A wired the value
        // into the typed `SettingKey::CommandCenterChord`; we don't
        // need a new IPC just to read one string.
        const v = await api.legacy_get_setting(SETTING_KEY);
        if (cancelled) return;
        const text = typeof v === "string" && v.length > 0 ? v : DEFAULT_CHORD;
        setValue(text);
        setDraft(text);
      } catch {
        // First-launch path — setting not yet materialized. Stay on
        // the default.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const commit = async () => {
    const next = draft.trim() || DEFAULT_CHORD;
    if (next === value) return;
    setSaving(true);
    try {
      // Typed-settings path (settings_v2) — the Rust side stores the
      // chord under `SettingKey::CommandCenterChord` as a JSON string.
      await api.legacy_set_setting(SETTING_KEY, next);
      setValue(next);
      setDraft(next);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className={styles.row}>
      <div className={styles.rowMain}>
        <span className={styles.rowLabel}>
          {t("settings.general.cmdCenterChord")}
        </span>
        <span className={styles.rowHelp}>
          {t("settings.general.cmdCenterChord.help")}
        </span>
      </div>
      <div className={styles.rowControl}>
        <input
          className={styles.input}
          type="text"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => void commit()}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void commit();
            }
          }}
          aria-label={t("settings.general.cmdCenterChord")}
          disabled={saving}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
        />
      </div>
    </div>
  );
}
