import type { Page } from "@playwright/test";
import { test, expect } from "./support/harness";
import { cloudMemberProject, HARNESS } from "./support/daemon-stub";
import type { DaemonScenario } from "./support/harness";

const PAIRED: DaemonScenario = {
  mcp: { state: "active_verified", reasons: [] },
  connectionRegistry: [{ status: "not_connected" }],
  memberships: [{ product_id: HARNESS.productId, setup_incomplete: false }],
  cloudProjects: cloudMemberProject(),
};

type Box = { x: number; y: number; width: number; height: number };

function overlaps(a: Box, b: Box): boolean {
  return !(
    a.x + a.width <= b.x ||
    b.x + b.width <= a.x ||
    a.y + a.height <= b.y ||
    b.y + b.height <= a.y
  );
}

async function expectNarrowNavHealthy(
  page: Page,
  inactiveTestId: "nav-products" | "nav-settings",
) {
  const inactive = page.getByTestId(inactiveTestId);
  const inactiveIcon = inactive.locator("span").nth(0);
  const inactiveLabel = inactive.locator("span").nth(1);
  const activeItem = page.locator('.rail-item[aria-current="page"]');
  const activeLabel = activeItem.locator("span").nth(1);
  const heading = page.locator(".page-heading");

  await expect(inactive).toBeVisible();
  await expect(inactiveLabel).toBeVisible();
  await expect(activeLabel).toBeVisible();
  await expect(page.getByTestId("nav-collapse")).toBeHidden();

  expect(
    await inactiveLabel.evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);

  const [iconBox, inactiveLabelBox, activeLabelBox, headingBox] = await Promise.all([
    inactiveIcon.boundingBox(),
    inactiveLabel.boundingBox(),
    activeLabel.boundingBox(),
    heading.boundingBox(),
  ]);
  expect(iconBox).not.toBeNull();
  expect(inactiveLabelBox).not.toBeNull();
  expect(activeLabelBox).not.toBeNull();
  expect(headingBox).not.toBeNull();

  expect(overlaps(iconBox!, inactiveLabelBox!)).toBe(false);
  expect(overlaps(inactiveLabelBox!, activeLabelBox!)).toBe(false);
  expect(overlaps(inactiveLabelBox!, headingBox!)).toBe(false);
  expect(overlaps(activeLabelBox!, headingBox!)).toBe(false);

  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBe(true);
}

test("phone-width Products and Settings navigation remains readable", async ({
  ui,
  page,
}) => {
  test.skip((page.viewportSize()?.width ?? 9999) > 720, "narrow viewport evidence only");

  await ui.open({
    id: "apc1-mobile-nav",
    title: "APC1 narrow Products and Settings navigation stays readable",
    daemon: PAIRED,
    path: "/app",
  });

  await expect(page.locator(".page-heading")).toHaveText("Products");
  await expectNarrowNavHealthy(page, "nav-settings");

  await page.getByTestId("nav-settings").click();
  await expect(page.locator(".page-heading")).toHaveText("Settings");
  await expectNarrowNavHealthy(page, "nav-products");

  await page.getByTestId("nav-products").click();
  await expect(page.locator(".page-heading")).toHaveText("Products");
  await expectNarrowNavHealthy(page, "nav-settings");

  await ui.capture({
    name: "01-mobile-products-settings-nav",
    assertions: [
      "Inactive navigation labels do not overlap their icons.",
      "Products and Settings labels do not overlap each other or the page heading.",
      "The desktop rail-collapse control is hidden at phone width.",
      "The page has no horizontal overflow.",
    ],
    primaryControls: [
      '[data-testid="nav-settings"]',
      '[data-testid="workspace-topbar"]',
    ],
  });
});
