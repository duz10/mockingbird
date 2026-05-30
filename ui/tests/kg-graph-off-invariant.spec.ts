// KG Phase 1C Wave 1C.5 -- graph-off-UI invariant judge, **tightened
// in Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5 / J3)**.
//
// Bead: `mb-f4gn`. Charter: ADR 0051 §"Invariants" -> J1
// (`kg-graph-off-ui-untouched`). Counterpart to the Rust-side
// `src-tauri/src/kg/graph_off_invariant.rs` probe (ADR 0050 §D8
// gate 2): the Rust probe proves the dictation-tail hook never
// writes to any `kg_*` table when off; this spec proves the UI side
// (a) never INVOKES any `kg_*` IPC when off, AND (b) the Dictations
// page fires ZERO `kg_*` IPCs **regardless of toggle state** now
// that Wave 1D.4 has fully relocated the KG retrieval surface to
// the KG dashboard.
//
// Why this matters: the entire Phase 1C/1D KG UI surface (filter
// chips, per-row chip strip, concept modal, flagged-band retry) is
// gated on `kgGraphEnabled === true` AND on the user being on the
// KG dashboard route. If either gate is wrong, the off-by-default
// privacy contract from ADR 0049 silently breaks. This spec is the
// mechanical regression gate.
//
// Walks (extended in 1D.4):
//   1. Settings -> KG tab (toggle visible + OFF). Spy sees only
//      kg_settings_get_all (one boot fetch from App.tsx + one tab
//      mount). The "Filing status" panel was relocated to the KG
//      dashboard in Wave 1D.4, so the Settings tab is now the
//      toggle only.
//   2. Dictations page (no KG surface, no IPC fires). Spy unchanged.
//   3. Click a session row to open the detail pane. Spy unchanged
//      (concept-modal triggers no longer exist on Dictations).
//   3b. Sidebar does NOT show the Knowledge Graph nav entry while OFF.
//   3c. Direct-URL /knowledge-graph renders the disabled-state
//       EmptyState; NO `kg_dashboard_snapshot` fires.
//   4. Positive control: flip toggle ON, then navigate to
//      /knowledge-graph. `kg_dashboard_snapshot` fires (proving
//      the spy can see kg_* calls when one is supposed to fire).
//   4b. **Tightened in 1D.4 / J3:** with toggle ON, navigate to
//       Dictations page. Capture the spy's `kg_*` set before and
//       after the nav + a row click + an interaction. The diff
//       must be EMPTY -- the Dictations page is now fully KG-free
//       regardless of toggle state.
//   5. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

// The single `kg_*` command authorized to fire while the toggle is
// OFF: the read-only settings snapshot that the App boot fetch +
// the Settings tab both need to know whether to hide / show the
// KG nav item and toggle state. Anything else in the spy's
// recorded set is a graph-off-invariant breach.
const OFF_MODE_ALLOWLIST = new Set(["kg_settings_get_all"]);

// Wave 1D.2 (`mb-j00j`) -- the KG dashboard IPC. Must NOT appear in
// the spy while the toggle is off AND we're on /knowledge-graph;
// MUST appear after the toggle flips ON and we visit
// /knowledge-graph as the positive control for the route.
const DASHBOARD_IPC = "kg_dashboard_snapshot";

test.describe("KG graph-off-UI invariant -- 1D.4 tightened (mb-f4gn / mb-6hm2)", () => {
  test("no kg_* IPC fires when off; Dictations page is KG-free even when on", async ({
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

    // Install the spy + an aggregator BEFORE the first navigation.
    // `addInitScript` runs in every new document on this page (so a
    // SPA route change doesn't lose the spy). The aggregator lives
    // on `window.__KG_IPC_CALLS__` for read-back via `evaluate`.
    await page.addInitScript(() => {
      const calls: string[] = [];
      // SAFETY: `__KG_IPC_SPY__` + `__KG_IPC_CALLS__` are test-only
      // window globals declared in `ui/src/lib/tauri.ts`.
      (window as unknown as { __KG_IPC_CALLS__: string[] }).__KG_IPC_CALLS__ =
        calls;
      (
        window as unknown as { __KG_IPC_SPY__: (cmd: string) => void }
      ).__KG_IPC_SPY__ = (cmd: string) => {
        calls.push(cmd);
      };
    });

    const readCalls = async (): Promise<string[]> =>
      page.evaluate(
        () =>
          (window as unknown as { __KG_IPC_CALLS__: string[] }).__KG_IPC_CALLS__
            .slice(),
      );

    const recordedKgCalls = async (): Promise<Set<string>> => {
      const all = await readCalls();
      return new Set(all.filter((c) => c.startsWith("kg_")));
    };

    // ── Walk 1: Settings -> KG tab, toggle OFF ───────────────────
    await page.goto("/#/settings");

    const kgTab = page
      .getByRole("navigation", { name: /settings sections/i })
      .getByRole("button", { name: /knowledge graph/i });
    await expect(kgTab).toBeVisible();
    await kgTab.click();

    const toggle = page.getByRole("checkbox", {
      name: /enable knowledge graph indexing/i,
    });
    await expect(toggle).toBeVisible();
    await expect(toggle).not.toBeChecked();

    // Wave 1D.4 subtraction: the "Filing status" panel relocated to
    // the KG dashboard's Flagged band. Settings -> KG should NOT
    // surface it regardless of toggle state.
    await expect(
      page.getByRole("heading", { name: /filing status/i }),
    ).toHaveCount(0);

    const afterWalk1 = await recordedKgCalls();
    expect(
      [...afterWalk1].sort(),
      `Walk 1 kg_* allowlist breach. Recorded: ${[...afterWalk1].join(", ")}`,
    ).toEqual([...OFF_MODE_ALLOWLIST].sort());
    await page.screenshot({
      path: "test-results/kg-graph-off-1-settings.png",
    });

    // ── Walk 2: Dictations page, no KG surface visible ──────────
    await page.goto("/#/dictations");

    // The filter bar's entity combobox is the canonical
    // KG-on-Dictations tell from Phase 1C; it must be gone now
    // (Wave 1D.4 removed it entirely).
    await expect(
      page.getByRole("combobox", { name: /filter dictations by entity/i }),
    ).toHaveCount(0);

    // No per-row chip strip either.
    await expect(
      page.getByRole("button", {
        name: /open concept page for (entity|tag)/i,
      }),
    ).toHaveCount(0);

    const afterWalk2 = await recordedKgCalls();
    expect(
      [...afterWalk2].sort(),
      `Walk 2 kg_* allowlist breach. Recorded: ${[...afterWalk2].join(", ")}`,
    ).toEqual([...OFF_MODE_ALLOWLIST].sort());
    await page.screenshot({
      path: "test-results/kg-graph-off-2-dictations.png",
    });

    // ── Walk 3: open a dictation row if any exist ───────────────
    const sessionsList = page.getByRole("listbox", { name: /^sessions$/i });
    if ((await sessionsList.count()) > 0) {
      const firstRow = sessionsList.getByRole("option").first();
      if ((await firstRow.count()) > 0) {
        await firstRow.click();
      }
    }
    await expect(page.getByRole("dialog")).toHaveCount(0);

    const afterWalk3 = await recordedKgCalls();
    expect(
      [...afterWalk3].sort(),
      `Walk 3 kg_* allowlist breach. Recorded: ${[...afterWalk3].join(", ")}`,
    ).toEqual([...OFF_MODE_ALLOWLIST].sort());

    // ── Walk 3b: sidebar must NOT show KG nav item ──────────────
    await expect(
      page
        .getByRole("complementary", { name: /primary navigation/i })
        .getByRole("link", { name: /knowledge graph/i }),
    ).toHaveCount(0);

    // ── Walk 3c: direct-URL guard on /knowledge-graph ───────────
    await page.goto("/#/knowledge-graph");
    await expect(
      page.getByRole("heading", { name: /knowledge graph is off/i }),
    ).toBeVisible();

    const afterWalk3c = await recordedKgCalls();
    expect(
      [...afterWalk3c].sort(),
      `Walk 3c kg_* allowlist breach (direct-URL /knowledge-graph). Recorded: ${[...afterWalk3c].join(", ")}`,
    ).toEqual([...OFF_MODE_ALLOWLIST].sort());
    expect(
      afterWalk3c.has(DASHBOARD_IPC),
      `kg_dashboard_snapshot fired with toggle OFF -- the route guard is broken.`,
    ).toBe(false);
    await page.screenshot({
      path: "test-results/kg-graph-off-3c-direct-url.png",
    });

    // ── Walk 4: positive control -- flip toggle ON ──────────────
    await page.goto("/#/settings");
    const kgTab2 = page
      .getByRole("navigation", { name: /settings sections/i })
      .getByRole("button", { name: /knowledge graph/i });
    await kgTab2.click();

    const toggle2 = page.getByRole("checkbox", {
      name: /enable knowledge graph indexing/i,
    });
    await toggle2.check();
    await expect(toggle2).toBeChecked();

    // The sidebar's KG nav item should appear reactively (the
    // store update fans out to the Sidebar memo). Wait on it
    // before navigating to /knowledge-graph.
    await expect(
      page
        .getByRole("complementary", { name: /primary navigation/i })
        .getByRole("link", { name: /knowledge graph/i }),
    ).toBeVisible();

    // Visit the KG screen -- this is the new positive control.
    // The dashboard's mount-effect fires kg_dashboard_snapshot.
    await page.goto("/#/knowledge-graph");
    await expect(
      page.getByRole("heading", { name: /^knowledge graph$/i, level: 1 }),
    ).toBeVisible();
    await expect
      .poll(async () => (await recordedKgCalls()).has(DASHBOARD_IPC))
      .toBe(true);
    await page.screenshot({
      path: "test-results/kg-graph-off-4-dashboard-on.png",
    });

    // ── Walk 4b (1D.4 J3 TIGHTENING): Dictations page stays
    //                                  KG-free even with toggle ON.
    // Snapshot the spy set, navigate to Dictations, click around,
    // snapshot again. The set difference must be empty -- no NEW
    // kg_* IPC may fire as a result of visiting Dictations,
    // regardless of toggle state. This is the J3 invariant from
    // ADR 0052 §"Acceptance gates".
    const beforeDictNav = await recordedKgCalls();
    await page.goto("/#/dictations");
    // Interact a bit so we're not just measuring mount cost.
    const dictListbox = page.getByRole("listbox", { name: /^sessions$/i });
    if ((await dictListbox.count()) > 0) {
      const firstRow = dictListbox.getByRole("option").first();
      if ((await firstRow.count()) > 0) {
        await firstRow.click();
      }
    }
    // Type into the search box -- the FTS5 path goes through the
    // non-kg `search_transcripts` IPC; should NOT trip the spy.
    const searchInput = page.getByRole("searchbox", {
      name: /search transcripts/i,
    });
    if ((await searchInput.count()) > 0) {
      await searchInput.fill("hello");
    }
    const afterDictNav = await recordedKgCalls();
    const dictDiff = [...afterDictNav].filter((c) => !beforeDictNav.has(c));
    expect(
      dictDiff,
      `Walk 4b: Dictations page fired new kg_* IPCs with toggle ON. Diff: ${dictDiff.join(", ")}. The J3 invariant requires the Dictations page to be KG-free regardless of toggle state (ADR 0052 D5).`,
    ).toEqual([]);
    await page.screenshot({
      path: "test-results/kg-graph-off-4b-dictations-on.png",
    });

    // ── Assertion 5: no console errors across all walks ────────
    expect(
      consoleErrors,
      `Unexpected console errors during graph-off-UI flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
