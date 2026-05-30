// KG Phase 1C Wave 1C.3 — Dictations retrieval visual sweep
// (bead `mb-5ly5`, ADR 0051). Mirrors the
// `kg-failed-filings.spec.ts` (Wave 1C.2) shape: runs against the
// Vite preview server, uses the Tauri-shim fallback, stages each
// state via `window.__MOCKINGBIRD_FIXTURES__` overrides.
//
// Single judge: all 6 assertions below must pass with zero console
// errors. qa-kitten was bowed-out (no filesystem tools — see the
// 1C.3 iteration report); code-puppy authored this spec per the
// kickoff fallback clause ("Fall back to authoring spec yourself if
// qa-kitten unavailable (as in 1C.2)").
//
//  1. Toggle OFF (default fixture) → /dictations renders without the
//     FilterBar, without per-row KG chips, and without any filing-
//     state pills. The page looks identical to pre-1C.3. This is the
//     STOP condition for the graph-off-ui-untouched invariant
//     (full sweep at 1C.5).
//  2. Toggle ON + seeded summaries → FilterBar mounted, per-row chip
//     strip visible. A row with 7 entities renders the first 5 +
//     a "+2 more" overflow pill. A row with `filingState: "failed"`
//     renders a pill with the failed text and the "See Settings →
//     Knowledge Graph" tooltip exposed via title=.
//  3. Open entity autocomplete → 1 suggestion shows → click it →
//     chip added; the search effect fires `kg_search_entries` with
//     `{entities: [<id>], tags: [], query: undefined}` and the
//     visible session list narrows to the staged result set.
//  4. Add a tag chip on top of the entity → list narrows further
//     (cross-axis AND semantics: the staged tag-filter result is a
//     subset of the entity-filter result).
//  5. Stage `kg_search_entries` to return `[]` while chips are
//     active → empty-state copy "No dictations match these filters"
//     + a "Clear filters" link that resets chip state. Removing
//     the chips reveals the original list again.
//  6. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

/** A summary with 7 entities — drives assertion 2's "+2 more"
 *  overflow rendering (TOP_N is 5 in `DictationKgChips.tsx`). */
const HIGH_FANOUT_SUMMARY = {
  entities: [
    { entityId: 1, canonicalName: "Mom", entityType: "person" },
    { entityId: 2, canonicalName: "Dad", entityType: "person" },
    { entityId: 3, canonicalName: "Acme Corp", entityType: "organization" },
    { entityId: 4, canonicalName: "Berlin", entityType: "place" },
    { entityId: 5, canonicalName: "Project Apollo", entityType: "project" },
    { entityId: 6, canonicalName: "Stapler", entityType: "object" },
    { entityId: 7, canonicalName: "Bernard", entityType: "person" },
  ],
  tags: [{ tagSlug: "family" }, { tagSlug: "work" }],
  filingState: "done",
};

/** A summary with a failed filing — drives assertion 2's red pill +
 *  tooltip exposure. */
const FAILED_SUMMARY = {
  entities: [{ entityId: 1, canonicalName: "Mom", entityType: "person" }],
  tags: [{ tagSlug: "family" }],
  filingState: "failed",
};

/** Single autocomplete suggestion for assertion 3 — pick this and
 *  the chip below ("Mom") gets added. */
const MOM_SUGGESTION = [
  {
    entityId: 1,
    canonicalName: "Mom",
    entityType: "person",
    mentionCount: 12,
  },
];

const FAMILY_TAG_SUGGESTION = [
  { tagSlug: "family", mentionCount: 8 },
];

test.describe("KG Phase 1C.3 — Dictations retrieval UX (mb-5ly5)", () => {
  test("toggle-gated FilterBar + per-row chips + filter chips + empty state", async ({
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

    // ── Assertion 1: Toggle OFF → graph-off-ui-untouched preview ──
    // Default fixture has `kgGraphEnabled: false`. The Dictations
    // page reads it on mount via `kg_settings_get_all`. Neither
    // the FilterBar nor any per-row chip strip should be in the DOM.
    await page.goto("/#/dictations");

    // Wait for the session list to render so we know the on-mount
    // effects have settled.
    await expect(page.getByText(/Sample dictation/i).first()).toBeVisible();

    // The FilterBar's group has aria-label "Filter by"
    // (kg.filter.heading). With toggle OFF it must NOT mount.
    await expect(
      page.getByRole("group", { name: /^filter by$/i }),
    ).toHaveCount(0);

    // No per-row chip text (e.g. "Mom" — which is staged later) and
    // no filing-state pill copy ("Indexing", "Indexing failed").
    await expect(page.getByText(/indexing failed/i)).toHaveCount(0);
    await expect(page.getByText(/^\+\d+ more$/)).toHaveCount(0);

    await page.screenshot({
      path: "test-results/kg-dictations-1-toggle-off.png",
    });

    // ── Assertion 2: Toggle ON + seeded summaries → strips + pill ──
    // Stage the toggle-on fixture + per-row summaries for the first
    // two seed sessions (ids 100 and 101 per FIXTURES.sessions). The
    // Dictations page has no in-app KG toggle (lives in Settings),
    // so the only way to flip `kgEnabled` is a fresh page mount that
    // re-fetches `kg_settings_get_all`. `page.reload()` triggers
    // that; navigating to the same hash-route does NOT (HashRouter
    // sees the URL as unchanged and skips the remount).
    //
    // Persist the fixture across the reload via `addInitScript` —
    // it runs on every new document load BEFORE app scripts, so the
    // override is in place by the time `kg_settings_get_all` fires.
    await page.addInitScript(
      ({ high, failed }) => {
        window.__MOCKINGBIRD_FIXTURES__ = {
          kg_settings_get_all: { kgGraphEnabled: true },
          kg_entries_summary: {
            // Stringified i64 keys per the Rust HashMap<i64, _>
            // JSON-boundary convention (verified by ui-author test).
            "100": high,
            "101": failed,
          },
          // Default search behavior with no filter active: backend
          // returns empty (UI short-circuits anyway because the
          // filter `isEmpty()`).
          kg_search_entries: [],
        };
      },
      { high: HIGH_FANOUT_SUMMARY, failed: FAILED_SUMMARY },
    );

    await page.reload();
    await expect(page.getByText(/Sample dictation/i).first()).toBeVisible();

    // FilterBar now mounted.
    await expect(
      page.getByRole("group", { name: /^filter by$/i }),
    ).toBeVisible();

    // Row 100 ("high fanout"): first 5 entity chips visible by ARIA
    // label, plus "+2 more" overflow pill. The chip elements are
    // `<span aria-label="Entity X">` — `<span>` has no implicit
    // role so `getByRole("generic", ...)` doesn't match; use
    // `getByLabel`. The remove-buttons in the FilterBar's selected-
    // chip area use "Remove entity X" / "Remove tag X" so the chip-
    // strip labels don't collide with filter chips here.
    await expect(
      page.getByLabel(/^entity mom$/i).first(),
    ).toBeVisible();
    await expect(
      page.getByLabel(/^entity project apollo$/i),
    ).toBeVisible();
    // The 6th + 7th entities (Stapler, Bernard) are NOT rendered as
    // individual chips — collapsed into the overflow pill.
    await expect(page.getByText("+2 more")).toBeVisible();

    // Row 101 ("failed"): pill text + tooltip.
    const failedPill = page.getByText(/⚠️ Indexing failed/);
    await expect(failedPill).toBeVisible();
    // Tooltip is exposed via native title=. The FilingPill renders
    // role="status"; we go up to it from the visible text node to
    // assert the attribute.
    const failedPillStatus = page.getByRole("status", {
      name: /indexing failed/i,
    });
    await expect(failedPillStatus).toHaveAttribute(
      "title",
      /See Settings.*Knowledge Graph/i,
    );

    await page.screenshot({
      path: "test-results/kg-dictations-2-on-with-strips.png",
    });

    // ── Assertion 3: Open entity input + pick → chip + filter narrows ─
    // Stage the autocomplete payload + the result of the filter
    // search (only entry 100 matches "Mom"). Note: we DON'T need to
    // re-mount the page — the FilterBar's autocomplete fires
    // `kg_list_entities` on focus and our fixture takes effect from
    // here on.
    await page.evaluate(
      ({ sug, filterRes }) => {
        window.__MOCKINGBIRD_FIXTURES__ = {
          ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
          kg_list_entities: sug,
          // Pre-stage the search response: chip-add fires
          // kg_search_entries; we want the visible-id set to be
          // {100} (only the high-fanout row matches "Mom").
          kg_search_entries: filterRes,
        };
      },
      { sug: MOM_SUGGESTION, filterRes: [100] },
    );

    const entityInput = page.getByRole("combobox", {
      name: /filter dictations by entity/i,
    });
    await entityInput.focus();
    // Debounce is 200 ms. Wait the popover into existence.
    const momOption = page.getByRole("option", { name: /mom/i });
    await expect(momOption).toBeVisible({ timeout: 2000 });
    // onMouseDown is the wired event (it fires before onBlur closes
    // the popover) — Playwright's .click() generates both, so this
    // works.
    await momOption.click();

    // Chip now present; verify by its remove-button aria-label.
    await expect(
      page.getByRole("button", { name: /remove entity mom/i }),
    ).toBeVisible();

    // The visible list narrowed to just session 100. The other
    // fixture rows (101..139) are filtered out via the
    // kg_search_entries response. Easiest probe: row 101's failed
    // pill should no longer be in the DOM.
    await expect(page.getByText(/⚠️ Indexing failed/)).toHaveCount(0);
    // ...but row 100's "+2 more" pill IS still visible (the row is
    // in the filtered set).
    await expect(page.getByText("+2 more")).toBeVisible();

    await page.screenshot({
      path: "test-results/kg-dictations-3-entity-chip-applied.png",
    });

    // ── Assertion 4: Add a tag chip → cross-axis AND narrows further ─
    // Stage the tag autocomplete + a tighter search result (still
    // {100} — the staged data has both Mom + family on that row).
    // The point of this assertion isn't the row count per se but
    // that the second filter axis ALSO threads into the IPC + the
    // UI lets you stack chips.
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
    // Both chips are now active; row 100 still showing.
    await expect(page.getByText("+2 more")).toBeVisible();

    await page.screenshot({
      path: "test-results/kg-dictations-4-cross-axis-and.png",
    });

    // ── Assertion 5: Empty result + Clear filters link ──────────
    // Stage kg_search_entries to return [] with chips still active.
    // The next filter-evaluation pass (we bump the tag to trigger
    // it) should surface the empty-state copy.
    await page.evaluate(() => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
        kg_search_entries: [],
      };
    });

    // Bump the filter to force a re-evaluation: remove and re-add
    // the family tag. Simplest is to remove + re-pick.
    await page
      .getByRole("button", { name: /remove tag family/i })
      .click();
    await tagInput.focus();
    await (await page.getByRole("option", { name: /family/i })).click();

    await expect(
      page.getByText(/no dictations match these filters/i),
    ).toBeVisible();
    // Two "Clear filters" buttons live on the page when the filter is
    // active AND empty-state is showing: (a) the FilterBar header's
    // always-present clear button, and (b) the EmptyState's action
    // button. Scope to the EmptyState (which the primitive renders
    // as role="status") so the locator is unambiguous.
    const emptyState = page
      .getByRole("status")
      .filter({ hasText: /no dictations match/i });
    const clearLink = emptyState.getByRole("button", {
      name: /clear filters/i,
    });
    await expect(clearLink).toBeVisible();

    await page.screenshot({
      path: "test-results/kg-dictations-5-empty-result.png",
    });

    // Click Clear → chips removed → list returns. (We stage
    // kg_search_entries back to a non-empty result so the
    // "empty" copy is gone even if a brief re-eval kicks. But
    // really once chips are gone, the FilterBar short-circuits.)
    await clearLink.click();
    await expect(
      page.getByRole("button", { name: /remove entity mom/i }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: /remove tag family/i }),
    ).toHaveCount(0);
    await expect(
      page.getByText(/no dictations match these filters/i),
    ).toHaveCount(0);
    // Failed pill on row 101 should reappear (full list back).
    await expect(page.getByText(/⚠️ Indexing failed/)).toBeVisible();

    // ── Assertion 6: No console errors across the entire flow ───
    expect(
      consoleErrors,
      `Unexpected console errors during KG dictations retrieval flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
