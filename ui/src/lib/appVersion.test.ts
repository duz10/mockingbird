// Tests for the `fetchAppVersion` helper that feeds Sidebar's
// footer label (mb-v7pd / v0.2.0-beta.1 smoke fix Bug 3).
//
// Tauri's `getVersion()` is not invocable from jsdom (no
// `__TAURI_INTERNALS__`), so the in-suite contract is:
//   * Outside Tauri, return the stable fallback `"dev"`.
//   * Honour `window.__MOCKINGBIRD_FIXTURES__.app_version` so
//     Playwright specs (and these tests) can pin a specific
//     string.
// The Tauri-shell path is exercised by the host smoke matrix
// documented in the brief's acceptance gates.

import { afterEach, describe, expect, it } from "vitest";

import { fetchAppVersion } from "./appVersion";

afterEach(() => {
  if (typeof window !== "undefined") {
    window.__MOCKINGBIRD_FIXTURES__ = undefined;
  }
});

describe("fetchAppVersion (jsdom / non-Tauri context)", () => {
  it("returns the stable 'dev' fallback when no fixture is set", async () => {
    const v = await fetchAppVersion();
    expect(v).toBe("dev");
  });

  it("honours window.__MOCKINGBIRD_FIXTURES__.app_version overrides", async () => {
    window.__MOCKINGBIRD_FIXTURES__ = { app_version: "0.2.0-beta.1" };
    const v = await fetchAppVersion();
    expect(v).toBe("0.2.0-beta.1");
  });

  it("honours arbitrary fixture strings (no hardcoded allowlist)", async () => {
    window.__MOCKINGBIRD_FIXTURES__ = { app_version: "9.9.9-rc.2" };
    const v = await fetchAppVersion();
    expect(v).toBe("9.9.9-rc.2");
  });

  it("falls through to 'dev' when the fixture map omits app_version", async () => {
    window.__MOCKINGBIRD_FIXTURES__ = { something_else: "ignored" };
    const v = await fetchAppVersion();
    expect(v).toBe("dev");
  });
});
