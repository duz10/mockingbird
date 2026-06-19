// Settings page — left-rail tabs, right panel renders the active tab.
// Tabs: General, Models, History & data, Meetings, Mobile Sync, Advanced.
// ADR 0046 Iter 4 / mb-vg3p added the Mobile Sync tab; the inline
// preview that used to live under Advanced was lifted into the new
// dedicated tab and deleted.
//
// State source of truth: `useAppStore.settings`. Writes go through
// `api.update_setting(key, value)` which we round-trip back into the
// store on success so external (e.g., DB-direct) changes still
// propagate on next refresh.
//
// Theme switch is special — applied to the DOM immediately so the
// user sees the change before the IPC round-trip.

import { useCallback, useEffect, useState } from "react";

import { Button, Card, PageHeader, Spinner } from "../components/primitives";
import { CATEGORIES } from "../components/UnsplashBackground/categories";
import {
  clearUnsplashApiKey,
  getPrefs,
  getUnsplashApiKey,
  PREFS_EVENT,
  setPref,
  setUnsplashApiKey,
  type BackgroundPrefs,
  type CurationMode,
} from "../components/UnsplashBackground/prefs";
import { toast } from "sonner";
import { FolderIcon } from "../design/Icon";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import type { LearningRun, SettingsSnapshot, ThemeChoice } from "../lib/types";
import { formatRelative } from "../lib/format";

import { CommandCenterChordRow } from "./SettingsCommandCenterRow";
import { SettingsActivityAudioRow } from "./SettingsActivityAudioRow";
import { SettingsActivityHardeningRow } from "./SettingsActivityHardeningRow";
import { SettingsDictationTab } from "./SettingsDictationTab";
import { SettingsKgTab } from "./SettingsKgTab";
import { SettingsMeetingTab } from "./SettingsMeetingTab";
import { SettingsMobileSyncTab } from "./SettingsMobileSyncTab";
import { SettingsPermissionsTab } from "./SettingsPermissionsTab";
import styles from "./Settings.module.css";

// ADR 0047 Wave 2C / mb-h0nn: the old `"history"` tab id is now
// `"dictation"`. The label key follows suit (`settings.tab.history` →
// `settings.tab.dictation`); the old key stays in en.json one cycle
// for back-compat and is removed at the next i18n consolidation.
type Tab =
  | "general"
  | "models"
  | "dictation"
  | "meeting"
  | "mobileSync"
  | "kg"
  | "permissions"
  | "advanced";

export function SettingsPage() {
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);
  const applyTheme = useAppStore((s) => s.applyTheme);

  const [tab, setTab] = useState<Tab>("general");

  // mb-mac-v1.4.6 — the macOS permissions tab only exists on macOS.
  // Detected once via `host_os()`; cheaper + more robust than UA sniffing.
  const [isMac, setIsMac] = useState(false);
  useEffect(() => {
    void api.host_os().then((os) => setIsMac(os === "macos"));
  }, []);

  // Lazy boot: if the store wasn't populated for some reason, fetch.
  useEffect(() => {
    if (!settings) {
      void api.get_settings().then(setSettings);
    }
  }, [settings, setSettings]);

  // Generic patcher — updates the store optimistically + persists.
  // `bool` values get coerced to "1"/"0" because the backend stores
  // settings as TEXT (see commands/settings.rs).
  const patch = useCallback(
    async (key: string, value: string | boolean | number, next: Partial<SettingsSnapshot>) => {
      if (!settings) return;
      setSettings({ ...settings, ...next });
      const str =
        typeof value === "boolean"
          ? value ? "1" : "0"
          : String(value);
      await api.update_setting(key, str);
    },
    [settings, setSettings],
  );

  if (!settings) {
    return (
      <>
        <PageHeader title={t("settings.title")} />
        <Spinner />
      </>
    );
  }

  return (
    <>
      <PageHeader title={t("settings.title")} />
      <div className={styles.shell}>
        <nav className={styles.tabs} aria-label="Settings sections">
          <TabBtn id="general" active={tab} setActive={setTab} label={t("settings.tab.general")} />
          <TabBtn id="models" active={tab} setActive={setTab} label={t("settings.tab.models")} />
          <TabBtn id="dictation" active={tab} setActive={setTab} label={t("settings.tab.dictation")} />
          <TabBtn id="meeting" active={tab} setActive={setTab} label={t("settings.tab.meeting")} />
          <TabBtn id="mobileSync" active={tab} setActive={setTab} label={t("settings.tab.mobileSync")} />
          <TabBtn id="kg" active={tab} setActive={setTab} label={t("settings.tab.kg")} />
          {isMac ? (
            <TabBtn
              id="permissions"
              active={tab}
              setActive={setTab}
              label={t("settings.tab.permissions")}
            />
          ) : null}
          <TabBtn id="advanced" active={tab} setActive={setTab} label={t("settings.tab.advanced")} />
        </nav>

        <div className={styles.panel}>
          {tab === "general" && (
            <GeneralPanel
              settings={settings}
              applyTheme={applyTheme}
              patch={patch}
            />
          )}
          {tab === "models" && <ModelsPanel settings={settings} patch={patch} />}
          {tab === "dictation" && (
            <SettingsDictationTab settings={settings} patch={patch} />
          )}
          {tab === "meeting" && <SettingsMeetingTab />}
          {tab === "mobileSync" && <SettingsMobileSyncTab />}
          {tab === "kg" && (
            <SettingsKgTab onOpenMobileSync={() => setTab("mobileSync")} />
          )}
          {tab === "permissions" && isMac && <SettingsPermissionsTab />}
          {tab === "advanced" && <AdvancedPanel settings={settings} patch={patch} />}
        </div>
      </div>
    </>
  );
}

function TabBtn({
  id,
  active,
  setActive,
  label,
}: {
  id: Tab;
  active: Tab;
  setActive: (t: Tab) => void;
  label: string;
}) {
  const isActive = id === active;
  return (
    <button
      type="button"
      className={`${styles.tab} ${isActive ? styles.tabActive : ""}`}
      aria-current={isActive ? "page" : undefined}
      onClick={() => setActive(id)}
    >
      {label}
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Reusable row + controls                                              */
/* ------------------------------------------------------------------ */

function Row({
  label,
  help,
  control,
}: {
  label: string;
  help?: string;
  control: React.ReactNode;
}) {
  return (
    <div className={styles.row}>
      <div className={styles.rowMain}>
        <span className={styles.rowLabel}>{label}</span>
        {help ? <span className={styles.rowHelp}>{help}</span> : null}
      </div>
      <div className={styles.rowControl}>{control}</div>
    </div>
  );
}

function Toggle({
  checked,
  onChange,
  ariaLabel,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  ariaLabel: string;
}) {
  return (
    <label className={styles.toggle}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        aria-label={ariaLabel}
      />
      <span className={styles.toggleTrack}>
        <span className={styles.toggleThumb} />
      </span>
    </label>
  );
}

/* ------------------------------------------------------------------ */
/* General panel                                                        */
/* ------------------------------------------------------------------ */

interface PanelProps {
  settings: SettingsSnapshot;
  patch: (
    key: string,
    value: string | boolean | number,
    next: Partial<SettingsSnapshot>,
  ) => void | Promise<void>;
}

function GeneralPanel({
  settings,
  applyTheme,
  patch,
}: PanelProps & { applyTheme: (theme: ThemeChoice) => void }) {
  const setTheme = (theme: ThemeChoice) => {
    applyTheme(theme);
    void patch("ui.theme", theme, { theme });
  };
  return (
    <>
    <Card title={t("settings.tab.general")}>
      <Row
        label={t("settings.general.theme")}
        control={
          <div className={styles.themePicker} role="radiogroup" aria-label="Theme">
            {(["system", "light", "dark"] as const).map((th) => (
              <button
                key={th}
                type="button"
                role="radio"
                aria-checked={settings.theme === th}
                className={`${styles.themeBtn} ${
                  settings.theme === th ? styles.themeBtnActive : ""
                }`}
                onClick={() => setTheme(th)}
              >
                {t(`settings.general.theme.${th}`)}
              </button>
            ))}
          </div>
        }
      />
      <Row
        label={t("settings.general.sound")}
        help={t("settings.general.sound.help")}
        control={
          <Toggle
            checked={settings.soundEnabled}
            onChange={(v) =>
              void patch("ui.sound_enabled", v, { soundEnabled: v })
            }
            ariaLabel={t("settings.general.sound")}
          />
        }
      />
      <Row
        label={t("settings.general.autostart")}
        control={
          <Toggle
            checked={settings.autostart}
            onChange={(v) => void patch("ui.autostart", v, { autostart: v })}
            ariaLabel={t("settings.general.autostart")}
          />
        }
      />
      <Row
        label={t("settings.general.reducedMotion")}
        help={t("settings.general.reducedMotion.help")}
        control={
          <Toggle
            checked={settings.reducedMotion}
            onChange={(v) =>
              void patch("ui.reduced_motion", v, { reducedMotion: v })
            }
            ariaLabel={t("settings.general.reducedMotion")}
          />
        }
      />
      {/* Phase 10 Wave 1A deferral landed in 1B: surface the
          `command_center_chord` setting so users can rebind. Stored
          as a free-form string (e.g. "RightCtrl+Space") parsed by
          `command_center::parse_chord` on the Rust side. A bad chord
          falls back to the default without breaking the app. */}
      <CommandCenterChordRow />
      {/* Phase 10 Wave 4 — Activity Capture audio toggle (ADR 0041).
          Default OFF (privacy by default). The Command Center reads
          this at session-start time; the IPC `activity_start` defaults
          to it when no explicit `withAudio` is passed. */}
      <SettingsActivityAudioRow />
      {/* Phase 10 Wave 5 — Hardening (ADR 0042 retention + ADR 0043
          exclusion rules). PDF export lives on the per-session view,
          not here. */}
      <SettingsActivityHardeningRow />
      </Card>
      <BackgroundCard />
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Background photo card -- Unsplash-powered ambient background.       */
/*                                                                    */
/* Storage:                                                           */
/* - Non-secret display prefs (enabled/mode/categories/overlay) go    */
/*   through `prefs.ts` -> localStorage.                              */
/* - API key goes through `prefs.ts` async accessors -> DPAPI         */
/*   (LR.0.B / mb-hiar, ADR 0055).                                    */
/*                                                                    */
/* This card reads both on mount, subscribes to `PREFS_EVENT` so      */
/* external writes propagate, and writes back on control change.      */
/* ------------------------------------------------------------------ */

function BackgroundCard() {
  const [prefs, setPrefsState] = useState<BackgroundPrefs>(() => getPrefs());
  // The committed API key, hydrated async from DPAPI. Empty string
  // when not yet loaded OR not configured; both states gate the
  // "key configured?" affordance to false, which is the safe default.
  const [apiKey, setApiKeyState] = useState<string>("");
  // Local mirror of the API-key input so we save on blur rather
  // than firing a DPAPI write on every keystroke.
  const [keyDraft, setKeyDraft] = useState("");

  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      setPrefsState(getPrefs());
      void getUnsplashApiKey().then((k) => {
        if (cancelled) return;
        setApiKeyState(k);
        // Only overwrite the draft when the user hasn't started
        // typing yet; we don't want a PREFS_EVENT from elsewhere
        // to clobber an in-progress edit.
        setKeyDraft((d) => (d === "" ? k : d));
      });
    };
    refresh();
    window.addEventListener(PREFS_EVENT, refresh);
    return () => {
      cancelled = true;
      window.removeEventListener(PREFS_EVENT, refresh);
    };
  }, []);

  const update = <K extends keyof BackgroundPrefs>(
    key: K,
    value: BackgroundPrefs[K],
  ) => {
    setPref(key, value);
    setPrefsState((p) => ({ ...p, [key]: value }));
  };

  const commitApiKey = (next: string) => {
    const trimmed = next.trim();
    if (trimmed === apiKey) return; // No-op, don't bother with IPC.
    if (trimmed.length === 0) {
      void clearUnsplashApiKey()
        .then(() => setApiKeyState(""))
        .catch((err) => {
          // eslint-disable-next-line no-console
          console.warn("[unsplash] clear api key failed", err);
          toast.error(t("settings.general.bg.apiKey.saveFailed"));
        });
      return;
    }
    void setUnsplashApiKey(trimmed)
      .then(() => setApiKeyState(trimmed))
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("[unsplash] set api key failed", err);
        toast.error(t("settings.general.bg.apiKey.saveFailed"));
      });
  };

  const toggleCategory = (slug: string) => {
    const has = prefs.categories.includes(slug);
    const next = has
      ? prefs.categories.filter((s) => s !== slug)
      : [...prefs.categories, slug];
    update("categories", next);
  };

  const keyConfigured = apiKey.length > 0;

  return (
    <Card title={t("settings.general.bg.title")}>
      <p
        style={{
          color: "var(--on-surf-muted)",
          margin: 0,
          font: "var(--type-sm)",
        }}
      >
        {t("settings.general.bg.help")}
      </p>

      <Row
        label={t("settings.general.bg.enabled")}
        help={
          keyConfigured
            ? undefined
            : t("settings.general.bg.enabled.needsKey")
        }
        control={
          <Toggle
            checked={prefs.enabled && keyConfigured}
            onChange={(v) => update("enabled", v)}
            ariaLabel={t("settings.general.bg.enabled")}
          />
        }
      />

      <Row
        label={t("settings.general.bg.apiKey")}
        help={t("settings.general.bg.apiKey.help")}
        control={
          <input
            type="password"
            value={keyDraft}
            onChange={(e) => setKeyDraft(e.target.value)}
            onBlur={() => commitApiKey(keyDraft)}
            placeholder={t("settings.general.bg.apiKey.placeholder")}
            spellCheck={false}
            autoComplete="off"
            style={{
              background: "var(--surf-1)",
              color: "var(--on-surf)",
              border: "1px solid var(--border)",
              borderRadius: "var(--r-2)",
              padding: "var(--s-2)",
              font: "var(--type-sm)",
              fontFamily: "var(--font-mono)",
              width: 280,
            }}
          />
        }
      />

      <Row
        label={t("settings.general.bg.mode")}
        control={
          <div
            role="radiogroup"
            aria-label={t("settings.general.bg.mode")}
            className={styles.themePicker}
          >
            {(["random", "curated"] as CurationMode[]).map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={prefs.mode === m}
                className={`${styles.themeBtn} ${
                  prefs.mode === m ? styles.themeBtnActive : ""
                }`}
                onClick={() => update("mode", m)}
              >
                {t(`settings.general.bg.mode.${m}`)}
              </button>
            ))}
          </div>
        }
      />

      {prefs.mode === "curated" ? (
        <Row
          label={t("settings.general.bg.categories")}
          help={t("settings.general.bg.categories.help")}
          control={
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fit, minmax(120px, 1fr))",
                gap: "var(--s-2)",
                width: "100%",
              }}
            >
              {CATEGORIES.map((c) => {
                const checked = prefs.categories.includes(c.slug);
                return (
                  <label
                    key={c.slug}
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "6px 10px",
                      borderRadius: "var(--r-pill)",
                      border: "1px solid var(--border)",
                      background: checked
                        ? "color-mix(in oklab, var(--mode-normal) 18%, var(--surf-1))"
                        : "var(--surf-1)",
                      cursor: "pointer",
                      font: "var(--type-sm)",
                      userSelect: "none",
                    }}
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggleCategory(c.slug)}
                      style={{ margin: 0 }}
                    />
                    {c.label}
                  </label>
                );
              })}
            </div>
          }
        />
      ) : null}
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Models panel — Ollama (local, always on) + Claude (opt-in cloud)     */
/* ------------------------------------------------------------------ */

function ModelsPanel({ settings, patch }: PanelProps) {
  return (
    <>
      <Card title={t("settings.models.ollama")}>
        <p style={{ color: "var(--on-surf-muted)", margin: 0, font: "var(--type-sm)" }}>
          {t("settings.models.ollama.help")}
        </p>
      </Card>
      <Card title={t("settings.models.claude")}>
        <Row
          label={
            settings.claudeKeyConfigured
              ? t("settings.models.claude.configured")
              : t("settings.models.claude.unconfigured")
          }
          control={
            settings.claudeKeyConfigured ? (
              <Button
                variant="danger"
                onClick={() => {
                  if (window.confirm(t("settings.models.claude.remove") + "?")) {
                    void patch("secrets.claude_key_configured", false, {
                      claudeKeyConfigured: false,
                    });
                  }
                }}
              >
                {t("settings.models.claude.remove")}
              </Button>
            ) : (
              <Button
                variant="primary"
                onClick={() => {
                  // Real flow: a modal prompts for the API key, the
                  // Rust side persists it via DPAPI + flips the
                  // configured flag. v1 just flips the flag so the UI
                  // can be exercised end-to-end without a secret store
                  // call — the modal lands in Phase 6 polish.
                  const key = window.prompt("Paste your Claude API key:");
                  if (!key) return;
                  void patch("secrets.claude_key_configured", true, {
                    claudeKeyConfigured: true,
                  });
                }}
              >
                {t("settings.models.claude.add")}
              </Button>
            )
          }
        />
      </Card>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Advanced panel — learning loop + folder shortcuts + data control     */
/* ------------------------------------------------------------------ */

function AdvancedPanel({ settings, patch }: PanelProps) {
  const [runs, setRuns] = useState<LearningRun[] | null>(null);
  const [paths, setPaths] = useState<{ dataDir: string; logsDir: string; modelsDir: string } | null>(
    null,
  );
  const [running, setRunning] = useState(false);

  useEffect(() => {
    void api.list_learning_runs(10).then(setRuns);
    void api.app_paths().then(setPaths);
  }, []);

  const triggerRun = async () => {
    setRunning(true);
    try {
      await api.trigger_learning_run();
      setRuns(await api.list_learning_runs(10));
    } finally {
      setRunning(false);
    }
  };

  return (
    <>
      <Card title={t("settings.advanced.learning")}>
        <p style={{ margin: 0, color: "var(--on-surf-muted)", font: "var(--type-sm)" }}>
          {t("settings.advanced.learning.help")}
        </p>
        <Row
          label={t("settings.advanced.learning")}
          control={
            <Toggle
              checked={settings.learningEnabled}
              onChange={(v) =>
                void patch("learning.enabled", v, { learningEnabled: v })
              }
              ariaLabel={t("settings.advanced.learning")}
            />
          }
        />
        <Row
          label={t("settings.advanced.learning.runNow")}
          control={
            <Button
              variant="primary"
              onClick={() => void triggerRun()}
              disabled={running || !settings.learningEnabled}
            >
              {running ? t("common.loading") : t("settings.advanced.learning.runNow")}
            </Button>
          }
        />
        {runs && runs.length > 0 ? (
          <>
            <span className={styles.rowLabel} style={{ marginTop: 8 }}>
              {t("settings.advanced.learning.recent")}
            </span>
            <div className={styles.learningRuns}>
              {runs.map((r) => (
                <div key={r.id} className={styles.learningRun}>
                  <span className={styles.learningRunDate}>
                    {formatRelative(r.startedAt)}
                  </span>
                  <span className={styles.learningRunStats}>
                    {r.examplesAdded ?? 0} examples ·{" "}
                    {r.dictionaryTermsAdded ?? 0} terms ·{" "}
                    {r.correctionsClassified ?? 0} corrections
                  </span>
                  <span
                    className={
                      r.rolledBack
                        ? styles.learningRunRolled
                        : styles.learningRunCommitted
                    }
                  >
                    {r.rolledBack ? "rolled back" : "committed"}
                  </span>
                </div>
              ))}
            </div>
          </>
        ) : null}
      </Card>

      <Card title="Folders">
        {paths
          ? ([
              [t("settings.advanced.dataFolder"), paths.dataDir],
              [t("settings.advanced.logsFolder"), paths.logsDir],
              [t("settings.advanced.modelsFolder"), paths.modelsDir],
            ] as const).map(([label, path]) => (
              <Row
                key={label}
                label={label}
                control={
                  <>
                    <span
                      className={styles.pathRow}
                      style={{ maxWidth: 360 }}
                      title={path}
                    >
                      <code>{path}</code>
                    </span>
                    <Button
                      onClick={() => void api.open_path(path)}
                      ariaLabel={`Open ${label}`}
                    >
                      <FolderIcon size={14} />
                    </Button>
                  </>
                }
              />
            ))
          : null}
      </Card>

      <Card title="Backups">
        <Row
          label={t("settings.advanced.export")}
          control={
            <Button
              onClick={() =>
                window.alert("Export ships in Phase 7 alongside the installer.")
              }
            >
              {t("settings.advanced.export")}
            </Button>
          }
        />
        <Row
          label={t("settings.advanced.import")}
          control={
            <Button
              onClick={() =>
                window.alert("Import ships in Phase 7 alongside the installer.")
              }
            >
              {t("settings.advanced.import")}
            </Button>
          }
        />
      </Card>

      <div className={styles.telemetryNote}>{t("settings.advanced.telemetry")}</div>
    </>
  );
}
