// KG Phase 1C Wave 1C.5 — graph-off-UI invariant judge.
//
// Bead: `mb-f4gn`. Charter: ADR 0051 §"Invariants" → J1
// (`kg-graph-off-ui-untouched`). Counterpart to the Rust-side
// `src-tauri/src/kg/graph_off_invariant.rs` probe (ADR 0050 §D8
// gate 2): the Rust probe proves the dictation-tail hook never
// writes to any `kg_*` table when off; this spec proves the UI side
// never INVOKES any `kg_*` IPC when off (besides the one read-only
// `kg_settings_get_all` call that exists *because* the user needs a
// way to flip the toggle).
//
// Why this matters: the entire Phase 1C UI surface (filter bar,
// per-row chip strip, concept modal, failed-filings card) is gated
// on `kgGraphEnabled === true`. If any of those gates is wrong, the
// off-by-default privacy contract from ADR 0049 silently breaks.
// This spec is the mechanical regression gate.
//
// Why deterministic and not LLM-graded: AGENTS.md §"Judges — when"
// authorizes a single one-off judge for a narrow invariant. The
// property is binary (set of recorded IPC command names == the
// expected allowlist); a language model adds noise without buying
// anything. Mirrors the Rust-side `kg_graph_off_invariant` probe's
// design decision.
//
// Spy mechanism: `lib/tauri.ts` exposes an opt-in
// `window.__KG_IPC_SPY__` callback that fires once per `invoke()`
// call (before the isTauri / fixture branch — so it captures every
// IPC name regardless of the runtime context). We install it in
// `addInitScript` so the hook is in place before the page's first
// IPC. Production cost: one `if (spy)` check per IPC; zero unless a
// test has opted in.
//
// Walks (per Decision B in the Wave 1C.5 kickoff, EXTENDED in 1D.2
// to cover the new `/knowledge-graph` route + sidebar nav):
//   1. Settings → KG tab (toggle visible + OFF + failing-filings
//      section NOT visible). Spy sees only kg_settings_get_all.
//   2. Dictations page (no filter bar, no per-row chips, no
//      filing-state pills). Spy unchanged.
//   3. Click a session row to open the detail pane (no concept
//      modal triggers reachable). Spy unchanged.
//   3b. (Wave 1D.2) Sidebar does NOT show the Knowledge Graph nav
//      entry while OFF. Spy unchanged.
//   3c. (Wave 1D.2) Direct-URL navigation to `/knowledge-graph`
//      with toggle OFF renders the disabled-state EmptyState; NO
//      `kg_dashboard_snapshot` IPC fires (only the already-allowed
//      `kg_settings_get_all`). Spy unchanged.
//   4. Positive control: flip toggle ON on Settings/KG tab; the
//      mount of SettingsKgFailedFilings fires kg_list_failed_filings
//      + kg_queue_status. Then navigate to /knowledge-graph and
//      verify kg_dashboard_snapshot DOES fire (proving the
//      structural ability to call the dashboard IPC when on).
//   5. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

// The single `kg_*` command authorized to fire while the toggle is
// OFF: the read-only settings snapshot that the Settings tab + the
// Dictations page mount-time effect both need to know whether to
// hide / show the KG UI elements. Anything else in the spy's
// recorded set is a graph-off-invariant breach.
const OFF_MODE_ALLOWLIST = new Set(["kg_settings_get_all"]);

// Commands we expect to see fire once the user flips the toggle ON.
// SettingsKgFailedFilings's mount effect drives both. We assert
// that AT LEAST ONE shows up after the flip — proving the spy +
// the UI are wired to actually emit `kg_*` IPCs when appropriate.
const POSITIVE_CONTROL_CANDIDATES = [
  "kg_list_failed_filings",
  "kg_queue_status",
];

// Wave 1D.2 (`mb-j00j`) -- the KG dashboard IPC. Must NOT appear in
// the spy while the toggle is off AND we're on /knowledge-graph;
// MUST appear after the toggle flips ON and we visit
// /knowledge-graph as the positive control for the new route.
const DASHBOARD_IPC = "kg_dashboard_snapshot";

test.describe("KG Phase 1C.5 — graph-off-UI invariant (mb-f4gn)", () => {
  test("no kg_* IPC fires when KgGraphEnabled is off; positive control proves the spy can see them", async ({
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

    // ── Walk 1: Settings → KG tab, toggle OFF ────────────────────
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

    // The "Filing status" heading is the SettingsKgFailedFilings
    // mount point; per Decision B Walk 1, it must NOT be in the DOM
    // while the toggle is OFF.
    await expect(
      page.getByRole("heading", { name: /filing status/i }),
    ).toHaveCount(0);

    // ── Assertion 1: only kg_settings_get_all has fired ─────────
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

    // The filter bar's entity combobox (aria-label
    // "Filter dictations by entity" per kg.filter.entities.aria)
    // is the canonical OFF-vs-ON tell on the Dictations page. It's
    // the FIRST piece of UI gated on kgGraphEnabled === true.
    await expect(
      page.getByRole("combobox", { name: /filter dictations by entity/i }),
    ).toHaveCount(0);

    // Per-row chip strip is rendered inside DictationsList rows;
    // each chip is a button with the kg.chip.*OpenAria label.
    // Absence (0 count) is the OFF-mode contract.
    await expect(
      page.getByRole("button", {
        name: /open concept page for (entity|tag)/i,
      }),
    ).toHaveCount(0);

    // ── Assertion 2: allowlist still holds after Walk 2 ─────────
    const afterWalk2 = await recordedKgCalls();
    expect(
      [...afterWalk2].sort(),
      `Walk 2 kg_* allowlist breach. Recorded: ${[...afterWalk2].join(", ")}`,
    ).toEqual([...OFF_MODE_ALLOWLIST].sort());
    await page.screenshot({
      path: "test-results/kg-graph-off-2-dictations.png",
    });

    // ── Walk 3: open a dictation row if any exist, no modal path ─
    // Rows are `role="option"` inside `role="listbox"
    // aria-label="Sessions"` (DictationsList.tsx). Fixture
    // list_sessions may be empty; we click if present, otherwise
    // skip and rely on the no-dialog assertion holding vacuously.
    const sessionsList = page.getByRole("listbox", { name: /^sessions$/i });
    if ((await sessionsList.count()) > 0) {
      const firstRow = sessionsList.getByRole("option").first();
      if ((await firstRow.count()) > 0) {
        await firstRow.click();
      }
    }
    // ConceptModal renders inside a `role="dialog"` portal; OFF
    // mode must keep its count at 0 across all row interactions.
    await expect(page.getByRole("dialog")).toHaveCount(0);

    // ── Assertion 3: allowlist STILL holds after Walk 3 ─────────
    const afterWalk3 = await recordedKgCalls();
    expect(
      [...afterWalk3].sort(),
      `Walk 3 kg_* allowlist breach. Recorded: ${[...afterWalk3].join(", ")}`,
    ).toEqual([...OFF_MODE_ALLOWLIST].sort());

    // ── Walk 3b (Wave 1D.2): sidebar must NOT show KG nav item ──
    // The KG nav item only renders when KgGraphEnabled is true.
    // We check by aria-label "Knowledge Graph" inside the
    // primary-navigation aside; toHaveCount(0) is the OFF-mode
    // contract. (We use `getByRole("link", ...)` so the assertion
    // doesn't accidentally match the page-header heading of the
    // KG dashboard page itself if a future change loads it.)
    await expect(
      page
        .getByRole("complementary", { name: /primary navigation/i })
        .getByRole("link", { name: /knowledge graph/i }),
    ).toHaveCount(0);

    // ── Walk 3c (Wave 1D.2): direct-URL guard on /knowledge-graph
    // Bookmarks / pasted URLs / browser-back into the route must
    // render the disabled-state and MUST NOT fire kg_dashboard_snapshot.
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
      `kg_dashboard_snapshot fired with toggle OFF — the route guard is broken.`,
    ).toBe(false);
    await page.screenshot({
      path: "test-results/kg-graph-off-3c-direct-url.png",
    });

    // ── Walk 4: positive control — flip toggle ON ───────────────
    // Re-navigate to Settings (the SPA route change re-runs
    // addInitScript per `addInitScript`'s contract, but the
    // existing aggregator array stays — we want cumulative call
    // tracking across the whole session).
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

    // SettingsKgFailedFilings mounts as soon as the toggle local
    // state flips ON; its useEffect fires kg_list_failed_filings +
    // kg_queue_status. Wait for the "Filing status" heading as the
    // proof-of-mount before reading the spy.
    await expect(
      page.getByRole("heading", { name: /filing status/i }),
    ).toBeVisible();

    // ── Assertion 4: at least one positive-control IPC fired ────
    const afterFlip = await recordedKgCalls();
    const positiveControlSeen = POSITIVE_CONTROL_CANDIDATES.some((cmd) =>
      afterFlip.has(cmd),
    );
    expect(
      positiveControlSeen,
      `Positive control failed: none of ${POSITIVE_CONTROL_CANDIDATES.join(", ")} fired after toggle ON. Recorded: ${[...afterFlip].join(", ")}. This means either the spy is broken (and the OFF-mode assertions are vacuous) or the SettingsKgFailedFilings mount stopped firing kg_*.`,
    ).toBe(true);
    await page.screenshot({
      path: "test-results/kg-graph-off-3-positive-control.png",
    });

    // ── Walk 4b (Wave 1D.2): /knowledge-graph mounts the dashboard
    // and fires kg_dashboard_snapshot now that the toggle is ON.
    // Also re-verifies the sidebar KG nav item is now present
    // (reactive store update from the toggle flip).
    await expect(
      page
        .getByRole("complementary", { name: /primary navigation/i })
        .getByRole("link", { name: /knowledge graph/i }),
    ).toBeVisible();
    await page.goto("/#/knowledge-graph");
    // The dashboard's PageHeader renders the title as an h1.
    await expect(
      page.getByRole("heading", { name: /^knowledge graph$/i, level: 1 }),
    ).toBeVisible();
    // Allow the mount-effect IPC to settle, then assert.
    await expect.poll(async () => (await recordedKgCalls()).has(DASHBOARD_IPC)).toBe(
      true,
    );
    await page.screenshot({
      path: "test-results/kg-graph-off-4b-dashboard-on.png",
    });

    // ── Assertion 5: no console errors across all walks ────────
    expect(
      consoleErrors,
      `Unexpected console errors during graph-off-UI flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
