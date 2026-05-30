// Settings -> Knowledge Graph tab.
//
// Phase 1C Wave 1C.1 (`mb-s6a8`, ADR 0051 D2) shipped the
// `KgGraphEnabled` activation toggle. Wave 1C.2 (`mb-9ufg`) added
// an inline Filing-status + failed-filings panel beneath it.
//
// Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) **subtracts** the
// failed-filings panel from this tab and relocates it onto the KG
// dashboard's Flagged-for-review band, where the rest of the
// queue UX already lives. The Settings tab now retains only the
// activation toggle (plus Wave 1D.5's vault-path / vocabularies /
// launch-into-Obsidian additions, which arrive in the next wave).
//
// Rationale: the failed-filings UX is KG plumbing, and after Wave
// 1D.2 the KG dashboard is the canonical home for KG plumbing.
// Keeping it duplicated on the Settings tab would be a SOLID/SRP
// violation in the small (two surfaces, one concern, one would
// drift) and a discoverability bug in the large (users would
// look for failed filings in two places). The toggle stays here
// because activation is a global preference -- it's where every
// other privacy/feature toggle lives.
//
// State source-of-truth: the typed Rust `Settings` facade reached
// via `api.kg_settings_get_all()` / `api.kg_settings_set()`. Per-
// tick worker poll (Wave 1C.1 / D6) means a flip here takes effect
// within ~5s of the next worker tick -- no restart required, no
// confirmation modal (D2 calls for single-tap reversible).
//
// Pattern mirrors `SettingsMeetingTab` (Phase MC Wave 5): typed
// snap fetched once on mount, optimistic local flip + persist,
// reload from the server on persist error, inline error banner
// (no toast -- the existing settings UX is banner-based).

import { useCallback, useEffect, useState } from "react";

import { Card, Spinner } from "../components/primitives";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import { api } from "../lib/tauri";
import type { KgSettings } from "../lib/types";

import styles from "./Settings.module.css";

export function SettingsKgTab() {
  const [snap, setSnap] = useState<KgSettings | null>(null);
  const [savingError, setSavingError] = useState<string | null>(null);
  // Phase 1D Wave 1D.2 (ADR 0052) -- broadcast toggle flips to the
  // app store so the Sidebar's KG nav item appears/disappears
  // reactively. The store is the source-of-truth for cross-page KG
  // visibility; this tab is one of two writers (the App-boot fetch
  // in App.tsx is the other / initial). Mirrors the same pattern
  // settings -> theme reactivity uses.
  const setKgGraphEnabledInStore = useAppStore((s) => s.setKgGraphEnabled);

  useEffect(() => {
    let cancelled = false;
    void api.kg_settings_get_all().then((s) => {
      if (!cancelled) {
        setSnap(s);
        // Re-sync the app store on tab mount -- handles the case
        // where another process (CLI, future external API) flipped
        // the setting since boot.
        setKgGraphEnabledInStore(s.kgGraphEnabled);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [setKgGraphEnabledInStore]);

  // Patch the one KG setting. Optimistic local flip + persist; on
  // persist error revert by re-reading from the server (mirror
  // SettingsMeetingTab.patch). The Rust-side allowlist
  // (`is_kg_setting_allowed_for_ui` in `commands/kg.rs`) currently
  // accepts only `kg_graph_enabled`; any future KG setting added
  // here must also be added there.
  const patch = useCallback(
    async <K extends keyof KgSettings>(key: K, value: KgSettings[K]) => {
      setSnap((prev) => (prev ? { ...prev, [key]: value } : prev));
      // Optimistically update the cross-page store too so the
      // Sidebar's KG nav item flips in the same React tick as the
      // toggle. If the persist fails, the catch block rolls both
      // back from the server snapshot.
      if (key === "kgGraphEnabled") {
        setKgGraphEnabledInStore(value as boolean);
      }
      try {
        // SettingKey::as_str for KgGraphEnabled is "kg_graph_enabled".
        await api.kg_settings_set(toDbKey(key), value);
        setSavingError(null);
      } catch (err) {
        setSavingError(String(err));
        const fresh = await api.kg_settings_get_all();
        setSnap(fresh);
        setKgGraphEnabledInStore(fresh.kgGraphEnabled);
      }
    },
    [setKgGraphEnabledInStore],
  );

  if (!snap) return <Spinner />;

  const enabled = snap.kgGraphEnabled;

  return (
    <div className={styles.stack}>
      {savingError ? (
        <div className={styles.errorBanner} role="alert">
          {t("kg.settings.saveError").replace("{error}", savingError)}
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

        {/* D2: post-enable notice. Only rendered when the toggle is
            ON -- the off-state copy on the toggle itself is enough
            to explain the off-state. Empirical copy: the 1C.0
            baseline measured p95 = 59s for a 5-segment dictation,
            so "about a minute" is honest. Visually distinct from
            the error banner via a neutral surface + mode-normal
            accent border. */}
        {enabled ? (
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
            <div
              style={{
                font: "var(--type-sm)",
                color: "var(--on-surf-muted)",
              }}
            >
              {t("kg.settings.notice.body")}
            </div>
          </div>
        ) : null}
      </Card>

      {/* Wave 1D.4 subtractive change: the SettingsKgFailedFilings
          card relocated to the KG dashboard's Flagged-for-review
          band. See `ui/src/routes/knowledge-graph/Dashboard.tsx`
          (FlaggedBand) for the current home of the queue + retry
          UX. */}
    </div>
  );
}

// Map the TS camelCase field name onto the DB-form SettingKey string
// the Rust allowlist matches. Kept as a tiny pure function so a
// future KG setting addition is a one-line extension here + a
// matching line in `is_kg_setting_allowed_for_ui`.
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
