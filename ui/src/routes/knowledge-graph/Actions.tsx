// Phase 1D Wave 1D.5 (`mb-navi`, ADR 0052) -- KG dashboard Actions
// band.
//
// One affordance today (launch-into-Obsidian); the band exists as a
// dedicated Card so future actions (export, rebuild index, etc.) can
// stack into the same surface without inflating the dashboard's
// top-level band count.
//
// Mirror of the Settings -> KG tab's launch button. Pulls the vault
// path from `api.vault_settings_get()` (ADR 0046 single source of
// truth), disables the button + shows a tooltip if unset, surfaces
// launch errors inline.
//
// Why a sibling component to the dashboard rather than inline in
// `Dashboard.tsx`: the dashboard is already pushing 350 lines and
// every band today owns its own state + IPC; this fits the existing
// CaptureBand / FlaggedBand pattern. SOLID/SRP in the small.

import { useCallback, useEffect, useState } from "react";

import { Button, Card } from "../../components/primitives";
import { t } from "../../i18n";
import { api } from "../../lib/tauri";
import type { VaultSettingsSnapshot } from "../../lib/types";

export function ActionsBand() {
  const [vault, setVault] = useState<VaultSettingsSnapshot | null>(null);
  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api.vault_settings_get().then((v) => {
      if (!cancelled) setVault(v);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const vaultConfigured = !!vault?.vaultPath && vault.vaultPath.trim() !== "";

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
      </div>
    </Card>
  );
}
