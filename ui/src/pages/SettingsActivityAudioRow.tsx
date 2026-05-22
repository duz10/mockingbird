// Phase 10 Wave 4: surface the `activity_audio_enabled` typed setting
// in Settings → General. Activity Capture sessions opened via the
// Command Center read this value at start time (per the
// `dispatch_activity_start` path in `command_center/mod.rs`). The IPC
// `activity_start(withAudio)` honors it as the default when JS doesn't
// pass an explicit override.
//
// Extracted into its own file mirroring `SettingsCommandCenterRow.tsx`
// for the same reason — Settings.tsx is at its 600-line ceiling.
//
// Storage: `SettingKey::ActivityAudioEnabled`, default `false` (privacy
// by default per Principle 4 spirit). Stored as JSON `true`/`false`
// through the legacy-bridge IPC + the same typed-setting plumbing the
// Command Center chord row uses.

import { useEffect, useState } from "react";

import { t } from "../i18n";
import { api } from "../lib/tauri";

import styles from "./Settings.module.css";

const SETTING_KEY = "activity_audio_enabled";

/**
 * Toggle row for Activity Capture's audio (mic + system loopback)
 * recording. Off by default. Re-loaded once on mount; we don't
 * subscribe to a changefeed because this is a rare-touch knob.
 */
export function SettingsActivityAudioRow() {
  const [enabled, setEnabled] = useState<boolean>(false);
  const [saving, setSaving] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const v = await api.legacy_get_setting(SETTING_KEY);
        if (cancelled) return;
        // The legacy bridge returns the raw JSON value as-is; the
        // server-side default is `false` so an empty / first-launch
        // read lands here as `false` too.
        setEnabled(Boolean(v));
      } catch {
        // First launch — stay on default.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const onToggle = async (next: boolean) => {
    setEnabled(next);
    setSaving(true);
    setError(null);
    try {
      await api.legacy_set_setting(SETTING_KEY, next);
    } catch (err) {
      // Roll back the optimistic flip so the displayed state matches
      // what the DB actually accepted.
      setEnabled(!next);
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className={styles.row}>
      <div className={styles.rowMain}>
        <span className={styles.rowLabel}>
          {t("settings.activity.audio")}
        </span>
        <span className={styles.rowHelp}>
          {t("settings.activity.audio.help")}
        </span>
        {error && (
          <span className={styles.rowHelp} role="alert">
            {error}
          </span>
        )}
      </div>
      <div className={styles.rowControl}>
        <input
          type="checkbox"
          checked={enabled}
          disabled={saving}
          onChange={(e) => void onToggle(e.target.checked)}
          aria-label={t("settings.activity.audio")}
        />
      </div>
    </div>
  );
}
