// Phase 1D Wave 1D.5 (`mb-navi`, ADR 0052) -- KG dashboard Actions
// band.
//
// Two affordances now (Phase 1E hotfix to 1E.3/1E.4 closed `mb-43xw`
// by adding the Reconcile button):
//
//   1. Open vault in Obsidian       (1D.5)
//   2. Reconcile vault + history    (1E hotfix; this commit)
//
// Why a sibling component to the dashboard rather than inline in
// `Dashboard.tsx`: the dashboard is already pushing 350 lines and
// every band today owns its own state + IPC; this fits the existing
// CaptureBand / FlaggedBand pattern. SOLID/SRP in the small.
//
// Mirror of the Settings -> KG tab's launch button. Pulls the vault
// path from `api.vault_settings_get()` (ADR 0046 single source of
// truth), pulls the KG toggle from `api.kg_settings_get_all()` (ADR
// 0051 single source of truth for the toggle), disables buttons + shows
// tooltips when prerequisites aren't met, surfaces errors inline.

import { useCallback, useEffect, useState } from "react";

import { Button, Card } from "../../components/primitives";
import { t } from "../../i18n";
import { api } from "../../lib/tauri";
import type {
  KgHistoryReconcileReport,
  KgReconcileReport,
  KgSettings,
  VaultSettingsSnapshot,
} from "../../lib/types";

/** Combined reconcile outcome rendered as one inline banner. Both
 *  IPCs are called in lockstep (per the hotfix kickoff -- they're
 *  symmetric post-seal projections of `sessions`) so the user sees
 *  one summary instead of two separate banners. */
interface ReconcileOutcome {
  vault: KgReconcileReport;
  history: KgHistoryReconcileReport;
}

function isClean(o: ReconcileOutcome): boolean {
  return (
    o.vault.missingFileCount === 0 &&
    o.vault.sealedCount === 0 &&
    o.vault.orphanFilesCount === 0 &&
    o.history.missingSidecarCount === 0 &&
    o.history.orphanSidecarCount === 0
  );
}

export function ActionsBand() {
  const [vault, setVault] = useState<VaultSettingsSnapshot | null>(null);
  const [kgSettings, setKgSettings] = useState<KgSettings | null>(null);
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [reconciling, setReconciling] = useState(false);
  const [reconcileError, setReconcileError] = useState<string | null>(null);
  const [reconcileOutcome, setReconcileOutcome] =
    useState<ReconcileOutcome | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api.vault_settings_get().then((v) => {
      if (!cancelled) setVault(v);
    });
    void api.kg_settings_get_all().then((s) => {
      if (!cancelled) setKgSettings(s);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const vaultConfigured = !!vault?.vaultPath && vault.vaultPath.trim() !== "";
  const kgOn = kgSettings?.kgGraphEnabled === true;

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

  const onReconcile = useCallback(async () => {
    setReconciling(true);
    setReconcileError(null);
    setReconcileOutcome(null);
    try {
      // Per the hotfix kickoff: expose entries + history reconcile
      // together because they're symmetric post-seal projections of
      // `sessions`. We surface one combined banner so the operator
      // sees the full drift picture, not two half-stories. Sequential
      // (not Promise.all) so any error in the first call short-circuits
      // before the second IPC fires -- both gates are identical, so a
      // toggle-off race during the click would otherwise fire two
      // identical errors.
      const vault = await api.kg_reconcile_vault();
      const history = await api.kg_reconcile_history();
      setReconcileOutcome({ vault, history });
    } catch (err) {
      setReconcileError(String(err));
    } finally {
      setReconciling(false);
    }
  }, []);

  // Tooltip precedence: KG toggle first (the user-controlled gate),
  // vault path second (the prerequisite). Matches the order the IPC
  // resolves on the Rust side.
  const reconcileDisabledTooltip = !kgOn
    ? t("kg.dashboard.actions.reconcile.disabled.kgOff")
    : !vaultConfigured
      ? t("kg.dashboard.actions.reconcile.disabled.noVault")
      : undefined;

  return (
    <Card title={t("kg.dashboard.actions.heading")}>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "var(--s-2)",
          alignItems: "flex-start",
        }}
      >
        <Button
          variant="primary"
          onClick={() => void onLaunch()}
          disabled={!vaultConfigured || launching}
          ariaLabel={t("kg.dashboard.actions.launch")}
          title={
            vaultConfigured
              ? undefined
              : t("kg.dashboard.actions.launch.disabled.tooltip")
          }
        >
          {t("kg.dashboard.actions.launch")}
        </Button>
        {launchError ? (
          <span
            role="alert"
            style={{ color: "var(--mode-error)", font: "var(--type-sm)" }}
          >
            {t("kg.dashboard.actions.launch.error").replace(
              "{error}",
              launchError,
            )}
          </span>
        ) : null}

        <Button
          variant="ghost"
          onClick={() => void onReconcile()}
          disabled={!kgOn || !vaultConfigured || reconciling}
          ariaLabel={t("kg.dashboard.actions.reconcile")}
          title={reconcileDisabledTooltip}
        >
          {reconciling
            ? t("kg.dashboard.actions.reconcile.busy")
            : t("kg.dashboard.actions.reconcile")}
        </Button>
        {reconcileError ? (
          <span
            role="alert"
            style={{ color: "var(--mode-error)", font: "var(--type-sm)" }}
          >
            {t("kg.dashboard.actions.reconcile.error").replace(
              "{error}",
              reconcileError,
            )}
          </span>
        ) : null}
        {reconcileOutcome ? (
          <span
            role="status"
            style={{ color: "var(--on-surf-muted)", font: "var(--type-sm)" }}
          >
            {isClean(reconcileOutcome)
              ? t("kg.dashboard.actions.reconcile.clean")
              : t("kg.dashboard.actions.reconcile.summary")
                  .replace(
                    "{missingFiles}",
                    String(reconcileOutcome.vault.missingFileCount),
                  )
                  .replace(
                    "{orphans}",
                    String(reconcileOutcome.vault.orphanFilesCount),
                  )
                  .replace(
                    "{sealed}",
                    String(reconcileOutcome.vault.sealedCount),
                  )
                  .replace(
                    "{missingSidecars}",
                    String(reconcileOutcome.history.missingSidecarCount),
                  )
                  .replace(
                    "{orphanSidecars}",
                    String(reconcileOutcome.history.orphanSidecarCount),
                  )}
          </span>
        ) : null}
      </div>
    </Card>
  );
}
