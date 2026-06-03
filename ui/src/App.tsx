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
import { bootApp } from "./lib/bootApp";
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
  const setAppVersion = useAppStore((s) => s.setAppVersion);
  const applyTheme = useAppStore((s) => s.applyTheme);

  // Boot — hydrate cross-route state from IPC. Each setter fires
  // independently in `bootApp`: a single rejected IPC does NOT
  // suppress the others. This is the `mb-v7pd` smoke fix for Bug 1
  // (sidebar mode label absent on launch) + Bug 2 (sidebar KG nav
  // item absent on launch); see `lib/bootApp.ts` for the full
  // pattern-fix rationale.
  useEffect(() => {
    void bootApp(api, {
      setModes,
      setSettings,
      setActiveModeSlug,
      setKgGraphEnabled,
      setAppVersion,
      applyTheme,
    });
  }, [
    setModes,
    setSettings,
    setActiveModeSlug,
    setKgGraphEnabled,
    setAppVersion,
    applyTheme,
  ]);

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
