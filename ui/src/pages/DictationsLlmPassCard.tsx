// On-demand LLM-pass card for the Dictations detail pane.
//
// Extracted from Dictations.tsx as part of Phase 1C Wave 1C.3
// (`mb-5ly5`) so the parent file gets back under the 600-LoC
// reviewability ceiling once the per-row KG chips + filter bar
// land. Behavior is byte-for-byte identical to the inline version
// the parent shipped through Wave 1C.2 -- this is a pure
// relocation + prop-threading change, no logic rewrites.
//
// Why a per-page card (vs. lifted to design tokens): same rationale
// as before -- both this surface and the meeting LLM-pass card are
// still tuning their visual identity. When they stabilize they
// graduate to the design system together.

import { useEffect, useRef, useState } from "react";

import { Button, Card } from "../components/primitives";
import { LlmRunButton } from "../components/LlmRunButton";
import { CheckIcon, CopyIcon } from "../design/Icon";
import { t } from "../i18n";
import { api } from "../lib/tauri";

import styles from "./Dictations.module.css";

/** The set of built-in LLM prompts the Dictations detail card
 *  exposes. Kept narrow because each prompt is an additional UX
 *  surface to maintain; new entries land via ADR. */
export type DictLlmPrompt =
  | "summary"
  | "action_items"
  | "cleaner_punctuation"
  | "compress";

const LLM_PROMPT_OPTIONS: Array<{ id: DictLlmPrompt; labelKey: string }> = [
  { id: "summary", labelKey: "meetings.llm.prompt.summary" },
  { id: "action_items", labelKey: "meetings.llm.prompt.action_items" },
  {
    id: "cleaner_punctuation",
    labelKey: "meetings.llm.prompt.cleaner_punctuation",
  },
  // ADR 0047 §Wave 2.6 -- the pull-only "tighten this up" Transform.
  // Lives next to the other built-ins because the picker shape is
  // identical; what makes Compress different is the prompt body,
  // not the IPC plumbing.
  { id: "compress", labelKey: "meetings.llm.prompt.compress" },
];

/* ------------------------------------------------------------------ */
/* Minimal LLM-output renderer.                                       */
/*                                                                    */
/* Our action_items / summary / cleaner_punctuation prompts emit at   */
/* most: paragraphs and `- ` bullet lists. We render exactly those    */
/* two shapes -- no react-markdown dep, no nested-list support, no    */
/* inline emphasis parsing. YAGNI: if a future prompt ever needs      */
/* tables or headings, swap in a real markdown lib then. Until then   */
/* this stays a ~40-line pure transform.                              */
/*                                                                    */
/* The Copy button copies `result.text` (the source markdown), so     */
/* paste-into-other-apps still gets dash bullets, not stripped text.  */
/* ------------------------------------------------------------------ */

function LlmMarkdownView({ text }: { text: string }) {
  const blocks = text
    .split(/\n\s*\n/)
    .map((b) => b.trim())
    .filter(Boolean);

  return (
    <div className={styles.llmResultMd}>
      {blocks.map((block, i) => {
        const lines = block
          .split("\n")
          .map((l) => l.trim())
          .filter(Boolean);
        const isBulletList =
          lines.length > 0 &&
          lines.every((l) => l.startsWith("- ") || l.startsWith("* "));
        if (isBulletList) {
          return (
            <ul key={i} className={styles.llmResultList}>
              {lines.map((l, j) => (
                <li key={j}>{l.slice(2)}</li>
              ))}
            </ul>
          );
        }
        return (
          <p key={i} className={styles.llmResultPara}>
            {block}
          </p>
        );
      })}
    </div>
  );
}

/** Optional, off the critical path. Calls the dictation LLM-pass IPC
 *  with a built-in prompt and renders the result. Nothing is
 *  persisted -- same invariant as the meeting LLM pass. */
export function DictationsLlmPassCard({ sessionId }: { sessionId: number }) {
  const [prompt, setPrompt] = useState<DictLlmPrompt>("summary");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<{ text: string; latencyMs: number } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number | null>(null);

  // Clear local state when the user navigates to a different
  // session. The LLM pass is per-session; carrying over a previous
  // result into a new session would be misleading.
  useEffect(() => {
    setResult(null);
    setError(null);
    setRunning(false);
    setCopied(false);
  }, [sessionId]);

  useEffect(() => {
    return () => {
      if (copyTimer.current) window.clearTimeout(copyTimer.current);
    };
  }, []);

  async function runPass() {
    setRunning(true);
    setError(null);
    try {
      const r = await api.dictation_run_llm_pass(sessionId, prompt);
      setResult({ text: r.text, latencyMs: r.latencyMs });
    } catch (e) {
      // Surface the backend error message verbatim -- it's already
      // a human-readable string from `commands::into_err`.
      setError(String(e));
    } finally {
      setRunning(false);
    }
  }

  async function copyOutput() {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.text);
    } catch {
      // Fall back silently -- the user can still select + copy by
      // hand. Not worth a toast for a 1-in-1000 permission edge.
      return;
    }
    setCopied(true);
    if (copyTimer.current) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopied(false), 1200);
  }

  return (
    <Card title={t("dictations.llm.title")}>
      <div className={styles.llmControls}>
        <label className={styles.llmPromptLabel}>
          {t("dictations.llm.prompt.label")}
          <select
            className={styles.llmPromptSelect}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value as DictLlmPrompt)}
            disabled={running}
          >
            {LLM_PROMPT_OPTIONS.map((opt) => (
              <option key={opt.id} value={opt.id}>
                {t(opt.labelKey)}
              </option>
            ))}
          </select>
        </label>
        <LlmRunButton
          onClick={runPass}
          running={running}
          idleLabel={t("dictations.llm.run")}
          runningLabel={t("dictations.llm.running")}
        />
      </div>

      {error ? (
        <div className={styles.llmError} role="alert">
          {t("dictations.llm.error").replace("{message}", error)}
        </div>
      ) : null}

      {result ? (
        <div className={styles.llmResult}>
          <div className={styles.llmResultMeta}>
            <span>
              {t("dictations.llm.latency").replace(
                "{ms}",
                String(result.latencyMs),
              )}
            </span>
            <Button onClick={copyOutput} ariaLabel={t("dictations.llm.copy")}>
              {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
              {copied ? t("dictations.llm.copied") : t("dictations.llm.copy")}
            </Button>
          </div>
          <LlmMarkdownView text={result.text} />
        </div>
      ) : !running && !error ? (
        <p className={styles.llmHelp}>{t("dictations.llm.notRun")}</p>
      ) : null}
    </Card>
  );
}
