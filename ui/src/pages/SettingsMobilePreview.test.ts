// Unit tests for the pure helpers behind the Mobile Sync (preview)
// settings section. ADR 0046 Iter 2 / mb-vg3p (partial).
//
// Same convention as MeetingRecordBar.test.ts: no @testing-library/
// react, no JSX mounting -- we extract the decision logic into pure
// functions and pin each branch directly. The component-level
// wiring (toggle persists, browse picker opens, etc.) will be
// covered by Playwright in Iter 4 alongside the full Mobile Sync
// tab.

import { describe, expect, it } from "vitest";

import {
  deriveStatus,
  formatExportToast,
  statusLabel,
} from "./SettingsMobilePreview";

describe("deriveStatus", () => {
  it("returns loading when no snapshot has arrived yet", () => {
    expect(deriveStatus(null)).toEqual({ kind: "loading" });
  });

  it("returns disabled when the toggle is off", () => {
    expect(
      deriveStatus({
        mobileSyncEnabled: false,
        vaultPath: "C:\\anywhere",
        vaultSyncRecordTypes: "both",
        vaultRetentionDays: 30,
      }),
    ).toEqual({ kind: "disabled" });
  });

  it("returns no-path when enabled but path is null", () => {
    expect(
      deriveStatus({
        mobileSyncEnabled: true,
        vaultPath: null,
        vaultSyncRecordTypes: "both",
        vaultRetentionDays: 30,
      }),
    ).toEqual({ kind: "no-path" });
  });

  it("returns no-path when enabled but path is whitespace-only", () => {
    // A user can type spaces into the input + tab away. We treat
    // that as 'not set' rather than 'invalid' -- the input clearly
    // wasn't filled in deliberately.
    expect(
      deriveStatus({
        mobileSyncEnabled: true,
        vaultPath: "   ",
        vaultSyncRecordTypes: "both",
        vaultRetentionDays: 30,
      }),
    ).toEqual({ kind: "no-path" });
  });

  it("returns ready when enabled with a non-empty path", () => {
    // We intentionally don't stat the path from the renderer (see
    // the component's inline comment). 'Ready' is optimistic;
    // Export-now surfaces real errors.
    expect(
      deriveStatus({
        mobileSyncEnabled: true,
        vaultPath: "C:\\Users\\you\\mockingbird-vault",
        vaultSyncRecordTypes: "both",
        vaultRetentionDays: 30,
      }),
    ).toEqual({ kind: "ready" });
  });
});

describe("statusLabel", () => {
  it("formats each kind to a user-visible string", () => {
    expect(statusLabel({ kind: "loading" })).toBe("Loading...");
    expect(statusLabel({ kind: "disabled" })).toBe("Disabled");
    expect(statusLabel({ kind: "no-path" })).toBe("Vault path not set");
    expect(
      statusLabel({ kind: "invalid", reason: "Permission denied" }),
    ).toBe("Vault path invalid: Permission denied");
    expect(statusLabel({ kind: "ready" })).toBe("Ready");
  });
});

describe("formatExportToast", () => {
  it("renders the disabled-sync toast when skipped is true", () => {
    expect(
      formatExportToast({ total: 0, changes: 0, archived: 0, skipped: true }),
    ).toBe("Mobile sync is disabled. Flip the toggle first.");
  });

  it("renders the up-to-date toast when changes + archived are both 0", () => {
    expect(
      formatExportToast({
        total: 17,
        changes: 0,
        archived: 0,
        skipped: false,
      }),
    ).toBe("Vault up to date (17 records).");
  });

  it("singularises 'change' when exactly 1 record was written", () => {
    expect(
      formatExportToast({ total: 4, changes: 1, archived: 0, skipped: false }),
    ).toBe("Exported 1 change to vault.");
  });

  it("pluralises and includes archived count when both are non-zero", () => {
    expect(
      formatExportToast({ total: 7, changes: 3, archived: 2, skipped: false }),
    ).toBe("Exported 3 changes (+2 archived) to vault.");
  });

  it("omits archived-count suffix when archived is 0 but changes is non-zero", () => {
    expect(
      formatExportToast({ total: 5, changes: 5, archived: 0, skipped: false }),
    ).toBe("Exported 5 changes to vault.");
  });
});
