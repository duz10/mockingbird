// Shared cleanup-status signposting components (DRY across the three
// isMac-gated surfaces: Settings Models card, Dictations, Modes).
//
// All copy comes from i18n; all colour/spacing from design tokens. Every
// piece here is driven by the SAME `cleanup_status` truth (via
// `useCleanupStatus`) so a user always gets one consistent story:
// "am I getting AI-cleaned text or raw, and if raw, why + the fix."

import { useCallback, useState } from "react";

import { Button, Card, Pill } from "../primitives";
import { t } from "../../i18n";
import type { CleanupStatus } from "../../lib/types";

import {
  cleanupDisplayState,
  useCleanupStatus,
  type CleanupDisplayState,
} from "./useCleanupStatus";
import styles from "./styles.module.css";

const OLLAMA_DOWNLOAD_URL = "https://ollama.com/download";

/* ------------------------------------------------------------------ */
/* Status pill — Active / Off / No model.                              */
/* ------------------------------------------------------------------ */

function pillFor(state: CleanupDisplayState): { tone: string; label: string } {
  switch (state) {
    case "active":
      return { tone: "status-ok", label: t("cleanup.pill.active") };
    case "noModel":
      return { tone: "status-idle", label: t("cleanup.pill.noModel") };
    case "ollamaDown":
    case "unknown":
    default:
      return { tone: "status-idle", label: t("cleanup.pill.off") };
  }
}

export function CleanupStatusPill({ state }: { state: CleanupDisplayState }) {
  const { tone, label } = pillFor(state);
  return <Pill tone={tone}>{label}</Pill>;
}

/* ------------------------------------------------------------------ */
/* Status line — the one-liner shown on the Settings card + reusable.  */
/* ------------------------------------------------------------------ */

function statusLineText(status: CleanupStatus | null): string {
  const state = cleanupDisplayState(status);
  switch (state) {
    case "active":
      return `${t("cleanup.status.activePrefix")}${
        status?.effectiveModel ? ` · ${status.effectiveModel}` : ""
      }`;
    case "noModel":
      return t("cleanup.status.noModel");
    case "ollamaDown":
      return t("cleanup.status.ollamaDown");
    case "unknown":
    default:
      return t("cleanup.status.checking");
  }
}

/* ------------------------------------------------------------------ */
/* Copy-to-clipboard button for the `ollama pull …` command.           */
/* ------------------------------------------------------------------ */

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const onCopy = useCallback(() => {
    // User-initiated UI copy (not a dictation paste): plain navigator
    // clipboard write, no save/restore dance required.
    void navigator.clipboard?.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  }, [text]);
  return (
    <Button variant="ghost" size="sm" onClick={onCopy}>
      {copied ? t("cleanup.copied") : t("cleanup.copy")}
    </Button>
  );
}

/* ------------------------------------------------------------------ */
/* Setup steps — the "Enable AI cleanup" block (Settings, off state).  */
/* ------------------------------------------------------------------ */

function CleanupSetupSteps({ recommendedPull }: { recommendedPull: string }) {
  const pullCmd = `ollama pull ${recommendedPull}`;
  return (
    <div className={styles.setup}>
      <p className={styles.setupTitle}>{t("cleanup.enable.title")}</p>
      <p className={styles.setupIntro}>{t("cleanup.enable.intro")}</p>
      <ol className={styles.steps}>
        <li className={styles.step}>
          {t("cleanup.enable.step1")}{" "}
          <a
            className={styles.link}
            href={OLLAMA_DOWNLOAD_URL}
            target="_blank"
            rel="noreferrer"
          >
            {t("cleanup.enable.step1.link")}
          </a>
        </li>
        <li className={styles.step}>
          {t("cleanup.enable.step2")}
          <span className={styles.cmdRow}>
            <code className={styles.cmd}>{pullCmd}</code>
            <CopyButton text={pullCmd} />
          </span>
        </li>
        <li className={styles.step}>{t("cleanup.enable.step3")}</li>
      </ol>
      <p className={styles.ramNote}>{t("cleanup.enable.ramNote")}</p>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* SURFACE A — the Settings → Models "Cleanup engine" card.            */
/* Source of truth; re-checks on window focus (via the hook).          */
/* ------------------------------------------------------------------ */

export function CleanupEngineCard() {
  const { status, state } = useCleanupStatus();
  return (
    <Card title={t("cleanup.engine.title")}>
      <div className={styles.statusRow}>
        <CleanupStatusPill state={state} />
        <p className={styles.statusText}>{statusLineText(status)}</p>
      </div>
      {state !== "active" && state !== "unknown" ? (
        <CleanupSetupSteps
          recommendedPull={status?.recommendedPull ?? "qwen2.5:3b"}
        />
      ) : null}
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* SURFACE B (part 1) — per-dictation badge.                           */
/* Driven by the dictation's OWN recorded model, not the live status:  */
/* a dictation saved raw last week stays "Raw" even if cleanup is now  */
/* active. `"passthrough"` (or empty) === raw.                         */
/* ------------------------------------------------------------------ */

/** Pure: is a recorded `model_used` value a raw/passthrough dictation? */
export function isPassthroughModel(modelUsed: string | null | undefined): boolean {
  const m = (modelUsed ?? "").trim().toLowerCase();
  return m === "" || m === "passthrough";
}

export function DictationCleanupBadge({
  modelUsed,
}: {
  modelUsed: string | null | undefined;
}) {
  const raw = isPassthroughModel(modelUsed);
  return (
    <span
      className={`${styles.badge} ${raw ? styles.badgeRaw : styles.badgeCleaned}`}
      title={raw ? undefined : modelUsed ?? undefined}
    >
      {raw ? t("cleanup.badge.raw") : t("cleanup.badge.cleaned")}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* SURFACE B (part 2) — dismissible passthrough banner (Dictations).   */
/* Shows only when the CURRENT cleanup state is passthrough and the    */
/* user hasn't dismissed it. Dismissal persists in localStorage.       */
/* ------------------------------------------------------------------ */

const BANNER_DISMISS_KEY = "mb.cleanup.passthroughBannerDismissed";

function bannerDismissed(): boolean {
  try {
    return window.localStorage.getItem(BANNER_DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}

export function CleanupPassthroughBanner({ onSetup }: { onSetup: () => void }) {
  const { state } = useCleanupStatus();
  const [dismissed, setDismissed] = useState(bannerDismissed);

  const dismiss = useCallback(() => {
    try {
      window.localStorage.setItem(BANNER_DISMISS_KEY, "1");
    } catch {
      /* localStorage unavailable — dismiss for this session only. */
    }
    setDismissed(true);
  }, []);

  // Only nag when cleanup is genuinely off (not while still checking).
  const isPassthrough = state === "ollamaDown" || state === "noModel";
  if (!isPassthrough || dismissed) return null;

  return (
    <div className={styles.banner} role="note">
      <p className={styles.bannerText}>{t("cleanup.banner.passthrough")}</p>
      <div className={styles.bannerActions}>
        <button type="button" className={styles.linkBtn} onClick={onSetup}>
          {t("cleanup.banner.cta")}
        </button>
        <Button variant="ghost" size="sm" onClick={dismiss}>
          {t("cleanup.banner.dismiss")}
        </Button>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* SURFACE C — inline Modes notice near the model pickers.             */
/* Shows only when cleanup is passthrough (Ollama offline / no model). */
/* ------------------------------------------------------------------ */

export function CleanupModesNotice({ onSetup }: { onSetup: () => void }) {
  const { state } = useCleanupStatus();
  const isPassthrough = state === "ollamaDown" || state === "noModel";
  if (!isPassthrough) return null;
  return (
    <div className={styles.notice} role="note">
      <span>{t("cleanup.modes.notice")}</span>
      <button type="button" className={styles.linkBtn} onClick={onSetup}>
        {t("cleanup.modes.cta")}
      </button>
    </div>
  );
}
