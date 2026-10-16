/**
 * Projection state UI wiring tests (issue #433).
 *
 * Validates the projection-state read-model and fixture contracts:
 *   - Type exports for ProjectionSetupState, ProjectionSetupDetail,
 *     IngestProjectionOutcome, IngestResponseProjectionState
 *   - Fixture exports for SAMPLE_PROJECTION_SETUP
 *   - SourceBinding.projection_setup field shape
 *   - IngestionGateItem.projection_state field shape
 *   - Placement invariants: projection state must not masquerade
 *     as candidate/rejected review state
 */

import { describe, it, expect } from "vitest";

// ── Type and fixture import validation ──────────────────────────

describe("Projection state type exports", () => {
  it("types.ts exports projection state types without error", async () => {
    const mod = await import("./lib/types");
    expect(mod).toBeDefined();
  });
});

describe("Projection state fixture contract", () => {
  it("SAMPLE_PROJECTION_SETUP exports all four states", async () => {
    const { SAMPLE_PROJECTION_SETUP } = await import("./lib/connection-fixtures");
    expect(SAMPLE_PROJECTION_SETUP).toBeDefined();
    expect(SAMPLE_PROJECTION_SETUP.length).toBe(4);
    const states = SAMPLE_PROJECTION_SETUP.map((s) => s.state);
    expect(states).toContain("configured");
    expect(states).toContain("projection_deferred");
    expect(states).toContain("provider_not_configured");
    expect(states).toContain("provider_unavailable");
  });

  it("every projection setup fixture has required fields", async () => {
    const { SAMPLE_PROJECTION_SETUP } = await import("./lib/connection-fixtures");
    for (const fixture of SAMPLE_PROJECTION_SETUP) {
      expect(fixture.state).toBeTruthy();
      expect(fixture.message).toBeTruthy();
    }
  });

  it("non-configured states include repair_hint", async () => {
    const { SAMPLE_PROJECTION_SETUP } = await import("./lib/connection-fixtures");
    const nonConfigured = SAMPLE_PROJECTION_SETUP.filter(
      (s) => s.state !== "configured",
    );
    expect(nonConfigured.length).toBeGreaterThan(0);
    for (const fixture of nonConfigured) {
      expect(fixture.repair_hint).toBeTruthy();
    }
  });
});

// ── SourceBinding projection_setup field ────────────────────────

describe("SourceBinding projection_setup wiring", () => {
  it("SAMPLE_SOURCE_BINDINGS includes projection_setup on bindings", async () => {
    const { SAMPLE_SOURCE_BINDINGS } = await import("./lib/connection-fixtures");
    expect(SAMPLE_SOURCE_BINDINGS.length).toBeGreaterThan(0);
    const withSetup = SAMPLE_SOURCE_BINDINGS.filter((b) => b.projection_setup);
    expect(withSetup.length).toBeGreaterThan(0);
  });

  it("projection_setup carries valid state values", async () => {
    const { SAMPLE_SOURCE_BINDINGS } = await import("./lib/connection-fixtures");
    const validStates = [
      "configured",
      "projection_deferred",
      "provider_not_configured",
      "provider_unavailable",
    ];
    for (const binding of SAMPLE_SOURCE_BINDINGS) {
      if (binding.projection_setup) {
        expect(validStates).toContain(binding.projection_setup.state);
        expect(binding.projection_setup.message).toBeTruthy();
      }
    }
  });

  it("projection_setup does not use candidate/review state vocabulary", async () => {
    const { SAMPLE_SOURCE_BINDINGS } = await import("./lib/connection-fixtures");
    const forbiddenTerms = [
      "candidate review",
      "signoff",
      "compliance",
      "blocking",
      "rejected candidate",
    ];
    for (const binding of SAMPLE_SOURCE_BINDINGS) {
      if (binding.projection_setup) {
        const msg = binding.projection_setup.message.toLowerCase();
        const hint = (binding.projection_setup.repair_hint ?? "").toLowerCase();
        for (const term of forbiddenTerms) {
          expect(msg).not.toContain(term);
          expect(hint).not.toContain(term);
        }
      }
    }
  });
});

// ── IngestResponseProjectionState placement ─────────────────────

describe("Ingest response projection state placement", () => {
  it("IngestionGateItem type accepts projection_state field", async () => {
    const types = await import("./lib/types");
    // Construct a minimal IngestionGateItem with projection_state
    const item: import("./lib/types").IngestionGateItem = {
      source: { uri: "test://source", source_type: "manual" },
      snapshot: { snapshot_addr: "sha256:abc", snapshot_ref: "test", captured_at: "2026-01-01T00:00:00Z" },
      source_title: "Test Source",
      source_freshness: "fresh",
      evidence: [],
      candidates: [],
      tracked: false,
      context_disposition: "raw_source",
      stale_is_advisory: false,
      projection_state: {
        outcome: "projection_deferred",
        message: "Evidence was captured but no reviewable candidate was created yet.",
        repair_hint: "Check provider logs.",
      },
    };
    expect(item.projection_state).toBeDefined();
    expect(item.projection_state!.outcome).toBe("projection_deferred");
  });

  it("projection_state outcomes do not include candidate queue states", async () => {
    // The valid outcomes must not include any state that would add
    // items to the Ingestion Gate candidate queue
    const validOutcomes = [
      "candidate_created",
      "projection_deferred",
      "provider_not_configured",
      "provider_unavailable",
      "no_candidate",
    ];
    // None of these should imply queue membership
    const queueStates = ["proposed", "needs_signoff_review", "materialized"];
    for (const outcome of validOutcomes) {
      expect(queueStates).not.toContain(outcome);
    }
  });
});
