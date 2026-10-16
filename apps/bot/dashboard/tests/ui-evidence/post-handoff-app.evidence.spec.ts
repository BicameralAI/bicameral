/**
 * Headed evidence for #1092 / Settings host IA — Bot `/app` is post-handoff only.
 *
 * Products home is never an MCP curriculum. MCP readiness lives in the HOST
 * footer strip and Settings → Agent host. Pair bootstrap is Cloud `/connect`
 * only; unpaired `/app` leaves without resurrecting install/gateway/pair rails.
 *
 * These scenarios prove that in a real browser at both required viewports.
 */

import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS, type McpState } from "./support/daemon-stub";
import type { DaemonScenario } from "./support/harness";

/** Copy that only ever belonged to the retired Bot-owned pre-pair lifecycle. */
const RETIRED_PRE_PAIR_COPY = [
  "Start local gateway",
  "Install or update Bicameral",
  "Pair this browser",
  "Install MCP",
  "Continue to Products",
  "Connect Bicameral on this device",
  "Step 1 of 4",
  "Step 4 of 4",
];

function pairedScenario(mcp: McpState): DaemonScenario {
  return {
    mcp: {
      state: mcp,
      reasons: mcp === "not_installed" ? ["package_absent"] : [],
      hostId: "claude",
    },
    connectionRegistry: [{ status: "not_connected" }],
    memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
    cloudProjects: cloudMemberProject(),
  };
}

const PRODUCTS_CONTROLS = [
  '[data-testid="paired-shell"]',
  '[data-testid="host-status-mcp"]',
];

/**
 * Scenario 1 — valid handoff, MCP not installed.
 *
 * Operator lands in Products/admin. MCP is a footer host-status chip only —
 * no permanent home essay, no Product-entry gate.
 */
test("paired handoff with MCP not installed still reaches Products", async ({ ui, page }) => {
  await ui.open({
    id: "app-mcp-not-installed",
    title: "Valid hosted handoff with MCP not installed reaches Products/admin; readiness is host-status only, not a gate",
    daemon: pairedScenario("not_installed"),
    path: "/app",
  });

  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(page.getByTestId("host-status-mcp")).toBeVisible();
  await expect(page.getByTestId("host-status-mcp")).toHaveAttribute(
    "data-mcp-state",
    "not_installed",
  );
  await expect(page.getByTestId("host-status-mcp")).toContainText(/MCP not set up/i);
  // No permanent MCP essay on Products home.
  await expect(page.getByTestId("paired-mcp-readiness")).toHaveCount(0);
  await expect(page.getByTestId("mcp-readiness-home")).toHaveCount(0);

  // No Product-entry gate anywhere on the surface.
  await expect(page.getByTestId("mcp-continue-products")).toHaveCount(0);
  for (const copy of RETIRED_PRE_PAIR_COPY) {
    await expect(page.getByText(copy, { exact: true })).toHaveCount(0);
  }

  await ui.capture({
    name: "01-products-reachable",
    assertions: [
      "A valid hosted handoff opens the paired Products/admin shell directly.",
      "MCP readiness is a HOST footer status chip (not_installed), not a home essay.",
      "No Continue-to-Products control exists, so MCP cannot gate Product entry.",
      "None of the retired Bot pre-pair lifecycle copy is rendered.",
    ],
    primaryControls: PRODUCTS_CONTROLS,
  });
});

/**
 * Scenario 2 — valid handoff, `installed_pending_host_action`.
 *
 * Footer stays truthful; detail opens under Settings → Agent host.
 */
test("paired handoff with MCP pending host action keeps admin access and truthful readiness", async ({ ui, page }) => {
  await ui.open({
    id: "app-mcp-pending-host-action",
    title: "Valid hosted handoff with MCP installed_pending_host_action keeps Products/admin; strip is truthful; Settings holds detail",
    daemon: pairedScenario("installed_pending_host_action"),
    path: "/app",
  });

  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(page.getByTestId("host-status-mcp")).toContainText(/MCP needs host/i);
  await expect(page.getByTestId("host-status-mcp")).toHaveAttribute(
    "data-mcp-state",
    "installed_pending_host_action",
  );
  await expect(page.getByText("Active · verified")).toHaveCount(0);
  await expect(page.getByTestId("mcp-continue-products")).toHaveCount(0);
  await expect(page.getByTestId("mcp-readiness-home")).toHaveCount(0);

  // Detail lives in Settings → Agent host, not on Products home.
  await page.getByTestId("host-status-mcp").click();
  await expect(page.getByTestId("hosted-settings")).toBeVisible();
  await expect(page.getByTestId("settings-agent-host")).toBeVisible();
  await expect(page.getByTestId("settings-mcp-card")).toBeVisible();
  await expect(page.getByText("Pending host action")).toBeVisible();

  await ui.capture({
    name: "01-pending-host-action",
    assertions: [
      "Products/admin remains reachable while MCP awaits host activation.",
      "HOST footer reports pending host action; detail opens in Settings → Agent host.",
      "No Product-entry gate appears in this state.",
    ],
    primaryControls: [
      '[data-testid="hosted-settings"]',
      '[data-testid="settings-mcp-card"]',
    ],
  });
});

/**
 * Scenario 3 — valid handoff, `active_verified`.
 */
test("paired handoff with verified MCP reports verification and adds no gate", async ({ ui, page }) => {
  await ui.open({
    id: "app-mcp-active-verified",
    title: "Valid hosted handoff with MCP active_verified reports ready in HOST footer and adds no Product-entry gate",
    daemon: pairedScenario("active_verified"),
    path: "/app",
  });

  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(page.getByTestId("host-status-mcp")).toContainText(/MCP ready/i);
  await expect(page.getByTestId("host-status-mcp")).toHaveAttribute(
    "data-mcp-state",
    "active_verified",
  );
  await expect(page.getByTestId("mcp-continue-products")).toHaveCount(0);
  await expect(page.getByTestId("mcp-readiness-home")).toHaveCount(0);
  for (const copy of RETIRED_PRE_PAIR_COPY) {
    await expect(page.getByText(copy, { exact: true })).toHaveCount(0);
  }

  await ui.capture({
    name: "01-active-verified",
    assertions: [
      "HOST footer reports the daemon's verified activation state as MCP ready.",
      "Verification adds no additional Product-entry gate.",
      "Products/admin is reachable on the same surface.",
    ],
    primaryControls: PRODUCTS_CONTROLS,
  });
});

/**
 * Scenario 4 — direct/unpaired `/app` entry.
 *
 * Bot must hand the operator back to Cloud `/connect` rather than resurrecting
 * its own pre-pair lifecycle. In this harness the preview server serves the SPA
 * for any path; production Cloud serves `/connect` HTML. Evidence shows Bot
 * leaves `/app` for `/connect` with no install/gateway/pair/MCP curriculum.
 */
test("direct unpaired /app entry returns to the Cloud pairing surface", async ({ ui, page }) => {
  await ui.open({
    id: "app-unpaired-direct-entry",
    title: "Direct/unpaired /app entry leaves Bot for Cloud /connect without resurrecting the Bot pre-pair lifecycle",
    daemon: { mcp: { state: "not_installed" } },
    path: "/app",
    handoff: false,
  });

  await page.waitForURL("**/connect", { timeout: 15_000 });
  expect(new URL(page.url()).pathname).toBe("/connect");

  await expect(page.getByTestId("hosted-handoff-leaving")).toBeVisible();
  await expect(page.getByTestId("paired-shell")).toHaveCount(0);
  await expect(page.getByTestId("hosted-pairing-screen")).toHaveCount(0);

  // The retired lifecycle must not come back on the unpaired path either.
  for (const copy of ["Start local gateway", "Install MCP", "Step 1 of 4", "Step 4 of 4"]) {
    await expect(page.getByText(copy, { exact: true })).toHaveCount(0);
  }

  await ui.capture({
    name: "01-returned-to-connect",
    assertions: [
      "Unpaired /app entry does not render Products/admin.",
      "The browser is navigated off /app to the canonical /connect pairing surface.",
      "Bot's retired install / gateway / pair / MCP pre-pair lifecycle is not rendered.",
    ],
    primaryControls: ['[data-testid="hosted-handoff-leaving"]'],
  });
});
