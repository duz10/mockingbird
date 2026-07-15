// KG Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) -- Concept modal
// drill-down. Origin: Phase 1C Wave 1C.4 / `mb-sx6p`; rehomed on
// the KG dashboard alongside the rest of the KG retrieval surface
// when Wave 1D.4 relocated the modal from Dictations to
// `routes/knowledge-graph/`.
//
// The modal mounts once at the dashboard level (KnowledgeGraphDashboard)
// and is opened from either:
//   * Retrieval band -- chip clicks on filtered rows
//   * Recent-activity band -- chip clicks on auto-rendered rows
//
// Both surfaces use the same `onConceptOpen` plumbing, so we only
// need to test ONE chip-click path here (the recent-activity band
// is easier to stage -- no filter dance required, just seed the
// dashboard snapshot with a row carrying chips).
//
// Six assertions, all must pass with zero console errors:
//   1. Toggle OFF -> /knowledge-graph renders disabled-state; no
//      concept-page buttons in the DOM.
//   2. Toggle ON + seeded recent-activity row -> entity chip
//      clickable; click opens the entity-mode modal with header,
//      mention count, aliases, recent-entries list.
//   3. Press Escape -> modal closes.
//   4. Click a tag chip on the same row -> tag-mode modal opens
//      (no aliases section, header is "#slug").
//   5. Click a recent-entries row inside the tag modal -> modal
//      closes (the dashboard's `onSelectEntry` handler is the
//      modal-close path; deep-linking to Dictations is a follow-up
//      bead).
//   6. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

import { forceWindowsHost } from "./kg-host";

const RECENT_ROW = {
  entryId: 100,
  title: "Talked with Mom about the calculus assignment",
  capturedIso: "2026-05-30T10:00:00Z",
  category: null as string | null,
  entities: [{ entityId: 42, canonicalName: "Mom", entityType: "person" }],
  tags: [{ tagSlug: "family" }],
};

const SEEDED_DASHBOARD = {
  counts: { totalEntities: 1, totalEntries: 1, entitiesByType: [] },
  queueStatus: {
    pending: 0,
    processing: 0,
    failed: 0,
    done: 1,
    lastDoneIso: "2026-05-30T10:00:00Z" as string | null,
  },
  recentActivity: [RECENT_ROW],
  flaggedForReview: [],
  upcomingDue: [],
};

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
      title: "Talked with Mom about the calculus assignment",
      capturedIso: "2026-05-27T10:00:00Z",
      category: null,
    },
  ],
};

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
  ],
};

test.describe("KG Phase 1D.4 -- concept modal (mb-6hm2 / mb-sx6p)", () => {
  test("entity + tag drill-down, Escape-to-close, recent-row-click closes", async ({
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

    // KG is Windows-only; force the fixture host to "windows" so the KG
    // route renders the real UI (not the macOS coming-soon path) —
    // mb-0cg. Must precede the first navigation.
    await forceWindowsHost(page);

    // ── Assertion 1: Toggle OFF -> no concept-page buttons ─────
    await page.goto("/#/knowledge-graph");
    await expect(
      page.getByRole("heading", { name: /knowledge graph is off/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: /open concept page for entity/i }),
    ).toHaveCount(0);
    await page.screenshot({
      path: "test-results/kg-concept-modal-1-toggle-off.png",
    });

    // ── Assertion 2: Entity chip click -> modal opens ──────────
    await page.addInitScript(
      ({ dash, entityDetail, tagDetail }) => {
        window.__MOCKINGBIRD_FIXTURES__ = {
          host_os: "windows", // mb-0cg — keep KG on the Windows code path
          kg_settings_get_all: { kgGraphEnabled: true },
          kg_dashboard_snapshot: dash,
          kg_entity_detail: entityDetail,
          kg_tag_detail: tagDetail,
          kg_search_entries: [],
          kg_list_entities: [],
          kg_list_tags: [],
          kg_entries_summary: {},
        };
      },
      {
        dash: SEEDED_DASHBOARD,
        entityDetail: MOM_ENTITY_DETAIL,
        tagDetail: FAMILY_TAG_DETAIL,
      },
    );
    await page.reload();
    await expect(
      page.getByRole("heading", { name: /^knowledge graph$/i, level: 1 }),
    ).toBeVisible();

    const momChip = page
      .getByRole("button", { name: /open concept page for entity mom/i })
      .first();
    await expect(momChip).toBeVisible();
    await momChip.click();

    const entityDialog = page.getByRole("dialog", { name: /^mom$/i });
    await expect(entityDialog).toBeVisible();
    await expect(
      entityDialog.getByRole("heading", { level: 2, name: /^mom$/i }),
    ).toBeVisible();
    await expect(entityDialog.getByLabel(/^entity type$/i)).toHaveText(
      /person/i,
    );
    await expect(entityDialog.getByText(/3 mentions/)).toBeVisible();
    await expect(entityDialog.getByText(/^also known as$/i)).toBeVisible();
    await expect(entityDialog.getByText(/^Mama$/)).toBeVisible();
    await expect(entityDialog.getByText(/^Mommy$/)).toBeVisible();
    const recentRows = entityDialog.getByLabel(/^open dictation /i);
    await expect(recentRows).toHaveCount(1);
    await page.screenshot({
      path: "test-results/kg-concept-modal-2-entity-open.png",
    });

    // ── Assertion 3: Escape closes ──────────────────────────────
    await page.keyboard.press("Escape");
    await expect(entityDialog).toBeHidden();

    // ── Assertion 4: Tag chip -> tag-mode modal opens ──────────
    const familyChip = page
      .getByRole("button", { name: /open concept page for tag family/i })
      .first();
    await expect(familyChip).toBeVisible();
    await familyChip.click();

    const tagDialog = page.getByRole("dialog", { name: /^#family$/i });
    await expect(tagDialog).toBeVisible();
    await expect(
      tagDialog.getByRole("heading", { level: 2, name: /^#family$/i }),
    ).toBeVisible();
    await expect(tagDialog.getByText(/5 mentions/)).toBeVisible();
    await expect(tagDialog.getByText(/^also known as$/i)).toHaveCount(0);
    await expect(tagDialog.getByLabel(/^entity type$/i)).toHaveCount(0);
    const tagRecentRows = tagDialog.getByLabel(/^open dictation /i);
    await expect(tagRecentRows).toHaveCount(1);
    await page.screenshot({
      path: "test-results/kg-concept-modal-3-tag-open.png",
    });

    // ── Assertion 5: Recent-row click closes the modal ─────────
    // On the dashboard, the dashboard's `onSelectEntry` handler
    // simply closes the modal (deep-linking to Dictations detail
    // is a follow-up bead). Verify the close happens.
    const firstRecent = tagRecentRows.first();
    await firstRecent.click();
    await expect(tagDialog).toBeHidden();
    await page.screenshot({
      path: "test-results/kg-concept-modal-4-row-click-closes.png",
    });

    // ── Assertion 6: No console errors ──────────────────────────
    expect(
      consoleErrors,
      `Unexpected console errors during KG concept-modal flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
