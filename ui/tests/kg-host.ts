// mb-0cg fallout — force the preview fixture layer to report a Windows
// host so the KG specs exercise the real Knowledge Graph UI instead of
// the macOS "coming soon" path.
//
// Background: the v1 honest-surface change made the KG route (and the
// Settings → KG tab) render a "Coming soon on macOS" placeholder
// whenever `host_os() === "macos"`. The Vite preview fixture in
// `ui/src/lib/tauri.ts` returns `host_os = "macos"` (so the macOS
// permissions tab is exercisable in `npm run preview`). That flipped
// every kg-*.spec.ts onto the coming-soon path and broke them.
//
// The KG subsystem is Windows-only (its filing worker, reverse-watcher
// and inbox courier are all `#[cfg(target_os = "windows")]`), so
// "windows" is the correct platform under test. We override the
// `host_os` command via `window.__MOCKINGBIRD_FIXTURES__` — the same
// per-command override hook the specs already use for kg_* fixtures.
//
// MUST be called BEFORE `page.goto(...)`: `addInitScript` runs in every
// fresh document, and the override has to be in place before the app's
// boot-time `host_os()` probe (App.tsx) resolves `isMac`.
//
// Note: specs that later do a FULL overwrite of
// `window.__MOCKINGBIRD_FIXTURES__ = { ... }` (rather than a spread
// merge) must also include `host_os: "windows"` in that object, since a
// bare overwrite on reload would otherwise drop the override.

import type { Page } from "@playwright/test";

export async function forceWindowsHost(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const w = window as unknown as {
      __MOCKINGBIRD_FIXTURES__?: Record<string, unknown>;
    };
    w.__MOCKINGBIRD_FIXTURES__ = {
      ...(w.__MOCKINGBIRD_FIXTURES__ ?? {}),
      host_os: "windows",
    };
  });
}
