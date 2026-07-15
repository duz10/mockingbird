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
import { migrateUnsplashApiKey } from "./lib/unsplashKeyMigration";

import styles from "./App.module.css";

interface AppProps {
  children: ReactNode;
}

export function App({ children }: AppProps) {
  const setModes = useAppStore((s) => s.setModes);
  const setSettings = useAppStore((s) => s.setSettings);
  const setActiveModeSlug = useAppStore((s) => s.setActiveModeSlug);
  const setKgGraphEnabled = useAppStore((s) => s.setKgGraphEnabled);
  const setIsMac = useAppStore((s) => s.setIsMac);
  const setAppVersion = useAppStore((s) => s.setAppVersion);
  const applyTheme = useAppStore((s) => s.applyTheme);

  // mb-1z0m (Round 4) -- React-mount beacon. Fires the no-state
  // `react_mounted` IPC the FIRST thing after the root commits,
  // BEFORE bootApp's 5-call fan-out. If a future `ipc::react` line
  // appears in `mockingbird.log` we know React rendered; if not,
  // chasing IPC-state bugs is looking under the wrong streetlight.
  // Fire-and-forget; do NOT block boot on the diagnostic.
  useEffect(() => {
    void api.react_mounted().catch(() => {});
  }, []);

  // LR.0.B / mb-hiar (ADR 0055) -- one-shot migration of the
  // Unsplash access key from legacy localStorage into DPAPI. Safe to
  // call on every boot; first-launch-after-update does the work,
  // subsequent boots short-circuit on "no-legacy-value". Fire-and-
  // forget; the migration helper never throws and the worst case
  // (DPAPI write fails) is that the user re-enters their key in
  // Settings. Dispatches PREFS_EVENT internally on success so the
  // background component picks up the migrated value without a
  // refresh.
  useEffect(() => {
    void migrateUnsplashApiKey();
  }, []);

  // macOS-port (v1 honest-surface) — resolve the host OS once so the
  // cross-route "Coming soon" treatment (Activity / Knowledge Graph /
  // Mobile Sync) can gate on it. Fire-and-forget; on any failure
  // `isMac` stays `null`, which every consumer treats as "not Mac"
  // (i.e. the full Windows-parity surface), so a failed probe can
  // never hide a Windows feature.
  useEffect(() => {
    void api
      .host_os()
      .then((os) => setIsMac(os === "macos"))
      .catch(() => setIsMac(false));
  }, [setIsMac]);

  // Boot — hydrate cross-route state from IPC. Each setter fires
  // independently in `bootApp`: a single rejected IPC does NOT
  // suppress the others. This is the `mb-v7pd` smoke fix for Bug 1
  // (sidebar mode label absent on launch) + Bug 2 (sidebar KG nav
  // item absent on launch); see `lib/bootApp.ts` for the full
  // pattern-fix rationale.
  useEffect(() => {
    void bootApp(
      api,
      {
        setModes,
        setSettings,
        setActiveModeSlug,
        setKgGraphEnabled,
        setAppVersion,
        applyTheme,
      },
      {
        // mb-1z0m (Round 3) -- mirror every boot-IPC outcome to the
        // Rust log via `report_ipc_status`. Fire-and-forget; if the
        // mirror itself rejects we deliberately swallow it (failing
        // the diagnostic is strictly less bad than crashing boot).
        reportStatus: (label, ok, reason) => {
          void api.report_ipc_status(label, ok, reason).catch(() => {});
        },
      },
    );
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
