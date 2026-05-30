// KG Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) -- Retrieval band
// on the KG dashboard.
//
// Origin: this spec started life as `kg-dictations-retrieval.spec.ts`
// (Phase 1C Wave 1C.3 / `mb-5ly5`) and tested the filter-chip +
// per-row chip UX that lived on the Dictations page. Wave 1D.4
// relocated the entire retrieval surface to the KG dashboard;
// this spec moves with it. Same fixture rig, new route + new
// assertion targets (Retrieval band instead of the Dictations
// list).
//
// What the Retrieval band looks like
// ----------------------------------
//   Idle state (no chips): an empty-state message telling the user
//   to pick a chip. No row list is rendered (avoids duplicating
//   the recent-activity band's content).
//   Filter-active state: rows that pass `kg_search_entries(filter)`
//   render below the chips with the per-row `EntryChips` strip.
//   Filter-active + empty result: the same "No dictations match
//   these filters" empty-state + Clear-filters CTA we used on
//   Dictations carries over verbatim.
//
//  1. Toggle OFF -> /knowledge-graph renders the disabled-state;
//     no FilterBar mounts.
//  2. Toggle ON + empty filter -> Retrieval band mounts; idle copy
//     visible; NO per-row chips (the band only renders rows when
//     a filter is active).
//  3. Open entity autocomplete -> pick a suggestion -> chip added;
//     `kg_search_entries` fires; the band renders the matching
//     session row with its per-row EntryChips strip.
//  4. Add a tag chip on top -> cross-axis AND narrows further;
//     both remove-chip buttons visible.
//  5. Stage `kg_search_entries` to return [] -> empty-state copy
//     + Clear-filters link; clicking Clear removes the chips and
//     returns the band to the idle copy.
//  6. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

const EMPTY_DASHBOARD = {
  counts: { totalEntities: 0, totalEntries: 0, entitiesByType: [] },
  queueStatus: {
    pending: 0,
    processing: 0,
    failed: 0,
    done: 0,
    lastDoneIso: null as string | null,
  },
  recentActivity: [],
  flaggedForReview: [],
  upcomingDue: [],
};

/** Per-row summary keyed off the first seed session id (100). */
const ROW_100_SUMMARY = {
  entities: [
    { entityId: 1, canonicalName: "Mom", entityType: "person" },
    { entityId: 2, canonicalName: "Dad", entityType: "person" },
  ],
  tags: [{ tagSlug: "family" }],
  filingState: "done",
};

const MOM_SUGGESTION = [
  {
    entityId: 1,
    canonicalName: "Mom",
    entityType: "person",
    mentionCount: 12,
  },
];

const FAMILY_TAG_SUGGESTION = [{ tagSlug: "family", mentionCount: 8 }];

test.describe("KG Phase 1D.4 -- Retrieval band (mb-6hm2)", () => {
  test("toggle-gated FilterBar + filter-driven row list + empty state", async ({
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

    // ── Assertion 1: Toggle OFF -> no FilterBar ─────────────────
    await page.goto("/#/knowledge-graph");
    await expect(
      page.getByRole("heading", { name: /knowledge graph is off/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("group", { name: /^filter by$/i }),
    ).toHaveCount(0);
    await page.screenshot({
      path: "test-results/kg-retrieval-1-toggle-off.png",
    });

    // ── Assertion 2: Toggle ON + empty filter -> idle copy ──────
    // Stage toggle-on + empty dashboard so the route mounts; the
    // Retrieval band is the unit under test.
    await page.addInitScript((dash) => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        kg_settings_get_all: { kgGraphEnabled: true },
        kg_dashboard_snapshot: dash,
        kg_search_entries: [],
        kg_list_entities: [],
        kg_list_tags: [],
        kg_entries_summary: {},
      };
    }, EMPTY_DASHBOARD);
    await page.reload();

    await expect(
      page.getByRole("heading", { name: /^find entries$/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("group", { name: /^filter by$/i }),
    ).toBeVisible();
    // Idle copy is visible; no chip-strip buttons anywhere on the
    // Retrieval band (the only chip-strip on the page when chips
    // are unselected is the recent-activity band's, which is
    // empty in this fixture).
    await expect(
      page.getByText(/pick an entity or tag above/i),
    ).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-retrieval-2-idle.png",
    });

    // ── Assertion 3: Pick entity chip -> row list renders ───────
    // Stage the autocomplete + filter result + per-row summary.
    // `list_sessions` is the base list the visible set
    // intersects against; the fixture provides it via the default
    // FIXTURES.sessions seed.
    await page.evaluate(
      ({ sug, filterRes, row100 }) => {
        window.__MOCKINGBIRD_FIXTURES__ = {
          ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
          kg_list_entities: sug,
          kg_search_entries: filterRes,
          kg_entries_summary: { "100": row100 },
        };
      },
      {
        sug: MOM_SUGGESTION,
        filterRes: [100],
        row100: ROW_100_SUMMARY,
      },
    );

    const entityInput = page.getByRole("combobox", {
      name: /filter dictations by entity/i,
    });
    await entityInput.focus();
    const momOption = page.getByRole("option", { name: /mom/i });
    await expect(momOption).toBeVisible({ timeout: 2000 });
    await momOption.click();

    // Chip-add fires `kg_search_entries`; the row list renders
    // below. The matched row carries Mom's entity chip (button
    // mode because the dashboard always passes `onConceptOpen`).
    await expect(
      page.getByRole("button", { name: /remove entity mom/i }),
    ).toBeVisible();
    await expect(
      page.getByLabel(/^open concept page for entity mom$/i).first(),
    ).toBeVisible();
    // Idle copy gone now that the filter is active.
    await expect(
      page.getByText(/pick an entity or tag above/i),
    ).toHaveCount(0);

    await page.screenshot({
      path: "test-results/kg-retrieval-3-entity-chip-applied.png",
    });

    // ── Assertion 4: Add tag chip -> cross-axis AND narrows ─────
    await page.evaluate(
      ({ tagSug, filterRes }) => {
        window.__MOCKINGBIRD_FIXTURES__ = {
          ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
          kg_list_tags: tagSug,
          kg_search_entries: filterRes,
        };
      },
      { tagSug: FAMILY_TAG_SUGGESTION, filterRes: [100] },
    );

    const tagInput = page.getByRole("combobox", {
      name: /filter dictations by tag/i,
    });
    await tagInput.focus();
    const familyOption = page.getByRole("option", { name: /family/i });
    await expect(familyOption).toBeVisible({ timeout: 2000 });
    await familyOption.click();

    await expect(
      page.getByRole("button", { name: /remove tag family/i }),
    ).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-retrieval-4-cross-axis-and.png",
    });

    // ── Assertion 5: Empty result + Clear filters ───────────────
    await page.evaluate(() => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
        kg_search_entries: [],
      };
    });
    // Bounce the tag chip to force re-evaluation with the new
    // search result.
    await page
      .getByRole("button", { name: /remove tag family/i })
      .click();
    await tagInput.focus();
    await (await page.getByRole("option", { name: /family/i })).click();

    await expect(
      page.getByText(/no dictations match these filters/i),
    ).toBeVisible();
    const emptyState = page
      .getByRole("status")
      .filter({ hasText: /no dictations match/i });
    const clearLink = emptyState.getByRole("button", {
      name: /clear filters/i,
    });
    await expect(clearLink).toBeVisible();

    await clearLink.click();
    await expect(
      page.getByRole("button", { name: /remove entity mom/i }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: /remove tag family/i }),
    ).toHaveCount(0);
    // Idle copy back.
    await expect(
      page.getByText(/pick an entity or tag above/i),
    ).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-retrieval-5-cleared.png",
    });

    // ── Assertion 6: No console errors ──────────────────────────
    expect(
      consoleErrors,
      `Unexpected console errors during KG retrieval flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
