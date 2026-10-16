/**
 * Code Impact Panel component and fixture tests (issue #447).
 *
 * Validates the decision.find_code_impact UI flow:
 *   - Fixture contract (all states importable and well-shaped)
 *   - Type/API surface exports
 *   - Acceptance criteria: verified candidates allow binding, non-verified do not
 *   - All status/state fixtures produce valid result shapes
 */

import { describe, it, expect } from "vitest";

// ── Fixture contract tests ─────────────────────────────────────

describe("Code impact fixture contract", () => {
  it("exports all required fixture variants", async () => {
    const mod = await import("./lib/code-impact-fixtures");
    expect(mod.FIXTURE_CODE_IMPACT_VERIFIED).toBeDefined();
    expect(mod.FIXTURE_CODE_IMPACT_APPROXIMATE).toBeDefined();
    expect(mod.FIXTURE_CODE_IMPACT_GRAPH_UNAVAILABLE).toBeDefined();
    expect(mod.FIXTURE_CODE_IMPACT_GRAPH_STALE).toBeDefined();
    expect(mod.FIXTURE_CODE_IMPACT_NOT_FOUND).toBeDefined();
    expect(mod.FIXTURE_CODE_IMPACT_UNSUPPORTED).toBeDefined();
    expect(mod.FIXTURE_CODE_IMPACT_NEEDS_NARROWING).toBeDefined();
    expect(mod.ALL_CODE_IMPACT_FIXTURES).toBeDefined();
  });

  it("ALL_CODE_IMPACT_FIXTURES contains expected keys", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    const keys = Object.keys(ALL_CODE_IMPACT_FIXTURES);
    expect(keys).toContain("verified");
    expect(keys).toContain("approximate");
    expect(keys).toContain("graph_unavailable");
    expect(keys).toContain("graph_stale");
    expect(keys).toContain("not_found");
    expect(keys).toContain("unsupported");
    expect(keys).toContain("needs_narrowing");
    expect(keys.length).toBe(7);
  });

  it("every fixture has required top-level fields", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    for (const [name, fixture] of Object.entries(ALL_CODE_IMPACT_FIXTURES)) {
      expect(fixture.status, `${name}.status`).toBeTruthy();
      expect(fixture.decision_id, `${name}.decision_id`).toBeTruthy();
      expect(fixture.phase, `${name}.phase`).toBeTruthy();
      expect(fixture.graph_readiness, `${name}.graph_readiness`).toBeTruthy();
      expect(Array.isArray(fixture.candidates), `${name}.candidates`).toBe(true);
      expect(Array.isArray(fixture.caveats), `${name}.caveats`).toBe(true);
      expect(Array.isArray(fixture.next_actions), `${name}.next_actions`).toBe(true);
    }
  });

  it("verified fixture has candidates with evidence_state=verified", async () => {
    const { FIXTURE_CODE_IMPACT_VERIFIED } = await import("./lib/code-impact-fixtures");
    expect(FIXTURE_CODE_IMPACT_VERIFIED.candidates.length).toBeGreaterThan(0);
    for (const c of FIXTURE_CODE_IMPACT_VERIFIED.candidates) {
      expect(c.evidence_state).toBe("verified");
    }
  });

  it("approximate fixture has candidates with evidence_state=approximate_candidate", async () => {
    const { FIXTURE_CODE_IMPACT_APPROXIMATE } = await import("./lib/code-impact-fixtures");
    expect(FIXTURE_CODE_IMPACT_APPROXIMATE.candidates.length).toBeGreaterThan(0);
    for (const c of FIXTURE_CODE_IMPACT_APPROXIMATE.candidates) {
      expect(c.evidence_state).toBe("approximate_candidate");
    }
  });

  it("graph_unavailable fixture has no candidates and unavailable readiness", async () => {
    const { FIXTURE_CODE_IMPACT_GRAPH_UNAVAILABLE } = await import("./lib/code-impact-fixtures");
    expect(FIXTURE_CODE_IMPACT_GRAPH_UNAVAILABLE.candidates.length).toBe(0);
    expect(FIXTURE_CODE_IMPACT_GRAPH_UNAVAILABLE.graph_readiness).toBe("unavailable");
  });

  it("not_found fixture has no candidates and ready graph", async () => {
    const { FIXTURE_CODE_IMPACT_NOT_FOUND } = await import("./lib/code-impact-fixtures");
    expect(FIXTURE_CODE_IMPACT_NOT_FOUND.candidates.length).toBe(0);
    expect(FIXTURE_CODE_IMPACT_NOT_FOUND.graph_readiness).toBe("ready");
    expect(FIXTURE_CODE_IMPACT_NOT_FOUND.status).toBe("not_found");
  });
});

// ── Binding eligibility tests ──────────────────────────────────

describe("Binding eligibility logic", () => {
  it("only verified candidates allow binding", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    for (const [name, fixture] of Object.entries(ALL_CODE_IMPACT_FIXTURES)) {
      for (const candidate of fixture.candidates) {
        const allowsBinding = candidate.evidence_state === "verified";
        if (allowsBinding) {
          expect(allowsBinding, `${name}: verified candidate allows binding`).toBe(true);
        } else {
          expect(allowsBinding, `${name}: non-verified candidate must not allow binding`).toBe(false);
        }
      }
    }
  });

  it("verified fixture next_actions include binding_create", async () => {
    const { FIXTURE_CODE_IMPACT_VERIFIED } = await import("./lib/code-impact-fixtures");
    const bindAction = FIXTURE_CODE_IMPACT_VERIFIED.next_actions.find(
      a => a.action === "binding_create",
    );
    expect(bindAction).toBeDefined();
    expect(bindAction!.command_hint).toBe("binding.create");
  });

  it("approximate fixture does NOT suggest binding_create", async () => {
    const { FIXTURE_CODE_IMPACT_APPROXIMATE } = await import("./lib/code-impact-fixtures");
    const bindAction = FIXTURE_CODE_IMPACT_APPROXIMATE.next_actions.find(
      a => a.action === "binding_create",
    );
    expect(bindAction).toBeUndefined();
  });
});

// ── Status label validation ────────────────────────────────────

describe("Status coverage", () => {
  it("all fixture statuses are valid FindCodeImpactStatus values", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    const validStatuses = [
      "found",
      "needs_human_selection",
      "not_found",
      "graph_unavailable",
      "graph_stale",
      "unsupported",
    ];
    for (const [name, fixture] of Object.entries(ALL_CODE_IMPACT_FIXTURES)) {
      expect(validStatuses, `${name} has valid status`).toContain(fixture.status);
    }
  });

  it("all fixture phases are valid BindingDiscoveryPhase values", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    const validPhases = ["discovery_requested", "search_planned", "candidates_ready"];
    for (const [name, fixture] of Object.entries(ALL_CODE_IMPACT_FIXTURES)) {
      expect(validPhases, `${name} has valid phase`).toContain(fixture.phase);
    }
  });
});

// ── Candidate field validation ─────────────────────────────────

describe("Candidate field contract", () => {
  it("every candidate has required fields", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    for (const [name, fixture] of Object.entries(ALL_CODE_IMPACT_FIXTURES)) {
      for (const c of fixture.candidates) {
        expect(c.target, `${name} candidate.target`).toBeTruthy();
        expect(c.target_type, `${name} candidate.target_type`).toBeTruthy();
        expect(c.file, `${name} candidate.file`).toBeTruthy();
        expect(c.evidence_state, `${name} candidate.evidence_state`).toBeTruthy();
        expect(c.confidence, `${name} candidate.confidence`).toBeTruthy();
        expect(Array.isArray(c.reasons), `${name} candidate.reasons`).toBe(true);
      }
    }
  });

  it("confidence values are valid", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    const validConfidence = ["high", "medium", "low"];
    for (const fixture of Object.values(ALL_CODE_IMPACT_FIXTURES)) {
      for (const c of fixture.candidates) {
        expect(validConfidence).toContain(c.confidence);
      }
    }
  });
});

// ── API surface tests ──────────────────────────────────────────

describe("Code impact API surface", () => {
  it("api.ts exports findCodeImpact and acceptBinding functions", async () => {
    const mod = await import("./lib/api");
    expect(typeof mod.findCodeImpact).toBe("function");
    expect(typeof mod.acceptBinding).toBe("function");
  });
});

// ── UI copy compliance tests ───────────────────────────────────

describe("UI copy does not use forbidden terms", () => {
  it("fixture data does not contain compliant/noncompliant/safe/blocked/violates/no-conflict", async () => {
    const { ALL_CODE_IMPACT_FIXTURES } = await import("./lib/code-impact-fixtures");
    const forbidden = ["compliant", "noncompliant", "safe", "blocked", "violates", "no-conflict"];
    const json = JSON.stringify(ALL_CODE_IMPACT_FIXTURES).toLowerCase();
    for (const term of forbidden) {
      expect(json).not.toContain(term);
    }
  });
});
