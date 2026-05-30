// KG Phase 1C Wave 1C.4 — concept modal visual sweep
// (bead `mb-sx6p`, ADR 0051 D4). Sibling of
// `kg-dictations-retrieval.spec.ts` (Wave 1C.3); reuses the
// `window.__MOCKINGBIRD_FIXTURES__` mid-flight mutation harness.
//
// qa-kitten bowed out (no filesystem tools on that agent) so this
// spec was authored by code-puppy per the Wave 1C.4 kickoff
// fallback clause ("expect fallback per 1C.3 — author Playwright
// yourself if so").
//
// Six assertions, all must pass with zero console errors:
//
//   1. Toggle OFF (default fixture) → entity + tag chips are NOT
//      rendered as `Open concept page for ...` buttons. A guard
//      against a future regression that flips chip-to-button
//      wiring on while graph is still off (the
//      graph-off-ui-untouched invariant; full sweep at 1C.5).
//   2. Toggle ON + click an entity chip → modal opens; header
//      shows the canonical entity name (h2); body shows the
//      mention counter, the aliases section, and the recent-
//      entries list (each row keyboard-activatable as a button).
//   3. Toggle ON + click a tag chip → modal opens with the
//      `#slug` header (per `kg.concept.tagTitle`), counter,
//      recent-entries list, AND no aliases section (tag mode).
//   4. Click a recent-entries row inside the modal → modal
//      closes AND that session is selected in the detail pane.
//   5. Press Escape with the modal open → modal closes (Dialog
//      primitive's native <dialog>.showModal() handles this; we
//      assert the contract holds end-to-end).
//   6. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

// ────────────────────────────────────────────────────────────────
// Fixture payloads. Keep them short — each one is the minimum the
// modal needs to render its shape. The wave-1C.3 fixture rig only
// pattern-matches by command name (not args), so we re-stage
// between chip clicks rather than try to vary per-id.
// ────────────────────────────────────────────────────────────────

/** Per-row summary keyed off the first two seed sessions. Mirrors
 *  the 1C.3 spec's HIGH_FANOUT / FAILED shapes but smaller — we
 *  only need ONE entity + ONE tag visible per row to click. */
const ROW_100_SUMMARY = {
  entities: [{ entityId: 42, canonicalName: "Mom", entityType: "person" }],
  tags: [{ tagSlug: "family" }],
  filingState: "done",
};

const ROW_101_SUMMARY = {
  entities: [{ entityId: 43, canonicalName: "Acme", entityType: "organization" }],
  tags: [{ tagSlug: "work" }],
  filingState: "done",
};

/** Entity-mode detail payload for the "Mom" chip. `recentEntries`
 *  points at session 100 so the assertion-4 click-to-jump can
 *  verify selection against the known fixture entry's `finalText`
 *  ("Hey team, just pushed the Phase 4 LLM cleanup work..."). */
const MOM_ENTITY_DETAIL = {
  entityId: 42,
  canonicalName: "Mom",
  entityType: "person",
  aliases: ["Mama", "Mommy"],
  mentionCount: 3,
  totalEntries: 2,
  recentEntries: [
    {
      entryId: 100,
      title: "Hey team, just pushed the Phase 4 LLM cleanup work.",
      capturedIso: "2026-05-27T10:00:00Z",
      category: null,
    },
    {
      entryId: 102,
      title: "Sample dictation about Mom",
      capturedIso: "2026-05-25T10:00:00Z",
      category: null,
    },
  ],
};

/** Tag-mode detail payload for the "family" chip. No aliases
 *  section in the modal for tags (TagDetail has no `aliases`
 *  field on the Rust side either). */
const FAMILY_TAG_DETAIL = {
  tagSlug: "family",
  mentionCount: 5,
  totalEntries: 3,
  recentEntries: [
    {
      entryId: 101,
      title: "Add a `--verbose` flag to the build script.",
      capturedIso: "2026-05-27T10:00:00Z",
      category: null,
    },
    {
      entryId: 103,
      title: "Sample dictation #3",
      capturedIso: "2026-05-25T10:00:00Z",
      category: null,
    },
  ],
};

test.describe("KG Phase 1C.4 — concept modal (mb-sx6p)", () => {
  test("entity + tag drill-down, click-to-jump, Escape-to-close", async ({
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

    // ── Assertion 1: Toggle OFF → no concept-page buttons ─────
    // Default fixture has `kgGraphEnabled: false`. The chip
    // strip's button mode is wired only when `onConceptOpen` is
    // passed down, which Dictations.tsx gates on `kgEnabled`. So
    // with graph OFF, there should be zero "Open concept page
    // for ..." buttons in the DOM.
    await page.goto("/#/dictations");
    await expect(page.getByText(/Sample dictation/i).first()).toBeVisible();

    await expect(
      page.getByRole("button", { name: /open concept page for entity/i }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: /open concept page for tag/i }),
    ).toHaveCount(0);

    await page.screenshot({
      path: "test-results/kg-concept-modal-1-toggle-off.png",
    });

    // ── Assertion 2: Toggle ON + entity chip click → modal ──
    // Stage the toggle-on settings + per-row summaries + the
    // entity-detail response. `addInitScript` survives reload;
    // `kg_settings_get_all` re-fires on the post-reload mount.
    await page.addInitScript(
      ({ r100, r101, entityDetail, tagDetail }) => {
        window.__MOCKINGBIRD_FIXTURES__ = {
          kg_settings_get_all: { kgGraphEnabled: true },
          kg_entries_summary: {
            // Stringified i64 keys per the Rust HashMap<i64, _>
            // JSON-boundary convention (verified by ui-author test
            // + the 1C.3 spec's identical staging).
            "100": r100,
            "101": r101,
          },
          kg_entity_detail: entityDetail,
          kg_tag_detail: tagDetail,
          // Empty so the filter-empty branch doesn't trigger.
          kg_search_entries: [],
        };
      },
      {
        r100: ROW_100_SUMMARY,
        r101: ROW_101_SUMMARY,
        entityDetail: MOM_ENTITY_DETAIL,
        tagDetail: FAMILY_TAG_DETAIL,
      },
    );

    await page.reload();
    await expect(page.getByText(/Sample dictation/i).first()).toBeVisible();

    // Concept-page buttons now present (one per chip; row 100
    // has Mom, row 101 has Acme; tags too).
    const momChip = page
      .getByRole("button", { name: /open concept page for entity mom/i })
      .first();
    await expect(momChip).toBeVisible();

    // Click → modal opens. The Dialog primitive uses native
    // <dialog>; the role exposed is "dialog" with the aria-label
    // resolved from `kg.concept.entityTitle` → "Mom" (no
    // truncation needed at this length).
    await momChip.click();

    const entityDialog = page.getByRole("dialog", { name: /^mom$/i });
    await expect(entityDialog).toBeVisible();

    // Header: h2 with "Mom"; entity-type badge underneath.
    await expect(
      entityDialog.getByRole("heading", { level: 2, name: /^mom$/i }),
    ).toBeVisible();
    await expect(
      entityDialog.getByLabel(/^entity type$/i),
    ).toHaveText(/person/i);

    // Body: mention count (pluralized — 3 mentions).
    await expect(entityDialog.getByText(/3 mentions/)).toBeVisible();
    // Aliases section is present for entity mode.
    await expect(entityDialog.getByText(/^also known as$/i)).toBeVisible();
    await expect(entityDialog.getByText(/^Mama$/)).toBeVisible();
    await expect(entityDialog.getByText(/^Mommy$/)).toBeVisible();
    // Recent-entries list: 2 rows, one per recentEntries item.
    // RecentRow is a <button> with an explicit role="listitem"
    // override, so `getByRole("button")` does NOT match it.
    // Per Wave 1C.3 LESSONS Finding 1 ("getByLabel not
    // getByRole"), match via the aria-label instead.
    const recentRows = entityDialog.getByLabel(/^open dictation /i);
    await expect(recentRows).toHaveCount(2);

    await page.screenshot({
      path: "test-results/kg-concept-modal-2-entity-open.png",
    });

    // ── Assertion 5 (out of order — easier here while we have
    //                 the entity modal open): Escape closes the
    //                 modal via the Dialog primitive. ─────────────
    await page.keyboard.press("Escape");
    await expect(entityDialog).toBeHidden();

    // ── Assertion 3: Tag chip → modal opens with tag-mode shape ─
    const familyChip = page
      .getByRole("button", { name: /open concept page for tag family/i })
      .first();
    await expect(familyChip).toBeVisible();
    await familyChip.click();

    // Title format is `#{slug}` per `kg.concept.tagTitle`.
    const tagDialog = page.getByRole("dialog", { name: /^#family$/i });
    await expect(tagDialog).toBeVisible();

    // h2 + counter + recent list — but NO aliases section
    // (tag mode renders no aliases at all).
    await expect(
      tagDialog.getByRole("heading", { level: 2, name: /^#family$/i }),
    ).toBeVisible();
    await expect(tagDialog.getByText(/5 mentions/)).toBeVisible();
    await expect(tagDialog.getByText(/^also known as$/i)).toHaveCount(0);
    // And no entity-type badge.
    await expect(tagDialog.getByLabel(/^entity type$/i)).toHaveCount(0);

    const tagRecentRows = tagDialog.getByLabel(/^open dictation /i);
    await expect(tagRecentRows).toHaveCount(2);

    await page.screenshot({
      path: "test-results/kg-concept-modal-3-tag-open.png",
    });

    // ── Assertion 4: Click a recent row → modal closes + entry
    //                 selected in detail pane. ────────────────────
    // The first row in FAMILY_TAG_DETAIL points at entryId 101.
    // Clicking it calls `setSelectedId(101)` which fires
    // `get_session_detail(101)`. The detail pane renders the
    // formatted startedAt as an h2; the body shows the final
    // text. Probe via the final-text string (stable in the
    // fixture row at i=1: "Add a `--verbose` flag...").
    const firstRecent = tagRecentRows.first();
    await expect(firstRecent).toHaveAccessibleName(
      /open dictation add a `--verbose` flag/i,
    );
    await firstRecent.click();

    // Modal gone first.
    await expect(tagDialog).toBeHidden();

    // Detail pane now shows session 101's content. The fixture's
    // `sessionDetails` array seeds id 100 explicitly; for 101 the
    // shim falls back to the auto-generated session, whose final
    // text matches the row at index i=1 in `FIXTURES.sessions`.
    // We probe via that string in the detail pane.
    //
    // The detail pane is the right-hand pane on /dictations. We
    // scope to the page-level text rather than a specific role
    // to avoid coupling to the detail pane's internal structure
    // (which is exercised independently by other specs).
    await expect(
      page.getByText(/add a `--verbose` flag to the build script/i).first(),
    ).toBeVisible();

    await page.screenshot({
      path: "test-results/kg-concept-modal-4-click-to-jump.png",
    });

    // ── Assertion 6: No console errors across the entire flow ──
    expect(
      consoleErrors,
      `Unexpected console errors during KG concept-modal flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
