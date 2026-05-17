// App-wide state — kept deliberately small. Most state lives in
// per-page React state; this store is for things that need to cross
// route boundaries (the selected session, the loaded mode list, the
// current theme).

import { create } from "zustand";
import type { ModeRow, SessionDetail, SettingsSnapshot, ThemeChoice } from "./types";

interface AppState {
  // Cached lookups — populated on first access by useMockingbird().
  modes: ModeRow[];
  settings: SettingsSnapshot | null;
  // History view's selected session detail (cleared on route change away).
  selectedSession: SessionDetail | null;

  setModes: (modes: ModeRow[]) => void;
  setSettings: (settings: SettingsSnapshot | null) => void;
  setSelectedSession: (s: SessionDetail | null) => void;
  applyTheme: (theme: ThemeChoice) => void;
}

export const useAppStore = create<AppState>((set) => ({
  modes: [],
  settings: null,
  selectedSession: null,
  setModes: (modes) => set({ modes }),
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
}));
