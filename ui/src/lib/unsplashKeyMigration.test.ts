// Tests for the LR.0.B Unsplash key migration helper (mb-hiar).
//
// We drive every documented branch of `migrateUnsplashApiKey`:
//
//   1. Shell absent -> skipped, legacy untouched (preview path).
//   2. No legacy value -> no-op.
//   3. Legacy value + no DPAPI value -> migrate + clear.
//   4. Legacy value + DPAPI value already set -> DPAPI wins, legacy
//      cleared, no overwrite (idempotency / conflict resolution).
//   5. DPAPI set throws -> failure surfaced, legacy preserved for
//      retry on next boot.
//   6. Idempotency: a second call after a successful migrate is a
//      no-op (no duplicate writes, no resurrection).

import { describe, expect, it, vi } from "vitest";

import { LEGACY_API_KEY_STORAGE_KEY } from "../components/UnsplashBackground/prefs";
import {
  migrateUnsplashApiKey,
  type MigrationStorage,
  type UnsplashMigrationApi,
} from "./unsplashKeyMigration";

interface IpcMockOpts {
  initialDpapi?: string | null;
  setThrows?: boolean;
}

function makeIpc(opts: IpcMockOpts = {}) {
  let stored: string | null = opts.initialDpapi ?? null;
  const ipc: UnsplashMigrationApi = {
    unsplash_get_api_key: vi.fn(async () => stored),
    unsplash_set_api_key: vi.fn(async (k: string) => {
      if (opts.setThrows) throw new Error("dpapi unavailable");
      stored = k;
    }),
  };
  return {
    ipc,
    get stored() {
      return stored;
    },
  };
}

function makeStorage(initial: Record<string, string> = {}): MigrationStorage & {
  store: Record<string, string>;
} {
  const store: Record<string, string> = { ...initial };
  return {
    store,
    getItem: (k) => (k in store ? store[k]! : null),
    removeItem: (k) => {
      delete store[k];
    },
  };
}

const KEY = LEGACY_API_KEY_STORAGE_KEY;
const SILENT = () => {};
const emitNoop = () => {};

describe("migrateUnsplashApiKey", () => {
  it("skips when no Tauri shell is available (preview path)", async () => {
    const ipcHarness = makeIpc();
    const storage = makeStorage({ [KEY]: "legacy-key" });
    const result = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: false,
      log: SILENT,
      emit: emitNoop,
    });
    expect(result).toBe("skipped-no-shell");
    // Legacy value MUST remain so the next real-shell boot can migrate.
    expect(storage.store[KEY]).toBe("legacy-key");
    expect(ipcHarness.ipc.unsplash_get_api_key).not.toHaveBeenCalled();
    expect(ipcHarness.ipc.unsplash_set_api_key).not.toHaveBeenCalled();
  });

  it("no-ops cleanly when there is no legacy value", async () => {
    const ipcHarness = makeIpc();
    const storage = makeStorage();
    const result = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit: emitNoop,
    });
    expect(result).toBe("no-legacy-value");
    expect(ipcHarness.ipc.unsplash_set_api_key).not.toHaveBeenCalled();
  });

  it("migrates a legacy value into DPAPI and clears localStorage", async () => {
    const ipcHarness = makeIpc({ initialDpapi: null });
    const storage = makeStorage({ [KEY]: "legacy-key-xyz" });
    const emit = vi.fn();
    const result = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit,
    });
    expect(result).toBe("migrated");
    expect(ipcHarness.stored).toBe("legacy-key-xyz");
    expect(storage.store[KEY]).toBeUndefined();
    expect(emit).toHaveBeenCalledTimes(1);
  });

  it("DPAPI wins on conflict; legacy is cleared without overwrite", async () => {
    const ipcHarness = makeIpc({ initialDpapi: "dpapi-wins" });
    const storage = makeStorage({ [KEY]: "legacy-loses" });
    const emit = vi.fn();
    const result = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit,
    });
    expect(result).toBe("already-migrated-cleared-legacy");
    expect(ipcHarness.stored).toBe("dpapi-wins");
    expect(storage.store[KEY]).toBeUndefined();
    expect(ipcHarness.ipc.unsplash_set_api_key).not.toHaveBeenCalled();
    expect(emit).toHaveBeenCalledTimes(1);
  });

  it("surfaces failure without losing the legacy value (retry-safe)", async () => {
    const ipcHarness = makeIpc({ initialDpapi: null, setThrows: true });
    const storage = makeStorage({ [KEY]: "stays-put" });
    const result = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit: emitNoop,
    });
    expect(result).toBe("migration-failed");
    // Legacy value MUST remain so a future boot can retry.
    expect(storage.store[KEY]).toBe("stays-put");
  });

  it("is idempotent: second call after migrate does nothing", async () => {
    const ipcHarness = makeIpc({ initialDpapi: null });
    const storage = makeStorage({ [KEY]: "round-trip-me" });
    const first = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit: emitNoop,
    });
    expect(first).toBe("migrated");
    expect(ipcHarness.stored).toBe("round-trip-me");

    // Second call -- no legacy left, so this is a clean no-op.
    const second = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit: emitNoop,
    });
    expect(second).toBe("no-legacy-value");
    expect(ipcHarness.stored).toBe("round-trip-me");
    expect(ipcHarness.ipc.unsplash_set_api_key).toHaveBeenCalledTimes(1);
  });

  it("treats whitespace-only legacy values as no legacy", async () => {
    const ipcHarness = makeIpc({ initialDpapi: null });
    const storage = makeStorage({ [KEY]: "   \t\n  " });
    const result = await migrateUnsplashApiKey({
      ipc: ipcHarness.ipc,
      storage,
      hasShell: true,
      log: SILENT,
      emit: emitNoop,
    });
    expect(result).toBe("no-legacy-value");
    expect(ipcHarness.ipc.unsplash_set_api_key).not.toHaveBeenCalled();
  });
});
