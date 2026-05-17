// Modes editor. One card per mode; edits are debounced + auto-saved
// per field. No global "Save" button — that's a category of UX bug
// (changes lost on tab-away). Auto-save keeps the user in flow.
//
// Hotkey is rendered as a non-editable badge for v1: capturing live
// key chords cleanly in a browser is a rabbit hole, and the Rust
// hotkey driver only listens for RightAlt today (Phase 5 polish item).

import { useCallback, useEffect, useMemo, useState } from "react";

import { PageHeader, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import type { ModeRow } from "../lib/types";

import styles from "./Modes.module.css";

const SAVE_DEBOUNCE_MS = 400;

export function ModesPage() {
  const modes = useAppStore((s) => s.modes);
  const setModes = useAppStore((s) => s.setModes);

  // Local copy keyed by slug so per-mode edits don't re-render siblings.
  const [savedSlug, setSavedSlug] = useState<string | null>(null);

  // First-load: the App boot already populates the store, but if the
  // user lands directly on this page (e.g., refresh), we re-fetch.
  useEffect(() => {
    if (modes.length === 0) {
      void api.list_modes().then(setModes);
    }
  }, [modes.length, setModes]);

  const handlePatch = useCallback(
    async (slug: string, patch: Partial<ModeRow>) => {
      // Optimistic update — apply locally first so the input doesn't
      // jitter back to the old value while the IPC round-trips.
      setModes(
        modes.map((m) => (m.slug === slug ? { ...m, ...patch } : m)),
      );
      await api.update_mode(slug, patch);
      setSavedSlug(slug);
      window.setTimeout(() => setSavedSlug((s) => (s === slug ? null : s)), 1200);
    },
    [modes, setModes],
  );

  if (modes.length === 0) {
    return (
      <>
        <PageHeader
          title={t("modes.title")}
          subtitle={t("modes.subtitle")}
        />
        <Spinner />
      </>
    );
  }

  return (
    <>
      <PageHeader title={t("modes.title")} subtitle={t("modes.subtitle")} />
      <div className={styles.shell}>
        {modes.map((m) => (
          <ModeCard
            key={m.slug}
            mode={m}
            justSaved={savedSlug === m.slug}
            onPatch={(patch) => void handlePatch(m.slug, patch)}
          />
        ))}
      </div>
    </>
  );
}

function ModeCard({
  mode,
  justSaved,
  onPatch,
}: {
  mode: ModeRow;
  justSaved: boolean;
  onPatch: (patch: Partial<ModeRow>) => void;
}) {
  // Local draft state for the debounced fields. We don't `setModes`
  // until the debounce fires — otherwise typing in "Temperature 0.4"
  // sends 3 IPC writes for "0", "0.", "0.4".
  const [temp, setTemp] = useState(mode.temperature.toString());
  const [maxTok, setMaxTok] = useState(mode.maxTokens.toString());
  const [model, setModel] = useState(mode.modelId);

  // Keep local in sync if the upstream changes (e.g., another tab).
  useEffect(() => setTemp(mode.temperature.toString()), [mode.temperature]);
  useEffect(() => setMaxTok(mode.maxTokens.toString()), [mode.maxTokens]);
  useEffect(() => setModel(mode.modelId), [mode.modelId]);

  // Debounce numeric / text fields.
  useEffect(() => {
    if (model === mode.modelId) return;
    const h = window.setTimeout(() => onPatch({ modelId: model }), SAVE_DEBOUNCE_MS);
    return () => window.clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model]);

  useEffect(() => {
    const parsed = Number.parseFloat(temp);
    if (!Number.isFinite(parsed)) return;
    if (parsed === mode.temperature) return;
    const h = window.setTimeout(
      () => onPatch({ temperature: parsed }),
      SAVE_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [temp]);

  useEffect(() => {
    const parsed = Number.parseInt(maxTok, 10);
    if (!Number.isFinite(parsed)) return;
    if (parsed === mode.maxTokens) return;
    const h = window.setTimeout(
      () => onPatch({ maxTokens: parsed }),
      SAVE_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [maxTok]);

  const modeColor = useMemo(
    () => `var(--mode-${mode.slug}, var(--mode-normal))`,
    [mode.slug],
  );

  return (
    <section
      className={`${styles.modeCard} ${!mode.enabled ? styles.disabled : ""}`}
      aria-label={`Mode: ${mode.label}`}
    >
      <header className={styles.header}>
        <h2 className={styles.title}>
          <span
            style={{
              display: "inline-block",
              width: 10,
              height: 10,
              borderRadius: "50%",
              background: modeColor,
            }}
          />
          {mode.label}
        </h2>
        <div className={styles.headerActions}>
          <span className={styles.hotkeyBadge} title={t("modes.field.hotkey")}>
            {mode.hotkey}
          </span>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={mode.enabled}
              onChange={(e) => onPatch({ enabled: e.target.checked })}
              aria-label={mode.enabled ? t("modes.disabled") : t("modes.enabled")}
            />
            <span className={styles.toggleTrack}>
              <span className={styles.toggleThumb} />
            </span>
          </label>
        </div>
      </header>

      <div className={styles.fields}>
        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor={`${mode.slug}-provider`}>
            {t("modes.field.provider")}
          </label>
          <select
            id={`${mode.slug}-provider`}
            className={styles.select}
            value={mode.provider}
            onChange={(e) => onPatch({ provider: e.target.value as "ollama" | "claude" })}
          >
            <option value="ollama">Ollama (local)</option>
            <option value="claude">Claude (cloud)</option>
          </select>
        </div>

        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor={`${mode.slug}-model`}>
            {t("modes.field.model")}
          </label>
          <input
            id={`${mode.slug}-model`}
            className={styles.input}
            value={model}
            onChange={(e) => setModel(e.target.value)}
          />
        </div>

        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor={`${mode.slug}-temp`}>
            {t("modes.field.temperature")}
          </label>
          <input
            id={`${mode.slug}-temp`}
            className={styles.input}
            type="number"
            min="0"
            max="2"
            step="0.05"
            value={temp}
            onChange={(e) => setTemp(e.target.value)}
          />
        </div>

        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor={`${mode.slug}-max`}>
            {t("modes.field.maxTokens")}
          </label>
          <input
            id={`${mode.slug}-max`}
            className={styles.input}
            type="number"
            min="64"
            max="32000"
            step="64"
            value={maxTok}
            onChange={(e) => setMaxTok(e.target.value)}
          />
        </div>
      </div>

      <div className={styles.saveRow}>
        {justSaved ? (
          <span className={styles.savedHint}>{t("modes.saved")}</span>
        ) : null}
      </div>
    </section>
  );
}
