/**
 * Harness self-proof.
 *
 * This spec exists so the evidence machinery itself is covered by a check that
 * fails when the harness stops exercising a real browser against the real
 * bundle. It deliberately asserts only what is true of the hosted SPA on every
 * branch (shell chrome renders, no page/console errors, no unmodelled API
 * traffic, both viewports usable). Behaviour claims belong in the per-surface
 * evidence specs that cite them.
 */

import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS } from "./support/daemon-stub";
import type { DaemonScenario } from "./support/harness";

const PAIRED: DaemonScenario = {
  mcp: { state: "active_verified", reasons: [] },
  connectionRegistry: [{ status: "not_connected" }],
  memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
  cloudProjects: cloudMemberProject(),
};

test("hosted app shell renders under a valid handoff", async ({ ui, page }) => {
  await ui.open({
    id: "app-shell",
    title: "Hosted SPA shell renders in a real browser under a valid hosted handoff",
    daemon: PAIRED,
    path: "/app",
  });

  await expect(page.getByTestId("app-header")).toBeVisible();
  await expect(page.getByTestId("hosted-dashboard")).toBeVisible();

  await ui.capture({
    name: "01-shell",
    assertions: [
      "The built SPA bundle loads and mounts in a real Chromium browser.",
      "Global hosted chrome (header + hosted dashboard root) is rendered.",
      "No uncaught page error, console error, or unmodelled API call occurred.",
    ],
    primaryControls: ['[data-testid="app-header"]', '[data-testid="hosted-dashboard"]'],
  });
});
