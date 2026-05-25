// ADR 0047 Wave 2C / mb-h0nn — Dictation settings tab.
//
// Promoted from the slim "Dictation data" panel (retention-only)
// inside Settings.tsx into a full top-level tab that mirrors the
// structural pattern of SettingsMeetingTab. Sections, top → bottom:
//
//   1. Cleanup behaviour   — cleanup-level dial + Q5_K_M opt-in
//   2. Activation          — read-only display of the PTT hotkey +
//                            in-app activation pointer
//   3. Per-mode tuning     — cross-link to the Modes page
//   4. Data retention      — preserved from the old History panel
//                            (Keep dictations for / Keep audio /
//                            Purge all)
//
// State sources:
//   - `useAppStore.settings` for retention controls (lifted from the
//     old HistoryPanel verbatim).
//   - `api.legacy_get_setting` / `api.legacy_set_setting` for the two
//     new typed-registry settings (DictationCleanupLevel +
//     PreferQ5Models). Same pattern SettingsMeetingTab uses for the
//     legacy-chord toggle. No new SettingsSnapshot fields needed.
//
// VRAM probe (ADR 0047 §Wave 2.4): the Rust-side `vram_probe::probe_vram_mib`
// exists but is NOT yet exposed as a Tauri command. Tracked in
// bead mb-e2t8. Until that lands, this tab renders the
// "VRAM probe unavailable" string under the Q5 toggle.

import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { Button, Card, Spinner } from "../components/primitives";
import { Switch } from "../design/components";
import { t } from "../i18n";
import { api } from "../lib/tauri";
import type { SettingsSnapshot } from "../lib/types";

import styles from "./Settings.module.css";

/* ------------------------------------------------------------------ */
/* Typed-registry constants                                             */
/* ------------------------------------------------------------------ */

/** Backing key for `SettingKey::DictationCleanupLevel`. Lowercase
 *  string: `"none" | "light" | "medium" | "high"`. Default is `"high"`
 *  (set by migration 020). */
const CLEANUP_LEVEL_KEY = "dictation_cleanup_level";
/** Backing key for `SettingKey::PreferQ5Models`. Bool, default false. */
const PREFER_Q5_KEY = "prefer_q5_models";
/** Q5 floor in MiB, mirrors `cleanup::vram_probe::Q5_VRAM_FLOOR_MIB`. */
const Q5_VRAM_FLOOR_MIB = 6144;

const CLEANUP_LEVELS = ["none", "light", "medium", "high"] as const;
type CleanupLevel = (typeof CLEANUP_LEVELS)[number];

function isCleanupLevel(value: unknown): value is CleanupLevel {
  return (
    typeof value === "string" &&
    (CLEANUP_LEVELS as readonly string[]).includes(value)
  );
}

interface PanelProps {
  settings: SettingsSnapshot;
  patch: (
    key: string,
    value: string | boolean | number,
    next: Partial<SettingsSnapshot>,
  ) => void | Promise<void>;
}

export function SettingsDictationTab({ settings, patch }: PanelProps) {
  // Cleanup-level + Q5 are read from the typed-settings registry; we
  // keep a local cache mirroring SettingsMeetingTab's pattern so the
  // controls update optimistically and roll back on persist error.
  const [level, setLevelState] = useState<CleanupLevel | null>(null);
  const [preferQ5, setPreferQ5State] = useState<boolean | null>(null);
  const [vramMib, setVramMib] = useState<number | null | "unavailable">(
    "unavailable",
  );
  const [savingError, setSavingError] = useState<string | null>(null);

  // Initial fetch — typed registry, defensive on first-launch misses.
  useEffect(() => {
    let cancelled = false;
    void api
      .legacy_get_setting(CLEANUP_LEVEL_KEY)
      .then((v) => {
        if (cancelled) return;
        setLevelState(isCleanupLevel(v) ? v : "high");
      })
      .catch(() => {
        if (!cancelled) setLevelState("high");
      });
    void api
      .legacy_get_setting(PREFER_Q5_KEY)
      .then((v) => {
        if (!cancelled) setPreferQ5State(Boolean(v));
      })
      .catch(() => {
        if (!cancelled) setPreferQ5State(false);
      });
    // VRAM probe IPC isn't wired yet (mb-e2t8). When it lands the
    // call here becomes `api.probe_vram_mib()` and the readout flips
    // from "unavailable" to the numeric value (or null when the
    // probe runs but can't find an Nvidia card).
    setVramMib("unavailable");
    return () => {
      cancelled = true;
    };
  }, []);

  const setLevelPersisted = useCallback(
    async (next: CleanupLevel) => {
      const prev = level;
      setLevelState(next);
      try {
        await api.legacy_set_setting(CLEANUP_LEVEL_KEY, next);
        setSavingError(null);
      } catch (err) {
        setSavingError(String(err));
        setLevelState(prev);
      }
    },
    [level],
  );

  const setPreferQ5Persisted = useCallback(
    async (next: boolean) => {
      const prev = preferQ5;
      setPreferQ5State(next);
      try {
        await api.legacy_set_setting(PREFER_Q5_KEY, next);
        setSavingError(null);
      } catch (err) {
        setSavingError(String(err));
        setPreferQ5State(prev);
      }
    },
    [preferQ5],
  );

  if (level === null || preferQ5 === null) {
    return <Spinner />;
  }

  const vramBelowFloor =
    typeof vramMib === "number" && vramMib < Q5_VRAM_FLOOR_MIB;

  return (
    <div className={styles.stack}>
      {savingError ? (
        <div className={styles.errorBanner} role="alert">
          {t("settings.meeting.saveError").replace("{error}", savingError)}
        </div>
      ) : null}

      <CleanupBehaviourCard
        level={level}
        onLevelChange={setLevelPersisted}
        preferQ5={preferQ5}
        onPreferQ5Change={setPreferQ5Persisted}
        vramMib={vramMib}
        vramBelowFloor={vramBelowFloor}
      />

      <ActivationCard />

      <PerModeCard />

      <DataRetentionCard settings={settings} patch={patch} />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* §1 — Cleanup behaviour                                              */
/* ------------------------------------------------------------------ */

interface CleanupCardProps {
  level: CleanupLevel;
  onLevelChange: (next: CleanupLevel) => void;
  preferQ5: boolean;
  onPreferQ5Change: (next: boolean) => void;
  vramMib: number | null | "unavailable";
  vramBelowFloor: boolean;
}

function CleanupBehaviourCard({
  level,
  onLevelChange,
  preferQ5,
  onPreferQ5Change,
  vramMib,
  vramBelowFloor,
}: CleanupCardProps) {
  return (
    <Card title={t("settings.dictation.cleanup")}>
      <p className={styles.dictationScopeNote}>
        {t("settings.dictation.cleanup.scope")}
      </p>

      <span className={styles.rowLabel}>
        {t("settings.dictation.cleanup.level")}
      </span>
      <CleanupLevelDial value={level} onChange={onLevelChange} />

      <div
        className={`${styles.q5Row} ${
          vramBelowFloor ? styles.q5Row_lowVram : ""
        }`}
      >
        <Switch
          label={t("settings.dictation.cleanup.preferQ5")}
          description={t("settings.dictation.cleanup.preferQ5.desc")}
          checked={preferQ5}
          onChange={(e) => onPreferQ5Change(e.target.checked)}
          aria-label={t("settings.dictation.cleanup.preferQ5")}
        />
        <VramReadout mib={vramMib} belowFloor={vramBelowFloor} />
      </div>
    </Card>
  );
}

/**
 * 4-option card-style radio group. role="radiogroup" with each child
 * containing a native radio input for keyboard + form semantics; CSS
 * paints the surrounding card.
 *
 * NOTE(mb-km6j): this is the 4th segmented-control pattern in the
 * codebase (Settings sub-tab strip, theme picker, sidebar nav are
 * the other three). Intentionally divergent here because none of the
 * existing patterns can carry a multi-line description per option.
 * When mb-km6j consolidates the family, fold this in.
 */
function CleanupLevelDial({
  value,
  onChange,
}: {
  value: CleanupLevel;
  onChange: (next: CleanupLevel) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={t("settings.dictation.cleanup.level")}
      className={styles.levelGroup}
    >
      {CLEANUP_LEVELS.map((lvl) => {
        const selected = lvl === value;
        const labelKey = `settings.dictation.cleanup.level.${lvl}` as const;
        const descKey = `${labelKey}.desc` as const;
        return (
          <label
            key={lvl}
            className={`${styles.levelCard} ${
              selected ? styles.levelCard_active : ""
            }`}
          >
            <input
              type="radio"
              name="dictation-cleanup-level"
              className={styles.levelCardInput}
              value={lvl}
              checked={selected}
              onChange={() => onChange(lvl)}
              aria-label={t(labelKey)}
            />
            <span className={styles.levelCardHeader}>
              <span className={styles.levelCardTitle}>{t(labelKey)}</span>
              {lvl === "high" ? (
                <span className={styles.levelCardBadge}>
                  {t("settings.dictation.cleanup.level.recommended")}
                </span>
              ) : null}
            </span>
            <span className={styles.levelCardDesc}>{t(descKey)}</span>
          </label>
        );
      })}
    </div>
  );
}

function VramReadout({
  mib,
  belowFloor,
}: {
  mib: number | null | "unavailable";
  belowFloor: boolean;
}) {
  // Three render branches:
  //   1. probe returned a positive number (rendered with floor-aware
  //      colour treatment)
  //   2. probe ran but returned None (no Nvidia card / parse fail)
  //   3. IPC not wired yet (mb-e2t8) — "unavailable"
  // All three collapse to a muted readout under the toggle. The
  // toggle stays clickable in every case; we never disable on the
  // strength of a probe result the user can't influence.
  if (mib === "unavailable" || mib === null) {
    return (
      <span className={styles.vramReadout}>
        {t("settings.dictation.cleanup.preferQ5.vramUnknown")}
      </span>
    );
  }
  const key = belowFloor
    ? "settings.dictation.cleanup.preferQ5.vramLow"
    : "settings.dictation.cleanup.preferQ5.vramOk";
  return (
    <span
      className={`${styles.vramReadout} ${
        belowFloor ? styles.vramReadout_low : ""
      }`}
    >
      {t(key).replace("{mib}", String(mib))}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* §2 — Activation (read-only)                                         */
/* ------------------------------------------------------------------ */

function ActivationCard() {
  return (
    <Card title={t("settings.dictation.activation")}>
      <div className={styles.readonlyRow}>
        <span className={styles.readonlyLabel}>
          {t("settings.dictation.activation.ptt")}
        </span>
        <span className={styles.readonlyValue}>
          {t("settings.dictation.activation.ptt.value")}
        </span>
      </div>
      <div className={styles.readonlyRow}>
        <span className={styles.readonlyLabel}>
          {t("settings.dictation.activation.inApp")}
        </span>
        <span className={styles.readonlyValue}>
          {t("settings.dictation.activation.inApp.value")}
        </span>
      </div>
      <p className={styles.readonlyNote}>
        {t("settings.dictation.activation.note")}
      </p>
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* §3 — Per-mode tuning (cross-link)                                   */
/* ------------------------------------------------------------------ */

function PerModeCard() {
  const navigate = useNavigate();
  return (
    <Card title={t("settings.dictation.perMode")}>
      <div className={styles.crossLinkRow}>
        <p className={styles.crossLinkDesc}>
          {t("settings.dictation.perMode.desc")}
        </p>
        <Button
          variant="primary"
          onClick={() => navigate("/modes")}
          ariaLabel={t("settings.dictation.perMode.open")}
        >
          {t("settings.dictation.perMode.open")}
        </Button>
      </div>
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* §4 — Data retention (lifted verbatim from the old HistoryPanel)     */
/* ------------------------------------------------------------------ */

function DataRetentionCard({ settings, patch }: PanelProps) {
  return (
    <Card title={t("settings.dictation.retention")}>
      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowLabel}>
            {t("settings.history.retentionDays")}
          </span>
        </div>
        <div className={styles.rowControl}>
          <select
            className={styles.select}
            value={String(settings.retentionDays)}
            onChange={(e) =>
              void patch("history.retention_days", e.target.value, {
                retentionDays: Number(e.target.value),
              })
            }
            aria-label={t("settings.history.retentionDays")}
          >
            <option value="30">{t("settings.history.retention.30")}</option>
            <option value="90">{t("settings.history.retention.90")}</option>
            <option value="180">{t("settings.history.retention.180")}</option>
            <option value="365">{t("settings.history.retention.365")}</option>
            <option value="-1">{t("settings.history.retention.forever")}</option>
          </select>
        </div>
      </div>

      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowLabel}>
            {t("settings.history.audioRetention")}
          </span>
          <span className={styles.rowHelp}>
            {t("settings.history.audioRetention.help")}
          </span>
        </div>
        <div className={styles.rowControl}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={settings.audioRetention}
              onChange={(e) =>
                void patch("history.audio_retention", e.target.checked, {
                  audioRetention: e.target.checked,
                })
              }
              aria-label={t("settings.history.audioRetention")}
            />
            <span className={styles.toggleTrack}>
              <span className={styles.toggleThumb} />
            </span>
          </label>
        </div>
      </div>

      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowLabel}>
            {t("settings.history.purge")}
          </span>
        </div>
        <div className={styles.rowControl}>
          <Button
            variant="danger"
            onClick={() => {
              const code = window.prompt(t("settings.history.purge.confirm"));
              if (code === "PURGE") {
                window.alert(
                  "Purge is wired but the IPC handler ships in Phase 7.",
                );
              }
            }}
          >
            {t("settings.history.purge")}
          </Button>
        </div>
      </div>
    </Card>
  );
}
