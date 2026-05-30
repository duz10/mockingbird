// KG Phase 1C Wave 1C.1 visual sweep (bead `mb-ucmx`, ADR 0051).
//
// Shape mirrors `ui/tests/smoke.spec.ts` — runs against the Vite
// preview server (no Tauri shell). The UI's `lib/tauri.ts` fixture
// layer serves `kg_settings_get_all` as `{ kgGraphEnabled: false }`
// and `kg_settings_set` as a silent no-op, which is exactly what we
// need to exercise the toggle + post-enable notice render contract
// end-to-end without a Rust process.
//
// Single judge: all 6 assertions below must pass.
//
//  1. /#/settings loads; the "Knowledge Graph" tab button is present.
//  2. Clicking the tab renders the KG panel with explainer + an OFF
//     toggle (fixture default `kgGraphEnabled: false`).
//  3. The `role="status"` notice block is NOT in the DOM while OFF.
//  4. Clicking the toggle flips it ON; the notice appears with the
//     empirical ~1 min indexing copy (kg.settings.notice.body).
//  5. Clicking again flips OFF; the notice goes away.
//  6. No console errors are logged during any interaction.

import { expect, test, type ConsoleMessage } from "@playwright/test";

test.describe("KG Phase 1C.1 — Settings KG tab (mb-ucmx)", () => {
  test("renders, toggles, and conditionally reveals the indexing notice", async ({
    page,
  }) => {
    const consoleErrors: string[] = [];
    page.on("console", (msg: ConsoleMessage) => {
      if (msg.type() === "error") {
        consoleErrors.push(`[console.error] ${msg.text()}`);
      }
    });
    page.on("pageerror", (err) => {
      consoleErrors.push(`[pageerror] ${err.message}`);
    });

    // ── Assertion 1: Settings page + KG tab present ──────────────
    await page.goto("/#/settings");

    const tabs = page.getByRole("navigation", { name: /settings sections/i });
    await expect(tabs).toBeVisible();

    const kgTab = tabs.getByRole("button", { name: /knowledge graph/i });
    await expect(kgTab).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-settings-1-tab-visible.png",
    });

    // ── Assertion 2: click tab → panel + OFF-by-default ──────────
    await kgTab.click();

    // The toggle's accessible name comes from `aria-label` on the
    // <input> per SettingsKgTab.tsx; matches the i18n key
    // `kg.settings.enabled.label` = "Enable Knowledge Graph indexing".
    const toggle = page.getByRole("checkbox", {
      name: /enable knowledge graph indexing/i,
    });
    await expect(toggle).toBeVisible();
    await expect(toggle).not.toBeChecked();
    await page.screenshot({
      path: "test-results/kg-settings-2-panel-off.png",
    });

    // ── Assertion 3: notice hidden while OFF ─────────────────────
    // Scoped lookup by aria-label so the assertion survives the
    // SettingsKgFailedFilings sibling `role="status"` that mounts
    // when the toggle is ON (Wave 1C.5 a11y polish, `mb-f4gn`).
    const notice = page.getByRole("status", {
      name: /indexing in progress/i,
    });
    await expect(notice).toHaveCount(0);

    // ── Assertion 4: toggle ON → notice appears ──────────────────
    await toggle.check();
    await expect(toggle).toBeChecked();
    await expect(notice).toBeVisible();
    // Sanity-check the body copy actually came from the empirically-
    // informed i18n string, not a fallback.
    await expect(notice).toContainText(/about a minute/i);
    await page.screenshot({
      path: "test-results/kg-settings-3-toggle-on.png",
    });

    // ── Assertion 5: toggle OFF → notice disappears ──────────────
    await toggle.uncheck();
    await expect(toggle).not.toBeChecked();
    await expect(notice).toHaveCount(0);
    await page.screenshot({
      path: "test-results/kg-settings-4-toggle-off.png",
    });

    // ── Assertion 6: no console errors across the flow ───────────
    expect(
      consoleErrors,
      `Unexpected console errors during KG settings flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
