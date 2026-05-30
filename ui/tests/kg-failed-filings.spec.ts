// KG Phase 1C Wave 1C.2 visual sweep (bead `mb-9ufg`, ADR 0051).
//
// Shape mirrors `ui/tests/kg-settings-tab.spec.ts` (Wave 1C.1):
// runs against the Vite preview server using the Tauri-shim
// fallback so we exercise every state without spinning up Rust.
// Per-command fixture overrides via `window.__MOCKINGBIRD_FIXTURES__`
// stage the seeded-failed-row + post-retry-empty states.
//
// Single judge: all 5 assertions below must pass.
//
//  1. Toggle OFF → the Filing-status card is NOT in the DOM (parent
//     SettingsKgTab gates the child on `kgGraphEnabled === true`,
//     so no DB query, no UI plumbing visible without opt-in).
//  2. Toggle ON + empty fixture → status line shows "Pending: 0 …
//     Last filed: never" and the failed-list renders the empty-
//     state copy ("No failed filings — all good!").
//  3. Toggle ON + seeded fixture (one failed row) → row renders with
//     "Dictation #1337", "3 attempts" pill, truncated lastError
//     (<=200ch shown) + full text exposed via `title=`, and the
//     Retry button is reachable by its ARIA label.
//  4. Click Retry → "Filing requeued" toast appears, the list
//     re-fetches (now empty per the pre-staged fixture), and the
//     status-line "Failed: N" count drops to 0.
//  5. No console errors across the entire flow.

import { expect, test, type ConsoleMessage } from "@playwright/test";

// Long enough to be visibly truncated past the 200-char limit and
// trip the `title=` tooltip branch — both production-realistic
// (matches the kind of message ollama / qdrant client errors emit).
const SEEDED_ERROR =
  "ollama refused embedding request: model qwen3-embedding-0.6b not " +
  "found locally. download via: ollama pull qwen3-embedding-0.6b. " +
  "this is a deliberately long error message to verify truncation " +
  "past the 200 character limit and tooltip exposure of the full text.";

const SEEDED_ROW = {
  queueId: 42,
  entryId: 1337,
  attemptCount: 3,
  lastError: SEEDED_ERROR,
  enqueuedIso: "2026-05-30T09:30:00Z",
  failedIso: "2026-05-30T09:31:14Z",
};

test.describe("KG Phase 1C.2 — failed-filings UX (mb-9ufg)", () => {
  test("toggle-gating, empty state, seeded row, retry → toast + refresh", async ({
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

    // ── Assertion 1: Toggle OFF → no Filing-status card ─────────
    // Default fixture is `kgGraphEnabled: false`, so the parent
    // SettingsKgTab does NOT mount SettingsKgFailedFilings.
    await page.goto("/#/settings");

    const tabs = page.getByRole("navigation", { name: /settings sections/i });
    await expect(tabs).toBeVisible();
    const kgTab = tabs.getByRole("button", { name: /knowledge graph/i });
    await kgTab.click();

    const toggle = page.getByRole("checkbox", {
      name: /enable knowledge graph indexing/i,
    });
    await expect(toggle).not.toBeChecked();
    // The Filing-status card is anchored on its "Filing status"
    // <Card title=...>; absent when the toggle is OFF.
    await expect(
      page.getByRole("heading", { name: /filing status/i }),
    ).toHaveCount(0);
    await page.screenshot({
      path: "test-results/kg-failed-1-toggle-off.png",
    });

    // ── Assertion 2: Toggle ON + empty fixture → empty state ────
    // Stage the empty-queue fixtures BEFORE flipping the toggle so
    // the on-mount fetch in SettingsKgFailedFilings reads the right
    // shape on first paint (no flash of stale data).
    await page.evaluate(() => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        kg_queue_status: {
          pending: 0,
          processing: 0,
          failed: 0,
          lastDoneIso: null,
        },
        kg_list_failed_filings: [],
      };
    });
    await toggle.check();
    await expect(toggle).toBeChecked();

    await expect(
      page.getByRole("heading", { name: /filing status/i }),
    ).toBeVisible();
    await expect(page.getByText(/pending:\s*0/i)).toBeVisible();
    await expect(page.getByText(/last filed:\s*never/i)).toBeVisible();
    await expect(page.getByText(/no failed filings/i)).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-failed-2-on-empty.png",
    });

    // ── Assertion 3: Seeded row → entry + pill + truncation + retry ─
    // Mutate the fixture map to surface the seeded row, then bounce
    // the toggle off→on. The child's mount-driven `useEffect`
    // refreshes the queue on every remount, which gets us the new
    // fixture without depending on a manual refresh button.
    await page.evaluate((row) => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        kg_queue_status: {
          pending: 3,
          processing: 1,
          failed: 1,
          lastDoneIso: "2026-05-30T10:15:23Z",
        },
        kg_list_failed_filings: [row],
      };
    }, SEEDED_ROW);
    await toggle.uncheck();
    await toggle.check();

    const list = page.getByRole("list", { name: /failed filings/i });
    await expect(list).toBeVisible();
    await expect(list.getByText(/dictation #1337/i)).toBeVisible();
    await expect(list.getByText(/3 attempts/i)).toBeVisible();
    await expect(page.getByText(/pending:\s*3/i)).toBeVisible();
    await expect(page.getByText(/failed:\s*1/i)).toBeVisible();

    // Truncation contract: rendered text is at most ~ERROR_TRUNCATE
    // chars (the production constant is 200; allow a tiny ellipsis-
    // slack budget). The full text is exposed via `title=` so screen
    // readers / hovering users see the whole thing.
    const errorCell = list.locator("div[title]").first();
    await expect(errorCell).toHaveAttribute("title", SEEDED_ERROR);
    const visibleText = (await errorCell.innerText()).trim();
    expect(visibleText.length).toBeLessThanOrEqual(220);
    expect(SEEDED_ERROR.length).toBeGreaterThan(220); // sanity: the
    // test fixture is genuinely long enough to be truncated.

    const retryBtn = page.getByRole("button", {
      name: /retry filing for dictation #1337/i,
    });
    await expect(retryBtn).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-failed-3-seeded-row.png",
    });

    // ── Assertion 4: Retry click → toast + list refresh → row gone ─
    // Stage the post-retry state BEFORE the click so the auto-
    // refresh that follows `kg_requeue_failed` sees an empty list
    // and dropped failed-count.
    await page.evaluate(() => {
      window.__MOCKINGBIRD_FIXTURES__ = {
        ...(window.__MOCKINGBIRD_FIXTURES__ ?? {}),
        kg_queue_status: {
          pending: 4,
          processing: 1,
          failed: 0,
          lastDoneIso: "2026-05-30T10:15:23Z",
        },
        kg_list_failed_filings: [],
      };
    });
    await retryBtn.click();

    await expect(page.getByText(/filing requeued/i)).toBeVisible();
    await expect(list).toHaveCount(0);
    await expect(page.getByText(/no failed filings/i)).toBeVisible();
    await expect(page.getByText(/failed:\s*0/i)).toBeVisible();
    await page.screenshot({
      path: "test-results/kg-failed-4-post-retry.png",
    });

    // ── Assertion 5: No console errors across the flow ──────────
    expect(
      consoleErrors,
      `Unexpected console errors during KG failed-filings flow:\n${consoleErrors.join("\n")}`,
    ).toEqual([]);
  });
});
