// One-shot migration: pre-LR.0.B users had their Unsplash access
// key sitting in `localStorage["mockingbird:unsplash:apiKey"]`.
// LR.0.B (ADR 0055 charter, bead mb-hiar) moved that into the
// platform secret store (DPAPI on Windows). On first launch after
// the update we lift any legacy value into DPAPI, then wipe the
// localStorage entry so the plaintext copy stops shadowing the
// encrypted one.
//
// Invariants this helper guarantees:
//
//   1. **Idempotent.** Calling twice never corrupts state, never
//      duplicates writes, never resurrects a cleared key.
//   2. **DPAPI wins on conflict.** If a user somehow has BOTH a
//      legacy localStorage value AND a DPAPI value (e.g. they entered
//      a new key in one session, then a stale window holding the old
//      localStorage tries to migrate later), the DPAPI value is
//      authoritative and the legacy entry is just cleared.
//   3. **Defensive cleanup.** If DPAPI already has a value, the
//      legacy entry is removed even when migration is technically a
//      no-op. Leaving the plaintext sitting there would defeat the
//      whole point of the wave.
//   4. **Boot-safe.** Never throws into the caller. Boot failures of
//      the secret store get logged and swallowed; the worst case is
//      the user re-enters their key in Settings.
//
// Implemented as a pure(-ish) function taking its dependencies as
// parameters so the test file can drive every branch without
// reaching for real Tauri.

import { api, isTauri } from "./tauri";
import {
  LEGACY_API_KEY_STORAGE_KEY,
  PREFS_EVENT,
} from "../components/UnsplashBackground/prefs";

/** Subset of the Tauri IPC surface the migration uses. */
export interface UnsplashMigrationApi {
  unsplash_get_api_key: () => Promise<string | null>;
  unsplash_set_api_key: (key: string) => Promise<void>;
}

/**
 * Minimal localStorage seam, mockable in tests. The browser-native
 * `Storage` interface satisfies this shape automatically.
 */
export interface MigrationStorage {
  getItem(key: string): string | null;
  removeItem(key: string): void;
}

/** Outcome of one migration call, returned for tests + logs. */
export type UnsplashMigrationResult =
  | "skipped-no-shell"
  | "skipped-no-storage"
  | "no-legacy-value"
  | "migrated"
  | "already-migrated-cleared-legacy"
  | "migration-failed";

interface MigrationOptions {
  /** Override the IPC surface. Defaults to the real `api`. */
  ipc?: UnsplashMigrationApi;
  /** Override the storage seam. Defaults to `window.localStorage`. */
  storage?: MigrationStorage | null;
  /** Force the shell check (tests). Defaults to `isTauri()`. */
  hasShell?: boolean;
  /** Logger. Defaults to `console.info` / `console.warn`. */
  log?: (level: "info" | "warn", msg: string, extra?: unknown) => void;
  /** Event dispatcher. Defaults to firing PREFS_EVENT on `window`.
   *  Tests pass a spy. */
  emit?: () => void;
}

/**
 * Migrate the Unsplash access key from legacy localStorage into the
 * DPAPI-backed secret store, if applicable. Safe to call on every
 * boot; first call does the work, subsequent calls are cheap no-ops.
 *
 * Returns a status code describing what happened, useful for tests
 * + the boot diagnostic stream. Never throws.
 */
export async function migrateUnsplashApiKey(
  options: MigrationOptions = {},
): Promise<UnsplashMigrationResult> {
  const hasShell = options.hasShell ?? isTauri();
  const storage = resolveStorage(options.storage);
  const ipc = options.ipc ?? api;
  const log =
    options.log ??
    ((level, msg, extra) => {
      // eslint-disable-next-line no-console
      (level === "warn" ? console.warn : console.info)(msg, extra ?? "");
    });
  const emit =
    options.emit ??
    (() => {
      if (typeof window !== "undefined") {
        window.dispatchEvent(new CustomEvent(PREFS_EVENT));
      }
    });

  if (!hasShell) {
    // Preview / jsdom contexts have no DPAPI to migrate into.
    // Deliberately leave the legacy value where it is so that when
    // the user next launches the real shell, migration runs there.
    return "skipped-no-shell";
  }
  if (!storage) {
    return "skipped-no-storage";
  }

  const legacy = readLegacy(storage);
  if (legacy === null) {
    return "no-legacy-value";
  }

  try {
    const current = await ipc.unsplash_get_api_key();
    if (current !== null && current.length > 0) {
      // DPAPI already has a value, the legacy entry is stale.
      // Wipe it but do NOT overwrite DPAPI (Invariant 2).
      storage.removeItem(LEGACY_API_KEY_STORAGE_KEY);
      log(
        "info",
        "[unsplash-migration] DPAPI value already present; cleared legacy localStorage entry",
      );
      emit();
      return "already-migrated-cleared-legacy";
    }

    await ipc.unsplash_set_api_key(legacy);
    storage.removeItem(LEGACY_API_KEY_STORAGE_KEY);
    log("info", "[unsplash-migration] migrated legacy key into DPAPI");
    emit();
    return "migrated";
  } catch (err) {
    // Don't blow up boot. Leave the legacy value in place so a
    // future boot can retry. Surface the error for diagnosis.
    log("warn", "[unsplash-migration] failed", err);
    return "migration-failed";
  }
}

function resolveStorage(
  override: MigrationStorage | null | undefined,
): MigrationStorage | null {
  if (override !== undefined) return override;
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function readLegacy(storage: MigrationStorage): string | null {
  try {
    const v = storage.getItem(LEGACY_API_KEY_STORAGE_KEY);
    if (v === null) return null;
    const trimmed = v.trim();
    return trimmed.length === 0 ? null : trimmed;
  } catch {
    return null;
  }
}
