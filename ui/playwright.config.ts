import { defineConfig, devices } from "@playwright/test";

// Mockingbird Playwright config.
//
// We run Playwright against the Vite preview server (no Tauri shell
// in the test path). The UI's Tauri shim auto-falls back to
// fixtures when `__TAURI_INTERNALS__` is absent — so component-level
// behavior + visual baselines are testable without a Rust process.
//
// App-level (real Tauri) tests are deferred — Tauri 2 WebDriver is
// still experimental on Windows. The fixtures we expose via
// `window.__MOCKINGBIRD_FIXTURES__` cover enough surface for
// component-level coverage.

export default defineConfig({
  testDir: "./tests",
  testMatch: /.*\.spec\.(ts|tsx)$/,
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: [["list"], ["html", { outputFolder: "playwright-report", open: "never" }]],
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    colorScheme: "dark",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
  ],
  webServer: {
    command: "npm run build && npm run preview",
    port: 4173,
    timeout: 120_000,
    reuseExistingServer: !process.env.CI,
  },
});
