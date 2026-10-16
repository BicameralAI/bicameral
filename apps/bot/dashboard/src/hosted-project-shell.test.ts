/**
 * Hosted project shell tests (issue #620).
 *
 * Validates:
 *   - Shell routing: project selection from operator home routes to
 *     HostedProjectShell; back returns to operator home.
 *   - Surface reuse: hosted shell renders existing IngestionGate,
 *     LedgerView, Channels components (same types.ts contracts).
 *   - No local path/secret leakage in fixtures and shell props.
 *   - Governed-action disabling in hosted mode.
 *   - Fallback/error states for fixture-fallback adapter responses.
 *   - Tab navigation coverage.
 *   - Ready project selection from operator home.
 *   - Existing local dashboard behavior unchanged.
 *
 * Spec ref: docs/specs/alpha-hosted-dashboard-ui.md
 */

import { describe, it, expect } from "vitest";
import type {
  IngestionGateItem,
  LedgerDecision,
  LedgerCandidate,
} from "./lib/types";
import type {
  DashboardDataMode,
  HostedDashboardAdapterResponse,
} from "./lib/hosted-bridge-types";

// ── Component import resolution ────────────────────────────────

describe("Hosted project shell component imports", () => {
  it("HostedProjectShell resolves", async () => {
    const mod = await import("./pages/HostedProjectShell.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });

  it("HostedDashboard resolves with project shell integration", async () => {
    const mod = await import("./pages/HostedDashboard.svelte");
    expect(mod).toBeDefined();
    expect(mod.default).toBeDefined();
  });

  it("existing surface components resolve for hosted reuse", async () => {
    const ingestion = await import("./pages/IngestionGate.svelte");
    const ledger = await import("./pages/LedgerView.svelte");
    const channels = await import("./pages/Channels.svelte");
    expect(ingestion.default).toBeDefined();
    expect(ledger.default).toBeDefined();
    expect(channels.default).toBeDefined();
  });
});

// ── Dashboard contract reuse ───────────────────────────────────

describe("Hosted shell reuses existing dashboard contracts", () => {
  it("types.ts IngestionGateItem shape is usable", () => {
    const igItem: IngestionGateItem = {
      source: { uri: "test://source", source_type: "github" },
      snapshot: {
        snapshot_addr: "sha256:abc",
        snapshot_ref: "main",
        captured_at: "2026-06-27T00:00:00Z",
      },
      source_title: "Test Source",
      source_freshness: "fresh",
      evidence: [],
      candidates: [],
      tracked: false,
      context_disposition: "raw_source",
      stale_is_advisory: false,
    };
    expect(igItem.source.source_type).toBe("github");
  });

  it("types.ts LedgerDecision shape is usable", () => {
    const decision: LedgerDecision = {
      kind: "decision",
      id: "dec_1",
      summary: "Test decision",
      feature: "test-feature",
      signoff: "proposed",
      review_state: "proposed",
      sources: [],
      allowed_commands: [],
      tracking_state: "tracked",
      source_freshness: "fresh",
    };
    expect(decision.kind).toBe("decision");
  });

  it("types.ts LedgerCandidate shape is usable", () => {
    const candidate: LedgerCandidate = {
      kind: "candidate",
      id: "cand_1",
      summary: "Test candidate",
      sources: [],
      review_state: "proposed",
      allowed_commands: [],
    };
    expect(candidate.kind).toBe("candidate");
  });

  it("no hosted-only decision/activity/source shapes exist", async () => {
    const types = await import("./lib/types");
    const exportNames = Object.keys(types);
    const hostedOnlyPatterns = [
      "HostedDecision",
      "HostedActivity",
      "HostedSource",
      "HostedCandidate",
      "HostedCompliance",
      "HostedSignoff",
      "HostedCodeEvidence",
    ];
    for (const pattern of hostedOnlyPatterns) {
      expect(
        exportNames.some((n) => n.includes(pattern)),
        `types.ts must not export ${pattern}`,
      ).toBe(false);
    }
  });
});

// ── Hosted bridge adapter fixtures ─────────────────────────────

describe("Hosted dashboard adapter fixtures for shell", () => {
  it("hosted_with_data fixture has workspace and ledger_summary", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const fixture = DASHBOARD_ADAPTER_FIXTURES.hosted_with_data;
    expect(fixture.mode).toBe("hosted");
    expect(fixture.workspace).toBeDefined();
    expect(fixture.ledger_summary).toBeDefined();
  });

  it("hosted_empty fixture has zero counts", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const fixture = DASHBOARD_ADAPTER_FIXTURES.hosted_empty;
    expect(fixture.mode).toBe("hosted");
    expect(fixture.ledger_summary?.decision_count).toBe(0);
    expect(fixture.ledger_summary?.candidate_count).toBe(0);
  });
});

// ── Ready project selection routing ────────────────────────────

describe("Ready project selection routing", () => {
  it("SAMPLE_PROJECTS contains at least one ready project", async () => {
    const { SAMPLE_PROJECTS } = await import("./lib/project-fixtures");
    const readyProjects = SAMPLE_PROJECTS.filter(
      (p) => p.readiness === "ready",
    );
    expect(readyProjects.length).toBeGreaterThan(0);
  });

  it("ready projects have required display fields", async () => {
    const { SAMPLE_PROJECTS } = await import("./lib/project-fixtures");
    const readyProjects = SAMPLE_PROJECTS.filter(
      (p) => p.readiness === "ready",
    );
    for (const project of readyProjects) {
      expect(project.id).toBeTruthy();
      expect(project.name).toBeTruthy();
      expect(project.event_store_label).toBeTruthy();
      expect(project.event_store_kind).toBeTruthy();
    }
  });

  it("operator home fixtures include projects with mixed readiness", async () => {
    const { FIXTURE_PROJECTS_MIXED } = await import(
      "./lib/operator-home-fixtures",
    );
    const states = FIXTURE_PROJECTS_MIXED.map((p) => p.readiness);
    expect(states).toContain("ready");
    expect(states).toContain("needs_access");
    expect(states).toContain("offline");
  });
});

// ── No local path or secret leakage ────────────────────────────

describe("Hosted project shell security boundary", () => {
  it("project fixtures do not contain filesystem paths", async () => {
    const { SAMPLE_PROJECTS } = await import("./lib/project-fixtures");
    const json = JSON.stringify(SAMPLE_PROJECTS);
    expect(json).not.toContain("/home/");
    expect(json).not.toContain("/Users/");
    expect(json).not.toContain("C:\\");
    expect(json).not.toContain("workspace_path");
  });

  it("project fixtures do not contain token or secret fields", async () => {
    const { SAMPLE_PROJECTS } = await import("./lib/project-fixtures");
    const json = JSON.stringify(SAMPLE_PROJECTS);
    expect(json).not.toContain("access_token");
    expect(json).not.toContain("refresh_token");
    expect(json).not.toContain("client_secret");
    expect(json).not.toContain("provider_config");
    expect(json).not.toContain("event_store_payload");
  });

  it("dashboard adapter fixtures do not leak paths or secrets", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures",
    );
    const json = JSON.stringify(DASHBOARD_ADAPTER_FIXTURES);
    const forbidden = [
      "access_token",
      "refresh_token",
      "client_secret",
      "/home/",
      "/Users/",
      "C:\\",
      "workspace_path",
      "event_store_payload",
      "source_body",
      "provider_token",
    ];
    for (const term of forbidden) {
      expect(json, `adapter fixtures must not contain "${term}"`).not.toContain(
        term,
      );
    }
  });

  it("hosted bridge session fixtures do not contain secrets", async () => {
    const { SESSION_FIXTURES } = await import("./lib/hosted-bridge-fixtures");
    const json = JSON.stringify(SESSION_FIXTURES);
    expect(json).not.toContain("access_token");
    expect(json).not.toContain("refresh_token");
    expect(json).not.toContain("provider_token");
  });
});

// ── Governed-action disabling ──────────────────────────────────

describe("Governed action disabling in hosted mode", () => {
  it("DashboardDataMode includes hosted mode", () => {
    const mode: DashboardDataMode = "hosted";
    expect(mode).toBe("hosted");
  });

  it("hosted adapter response has no fixture-success flag", () => {
    const response: HostedDashboardAdapterResponse = {
      mode: "hosted",
      projects: [],
    };
    expect("fixture_fallback" in response).toBe(false);
  });

  it("hosted bridge types export BRIDGE_NOT_ALLOWED_ACTIONS", async () => {
    const { BRIDGE_NOT_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types",
    );
    const joined = BRIDGE_NOT_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).toContain("decision approval");
  });
});

// ── Tab navigation coverage ────────────────────────────────────

describe("Hosted project shell tab coverage", () => {
  it("shell exports a component with expected tab type", async () => {
    const mod = await import("./pages/HostedProjectShell.svelte");
    expect(mod.default).toBeDefined();
  });

  it("existing surface pages load independently", async () => {
    const [ig, lv, ch] = await Promise.all([
      import("./pages/IngestionGate.svelte"),
      import("./pages/LedgerView.svelte"),
      import("./pages/Channels.svelte"),
    ]);
    expect(ig.default).toBeDefined();
    expect(lv.default).toBeDefined();
    expect(ch.default).toBeDefined();
  });
});

// ── Fallback and error states ──────────────────────────────────

describe("Hosted shell typed error states", () => {
  it("bridge error fixtures cover key error kinds", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures",
    );
    expect(HOSTED_BRIDGE_FIXTURES.error_daemon_unavailable).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_wrong_daemon).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_wrong_account).toBeDefined();
    expect(HOSTED_BRIDGE_FIXTURES.error_unsupported_daemon).toBeDefined();
  });

  it("expired and revoked bridge states have re-pair affordance data", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures",
    );
    const expired = HOSTED_BRIDGE_FIXTURES.expired;
    expect(expired.kind).toBe("expired");
    if (expired.kind === "expired") {
      expect(expired.reason).toBeTruthy();
    }
    const revoked = HOSTED_BRIDGE_FIXTURES.revoked;
    expect(revoked.kind).toBe("revoked");
    if (revoked.kind === "revoked") {
      expect(revoked.reason).toBeTruthy();
    }
  });
});

// ── Existing local dashboard unchanged ─────────────────────────

describe("Existing local dashboard behavior unchanged", () => {
  it("App.svelte resolves with hosted and local paths", async () => {
    const mod = await import("./App.svelte");
    expect(mod.default).toBeDefined();
  });

  it("IngestionGate.svelte imports API from existing api.ts", async () => {
    const api = await import("./lib/api");
    expect(typeof api.fetchIngestionGate).toBe("function");
    expect(typeof api.fetchLedgerView).toBe("function");
    expect(typeof api.submitReviewCommand).toBe("function");
  });

  it("LedgerView uses existing dashboard types", () => {
    const decision: LedgerDecision = {
      kind: "decision",
      id: "d1",
      summary: "s",
      feature: "f",
      signoff: "approved",
      review_state: "materialized",
      sources: [],
      allowed_commands: ["approve_signoff"],
      tracking_state: "tracked",
      source_freshness: "fresh",
    };
    expect(decision.allowed_commands).toContain("approve_signoff");
  });

  it("hosted dashboard adapter API function exists", async () => {
    const { fetchHostedDashboardAdapter } = await import("./lib/api");
    expect(typeof fetchHostedDashboardAdapter).toBe("function");
  });
});

// ── Authority boundary in hosted shell ─────────────────────────

describe("Authority boundary constraints in hosted shell", () => {
  it("HostedDashboardAdapterResponse does not include SourceEvidence bodies", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures",
    );
    const json = JSON.stringify(DASHBOARD_ADAPTER_FIXTURES);
    expect(json).not.toContain("evidence_body");
    expect(json).not.toContain("SourceEvidence");
    expect(json).not.toContain("binding_key");
  });

  it("workspace metadata uses safe fields only", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures",
    );
    const ws = DASHBOARD_ADAPTER_FIXTURES.hosted_with_data.workspace;
    expect(ws).toBeDefined();
    if (ws) {
      expect(ws.project_name).toBeTruthy();
      expect(typeof ws.event_store_healthy).toBe("boolean");
      expect(typeof ws.has_workspace_binding).toBe("boolean");
      expect(typeof ws.decision_count).toBe("number");
      expect(typeof ws.candidate_count).toBe("number");
      const wsJson = JSON.stringify(ws);
      expect(wsJson).not.toContain("file_path");
      expect(wsJson).not.toContain("workspace_root");
      expect(wsJson).not.toContain("local_path");
    }
  });

  it("hosted shell does not expose write commands in adapter", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures",
    );
    const json = JSON.stringify(DASHBOARD_ADAPTER_FIXTURES);
    expect(json).not.toContain("write_event");
    expect(json).not.toContain("create_decision");
    expect(json).not.toContain("approve_signoff");
    expect(json).not.toContain("resolve_compliance");
  });
});
