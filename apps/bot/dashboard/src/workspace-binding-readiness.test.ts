/**
 * Hosted workspace binding readiness UI tests (bicameral-bot#734).
 *
 * Covers the daemon-owned readiness projection (#733) consumer:
 *  - pure view derivation for unbound / bound / repair-required / unsupported
 *    / loading states,
 *  - live fetch classification (including the daemon `upgrade_required`
 *    fail-closed path and network errors),
 *  - the WorkspaceBindingCard rendering for every state,
 *  - a no-path / no-secret rendering assertion, and
 *  - the WorkspaceBindingGate polling until the daemon reports bound.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@testing-library/svelte";
import * as api from "./lib/api";
import {
  deriveWorkspaceBindingView,
  loadWorkspaceBindingView,
  shouldPoll,
  type WorkspaceBindingView,
} from "./lib/workspace-binding-readiness";
import {
  WORKSPACE_READINESS_FIXTURES,
} from "./lib/workspace-binding-fixtures";
import {
  WORKSPACE_READINESS_FORBIDDEN_FIELDS,
  type HostedWorkspaceBindingReadiness,
  type SelectedProjectSessionResponse,
  type HostedDashboardCompatibility,
} from "./lib/hosted-bridge-types";
import WorkspaceBindingCard from "./pages/WorkspaceBindingCard.svelte";
import WorkspaceBindingGate from "./pages/WorkspaceBindingGate.svelte";
import type { ProjectEntry } from "./lib/project-fixtures";

const UPGRADE_REQUIRED_COMPAT: HostedDashboardCompatibility = {
  cloud_expected_protocol_version: 4,
  daemon_protocol_version: 1,
  state: "hosted_dashboard_upgrade_required",
  protocol_versions_behind: 3,
  upgrade_required: true,
  forward_skew: false,
  controls_available: false,
  read_only_projections_available: false,
  repair: {
    auto_update_available: false,
    update_command: "bicameral daemon update",
    instruction: "Run this command to update your local Bicameral daemon.",
  },
  note: "The local daemon is too far behind to use with the hosted dashboard.",
};

afterEach(() => cleanup());

// ── Pure derivation ────────────────────────────────────────────────

describe("deriveWorkspaceBindingView", () => {
  it("derives an actionable unbound CTA (not a failure) with MCP primary + CLI fallback", () => {
    const view = deriveWorkspaceBindingView({
      readiness: WORKSPACE_READINESS_FIXTURES.unbound,
      polling: true,
    });
    expect(view.kind).toBe("unbound");
    if (view.kind !== "unbound") throw new Error("expected unbound");
    expect(view.projectName).toBe("Ada Service");
    expect(view.mcpAvailable).toBe(true);
    expect(view.cliAvailable).toBe(true);
    expect(view.command).toBe("bicameral workspace bind");
    expect(view.polling).toBe(true);
  });

  it("derives a bound view with no next action", () => {
    const view = deriveWorkspaceBindingView({
      readiness: WORKSPACE_READINESS_FIXTURES.bound,
    });
    expect(view.kind).toBe("bound");
  });

  it("derives repair-required guidance", () => {
    const view = deriveWorkspaceBindingView({
      readiness: WORKSPACE_READINESS_FIXTURES.repair_required,
    });
    expect(view.kind).toBe("repair_required");
    if (view.kind !== "repair_required") throw new Error("expected repair");
    expect(view.caveats.length).toBeGreaterThan(0);
    expect(view.command).toBe("bicameral workspace repair");
  });

  it("returns loading when readiness is not yet resolved", () => {
    expect(deriveWorkspaceBindingView({ readiness: null }).kind).toBe("loading");
  });

  it("fails closed to unsupported (no bind CTA) when the daemon needs an upgrade", () => {
    const view = deriveWorkspaceBindingView({
      readiness: WORKSPACE_READINESS_FIXTURES.unbound,
      compatibility: UPGRADE_REQUIRED_COMPAT,
    });
    expect(view.kind).toBe("unsupported");
    if (view.kind !== "unsupported") throw new Error("expected unsupported");
    expect(view.repair?.update_command).toBe("bicameral daemon update");
  });

  it("fails closed when upgradeRequired is signalled even without a compat handshake", () => {
    const view = deriveWorkspaceBindingView({
      readiness: WORKSPACE_READINESS_FIXTURES.unbound,
      upgradeRequired: true,
    });
    expect(view.kind).toBe("unsupported");
  });

  it("only marks the unbound state as pollable", () => {
    expect(shouldPoll(deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.unbound }))).toBe(true);
    expect(shouldPoll(deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.bound }))).toBe(false);
    expect(shouldPoll(deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.repair_required }))).toBe(false);
  });
});

// ── Live fetch classification ──────────────────────────────────────

function sessionResponse(
  readiness: HostedWorkspaceBindingReadiness | undefined,
  overrides: Partial<SelectedProjectSessionResponse> = {},
): SelectedProjectSessionResponse {
  return {
    pairing_state: "hosted_bridge_paired",
    selected_project: { project_id: "ada-service" },
    binding_readiness: readiness?.binding_state === "local_workspace_bound" ? "ready" : "unbound",
    controls_available: true,
    workspace_readiness: readiness,
    resolved_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("loadWorkspaceBindingView", () => {
  afterEach(() => vi.restoreAllMocks());

  it("classifies a fetched unbound projection into an unbound CTA view", async () => {
    vi.spyOn(api, "fetchSelectedProjectReadiness").mockResolvedValue(
      sessionResponse(WORKSPACE_READINESS_FIXTURES.unbound),
    );
    const view = await loadWorkspaceBindingView({
      sessionToken: "tok",
      projectId: "ada-service",
    });
    expect(view.kind).toBe("unbound");
  });

  it("fails closed when the daemon reports binding_readiness=upgrade_required", async () => {
    vi.spyOn(api, "fetchSelectedProjectReadiness").mockResolvedValue(
      sessionResponse(undefined, { binding_readiness: "upgrade_required" }),
    );
    const view = await loadWorkspaceBindingView({
      sessionToken: "tok",
      projectId: "ada-service",
    });
    expect(view.kind).toBe("unsupported");
  });

  it("returns a retryable error view when the daemon is unreachable", async () => {
    vi.spyOn(api, "fetchSelectedProjectReadiness").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );
    const view = await loadWorkspaceBindingView({
      sessionToken: "tok",
      projectId: "ada-service",
    });
    expect(view.kind).toBe("error");
    if (view.kind !== "error") throw new Error("expected error");
    expect(view.retryable).toBe(true);
  });
});

// ── Component rendering ────────────────────────────────────────────

describe("WorkspaceBindingCard", () => {
  it("renders the unbound state as a next-step CTA with MCP primary + CLI fallback", () => {
    const { getByTestId } = render(WorkspaceBindingCard, {
      props: {
        view: deriveWorkspaceBindingView({
          readiness: WORKSPACE_READINESS_FIXTURES.unbound,
          polling: true,
        }),
      },
    });
    const cta = getByTestId("binding-cta");
    expect(cta).toBeTruthy();
    // Reads as a next step, not a failure.
    expect(getByTestId("binding-next-step-badge").textContent).toMatch(/next step/i);
    expect(getByTestId("binding-mcp-primary")).toBeTruthy();
    expect(getByTestId("binding-cli-fallback")).toBeTruthy();
    expect(getByTestId("binding-polling")).toBeTruthy();
    expect(cta.textContent?.toLowerCase()).not.toContain("failed");
    expect(cta.textContent?.toLowerCase()).not.toContain("error");
  });

  it("renders the bound state without a next action", () => {
    const { getByTestId, queryByTestId } = render(WorkspaceBindingCard, {
      props: {
        view: deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.bound }),
      },
    });
    expect(getByTestId("binding-bound")).toBeTruthy();
    expect(queryByTestId("binding-cta")).toBeNull();
  });

  it("renders repair-required guidance", () => {
    const { getByTestId } = render(WorkspaceBindingCard, {
      props: {
        view: deriveWorkspaceBindingView({
          readiness: WORKSPACE_READINESS_FIXTURES.repair_required,
        }),
      },
    });
    expect(getByTestId("binding-repair")).toBeTruthy();
  });

  it("renders the unsupported state fail-closed with upgrade guidance and no bind CTA", () => {
    const { getByTestId, queryByTestId } = render(WorkspaceBindingCard, {
      props: {
        view: deriveWorkspaceBindingView({
          readiness: WORKSPACE_READINESS_FIXTURES.unbound,
          compatibility: UPGRADE_REQUIRED_COMPAT,
        }),
      },
    });
    expect(getByTestId("binding-unsupported")).toBeTruthy();
    expect(getByTestId("binding-upgrade-command").textContent).toContain(
      "bicameral daemon update",
    );
    // Fails closed: no bind controls in the unsupported state.
    expect(queryByTestId("binding-mcp-primary")).toBeNull();
    expect(queryByTestId("binding-cli-fallback")).toBeNull();
  });

  it("renders loading and error states", () => {
    const loading = render(WorkspaceBindingCard, { props: { view: { kind: "loading" } } });
    expect(loading.getByTestId("binding-loading")).toBeTruthy();
    cleanup();

    const onretry = vi.fn();
    const errored = render(WorkspaceBindingCard, {
      props: {
        view: { kind: "error", message: "boom", retryable: true } as WorkspaceBindingView,
        onretry,
      },
    });
    expect(errored.getByTestId("binding-error")).toBeTruthy();
    expect(errored.getByTestId("binding-retry")).toBeTruthy();
  });
});

// ── No-path / no-secret rendering assertion ────────────────────────

describe("hosted workspace binding never renders local paths or secrets", () => {
  const PROHIBITED = [
    /\/(?:home|Users|var|tmp|opt|root)\//, // POSIX absolute paths
    /[A-Za-z]:\\\\/, // Windows absolute paths
    /(?:ghp_|gho_|github_pat_|xox[baprs]-|sk-)/, // common token prefixes
  ];

  it("no readiness fixture carries a forbidden field or path-like value", () => {
    for (const readiness of Object.values(WORKSPACE_READINESS_FIXTURES)) {
      const json = JSON.stringify(readiness);
      for (const field of WORKSPACE_READINESS_FORBIDDEN_FIELDS) {
        expect(json).not.toContain(`"${field}"`);
      }
      for (const pattern of PROHIBITED) {
        expect(pattern.test(json)).toBe(false);
      }
    }
  });

  it("no rendered binding state leaks a local path or secret", () => {
    const readinessWithPathAttempt: HostedWorkspaceBindingReadiness = {
      ...WORKSPACE_READINESS_FIXTURES.unbound,
      display: { display_name: "Ada Service" },
    };
    for (const view of [
      deriveWorkspaceBindingView({ readiness: readinessWithPathAttempt, polling: true }),
      deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.bound }),
      deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.repair_required }),
      deriveWorkspaceBindingView({
        readiness: WORKSPACE_READINESS_FIXTURES.unbound,
        compatibility: UPGRADE_REQUIRED_COMPAT,
      }),
    ]) {
      const { container } = render(WorkspaceBindingCard, { props: { view } });
      const text = container.textContent ?? "";
      for (const pattern of PROHIBITED) {
        expect(pattern.test(text)).toBe(false);
      }
      cleanup();
    }
  });
});

// ── Polling gate ───────────────────────────────────────────────────

describe("WorkspaceBindingGate polling", () => {
  const project: ProjectEntry = {
    id: "ada-service",
    name: "Ada Service",
    event_store_kind: "github",
    event_store_label: "GitHub",
    readiness: "needs_access",
  };

  it("polls the daemon and auto-updates + fires onbound when readiness flips to bound", async () => {
    const views: WorkspaceBindingView[] = [
      deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.unbound, polling: true }),
      deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.unbound, polling: true }),
      deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.bound }),
    ];
    let call = 0;
    const load = vi.fn(async () => views[Math.min(call++, views.length - 1)]);
    const onbound = vi.fn();

    const { getByTestId } = render(WorkspaceBindingGate, {
      props: { project, sessionToken: "tok", pollIntervalMs: 10, load, onbound },
    });

    // First render shows the unbound CTA while polling.
    await waitFor(() => expect(getByTestId("binding-cta")).toBeTruthy());

    // Keeps polling until the daemon reports bound, then fires onbound.
    await waitFor(() => expect(onbound).toHaveBeenCalledTimes(1), { timeout: 2000 });
    expect(load.mock.calls.length).toBeGreaterThanOrEqual(3);
  });

  it("does not poll and surfaces an error when there is no session token", async () => {
    const load = vi.fn(async () =>
      deriveWorkspaceBindingView({ readiness: WORKSPACE_READINESS_FIXTURES.unbound }),
    );
    const { getByTestId } = render(WorkspaceBindingGate, {
      props: { project, pollIntervalMs: 10, load },
    });
    await waitFor(() => expect(getByTestId("binding-error")).toBeTruthy());
    expect(load).not.toHaveBeenCalled();
  });
});
