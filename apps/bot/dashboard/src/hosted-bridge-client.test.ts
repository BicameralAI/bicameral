/**
 * Hosted bridge client tests — issue #648.
 *
 * Validates:
 *   1. Live bridge client module exports and function signatures.
 *   2. Daemon network failure → daemon_unavailable error state.
 *   3. HTTP 500 from hosted-bridge routes → daemon_unavailable repair state.
 *   4. Successful pair response → paired state with session token.
 *   5. Successful project fetch → ProjectEntry[] mapping.
 *   6. classifyHostedRouteError covers network and 500 cases.
 *   7. Fixture data remains available for tests (not removed).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import appSource from "./App.svelte?raw";
import hostedDashboardSource from "./pages/HostedDashboard.svelte?raw";
import hostedProjectShellSource from "./pages/HostedProjectShell.svelte?raw";
import ingestionGateSource from "./pages/IngestionGate.svelte?raw";
import settingsSource from "./pages/Settings.svelte?raw";
import projectSettingsSource from "./pages/ProjectSettings.svelte?raw";
import userSettingsSource from "./pages/UserSettings.svelte?raw";
import setupWizardSource from "./pages/SetupWizard.svelte?raw";
import projectSelectorSource from "./lib/ProjectSelector.svelte?raw";

// ═══════════════════════════════════════════════════════════════════════════
// 1. Module exports
// ═══════════════════════════════════════════════════════════════════════════

describe("Hosted bridge client module exports", () => {
  it("exports pairWithDaemon function", async () => {
    const mod = await import("./lib/hosted-bridge-client");
    expect(typeof mod.pairWithDaemon).toBe("function");
  });

  it("exports pollBridgeStatus function", async () => {
    const mod = await import("./lib/hosted-bridge-client");
    expect(typeof mod.pollBridgeStatus).toBe("function");
  });

  it("exports fetchDaemonProjects function", async () => {
    const mod = await import("./lib/hosted-bridge-client");
    expect(typeof mod.fetchDaemonProjects).toBe("function");
  });

  it("exports classifyHostedRouteError function", async () => {
    const mod = await import("./lib/hosted-bridge-client");
    expect(typeof mod.classifyHostedRouteError).toBe("function");
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 2. Daemon network failure → daemon_unavailable
// ═══════════════════════════════════════════════════════════════════════════

describe("Daemon network failure produces daemon_unavailable state", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("pairWithDaemon returns daemon_unavailable on network error", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );
    const { pairWithDaemon } = await import("./lib/hosted-bridge-client");
    const result = await pairWithDaemon(
      "https://app.bicameral.cloud",
      "acct_test",
      "csrf_token",
      "team-1",
      "cloud-token-1",
    );
    expect(result.state.kind).toBe("error");
    if (result.state.kind === "error") {
      expect(result.state.errorKind).toBe("daemon_unavailable");
      expect(result.state.retryAfterRepair).toBe(true);
      expect(result.state.message).toContain("not reachable");
    }
  });

  it("pollBridgeStatus returns daemon_unavailable on network error", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );
    const { pollBridgeStatus } = await import("./lib/hosted-bridge-client");
    const result = await pollBridgeStatus("test-session-token");
    expect(result.state.kind).toBe("error");
    if (result.state.kind === "error") {
      expect(result.state.errorKind).toBe("daemon_unavailable");
      expect(result.state.retryAfterRepair).toBe(true);
    }
  });

  it("fetchDaemonProjects returns daemon_unavailable on network error", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new TypeError("Failed to fetch"),
    );
    const { fetchDaemonProjects } = await import("./lib/hosted-bridge-client");
    const result = await fetchDaemonProjects("test-session-token");
    expect(result.projects).toEqual([]);
    expect(result.error).toBeDefined();
    if (result.error && result.error.kind === "error") {
      expect(result.error.errorKind).toBe("daemon_unavailable");
      expect(result.error.retryAfterRepair).toBe(true);
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 3. HTTP 500 from hosted-bridge routes → repair state
// ═══════════════════════════════════════════════════════════════════════════

describe("HTTP 500 from hosted-bridge routes produces repair state", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("pairWithDaemon maps typed 400 MissingTeamId to return-to-/connect recovery", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          error: "missing required field: team_id",
          kind: "MissingTeamId",
          message: "POST /pair requires non-empty team_id",
        }),
        { status: 400, headers: { "content-type": "application/json" } },
      ),
    );
    const { pairWithDaemon, isPairInitRecoveryError } = await import(
      "./lib/hosted-bridge-client"
    );
    const result = await pairWithDaemon(
      "https://app.bicameral.cloud",
      "acct_test",
      "csrf_token",
      "",
      "cloud-token-1",
    );
    expect(result.state.kind).toBe("error");
    if (result.state.kind === "error") {
      expect(result.state.errorKind).toBe("missing_team_id");
      expect(isPairInitRecoveryError(result.state)).toBe(true);
    }
  });

  it("pairWithDaemon maps typed 400 MissingCloudAccessToken to return-to-/connect recovery", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          error: "missing required field: cloud_access_token",
          kind: "MissingCloudAccessToken",
          message: "POST /pair requires non-empty cloud_access_token",
        }),
        { status: 400, headers: { "content-type": "application/json" } },
      ),
    );
    const { pairWithDaemon, isPairInitRecoveryError } = await import(
      "./lib/hosted-bridge-client"
    );
    const result = await pairWithDaemon(
      "https://app.bicameral.cloud",
      "acct_test",
      "csrf_token",
      "team-1",
      "",
    );
    expect(result.state.kind).toBe("error");
    if (result.state.kind === "error") {
      expect(result.state.errorKind).toBe("missing_cloud_access_token");
      expect(isPairInitRecoveryError(result.state)).toBe(true);
    }
  });

  it("pairWithDaemon returns error state on HTTP 500", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ error: "internal_error", message: "Internal Server Error" }), {
        status: 500,
        headers: { "content-type": "application/json" },
      }),
    );
    const { pairWithDaemon } = await import("./lib/hosted-bridge-client");
    const result = await pairWithDaemon(
      "https://app.bicameral.cloud",
      "acct_test",
      "csrf_token",
      "team-1",
      "cloud-token-1",
    );
    expect(result.state.kind).toBe("error");
    if (result.state.kind === "error") {
      expect(result.state.errorKind).toBe("daemon_unavailable");
      expect(result.state.retryAfterRepair).toBe(true);
    }
  });

  it("fetchDaemonProjects returns error state on HTTP 500", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("Internal Server Error", { status: 500 }),
    );
    const { fetchDaemonProjects } = await import("./lib/hosted-bridge-client");
    const result = await fetchDaemonProjects("test-session-token");
    expect(result.projects).toEqual([]);
    expect(result.error).toBeDefined();
    if (result.error && result.error.kind === "error") {
      expect(result.error.retryAfterRepair).toBe(true);
    }
  });

  it("500 error does not render raw error text in state message", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("Internal Server Error", { status: 500 }),
    );
    const { fetchDaemonProjects } = await import("./lib/hosted-bridge-client");
    const result = await fetchDaemonProjects("test-session-token");
    if (result.error && result.error.kind === "error") {
      expect(result.error.message).not.toContain("Internal Server Error");
      expect(result.error.message).toContain("HTTP 500");
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 4. Successful pair response → paired state
// ═══════════════════════════════════════════════════════════════════════════

describe("Successful daemon pairing", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("pairWithDaemon returns paired state on success", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          state: "paired",
          session_token: "tok_abc",
          expires_at: "2026-07-01T10:00:00Z",
          capabilities: {
            bridge_supported: true,
            bridge_protocol_version: "v1",
            project_list_available: true,
            workspace_binding_available: false,
          },
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
    const { pairWithDaemon } = await import("./lib/hosted-bridge-client");
    const result = await pairWithDaemon(
      "https://app.bicameral.cloud",
      "acct_test_12345",
      "csrf_token",
      "team-1",
      "cloud-token-1",
    );
    expect(result.state.kind).toBe("paired");
    expect(result.sessionToken).toBe("tok_abc");
    if (result.state.kind === "paired") {
      expect(result.state.expiresAt).toBe("2026-07-01T10:00:00Z");
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 5. Successful project fetch → ProjectEntry[] mapping
// ═══════════════════════════════════════════════════════════════════════════

describe("Successful daemon project fetch", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("fetchDaemonProjects maps workspace-discovery response to ProjectEntry[]", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          workspaces: [
            {
              project_name: "my-service",
              event_store_healthy: true,
              has_workspace_binding: true,
              setup_state: "normal",
              code_grounding_ready: true,
              decision_count: 5,
              candidate_count: 2,
              event_store_kind: "github",
            },
            {
              project_name: "broken-svc",
              event_store_healthy: false,
              has_workspace_binding: false,
              setup_state: "needs_setup",
              code_grounding_ready: false,
              decision_count: 0,
              candidate_count: 0,
            },
          ],
          projection_only: true,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
    const { fetchDaemonProjects } = await import("./lib/hosted-bridge-client");
    const result = await fetchDaemonProjects("test-session-token");
    expect(result.error).toBeUndefined();
    expect(result.projects.length).toBe(2);

    const ready = result.projects[0];
    expect(ready.name).toBe("my-service");
    expect(ready.readiness).toBe("ready");
    expect(ready.event_store_kind).toBe("github");
    expect(ready.id).toBeTruthy();

    const broken = result.projects[1];
    expect(broken.name).toBe("broken-svc");
    expect(broken.readiness).toBe("needs_access");
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 6. classifyHostedRouteError coverage
// ═══════════════════════════════════════════════════════════════════════════

describe("classifyHostedRouteError", () => {
  it("classifies TypeError as daemon_unavailable", async () => {
    const { classifyHostedRouteError } = await import(
      "./lib/hosted-bridge-client"
    );
    const state = classifyHostedRouteError(new TypeError("Failed to fetch"));
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("daemon_unavailable");
      expect(state.retryAfterRepair).toBe(true);
    }
  });

  it("classifies HTTP 500 status as daemon_unavailable repair state", async () => {
    const { classifyHostedRouteError } = await import(
      "./lib/hosted-bridge-client"
    );
    const state = classifyHostedRouteError(new Error("server error"), 500);
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("daemon_unavailable");
      expect(state.retryAfterRepair).toBe(true);
      expect(state.message).toContain("500");
    }
  });

  it("classifies HTTP 502 status as daemon_unavailable repair state", async () => {
    const { classifyHostedRouteError } = await import(
      "./lib/hosted-bridge-client"
    );
    const state = classifyHostedRouteError(new Error("bad gateway"), 502);
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("daemon_unavailable");
      expect(state.retryAfterRepair).toBe(true);
    }
  });

  it("classifies unknown error without status as daemon_unavailable", async () => {
    const { classifyHostedRouteError } = await import(
      "./lib/hosted-bridge-client"
    );
    const state = classifyHostedRouteError(new Error("something went wrong"));
    expect(state.kind).toBe("error");
    if (state.kind === "error") {
      expect(state.errorKind).toBe("daemon_unavailable");
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 7. Fixture data remains available for tests
// ═══════════════════════════════════════════════════════════════════════════

describe("Fixture data remains available for tests and development", () => {
  it("HOSTED_BRIDGE_FIXTURES still exports all states", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    expect(HOSTED_BRIDGE_FIXTURES.unpaired).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.paired).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_daemon_unavailable).toBeDefined();
  });

  it("SAMPLE_PROJECTS still exports fixture data", async () => {
    const { SAMPLE_PROJECTS } = await import("./lib/project-fixtures");
    expect(SAMPLE_PROJECTS.length).toBeGreaterThan(0);
  });

  it("DASHBOARD_ADAPTER_FIXTURES has no unpaired fixture-success response", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    expect(DASHBOARD_ADAPTER_FIXTURES.hosted_with_data).toBeDefined();
    expect(DASHBOARD_ADAPTER_FIXTURES.fixture_fallback).toBeUndefined();
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 8. App.svelte hosted mode no longer imports HOSTED_BRIDGE_FIXTURES
// ═══════════════════════════════════════════════════════════════════════════

describe("Production hosted mode uses live bridge client", () => {
  it("App.svelte resolves with bridge client imports", async () => {
    const mod = await import("./App.svelte");
    expect(mod.default).toBeDefined();
  });

  it("hosted-bridge-client module resolves", async () => {
    const mod = await import("./lib/hosted-bridge-client");
    expect(mod).toBeDefined();
  });

  it("HostedDashboard defaults projects to empty array (not SAMPLE_PROJECTS)", async () => {
    const mod = await import("./pages/HostedDashboard.svelte");
    expect(mod.default).toBeDefined();
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 9. Production runtime does not consume fixture data
// ═══════════════════════════════════════════════════════════════════════════

describe("Production dashboard runtime does not consume fixture data", () => {
  it("App.svelte hosted mode does not import hosted bridge fixtures or sample projects", () => {
    const app = appSource;
    expect(app).not.toContain("HOSTED_BRIDGE_FIXTURES");
    expect(app).not.toContain("projects={handoff ? hostedProjects : SAMPLE_PROJECTS}");
    expect(app).toContain("projects={hostedProjects}");
    expect(app).toContain("sessionToken={bridgeSessionToken}");
  });

  it("hosted project shell does not mount local activity or local ledger pages", () => {
    const shell = hostedProjectShellSource;
    expect(shell).not.toContain('from "./IngestionGate.svelte"');
    expect(shell).not.toContain('from "./LedgerView.svelte"');
    expect(shell).toContain('from "./HostedLedgerView.svelte"');
    expect(shell).toContain("hosted-activity-deferred");
  });

  it("hosted operator home wires Create to Cloud registration and Join per-Product (cloud#66, bot#768)", () => {
    const hostedDashboard = hostedDashboardSource;
    // Create opens the registration panel; Join is now a daemon-authoritative
    // per-Product action routed to App's token-owning join handler.
    expect(hostedDashboard).toContain("oncreate={openCreate}");
    expect(hostedDashboard).toContain("onjoin={handleJoin}");
    expect(hostedDashboard).toContain("HostedProjectRegistration");
    expect(hostedDashboard).toContain("registrationOpen");
    expect(hostedDashboard).toContain('registrationResult?.status === "recorded"');
    expect(hostedDashboard).not.toContain("handleRegisterRequest");
    expect(hostedDashboard).not.toContain('data-testid="register-notice"');
    // No legacy global/free-form Join panel mode remains.
    expect(hostedDashboard).not.toContain("openJoin");
    expect(hostedDashboard).not.toContain('onsubmitjoin');

    const app = appSource;
    expect(app).toContain("isPairInitRecoveryError");
    expect(app).toContain("restartHostedPairing");
    expect(app).toContain("oncreateproject={handleHostedCreate}");
    expect(app).toContain("onjoinproduct={handleHostedProductJoin}");
    expect(app).not.toContain("onjoinproject=");
  });

  it("settings and setup runtime pages do not import sample connection/source/project data", () => {
    const files = [
      ["./pages/IngestionGate.svelte", ingestionGateSource],
      ["./pages/Settings.svelte", settingsSource],
      ["./pages/ProjectSettings.svelte", projectSettingsSource],
      ["./pages/UserSettings.svelte", userSettingsSource],
      ["./pages/SetupWizard.svelte", setupWizardSource],
      ["./lib/ProjectSelector.svelte", projectSelectorSource],
    ] as const;
    const forbidden = [
      "SAMPLE_TOOL_SUBMISSIONS",
      "SAMPLE_CONNECTIONS",
      "SAMPLE_SOURCE_BINDINGS",
      "SAMPLE_DISCOVERED_PROJECTS",
      "SAMPLE_PROJECTS",
    ];

    for (const [file, text] of files) {
      for (const token of forbidden) {
        expect(text, `${file} must not consume ${token}`).not.toContain(token);
      }
    }
  });
});
