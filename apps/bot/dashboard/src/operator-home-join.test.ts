/**
 * Operator Home per-Product join UI tests (bot#768).
 *
 * Renders OperatorHome against the domain-mix fixture and asserts the
 * user-facing acceptance criteria:
 *   - members appear under `Your products` with Open/Continue setup/Repair;
 *   - available non-members appear under `Available for <domain>` with a
 *     row-level, keyboard-accessible Join;
 *   - joining disables the row action, shows `Joining…`, and never moves the
 *     row into `Your products`;
 *   - typed failures render concise recovery copy with a safe retry for
 *     retryable codes and none for non-retryable ones;
 *   - no global/free-form Join or invite/team-id control exists.
 */

import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup, fireEvent } from "@testing-library/svelte";
import OperatorHome from "./pages/OperatorHome.svelte";
import OperatorHomeSource from "./pages/OperatorHome.svelte?raw";
import { FIXTURE_PROJECTS_DOMAIN_MIX } from "./lib/operator-home-fixtures";
import { failureView } from "./lib/product-join";

afterEach(() => cleanup());

describe("OperatorHome hosted shell and ledger semantics", () => {
  it("composes the Product ledger beneath the global hosted header", () => {
    const { getByTestId, getByRole, queryByTestId } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX },
    });

    // App.svelte owns the single global header and daemon-disconnect overlay.
    // OperatorHome must not render a second hosted header underneath it.
    expect(queryByTestId("app-header")).toBeNull();
    expect(getByRole("navigation", { name: "Workspace" })).toBeTruthy();
    expect(getByTestId("workspace-topbar")).toBeTruthy();
    expect(getByTestId("product-ledger-main").tagName).toBe("MAIN");
    // Title lives in thin topbar only (no page-level H1 essay).
    expect(getByTestId("workspace-topbar").textContent).toMatch(/Products/);
  });

  it("uses accessible tables with the live design's truthful columns", () => {
    const { getByRole, getAllByRole } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX },
    });

    expect(getByRole("table", { name: "Your products" })).toBeTruthy();
    expect(getByRole("table", { name: /Available for acme\.example/ })).toBeTruthy();
    const headers = getAllByRole("columnheader").map((header) =>
      header.textContent?.trim(),
    );
    expect(headers).toEqual([
      "Product",
      "Event store",
      "Status",
      "Updated",
      "",
      "Product",
      "Event store",
      "Status",
      "Updated",
      "",
    ]);
  });

  it("keeps the live narrow shell contract in component CSS", () => {
    expect(OperatorHomeSource).toContain("@media (max-width: 720px)");
    expect(OperatorHomeSource).toContain(".navigation-rail");
    expect(OperatorHomeSource).toContain("display: none");
    expect(OperatorHomeSource).toContain("data-label");
  });
});

describe("OperatorHome grouping and member actions", () => {
  it("renders the live Product-ledger hierarchy without fabricating recency", () => {
    const { getByRole, getByTestId, queryByText } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX },
    });

    // Shell topbar title only — no OPERATOR HOME / page blurb.
    expect(getByTestId("workspace-topbar").textContent).toMatch(/Products/);
    expect(queryByText(/OPERATOR HOME/i)).toBeNull();
    expect(getByRole("button", { name: "Create product" })).toBeTruthy();

    // The live projection has no authoritative updated-at field. Keep the
    // Figma hierarchy while refusing to invent the example timestamps.
    expect(queryByText(/\d+[mh] ago/)).toBeNull();
  });

  it("groups members under Your products and available under the domain heading", () => {
    const { container, getByTestId } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX },
    });
    expect(getByTestId("your-products-section")).toBeTruthy();
    expect(getByTestId("available-heading").textContent).toContain(
      "Available for acme.example",
    );
    const memberRows = container.querySelectorAll('[data-membership="member"]');
    const availableRows = container.querySelectorAll(
      '[data-membership="available"]',
    );
    expect(memberRows.length).toBe(3);
    expect(availableRows.length).toBe(2);
  });

  it("derives Open / Continue setup / Repair for member rows", () => {
    const { container } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX },
    });
    const actions = [...container.querySelectorAll('[data-membership="member"]')].map(
      (row) => row.querySelector("[data-action]")?.getAttribute("data-action"),
    );
    expect(actions).toEqual(["open", "continue_setup", "repair"]);
  });

  it("exposes a keyboard-accessible Join per available row and no global Join", () => {
    const { container } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX },
    });
    const joins = container.querySelectorAll('[data-testid="join-action"]');
    expect(joins.length).toBe(2);
    for (const btn of joins) {
      expect(btn.tagName).toBe("BUTTON");
      expect(btn.getAttribute("aria-label")).toMatch(/^Join /);
    }
    // No global/free-form Join or invite/team-id control.
    expect(container.textContent?.toLowerCase() ?? "").not.toContain(
      "join project",
    );
    expect(container.querySelector("input")).toBeNull();
  });
});

describe("OperatorHome join in-flight state", () => {
  it("disables the row action and shows Joining… without moving the row", () => {
    const target = FIXTURE_PROJECTS_DOMAIN_MIX.find(
      (p) => p.membership === "available",
    )!;
    const { container } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX, joiningId: target.id },
    });
    const row = [
      ...container.querySelectorAll('[data-membership="available"]'),
    ].find((r) => r.textContent?.includes(target.name))!;
    const btn = row.querySelector<HTMLButtonElement>(
      '[data-testid="join-action"]',
    )!;
    expect(btn.disabled).toBe(true);
    expect(btn.getAttribute("aria-busy")).toBe("true");
    expect(btn.textContent?.trim()).toBe("Joining…");
    // The joining row is still under Available, not Your products.
    expect(container.querySelectorAll('[data-membership="member"]').length).toBe(
      3,
    );
  });

  it("fires onjoin with the project when an idle Join is clicked", async () => {
    const target = FIXTURE_PROJECTS_DOMAIN_MIX.find(
      (p) => p.membership === "available",
    )!;
    const onjoin = vi.fn();
    const { container } = render(OperatorHome, {
      props: { projects: FIXTURE_PROJECTS_DOMAIN_MIX, onjoin },
    });
    const row = [
      ...container.querySelectorAll('[data-membership="available"]'),
    ].find((r) => r.textContent?.includes(target.name))!;
    const btn = row.querySelector<HTMLButtonElement>(
      '[data-testid="join-action"]',
    )!;
    expect(btn.disabled).toBe(false);
    await fireEvent.click(btn);
    expect(onjoin).toHaveBeenCalledWith(target);
  });
});

describe("OperatorHome join failure state", () => {
  it("renders concise recovery copy and a retry for retryable failures", () => {
    const target = FIXTURE_PROJECTS_DOMAIN_MIX.find(
      (p) => p.membership === "available",
    )!;
    const { container } = render(OperatorHome, {
      props: {
        projects: FIXTURE_PROJECTS_DOMAIN_MIX,
        joinFailures: { [target.id]: failureView("expired_ticket") },
      },
    });
    const row = [
      ...container.querySelectorAll('[data-membership="available"]'),
    ].find((r) => r.textContent?.includes(target.name))!;
    expect(
      row.querySelector('[data-testid="join-failure"]')?.textContent,
    ).toContain("expired");
    expect(row.querySelector('[data-testid="join-retry"]')).not.toBeNull();
    // A failed row never claims membership.
    expect(row.getAttribute("data-membership")).toBe("available");
  });

  it("offers no retry for a non-retryable failure", () => {
    const target = FIXTURE_PROJECTS_DOMAIN_MIX.find(
      (p) => p.membership === "available",
    )!;
    const { container } = render(OperatorHome, {
      props: {
        projects: FIXTURE_PROJECTS_DOMAIN_MIX,
        joinFailures: { [target.id]: failureView("wrong_domain") },
      },
    });
    const row = [
      ...container.querySelectorAll('[data-membership="available"]'),
    ].find((r) => r.textContent?.includes(target.name))!;
    expect(row.querySelector('[data-testid="join-retry"]')).toBeNull();
    expect(row.querySelector('[data-testid="join-action"]')).toBeNull();
    expect(
      row.querySelector('[data-testid="join-failure"]')?.textContent,
    ).toContain("isn’t open to your email domain");
  });
});
