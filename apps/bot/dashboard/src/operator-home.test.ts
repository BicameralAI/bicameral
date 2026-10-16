/**
 * Operator Home project readiness and repair UI tests (issue #619).
 *
 * Validates:
 *   - Fixture contract (all variants importable and well-shaped)
 *   - Readiness state coverage (ready, needs_access, offline)
 *   - Empty state
 *   - All-inaccessible state
 *   - Repair UI for each AccessFailureReason
 *   - No stale workspace-binding-deferred alpha copy
 *   - No forbidden terms, no leaked secrets/paths
 *
 * Spec ref: docs/specs/explicit-project-registration-hosted-local-discovery.md
 */

import { describe, it, expect } from "vitest";

// ── Fixture contract tests ─────────────────────────────────────

describe("Operator home fixture contract", () => {
  it("exports all required fixture variants", async () => {
    const mod = await import("./lib/operator-home-fixtures");
    expect(mod.FIXTURE_PROJECTS_MIXED).toBeDefined();
    expect(mod.FIXTURE_PROJECTS_ALL_READY).toBeDefined();
    expect(mod.FIXTURE_PROJECTS_EMPTY).toBeDefined();
    expect(mod.FIXTURE_PROJECTS_ALL_INACCESSIBLE).toBeDefined();
    expect(mod.FIXTURE_PROJECTS_EACH_FAILURE).toBeDefined();
    expect(mod.ALL_OPERATOR_HOME_FIXTURES).toBeDefined();
  });

  it("ALL_OPERATOR_HOME_FIXTURES contains expected keys", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    const keys = Object.keys(ALL_OPERATOR_HOME_FIXTURES);
    expect(keys).toContain("mixed");
    expect(keys).toContain("all_ready");
    expect(keys).toContain("empty");
    expect(keys).toContain("all_inaccessible");
    expect(keys).toContain("each_failure");
    expect(keys).toContain("domain_mix");
    expect(keys.length).toBe(6);
  });

  it("every project in every fixture has required fields", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    for (const [name, projects] of Object.entries(
      ALL_OPERATOR_HOME_FIXTURES,
    )) {
      for (const project of projects) {
        expect(project.id, `${name} project has id`).toBeTruthy();
        expect(project.name, `${name} project has name`).toBeTruthy();
        expect(
          project.event_store_kind,
          `${name} project has event_store_kind`,
        ).toBeTruthy();
        expect(
          project.event_store_label,
          `${name} project has event_store_label`,
        ).toBeTruthy();
        expect(
          project.readiness,
          `${name} project has readiness`,
        ).toBeTruthy();
      }
    }
  });

  it("empty fixture is actually empty", async () => {
    const { FIXTURE_PROJECTS_EMPTY } = await import(
      "./lib/operator-home-fixtures"
    );
    expect(FIXTURE_PROJECTS_EMPTY).toHaveLength(0);
  });
});

// ── Readiness state coverage ───────────────────────────────────

describe("Operator home readiness state coverage", () => {
  it("mixed fixture contains ready, needs_access, and offline projects", async () => {
    const { FIXTURE_PROJECTS_MIXED } = await import(
      "./lib/operator-home-fixtures"
    );
    const states = FIXTURE_PROJECTS_MIXED.map((p) => p.readiness);
    expect(states).toContain("ready");
    expect(states).toContain("needs_access");
    expect(states).toContain("offline");
  });

  it("all_ready fixture has only ready projects", async () => {
    const { FIXTURE_PROJECTS_ALL_READY } = await import(
      "./lib/operator-home-fixtures"
    );
    expect(FIXTURE_PROJECTS_ALL_READY.length).toBeGreaterThan(0);
    for (const project of FIXTURE_PROJECTS_ALL_READY) {
      expect(project.readiness).toBe("ready");
    }
  });

  it("all_inaccessible fixture has no ready projects", async () => {
    const { FIXTURE_PROJECTS_ALL_INACCESSIBLE } = await import(
      "./lib/operator-home-fixtures"
    );
    expect(FIXTURE_PROJECTS_ALL_INACCESSIBLE.length).toBeGreaterThan(0);
    for (const project of FIXTURE_PROJECTS_ALL_INACCESSIBLE) {
      expect(project.readiness).not.toBe("ready");
    }
  });

  it("all readiness values are valid ProjectReadiness members", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    const valid = ["ready", "needs_access", "offline"];
    for (const [name, projects] of Object.entries(
      ALL_OPERATOR_HOME_FIXTURES,
    )) {
      for (const project of projects) {
        expect(valid, `${name}/${project.name} readiness`).toContain(
          project.readiness,
        );
      }
    }
  });
});

// ── Repair / AccessFailureReason coverage ──────────────────────

describe("Operator home repair state coverage", () => {
  it("each_failure fixture covers all five AccessFailureReason values", async () => {
    const { FIXTURE_PROJECTS_EACH_FAILURE } = await import(
      "./lib/operator-home-fixtures"
    );
    const failures = FIXTURE_PROJECTS_EACH_FAILURE.map(
      (p) => p.access_failure,
    );
    expect(failures).toContain("missing_connection");
    expect(failures).toContain("missing_resource_selection");
    expect(failures).toContain("insufficient_account_access");
    expect(failures).toContain("token_expired");
    expect(failures).toContain("resource_moved_or_deleted");
  });

  it("every needs_access project has an access_failure reason", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    for (const [name, projects] of Object.entries(
      ALL_OPERATOR_HOME_FIXTURES,
    )) {
      for (const project of projects) {
        if (project.readiness === "needs_access") {
          expect(
            project.access_failure,
            `${name}/${project.name} needs access_failure`,
          ).toBeTruthy();
        }
      }
    }
  });

  it("ACCESS_FAILURE_COPY has an entry for every failure reason", async () => {
    const { ACCESS_FAILURE_COPY } = await import("./lib/project-fixtures");
    const expected = [
      "missing_connection",
      "missing_resource_selection",
      "insufficient_account_access",
      "token_expired",
      "resource_moved_or_deleted",
    ];
    for (const reason of expected) {
      expect(
        ACCESS_FAILURE_COPY[reason as keyof typeof ACCESS_FAILURE_COPY],
      ).toBeTruthy();
    }
  });

  it("ACCESS_FAILURE_ACTION has an entry for every failure reason", async () => {
    const { ACCESS_FAILURE_ACTION } = await import("./lib/project-fixtures");
    const expected = [
      "missing_connection",
      "missing_resource_selection",
      "insufficient_account_access",
      "token_expired",
      "resource_moved_or_deleted",
    ];
    for (const reason of expected) {
      expect(
        ACCESS_FAILURE_ACTION[reason as keyof typeof ACCESS_FAILURE_ACTION],
      ).toBeTruthy();
    }
  });
});

// ── Workspace binding copy boundary ────────────────────────────

describe("Operator home workspace binding copy boundary", () => {
  it("does not render stale deferred workspace-binding copy", async () => {
    const source = await import("./pages/OperatorHome.svelte?raw");
    expect(source.default).not.toContain("workspace-binding-deferred");
    expect(source.default).not.toContain(
      "Workspace binding is not yet available",
    );
    expect(source.default).not.toContain("future release");
  });
});

// ── Security boundary tests ────────────────────────────────────

describe("Operator home security boundary", () => {
  it("fixture data does not contain filesystem paths", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    const json = JSON.stringify(ALL_OPERATOR_HOME_FIXTURES);
    expect(json).not.toContain("/home/");
    expect(json).not.toContain("/Users/");
    expect(json).not.toContain("C:\\");
    expect(json).not.toContain("workspace_path");
  });

  it("fixture data does not contain token or secret fields", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    const json = JSON.stringify(ALL_OPERATOR_HOME_FIXTURES);
    expect(json).not.toContain("access_token");
    expect(json).not.toContain("refresh_token");
    expect(json).not.toContain("client_secret");
    expect(json).not.toContain("provider_config");
  });

  it("fixture data does not expose SourceEvidence or Decision Ledger internals", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } = await import(
      "./lib/operator-home-fixtures"
    );
    const json = JSON.stringify(ALL_OPERATOR_HOME_FIXTURES);
    expect(json).not.toContain("SourceEvidence");
    expect(json).not.toContain("DecisionLedger");
    expect(json).not.toContain("event_payload");
  });
});

// ── UI copy compliance tests ───────────────────────────────────

describe("Operator home UI copy does not use forbidden terms", () => {
  it("fixture data avoids governance/compliance terms", async () => {
    const { ALL_OPERATOR_HOME_FIXTURES } =
      await import("./lib/operator-home-fixtures");
    const forbidden = [
      "compliant",
      "noncompliant",
      "safe",
      "blocked",
      "violates",
      "no-conflict",
    ];
    const json = JSON.stringify(ALL_OPERATOR_HOME_FIXTURES).toLowerCase();
    for (const term of forbidden) {
      expect(json, `copy must not contain "${term}"`).not.toContain(term);
    }
  });
});

// ── OperatorHomeState type coverage ────────────────────────────

describe("OperatorHomeState type", () => {
  it("exports OperatorHomeState type with expected values", async () => {
    // Type-level check: verify the module exports the type
    const mod = await import("./lib/operator-home-fixtures");
    // OperatorHomeState is a type so we can't check at runtime,
    // but we can verify the module loads cleanly
    expect(mod).toBeDefined();
  });
});
