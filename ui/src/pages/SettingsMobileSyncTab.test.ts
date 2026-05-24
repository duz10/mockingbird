// ADR 0046 Iter 4 / mb-vg3p — unit tests for the pure helpers behind
// the Mobile Sync settings tab. Same convention as the preceding
// SettingsMobilePreview tests: no React mounting, no IPC stubbing —
// just pin every branch of the decision logic.

import { describe, expect, it } from "vitest";

import {
  defaultByteCapForBackend,
  deriveOverallStatus,
  formatByteCapMB,
  formatRelativeMs,
  inboxBadge,
  outboundBadge,
  parseByteCapMB,
  statusLabel,
  VAULT_DB_KEYS,
} from "./SettingsMobileSyncTab";
import type { VaultSettingsSnapshot } from "../lib/types";

const baseSnap: VaultSettingsSnapshot = {
  mobileSyncEnabled: false,
  vaultPath: null,
  vaultSyncRecordTypes: "both",
  vaultRetentionDays: 30,
  vaultSyncBackend: "obsidian-sync-standard",
  syncTierByteCap: 5_000_000,
  keepAudioBlobs: true,
  vaultDebugKeepCouriers: false,
};

describe("VAULT_DB_KEYS", () => {
  it("covers every camelCase field on the settings snapshot", () => {
    // Pin every key in lock-step with the Rust `SettingKey::as_str()`
    // mapping. If a new key lands on the snapshot type, this test
    // forces the dev to add the mirror entry here.
    expect(VAULT_DB_KEYS).toEqual({
      mobileSyncEnabled: "mobile_sync_enabled",
      vaultPath: "vault_path",
      vaultSyncRecordTypes: "vault_sync_record_types",
      vaultRetentionDays: "vault_retention_days",
      vaultSyncBackend: "vault_sync_backend",
      syncTierByteCap: "sync_tier_byte_cap",
      keepAudioBlobs: "keep_audio_blobs",
      vaultDebugKeepCouriers: "vault_debug_keep_couriers",
    });
  });
});

describe("deriveOverallStatus", () => {
  it("returns loading when no snapshot has arrived yet", () => {
    expect(deriveOverallStatus(null)).toEqual({ kind: "loading" });
  });

  it("returns disabled when the toggle is off", () => {
    expect(
      deriveOverallStatus({ ...baseSnap, mobileSyncEnabled: false }),
    ).toEqual({ kind: "disabled" });
  });

  it("returns no-path when enabled but path is null", () => {
    expect(
      deriveOverallStatus({
        ...baseSnap,
        mobileSyncEnabled: true,
        vaultPath: null,
      }),
    ).toEqual({ kind: "no-path" });
  });

  it("returns no-path when enabled but path is whitespace-only", () => {
    expect(
      deriveOverallStatus({
        ...baseSnap,
        mobileSyncEnabled: true,
        vaultPath: "   ",
      }),
    ).toEqual({ kind: "no-path" });
  });

  it("returns ready when enabled with a non-empty path", () => {
    expect(
      deriveOverallStatus({
        ...baseSnap,
        mobileSyncEnabled: true,
        vaultPath: "C:\\Users\\you\\mockingbird-vault",
      }),
    ).toEqual({ kind: "ready" });
  });
});

describe("statusLabel", () => {
  it("formats each kind to a user-visible string", () => {
    expect(statusLabel({ kind: "loading" })).toBe("Loading…");
    expect(statusLabel({ kind: "disabled" })).toBe("Off");
    expect(statusLabel({ kind: "no-path" })).toBe("On — vault folder not set");
    expect(statusLabel({ kind: "ready" })).toBe("On");
  });
});

describe("defaultByteCapForBackend", () => {
  it("maps Standard to 5 MB", () => {
    expect(defaultByteCapForBackend("obsidian-sync-standard")).toBe(5_000_000);
  });
  it("maps Plus to 200 MB", () => {
    expect(defaultByteCapForBackend("obsidian-sync-plus")).toBe(200_000_000);
  });
  it("maps Manual to the 0 (no-cap) sentinel", () => {
    expect(defaultByteCapForBackend("manual")).toBe(0);
  });
});

describe("formatByteCapMB", () => {
  it("renders 0 as 'no cap'", () => {
    expect(formatByteCapMB(0)).toBe("no cap");
  });
  it("renders a whole-MB count without trailing decimal", () => {
    expect(formatByteCapMB(5_000_000)).toBe("5 MB");
    expect(formatByteCapMB(200_000_000)).toBe("200 MB");
  });
  it("keeps a single decimal for non-integer megabytes", () => {
    expect(formatByteCapMB(7_500_000)).toBe("7.5 MB");
  });
});

describe("parseByteCapMB", () => {
  it("maps empty string to the 0 sentinel", () => {
    expect(parseByteCapMB("")).toBe(0);
    expect(parseByteCapMB("   ")).toBe(0);
  });
  it("parses whole megabytes", () => {
    expect(parseByteCapMB("5")).toBe(5_000_000);
  });
  it("parses fractional megabytes via rounding", () => {
    expect(parseByteCapMB("7.5")).toBe(7_500_000);
  });
  it("rejects garbage by returning null", () => {
    expect(parseByteCapMB("abc")).toBeNull();
    expect(parseByteCapMB("-5")).toBeNull();
  });
});

describe("formatRelativeMs", () => {
  it("returns 'never' on null", () => {
    expect(formatRelativeMs(null)).toBe("never");
  });
  it("returns 'just now' under 5 s", () => {
    expect(formatRelativeMs(0)).toBe("just now");
    expect(formatRelativeMs(4_999)).toBe("just now");
  });
  it("returns seconds under a minute", () => {
    expect(formatRelativeMs(12_000)).toBe("12s ago");
    expect(formatRelativeMs(59_000)).toBe("59s ago");
  });
  it("returns minutes under an hour", () => {
    expect(formatRelativeMs(60_000)).toBe("1m ago");
    expect(formatRelativeMs(59 * 60_000)).toBe("59m ago");
  });
  it("returns hours under a day", () => {
    expect(formatRelativeMs(60 * 60_000)).toBe("1h ago");
    expect(formatRelativeMs(23 * 60 * 60_000)).toBe("23h ago");
  });
  it("returns 'yesterday' at exactly 1 day", () => {
    expect(formatRelativeMs(24 * 60 * 60_000)).toBe("yesterday");
  });
  it("returns days otherwise", () => {
    expect(formatRelativeMs(3 * 24 * 60 * 60_000)).toBe("3d ago");
  });
});

describe("outboundBadge", () => {
  it("shows Loading on null status", () => {
    expect(outboundBadge(null).label).toBe("Loading…");
  });
  it("shows Stopped when runtime is off", () => {
    expect(
      outboundBadge({
        running: false,
        manifestAgeMs: null,
        manifestModifiedIso: null,
        lastError: null,
      }).label,
    ).toBe("Stopped");
  });
  it("shows Idle when running with no manifest yet", () => {
    expect(
      outboundBadge({
        running: true,
        manifestAgeMs: null,
        manifestModifiedIso: null,
        lastError: null,
      }).label,
    ).toBe("Idle");
  });
  it("renders the relative manifest age when running and healthy", () => {
    expect(
      outboundBadge({
        running: true,
        manifestAgeMs: 12_000,
        manifestModifiedIso: "2026-05-27T00:00:00Z",
        lastError: null,
      }).label,
    ).toBe("12s ago");
  });
  it("flags errors with the warn tone", () => {
    const b = outboundBadge({
      running: true,
      manifestAgeMs: 0,
      manifestModifiedIso: null,
      lastError: "Permission denied",
    });
    expect(b.label).toBe("Error");
    expect(b.tone).toBe("status-warn");
  });
});

describe("inboxBadge", () => {
  it("shows Loading on null status", () => {
    expect(inboxBadge(null).label).toBe("Loading…");
  });
  it("shows Stopped when runtime is off", () => {
    expect(
      inboxBadge({
        running: false,
        watchPath: null,
        lastArchivedIso: null,
        failedCount: 0,
        lastError: null,
      }).label,
    ).toBe("Stopped");
  });
  it("shows Watching when running, with or without archive history", () => {
    expect(
      inboxBadge({
        running: true,
        watchPath: "C:\\v\\inbox",
        lastArchivedIso: null,
        failedCount: 0,
        lastError: null,
      }).label,
    ).toBe("Watching");
    expect(
      inboxBadge({
        running: true,
        watchPath: "C:\\v\\inbox",
        lastArchivedIso: "2026-05-27T00:00:00Z",
        failedCount: 0,
        lastError: null,
      }).label,
    ).toBe("Watching");
  });
});
