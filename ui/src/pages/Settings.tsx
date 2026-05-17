// Settings page — left-rail tabs, right panel renders the active tab.
// 4 tabs: General, Models, History & data, Advanced.
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
import { FolderIcon } from "../design/Icon";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import type { LearningRun, SettingsSnapshot, ThemeChoice } from "../lib/types";
import { formatRelative } from "../lib/format";

import styles from "./Settings.module.css";

type Tab = "general" | "models" | "history" | "advanced";

export function SettingsPage() {
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);
  const applyTheme = useAppStore((s) => s.applyTheme);

  const [tab, setTab] = useState<Tab>("general");

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
          <TabBtn id="history" active={tab} setActive={setTab} label={t("settings.tab.history")} />
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
          {tab === "history" && <HistoryPanel settings={settings} patch={patch} />}
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
/* History & data panel                                                 */
/* ------------------------------------------------------------------ */

function HistoryPanel({ settings, patch }: PanelProps) {
  return (
    <Card title={t("settings.tab.history")}>
      <Row
        label={t("settings.history.retentionDays")}
        control={
          <select
            className={styles.select}
            value={String(settings.retentionDays)}
            onChange={(e) =>
              void patch(
                "history.retention_days",
                e.target.value,
                { retentionDays: Number(e.target.value) },
              )
            }
            aria-label={t("settings.history.retentionDays")}
          >
            <option value="30">{t("settings.history.retention.30")}</option>
            <option value="90">{t("settings.history.retention.90")}</option>
            <option value="180">{t("settings.history.retention.180")}</option>
            <option value="365">{t("settings.history.retention.365")}</option>
            <option value="-1">{t("settings.history.retention.forever")}</option>
          </select>
        }
      />
      <Row
        label={t("settings.history.audioRetention")}
        help={t("settings.history.audioRetention.help")}
        control={
          <Toggle
            checked={settings.audioRetention}
            onChange={(v) =>
              void patch("history.audio_retention", v, { audioRetention: v })
            }
            ariaLabel={t("settings.history.audioRetention")}
          />
        }
      />
      <Row
        label={t("settings.history.purge")}
        control={
          <Button
            variant="danger"
            onClick={() => {
              const code = window.prompt(t("settings.history.purge.confirm"));
              if (code === "PURGE") {
                // Wire to a future `purge_all_history` command; for now
                // this is a no-op so the UX is testable.
                window.alert("Purge is wired but the IPC handler ships in Phase 7.");
              }
            }}
          >
            {t("settings.history.purge")}
          </Button>
        }
      />
    </Card>
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
