// App-wide state — kept deliberately small. Most state lives in
// per-page React state; this store is for things that need to cross
// route boundaries (the selected session, the loaded mode list, the
// current theme, the design-system version flag).

import { create } from "zustand";
import type { ModeRow, SessionDetail, SettingsSnapshot, ThemeChoice } from "./types";

/**
 * Which iteration of the visual design system the app should render.
 *
 *   - "v1": the legacy cool blue-grey + Inter look (everything pre-ADR-0023).
 *   - "v2": the warm-earth + Liquid Glass + Fraunces look (ADR 0023).
 *
 * During the wave-by-wave migration (W1..W6) we keep both alive side-
 * by-side; flipping this value sets `<html data-design="v1|v2">` which
 * is the *only* switch any v2 CSS reads. W6's cutover hard-codes the
 * default to "v2" and deletes the v1 tokens in the same commit.
 */
export type DesignVersion = "v1" | "v2";

const DESIGN_VERSION_LS_KEY = "mb.designVersion";

function readInitialDesignVersion(): DesignVersion {
  if (typeof window === "undefined") return "v1";
  try {
    const stored = window.localStorage.getItem(DESIGN_VERSION_LS_KEY);
    if (stored === "v1" || stored === "v2") return stored;
  } catch {
    // localStorage can throw in sandboxed contexts — fall through to default.
  }
  return "v1";
}

function syncDesignVersionToDom(version: DesignVersion): void {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-design", version);
}

interface AppState {
  // Cached lookups — populated on first access by useMockingbird().
  modes: ModeRow[];
  /**
   * Slug of the currently-active transcription mode. `null` until
   * the first `get_active_mode` IPC call resolves. Used by both the
   * Modes page (to render the selected card) and the Sidebar
   * (active-mode indicator).
   */
  activeModeSlug: string | null;
  settings: SettingsSnapshot | null;
  // History view's selected session detail (cleared on route change away).
  selectedSession: SessionDetail | null;

  /** Current design-system version. See {@link DesignVersion}. */
  designVersion: DesignVersion;

  setModes: (modes: ModeRow[]) => void;
  setActiveModeSlug: (slug: string | null) => void;
  setSettings: (settings: SettingsSnapshot | null) => void;
  setSelectedSession: (s: SessionDetail | null) => void;
  applyTheme: (theme: ThemeChoice) => void;
  setDesignVersion: (version: DesignVersion) => void;
}

export const useAppStore = create<AppState>((set) => ({
  modes: [],
  activeModeSlug: null,
  settings: null,
  selectedSession: null,
  designVersion: readInitialDesignVersion(),
  setModes: (modes) => set({ modes }),
  setActiveModeSlug: (slug) => set({ activeModeSlug: slug }),
  setSettings: (settings) => set({ settings }),
  setSelectedSession: (s) => set({ selectedSession: s }),
  applyTheme: (theme) => {
    if (typeof document === "undefined") return;
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else {
      document.documentElement.setAttribute("data-theme", theme);
    }
  },
  setDesignVersion: (version) => {
    if (typeof window !== "undefined") {
      try {
        window.localStorage.setItem(DESIGN_VERSION_LS_KEY, version);
      } catch {
        // Ignore — same fallback policy as the read path.
      }
    }
    syncDesignVersionToDom(version);
    set({ designVersion: version });
  },
}));

// Apply the initial value to the DOM at module-evaluation time so the
// first paint reflects the persisted choice — avoids a v1→v2 flash on
// reload when v2 is the user's selection.
syncDesignVersionToDom(useAppStore.getState().designVersion);

/**
 * Developer-only escape hatch: exposes a global toggle on `window` so
 * we can flip the design system from devtools without wiring a Settings
 * UI yet (that lands in Wave 6). Use:
 *
 *     mb.setDesign("v2"); location.reload();
 *
 * Stripped at build time? No — Vite tree-shakes nothing here because
 * the side-effect is intentional. It's tiny and only attached in the
 * browser; the cost is a single property on `window`.
 */
if (typeof window !== "undefined") {
  (window as unknown as { mb?: Record<string, unknown> }).mb = {
    ...((window as unknown as { mb?: Record<string, unknown> }).mb ?? {}),
    setDesign: (version: DesignVersion) =>
      useAppStore.getState().setDesignVersion(version),
    getDesign: () => useAppStore.getState().designVersion,
  };
}
