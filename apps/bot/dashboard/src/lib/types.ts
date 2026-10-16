/** Dashboard API contract types — mirrors ADR-0006 data contracts. */

// ── Source provenance ────────────────────────────────────────────

export type SourceRef = {
  uri: string;
  source_type: string;
};

export type SourceSnapshotRef = {
  snapshot_addr: string;
  snapshot_ref: string;
  captured_at: string;
};

export type SourceEvidenceItem = {
  id: string;
  source_uri: string;
  snapshot_addr: string;
  snapshot_ref: string;
  pointer_type: string;
  pointer_value: string;
  excerpt: string;
  captured_at: string;
};

// ── Ingestion Gate ───────────────────────────────────────────────

export type DecisionCandidatePreview = {
  id?: string;
  summary: string;
  feature_hint?: string;
  evidence_refs: string[];
  extraction_confidence?: number;
  conflict_hint?: boolean;
  review_state?: ReviewLifecycleState;
  candidate_state?: CandidateState;
};

export type ContextDisposition =
  | "raw_source"
  | "candidate_context"
  | "tracked_decision";

export type IngestionGateItem = {
  source: SourceRef;
  snapshot: SourceSnapshotRef;
  source_title: string;
  source_freshness: "fresh" | "stale" | "offline" | "unknown";
  evidence: SourceEvidenceItem[];
  candidates: DecisionCandidatePreview[];
  tracked: boolean;
  context_disposition: ContextDisposition;
  stale_is_advisory: boolean;
  projection_state?: IngestResponseProjectionState;
};

// ── Ledger View ──────────────────────────────────────────────────

export type SignoffState =
  | "proposed"
  | "approved"
  | "rejected"
  | "collision_pending"
  | "superseded";

export type ReviewLifecycleState =
  | "advisory"
  | "proposed"
  | "needs_signoff_review"
  | "needs_grounding_review"
  | "needs_compliance_review"
  | "collision_pending"
  | "stale_source_pending"
  | "ready_to_materialize"
  | "materialized";

export type CandidateState =
  | "proposed"
  | "promoted"
  | "rejected";

export type GraphEvidenceState =
  | "verified"
  | "not_found"
  | "unknown_not_indexed"
  | "unknown_stale"
  | "ambiguous"
  | "unsupported"
  | "approximate_candidate";

export type CandidateCommandKind =
  | "accept_candidate"
  | "reject_candidate"
  | "edit_candidate"
  | "request_context"
  | "assign_reviewer";

// ── Graph readiness view ─────────────────────────────────────────

export type GraphReadiness =
  | "ready"
  | "indexing"
  | "stale"
  | "failed"
  | "not_configured";

export type GraphScopeCoverage =
  | "full"
  | "partial";

export type GraphReadinessView = {
  readiness: GraphReadiness;
  graph_snapshot_id?: string;
  detail?: string;
  scope_coverage?: GraphScopeCoverage;
};

// ── Evidence refresh outcome ─────────────────────────────────────

export type EvidenceRefreshStatus =
  | "refreshed"
  | "ineligible"
  | "not_found"
  | "unsupported";

export type CodeSourceCurrentness =
  | "current"
  | "stale"
  | "moved"
  | "deleted"
  | "unavailable";

export type CodeSourceRefreshResult = {
  binding_id: string;
  currentness: CodeSourceCurrentness;
  detail?: string;
};

export type EvidenceRefreshOutcome = {
  decision_id: string;
  status: EvidenceRefreshStatus;
  binding_results: CodeSourceRefreshResult[];
  detail?: string;
};

export type DecisionCommandKind =
  | "approve_signoff"
  | "reject_signoff"
  | "request_context"
  | "assign_reviewer"
  | "bind_to_code"
  | "supersede_decision"
  | "mark_different_scopes"
  | "refresh_source"
  | "untrack_decision";

export type CodeRegion = {
  file: string;
  start_line: number;
  end_line: number;
  content_hash?: string;
};

export type LedgerCandidate = {
  kind: "candidate";
  id: string;
  summary: string;
  feature_hint?: string;
  sources: SourceEvidenceItem[];
  review_state: ReviewLifecycleState;
  allowed_commands: CandidateCommandKind[];
};

export type TrackingState = "tracked" | "untracked";

export type CodeEvidenceAction =
  | "view_provenance"
  | "accept_binding"
  | "refine_target"
  | "revalidate"
  | "mark_review_needed"
  | "keep_source_only"
  | "build_index"
  | "expand_index"
  | "rebuild_graph"
  | "choose_target"
  | "refine_hint"
  | "request_hosted_check"
  | "validate_exactly"
  | "dismiss_suggestion"
  | "keep_as_advisory";

export type CodeEvidenceSection = {
  evidence_state: GraphEvidenceState;
  evidence_label: string;
  evidence_copy: string;
  graph_snapshot_id?: string;
  validated_sha?: string;
  scope_sufficiency?: string;
  allowed_actions: CodeEvidenceAction[];
};

export type FreshnessAction =
  | "refresh_source"
  | "revalidate_binding"
  | "mark_reviewed"
  | "untrack";

export type FreshnessWarning = {
  message: string;
  actions: FreshnessAction[];
};

export type LedgerDecision = {
  kind: "decision";
  id: string;
  summary: string;
  feature: string;
  parent_id?: string;
  signoff: SignoffState;
  review_state: ReviewLifecycleState;
  sources: SourceEvidenceItem[];
  regions?: CodeRegion[];
  conflicts_with?: string[];
  discovered?: boolean;
  allowed_commands: DecisionCommandKind[];
  graph_readiness?: GraphReadinessView;
  tracking_state: TrackingState;
  source_freshness: "fresh" | "stale" | "offline" | "unknown";
  code_evidence?: CodeEvidenceSection;
  freshness_warning?: FreshnessWarning;
};

export type LedgerReviewItem = LedgerDecision | LedgerCandidate;

// ── Code Impact Discovery (decision.find_code_impact) ────────────

export type FindCodeImpactStatus =
  | "found"
  | "needs_human_selection"
  | "not_found"
  | "graph_unavailable"
  | "graph_stale"
  | "unsupported";

export type BindingDiscoveryPhase =
  | "discovery_requested"
  | "search_planned"
  | "candidates_ready";

export type LocatorGraphReadiness =
  | "ready"
  | "indexing"
  | "stale"
  | "unavailable";

export type LocatorTargetType = "file_path" | "code_region" | "symbol";

export type LocatorEvidenceState =
  | "verified"
  | "not_found"
  | "unknown_not_indexed"
  | "unknown_stale"
  | "ambiguous"
  | "unsupported"
  | "approximate_candidate";

export type LocatorConfidence = "high" | "medium" | "low";

export type LocatorReasonCode =
  | "exact_symbol"
  | "exact_path"
  | "lexical_match"
  | "fuzzy_match"
  | "ast_pattern"
  | "heuristic";

export type CodeImpactCaveatKind =
  | "graph_stale"
  | "partial_index"
  | "limited_source_context"
  | "approximate_candidates"
  | "truncated";

export type LocatorReason = {
  code: LocatorReasonCode;
  detail: string;
};

export type LocatorSnapshotInfo = {
  graph_snapshot_id?: string;
  repo_id?: string;
  commit_sha?: string;
  index_version?: string;
  ref_name?: string;
};

export type SearchPlanSummary = {
  strategy: string;
  queries_executed: number;
  hint_seeded: boolean;
};

export type CodeImpactCandidate = {
  target: string;
  target_type: LocatorTargetType;
  file: string;
  range?: string;
  symbol?: string;
  evidence_state: LocatorEvidenceState;
  confidence: LocatorConfidence;
  reasons: LocatorReason[];
  graph_snapshot_id?: string;
  validated_sha?: string;
  content_hash?: string;
};

export type CodeImpactCaveat = {
  kind: CodeImpactCaveatKind;
  detail: string;
};

export type FindCodeImpactNextAction = {
  action: string;
  description: string;
  command_hint?: string;
};

export type FindCodeImpactResult = {
  status: FindCodeImpactStatus;
  decision_id: string;
  phase: BindingDiscoveryPhase;
  graph_readiness: LocatorGraphReadiness;
  snapshot?: LocatorSnapshotInfo;
  search_plan?: SearchPlanSummary;
  candidates: CodeImpactCandidate[];
  caveats: CodeImpactCaveat[];
  next_actions: FindCodeImpactNextAction[];
};

// ── Connection metadata registry ─────────────────────────────────

export type ConnectionStatus =
  | "connected"
  | "action_needed"
  | "not_connected"
  | "unsupported";

export type AuthMethod = "oauth" | "device_flow" | "imported_token" | "pat";

export type ConnectionMetadata = {
  status: ConnectionStatus;
  account_label?: string;
  capabilities: string[];
  updated_at?: string;
  auth_method?: AuthMethod;
};

export type ConnectionRegistry = {
  connections: Record<string, ConnectionMetadata>;
};

// ── Connection start outcome ─────────────────────────────────────

// ── Google Drive resource selection (daemon-owned, #1264) ────────
//
// The daemon validates the folder; the SPA only ever sees this redacted
// projection (no tokens, no codes, no verifiers).

export type ProviderResourceState =
  | "ready"
  | "pending_selection"
  | "not_ready"
  | "action_needed"
  | "unsupported";

export type ProviderResourceDescriptor = {
  provider: string;
  resource_kind: string;
  folder_id: string;
  folder_display_name: string;
  drive_id?: string;
  mime_type: string;
  can_add_children: boolean;
  can_list_children?: boolean;
};

export type ResourceSelectionStartOutcome =
  | {
      outcome: "start_url";
      provider: string;
      url: string;
      selection_session_id: string;
    }
  | { outcome: "action_needed"; provider: string; message: string }
  | { outcome: "unsupported"; provider: string; message: string };

export type ResourceSelectionProjection = {
  provider: string;
  state: ProviderResourceState;
  message: string;
  resource?: ProviderResourceDescriptor;
};

export type ConnectionStartOutcome =
  | { outcome: "connected"; provider: string; account_label?: string }
  | {
      outcome: "start_url";
      provider: string;
      url: string;
      auth_session_id: string;
      user_code?: string;
      flow_type?: "device_flow" | "local_loopback";
    }
  | { outcome: "action_needed"; provider: string; message: string }
  | { outcome: "unsupported"; provider: string; message: string };

// ── Connection completion outcome ────────────────────────────────

export type ConnectionCompletionOutcome =
  | {
      outcome: "connected";
      provider: string;
      account_label?: string;
      auth_method?: string;
      capabilities?: string[];
    }
  | { outcome: "action_needed"; provider: string; message: string }
  | { outcome: "denied"; provider: string; message: string }
  | { outcome: "expired"; provider: string; message: string }
  | { outcome: "unsupported"; provider: string; message: string }
  | { outcome: "provider_unavailable"; provider: string; message: string }
  | { outcome: "pending"; provider: string; message?: string };

// ── Projection setup state (Settings / Sources / Connections) ────

export type ProjectionSetupState =
  | "configured"
  | "projection_deferred"
  | "provider_not_configured"
  | "provider_unavailable";

export type ProjectionSetupDetail = {
  state: ProjectionSetupState;
  provider_id?: string;
  message: string;
  repair_hint?: string;
};

// ── Ingest/tool response projection state ────────────────────────

export type IngestProjectionOutcome =
  | "candidate_created"
  | "projection_deferred"
  | "provider_not_configured"
  | "provider_unavailable"
  | "no_candidate";

export type IngestResponseProjectionState = {
  outcome: IngestProjectionOutcome;
  message: string;
  repair_hint?: string;
};

// ── Review command (emitted by UI) ───────────────────────────────

export type ReviewCommandPayload = {
  target_id: string;
  command:
    | CandidateCommandKind
    | DecisionCommandKind
    | "ingest_batch"
    | "reject_candidate"
    | "demote_decision";
  reason?: string;
};
