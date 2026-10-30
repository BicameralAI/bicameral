/**
 * Browser evidence for bot#1347 — hosted shell version and update guidance.
 *
 * The hosted header must not present a version the SPA cannot verify, and the
 * "update the local daemon" guidance must appear only for Cloud's evaluated
 * `upgrade_required` verdict, never for a discoverable row whose compatibility
 * Cloud has not evaluated (`daemon_compatible: false` with no paired daemon).
 */

import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS } from "./support/daemon-stub";
import type { DaemonScenario } from "./support/harness";

const UPDATE_COPY = /update the local daemon/i;

function scenario(projects: Record<string, unknown>[]): DaemonScenario {
  return {
    mcp: { state: "not_installed", reasons: ["package_absent"], hostId: "claude" },
    connectionRegistry: [{ status: "not_connected" }],
    memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
    cloudProjects: { projects, selected_project_id: null },
  };
}

const memberRow = (cloudMemberProject().projects as Record<string, unknown>[])[0];

test("hosted header shows no unverifiable version and discoverable rows carry no update prompt", async ({ ui, page }) => {
  await ui.open({
    id: "app-shell-version-truthful",
    title: "Hosted header has no version pill; a discoverable row with unknown compatibility has no update-the-daemon prompt",
    daemon: scenario([
      memberRow,
      {
        project_id: "prd_discoverable_01",
        team_id: HARNESS.teamId,
        display_name: "Discoverable Product",
        pairing_status: "unpaired",
        binding_readiness: "unbound",
        daemon_compatible: false,
        is_member: false,
      },
    ]),
    path: "/app",
  });

  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(page.getByTestId("app-shell-version")).toHaveCount(0);
  await expect(page.getByText(/\bv?0\.1\.25\b/)).toHaveCount(0);
  await expect(page.getByText("Discoverable Product")).toBeVisible();
  await expect(page.getByText(UPDATE_COPY)).toHaveCount(0);

  await ui.capture({
    name: "01-no-version-pill-no-update-prompt",
    assertions: [
      "The hosted header renders no version label.",
      "A discoverable Product row with unevaluated compatibility shows no update-the-daemon text.",
    ],
    primaryControls: ['[data-testid="paired-shell"]'],
  });

  // The removed label must not come back after a reload (bicameral#53).
  await page.reload();
  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(page.getByTestId("app-shell-version")).toHaveCount(0);
  await expect(page.getByText(/\bv?0\.1\.25\b/)).toHaveCount(0);
});

test("an evaluated upgrade_required verdict still tells the operator to update the daemon", async ({ ui, page }) => {
  await ui.open({
    id: "app-update-guidance-upgrade-required",
    title: "A Cloud upgrade_required verdict keeps the update-the-daemon guidance",
    daemon: scenario([
      { ...memberRow, binding_readiness: "upgrade_required", daemon_compatible: false },
    ]),
    path: "/app",
  });

  await expect(page.getByTestId("paired-shell")).toBeVisible();
  await expect(page.getByTestId("app-shell-version")).toHaveCount(0);
  await expect(page.getByText(UPDATE_COPY)).toBeVisible();

  await ui.capture({
    name: "01-upgrade-required-keeps-guidance",
    assertions: [
      "Cloud's upgrade_required verdict keeps the update-the-daemon guidance.",
      "No version pill is rendered.",
    ],
    primaryControls: ['[data-testid="paired-shell"]'],
  });
});
