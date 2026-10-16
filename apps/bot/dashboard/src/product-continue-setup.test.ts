/**
 * Local setup continuation UI/contract tests (bot#769).
 *
 * Covers the frontend surface for continuing setup after a canonical join:
 *   1. `submitProductContinueSetup` posts only safe, non-secret fields to the
 *      paired daemon and surfaces typed rejections (never fabricates success).
 *   2. `applySetupProjection` maps the daemon readiness projection onto a row's
 *      readiness axes and never rolls membership back (never `join`).
 *   3. `fetchDaemonProjects` treats a `LocalWorkspaceBinding` as optional: a
 *      healthy event store is `ready` (Open) even without a workspace binding.
 *   4. App/HostedDashboard wire the daemon continuation hop.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import appSource from "./App.svelte?raw";
import hostedDashboardSource from "./pages/HostedDashboard.svelte?raw";
import type { ProjectEntry } from "./lib/project-fixtures";
import { rowAction } from "./lib/operator-home-model";
import { applySetupProjection } from "./lib/hosted-bridge-client";
import type { HostedSetupProjection } from "./lib/api";

function member(overrides: Partial<ProjectEntry> = {}): ProjectEntry {
  return {
    id: "prd_test",
    name: "Acme Checkout",
    event_store_kind: "github",
    event_store_label: "GitHub event store",
    readiness: "ready",
    membership: "member",
    ...overrides,
  };
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. submitProductContinueSetup — safe request + typed rejection
// ═══════════════════════════════════════════════════════════════════════════

describe("submitProductContinueSetup", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("posts only safe non-secret fields to the paired daemon route", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          product_id: "prd_test",
          readiness: "open",
          has_registration: true,
          membership_retained: true,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
    const { submitProductContinueSetup } = await import("./lib/api");
    const projection = await submitProductContinueSetup({
      sessionToken: "tok",
      productId: "prd_test",
      registrationRef: "reg_opaque",
      providerHint: "github",
    });
    expect(projection.readiness).toBe("open");

    const [url, init] = fetchSpy.mock.calls[0];
    expect(String(url)).toContain(
      "/api/v1/hosted-bridge/product-continue-setup",
    );
    expect((init as RequestInit).method).toBe("POST");
    const headers = (init as RequestInit).headers as Record<string, string>;
    expect(headers["x-bridge-session-token"]).toBe("tok");

    const sent = JSON.parse((init as RequestInit).body as string);
    // Every field sent is drawn only from the bounded, non-authoritative
    // continuation contract (undefined optionals are dropped by JSON.stringify).
    const allowed = [
      "account_label",
      "product_id",
      "provider_hint",
      "registration_ref",
      "request_id",
      "substrate_hint",
    ];
    for (const key of Object.keys(sent)) {
      expect(allowed).toContain(key);
    }
    expect(sent.product_id).toBe("prd_test");
    expect(sent.registration_ref).toBe("reg_opaque");
    // No provider secret, raw descriptor, filesystem path, membership claim, or
    // actor/authority context is ever sent from the SPA.
    const raw = (init as RequestInit).body as string;
    for (const forbidden of [
      "token",
      "secret",
      "credential",
      "descriptor",
      "/home/",
      "authority",
      "membership",
      "actor",
    ]) {
      expect(raw.toLowerCase()).not.toContain(forbidden);
    }
  });

  it("surfaces a typed membership-required rejection instead of faking success", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          code: "membership_required",
          membership_retained: false,
        }),
        { status: 409, headers: { "content-type": "application/json" } },
      ),
    );
    const { submitProductContinueSetup } = await import("./lib/api");
    await expect(
      submitProductContinueSetup({
        sessionToken: "tok",
        productId: "prd_test",
        registrationRef: "reg_opaque",
      }),
    ).rejects.toMatchObject({ status: 409, body: { code: "membership_required" } });
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 2. applySetupProjection — readiness mapping, membership never rolled back
// ═══════════════════════════════════════════════════════════════════════════

describe("applySetupProjection", () => {
  const cases: Array<[HostedSetupProjection["readiness"], ReturnType<typeof rowAction>]> = [
    ["open", "open"],
    ["continue_setup", "continue_setup"],
    ["repair", "repair"],
  ];

  it.each(cases)("readiness %s → row action %s and stays a member", (readiness, expected) => {
    const projection: HostedSetupProjection = {
      product_id: "prd_test",
      readiness,
      has_registration: readiness !== "repair",
      membership_retained: true,
    };
    const next = applySetupProjection(member(), projection);
    // Membership axis is never touched — a joined Product never reverts to Join.
    expect(next.membership).toBe("member");
    expect(rowAction(next)).toBe(expected);
    expect(rowAction(next)).not.toBe("join");
  });

  it("open clears the incomplete-setup flag, continue_setup sets it", () => {
    const open = applySetupProjection(member({ setup_incomplete: true }), {
      product_id: "prd_test",
      readiness: "open",
      has_registration: true,
      membership_retained: true,
    });
    expect(open.readiness).toBe("ready");
    expect(open.setup_incomplete).toBe(false);

    const cont = applySetupProjection(member(), {
      product_id: "prd_test",
      readiness: "continue_setup",
      has_registration: false,
      membership_retained: true,
    });
    expect(cont.readiness).toBe("ready");
    expect(cont.setup_incomplete).toBe(true);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 3. LocalWorkspaceBinding stays optional (does not block Open)
// ═══════════════════════════════════════════════════════════════════════════

describe("fetchDaemonProjects readiness (workspace binding optional)", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("a healthy event store is ready even without a workspace binding", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          workspaces: [
            {
              project_name: "registered-no-binding",
              event_store_healthy: true,
              has_workspace_binding: false,
              event_store_kind: "github",
            },
            {
              project_name: "unhealthy-store",
              event_store_healthy: false,
              has_workspace_binding: true,
              event_store_kind: "github",
            },
          ],
          projection_only: true,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
    const { fetchDaemonProjects } = await import("./lib/hosted-bridge-client");
    const { projects } = await fetchDaemonProjects("tok");

    // Registered + healthy store, no binding → Open (ready), not gated.
    expect(projects[0].readiness).toBe("ready");
    expect(projects[0].access_failure).toBeUndefined();
    // Unhealthy event store → needs_access regardless of binding.
    expect(projects[1].readiness).toBe("needs_access");
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 4. Wiring: the daemon continuation hop is driven from App/HostedDashboard
// ═══════════════════════════════════════════════════════════════════════════

describe("continuation wiring", () => {
  it("App drives the daemon product.continue_setup hop and passes it down", () => {
    expect(appSource).toContain("submitProductContinueSetup");
    expect(appSource).toContain("handleHostedContinueSetup");
    expect(appSource).toContain("oncontinuesetup={handleHostedContinueSetup}");
    // Never claims membership on the SPA side.
    expect(appSource).not.toContain('membership: "member"');
  });

  it("HostedDashboard applies the daemon readiness projection and opens on success", () => {
    expect(hostedDashboardSource).toContain("applySetupProjection");
    expect(hostedDashboardSource).toContain("oncontinuesetup?.(");
    expect(hostedDashboardSource).toContain('projection.readiness === "open"');
  });
});
