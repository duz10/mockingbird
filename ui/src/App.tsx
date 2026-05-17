// Top-level app shell: sidebar + main content area.
//
// Uses CSS grid for layout (sidebar 220px + flexible content). The
// sidebar is sticky on scroll because the content area scrolls
// independently per page.

import { useEffect, type ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { Sidebar } from "./components/Sidebar";
import { api } from "./lib/tauri";
import { useAppStore } from "./lib/store";

import styles from "./App.module.css";

interface AppProps {
  children: ReactNode;
}

export function App({ children }: AppProps) {
  const setModes = useAppStore((s) => s.setModes);
  const setSettings = useAppStore((s) => s.setSettings);
  const applyTheme = useAppStore((s) => s.applyTheme);

  // Boot — load modes + settings once and apply theme. Pure idempotent
  // effect; no cleanup needed.
  useEffect(() => {
    void (async () => {
      try {
        const [modes, settings] = await Promise.all([
          api.list_modes(),
          api.get_settings(),
        ]);
        setModes(modes);
        setSettings(settings);
        applyTheme(settings.theme);
      } catch (err) {
        // Stay alive — the page will surface its own error state.
        // eslint-disable-next-line no-console
        console.warn("App boot fetch failed", err);
      }
    })();
  }, [setModes, setSettings, applyTheme]);

  return (
    <div className={styles.shell}>
      <Sidebar />
      <main className={styles.content} role="main">
        {children}
      </main>
    </div>
  );
}

// Re-exported for tests + a couple of pages that link back to the home tab.
export { NavLink };
