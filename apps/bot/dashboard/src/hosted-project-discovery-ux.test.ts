/**
 * Hosted domain-gated project discovery UX tests (bicameral-bot#751, #768).
 *
 * Validates the PRD/cloud#95 discovery model in the hosted SPA, updated for the
 * bot#768 per-Product same-domain join slice:
 *  - the Product discovery path never presents invite-code entry;
 *  - the empty state guides the operator to create a Product or wait for
 *    domain/team eligibility (not to enter an invite code);
 *  - the legacy manual invite/join affordance has been fully removed from the
 *    primary hosted path (no legacy link, no team-id/invite form);
 *  - the discovery contract mirror carries the safe domain/team eligibility
 *    field and maps it through to the UI entry;
 *  - eligible projects render, an empty eligible list renders the guidance,
 *    and a create-success refreshes discovery from the Cloud API.
 *
 * Spec ref: issue #751 (historical), issue #768; cloud#95 domain-gated contract.
 */

import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/svelte";
import OperatorHome from "./pages/OperatorHome.svelte";
import HostedProjectRegistration from "./pages/HostedProjectRegistration.svelte";
import OperatorHomeSource from "./pages/OperatorHome.svelte?raw";
import AppSource from "./App.svelte?raw";
import {
  DISCOVERY_EMPTY_TITLE,
  DISCOVERY_EMPTY_DESCRIPTION,
} from "./lib/operator-home-fixtures";
import {
  applyDaemonMembershipProjection,
  mapCloudProjectItem,
} from "./lib/cloud-project-discovery";
import type { CloudProjectListItem } from "./lib/cloud-project-discovery";
import type { ProjectEntry } from "./lib/project-fixtures";

afterEach(() => cleanup());

const ELIGIBLE_PROJECTS: ProjectEntry[] = [
  {
    id: "proj_eligible_1",
    name: "checkout-service",
    event_store_kind: "github",
    event_store_label: "Cloud project",
    readiness: "ready",
    team_id: "team_alpha",
    eligibility: "domain",
  },
  {
    id: "proj_eligible_2",
    name: "payments-api",
    event_store_kind: "github",
    event_store_label: "Cloud project",
    readiness: "ready",
    team_id: "team_alpha",
    eligibility: "team",
  },
];

// ── Domain-gated discovery copy ────────────────────────────────────

describe("domain-gated discovery copy (cloud#95)", () => {
  it("empty-state copy guides create / eligibility and never mentions invite codes", () => {
    const lower = DISCOVERY_EMPTY_DESCRIPTION.toLowerCase();
    expect(lower).not.toContain("invite");
    expect(lower).not.toContain("join code");
    expect(lower).toContain("create");
    // Domain/team eligibility framing, not manual entry.
    expect(lower).toMatch(/team|domain/);
    expect(DISCOVERY_EMPTY_TITLE.toLowerCase()).not.toContain("invite");
  });
});

// ── Empty eligible list → guidance, not invite entry (AC4) ─────────

describe("OperatorHome empty eligible list", () => {
  it("renders create as the primary action and the domain/team guidance", () => {
    const { container } = render(OperatorHome, {
      props: { projects: [], oncreate: () => {} },
    });
    const empty = container.querySelector('[data-testid="empty-state"]');
    expect(empty).not.toBeNull();
    const create = container.querySelector('[data-testid="empty-create"]');
    expect(create).not.toBeNull();
    expect(create?.classList.contains("btn-dark")).toBe(true);
    const description = container.querySelector(
      '[data-testid="empty-description"]',
    );
    expect(description?.textContent ?? "").toBe(DISCOVERY_EMPTY_DESCRIPTION);
  });

  it("no control prompts for an invite code or free-form join", () => {
    const { container } = render(OperatorHome, {
      props: { projects: [], oncreate: () => {} },
    });
    const actions = container.querySelector(".empty-actions");
    expect(actions?.textContent?.toLowerCase() ?? "").not.toContain("invite");
    expect(actions?.textContent?.toLowerCase() ?? "").not.toContain("join");
    // No free-text invite-code input anywhere in the default empty state.
    const empty = container.querySelector('[data-testid="empty-state"]');
    expect(empty?.querySelector("input")).toBeNull();
  });

  it("no longer renders any legacy invite affordance (bot#768)", () => {
    const { container } = render(OperatorHome, {
      props: { projects: [], oncreate: () => {} },
    });
    expect(
      container.querySelector('[data-testid="legacy-invite-link"]'),
    ).toBeNull();
  });
});

// ── Eligible projects render (AC1) ─────────────────────────────────

describe("OperatorHome eligible projects", () => {
  it("renders discovered eligible projects and no empty state", () => {
    const { container } = render(OperatorHome, {
      props: { projects: ELIGIBLE_PROJECTS, oncreate: () => {} },
    });
    expect(container.querySelector('[data-testid="empty-state"]')).toBeNull();
    const text = container.textContent ?? "";
    expect(text).toContain("checkout-service");
    expect(text).toContain("payments-api");
    // No invite-code prompt when eligible projects are present.
    expect(text.toLowerCase()).not.toContain("invite code");
  });
});

// ── Legacy invite-code join form fully removed (AC3, bot#768) ──────

describe("HostedProjectRegistration legacy join removal", () => {
  it("renders only the create form, never a legacy join form", () => {
    const { container } = render(HostedProjectRegistration, {});
    const create = container.querySelector('[data-testid="create-project-form"]');
    expect(create).not.toBeNull();
    expect(create?.getAttribute("data-deprecated")).toBeNull();
    expect(
      container.querySelector('[data-testid="join-project-form"]'),
    ).toBeNull();
    expect(
      container.querySelector('[data-testid="join-team-id"]'),
    ).toBeNull();
    expect(container.textContent?.toLowerCase() ?? "").not.toContain(
      "invite code",
    );
  });

  it("shows a daemon support id for a retryable publication failure", () => {
    const { container } = render(HostedProjectRegistration, {
      props: {
        result: {
          status: "publication_retry_needed",
          phase: "publication_retry_needed",
          message: "The Product exists locally but is not shared yet.",
          canRetry: true,
          supportId: "req_publish_missing",
        },
      },
    });

    expect(
      container.querySelector('[data-testid="registration-support-id"]')
        ?.textContent,
    ).toContain("req_publish_missing");
  });
});

// ── Discovery contract eligibility field (AC5) ─────────────────────

describe("cloud discovery eligibility mapping", () => {
  const base: Omit<CloudProjectListItem, "discovery_eligibility"> = {
    project_id: "proj_x",
    team_id: "team_x",
    display_name: "X",
    pairing_status: "paired",
    binding_readiness: "ready",
    daemon_compatible: true,
  };

  it("maps domain/team/membership eligibility through to the UI entry", () => {
    for (const eligibility of ["domain", "team", "membership"] as const) {
      const entry = mapCloudProjectItem({
        ...base,
        discovery_eligibility: eligibility,
      });
      expect(entry.eligibility).toBe(eligibility);
    }
  });

  it("leaves eligibility undefined when Cloud omits the hint", () => {
    const entry = mapCloudProjectItem({ ...base });
    expect(entry.eligibility).toBeUndefined();
    // Safe-field boundary: no invite/join-code field is introduced.
    expect(JSON.stringify(entry).toLowerCase()).not.toContain("join_code");
    expect(JSON.stringify(entry).toLowerCase()).not.toContain("invite");
  });

  it("defaults to member when Cloud omits is_member (pre-#768 rows)", () => {
    const entry = mapCloudProjectItem({ ...base });
    expect(entry.membership).toBeUndefined();
    expect(entry.preview).toBeUndefined();
  });

  it("maps an is_member=false row to an available join row with its preview", () => {
    const entry = mapCloudProjectItem({
      ...base,
      is_member: false,
      verified_domain: "acme.example",
      discovery_eligibility: "domain",
      preview: {
        preview_id: "prev_x",
        product_id: "proj_x",
        product_display_name: "X",
        verified_domain: "acme.example",
        canonical: false,
        expires_at: "2999-01-01T00:00:00Z",
      },
    });
    expect(entry.membership).toBe("available");
    expect(entry.verified_domain).toBe("acme.example");
    expect(entry.preview?.preview_id).toBe("prev_x");
    // Directory row is non-canonical; never claims membership.
    expect(entry.preview?.canonical).toBe(false);
  });

  it("keeps a member row's preview absent even if Cloud sent one", () => {
    const entry = mapCloudProjectItem({
      ...base,
      is_member: true,
      setup_incomplete: true,
      preview: {
        preview_id: "prev_x",
        product_id: "proj_x",
        product_display_name: "X",
        canonical: false,
        expires_at: "2999-01-01T00:00:00Z",
      },
    });
    expect(entry.membership).toBeUndefined();
    expect(entry.preview).toBeUndefined();
    expect(entry.setup_incomplete).toBe(true);
  });

  it("reconciles an available Cloud row with daemon-canonical membership", () => {
    const available = mapCloudProjectItem({
      ...base,
      project_id: "prd_joined",
      is_member: false,
      preview: {
        preview_id: "prev_joined",
        product_id: "prd_joined",
        product_display_name: "Joined",
        canonical: false,
        expires_at: "2999-01-01T00:00:00Z",
      },
    });
    const untouched = mapCloudProjectItem({
      ...base,
      project_id: "prd_available",
      is_member: false,
    });

    const reconciled = applyDaemonMembershipProjection(
      [available, untouched],
      [{ product_id: "prd_joined", setup_incomplete: true }],
    );

    expect(reconciled[0].membership).toBeUndefined();
    expect(reconciled[0].preview).toBeUndefined();
    expect(reconciled[0].setup_incomplete).toBe(true);
    expect(reconciled[1].membership).toBe("available");
  });
});

// ── Create-success refresh from Cloud discovery (AC7) ──────────────

describe("create-success refreshes Cloud discovery", () => {
  it("reloads Cloud discovery and daemon membership after a recorded create", () => {
    expect(AppSource).toContain('registrationResult.status === "recorded"');
    expect(AppSource).toContain("await loadHostedProjectDiscovery();");
    expect(AppSource).toContain("loadCloudProjects(");
    expect(AppSource).toContain("fetchHostedProductMemberships(");
    expect(AppSource).toContain("applyDaemonMembershipProjection(");
  });

  it("empty-state markup carries no literal invite-code copy (uses shared constants)", () => {
    // The default discovery view renders copy from shared constants; the raw
    // template must not hard-code an invite-code prompt in the default path.
    const start = OperatorHomeSource.indexOf('data-testid="empty-state"');
    const emptyBranch = OperatorHomeSource.slice(
      start,
      OperatorHomeSource.indexOf("{:else}", start),
    );
    expect(emptyBranch.toLowerCase()).not.toContain("invite code");
    expect(emptyBranch).toContain("DISCOVERY_EMPTY_DESCRIPTION");
  });
});
