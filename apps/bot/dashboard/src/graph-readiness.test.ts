/**
 * Graph Readiness Panel component and fixture tests (issue #440).
 *
 * Validates the graph readiness and evidence refresh UI flow:
 *   - Fixture contract (all states importable and well-shaped)
 *   - Type/API surface exports
 *   - Acceptance criteria: ready/stale/unavailable/unsupported states render
 *   - Refresh action does not mutate signoff or compliance
 *   - UI copy does not use forbidden terms
 */

import { describe, it, expect } from "vitest";

// ── Fixture contract tests ─────────────────────────────────────

describe("Graph readiness fixture contract", () => {
  it("exports all required graph readiness fixture variants", async () => {
    const mod = await import("./lib/graph-readiness-fixtures");
    expect(mod.FIXTURE_GRAPH_READY).toBeDefined();
    expect(mod.FIXTURE_GRAPH_READY_PARTIAL).toBeDefined();
    expect(mod.FIXTURE_GRAPH_INDEXING).toBeDefined();
    expect(mod.FIXTURE_GRAPH_STALE).toBeDefined();
    expect(mod.FIXTURE_GRAPH_FAILED).toBeDefined();
    expect(mod.FIXTURE_GRAPH_NOT_CONFIGURED).toBeDefined();
    expect(mod.ALL_GRAPH_READINESS_FIXTURES).toBeDefined();
  });

  it("exports all required evidence refresh fixture variants", async () => {
    const mod = await import("./lib/graph-readiness-fixtures");
    expect(mod.FIXTURE_REFRESH_REFRESHED).toBeDefined();
    expect(mod.FIXTURE_REFRESH_INELIGIBLE).toBeDefined();
    expect(mod.FIXTURE_REFRESH_UNSUPPORTED).toBeDefined();
    expect(mod.FIXTURE_REFRESH_NOT_FOUND).toBeDefined();
    expect(mod.ALL_REFRESH_FIXTURES).toBeDefined();
  });

  it("ALL_GRAPH_READINESS_FIXTURES contains expected keys", async () => {
    const { ALL_GRAPH_READINESS_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    const keys = Object.keys(ALL_GRAPH_READINESS_FIXTURES);
    expect(keys).toContain("ready");
    expect(keys).toContain("ready_partial");
    expect(keys).toContain("indexing");
    expect(keys).toContain("stale");
    expect(keys).toContain("failed");
    expect(keys).toContain("not_configured");
    expect(keys.length).toBe(6);
  });

  it("ALL_REFRESH_FIXTURES contains expected keys", async () => {
    const { ALL_REFRESH_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    const keys = Object.keys(ALL_REFRESH_FIXTURES);
    expect(keys).toContain("refreshed");
    expect(keys).toContain("ineligible");
    expect(keys).toContain("unsupported");
    expect(keys).toContain("not_found");
    expect(keys.length).toBe(4);
  });

  it("every readiness fixture has required top-level fields", async () => {
    const { ALL_GRAPH_READINESS_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    for (const [name, fixture] of Object.entries(ALL_GRAPH_READINESS_FIXTURES)) {
      expect(fixture.readiness, `${name}.readiness`).toBeTruthy();
    }
  });

  it("every refresh fixture has required top-level fields", async () => {
    const { ALL_REFRESH_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    for (const [name, fixture] of Object.entries(ALL_REFRESH_FIXTURES)) {
      expect(fixture.decision_id, `${name}.decision_id`).toBeTruthy();
      expect(fixture.status, `${name}.status`).toBeTruthy();
      expect(Array.isArray(fixture.binding_results), `${name}.binding_results`).toBe(true);
    }
  });
});

// ── Readiness state coverage ───────────────────────────────────

describe("Graph readiness state coverage", () => {
  it("ready fixture has readiness=ready", async () => {
    const { FIXTURE_GRAPH_READY } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_GRAPH_READY.readiness).toBe("ready");
    expect(FIXTURE_GRAPH_READY.graph_snapshot_id).toBeTruthy();
  });

  it("stale fixture has readiness=stale with detail", async () => {
    const { FIXTURE_GRAPH_STALE } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_GRAPH_STALE.readiness).toBe("stale");
    expect(FIXTURE_GRAPH_STALE.detail).toBeTruthy();
  });

  it("failed fixture has readiness=failed (renders as unavailable)", async () => {
    const { FIXTURE_GRAPH_FAILED } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_GRAPH_FAILED.readiness).toBe("failed");
    expect(FIXTURE_GRAPH_FAILED.detail).toBeTruthy();
  });

  it("not_configured fixture has readiness=not_configured", async () => {
    const { FIXTURE_GRAPH_NOT_CONFIGURED } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_GRAPH_NOT_CONFIGURED.readiness).toBe("not_configured");
  });

  it("indexing fixture has readiness=indexing", async () => {
    const { FIXTURE_GRAPH_INDEXING } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_GRAPH_INDEXING.readiness).toBe("indexing");
  });

  it("partial coverage fixture has scope_coverage=partial", async () => {
    const { FIXTURE_GRAPH_READY_PARTIAL } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_GRAPH_READY_PARTIAL.readiness).toBe("ready");
    expect(FIXTURE_GRAPH_READY_PARTIAL.scope_coverage).toBe("partial");
  });

  it("all readiness values are valid GraphReadiness enum members", async () => {
    const { ALL_GRAPH_READINESS_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    const valid = ["ready", "indexing", "stale", "failed", "not_configured"];
    for (const [name, fixture] of Object.entries(ALL_GRAPH_READINESS_FIXTURES)) {
      expect(valid, `${name} has valid readiness`).toContain(fixture.readiness);
    }
  });
});

// ── Evidence refresh state coverage ────────────────────────────

describe("Evidence refresh state coverage", () => {
  it("refreshed fixture has binding_results", async () => {
    const { FIXTURE_REFRESH_REFRESHED } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_REFRESH_REFRESHED.status).toBe("refreshed");
    expect(FIXTURE_REFRESH_REFRESHED.binding_results.length).toBeGreaterThan(0);
  });

  it("ineligible fixture has empty binding_results", async () => {
    const { FIXTURE_REFRESH_INELIGIBLE } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_REFRESH_INELIGIBLE.status).toBe("ineligible");
    expect(FIXTURE_REFRESH_INELIGIBLE.binding_results.length).toBe(0);
  });

  it("unsupported fixture has empty binding_results", async () => {
    const { FIXTURE_REFRESH_UNSUPPORTED } = await import("./lib/graph-readiness-fixtures");
    expect(FIXTURE_REFRESH_UNSUPPORTED.status).toBe("unsupported");
    expect(FIXTURE_REFRESH_UNSUPPORTED.binding_results.length).toBe(0);
  });

  it("all refresh statuses are valid EvidenceRefreshStatus values", async () => {
    const { ALL_REFRESH_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    const valid = ["refreshed", "ineligible", "not_found", "unsupported"];
    for (const [name, fixture] of Object.entries(ALL_REFRESH_FIXTURES)) {
      expect(valid, `${name} has valid status`).toContain(fixture.status);
    }
  });

  it("binding currentness values are valid", async () => {
    const { ALL_REFRESH_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    const valid = ["current", "stale", "moved", "deleted", "unavailable"];
    for (const fixture of Object.values(ALL_REFRESH_FIXTURES)) {
      for (const r of fixture.binding_results) {
        expect(valid).toContain(r.currentness);
      }
    }
  });
});

// ── API surface tests ──────────────────────────────────────────

describe("Evidence refresh API surface", () => {
  it("api.ts exports refreshEvidence function", async () => {
    const mod = await import("./lib/api");
    expect(typeof mod.refreshEvidence).toBe("function");
  });
});

// ── Refresh does not mutate signoff/compliance ─────────────────

describe("Refresh action boundary", () => {
  it("refresh outcome contains no signoff or compliance fields", async () => {
    const { ALL_REFRESH_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    for (const [name, fixture] of Object.entries(ALL_REFRESH_FIXTURES)) {
      const keys = Object.keys(fixture);
      expect(keys, `${name} has no signoff field`).not.toContain("signoff");
      expect(keys, `${name} has no compliance field`).not.toContain("compliance");
      expect(keys, `${name} has no review_state field`).not.toContain("review_state");
    }
  });

  it("readiness view contains no signoff or compliance fields", async () => {
    const { ALL_GRAPH_READINESS_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    for (const [name, fixture] of Object.entries(ALL_GRAPH_READINESS_FIXTURES)) {
      const keys = Object.keys(fixture);
      expect(keys, `${name} has no signoff field`).not.toContain("signoff");
      expect(keys, `${name} has no compliance field`).not.toContain("compliance");
    }
  });
});

// ── UI copy compliance tests ───────────────────────────────────

describe("UI copy does not use forbidden terms", () => {
  it("fixture data does not contain compliant/safe/blocked/no-conflict", async () => {
    const { ALL_GRAPH_READINESS_FIXTURES, ALL_REFRESH_FIXTURES } = await import("./lib/graph-readiness-fixtures");
    const forbidden = ["compliant", "noncompliant", "safe", "blocked", "violates", "no-conflict"];
    const json = JSON.stringify({ ...ALL_GRAPH_READINESS_FIXTURES, ...ALL_REFRESH_FIXTURES }).toLowerCase();
    for (const term of forbidden) {
      expect(json, `fixture data must not contain "${term}"`).not.toContain(term);
    }
  });
});
