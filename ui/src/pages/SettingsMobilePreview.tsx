// Settings → Advanced → Mobile Sync (preview) section.
//
// ADR 0046 Iter 2 / mb-vg3p (partial). The minimum surface area
// needed to flip mobile-sync on for smoke testing:
//
//   - Master toggle (`MobileSyncEnabled`).
//   - Vault path input (`VaultPath`) + "Browse..." button that opens
//     a native folder picker via the `vault_pick_directory` IPC.
//   - Status line (Disabled / Path not set / Path invalid / Ready).
//   - "Export now" button -> `vault_export_now`, toast on completion.
//
// The full Mobile Sync tab (record-type selector, retention dropdown,
// `VaultDebugKeepCouriers`, connection-health card, tier copy block,
// plain-language opt-in copy) defers to Iter 4. Sibling-tab parity
// with the Meeting tab would be premature now.
//
// Styling reuses the existing Settings page primitives (`Row`,
// `Toggle`) imported from `./Settings` so the spacing + typography
// matches the rest of the page without duplicating CSS.

import { useCallback, useEffect, useRef, useState } from "react";

import { Button, Card } from "../components/primitives";
import { api } from "../lib/tauri";
import type { VaultSettingsSnapshot } from "../lib/types";

import styles from "./Settings.module.css";

// Re-export-style local copies of the Row + Toggle helpers from
// Settings.tsx. Those are file-local there; rather than refactor
// them into a shared module mid-iteration, mirror the markup with
// the same class names so styling stays identical. If we end up
// growing this section to three+ rows we'll lift Row/Toggle into
// `Settings.shared.tsx` -- but that's a refactor for Iter 4, not
// now (YAGNI).

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
        role="switch"
        aria-checked={checked}
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

export type MobilePreviewStatus =
  | { kind: "loading" }
  | { kind: "disabled" }
  | { kind: "no-path" }
  | { kind: "invalid"; reason: string }
  | { kind: "ready" };

export function deriveStatus(
  snap: VaultSettingsSnapshot | null,
): MobilePreviewStatus {
  if (snap === null) return { kind: "loading" };
  if (!snap.mobileSyncEnabled) return { kind: "disabled" };
  const path = snap.vaultPath?.trim() ?? "";
  if (path === "") return { kind: "no-path" };
  // We can't synchronously stat the path from the renderer; the
  // Rust side already validates on use. Showing "Ready" optimistically
  // is fine -- the Export-now button surfaces real errors via the
  // IPC's Err path. Iter 4 will add a backend probe IPC that returns
  // an explicit "exists+writable" boolean for richer UI feedback.
  return { kind: "ready" };
}

export function statusLabel(s: MobilePreviewStatus): string {
  switch (s.kind) {
    case "loading":
      return "Loading...";
    case "disabled":
      return "Disabled";
    case "no-path":
      return "Vault path not set";
    case "invalid":
      return `Vault path invalid: ${s.reason}`;
    case "ready":
      return "Ready";
  }
}

/** Builds the toast string for an `Export now` result. Three branches:
 *  skipped (sync disabled / no path), no-op (nothing to write), and
 *  changes happened (with optional archived suffix). Exported so the
 *  unit test can pin every branch without mounting React. */
export function formatExportToast(summary: {
  total: number;
  changes: number;
  archived: number;
  skipped: boolean;
}): string {
  if (summary.skipped) {
    return "Mobile sync is disabled. Flip the toggle first.";
  }
  if (summary.changes === 0 && summary.archived === 0) {
    return `Vault up to date (${summary.total} records).`;
  }
  return (
    `Exported ${summary.changes} change${summary.changes === 1 ? "" : "s"} ` +
    `${summary.archived > 0 ? `(+${summary.archived} archived) ` : ""}` +
    `to vault.`
  );
}

export function SettingsMobilePreview() {
  const [snap, setSnap] = useState<VaultSettingsSnapshot | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Initial fetch.
  useEffect(() => {
    void api.vault_settings_get().then(setSnap);
  }, []);

  // Auto-clear toast after 3s.
  useEffect(() => {
    if (toast === null) return;
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToast(null), 3_000);
    return () => {
      if (toastTimer.current) clearTimeout(toastTimer.current);
    };
  }, [toast]);

  const persist = useCallback(
    async (
      key: keyof VaultSettingsSnapshot,
      jsonValue: unknown,
      next: Partial<VaultSettingsSnapshot>,
    ) => {
      if (!snap) return;
      // Optimistic update so the toggle / input feels instant.
      setSnap({ ...snap, ...next });
      // Translate camelCase JS key -> snake_case DB key
      // (SettingKey::as_str()).
      const dbKey = {
        mobileSyncEnabled: "mobile_sync_enabled",
        vaultPath: "vault_path",
        vaultSyncRecordTypes: "vault_sync_record_types",
        vaultRetentionDays: "vault_retention_days",
      }[key];
      try {
        await api.vault_settings_set(dbKey, jsonValue);
      } catch (e) {
        // Roll back on failure so the UI doesn't drift from the DB.
        setSnap(snap);
        setToast(`Settings save failed: ${e}`);
      }
    },
    [snap],
  );

  const handleBrowse = useCallback(async () => {
    try {
      const picked = await api.vault_pick_directory();
      if (picked) {
        await persist("vaultPath", picked, { vaultPath: picked });
      }
    } catch (e) {
      setToast(`Folder picker failed: ${e}`);
    }
  }, [persist]);

  const handleExportNow = useCallback(async () => {
    setExportBusy(true);
    try {
      const summary = await api.vault_export_now();
      setToast(formatExportToast(summary));
    } catch (e) {
      setToast(`Export failed: ${e}`);
    } finally {
      setExportBusy(false);
    }
  }, []);

  const status = deriveStatus(snap);
  const exportDisabled = status.kind !== "ready" || exportBusy;

  return (
    <Card title="Mobile Sync (preview)">
      <p
        style={{
          margin: 0,
          color: "var(--on-surf-muted)",
          font: "var(--type-sm)",
        }}
      >
        Writes transcripts to your Obsidian vault as Markdown. Preview --
        full controls land in a later iteration.
      </p>

      <Row
        label="Enable mobile sync"
        help="When on, every completed dictation + meeting is written to the vault."
        control={
          <Toggle
            checked={snap?.mobileSyncEnabled ?? false}
            onChange={(v) =>
              void persist("mobileSyncEnabled", v, { mobileSyncEnabled: v })
            }
            ariaLabel="Enable mobile sync"
          />
        }
      />

      <Row
        label="Vault folder"
        help="The Obsidian vault directory we'll write to. Pick the same folder Obsidian opens."
        control={
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input
              type="text"
              value={snap?.vaultPath ?? ""}
              placeholder="C:\\Users\\you\\mockingbird-vault"
              aria-label="Vault directory path"
              onChange={(e) =>
                void persist("vaultPath", e.target.value || null, {
                  vaultPath: e.target.value || null,
                })
              }
              style={{
                width: 320,
                padding: "6px 8px",
                background: "var(--surface-1)",
                color: "var(--on-surface)",
                border: "1px solid var(--border-subtle)",
                borderRadius: 4,
                font: "var(--type-sm)",
              }}
            />
            <Button
              variant="ghost"
              onClick={() => void handleBrowse()}
              aria-label="Browse for vault directory"
            >
              Browse...
            </Button>
          </div>
        }
      />

      <Row
        label="Status"
        control={
          <span
            aria-live="polite"
            style={{
              font: "var(--type-sm)",
              color:
                status.kind === "ready"
                  ? "var(--on-surface)"
                  : "var(--on-surf-muted)",
            }}
          >
            {statusLabel(status)}
          </span>
        }
      />

      <Row
        label="Export now"
        help="Run reconciliation on demand. Normally automatic after every save."
        control={
          <Button
            variant="primary"
            onClick={() => void handleExportNow()}
            disabled={exportDisabled}
            aria-label="Export all records to vault now"
          >
            {exportBusy ? "Exporting..." : "Export now"}
          </Button>
        }
      />

      {toast ? (
        <div
          role="status"
          aria-live="polite"
          style={{
            marginTop: 12,
            padding: "8px 12px",
            background: "var(--surface-1)",
            border: "1px solid var(--border-subtle)",
            borderRadius: 4,
            color: "var(--on-surface)",
            font: "var(--type-sm)",
          }}
        >
          {toast}
        </div>
      ) : null}
    </Card>
  );
}
