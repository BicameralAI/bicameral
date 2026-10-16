// @vitest-environment node
/**
 * Hosted-dashboard daemon protocol compatibility tests — #697 / #711.
 *
 * Validates the client-side compatibility evaluation (mirror of the Rust
 * `evaluate_hosted_dashboard_compatibility`) and the repair / forward-skew UI
 * wiring, covering the four required cases: current, N-1, N-2-old, forward-skew.
 */

import { describe, it, expect } from "vitest";
import {
  evaluateHostedDashboardCompatibility,
  HOSTED_DASHBOARD_PROTOCOL_VERSION,
  HOSTED_DASHBOARD_SUPPORT_WINDOW,
} from "./lib/hosted-bridge-types";
import compatNoticeSource from "./pages/HostedCompatibilityNotice.svelte?raw";
import hostedDashboardSource from "./pages/HostedDashboard.svelte?raw";

// ═══════════════════════════════════════════════════════════════════════════
// 1. Evaluation keyed on integer daemon_protocol_version (never semver)
// ═══════════════════════════════════════════════════════════════════════════

describe("evaluateHostedDashboardCompatibility", () => {
  it("current protocol → fully available, no repair", () => {
    const c = evaluateHostedDashboardCompatibility(2, 2);
    expect(c.state).toBe("current");
    expect(c.protocol_versions_behind).toBe(0);
    expect(c.upgrade_required).toBe(false);
    expect(c.forward_skew).toBe(false);
    expect(c.controls_available).toBe(true);
    expect(c.read_only_projections_available).toBe(true);
    expect(c.repair).toBeUndefined();
  });

  it("N-1 protocol → supported_previous, still usable", () => {
    const c = evaluateHostedDashboardCompatibility(2, 1);
    expect(c.state).toBe("supported_previous");
    expect(c.protocol_versions_behind).toBe(HOSTED_DASHBOARD_SUPPORT_WINDOW);
    expect(c.upgrade_required).toBe(false);
    expect(c.controls_available).toBe(true);
    expect(c.read_only_projections_available).toBe(true);
    expect(c.repair).toBeUndefined();
  });

  it("N-2-old protocol → upgrade_required with one-action repair path", () => {
    const c = evaluateHostedDashboardCompatibility(3, 1);
    expect(c.state).toBe("hosted_dashboard_upgrade_required");
    expect(c.protocol_versions_behind).toBe(2);
    expect(c.upgrade_required).toBe(true);
    expect(c.forward_skew).toBe(false);
    // Too-old daemon is gated entirely.
    expect(c.controls_available).toBe(false);
    expect(c.read_only_projections_available).toBe(false);
    // One-action repair: a single copy-paste command.
    expect(c.repair).toBeDefined();
    expect(c.repair?.update_command).toBeTruthy();
    expect(c.repair?.instruction).toBeTruthy();
  });

  it("newer daemon → forward_skew, degrades safely (no hard fail)", () => {
    const c = evaluateHostedDashboardCompatibility(2, 3);
    expect(c.state).toBe("hosted_dashboard_forward_skew");
    expect(c.protocol_versions_behind).toBe(-1);
    expect(c.upgrade_required).toBe(false);
    expect(c.forward_skew).toBe(true);
    // Reads still render; controls hide until the dashboard catches up.
    expect(c.controls_available).toBe(false);
    expect(c.read_only_projections_available).toBe(true);
    expect(c.repair).toBeUndefined();
  });

  it("SPA default expected protocol matches the daemon build constant", () => {
    const c = evaluateHostedDashboardCompatibility(
      HOSTED_DASHBOARD_PROTOCOL_VERSION,
      HOSTED_DASHBOARD_PROTOCOL_VERSION,
    );
    expect(c.state).toBe("current");
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 2. Repair notice UI renders the one-action path and forward-skew state
// ═══════════════════════════════════════════════════════════════════════════

describe("HostedCompatibilityNotice UI", () => {
  it("renders a copy-paste update command for the upgrade-required repair", () => {
    expect(compatNoticeSource).toContain('data-testid="compat-update-command"');
    expect(compatNoticeSource).toContain('data-testid="compat-copy-command"');
    expect(compatNoticeSource).toContain("clipboard.writeText");
  });

  it("offers an auto-update action when the daemon supports it", () => {
    expect(compatNoticeSource).toContain("auto_update_available");
    expect(compatNoticeSource).toContain('data-testid="compat-auto-update"');
  });

  it("renders a benign catching-up state for forward skew", () => {
    expect(compatNoticeSource).toContain('data-testid="compat-forward-skew-title"');
    expect(compatNoticeSource).toContain("catching up");
  });

  it("distinguishes each compatibility state via data-compat-state", () => {
    expect(compatNoticeSource).toContain("data-compat-state={compatibility.state}");
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 3. HostedDashboard gates on upgrade-required and banners other skew states
// ═══════════════════════════════════════════════════════════════════════════

describe("HostedDashboard compatibility wiring", () => {
  it("blocks the paired shell and shows the notice when upgrade is required", () => {
    expect(hostedDashboardSource).toContain("upgradeRequired");
    expect(hostedDashboardSource).toContain("HostedCompatibilityNotice");
  });

  it("shows a non-blocking banner for forward-skew / N-1 states", () => {
    expect(hostedDashboardSource).toContain("compatBanner");
    expect(hostedDashboardSource).toContain("hosted_dashboard_forward_skew");
    expect(hostedDashboardSource).toContain("supported_previous");
  });
});
