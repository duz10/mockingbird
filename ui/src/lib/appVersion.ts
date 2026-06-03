// App version fetcher.
//
// `mb-v7pd` (v0.2.0-beta.1 smoke fix Bug 3) — the sidebar previously
// displayed a hardcoded `v0.1.0` string that drifted away from the
// real shipped version. The lesson: ANY hardcoded version literal in
// the UI source is a stale-display incident waiting to happen, because
// release waves bump manifests but rarely audit JSX literals.
//
// Source-of-truth: Tauri's runtime `getVersion()` from
// `@tauri-apps/api/app`, which returns the version baked into
// `src-tauri/tauri.conf.json` at build time. The advantage over a
// Vite-import from `package.json` is that it pulls from the SAME
// manifest the OS uses to stamp ProductVersion / FileVersion on the
// `.exe`, so the displayed string can't drift away from the binary
// metadata.
//
// Outside Tauri (vite preview, vitest jsdom, Playwright preview
// runs) we return a stable fallback `"dev"` so the surrounding UI
// doesn't render an awkward `vundefined`. Tests can override via
// `window.__MOCKINGBIRD_FIXTURES__.app_version` to assert specific
// strings.

import { isTauri } from "./tauri";

const FIXTURE_KEY = "app_version";
const FALLBACK_VERSION = "dev";

/** Fetch the app version. See module doc for context. */
export async function fetchAppVersion(): Promise<string> {
  const override =
    typeof window !== "undefined"
      ? (window.__MOCKINGBIRD_FIXTURES__?.[FIXTURE_KEY] as string | undefined)
      : undefined;
  if (typeof override === "string") return override;

  if (!isTauri()) return FALLBACK_VERSION;

  // Dynamic import keeps `@tauri-apps/api/app` out of the
  // non-Tauri bundle path (same pattern as `lib/tauri.ts`).
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}
