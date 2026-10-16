/** Fixture data for graph readiness and evidence refresh UI flow.
 *
 * Mirrors the Rust API shapes from bicameral-api/src/evidence.rs.
 * Used for fixture-backed rendering and component tests.
 *
 * UI copy avoids: compliant, noncompliant, safe, blocked, violates, no-conflict.
 */

import type {
  GraphReadinessView,
  EvidenceRefreshOutcome,
} from "./types";

// ── GraphReadinessView fixtures ────────────────────────────────

export const FIXTURE_GRAPH_READY: GraphReadinessView = {
  readiness: "ready",
  graph_snapshot_id: "gs-abc123",
  scope_coverage: "full",
};

export const FIXTURE_GRAPH_READY_PARTIAL: GraphReadinessView = {
  readiness: "ready",
  graph_snapshot_id: "gs-partial-001",
  detail: "Indexed Rust and TypeScript files only",
  scope_coverage: "partial",
};

export const FIXTURE_GRAPH_INDEXING: GraphReadinessView = {
  readiness: "indexing",
  detail: "Graph snapshot build in progress",
};

export const FIXTURE_GRAPH_STALE: GraphReadinessView = {
  readiness: "stale",
  graph_snapshot_id: "gs-old-001",
  detail: "Index is 5 commits behind HEAD",
};

export const FIXTURE_GRAPH_FAILED: GraphReadinessView = {
  readiness: "failed",
  detail: "Indexing failed: parse error in src/legacy.rs",
};

export const FIXTURE_GRAPH_NOT_CONFIGURED: GraphReadinessView = {
  readiness: "not_configured",
  detail: "No graph provider configured for this workspace",
};

// ── EvidenceRefreshOutcome fixtures ────────────────────────────

export const FIXTURE_REFRESH_REFRESHED: EvidenceRefreshOutcome = {
  decision_id: "DEC-701",
  status: "refreshed",
  binding_results: [
    { binding_id: "BIND-001", currentness: "current" },
    { binding_id: "BIND-002", currentness: "stale", detail: "File modified since last check" },
  ],
};

export const FIXTURE_REFRESH_INELIGIBLE: EvidenceRefreshOutcome = {
  decision_id: "DEC-702",
  status: "ineligible",
  binding_results: [],
  detail: "Decision is not tracked or has no bindings",
};

export const FIXTURE_REFRESH_UNSUPPORTED: EvidenceRefreshOutcome = {
  decision_id: "DEC-703",
  status: "unsupported",
  binding_results: [],
  detail: "Graph provider not available for this workspace",
};

export const FIXTURE_REFRESH_NOT_FOUND: EvidenceRefreshOutcome = {
  decision_id: "DEC-704",
  status: "not_found",
  binding_results: [],
  detail: "Decision not found in the ledger",
};

// ── Aggregate collections ──────────────────────────────────────

export const ALL_GRAPH_READINESS_FIXTURES: Record<string, GraphReadinessView> = {
  ready: FIXTURE_GRAPH_READY,
  ready_partial: FIXTURE_GRAPH_READY_PARTIAL,
  indexing: FIXTURE_GRAPH_INDEXING,
  stale: FIXTURE_GRAPH_STALE,
  failed: FIXTURE_GRAPH_FAILED,
  not_configured: FIXTURE_GRAPH_NOT_CONFIGURED,
};

export const ALL_REFRESH_FIXTURES: Record<string, EvidenceRefreshOutcome> = {
  refreshed: FIXTURE_REFRESH_REFRESHED,
  ineligible: FIXTURE_REFRESH_INELIGIBLE,
  unsupported: FIXTURE_REFRESH_UNSUPPORTED,
  not_found: FIXTURE_REFRESH_NOT_FOUND,
};
