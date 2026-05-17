import { expect, test } from "@playwright/test";

test.describe("Wave A — app shell smoke", () => {
  test("main window renders sidebar nav with all six entries", async ({ page }) => {
    await page.goto("/");
    // Hash-router lands on /insights by default.
    await expect(page).toHaveURL(/#\/insights$/);

    const nav = page.getByRole("navigation", { name: /primary/i });
    await expect(nav).toBeVisible();

    for (const label of [
      "Insights",
      "History",
      "Dictionary",
      "Modes",
      "Settings",
      "About",
    ]) {
      await expect(nav.getByRole("link", { name: label })).toBeVisible();
    }
  });

  test("brand label is present", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Mockingbird", { exact: true })).toBeVisible();
  });

  test("navigating to About renders the tagline", async ({ page }) => {
    await page.goto("/#/about");
    await expect(page.getByRole("heading", { name: /About Mockingbird/i })).toBeVisible();
    await expect(page.getByText(/Local-first, system-wide voice dictation/i)).toBeVisible();
  });

  test("recording overlay HTML renders Wave-B placeholder", async ({ page }) => {
    await page.goto("/recording.html");
    await expect(page.getByText(/Wave B placeholder/)).toBeVisible();
  });

  test("design tokens are loaded (sidebar has expected bg)", async ({ page }) => {
    await page.goto("/");
    const sidebar = page.getByRole("navigation", { name: /primary/i });
    const bg = await sidebar.evaluate((el) =>
      getComputedStyle(el.parentElement!).getPropertyValue("background-color"),
    );
    // OKLCH gets serialized as oklch(...) or rgb(...) depending on
    // browser. Either way it should be non-empty + not default white.
    expect(bg).not.toEqual("");
    expect(bg).not.toEqual("rgb(255, 255, 255)");
  });
});
