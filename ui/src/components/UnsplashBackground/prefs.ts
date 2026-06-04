// Background-photo user preferences.
//
// Two storage seams, deliberately separated:
//
//   1. **Non-secret display prefs** (enabled / mode / categories /
//      overlay) — localStorage. Synchronous reads. Persists across
//      runs. Fits in a few hundred bytes. No reason to involve Rust.
//
//   2. **Unsplash access key** — DPAPI on Windows (real shell) via
//      `unsplash_{get,set,clear}_api_key` IPC. Stub in fixture mode.
//      Async accessors only. LR.0.B (ADR 0055) moved this off
//      localStorage; see `lib/unsplashKeyMigration.ts` for the
//      first-launch-after-update migration path.
//
// Why split the types: the old `BackgroundPrefs.apiKey` was a lie
// once we moved storage. Keeping it on the same record would either
// (a) force every reader to await the prefs (gross — they don't all
// need the key) or (b) leave callers thinking they have the key when
// they actually have an empty string until hydration finishes. The
// split makes the lifecycle visible at the call site.
//
// PREFS_EVENT is fired on writes to EITHER store so subscribers can
// re-read whichever set they care about without coordinating event
// names. Cheap: one custom event, no payload.

import { api, isTauri } from "../../lib/tauri";

const KEY_PREFIX = "mockingbird:unsplash:";
const K_ENABLED = `${KEY_PREFIX}enabled`;
const K_MODE = `${KEY_PREFIX}mode`;
const K_CATEGORIES = `${KEY_PREFIX}categories`;
const K_OVERLAY = `${KEY_PREFIX}overlay`;

/**
 * Legacy localStorage key for the Unsplash access key, used ONLY by
 * the one-shot migration in `lib/unsplashKeyMigration.ts`. New code
 * MUST go through [`getUnsplashApiKey`] / [`setUnsplashApiKey`] /
 * [`clearUnsplashApiKey`] instead.
 */
export const LEGACY_API_KEY_STORAGE_KEY = `${KEY_PREFIX}apiKey`;

/**
 * Curation mode:
 *   - `random`   no query filter; Unsplash picks from the entire
 *                public pool (orientation + safety filters still
 *                apply at fetch time).
 *   - `curated`  caller picks one of the user's selected categories
 *                per rotation, passes it as `query=`.
 */
export type CurationMode = "random" | "curated";

/**
 * Non-secret display prefs persisted to localStorage. The Unsplash
 * access key lives separately, see module docs.
 */
export interface BackgroundPrefs {
  enabled: boolean;
  mode: CurationMode;
  /** Category slugs the user has selected when `mode === 'curated'`. */
  categories: string[];
  /**
   * 0..1 opacity of the dark overlay rendered above the photo.
   * Defaults to 0 (no overlay), exposed so users (and us) can dial
   * in legibility once we see what the glass surfaces actually look
   * like on top of arbitrary photos.
   */
  overlay: number;
}

const DEFAULTS: BackgroundPrefs = {
  enabled: false,
  mode: "random",
  categories: [],
  overlay: 0,
};

/** Read all non-secret prefs in one pass. Cheap, synchronous. */
export function getPrefs(): BackgroundPrefs {
  if (typeof window === "undefined") return DEFAULTS;
  try {
    return {
      enabled: localStorage.getItem(K_ENABLED) === "1",
      mode: (localStorage.getItem(K_MODE) as CurationMode) ?? DEFAULTS.mode,
      categories: parseList(localStorage.getItem(K_CATEGORIES)),
      overlay: parseOverlay(localStorage.getItem(K_OVERLAY)),
    };
  } catch {
    // localStorage can throw in private-browsing edge cases. Fall
    // back to defaults so the app still renders.
    return DEFAULTS;
  }
}

/** Update a single non-secret pref. Triggers PREFS_EVENT so the
 *  background component can react without prop-drilling. */
export function setPref<K extends keyof BackgroundPrefs>(
  key: K,
  value: BackgroundPrefs[K],
): void {
  if (typeof window === "undefined") return;
  try {
    switch (key) {
      case "enabled":
        localStorage.setItem(K_ENABLED, value ? "1" : "0");
        break;
      case "mode":
        localStorage.setItem(K_MODE, String(value));
        break;
      case "categories":
        localStorage.setItem(K_CATEGORIES, (value as string[]).join(","));
        break;
      case "overlay":
        localStorage.setItem(K_OVERLAY, String(value));
        break;
    }
  } catch {
    // Write failures are not worth crashing over; user will just
    // re-enter on next launch.
  }
  emitPrefsChanged();
}

/** Name of the custom event fired on every write to either store.
 *  Subscribers re-read whichever side they care about. */
export const PREFS_EVENT = "mockingbird:unsplash-prefs-changed";

function emitPrefsChanged(): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(PREFS_EVENT));
}

/* ------------------------------------------------------------------ */
/* Unsplash access key, DPAPI-backed (LR.0.B / mb-hiar, ADR 0055).    */
/*                                                                    */
/* The browser/jsdom preview path (no Tauri shell) returns "" from    */
/* the getter and silently no-ops the setter; the fixtures in        */
/* `lib/tauri.ts` already model that. Real shell hits DPAPI.          */
/* ------------------------------------------------------------------ */

/**
 * Read the user's Unsplash access key from the platform secret store.
 * Returns `""` (NOT `null`) when no key is set, matching the legacy
 * synchronous prefs contract so consumers can keep using
 * `apiKey.length > 0` to gate features.
 */
export async function getUnsplashApiKey(): Promise<string> {
  try {
    const raw = await api.unsplash_get_api_key();
    return raw ?? "";
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[unsplash] get_api_key failed", err);
    return "";
  }
}

/**
 * Persist the Unsplash access key via DPAPI. Trims whitespace and
 * rejects empty strings up front to match the Rust-side validation
 * (saves a round trip). Dispatches PREFS_EVENT on success so
 * subscribers re-fetch.
 */
export async function setUnsplashApiKey(key: string): Promise<void> {
  const trimmed = key.trim();
  if (trimmed.length === 0) {
    throw new Error("unsplash api key cannot be empty");
  }
  await api.unsplash_set_api_key(trimmed);
  emitPrefsChanged();
}

/**
 * Remove the stored Unsplash access key. Idempotent. Dispatches
 * PREFS_EVENT so subscribers see the cleared state.
 */
export async function clearUnsplashApiKey(): Promise<void> {
  await api.unsplash_clear_api_key();
  emitPrefsChanged();
}

/** True iff we are running with a real Tauri shell. Re-exported so
 *  the migration module doesn't need to import from two places. */
export function hasTauriShell(): boolean {
  return isTauri();
}

function parseList(raw: string | null): string[] {
  if (!raw) return [];
  return raw.split(",").filter(Boolean);
}

function parseOverlay(raw: string | null): number {
  if (!raw) return DEFAULTS.overlay;
  const n = Number(raw);
  if (!Number.isFinite(n)) return DEFAULTS.overlay;
  // Clamp; invalid values shouldn't blow out the rendering.
  return Math.min(1, Math.max(0, n));
}
