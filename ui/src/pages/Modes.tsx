// Modes editor with two distinct sections:
//
//   1. **Transcription modes** (casual / normal / formal — see
//      migration 008) — exactly
//      ONE is active at a time. The active one is what Right-Alt
//      uses for the next dictation. Card UI: a "Use this mode" radio
//      affordance replaces the legacy enable/disable toggle, the
//      per-mode hotkey badge is hidden (Right-Alt is global), and the
//      active card gets an accent border + "Active" pill.
//
//   2. **AI command modes** (rewrite / expand / summarize) — toggle
//      on/off independently. Each has its own hotkey when enabled.
//      These act on selected/clipboard text — they're not candidates
//      for the active-mode setting because there's no audio input
//      concept attached. UI: kept as-is (toggle + hotkey badge).
//
// All field edits (provider/model/temp/max-tokens) auto-save on a
// 400 ms debounce. No global "Save" button — that's a category of
// UX bug (changes lost on tab-away). Auto-save keeps the user in
// flow.

import { useCallback, useEffect, useMemo, useState } from "react";

import { PageHeader, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import { isTranscriptionSlug } from "../lib/types";
import type { ModeRow } from "../lib/types";

import styles from "./Modes.module.css";

const SAVE_DEBOUNCE_MS = 400;

/**
 * DOM id for the shared `<datalist>` of locally-installed Ollama
 * models. Every model `<input>` references it via `list=`. Kept as
 * a constant so the producer + consumer can't drift.
 */
const MODELS_DATALIST_ID = "ollama-installed-models";

/**
 * Transcription-mode slugs that the database still holds (with
 * `enabled = 0`, for historical session resolution) but that the
 * UI must NOT display. Currently: the pre-Wave-2 trio that was
 * replaced by casual/normal/formal in migration 008. Centralised
 * here so future deprecations don't have to hunt for the filter.
 */
const DEPRECATED_SLUGS: ReadonlySet<string> = new Set(["verbose", "fragment"]);

export function ModesPage() {
  const modes = useAppStore((s) => s.modes);
  const setModes = useAppStore((s) => s.setModes);
  const activeModeSlug = useAppStore((s) => s.activeModeSlug);
  const setActiveModeSlug = useAppStore((s) => s.setActiveModeSlug);

  const [savedSlug, setSavedSlug] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // Installed Ollama tags for the model dropdown. Empty = Ollama
  // unreachable; the input falls back to free-text-only.
  const [installedModels, setInstalledModels] = useState<string[]>([]);

  // First-load: re-fetch modes + active selection if not warm.
  useEffect(() => {
    if (modes.length === 0) {
      void api.list_modes().then(setModes).catch((err: unknown) => {
        setLoadError(err instanceof Error ? err.message : String(err));
      });
    }
    if (activeModeSlug === null) {
      void api
        .get_active_mode()
        .then((a) => setActiveModeSlug(a.slug))
        .catch(() => {
          // Non-fatal — fall back to displaying nothing as active.
          // The Modes UI still works; the user just sees no card
          // highlighted. Logging the error would noise up the console.
        });
    }
    // Fire-and-forget. IPC returns [] on Ollama-unreachable so we
    // never reject — no .catch needed.
    void api.list_installed_models().then(setInstalledModels);
  }, [modes.length, activeModeSlug, setModes, setActiveModeSlug]);

  const handlePatch = useCallback(
    async (slug: string, patch: Partial<ModeRow>) => {
      setModes(modes.map((m) => (m.slug === slug ? { ...m, ...patch } : m)));
      await api.update_mode(slug, patch);
      setSavedSlug(slug);
      window.setTimeout(() => setSavedSlug((s) => (s === slug ? null : s)), 1200);
    },
    [modes, setModes],
  );

  const handleSetActive = useCallback(
    async (slug: string) => {
      // Optimistic — flip the highlight immediately. If the IPC
      // fails we revert and surface the error.
      const prev = activeModeSlug;
      setActiveModeSlug(slug);
      try {
        await api.set_active_mode(slug);
        setSavedSlug(slug);
        window.setTimeout(
          () => setSavedSlug((s) => (s === slug ? null : s)),
          1200,
        );
      } catch (err) {
        setActiveModeSlug(prev);
        setLoadError(err instanceof Error ? err.message : String(err));
      }
    },
    [activeModeSlug, setActiveModeSlug],
  );

  // Split modes into the two visual groups. We do this in render
  // (not on the Rust side) because it's purely a UI concern and the
  // categorisation may change without a backend release.
  //
  // Filter the deprecated slugs (see module-level const) out of
  // BOTH visual groups. The DB rows survive for historical
  // session resolution; this is a pure UI concern.
  const { transcription, commands } = useMemo(() => {
    const transcription: ModeRow[] = [];
    const commands: ModeRow[] = [];
    for (const m of modes) {
      if (DEPRECATED_SLUGS.has(m.slug)) continue;
      if (isTranscriptionSlug(m.slug)) {
        transcription.push(m);
      } else {
        commands.push(m);
      }
    }
    return { transcription, commands };
  }, [modes]);

  if (modes.length === 0) {
    return (
      <>
        <PageHeader title={t("modes.title")} subtitle={t("modes.subtitle")} />
        {loadError ? (
          <div className={styles.errorBox} role="alert">
            <strong>Couldn&apos;t load modes.</strong>
            <pre>{loadError}</pre>
          </div>
        ) : (
          <Spinner />
        )}
      </>
    );
  }

  return (
    <>
      <PageHeader title={t("modes.title")} subtitle={t("modes.subtitle")} />
      {/*
        Single shared <datalist> for all model <input>s on the page.
        Browsers de-dup automatically when multiple inputs reference
        the same `list=` id, so this is the DRY way to wire suggestions
        for N cards. Empty list (Ollama unreachable) is fine — the
        input degrades to plain free-text.
      */}
      <datalist id={MODELS_DATALIST_ID}>
        {installedModels.map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      <div className={styles.shell}>
        {transcription.length > 0 ? (
          <section className={styles.group} aria-labelledby="modes-tx-heading">
            <header className={styles.groupHeader}>
              <h2 id="modes-tx-heading" className={styles.groupTitle}>
                {t("modes.section.transcription")}
              </h2>
              <p className={styles.groupHelp}>
                {t("modes.section.transcription.help")}
              </p>
            </header>
            <div
              className={styles.cards}
              role="radiogroup"
              aria-label={t("modes.section.transcription")}
            >
              {transcription.map((m) => (
                <ModeCard
                  key={m.slug}
                  mode={m}
                  variant="transcription"
                  isActive={m.slug === activeModeSlug}
                  justSaved={savedSlug === m.slug}
                  installedModels={installedModels}
                  onPatch={(patch) => void handlePatch(m.slug, patch)}
                  onSetActive={() => void handleSetActive(m.slug)}
                />
              ))}
            </div>
          </section>
        ) : null}

        {commands.length > 0 ? (
          <section className={styles.group} aria-labelledby="modes-cmd-heading">
            <header className={styles.groupHeader}>
              <h2 id="modes-cmd-heading" className={styles.groupTitle}>
                {t("modes.section.commands")}
              </h2>
              <p className={styles.groupHelp}>
                {t("modes.section.commands.help")}
              </p>
            </header>
            <div className={styles.cards}>
              {commands.map((m) => (
                <ModeCard
                  key={m.slug}
                  mode={m}
                  variant="command"
                  isActive={false}
                  justSaved={savedSlug === m.slug}
                  installedModels={installedModels}
                  onPatch={(patch) => void handlePatch(m.slug, patch)}
                  onSetActive={() => {
                    /* not applicable for command modes */
                  }}
                />
              ))}
            </div>
          </section>
        ) : null}
      </div>
    </>
  );
}

type CardVariant = "transcription" | "command";

interface ModeCardProps {
  mode: ModeRow;
  variant: CardVariant;
  isActive: boolean;
  justSaved: boolean;
  /**
   * Names of locally-installed Ollama models, surfaced via the
   * shared `<datalist>` for autocomplete on the model field. Empty
   * Vec = no suggestions; input still accepts any string.
   */
  installedModels: string[];
  onPatch: (patch: Partial<ModeRow>) => void;
  onSetActive: () => void;
}

function ModeCard({
  mode,
  variant,
  isActive,
  justSaved,
  installedModels,
  onPatch,
  onSetActive,
}: ModeCardProps) {
  // Local draft state for the debounced fields. We don't `setModes`
  // until the debounce fires — otherwise typing in "Temperature 0.4"
  // sends 3 IPC writes for "0", "0.", "0.4".
  const [temp, setTemp] = useState(mode.temperature.toString());
  const [maxTok, setMaxTok] = useState(mode.maxTokens.toString());
  const [model, setModel] = useState(mode.modelId);

  useEffect(() => setTemp(mode.temperature.toString()), [mode.temperature]);
  useEffect(() => setMaxTok(mode.maxTokens.toString()), [mode.maxTokens]);
  useEffect(() => setModel(mode.modelId), [mode.modelId]);

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

  const isTranscription = variant === "transcription";
  // Command modes use the legacy "disabled" dimming. Transcription
  // modes are NEVER dimmed — even the non-active ones are fully
  // configurable; "active" is just a selection, not an enablement.
  const cardClass = [
    styles.modeCard,
    !isTranscription && !mode.enabled ? styles.disabled : "",
    isActive ? styles.activeCard : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <section className={cardClass} aria-label={`Mode: ${mode.label}`}>
      <header className={styles.header}>
        <h3 className={styles.title}>
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
          {isActive ? (
            <span className={styles.activePill} aria-label={t("modes.active")}>
              {t("modes.active")}
            </span>
          ) : null}
        </h3>
        <div className={styles.headerActions}>
          {isTranscription ? (
            // Radio-like "Use this mode" button. We use a button +
            // role=radio (not a real <input type=radio>) because the
            // visual treatment is bigger than a radio dot, and screen
            // readers care about the role + aria-checked, not the
            // underlying element type.
            <button
              type="button"
              role="radio"
              aria-checked={isActive}
              className={
                isActive ? styles.useModeBtnActive : styles.useModeBtn
              }
              onClick={onSetActive}
              disabled={isActive}
            >
              {isActive ? t("modes.active") : t("modes.setActive")}
            </button>
          ) : (
            <>
              <span
                className={styles.hotkeyBadge}
                title={t("modes.field.hotkey")}
              >
                {mode.hotkey}
              </span>
              <label className={styles.toggle}>
                <input
                  type="checkbox"
                  checked={mode.enabled}
                  onChange={(e) => onPatch({ enabled: e.target.checked })}
                  aria-label={
                    mode.enabled ? t("modes.disabled") : t("modes.enabled")
                  }
                />
                <span className={styles.toggleTrack}>
                  <span className={styles.toggleThumb} />
                </span>
              </label>
            </>
          )}
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
            onChange={(e) =>
              onPatch({ provider: e.target.value as "ollama" | "claude" })
            }
          >
            <option value="ollama">Ollama (local)</option>
            <option value="claude">Claude (cloud)</option>
          </select>
        </div>

        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor={`${mode.slug}-model`}>
            {t("modes.field.model")}
          </label>
          {/*
            `list=` makes this a combobox: typing filters the
            datalist suggestions, but the user can also free-text
            any model tag (useful for cloud providers or a model
            they're about to `ollama pull`). The empty-installed
            case degrades to a plain text input — no broken UX.
          */}
          <input
            id={`${mode.slug}-model`}
            className={styles.input}
            list={MODELS_DATALIST_ID}
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder={
              installedModels.length === 0
                ? t("modes.field.model.empty")
                : undefined
            }
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
          <span className={styles.savedHint}>
            {isTranscription && isActive
              ? t("modes.activated")
              : t("modes.saved")}
          </span>
        ) : null}
      </div>
    </section>
  );
}
