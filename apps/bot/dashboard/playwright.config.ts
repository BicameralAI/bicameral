import { defineConfig, devices } from "@playwright/test";

/**
 * Headed UI evidence harness (Bot-hosted, reusable).
 *
 * This config drives the REAL built Svelte SPA in a real Chromium browser
 * against a `vite preview` server, so a passing run is browser evidence rather
 * than a DOM-only component render. Deterministic daemon/Cloud responses are
 * injected purely as Playwright request interception inside the harness; no
 * fixture in this tree is reachable from, or authoritative for, production.
 *
 * Viewports are fixed by the evidence contract: desktop 1440x900 and narrow
 * 390x844. Every scenario runs at both.
 */

const PORT = Number(process.env.UI_EVIDENCE_PORT ?? 4180);
const BASE_URL = `http://127.0.0.1:${PORT}`;

/** `1` runs a visible browser window (CI uses xvfb); otherwise headless Chromium. */
export const HEADED = process.env.UI_EVIDENCE_HEADED === "1";

export const EVIDENCE_DIR = "evidence/ui";

export default defineConfig({
  testDir: "tests/ui-evidence",
  testMatch: /.*\.evidence\.spec\.ts$/,
  outputDir: `${EVIDENCE_DIR}/_playwright`,
  // Evidence must be reproducible and attributable, so no retries and no
  // parallel interleaving of screenshot capture.
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  globalSetup: "./tests/ui-evidence/global-setup.ts",
  globalTeardown: "./tests/ui-evidence/global-teardown.ts",
  reporter: [
    ["list"],
    ["json", { outputFile: `${EVIDENCE_DIR}/report/results.json` }],
  ],
  use: {
    baseURL: BASE_URL,
    headless: !HEADED,
    trace: "on",
    video: "off",
    // Screenshots are captured explicitly on SUCCESS by the harness; the
    // built-in failure screenshot stays on as a debugging aid.
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "desktop-1440x900",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } },
    },
    {
      name: "narrow-390x844",
      use: { ...devices["Desktop Chrome"], viewport: { width: 390, height: 844 } },
    },
  ],
  webServer: {
    command: `npm run build && npm run preview -- --port ${PORT} --strictPort --host 127.0.0.1`,
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
