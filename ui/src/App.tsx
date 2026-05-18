// Top-level app shell: sidebar + main content area.
//
// Uses CSS grid for layout (sidebar 220px + flexible content). The
// sidebar is sticky on scroll because the content area scrolls
// independently per page.

import { useEffect, type ReactNode } from "react";
import { NavLink } from "react-router-dom";

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
  const applyTheme = useAppStore((s) => s.applyTheme);

  // Boot — load modes, settings, and active-mode selection in
  // parallel, then apply theme. The active-mode fetch is what feeds
  // the Sidebar indicator on first paint (without it, the indicator
  // wouldn't appear until the user visited the Modes page).
  useEffect(() => {
    void (async () => {
      try {
        const [modes, settings, activeMode] = await Promise.all([
          api.list_modes(),
          api.get_settings(),
          api.get_active_mode(),
        ]);
        setModes(modes);
        setSettings(settings);
        setActiveModeSlug(activeMode.slug);
        applyTheme(settings.theme);
      } catch (err) {
        // Stay alive — the page will surface its own error state.
        // eslint-disable-next-line no-console
        console.warn("App boot fetch failed", err);
      }
    })();
  }, [setModes, setSettings, setActiveModeSlug, applyTheme]);

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
    </>
  );
}

// Re-exported for tests + a couple of pages that link back to the home tab.
export { NavLink };
