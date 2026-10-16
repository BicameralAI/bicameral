/** Fixture data for the decision.find_code_impact UI flow.
 *
 * Mirrors the Rust API fixture shapes from bicameral-api/src/fixtures.rs.
 * Used for fixture-backed rendering and component tests.
 */

import type { FindCodeImpactResult } from "./types";

export const FIXTURE_CODE_IMPACT_VERIFIED: FindCodeImpactResult = {
  status: "found",
  decision_id: "DEC-601",
  phase: "candidates_ready",
  graph_readiness: "ready",
  snapshot: {
    graph_snapshot_id: "gs-impact-001",
    repo_id: "repo-fixture",
    commit_sha: "a77b83a",
    index_version: "1",
    ref_name: "HEAD",
  },
  search_plan: {
    strategy: "source_context",
    queries_executed: 3,
    hint_seeded: true,
  },
  candidates: [
    {
      target: "src/routes/healthz.rs",
      target_type: "file_path",
      file: "src/routes/healthz.rs",
      range: "10:1-25:1",
      symbol: "HealthCheckHandler::handle",
      evidence_state: "verified",
      confidence: "high",
      reasons: [
        { code: "exact_symbol", detail: "Symbol matches Decision source context" },
        { code: "exact_path", detail: "File exists at snapshot a77b83a" },
      ],
      graph_snapshot_id: "gs-impact-001",
      validated_sha: "a77b83a",
    },
  ],
  caveats: [],
  next_actions: [
    {
      action: "binding_create",
      description: "Create binding evidence for verified candidate",
      command_hint: "binding.create",
    },
  ],
};

export const FIXTURE_CODE_IMPACT_APPROXIMATE: FindCodeImpactResult = {
  status: "needs_human_selection",
  decision_id: "DEC-602",
  phase: "candidates_ready",
  graph_readiness: "ready",
  snapshot: {
    graph_snapshot_id: "gs-approx-001",
    commit_sha: "def456",
    index_version: "1",
  },
  search_plan: {
    strategy: "source_context",
    queries_executed: 2,
    hint_seeded: false,
  },
  candidates: [
    {
      target: "src/conn_a.rs",
      target_type: "file_path",
      file: "src/conn_a.rs",
      evidence_state: "approximate_candidate",
      confidence: "medium",
      reasons: [
        { code: "lexical_match", detail: "Lexical match; scope insufficient for exact proof" },
      ],
    },
    {
      target: "src/conn_b.rs",
      target_type: "file_path",
      file: "src/conn_b.rs",
      evidence_state: "approximate_candidate",
      confidence: "low",
      reasons: [
        { code: "lexical_match", detail: "Weaker lexical match" },
      ],
    },
  ],
  caveats: [
    { kind: "approximate_candidates", detail: "All candidates are approximate; human selection required" },
  ],
  next_actions: [
    { action: "choose_candidate", description: "Select the correct candidate for binding" },
    { action: "refresh_graph", description: "Refresh graph for better precision", command_hint: "graph.refresh" },
  ],
};

export const FIXTURE_CODE_IMPACT_GRAPH_UNAVAILABLE: FindCodeImpactResult = {
  status: "graph_unavailable",
  decision_id: "DEC-603",
  phase: "discovery_requested",
  graph_readiness: "unavailable",
  candidates: [],
  caveats: [],
  next_actions: [
    { action: "build_index", description: "Build or configure a code graph for this workspace" },
  ],
};

export const FIXTURE_CODE_IMPACT_GRAPH_STALE: FindCodeImpactResult = {
  status: "graph_stale",
  decision_id: "DEC-604",
  phase: "candidates_ready",
  graph_readiness: "stale",
  snapshot: {
    graph_snapshot_id: "gs-stale-001",
    commit_sha: "old123",
    index_version: "1",
    ref_name: "HEAD~5",
  },
  candidates: [
    {
      target: "src/stale_target.rs",
      target_type: "file_path",
      file: "src/stale_target.rs",
      range: "1:1-10:1",
      evidence_state: "unknown_stale",
      confidence: "medium",
      reasons: [
        { code: "exact_path", detail: "Path found but graph snapshot is behind HEAD" },
      ],
      graph_snapshot_id: "gs-stale-001",
      validated_sha: "old123",
    },
  ],
  caveats: [
    { kind: "graph_stale", detail: "Graph snapshot is behind current HEAD; results may be outdated" },
  ],
  next_actions: [
    { action: "refresh_graph", description: "Refresh graph to current HEAD", command_hint: "graph.refresh" },
  ],
};

export const FIXTURE_CODE_IMPACT_NOT_FOUND: FindCodeImpactResult = {
  status: "not_found",
  decision_id: "DEC-605",
  phase: "candidates_ready",
  graph_readiness: "ready",
  snapshot: {
    graph_snapshot_id: "gs-nf-001",
    commit_sha: "abc999",
    index_version: "1",
    ref_name: "HEAD",
  },
  search_plan: {
    strategy: "source_context",
    queries_executed: 4,
    hint_seeded: false,
  },
  candidates: [],
  caveats: [
    { kind: "limited_source_context", detail: "Decision source context may be too abstract for code location" },
  ],
  next_actions: [
    { action: "refine_hint", description: "Provide additional file/symbol hints to narrow the search" },
  ],
};

export const FIXTURE_CODE_IMPACT_UNSUPPORTED: FindCodeImpactResult = {
  status: "unsupported",
  decision_id: "DEC-606",
  phase: "discovery_requested",
  graph_readiness: "ready",
  candidates: [],
  caveats: [],
  next_actions: [],
};

export const FIXTURE_CODE_IMPACT_NEEDS_NARROWING: FindCodeImpactResult = {
  status: "needs_human_selection",
  decision_id: "DEC-607",
  phase: "candidates_ready",
  graph_readiness: "ready",
  snapshot: {
    graph_snapshot_id: "gs-narrow-001",
    commit_sha: "nar789",
    index_version: "1",
    ref_name: "HEAD",
  },
  search_plan: {
    strategy: "source_context",
    queries_executed: 5,
    hint_seeded: false,
  },
  candidates: [
    {
      target: "src/auth/login.rs",
      target_type: "symbol",
      file: "src/auth/login.rs",
      range: "20:1-45:1",
      symbol: "AuthService::login",
      evidence_state: "approximate_candidate",
      confidence: "medium",
      reasons: [
        { code: "fuzzy_match", detail: "Fuzzy match against decision keywords" },
      ],
      graph_snapshot_id: "gs-narrow-001",
      validated_sha: "nar789",
    },
    {
      target: "src/auth/session.rs",
      target_type: "symbol",
      file: "src/auth/session.rs",
      range: "5:1-30:1",
      symbol: "SessionManager::create",
      evidence_state: "approximate_candidate",
      confidence: "medium",
      reasons: [
        { code: "fuzzy_match", detail: "Fuzzy match against decision keywords" },
      ],
      graph_snapshot_id: "gs-narrow-001",
      validated_sha: "nar789",
    },
    {
      target: "src/auth/token.rs",
      target_type: "file_path",
      file: "src/auth/token.rs",
      evidence_state: "approximate_candidate",
      confidence: "low",
      reasons: [
        { code: "lexical_match", detail: "Weak lexical match" },
      ],
    },
  ],
  caveats: [
    { kind: "approximate_candidates", detail: "Multiple approximate candidates; narrow with hints" },
    { kind: "truncated", detail: "Results limited to 3 of 7 candidates" },
  ],
  next_actions: [
    { action: "refine_hint", description: "Add file or symbol hints to narrow candidates" },
    { action: "choose_candidate", description: "Select the correct candidate from the list" },
  ],
};

/** All fixture variants for iteration/testing. */
export const ALL_CODE_IMPACT_FIXTURES: Record<string, FindCodeImpactResult> = {
  verified: FIXTURE_CODE_IMPACT_VERIFIED,
  approximate: FIXTURE_CODE_IMPACT_APPROXIMATE,
  graph_unavailable: FIXTURE_CODE_IMPACT_GRAPH_UNAVAILABLE,
  graph_stale: FIXTURE_CODE_IMPACT_GRAPH_STALE,
  not_found: FIXTURE_CODE_IMPACT_NOT_FOUND,
  unsupported: FIXTURE_CODE_IMPACT_UNSUPPORTED,
  needs_narrowing: FIXTURE_CODE_IMPACT_NEEDS_NARROWING,
};
