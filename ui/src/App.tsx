// Top-level app shell: sidebar + main content area.
//
// Uses CSS grid for layout (sidebar 220px + flexible content). The
// sidebar is sticky on scroll because the content area scrolls
// independently per page.

import { useEffect, type ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { ImportProgressOverlay } from "./components/ImportProgressOverlay";
import { Sidebar } from "./components/Sidebar";
import { UnsplashBackground } from "./components/UnsplashBackground";
import { api } from "./lib/tauri";
import { useAppStore } from "./lib/store";

import styles from "./App.module.css";

interface AppProps {
  children: ReactNode;
}

export function App({ children }: AppProps) {
  const setModes = useAppStore((s) => s.setModes);
  const setSettings = useAppStore((s) => s.setSettings);
  const setActiveModeSlug = useAppStore((s) => s.setActiveModeSlug);
  const setKgGraphEnabled = useAppStore((s) => s.setKgGraphEnabled);
  const applyTheme = useAppStore((s) => s.applyTheme);

  // Boot — load modes, settings, active-mode selection, and the
  // KG-graph toggle in parallel, then apply theme. The active-mode
  // fetch feeds the Sidebar indicator on first paint. The KG-toggle
  // fetch feeds the Sidebar's conditional KG nav item (Phase 1D
  // Wave 1D.2 / ADR 0052) so the item appears/disappears reactively
  // when SettingsKgTab flips the toggle, without an app restart.
  useEffect(() => {
    void (async () => {
      try {
        const [modes, settings, activeMode, kgSettings] = await Promise.all([
          api.list_modes(),
          api.get_settings(),
          api.get_active_mode(),
          api.kg_settings_get_all(),
        ]);
        setModes(modes);
        setSettings(settings);
        setActiveModeSlug(activeMode.slug);
        setKgGraphEnabled(kgSettings.kgGraphEnabled);
        applyTheme(settings.theme);
      } catch (err) {
        // Stay alive — the page will surface its own error state.
        // eslint-disable-next-line no-console
        console.warn("App boot fetch failed", err);
      }
    })();
  }, [setModes, setSettings, setActiveModeSlug, setKgGraphEnabled, applyTheme]);

  return (
    <>
      {/*
        Photo background — sits behind the entire shell (z-index 0,
        pointer-events: none). Renders nothing when the user has
        the feature disabled or hasn't entered an Unsplash key, so
        there's no cost when off.
      */}
      <UnsplashBackground />
      <div className={styles.shell}>
        <Sidebar />
        <main className={styles.content} role="main">
          {children}
        </main>
      </div>
      {/*
        ADR 0046 Iter 4 / mb-q1xt — surfaces the `+ Audio file`
        IPC + inbox-courier ingest pipeline. Self-mounted in the
        main app shell because the recording overlay's webview is
        owned by the dictation orchestrator and isn't trivially
        repurposable for non-PTT progress.
      */}
      <ImportProgressOverlay />
    </>
  );
}

// Re-exported for tests + a couple of pages that link back to the home tab.
export { NavLink };
