// Settings -> Knowledge Graph tab.
//
// Phase 1C Wave 1C.1 (`mb-s6a8`, ADR 0051 D2) shipped the
// `KgGraphEnabled` activation toggle. Wave 1C.2 (`mb-9ufg`) added
// an inline Filing-status + failed-filings panel beneath it.
//
// Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) **subtracted** the
// failed-filings panel from this tab and relocated it onto the KG
// dashboard's Flagged-for-review band.
//
// Phase 1D Wave 1D.5 (`mb-navi`, ADR 0052) fleshes the tab back out
// with read-only KG-flavoured reference rows (none of which need a
// new SQL surface):
//
//   * Vault path display -- pulls `vaultPath` from the Mobile Sync
//     settings (ADR 0046 single source of truth). The tab does NOT
//     edit the path; an inline button cross-navigates to the Mobile
//     Sync tab via `onOpenMobileSync` (Settings.tsx owns the
//     `setTab` state).
//   * Vocabularies review -- the v1 controlled categories +
//     entry types (read-only). Sourced from `kg_vocabularies_get`
//     so the displayed list cannot drift from what the pipeline
//     actually emits; the Rust IPC is statically derived from the
//     `Category` / `EntryType` enums and pinned by a unit test.
//   * Processing-mode indicator -- single-line label per spec
//     §15.5 ("ingest mode: silent").
//   * Dual-write copy -- one-line reminder of how audio vs text
//     notes route into Dictations vs KG (matches the dashboard's
//     CaptureBand subtitle).
//   * Launch-into-Obsidian button -- mirror of the KG dashboard's
//     `Actions` band. Enabled iff a vault path is configured;
//     disabled (+ tooltip) otherwise.
//
// State source-of-truth: typed Rust `Settings` facade via
// `api.kg_settings_get_all()` / `api.kg_settings_set()` for the
// toggle, `api.vault_settings_get()` for the vault path,
// `api.kg_vocabularies_get()` for the lists, `api.kg_launch_obsidian()`
// for the button click. Pattern mirrors `SettingsMeetingTab`.

import { useCallback, useEffect, useState } from "react";

import { Button, Card, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import type {
  KgSettings,
  VaultSettingsSnapshot,
  Vocabularies,
} from "../lib/types";

import styles from "./Settings.module.css";

export interface SettingsKgTabProps {
  /** Callback that flips the parent's tab state to the Mobile Sync
   *  tab. Optional so the component can be mounted directly in
   *  isolation tests; in production wiring (`Settings.tsx`) it
   *  always gets a real handler. */
  onOpenMobileSync?: () => void;
}

export function SettingsKgTab({ onOpenMobileSync }: SettingsKgTabProps = {}) {
  const [snap, setSnap] = useState<KgSettings | null>(null);
  const [savingError, setSavingError] = useState<string | null>(null);
  // Wave 1D.5: vault path + vocabularies are sibling reads to the
  // KG settings snapshot. They live in component state rather than
  // the global app store because (a) no other surface reads them
  // off the store today, and (b) they're cheap (one IPC each, one
  // time, on mount). Promote later if a second consumer appears.
  const [vault, setVault] = useState<VaultSettingsSnapshot | null>(null);
  const [vocab, setVocab] = useState<Vocabularies | null>(null);
  const [vocabError, setVocabError] = useState<string | null>(null);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launching, setLaunching] = useState(false);
  // Phase 1E Wave 1E.1 (mb-e16d, ADR 0053 D1) -- inline error from
  // the toggle-on bootstrap step. Lives next to savingError so the
  // user sees "toggle saved but subtree creation failed" as ONE
  // surface, not two stacked banners.
  const [bootstrapError, setBootstrapError] = useState<string | null>(null);

  // Phase 1D Wave 1D.2 (ADR 0052) -- broadcast toggle flips to the
  // app store so the Sidebar's KG nav item appears/disappears
  // reactively.
  const setKgGraphEnabledInStore = useAppStore((s) => s.setKgGraphEnabled);

  // Single mount effect that kicks off all three reads in parallel.
  // No `Promise.all` -- each setter is independent and we want
  // partial render-on-first-arrival rather than block-until-all-done.
  useEffect(() => {
    let cancelled = false;
    void api.kg_settings_get_all().then((s) => {
      if (cancelled) return;
      setSnap(s);
      setKgGraphEnabledInStore(s.kgGraphEnabled);
    });
    void api.vault_settings_get().then((v) => {
      if (!cancelled) setVault(v);
    });
    void api
      .kg_vocabularies_get()
      .then((v) => {
        if (!cancelled) {
          setVocab(v);
          setVocabError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) setVocabError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [setKgGraphEnabledInStore]);

  const patch = useCallback(
    async <K extends keyof KgSettings>(key: K, value: KgSettings[K]) => {
      setSnap((prev) => (prev ? { ...prev, [key]: value } : prev));
      if (key === "kgGraphEnabled") {
        setKgGraphEnabledInStore(value as boolean);
      }
      try {
        await api.kg_settings_set(toDbKey(key), value);
        setSavingError(null);
      } catch (err) {
        setSavingError(String(err));
        const fresh = await api.kg_settings_get_all();
        setSnap(fresh);
        setKgGraphEnabledInStore(fresh.kgGraphEnabled);
        return;
      }
      // Phase 1E Wave 1E.1 (mb-e16d, ADR 0053 D1) -- on a successful
      // OFF -> ON toggle, fire the vault subtree bootstrap. The Rust
      // side is idempotent + safe to call from boot too, so the
      // "failed silently because some race" worst case is recovered
      // on the next app boot. We still surface failures inline so
      // the user has a clear next action (fix the vault path or
      // disk perms) without having to restart.
      if (key === "kgGraphEnabled" && value === true) {
        try {
          await api.kg_subtree_bootstrap();
          setBootstrapError(null);
        } catch (err) {
          setBootstrapError(String(err));
        }
      } else if (key === "kgGraphEnabled" && value === false) {
        // Toggle-off leaves the subtree on disk (per ADR 0053 D1:
        // "user content lives there; never destructively cleaned
        // up"). Clear any stale bootstrap error so the next on-flip
        // gets a fresh banner.
        setBootstrapError(null);
      }
    },
    [setKgGraphEnabledInStore],
  );

  const onLaunch = useCallback(async () => {
    setLaunching(true);
    setLaunchError(null);
    try {
      await api.kg_launch_obsidian();
    } catch (err) {
      setLaunchError(String(err));
    } finally {
      setLaunching(false);
    }
  }, []);

  // Settings snapshot is the only true blocker; vault + vocab can
  // render their loading states inline rather than spinner-gating
  // the whole tab.
  if (!snap) return <Spinner />;

  const enabled = snap.kgGraphEnabled;
  const vaultConfigured = !!vault?.vaultPath && vault.vaultPath.trim() !== "";

  return (
    <div className={styles.stack}>
      {savingError ? (
        <div className={styles.errorBanner} role="alert">
          {t("kg.settings.saveError").replace("{error}", savingError)}
        </div>
      ) : null}

      {bootstrapError ? (
        <div className={styles.errorBanner} role="alert">
          {t("kg.settings.bootstrapError").replace("{error}", bootstrapError)}
        </div>
      ) : null}

      <Card title={t("kg.settings.title")}>
        <p
          style={{
            margin: 0,
            color: "var(--on-surf-muted)",
            font: "var(--type-sm)",
          }}
        >
          {t("kg.settings.explainer")}
        </p>

        <Row label={t("kg.settings.enabled.label")}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => void patch("kgGraphEnabled", e.target.checked)}
              aria-label={t("kg.settings.enabled.label")}
            />
            <span>
              {enabled
                ? t("kg.settings.enabled.on")
                : t("kg.settings.enabled.off")}
            </span>
          </label>
        </Row>

        {enabled ? <PostEnableNotice /> : null}
      </Card>

      {/* ---- Vault (read-only mirror of Mobile Sync settings) ---- */}
      <Card title={t("kg.settings.vault.heading")}>
        <Row label={t("kg.settings.vault.pathLabel")}>
          {vault === null ? (
            <Spinner />
          ) : vaultConfigured ? (
            <span
              className={styles.pathRow}
              title={vault.vaultPath ?? undefined}
              style={{ maxWidth: 360 }}
            >
              <code>{vault.vaultPath}</code>
            </span>
          ) : (
            <span
              style={{
                color: "var(--on-surf-muted)",
                font: "var(--type-sm)",
              }}
            >
              {t("kg.settings.vault.unset")}
            </span>
          )}
        </Row>

        {!vaultConfigured && vault !== null ? (
          <Row label="">
            <div
              style={{
                display: "flex",
                gap: "var(--s-2)",
                alignItems: "center",
                font: "var(--type-sm)",
                color: "var(--on-surf-muted)",
              }}
            >
              <span>{t("kg.settings.vault.unset.help")}</span>
              {onOpenMobileSync ? (
                <Button
                  size="sm"
                  onClick={onOpenMobileSync}
                  ariaLabel={t("kg.settings.vault.openMobileSync")}
                >
                  {t("kg.settings.vault.openMobileSync")}
                </Button>
              ) : null}
            </div>
          </Row>
        ) : null}

        <Row label="">
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--s-2)" }}>
            <Button
              variant="primary"
              onClick={() => void onLaunch()}
              disabled={!vaultConfigured || launching}
              ariaLabel={t("kg.settings.vault.launch")}
              title={
                vaultConfigured
                  ? undefined
                  : t("kg.settings.vault.launch.disabled.tooltip")
              }
            >
              {t("kg.settings.vault.launch")}
            </Button>
            {launchError ? (
              <span role="alert" style={{ color: "var(--mode-error)", font: "var(--type-sm)" }}>
                {t("kg.settings.vault.launch.error").replace("{error}", launchError)}
              </span>
            ) : null}
          </div>
        </Row>

        <p
          style={{
            margin: 0,
            color: "var(--on-surf-muted)",
            font: "var(--type-xs)",
          }}
        >
          {t("kg.settings.vault.singleSourceNote")}
        </p>
      </Card>

      {/* ---- Vocabularies (read-only display) ---- */}
      <Card title={t("kg.settings.vocab.heading")}>
        <p
          style={{
            margin: 0,
            color: "var(--on-surf-muted)",
            font: "var(--type-sm)",
          }}
        >
          {t("kg.settings.vocab.subtitle")}
        </p>
        {vocabError ? (
          <div className={styles.errorBanner} role="alert">
            {t("kg.settings.vocab.loadError").replace("{error}", vocabError)}
          </div>
        ) : vocab === null ? (
          <Spinner />
        ) : (
          <>
            <VocabList
              heading={t("kg.settings.vocab.categories")}
              values={vocab.categories}
            />
            <VocabList
              heading={t("kg.settings.vocab.entryTypes")}
              values={vocab.entryTypes}
            />
          </>
        )}
      </Card>

      {/* ---- Processing mode (informational) ---- */}
      <Card title={t("kg.settings.mode.heading")}>
        <Row label={t("kg.settings.mode.indicator")}>
          <span
            style={{
              color: "var(--on-surf-muted)",
              font: "var(--type-sm)",
            }}
          >
            {t("kg.settings.mode.body")}
          </span>
        </Row>
      </Card>

      {/* ---- Dual-write info ---- */}
      <Card title={t("kg.settings.dualWrite.heading")}>
        <p
          style={{
            margin: 0,
            color: "var(--on-surf-muted)",
            font: "var(--type-sm)",
          }}
        >
          {t("kg.settings.dualWrite.body")}
        </p>
      </Card>
    </div>
  );
}

function PostEnableNotice() {
  return (
    <div
      role="status"
      aria-live="polite"
      aria-label={t("kg.settings.notice.title")}
      style={{
        marginTop: "var(--s-3)",
        background: "var(--surf-1)",
        borderLeft: "3px solid var(--mode-normal)",
        borderRadius: "var(--r-2)",
        padding: "var(--s-2) var(--s-3)",
      }}
    >
      <div
        style={{
          font: "var(--type-sm)",
          fontWeight: 600,
          color: "var(--on-surf)",
          marginBottom: "var(--s-1)",
        }}
      >
        {t("kg.settings.notice.title")}
      </div>
      <div style={{ font: "var(--type-sm)", color: "var(--on-surf-muted)" }}>
        {t("kg.settings.notice.body")}
      </div>
    </div>
  );
}

function VocabList({ heading, values }: { heading: string; values: string[] }) {
  return (
    <div style={{ marginTop: "var(--s-2)" }}>
      <div
        style={{
          font: "var(--type-sm)",
          fontWeight: 600,
          color: "var(--on-surf)",
          marginBottom: "var(--s-1)",
        }}
      >
        {heading}
      </div>
      <ul
        aria-label={heading}
        style={{
          margin: 0,
          paddingLeft: "var(--s-4)",
          color: "var(--on-surf-muted)",
          font: "var(--type-sm)",
        }}
      >
        {values.map((v) => (
          <li key={v}>{v}</li>
        ))}
      </ul>
    </div>
  );
}

// Map the TS camelCase field name onto the DB-form SettingKey string
// the Rust allowlist matches.
function toDbKey(key: keyof KgSettings): string {
  switch (key) {
    case "kgGraphEnabled":
      return "kg_graph_enabled";
  }
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
