// macOS-only per-mode LLM PROMPT editor (ADR 0067).
//
// WHY this exists: power users want to tune the cleanup prompt per mode.
// The shipped prompt bodies are immutable (migration-seeded, append-only
// per ADR 0008); this editor stores a user EDIT in a separate override
// layer (`mode_prompt_overrides`, migration 030) so the shipped defaults
// stay the source of truth and Revert is a clean delete. At dictation
// time a user override is used VERBATIM and skips the small-model tier
// substitution (precedence: user override > tier > default).
//
// Self-fetching: it owns its `get_effective_prompt` round-trip so the
// parent (Modes.tsx) only renders <MacPromptEditor slug=... /> — keeping
// the already-large Modes page small. Gated to macOS + dictation modes by
// the caller. Non-macOS never mounts this → Windows byte-identical.

import { useCallback, useEffect, useState } from "react";

import { t } from "../i18n";
import { api } from "../lib/tauri";
import type { EffectivePrompt } from "../lib/types";

import styles from "./Modes.module.css";

interface MacPromptEditorProps {
  /** Mode slug whose prompt is being edited (normal/casual/formal). */
  slug: string;
}

/**
 * Collapsible prompt editor: a textarea seeded with the effective prompt
 * (user override if set, else the shipped default), a Save that persists
 * the override, and a Revert that clears it. A badge makes it obvious
 * whether the user is viewing the shipped default or their custom edit.
 */
export function MacPromptEditor({ slug }: MacPromptEditorProps) {
  const [open, setOpen] = useState(false);
  const [prompt, setPrompt] = useState<EffectivePrompt | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [savedTick, setSavedTick] = useState(false);

  const load = useCallback(() => {
    void api.get_effective_prompt(slug).then((p) => {
      setPrompt(p);
      setDraft(p.effectiveBody);
    });
  }, [slug]);

  // Fetch lazily on first expand — most users never open the editor, so
  // we avoid N extra IPC round-trips on the Modes page load.
  useEffect(() => {
    if (open && prompt === null) load();
  }, [open, prompt, load]);

  const handleSave = useCallback(async () => {
    if (draft.trim().length === 0) return;
    setBusy(true);
    try {
      await api.set_mode_prompt_override(slug, draft);
      setSavedTick(true);
      window.setTimeout(() => setSavedTick(false), 1500);
      load();
    } finally {
      setBusy(false);
    }
  }, [slug, draft, load]);

  const handleRevert = useCallback(async () => {
    setBusy(true);
    try {
      await api.clear_mode_prompt_override(slug);
      load();
    } finally {
      setBusy(false);
    }
  }, [slug, load]);

  const dirty = prompt !== null && draft !== prompt.effectiveBody;
  const canSave = dirty && draft.trim().length > 0 && !busy;

  return (
    <div className={styles.promptEditor}>
      <button
        type="button"
        className={styles.promptToggle}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {open ? "▾" : "▸"} {t("modes.prompt.title")}
        {prompt?.isOverridden ? (
          <span className={styles.promptBadgeCustom}>
            {t("modes.prompt.badge.custom")}
          </span>
        ) : null}
      </button>

      {open ? (
        prompt === null ? (
          <p className={styles.promptHint}>{t("modes.prompt.loading")}</p>
        ) : (
          <>
            <p className={styles.promptHint}>
              {prompt.isOverridden
                ? t("modes.prompt.statusCustom")
                : `${t("modes.prompt.statusDefault")} (v${prompt.defaultVersion})`}
            </p>
            <textarea
              className={styles.promptTextarea}
              value={draft}
              spellCheck={false}
              rows={10}
              onChange={(e) => setDraft(e.target.value)}
              aria-label={t("modes.prompt.title")}
            />
            <div className={styles.promptActions}>
              <button
                type="button"
                className={styles.promptSaveBtn}
                disabled={!canSave}
                onClick={() => void handleSave()}
              >
                {t("modes.prompt.save")}
              </button>
              <button
                type="button"
                className={styles.revertBtn}
                disabled={busy || !prompt.isOverridden}
                onClick={() => void handleRevert()}
              >
                {t("modes.prompt.revert")}
              </button>
              {savedTick ? (
                <span className={styles.savedHint}>{t("modes.prompt.saved")}</span>
              ) : null}
            </div>
          </>
        )
      ) : null}
    </div>
  );
}
