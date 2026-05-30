// App-wide state — kept deliberately small. Most state lives in
// per-page React state; this store is for things that need to cross
// route boundaries (the selected session, the loaded mode list, the
// current theme).
//
// W6 (the design-language cutover) removed the design-version flag
// and the mb.setDesign / mb.getDesign devtools shims. The Design
// Language v1 surface is now the only surface.

import { create } from "zustand";
import type { ModeRow, SessionDetail, SettingsSnapshot, ThemeChoice } from "./types";

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
  /**
   * Phase 1D Wave 1D.2 (ADR 0052) — global KG-graph activation
   * toggle, mirrored from `KgGraphEnabled` in the typed Settings
   * facade. `null` until the first `kg_settings_get_all` IPC call
   * resolves at App boot. Lives in app-store rather than per-page
   * state because the **Sidebar** subscribes to it (conditional KG
   * nav-item render) AND the **SettingsKgTab** writes to it on
   * toggle flip. That cross-page coupling is exactly what the store
   * is for. The graph-off-UI invariant judge depends on this being
   * reactive — flip the toggle off and the nav item must disappear
   * without an app restart.
   */
  kgGraphEnabled: boolean | null;
  // History view's selected session detail (cleared on route change away).
  selectedSession: SessionDetail | null;

  setModes: (modes: ModeRow[]) => void;
  setActiveModeSlug: (slug: string | null) => void;
  setSettings: (settings: SettingsSnapshot | null) => void;
  setKgGraphEnabled: (on: boolean | null) => void;
  setSelectedSession: (s: SessionDetail | null) => void;
  applyTheme: (theme: ThemeChoice) => void;
}

export const useAppStore = create<AppState>((set) => ({
  modes: [],
  activeModeSlug: null,
  settings: null,
  kgGraphEnabled: null,
  selectedSession: null,
  setModes: (modes) => set({ modes }),
  setActiveModeSlug: (slug) => set({ activeModeSlug: slug }),
  setSettings: (settings) => set({ settings }),
  setKgGraphEnabled: (on) => set({ kgGraphEnabled: on }),
  setSelectedSession: (s) => set({ selectedSession: s }),
  applyTheme: (theme) => {
    if (typeof document === "undefined") return;
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else {
      document.documentElement.setAttribute("data-theme", theme);
    }
  },
}));
