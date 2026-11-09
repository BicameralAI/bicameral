/**
 * Headed evidence for #54 - "Retry sharing" stays usable after auto-resume.
 *
 * A reload with a browser-local pending create (phase `canonical_created`)
 * makes `/app` reopen the registration view and retry the publish once on its
 * own. That view never shows the create form and never runs a folder pick, so
 * the retry button must not depend on either. The narrow project runs at
 * 375px, the width #54 names.
 */

import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS } from "./support/daemon-stub";

/** Unsigned token with a synthetic email; only its domain is read as routing metadata. */
function syntheticCloudToken(): string {
  const encode = (value: object) =>
    btoa(JSON.stringify(value)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `${encode({ alg: "none" })}.${encode({ email: "operator@evidence.example" })}.not-a-signature`;
}

const PENDING_KEY = `bicameral_hosted_product_create_v1:${encodeURIComponent(HARNESS.accountId)}`;
const COMMAND_PATH = "/api/v1/hosted-bridge/command";
const PUBLISH_DENIED = {
  status: 200,
  body: {
    status: "error",
    message: "Publication metadata is not available yet.",
    request_id: "req_evidence_publish",
  },
};

test("retry sharing stays clickable and runs a new retry after the automatic retry fails", async ({
  ui,
  page,
}, testInfo) => {
  if (testInfo.project.name.startsWith("narrow")) {
    await page.setViewportSize({ width: 375, height: 812 });
  }

  await page.addInitScript(
    ({ key, value }: { key: string; value: string }) => {
      window.localStorage.setItem(key, value);
    },
    {
      key: PENDING_KEY,
      value: JSON.stringify({
        version: 1,
        cloudAccountId: HARNESS.accountId,
        displayName: "Checkout Service",
        verifiedDomain: "evidence.example",
        createIdempotencyKey: "create_evidence",
        publishIdempotencyKey: "publish_evidence",
        phase: "canonical_created",
        productId: HARNESS.productId,
        startedAt: "2026-01-01T00:00:00Z",
      }),
    },
  );

  await ui.open({
    id: "retry-sharing-after-auto-resume",
    title: "After the automatic publish retry fails, Retry sharing is enabled and starts a new retry",
    daemon: {
      mcp: { state: "active_verified" },
      connectionRegistry: [{ status: "connected", account_label: "Evidence Drive" }],
      memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
      cloudProjects: cloudMemberProject(),
      extra: { [COMMAND_PATH]: [PUBLISH_DENIED] },
    },
    handoffOverrides: { cloud_access_token: syntheticCloudToken() },
    path: "/app",
  });

  const retry = page.getByTestId("registration-retry");
  await expect(page.getByTestId("registration-retry-needed")).toBeVisible();
  await expect(retry).toHaveText("Retry sharing");
  await expect(retry).toBeEnabled();
  expect(ui.stub.countMatching(COMMAND_PATH)).toBe(1);

  await ui.capture({
    name: "01-after-auto-retry",
    assertions: [
      "The automatic publish retry ran once and failed.",
      "Retry sharing is enabled although no create form or folder pick ran in this session.",
    ],
    primaryControls: ['[data-testid="registration-retry"]', '[data-testid="registration-cancel"]'],
  });

  await retry.click();
  await expect.poll(() => ui.stub.countMatching(COMMAND_PATH)).toBe(2);
  await expect(retry).toBeEnabled();

  await ui.capture({
    name: "02-manual-retry",
    assertions: [
      "Clicking Retry sharing sent a second publish command.",
      "After the second failure, Retry sharing is enabled again.",
    ],
    primaryControls: ['[data-testid="registration-retry"]', '[data-testid="registration-cancel"]'],
  });
});
