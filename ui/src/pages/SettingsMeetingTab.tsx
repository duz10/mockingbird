// Phase MC Wave 5 — Meeting settings tab.
//
// Extracted from Settings.tsx to keep that file under the 600-line
// cap (it was 671 lines at extraction time). Hosts the controls for
// the meeting-side SettingKey variants per docs/phases/
// phase-mc-wave5-brief.md §5.2.
//
// State source-of-truth: the typed Rust `Settings` facade, reached
// via `api.meeting_settings_get_all()` / `api.meeting_settings_set()`.
// `hotkeyPaused` is read-only here on purpose — toggling it has to
// go through `meetings.setPaused()` so the activation thread gets
// the PauseToggle event alongside the settings write.
//
// No tests for the JSX rendering — we don't ship @testing-library/
// react. The IPC contract is exercised by `Settings.meeting.test.ts`
// in this directory (pure mock-IPC round-trip).

import { useCallback, useEffect, useState } from "react";

import { Card, Spinner } from "../components/primitives";
import { t } from "../i18n";
import {
  clampMaxDuration,
  meetings,
  MEETING_MAX_DURATION_MAX_SEC,
  MEETING_MAX_DURATION_MIN_SEC,
} from "../lib/meetings";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import type { MeetingSettingsSnapshot } from "../lib/types";

import styles from "./Settings.module.css";

const MODIFIER_OPTIONS = [
  "VK_RCONTROL",
  "VK_LCONTROL",
  "VK_RMENU", // Right Alt
  "VK_LMENU", // Left Alt
  "VK_F13",
] as const;

// mb-fc1: VK_OEM_PERIOD is the post-Copilot-collision default (Right
// Ctrl + `.`). Order intentionally puts it first so the picker shows
// the current default selected on a fresh DB.
const MAIN_KEY_OPTIONS = [
  "VK_OEM_PERIOD",
  "VK_OEM_COMMA",
  "VK_OEM_1", // ;:
  "VK_OEM_5", // \|
  "VK_M",
  "VK_F13",
  "VK_F14",
  "VK_F15",
] as const;

// Human labels for the main-key options. Mirrors `short_label()` in
// `meetings/runtime.rs` so log lines and the picker agree.
const MAIN_KEY_LABELS: Record<(typeof MAIN_KEY_OPTIONS)[number], string> = {
  VK_OEM_PERIOD: "Period  .",
  VK_OEM_COMMA: "Comma  ,",
  VK_OEM_1: "Semicolon  ;",
  VK_OEM_5: "Backslash  \\",
  VK_M: "M",
  VK_F13: "F13",
  VK_F14: "F14",
  VK_F15: "F15",
};

const SOURCE_OPTIONS = ["mic", "system", "both"] as const;

// `paragraph_gap_ms` is clamped server-side to [500, 10_000]; mirror
// the same bounds on the slider so the user gets immediate feedback.
const PARAGRAPH_GAP_MIN = 500;
const PARAGRAPH_GAP_MAX = 10_000;
const PARAGRAPH_GAP_STEP = 100;

// Speaker labels are free text but absurdly long values would break
// the export markdown header. 30 chars matches the master-plan call.
const SPEAKER_LABEL_MAX_LENGTH = 30;

const LEGACY_CHORD_SETTING = "legacy_meeting_chord_enabled";

export function SettingsMeetingTab() {
  // mb-7rl: the meeting activation chord is `#[cfg(windows)]` — on macOS
  // meetings start/stop from the Meetings-page buttons, so the whole
  // VK-name chord picker (modifier + main-key + pause + legacy-chord) is
  // a Windows-only surface and must not show. `isMac` is `null` on
  // Windows, so `!isMac` renders the card exactly as today there.
  const isMac = useAppStore((s) => s.isMac);
  const [snap, setSnap] = useState<MeetingSettingsSnapshot | null>(null);
  const [savingError, setSavingError] = useState<string | null>(null);
  // Phase 10 Wave 1A deferral landed in Wave 1B: the typed-settings
  // toggle that flips the meeting chord between the legacy
  // direct-start behavior and the new Command-Center-mediated path.
  // Default is `false` (CC-mediated) per ADR 0037.
  const [legacyChord, setLegacyChord] = useState<boolean>(false);

  useEffect(() => {
    let cancelled = false;
    void api.meeting_settings_get_all().then((s) => {
      if (!cancelled) setSnap(s);
    });
    void api
      .legacy_get_setting(LEGACY_CHORD_SETTING)
      .then((v) => {
        if (!cancelled) setLegacyChord(Boolean(v));
      })
      .catch(() => {
        /* first-launch — stay on default */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const setLegacyChordPersisted = useCallback(async (next: boolean) => {
    setLegacyChord(next);
    try {
      await api.legacy_set_setting(LEGACY_CHORD_SETTING, next);
      setSavingError(null);
    } catch (err) {
      setSavingError(String(err));
      // Roll back the optimistic flip.
      setLegacyChord(!next);
    }
  }, []);

  // Patch one setting. Optimistic local update + persist; on persist
  // error we revert + surface the error. The Rust side is the source
  // of truth — the snapshot in state is a cache.
  const patch = useCallback(
    async <K extends keyof MeetingSettingsSnapshot>(
      key: K,
      value: MeetingSettingsSnapshot[K],
      dbKey: string,
    ) => {
      setSnap((prev) => (prev ? { ...prev, [key]: value } : prev));
      try {
        await api.meeting_settings_set(dbKey, value);
        setSavingError(null);
      } catch (err) {
        setSavingError(String(err));
        // Reload from the server to recover from a partial write.
        const fresh = await api.meeting_settings_get_all();
        setSnap(fresh);
      }
    },
    [],
  );

  // Pause toggle goes through the dedicated command so the runtime
  // injects the PauseToggle activation event in addition to writing
  // the setting. We optimistically flip the local state for snappy
  // UI; the next snapshot refresh corrects any drift.
  const togglePaused = useCallback(async () => {
    if (!snap) return;
    const next = !snap.hotkeyPaused;
    setSnap({ ...snap, hotkeyPaused: next });
    try {
      await meetings.setPaused(next);
      setSavingError(null);
    } catch (err) {
      setSavingError(String(err));
      const fresh = await api.meeting_settings_get_all();
      setSnap(fresh);
    }
  }, [snap]);

  if (!snap) return <Spinner />;

  return (
    <div className={styles.stack}>
      {savingError ? (
        <div className={styles.errorBanner} role="alert">
          {t("settings.meeting.saveError").replace("{error}", savingError)}
        </div>
      ) : null}

      {!isMac ? (
      <Card title={t("settings.meeting.activation")}>
        <Row label={t("settings.meeting.modifier")}>
          <select
            className={styles.select}
            value={snap.hotkeyModifier}
            onChange={(e) =>
              void patch(
                "hotkeyModifier",
                e.target.value,
                "meeting_hotkey_modifier",
              )
            }
          >
            {MODIFIER_OPTIONS.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t("settings.meeting.mainKey")}>
          <select
            className={styles.select}
            value={snap.hotkeyKey}
            onChange={(e) =>
              void patch("hotkeyKey", e.target.value, "meeting_hotkey_key")
            }
          >
            {/* If the stored value isn't one of our curated options
                (e.g. user hand-edited the DB to VK_F8), render it as
                a passthrough so the picker doesn't appear broken. */}
            {!(MAIN_KEY_OPTIONS as readonly string[]).includes(
              snap.hotkeyKey,
            ) ? (
              <option value={snap.hotkeyKey}>{snap.hotkeyKey}</option>
            ) : null}
            {MAIN_KEY_OPTIONS.map((k) => (
              <option key={k} value={k}>
                {MAIN_KEY_LABELS[k]}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t("settings.meeting.hotkeyPaused")}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={snap.hotkeyPaused}
              onChange={() => void togglePaused()}
            />
            <span>
              {snap.hotkeyPaused
                ? t("settings.meeting.hotkeyPaused.on")
                : t("settings.meeting.hotkeyPaused.off")}
            </span>
          </label>
        </Row>
        <Row label={t("settings.meeting.legacyChord")}>
          {/* Phase 10 Wave 1A deferral landed in Wave 1B: surface the
              `legacy_meeting_chord_enabled` typed setting + a one-click
              "restore old behavior" button. When enabled, the meeting
              chord skips the Command Center and starts a meeting
              directly (the pre-ADR-0037 behavior). */}
          <div className={styles.sliderRow}>
            <label className={styles.toggle}>
              <input
                type="checkbox"
                checked={legacyChord}
                onChange={(e) =>
                  void setLegacyChordPersisted(e.target.checked)
                }
              />
              <span>{t("settings.meeting.legacyChord.help")}</span>
            </label>
            <button
              type="button"
              className={styles.input}
              style={{ cursor: "pointer" }}
              onClick={() => void setLegacyChordPersisted(true)}
              disabled={legacyChord}
            >
              {t("settings.meeting.legacyChord.restore")}
            </button>
          </div>
        </Row>
      </Card>
      ) : null}

      <Card title={t("settings.meeting.transcript")}>
        <Row label={t("settings.meeting.defaultSource")}>
          <select
            className={styles.select}
            value={snap.defaultSource}
            onChange={(e) =>
              void patch(
                "defaultSource",
                e.target.value as MeetingSettingsSnapshot["defaultSource"],
                "meeting_default_source",
              )
            }
          >
            {SOURCE_OPTIONS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t("settings.meeting.fillerStrip")}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={snap.fillerStripEnabled}
              onChange={(e) =>
                void patch(
                  "fillerStripEnabled",
                  e.target.checked,
                  "meeting_filler_strip_enabled",
                )
              }
            />
            <span>
              {snap.fillerStripEnabled
                ? t("settings.meeting.fillerStrip.on")
                : t("settings.meeting.fillerStrip.off")}
            </span>
          </label>
        </Row>
        <Row label={t("settings.meeting.maxDuration")}>
          {/* ADR 0032 / mb-mom: surface MeetingMaxDurationSeconds so
              users can shorten the cap (self-imposed discipline) or
              extend it for an all-hands. Server clamp is the source
              of truth; clampMaxDuration mirrors it for UX. */}
          <div className={styles.sliderRow}>
            <input
              className={styles.input}
              type="number"
              min={MEETING_MAX_DURATION_MIN_SEC}
              max={MEETING_MAX_DURATION_MAX_SEC}
              step={60}
              value={snap.maxDurationSeconds}
              onChange={(e) =>
                void patch(
                  "maxDurationSeconds",
                  clampMaxDuration(parseInt(e.target.value, 10)),
                  "meeting_max_duration_seconds",
                )
              }
            />
            <span>{t("settings.meeting.maxDuration.unit")}</span>
          </div>
        </Row>
        <Row label={t("settings.meeting.paragraphGap")}>
          <div className={styles.sliderRow}>
            <input
              type="range"
              min={PARAGRAPH_GAP_MIN}
              max={PARAGRAPH_GAP_MAX}
              step={PARAGRAPH_GAP_STEP}
              value={snap.paragraphGapMs}
              onChange={(e) =>
                void patch(
                  "paragraphGapMs",
                  parseInt(e.target.value, 10),
                  "meeting_paragraph_gap_ms",
                )
              }
            />
            <span>{snap.paragraphGapMs} ms</span>
          </div>
        </Row>
        <Row label={t("settings.meeting.speakerLabelMic")}>
          <input
            className={styles.input}
            type="text"
            maxLength={SPEAKER_LABEL_MAX_LENGTH}
            value={snap.speakerLabelMic}
            onChange={(e) =>
              void patch(
                "speakerLabelMic",
                e.target.value,
                "meeting_speaker_label_mic",
              )
            }
          />
        </Row>
        <Row label={t("settings.meeting.speakerLabelSys")}>
          <input
            className={styles.input}
            type="text"
            maxLength={SPEAKER_LABEL_MAX_LENGTH}
            value={snap.speakerLabelSys}
            onChange={(e) =>
              void patch(
                "speakerLabelSys",
                e.target.value,
                "meeting_speaker_label_sys",
              )
            }
          />
        </Row>
        <Row label={t("settings.meeting.llmPass")}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={snap.llmPassEnabled}
              onChange={(e) =>
                void patch(
                  "llmPassEnabled",
                  e.target.checked,
                  "meeting_llm_pass_enabled",
                )
              }
            />
            <span>
              {snap.llmPassEnabled
                ? t("settings.meeting.llmPass.on")
                : t("settings.meeting.llmPass.off")}
            </span>
          </label>
        </Row>
      </Card>

      <Card title={t("settings.meeting.audioRetention")}>
        <Row label={t("settings.meeting.audioRetention.inherit")}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={snap.audioRetentionDays === null}
              onChange={(e) =>
                void patch(
                  "audioRetentionDays",
                  e.target.checked ? null : 30,
                  "meeting_audio_retention_days",
                )
              }
            />
            <span>
              {snap.audioRetentionDays === null
                ? t("settings.meeting.audioRetention.inherit.on")
                : t("settings.meeting.audioRetention.inherit.off")}
            </span>
          </label>
        </Row>
        {snap.audioRetentionDays !== null ? (
          <Row label={t("settings.meeting.audioRetention.days")}>
            <input
              className={styles.input}
              type="number"
              min={1}
              max={3650}
              value={snap.audioRetentionDays}
              onChange={(e) =>
                void patch(
                  "audioRetentionDays",
                  parseInt(e.target.value, 10) || 30,
                  "meeting_audio_retention_days",
                )
              }
            />
          </Row>
        ) : null}
      </Card>
    </div>
  );
}

function Row({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className={styles.row}>
      <label className={styles.rowLabel}>{label}</label>
      <div className={styles.rowControl}>{children}</div>
    </div>
  );
}
