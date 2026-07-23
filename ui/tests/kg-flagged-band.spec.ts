// KG Phase 1D Wave 1D.4 (`mb-6hm2`, ADR 0052 D5) -- Flagged-for-
// review band on the KG dashboard.
//
// Origin: this spec started life as `kg-failed-filings.spec.ts`
// (Phase 1C Wave 1C.2 / `mb-9ufg`) and tested the
// `SettingsKgFailedFilings` panel that briefly lived under
// Settings -> Knowledge Graph. Wave 1D.4 relocated the failed-
// filings + retry UX onto the KG dashboard's Flagged-for-review
// band; this spec moves with it. Same five assertion shape, same
// fixture rig, new route + new selectors.
//
//  1. Toggle OFF -> /knowledge-graph renders the disabled-state
//     EmptyState; no FlaggedBand mounts (the route's guard short-
//     circuits the whole dashboard).
//  2. Toggle ON + empty fixture -> dashboard mounts; FlaggedBand
//     shows the empty-state copy ("Nothing flagged -- all filings
//     succeeded.").
//  3. Toggle ON + seeded fixture (one failed row) -> FlaggedBand
//     row renders with "Dictation #1337", "3 attempts" pill, and
//     a Retry button reachable by its ARIA label.
//  4. Click Retry -> "Filing requeued" toast appears, the
//     dashboard refetches via `kg_dashboard_snapshot` (now empty
//     per the pre-staged fixture), and the row drops out.
//  5. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

import { forceWindowsHost } from "./kg-host";

const SEEDED_ROW = {
  queueId: 42,
  entryId: 1337,
  attemptCount: 3,
  lastError: "ollama refused embedding request: model not found",
  enqueuedIso: "2026-05-30T09:30:00Z",
  failedIso: "2026-05-30T09:31:14Z",
};

const EMPTY_SNAPSHOT = {
  counts: { totalEntities: 0, totalEntries: 0, entitiesByType: [] },
  queueStatus: {
    pending: 0,
    processing: 0,
    failed: 0,
    done: 0,
    lastDoneIso: null as string | null,
  },
  recentActivity: [] as unknown[],
  flaggedForReview: [] as unknown[],
  upcomingDue: [] as unknown[],
};

const SEEDED_SNAPSHOT = {
  ...EMPTY_SNAPSHOT,
  queueStatus: {
    pending: 3,
    processing: 1,
    failed: 1,
    done: 0,
    lastDoneIso: "2026-05-30T10:15:23Z",
  },
  flaggedForReview: [SEEDED_ROW],
};

const POST_RETRY_SNAPSHOT = {
  ...EMPTY_SNAPSHOT,
  queueStatus: {
    pending: 4,
    processing: 1,
    failed: 0,
    done: 0,
    lastDoneIso: "2026-05-30T10:15:23Z",
  },
  flaggedForReview: [],
};

test.describe("KG Phase 1D.4 -- Flagged-for-review band (mb-6hm2)", () => {
  test("toggle gating, empty state, seeded row, retry -> toast + refresh", async ({
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

    // ── Assertion 1: Toggle OFF -> disabled-state on /knowledge-graph ─
    await page.goto("/#/knowledge-graph");
    await expect(
      page.getByRole("heading", { name: /knowledge graph is off/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /flagged for review/i }),
    ).toHaveCount(0);
    await page.screenshot({
      path: "test-results/kg-flagged-1-toggle-off.png",
    });

    // ── Assertion 2: Toggle ON + empty snapshot -> empty-state copy ─
    // Stage the empty snapshot BEFORE flipping the toggle so the
    // dashboard's mount-time fetch reads the right shape on first
    // paint.
    await page.addInitScript((snap) => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        host_os: "windows", // mb-0cg — keep KG on the Windows code path
        kg_settings_get_all: { kgGraphEnabled: true },
        kg_dashboard_snapshot: snap,
      };
    }, EMPTY_SNAPSHOT);
    await page.reload();

    await expect(
      page.getByRole("heading", { name: /^knowledge graph$/i, level: 1 }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /flagged for review/i }),
    ).toBeVisible();
    await expect(page.getByText(/nothing flagged/i)).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-flagged-2-on-empty.png",
    });

    // ── Assertion 3: Seeded row -> entry + pill + retry button ──
    // Stage via addInitScript (NOT page.evaluate): Assertion 2's
    // addInitScript re-runs on every navigation and resets the snapshot
    // to EMPTY, so a runtime-only (page.evaluate) seed would be wiped by
    // the reload below. Registering this init script AFTER Assertion 2's
    // means it runs last on reload and its seeded snapshot wins.
    await page.addInitScript((snap) => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
        kg_dashboard_snapshot: snap,
      };
    }, SEEDED_SNAPSHOT);
    // Refetch via the CaptureBand's onFiled hook would normally
    // drive a snapshot refresh; here we just reload to re-trigger
    // the mount-time fetch with the new fixture.
    await page.reload();

    const list = page.getByRole("list", { name: /flagged for review/i });
    await expect(list).toBeVisible();
    await expect(list.getByText(/dictation #1337/i)).toBeVisible();
    await expect(list.getByText(/3 attempts/i)).toBeVisible();
    const retryBtn = page.getByRole("button", {
      name: /retry filing for dictation #1337/i,
    });
    await expect(retryBtn).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-flagged-3-seeded-row.png",
    });

    // ── Assertion 4: Retry click -> toast + list refresh -> row gone ─
    // Stage the post-retry snapshot so the auto-refresh that
    // follows `kg_requeue_failed` reads an empty flagged set.
    await page.evaluate((snap) => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
        kg_dashboard_snapshot: snap,
      };
    }, POST_RETRY_SNAPSHOT);
    await retryBtn.click();

    await expect(page.getByText(/filing requeued/i)).toBeVisible();
    await expect(list).toHaveCount(0);
    await expect(page.getByText(/nothing flagged/i)).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-flagged-4-post-retry.png",
    });

    // ── Assertion 5: No console errors across the flow ──────────
    expect(
      consoleErrors,
      `Unexpected console errors during KG flagged-band flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
