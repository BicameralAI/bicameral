/**
 * Browser evidence for bicameral#52 (bot#1346) — the hosted header and the
 * "Local daemon disconnected" dialog must agree, and a stopped daemon must be
 * noticed without a reload.
 *
 * The daemon is stubbed by request interception, as in every spec here: "stop"
 * means the status route refuses the connection, "restart" means the stub
 * answers again. Time is driven by Playwright's clock so one polling interval
 * passes without waiting for it. This is not served-build or real-daemon
 * evidence; the Factory #962 clean-user rerun is.
 */

import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS } from "./support/daemon-stub";
import type { DaemonScenario } from "./support/harness";

const STATUS_ROUTE = "**/api/v1/hosted-bridge/status";
const ONE_POLL_INTERVAL_MS = 6_000;

function scenario(): DaemonScenario {
  return {
    mcp: { state: "not_installed", reasons: ["package_absent"], hostId: "claude" },
    connectionRegistry: [{ status: "not_connected" }],
    memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
    cloudProjects: {
      projects: cloudMemberProject().projects as Record<string, unknown>[],
      selected_project_id: null,
    },
    // The route the liveness poll calls while the daemon is up.
    bridgeStatus: {
      status: 200,
      body: {
        pairing_state: "hosted_bridge_paired",
        origin: "https://app.example.test",
        cloud_account_id: "acct-synthetic",
        paired_at: "2026-01-01T00:00:00Z",
        expires_at: "2099-01-01T00:00:00Z",
      },
    },
  };
}

/** The browser logs the refused connection when the daemon "stops". */
const REFUSED_CONNECTION = /ERR_CONNECTION_REFUSED/i;

test("a stopped daemon is detected without a reload and the header never says Connected beside the dialog", async ({ ui, page }) => {
  await page.clock.install();
  await ui.open({
    id: "app-daemon-stop-detected",
    title: "Stopping the daemon turns the header to not connected and opens the dialog, with no reload",
    daemon: scenario(),
    path: "/app",
  });

  ui.allowConsoleError(REFUSED_CONNECTION);
  const pill = page.getByText("Connected", { exact: true });
  const dialog = page.getByRole("dialog", { name: "Local daemon disconnected" });

  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(pill).toBeVisible();
  await expect(dialog).toHaveCount(0);

  // The daemon stops: nothing else is clicked, nothing is reloaded.
  await page.route(STATUS_ROUTE, (route) => route.abort("connectionrefused"));
  await page.clock.fastForward(ONE_POLL_INTERVAL_MS);

  await expect(dialog).toBeVisible();
  await expect(pill).toHaveCount(0);
  await expect(page.getByText("Not connected", { exact: true })).toBeVisible();

  await ui.capture({
    name: "01-stop-detected-header-and-dialog-agree",
    assertions: [
      "The dialog appeared after the daemon stopped, without a page reload.",
      "The header pill reads Not connected; Connected is not shown beside the dialog.",
    ],
    primaryControls: ['[role="dialog"]'],
  });

  // A reload while the daemon is still down must not bring the contradiction back.
  await page.reload();
  await page.clock.fastForward(ONE_POLL_INTERVAL_MS);
  await expect(dialog).toBeVisible();
  await expect(pill).toHaveCount(0);
});

test("the daemon coming back clears the dialog and the header returns to Connected", async ({ ui, page }) => {
  await page.clock.install();
  await ui.open({
    id: "app-daemon-recovers",
    title: "Restarting the daemon clears the dialog and restores the Connected header, no stale state",
    daemon: scenario(),
    path: "/app",
  });

  ui.allowConsoleError(REFUSED_CONNECTION);
  const pill = page.getByText("Connected", { exact: true });
  const dialog = page.getByRole("dialog", { name: "Local daemon disconnected" });

  await page.route(STATUS_ROUTE, (route) => route.abort("connectionrefused"));
  await page.clock.fastForward(ONE_POLL_INTERVAL_MS);
  await expect(dialog).toBeVisible();

  // The daemon restarts: the stub answers the status route again.
  await page.unroute(STATUS_ROUTE);
  await page.clock.fastForward(ONE_POLL_INTERVAL_MS);

  await expect(dialog).toHaveCount(0);
  await expect(pill).toBeVisible();

  await ui.capture({
    name: "01-recovered-no-stale-state",
    assertions: [
      "After the daemon answers again the dialog is gone.",
      "The header reads Connected and the page is interactive again.",
    ],
    primaryControls: ['[data-testid="paired-shell"]'],
  });
});
