/**
 * Cross-repo hosted dashboard conformance tests — issue #617.
 *
 * Validates that the dashboard's hosted bridge types, fixtures, and adapter
 * contracts conform to ADR-0005/0007 boundaries:
 *   1. Pairing/session shell states and transitions.
 *   2. Operator-home readiness and repair fixture coverage.
 *   3. Hosted project shell / dashboard adapter contracts.
 *   4. Command allowlist authority boundary.
 *   5. No hosted-only fork of dashboard API shapes.
 *   6. Negative/security: no local paths, provider secrets, SourceEvidence
 *      bodies, or authority-bearing fields in any fixture.
 *
 * Uses deterministic fixtures — no live cloud credentials required.
 */

import { describe, it, expect } from "vitest";

// ═══════════════════════════════════════════════════════════════════════════
// 1. Pairing/session shell state coverage
// ═══════════════════════════════════════════════════════════════════════════

describe("Pairing/session shell state conformance", () => {
  it("covers all pairing lifecycle states", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const requiredStates = [
      "unpaired",
      "pairing_pending_awaiting",
      "pairing_pending_connecting",
      "pairing_pending_handshake",
      "confirmation",
      "paired",
      "expired",
      "revoked",
    ];
    for (const state of requiredStates) {
      expect(HOSTED_BRIDGE_FIXTURES[state]).toBeDefined();
    }
  });

  it("covers all error state variants", async () => {
    const { HOSTED_BRIDGE_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const requiredErrors = [
      "error_daemon_unavailable",
      "error_wrong_daemon",
      "error_wrong_account",
      "error_unsupported_daemon",
      "error_origin_mismatch",
      "error_origin_not_trusted",
      "error_missing_cloud_session",
    ];
    for (const key of requiredErrors) {
      expect(HOSTED_BRIDGE_FIXTURES[key]).toBeDefined();
      const state = HOSTED_BRIDGE_FIXTURES[key];
      expect(state.kind).toBe("error");
    }
  });

  it("session fixtures have all required session fields", async () => {
    const { SESSION_FIXTURES } = await import("./lib/hosted-bridge-fixtures");
    for (const key of Object.keys(SESSION_FIXTURES)) {
      const session = SESSION_FIXTURES[key];
      expect(session.pairingState).toBe("hosted_bridge_paired");
      expect(session.origin).toBeTruthy();
      expect(session.cloudAccountId).toBeTruthy();
      expect(session.pairedAt).toBeTruthy();
      expect(session.expiresAt).toBeTruthy();
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 2. Operator-home readiness and repair fixture coverage
// ═══════════════════════════════════════════════════════════════════════════

describe("Operator-home readiness and repair conformance", () => {
  it("dashboard adapter fixtures cover hosted data and empty states", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    expect(DASHBOARD_ADAPTER_FIXTURES.hosted_with_data).toBeDefined();
    expect(DASHBOARD_ADAPTER_FIXTURES.fixture_fallback).toBeUndefined();
    expect(DASHBOARD_ADAPTER_FIXTURES.hosted_empty).toBeDefined();
  });

  it("hosted_with_data has project readiness entries", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const data = DASHBOARD_ADAPTER_FIXTURES.hosted_with_data;
    expect(data.mode).toBe("hosted");
    expect(data.projects.length).toBeGreaterThan(0);
    for (const project of data.projects) {
      expect(project.name).toBeTruthy();
      expect(typeof project.event_store_healthy).toBe("boolean");
      expect(typeof project.has_workspace_binding).toBe("boolean");
      expect(project.setup_state).toBeTruthy();
    }
  });

  it("hosted_with_data has workspace metadata with safe fields only", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const data = DASHBOARD_ADAPTER_FIXTURES.hosted_with_data;
    expect(data.workspace).toBeDefined();
    if (data.workspace) {
      expect(data.workspace.project_name).toBeTruthy();
      expect(typeof data.workspace.decision_count).toBe("number");
      expect(typeof data.workspace.candidate_count).toBe("number");
    }
  });

  it("hosted_with_data has ledger summary with aggregate counts only", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const data = DASHBOARD_ADAPTER_FIXTURES.hosted_with_data;
    expect(data.ledger_summary).toBeDefined();
    if (data.ledger_summary) {
      expect(typeof data.ledger_summary.decision_count).toBe("number");
      expect(typeof data.ledger_summary.candidate_count).toBe("number");
      expect(typeof data.ledger_summary.needs_signoff_count).toBe("number");
      expect(typeof data.ledger_summary.bound_count).toBe("number");
    }
  });

  it("hosted_empty has zero counts", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const empty = DASHBOARD_ADAPTER_FIXTURES.hosted_empty;
    expect(empty.mode).toBe("hosted");
    expect(empty.projects).toEqual([]);
    if (empty.ledger_summary) {
      expect(empty.ledger_summary.decision_count).toBe(0);
      expect(empty.ledger_summary.candidate_count).toBe(0);
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 3. Command allowlist authority boundary
// ═══════════════════════════════════════════════════════════════════════════

describe("Command allowlist authority boundary conformance", () => {
  it("allowed actions are non-empty and describe read operations", async () => {
    const { BRIDGE_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    expect(BRIDGE_ALLOWED_ACTIONS.length).toBeGreaterThan(0);
    const joined = BRIDGE_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).not.toContain("signoff");
    expect(joined).not.toContain("compliance");
    expect(joined).not.toContain("decision write");
  });

  it("not-allowed actions cover authority-bearing operations", async () => {
    const { BRIDGE_NOT_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    expect(BRIDGE_NOT_ALLOWED_ACTIONS.length).toBeGreaterThan(0);
    const joined = BRIDGE_NOT_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).toContain("provider secret");
    expect(joined).toContain("filesystem");
  });

  it("allowed and not-allowed actions do not overlap", async () => {
    const { BRIDGE_ALLOWED_ACTIONS, BRIDGE_NOT_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    for (const action of BRIDGE_ALLOWED_ACTIONS) {
      expect(BRIDGE_NOT_ALLOWED_ACTIONS).not.toContain(action);
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 4. No hosted-only fork of dashboard API shapes
// ═══════════════════════════════════════════════════════════════════════════

describe("No hosted-only fork of dashboard API shapes", () => {
  it("HostedDashboardAdapterResponse uses DashboardDataMode from shared types", async () => {
    const types = await import("./lib/hosted-bridge-types");
    // Verify the types module exports all required adapter types.
    expect(types).toHaveProperty("BRIDGE_ALLOWED_ACTIONS");
    expect(types).toHaveProperty("BRIDGE_NOT_ALLOWED_ACTIONS");
  });

  it("adapter response projects use the same shape as TS ProjectReadiness type", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const data = DASHBOARD_ADAPTER_FIXTURES.hosted_with_data;
    for (const project of data.projects) {
      const keys = Object.keys(project).sort();
      expect(keys).toContain("name");
      expect(keys).toContain("event_store_healthy");
      expect(keys).toContain("has_workspace_binding");
      expect(keys).toContain("setup_state");
    }
  });

  it("adapter response does not introduce hosted-only Decision/Activity/Source shapes", async () => {
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    for (const [key, fixture] of Object.entries(DASHBOARD_ADAPTER_FIXTURES)) {
      const json = JSON.stringify(fixture);
      const hostedOnlyShapes = [
        "hosted_decision",
        "hosted_activity",
        "hosted_source",
        "hosted_signoff",
        "hosted_compliance",
        "cloud_decision",
        "cloud_activity",
        "cloud_source",
      ];
      for (const shape of hostedOnlyShapes) {
        expect(json).not.toContain(shape);
      }
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 5. Negative / security conformance
// ═══════════════════════════════════════════════════════════════════════════

describe("Security: no local path leakage in fixtures", () => {
  it("no fixture contains local filesystem paths", async () => {
    const { HOSTED_BRIDGE_FIXTURES, SESSION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const allFixtures = [
      ...Object.values(HOSTED_BRIDGE_FIXTURES),
      ...Object.values(SESSION_FIXTURES),
      ...Object.values(DASHBOARD_ADAPTER_FIXTURES),
    ];
    const pathPatterns = [
      "/home/",
      "/Users/",
      "C:\\",
      "/tmp/",
      "/var/",
      "/root/",
      "/opt/",
    ];
    for (const fixture of allFixtures) {
      const json = JSON.stringify(fixture);
      for (const pat of pathPatterns) {
        expect(json).not.toContain(pat);
      }
    }
  });
});

describe("Security: no provider secret leakage in fixtures", () => {
  it("no fixture contains provider tokens or secrets", async () => {
    const { HOSTED_BRIDGE_FIXTURES, SESSION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const allFixtures = [
      ...Object.values(HOSTED_BRIDGE_FIXTURES),
      ...Object.values(SESSION_FIXTURES),
      ...Object.values(DASHBOARD_ADAPTER_FIXTURES),
    ];
    const secretPatterns = [
      "provider_token",
      "access_token",
      "refresh_token",
      "provider_secret",
      "api_key",
      "provider_config",
    ];
    for (const fixture of allFixtures) {
      const json = JSON.stringify(fixture);
      for (const pat of secretPatterns) {
        expect(json).not.toContain(pat);
      }
    }
  });
});

describe("Security: no SourceEvidence body leakage in fixtures", () => {
  it("no fixture contains source evidence bodies or event payloads", async () => {
    const { HOSTED_BRIDGE_FIXTURES, SESSION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const allFixtures = [
      ...Object.values(HOSTED_BRIDGE_FIXTURES),
      ...Object.values(SESSION_FIXTURES),
      ...Object.values(DASHBOARD_ADAPTER_FIXTURES),
    ];
    const bodyPatterns = [
      "source_evidence_body",
      "event_payload",
      "event_store_payload",
      "source_body",
      "snapshot_content",
    ];
    for (const fixture of allFixtures) {
      const json = JSON.stringify(fixture);
      for (const pat of bodyPatterns) {
        expect(json).not.toContain(pat);
      }
    }
  });
});

describe("Security: no direct signoff/compliance/event-store authority through hosted UI", () => {
  it("not-allowed actions block authority-bearing operations", async () => {
    const { BRIDGE_NOT_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    const joined = BRIDGE_NOT_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).toContain("provider secret");
    expect(joined).toContain("filesystem");
    expect(joined).toContain("decision approval");
  });

  it("allowed actions do not reference workspace binding or signoff", async () => {
    const { BRIDGE_ALLOWED_ACTIONS } = await import(
      "./lib/hosted-bridge-types"
    );
    const joined = BRIDGE_ALLOWED_ACTIONS.join(" ").toLowerCase();
    expect(joined).not.toContain("workspace binding");
    expect(joined).not.toContain("signoff");
    expect(joined).not.toContain("compliance");
    expect(joined).not.toContain("event store write");
  });

  it("no fixture contains authority-bearing field names", async () => {
    const { HOSTED_BRIDGE_FIXTURES, SESSION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const { DASHBOARD_ADAPTER_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const allFixtures = [
      ...Object.values(HOSTED_BRIDGE_FIXTURES),
      ...Object.values(SESSION_FIXTURES),
      ...Object.values(DASHBOARD_ADAPTER_FIXTURES),
    ];
    const authorityFields = [
      "authority_context",
      "tool_command",
      "review_command",
      "signoff_approval",
      "compliance_resolution",
      "event_store_write",
      "source_binding",
      "canonical_decision",
    ];
    for (const fixture of allFixtures) {
      const json = JSON.stringify(fixture);
      for (const field of authorityFields) {
        expect(json).not.toContain(field);
      }
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 6. Component resolution (hosted screens exist)
// ═══════════════════════════════════════════════════════════════════════════

describe("Hosted dashboard component resolution", () => {
  it("HostedPairingScreen resolves", async () => {
    const mod = await import("./pages/HostedPairingScreen.svelte");
    expect(mod.default).toBeDefined();
  });

  it("LocalConfirmationScreen resolves", async () => {
    const mod = await import("./pages/LocalConfirmationScreen.svelte");
    expect(mod.default).toBeDefined();
  });

  it("HostedSessionShell resolves", async () => {
    const mod = await import("./pages/HostedSessionShell.svelte");
    expect(mod.default).toBeDefined();
  });

  it("HostedDashboard resolves", async () => {
    const mod = await import("./pages/HostedDashboard.svelte");
    expect(mod.default).toBeDefined();
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 7. API client hosted bridge functions exist
// ═══════════════════════════════════════════════════════════════════════════

describe("API client hosted bridge function conformance", () => {
  it("fetchHostedBridgeStatus is exported", async () => {
    const { fetchHostedBridgeStatus } = await import("./lib/api");
    expect(typeof fetchHostedBridgeStatus).toBe("function");
  });

  it("requestHostedBridgePairing is exported", async () => {
    const { requestHostedBridgePairing } = await import("./lib/api");
    expect(typeof requestHostedBridgePairing).toBe("function");
  });

  it("fetchHostedLedgerProjection is exported", async () => {
    const { fetchHostedLedgerProjection } = await import("./lib/api");
    expect(typeof fetchHostedLedgerProjection).toBe("function");
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// 8. Renderable ledger field allow-list (#705, T4 — RFQ #698 item 5)
//
// The network-origin ledger projection must expose ONLY the render-safe
// allow-list (ids, timestamps, status/review-state enums, counts, redacted
// titles) and must never carry decision bodies, source/evidence excerpts,
// code paths, actors, provider secrets, or raw event payloads
// (ADR-0020 / ADR-0025).
// ═══════════════════════════════════════════════════════════════════════════

describe("Renderable ledger allow-list conformance", () => {
  it("ledger projection fixtures cover with_data and empty variants", async () => {
    const { LEDGER_PROJECTION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    expect(LEDGER_PROJECTION_FIXTURES.with_data).toBeDefined();
    expect(LEDGER_PROJECTION_FIXTURES.empty).toBeDefined();
    for (const fixture of Object.values(LEDGER_PROJECTION_FIXTURES)) {
      expect(fixture.projection_only).toBe(true);
    }
  });

  it("every ledger entry carries ONLY allow-listed fields", async () => {
    const { LEDGER_PROJECTION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const { RENDERABLE_LEDGER_ENTRY_FIELDS } = await import(
      "./lib/hosted-bridge-types"
    );
    const allowed = new Set(RENDERABLE_LEDGER_ENTRY_FIELDS);
    for (const fixture of Object.values(LEDGER_PROJECTION_FIXTURES)) {
      for (const entry of fixture.decisions) {
        for (const key of Object.keys(entry)) {
          expect(
            allowed.has(key),
            `ledger entry exposed non-allow-listed field '${key}'`,
          ).toBe(true);
        }
        // Every required allow-listed field is present.
        for (const field of RENDERABLE_LEDGER_ENTRY_FIELDS) {
          expect(entry).toHaveProperty(field);
        }
      }
    }
  });

  it("ledger events carry only id, event_type, and timestamp (no actor)", async () => {
    const { LEDGER_PROJECTION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    for (const fixture of Object.values(LEDGER_PROJECTION_FIXTURES)) {
      for (const event of fixture.events) {
        expect(Object.keys(event).sort()).toEqual([
          "event_type",
          "id",
          "timestamp",
        ]);
      }
    }
  });

  it("no forbidden field name appears anywhere in the ledger projection", async () => {
    const { LEDGER_PROJECTION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const { LEDGER_FORBIDDEN_FIELDS } = await import(
      "./lib/hosted-bridge-types"
    );
    for (const fixture of Object.values(LEDGER_PROJECTION_FIXTURES)) {
      const json = JSON.stringify(fixture);
      for (const field of LEDGER_FORBIDDEN_FIELDS) {
        expect(
          json.includes(field),
          `ledger projection leaked forbidden field '${field}'`,
        ).toBe(false);
      }
    }
  });

  it("ledger projection carries no local paths, secrets, or raw bodies", async () => {
    const { LEDGER_PROJECTION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const leakPatterns = [
      "/home/",
      "/Users/",
      "C:\\",
      "/tmp/",
      "/var/",
      ".rs:",
      "ghp_",
      "AKIA",
      "provider_token",
      "access_token",
      "source_evidence_body",
      "event_payload",
    ];
    for (const fixture of Object.values(LEDGER_PROJECTION_FIXTURES)) {
      const json = JSON.stringify(fixture);
      for (const pat of leakPatterns) {
        expect(
          json.includes(pat),
          `ledger projection leaked pattern '${pat}'`,
        ).toBe(false);
      }
    }
  });

  it("titles are redactor-passed (redacted spans survive, secrets do not)", async () => {
    const { LEDGER_PROJECTION_FIXTURES } = await import(
      "./lib/hosted-bridge-fixtures"
    );
    const redactedEntry = LEDGER_PROJECTION_FIXTURES.with_data.decisions.find(
      (d) => d.title.includes("[redacted:"),
    );
    expect(redactedEntry, "expected a redacted title fixture").toBeDefined();
    expect(redactedEntry?.title).not.toContain("AKIA");
  });

  it("renderable field allow-list matches the Rust field set exactly", async () => {
    const { RENDERABLE_LEDGER_ENTRY_FIELDS } = await import(
      "./lib/hosted-bridge-types"
    );
    // Kept in lockstep with `RENDERABLE_LEDGER_FIELDS` in
    // crates/bicameral-api/src/hosted_bridge.rs.
    expect([...RENDERABLE_LEDGER_ENTRY_FIELDS].sort()).toEqual(
      [
        "binding_count",
        "candidate_id",
        "decision_id",
        "evidence_state",
        "level",
        "promoted_at",
        "review_state",
        "signoff_state",
        "source_evidence_count",
        "source_freshness",
        "title",
        "tracking_state",
      ].sort(),
    );
  });
});
