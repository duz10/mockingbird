// ADR 0046 Iter 4 / mb-vg3p — Mobile Sync settings tab.
//
// Lifted out of the inline preview section that used to live under
// Settings → Advanced (deleted in this same commit). Surfaces every
// ADR 0046 user-facing knob in one place:
//
//   1. Status card    — overall on/off + vault path + live runtime
//                       health (outbound projection + inbox courier).
//   2. Toggle + copy  — `MobileSyncEnabled` with plain-language
//                       opt-in explanation.
//   3. Advanced       — collapsed `<details>` exposing the four
//                       remaining keys: `VaultSyncBackend`,
//                       `SyncTierByteCap`, `KeepAudioBlobs`,
//                       `VaultDebugKeepCouriers`.
//   4. Setup help     — pointer to `docs/mobile/ios-shortcut.md`
//                       and the Obsidian Sync diagnostic.
//
// Decision logic is extracted into pure exported functions so the
// vitest suite can pin every branch without mounting React (same
// convention as `MeetingRecordBar.test.ts`). Live IPC + 5s polling
// is exercised by Playwright in the broader Iter 4 polish judges.

import { useCallback, useEffect, useRef, useState } from "react";

import { Button, Card, Pill } from "../components/primitives";
import { api } from "../lib/tauri";
import type {
  InboxRuntimeStatus,
  VaultRuntimeStatus,
  VaultSettingsSnapshot,
  VaultSyncBackend,
} from "../lib/types";

import {
  NestedVaultWizard,
  type NestedVaultInfo,
} from "./NestedVaultWizard";
import styles from "./Settings.module.css";

// --------------------------------------------------------------------
// Pure helpers (exported for unit testing)
// --------------------------------------------------------------------

/** Camel-case → snake_case for every settings key surfaced by this
 *  tab. Mirror of `SettingKey::as_str()` on the Rust side; the
 *  `vault_settings_set` IPC validates against this exact set, so
 *  any drift surfaces as a 400-style error toast. */
export const VAULT_DB_KEYS: Record<keyof VaultSettingsSnapshot, string> = {
  mobileSyncEnabled: "mobile_sync_enabled",
  vaultPath: "vault_path",
  vaultSyncRecordTypes: "vault_sync_record_types",
  vaultRetentionDays: "vault_retention_days",
  vaultSyncBackend: "vault_sync_backend",
  syncTierByteCap: "sync_tier_byte_cap",
  keepAudioBlobs: "keep_audio_blobs",
  vaultDebugKeepCouriers: "vault_debug_keep_couriers",
};

export type OverallStatus =
  | { kind: "loading" }
  | { kind: "disabled" }
  | { kind: "no-path" }
  | { kind: "ready" };

export function deriveOverallStatus(
  snap: VaultSettingsSnapshot | null,
): OverallStatus {
  if (snap === null) return { kind: "loading" };
  if (!snap.mobileSyncEnabled) return { kind: "disabled" };
  const path = snap.vaultPath?.trim() ?? "";
  if (path === "") return { kind: "no-path" };
  return { kind: "ready" };
}

export function statusLabel(s: OverallStatus): string {
  switch (s.kind) {
    case "loading":
      return "Loading…";
    case "disabled":
      return "Off";
    case "no-path":
      return "On — vault folder not set";
    case "ready":
      return "On";
  }
}

/** Default per-file byte cap by sync backend. Matches Obsidian Sync
 *  Standard's 5 MB ceiling, Plus's 200 MB ceiling, and a sentinel
 *  zero for Manual (which has no enforced cap — the value is a
 *  warning threshold, not a hard limit). */
export function defaultByteCapForBackend(backend: VaultSyncBackend): number {
  switch (backend) {
    case "obsidian-sync-standard":
      return 5_000_000;
    case "obsidian-sync-plus":
      return 200_000_000;
    case "manual":
      return 0;
  }
}

/** Display a byte count as megabytes with at most one decimal place.
 *  `0` renders as "no cap" because that's how the Manual backend
 *  encodes "warn-on-nothing". */
export function formatByteCapMB(bytes: number): string {
  if (bytes === 0) return "no cap";
  const mb = bytes / 1_000_000;
  // Drop trailing ".0" so 5 MB doesn't render as "5.0 MB".
  const rendered = Number.isInteger(mb) ? `${mb}` : mb.toFixed(1);
  return `${rendered} MB`;
}

/** Parse a megabyte string back into bytes for the IPC. Returns
 *  `null` on garbage input so the caller can keep the existing
 *  value rather than clobbering with NaN. Empty string maps to
 *  `0` (the Manual "no cap" sentinel). */
export function parseByteCapMB(input: string): number | null {
  const trimmed = input.trim();
  if (trimmed === "") return 0;
  const mb = Number.parseFloat(trimmed);
  if (!Number.isFinite(mb) || mb < 0) return null;
  return Math.round(mb * 1_000_000);
}

/** Format an age-in-milliseconds as a short relative label suitable
 *  for status pills ("12s ago", "3m ago", "1h ago", "yesterday").
 *  `null` short-circuits to "never". */
export function formatRelativeMs(ms: number | null): string {
  if (ms === null) return "never";
  const s = Math.max(0, Math.floor(ms / 1_000));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d === 1) return "yesterday";
  return `${d}d ago`;
}

/** Compose the courier sub-status pill copy. Three branches:
 *  not-running, running-never-archived, running-with-history. */
export function inboxBadge(
  s: InboxRuntimeStatus | null,
): { tone: string; label: string } {
  if (s === null) return { tone: "status-warn", label: "Loading…" };
  if (!s.running) return { tone: "on-surf-muted", label: "Stopped" };
  if (s.lastArchivedIso === null) {
    return { tone: "status-ok", label: "Watching" };
  }
  // Don't try to compute age client-side off an ISO string — we
  // surface the ISO directly via the `title` attribute on the
  // status card; the badge stays brief.
  return { tone: "status-ok", label: "Watching" };
}

/** Compose the outbound projection sub-status pill copy. */
export function outboundBadge(
  s: VaultRuntimeStatus | null,
): { tone: string; label: string } {
  if (s === null) return { tone: "status-warn", label: "Loading…" };
  if (!s.running) return { tone: "on-surf-muted", label: "Stopped" };
  if (s.lastError !== null) return { tone: "status-warn", label: "Error" };
  if (s.manifestAgeMs === null) {
    return { tone: "status-ok", label: "Idle" };
  }
  return { tone: "status-ok", label: formatRelativeMs(s.manifestAgeMs) };
}

// --------------------------------------------------------------------
// Row / Toggle helpers (mirror of the file-local helpers in
// Settings.tsx — keeping markup identical so spacing + typography
// matches the rest of the page).
// --------------------------------------------------------------------

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

// --------------------------------------------------------------------
// Tab component
// --------------------------------------------------------------------

/** Poll cadence for the runtime-status IPCs while the tab is
 *  mounted. 5 s matches the spec; cheap on the Rust side (manifest
 *  stat + a couple of dir scans). */
const STATUS_POLL_MS = 5_000;

export function SettingsMobileSyncTab() {
  const [snap, setSnap] = useState<VaultSettingsSnapshot | null>(null);
  const [vaultStatus, setVaultStatus] = useState<VaultRuntimeStatus | null>(
    null,
  );
  const [inboxStatus, setInboxStatus] = useState<InboxRuntimeStatus | null>(
    null,
  );
  const [exportBusy, setExportBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Initial fetch + 5s polling of runtime status while mounted.
  useEffect(() => {
    void api.vault_settings_get().then(setSnap);
    let cancelled = false;
    const refresh = async () => {
      try {
        const [v, i] = await Promise.all([
          api.vault_runtime_status(),
          api.inbox_runtime_status(),
        ]);
        if (!cancelled) {
          setVaultStatus(v);
          setInboxStatus(i);
        }
      } catch {
        // Surface nothing — the badges fall back to their loading
        // tone; the next tick will retry. We deliberately don't
        // toast here because a transient IPC blip during shutdown
        // shouldn't startle the user.
      }
    };
    void refresh();
    const id = setInterval(() => void refresh(), STATUS_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
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
    async <K extends keyof VaultSettingsSnapshot>(
      key: K,
      jsonValue: unknown,
      next: Pick<VaultSettingsSnapshot, K>,
    ) => {
      if (!snap) return;
      // Optimistic update so toggles feel instant.
      const prev = snap;
      setSnap({ ...snap, ...next });
      try {
        await api.vault_settings_set(VAULT_DB_KEYS[key], jsonValue);
      } catch (e) {
        // Roll back so the UI doesn't drift from the DB.
        setSnap(prev);
        setToast(`Settings save failed: ${String(e)}`);
      }
    },
    [snap],
  );

  // ADR 0046 Iter 4 / mb-3xww — nested-vault wizard state. When
  // the user picks a folder INSIDE an existing Obsidian vault, we
  // open the wizard with the detection result + defer persistence
  // until the user picks a branch.
  const [nestedInfo, setNestedInfo] = useState<NestedVaultInfo | null>(null);

  const tryPersistVaultPath = useCallback(
    async (candidate: string) => {
      try {
        const check = await api.vault_check_path(candidate);
        if (check.kind === "nestedVault") {
          setNestedInfo({
            candidatePath: candidate,
            parentVault: check.parentVault,
            suggestedSibling: check.suggestedSibling,
          });
          return;
        }
        await persist("vaultPath", candidate, { vaultPath: candidate });
      } catch (e) {
        setToast(`Vault path check failed: ${String(e)}`);
      }
    },
    [persist],
  );

  const handleBrowse = useCallback(async () => {
    try {
      const picked = await api.vault_pick_directory();
      if (picked) {
        await tryPersistVaultPath(picked);
      }
    } catch (e) {
      setToast(`Folder picker failed: ${String(e)}`);
    }
  }, [tryPersistVaultPath]);

  // Wizard callbacks. Kept here (not inside the JSX) so the deps
  // are explicit and React doesn't see fresh closures every render.
  const handleNestedAccept = useCallback(
    async (
      finalPath: string,
      { acceptedNested }: { acceptedNested: boolean },
    ) => {
      setNestedInfo(null);
      try {
        await persist("vaultPath", finalPath, { vaultPath: finalPath });
        if (acceptedNested) {
          // Out-of-scope: a persisted "accepted nested-vault risk"
          // flag. For now we just log a tracing breadcrumb via
          // a toast (visible to the user) so support has something
          // to grep for. Promote to a SettingKey if churn shows up.
          setToast(
            "Saved nested vault path. Watch for Obsidian sync conflicts.",
          );
        }
      } catch (e) {
        setToast(`Save failed: ${String(e)}`);
      }
    },
    [persist],
  );

  const handleNestedPickDifferent = useCallback(() => {
    setNestedInfo(null);
    // Re-open the picker on the next tick so React has time to
    // unmount the dialog before the native modal grabs focus.
    setTimeout(() => void handleBrowse(), 0);
  }, [handleBrowse]);

  const handleExportNow = useCallback(async () => {
    setExportBusy(true);
    try {
      const summary = await api.vault_export_now();
      if (summary.skipped) {
        setToast("Mobile sync is disabled. Flip the toggle first.");
      } else if (summary.changes === 0 && summary.archived === 0) {
        setToast(`Vault up to date (${summary.total} records).`);
      } else {
        const changes = `${summary.changes} change${summary.changes === 1 ? "" : "s"}`;
        const archived = summary.archived > 0 ? ` (+${summary.archived} archived)` : "";
        setToast(`Exported ${changes}${archived} to vault.`);
      }
    } catch (e) {
      setToast(`Export failed: ${String(e)}`);
    } finally {
      setExportBusy(false);
    }
  }, []);

  /** Backend swap flips the persisted byte-cap value to the new
   *  default IFF the cap currently sits at the OUTGOING backend's
   *  default (i.e. the user hasn't manually customised it). Avoids
   *  silently clobbering an explicitly-set 50 MB cap when the user
   *  switches between tiers. */
  const handleBackendChange = useCallback(
    async (next: VaultSyncBackend) => {
      if (!snap) return;
      const prevDefault = defaultByteCapForBackend(snap.vaultSyncBackend);
      const newDefault = defaultByteCapForBackend(next);
      const newCap =
        snap.syncTierByteCap === prevDefault ? newDefault : snap.syncTierByteCap;
      // Persist backend first, then cap if it changed.
      await persist("vaultSyncBackend", next, { vaultSyncBackend: next });
      if (newCap !== snap.syncTierByteCap) {
        await persist("syncTierByteCap", newCap, { syncTierByteCap: newCap });
      }
    },
    [snap, persist],
  );

  const status = deriveOverallStatus(snap);
  const outBadge = outboundBadge(vaultStatus);
  const inBadge = inboxBadge(inboxStatus);
  const exportDisabled = status.kind !== "ready" || exportBusy;

  return (
    <>
      {/* --------------- Status card --------------- */}
      <Card title="Mobile Sync">
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
          label="Vault folder"
          help="The Obsidian vault directory we mirror your records into."
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
                aria-label="Change vault directory"
              >
                Change…
              </Button>
            </div>
          }
        />

        <Row
          label="Outbound projection"
          help="Mockingbird → vault. Writes a Markdown record for every saved dictation + meeting."
          control={
            <span title={vaultStatus?.lastError ?? undefined}>
              <Pill tone={outBadge.tone}>{outBadge.label}</Pill>
            </span>
          }
        />

        <Row
          label="Inbox courier"
          help="Watches `<vault>/inbox/` for new audio files dropped by the iOS Shortcut."
          control={
            <span title={inboxStatus?.watchPath ?? undefined}>
              <Pill tone={inBadge.tone}>{inBadge.label}</Pill>
            </span>
          }
        />

        {inboxStatus && inboxStatus.failedCount > 0 ? (
          <Row
            label="Failed files"
            help={`${inboxStatus.failedCount} file${inboxStatus.failedCount === 1 ? "" : "s"} in inbox/_failed/. Click to inspect.`}
            control={
              <Button
                variant="ghost"
                aria-label="Open inbox/_failed/ folder"
                onClick={() => {
                  // Reuse `vault_pick_directory`'s sibling IPC if we
                  // ever ship one; for now surface the path via toast
                  // so the user can paste it into Explorer. Keeps
                  // this tab dependency-free from the file-open
                  // plumbing the Advanced tab uses.
                  if (inboxStatus.watchPath) {
                    setToast(`${inboxStatus.watchPath}\\_failed`);
                  }
                }}
              >
                Show path
              </Button>
            }
          />
        ) : null}

        <Row
          label="Manual reconcile"
          help="Force-run reconciliation. Normally automatic after every save."
          control={
            <Button
              variant="primary"
              onClick={() => void handleExportNow()}
              disabled={exportDisabled}
              aria-label="Export all records to vault now"
            >
              {exportBusy ? "Exporting…" : "Export now"}
            </Button>
          }
        />
      </Card>

      {/* --------------- Opt-in toggle + copy --------------- */}
      <Card title="Enable">
        <p
          style={{
            margin: "0 0 12px 0",
            color: "var(--on-surf-muted)",
            font: "var(--type-sm)",
            lineHeight: 1.5,
          }}
        >
          Mockingbird mirrors your dictation and meeting history into the vault
          folder you choose, and watches the <code>inbox/</code> subfolder for
          new audio files from your phone. Everything stays local to your
          devices; Obsidian Sync (or whatever sync mechanism you pick) handles
          device-to-device transfer end-to-end encrypted.
        </p>
        <Row
          label="Enable mobile sync"
          help="When on, every completed dictation and meeting is written to the vault."
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
      </Card>

      {/* --------------- Advanced --------------- */}
      <Card title="Advanced">
        <details>
          <summary
            style={{
              cursor: "pointer",
              font: "var(--type-sm)",
              color: "var(--on-surf-muted)",
              margin: "0 0 12px 0",
            }}
          >
            Backend, byte cap, audio retention, debug
          </summary>

          <Row
            label="Sync backend"
            help="Selecting your Obsidian Sync tier sets a sensible default for the byte cap."
            control={
              <select
                aria-label="Sync backend"
                value={snap?.vaultSyncBackend ?? "obsidian-sync-standard"}
                onChange={(e) =>
                  void handleBackendChange(e.target.value as VaultSyncBackend)
                }
                style={{
                  padding: "6px 8px",
                  background: "var(--surface-1)",
                  color: "var(--on-surface)",
                  border: "1px solid var(--border-subtle)",
                  borderRadius: 4,
                  font: "var(--type-sm)",
                }}
              >
                <option value="obsidian-sync-standard">
                  Obsidian Sync · Standard (5 MB per file)
                </option>
                <option value="obsidian-sync-plus">
                  Obsidian Sync · Plus (200 MB per file)
                </option>
                <option value="manual">Manual (no built-in sync)</option>
              </select>
            }
          />

          <Row
            label="Per-file warning"
            help="Records over this size show a warning. 0 disables the warning entirely (Manual backend default)."
            control={
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input
                  type="number"
                  min={0}
                  step="0.5"
                  value={
                    snap === null
                      ? ""
                      : snap.syncTierByteCap === 0
                        ? "0"
                        : `${snap.syncTierByteCap / 1_000_000}`
                  }
                  aria-label="Per-file warning threshold in megabytes"
                  onChange={(e) => {
                    const parsed = parseByteCapMB(e.target.value);
                    if (parsed === null) return;
                    void persist("syncTierByteCap", parsed, {
                      syncTierByteCap: parsed,
                    });
                  }}
                  style={{
                    width: 100,
                    padding: "6px 8px",
                    background: "var(--surface-1)",
                    color: "var(--on-surface)",
                    border: "1px solid var(--border-subtle)",
                    borderRadius: 4,
                    font: "var(--type-sm)",
                  }}
                />
                <span
                  style={{
                    font: "var(--type-sm)",
                    color: "var(--on-surf-muted)",
                  }}
                >
                  MB ({formatByteCapMB(snap?.syncTierByteCap ?? 0)})
                </span>
              </div>
            }
          />

          <Row
            label="Keep audio after ingest"
            help="When on, original audio is preserved in inbox/_archive/. Turn off to delete after successful ingest (saves space)."
            control={
              <Toggle
                checked={snap?.keepAudioBlobs ?? true}
                onChange={(v) =>
                  void persist("keepAudioBlobs", v, { keepAudioBlobs: v })
                }
                ariaLabel="Keep audio after ingest"
              />
            }
          />

          <Row
            label="Keep intermediate courier files (debug)"
            help="Developer toggle. Leaves intermediate processing artefacts on disk for inspection."
            control={
              <Toggle
                checked={snap?.vaultDebugKeepCouriers ?? false}
                onChange={(v) =>
                  void persist("vaultDebugKeepCouriers", v, {
                    vaultDebugKeepCouriers: v,
                  })
                }
                ariaLabel="Keep intermediate courier files (debug)"
              />
            }
          />
        </details>
      </Card>

      {/* --------------- Setup help --------------- */}
      <Card title="Setup help">
        <p
          style={{
            margin: "0 0 8px 0",
            color: "var(--on-surf-muted)",
            font: "var(--type-sm)",
            lineHeight: 1.5,
          }}
        >
          To capture voice memos from your phone:
        </p>
        <ol
          style={{
            margin: "0 0 12px 18px",
            padding: 0,
            color: "var(--on-surf-muted)",
            font: "var(--type-sm)",
            lineHeight: 1.6,
          }}
        >
          <li>
            Install the Mockingbird iOS Shortcut — full walkthrough in{" "}
            <code>docs/mobile/ios-shortcut.md</code>.
          </li>
          <li>
            Configure the Shortcut to save into <code>&lt;vault&gt;/inbox/</code>.
          </li>
          <li>
            On a slow sync, check Obsidian Sync's log: <em>Settings →
            Sync → ⋯ → Show sync log</em>.
          </li>
        </ol>
      </Card>

      {/* --------------- Toast --------------- */}
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

      {/* ADR 0046 Iter 4 / mb-3xww — nested-vault wizard. Rendered
          inline so the dim-out backdrop covers the Settings page; we
          deliberately don't portal-mount because there's no other
          DOM that could end up above this dialog in normal use. */}
      {nestedInfo && (
        <NestedVaultWizard
          info={nestedInfo}
          onAccept={(p, opts) => void handleNestedAccept(p, opts)}
          onPickDifferent={handleNestedPickDifferent}
          onCancel={() => setNestedInfo(null)}
        />
      )}
    </>
  );
}
