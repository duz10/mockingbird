// Background-photo user preferences.
//
// **Storage seam**: today these live in `localStorage` because the
// preview build (`npm run preview`) runs without a Rust backend, and
// we want the full UX testable there before plumbing through Tauri.
// When this graduates to the release build, the API key migrates to
// DPAPI (same as Claude) and the enabled/mode/categories flags
// migrate to the settings table — callers don't need to know, because
// they all go through `getPrefs()` / `setPref()` here.
//
// Keep the surface area tiny on purpose. If you find yourself adding
// a fifth pref, consider whether it really belongs on a global
// background config vs. somewhere mode-specific.
//
// Why localStorage and not sessionStorage / IndexedDB:
// - We want persistence across runs (user shouldn't re-enter their
//   API key).
// - The data is tiny (a key + a handful of booleans + a slug array).
// - localStorage is synchronous which means `getPrefs()` can return
//   a value immediately on first paint, no flash-of-no-background.

const KEY_PREFIX = "mockingbird:unsplash:";
const K_API_KEY = `${KEY_PREFIX}apiKey`;
const K_ENABLED = `${KEY_PREFIX}enabled`;
const K_MODE = `${KEY_PREFIX}mode`;
const K_CATEGORIES = `${KEY_PREFIX}categories`;
const K_OVERLAY = `${KEY_PREFIX}overlay`;

/**
 * Curation mode:
 *   - `random`   → no query filter; Unsplash picks from the entire
 *                  public pool (orientation + safety filters still
 *                  apply at fetch time).
 *   - `curated`  → caller picks one of the user's selected
 *                  categories per rotation, passes it as `query=`.
 */
export type CurationMode = "random" | "curated";

export interface BackgroundPrefs {
  apiKey: string;
  enabled: boolean;
  mode: CurationMode;
  /** Category slugs the user has selected when `mode === 'curated'`. */
  categories: string[];
  /**
   * 0..1 opacity of the dark overlay rendered above the photo.
   * Defaults to 0 (no overlay) — we expose this so users (and us)
   * can dial in legibility once we see what the glass surfaces
   * actually look like on top of arbitrary photos.
   */
  overlay: number;
}

const DEFAULTS: BackgroundPrefs = {
  apiKey: "",
  enabled: false,
  mode: "random",
  categories: [],
  overlay: 0,
};

/** Read all prefs in one pass. Cheap — no JSON parse on the hot path. */
export function getPrefs(): BackgroundPrefs {
  if (typeof window === "undefined") return DEFAULTS;
  try {
    return {
      apiKey: localStorage.getItem(K_API_KEY) ?? DEFAULTS.apiKey,
      enabled: localStorage.getItem(K_ENABLED) === "1",
      mode: (localStorage.getItem(K_MODE) as CurationMode) ?? DEFAULTS.mode,
      categories: parseList(localStorage.getItem(K_CATEGORIES)),
      overlay: parseOverlay(localStorage.getItem(K_OVERLAY)),
    };
  } catch {
    // localStorage can throw in private-browsing edge cases. Fall
    // back to defaults so the app still renders — better a missing
    // background than a white screen.
    return DEFAULTS;
  }
}

/** Update a single pref. Triggers a `storage`-shaped custom event
 *  so the background component can react without prop-drilling. */
export function setPref<K extends keyof BackgroundPrefs>(
  key: K,
  value: BackgroundPrefs[K],
): void {
  if (typeof window === "undefined") return;
  try {
    switch (key) {
      case "apiKey":
        localStorage.setItem(K_API_KEY, String(value));
        break;
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
    // Same swallow as getPrefs — write failures are not worth
    // crashing over; user will just re-enter on next launch.
  }
  window.dispatchEvent(new CustomEvent(PREFS_EVENT));
}

/** Name of the custom event we fire on every write. Components
 *  subscribe to it to live-update without a global store. */
export const PREFS_EVENT = "mockingbird:unsplash-prefs-changed";

function parseList(raw: string | null): string[] {
  if (!raw) return [];
  return raw.split(",").filter(Boolean);
}

function parseOverlay(raw: string | null): number {
  if (!raw) return DEFAULTS.overlay;
  const n = Number(raw);
  if (!Number.isFinite(n)) return DEFAULTS.overlay;
  // Clamp — invalid values shouldn't blow out the rendering.
  return Math.min(1, Math.max(0, n));
}
