/**
 * Production hosted-runtime gate regression tests (issue #717).
 *
 * Proves the production hosted bundle cannot enter the same-origin local
 * dashboard (`SetupWizard`, local sidebar pages, local project fixtures) and
 * that `?hosted` is not the production authority switch, per
 * `docs/specs/hosted-and-dev-dashboard-mode-runtime-architecture.md`.
 *
 * Two layers:
 *   1. Unit tests of the pure mode-resolution logic.
 *   2. Raw-source assertions that App.svelte wires that logic so the local
 *      dashboard branch is structurally gated behind the dev-only flag.
 */

import { describe, it, expect } from "vitest";
import appSource from "./App.svelte?raw";
import {
  resolveDashboardRuntimeMode,
  resolveDevDashboardFlag,
  localDashboardReachable,
} from "./lib/dashboard-runtime-mode";

// ── Build-time dev-dashboard flag ──────────────────────────────

describe("resolveDevDashboardFlag: production builds are hosted-only", () => {
  it("production build (PROD=true) does NOT compile the dev dashboard in", () => {
    expect(resolveDevDashboardFlag({ PROD: true })).toBe(false);
  });

  it("dev/test builds also use the hosted runtime", () => {
    expect(resolveDevDashboardFlag({ PROD: false })).toBe(false);
    expect(resolveDevDashboardFlag({})).toBe(false);
  });

});

// ── Runtime mode selection ─────────────────────────────────────

describe("resolveDashboardRuntimeMode: production cannot enter local mode", () => {
  it("production (no dev harness) is always hosted, regardless of inputs", () => {
    for (const hasHandoff of [true, false]) {
      for (const hostedQueryParam of [true, false]) {
        expect(
          resolveDashboardRuntimeMode({
            devDashboardMode: false,
            hasHandoff,
            hostedQueryParam,
          }),
        ).toBe("hosted");
      }
    }
  });

  it("`?hosted` is NOT the production authority switch: no handoff still hosted", () => {
    // Production build: ?hosted present but no authenticated handoff — must not
    // fall back to a local dashboard; production hosting is explicit.
    expect(
      resolveDashboardRuntimeMode({
        devDashboardMode: false,
        hasHandoff: false,
        hostedQueryParam: true,
      }),
    ).toBe("hosted");
    // And with ?hosted absent + no handoff, production is still hosted (never local).
    expect(
      resolveDashboardRuntimeMode({
        devDashboardMode: false,
        hasHandoff: false,
        hostedQueryParam: false,
      }),
    ).toBe("hosted");
  });

  it("an authenticated hosted handoff enters hosted mode in dev too", () => {
    expect(
      resolveDashboardRuntimeMode({
        devDashboardMode: true,
        hasHandoff: true,
        hostedQueryParam: false,
      }),
    ).toBe("hosted");
  });

  it("dev without handoff remains hosted and never selects fixtures", () => {
    expect(
      resolveDashboardRuntimeMode({
        devDashboardMode: true,
        hasHandoff: false,
        hostedQueryParam: false,
      }),
    ).toBe("hosted");
  });

  it("dev harness may exercise pairing UI via ?hosted", () => {
    expect(
      resolveDashboardRuntimeMode({
        devDashboardMode: true,
        hasHandoff: false,
        hostedQueryParam: true,
      }),
    ).toBe("hosted");
  });
});

describe("localDashboardReachable: local fixture UI is removed", () => {
  it("hosted (production runtime) can never render local dashboard surfaces", () => {
    expect(localDashboardReachable("hosted")).toBe(false);
  });

});

// ── App.svelte raw-source wiring ───────────────────────────────

describe("App.svelte is hosted-only", () => {
  it("does not import or install the removed dev fixture harness", () => {
    expect(appSource).not.toContain("./dev/");
    expect(appSource).not.toContain("getDevProjects");
    expect(appSource).toContain("const hostedMode = true");
    expect(appSource).toContain("const devDashboardMode = false");
  });

  it("no longer treats `?hosted` alone as the hosted-mode authority", () => {
    // The retired implicit switch derived hostedMode directly from the query
    // param OR'd with the handoff. hostedMode must now come from runtimeMode.
    expect(appSource).not.toContain(
      'handoff !== null || new URLSearchParams(window.location.search).has("hosted")',
    );
  });

  it("renders HostedDashboard as the only reachable branch", () => {
    const hostedBranch = appSource.indexOf("{#if hostedMode}");
    const devGuard = appSource.indexOf("{:else if devDashboardMode}");
    const setupWizard = appSource.indexOf("<SetupWizard");
    const localLayout = appSource.indexOf('<div class="layout">');

    expect(hostedBranch).toBeGreaterThanOrEqual(0);
    expect(devGuard).toBeGreaterThan(hostedBranch);
    // Legacy components remain temporarily compiled but are unreachable; no
    // fixture harness or runtime flag can activate them.
    expect(setupWizard).toBeGreaterThan(devGuard);
    expect(localLayout).toBeGreaterThan(devGuard);
  });

  it("skips the same-origin setup probe", () => {
    expect(appSource).toContain("if (!devDashboardMode)");
    expect(appSource).toContain("if (devDashboardMode) {");
  });
});
