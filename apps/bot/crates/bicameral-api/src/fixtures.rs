//! Typed fixture constructors for Bot-MCP lifecycle `ToolRequest` shapes.
//!
//! Downstream crates import these helpers to build valid lifecycle requests
//! without re-reading the runtime spec.  Each constructor produces a fully
//! populated `ToolRequest` that round-trips through `serde_json` without
//! data loss.
//!
//! **Non-goal:** dispatcher routing, capability negotiation, or runtime
//! handshake logic.  Fixtures describe wire shapes, not execution behavior.

use crate::tool_request::*;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Authority helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid `AuthorityContext` for fixture use.
pub fn authority_mcp(actor: &str, session: &str, workspace: &str) -> AuthorityContext {
    AuthorityContext {
        actor_id: actor.into(),
        auth_method: AuthMethod::McpSession,
        session_id: Some(session.into()),
        workspace: Some(workspace.into()),
        policy_scope: vec!["default".into()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert("surface".into(), serde_json::Value::String("mcp".into()));
            m
        },
    }
}

/// Build a CLI `AuthorityContext` for fixture use.
pub fn authority_cli(actor: &str, workspace: &str) -> AuthorityContext {
    AuthorityContext {
        actor_id: actor.into(),
        auth_method: AuthMethod::Cli,
        session_id: None,
        workspace: Some(workspace.into()),
        policy_scope: vec!["default".into()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert("surface".into(), serde_json::Value::String("cli".into()));
            m
        },
    }
}

/// Build a dashboard `AuthorityContext` for fixture use.
pub fn authority_dashboard(actor: &str, workspace: &str) -> AuthorityContext {
    AuthorityContext {
        actor_id: actor.into(),
        auth_method: AuthMethod::Dashboard,
        session_id: None,
        workspace: Some(workspace.into()),
        policy_scope: vec!["default".into(), "review".into()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert(
                "surface".into(),
                serde_json::Value::String("dashboard".into()),
            );
            m
        },
    }
}

/// Build a hosted-bridge `AuthorityContext` for fixture use (#766).
///
/// This mirrors the authority the gateway mints only after validating a paired
/// hosted session: the daemon-derived `actor_id` carries the bound Cloud
/// account (`bridge:<cloud_account_id>`) and never a raw session token.
pub fn authority_hosted_bridge(cloud_account_id: &str) -> AuthorityContext {
    AuthorityContext {
        actor_id: format!("bridge:{cloud_account_id}"),
        auth_method: AuthMethod::HostedBridge,
        session_id: Some("bridge-sess".into()),
        workspace: None,
        policy_scope: vec!["hosted_bridge".into()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert(
                "surface".into(),
                serde_json::Value::String("hosted_bridge".into()),
            );
            m
        },
    }
}

// ---------------------------------------------------------------------------
// Accepted lifecycle ToolRequest constructors
// ---------------------------------------------------------------------------

/// `ingest.submit_local` — accepted shape with evidence.
pub fn ingest_submit_local() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://linear.app/team/issue/PROJ-42".into(),
            source_type: "linear".into(),
            label: Some("PROJ-42: Add health check".into()),
            title: "Health check endpoint must verify DB connection".into(),
            description: "The /healthz endpoint returns 200 only when the DB pool is healthy."
                .into(),
            suggested_level: Some("architecture".into()),
            snapshot_content: Some("Title: Add health check endpoint\n...".into()),
            evidence: vec![IngestEvidenceItem {
                excerpt: "The service needs a /healthz endpoint".into(),
                span_start: Some(0),
                span_end: Some(37),
                confidence: Some(0.92),
            }],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-001", "/home/user/project"),
    )
}

/// `ingest.submit_managed` — managed provider-backed ingestion (provider not connected scenario).
pub fn ingest_submit_managed(provider_id: &str, source_binding_id: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitManaged(IngestSubmitManagedParams {
            provider_id: provider_id.into(),
            source_binding_id: source_binding_id.into(),
            source_uri: "https://github.com/acme/checkout".into(),
            title: "Managed source evidence".into(),
            description: "Evidence from a managed provider source".into(),
            snapshot_content: None,
            evidence: vec![IngestEvidenceItem {
                excerpt: "rate limiting is required".into(),
                span_start: None,
                span_end: None,
                confidence: Some(0.8),
            }],
            metadata: None,
        }),
        authority_mcp("agent-mcp-001", "sess-managed-001", "/home/user/project"),
    )
}

/// `preflight.run` — accepted shape with file and symbol scope.
pub fn preflight_run() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::PreflightRun(PreflightRunParams {
            files: vec!["src/routes/healthz.rs".into(), "src/db/pool.rs".into()],
            symbols: vec!["HealthCheckHandler".into()],
            diff_context: None,
            branch: Some("feature/healthz".into()),
            checkpoint_hint: None,
            managed_planning: None,
        }),
        authority_mcp(
            "agent-copilot-02",
            "sess-lifecycle-002",
            "/home/user/project",
        ),
    )
}

/// Alpha staged preflight response fixture — expected wire shape.
///
/// Capture, projection, and enforcement are `not_configured`.
/// Lookup is `completed` with sample decision refs and limitations.
/// Session directive is `continue`. No checkpoint hint (default lookup).
pub fn staged_preflight_response() -> crate::staged_preflight::StagedPreflightResponse {
    crate::staged_preflight::StagedPreflightResponse::alpha_from_lookup(
        vec![
            "a0000001-0000-4000-8000-000000000001".into(),
            "a0000001-0000-4000-8000-000000000002".into(),
        ],
        vec![
            "graph: Graph data unavailable. Using source-only preflight.".into(),
            "team_active_prs: Local preflight cannot see team-wide active PR branches.".into(),
        ],
        None,
    )
}

/// `preflight.run` with checkpoint hint — accepted shape.
pub fn preflight_run_with_checkpoint_hint() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::PreflightRun(PreflightRunParams {
            files: vec!["src/routes/healthz.rs".into()],
            symbols: vec![],
            diff_context: None,
            branch: None,
            checkpoint_hint: Some(crate::tool_request::CheckpointHint::PreWork),
            managed_planning: None,
        }),
        authority_mcp(
            "agent-copilot-03",
            "sess-lifecycle-003",
            "/home/user/project",
        ),
    )
}

/// Alpha staged preflight response with checkpoint hint echoed.
pub fn staged_preflight_response_with_hint() -> crate::staged_preflight::StagedPreflightResponse {
    crate::staged_preflight::StagedPreflightResponse::alpha_from_lookup(
        vec!["a0000001-0000-4000-8000-000000000001".into()],
        vec!["graph: Graph data unavailable. Using source-only preflight.".into()],
        Some(crate::tool_request::CheckpointHint::PreWork),
    )
}

/// Canonical alpha staged-recall `ToolResponse.result` envelope fixture (#817).
///
/// Deterministic wire shape for the daemon-owned envelope: read-only authority,
/// daemon-authored product content (staged payload, recall packet, caveats),
/// and validation-only witnesses (RuntimeTrace correlation + not-consumed
/// topology receipt). Uses fixed identifiers so the shape round-trips stably.
pub fn alpha_recall_envelope() -> crate::alpha_recall::AlphaRecallEnvelope {
    use crate::alpha_recall::{
        AlphaRecallCaveat, AlphaRecallCaveatScope, AlphaRecallCaveatStatus, AlphaRecallEnvelope,
        AlphaRecallProduct, AlphaRecallTopologyWitness, AlphaRecallTraceWitness,
        AlphaRecallWitness,
    };

    // Pin the packet id so the wire shape is deterministic (the alpha packet
    // otherwise generates a random id per call).
    let mut packet = crate::recall::RecallPacket::new_alpha();
    packet.packet_id = Uuid::parse_str("a0000001-0000-4000-8000-000000000011").unwrap();

    // Carry one reviewable candidate so the golden wire shape exercises the v2
    // packet contract: informed-review fields plus a derived, typed
    // decision-attention signal (issue #827).
    packet.decision_candidates = vec![crate::recall::SurfacedDecisionCandidate {
        candidate_id: Uuid::parse_str("c0000001-0000-4000-8000-000000000012").unwrap(),
        title: "Rate limit public API before launch".into(),
        summary: "Public API endpoints should enforce a per-client rate limit before launch."
            .into(),
        source_refs: vec![crate::recall::SourceRef {
            uri: "https://linear.app/team/issue/PROJ-101".into(),
            kind: "linear".into(),
            cited_span: Some("Rate limiting is required before public launch".into()),
        }],
        origin_surface: "linear".into(),
        extraction_confidence: crate::candidate::ExtractionConfidence::Medium,
        ambiguities: Vec::new(),
        related_decision_refs: Vec::new(),
        candidate_relationship: crate::recall::CandidateRelationship::NeedsSignoffReview,
        relevance_reasons: vec![crate::recall::CandidateRelevance::SourceTextMatch],
        evidence_freshness: crate::recall::CandidateEvidenceFreshness::Current,
        allowed_outcomes: vec![
            crate::recall::PromotionOutcome::NewConstraint,
            crate::recall::PromotionOutcome::NeedsSignoffReview,
            crate::recall::PromotionOutcome::DismissedAsNoise,
        ],
        code_evidence_refs: Vec::new(),
        review_state: crate::candidate::ReviewLifecycleState::Proposed,
        candidate_state: crate::candidate::CandidateState::Proposed,
    }];
    packet = packet.with_derived_decision_attention();

    let product = AlphaRecallProduct {
        staged: staged_preflight_response(),
        packet,
        caveats: vec![
            AlphaRecallCaveat {
                scope: AlphaRecallCaveatScope::Graph,
                status: AlphaRecallCaveatStatus::Unavailable,
                detail: "Graph data unavailable. Using source-only recall.".into(),
            },
            AlphaRecallCaveat {
                scope: AlphaRecallCaveatScope::Index,
                status: AlphaRecallCaveatStatus::Partial,
                detail: "Decision index is partially compiled for the requested scope.".into(),
            },
        ],
    };
    let witness = AlphaRecallWitness {
        runtime_trace: AlphaRecallTraceWitness {
            request_id: Uuid::parse_str("a0000001-0000-4000-8000-000000000010").unwrap(),
            command: "preflight.run".into(),
            caller_surface: "mcp".into(),
            status: crate::tool_request::TraceStatus::Ok,
        },
        topology_receipt: Some(AlphaRecallTopologyWitness::not_consumed()),
    };
    AlphaRecallEnvelope::new("v2", product, witness)
}

/// `binding.create` — accepted shape with binding spec and snapshot id.
pub fn binding_create() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: "DEC-101".into(),
            bindings: vec![
                BindingSpec {
                    file: "src/routes/healthz.rs".into(),
                    range: Some("10:1-25:1".into()),
                    symbol: Some("HealthCheckHandler::handle".into()),
                    from_discovery: None,
                },
                BindingSpec {
                    file: "src/db/pool.rs".into(),
                    range: None,
                    symbol: Some("ConnectionPool::is_healthy".into()),
                    from_discovery: None,
                },
            ],
            commit_sha: Some("abc123def456".into()),
            ref_name: Some("feature/healthz".into()),
            graph_snapshot_id: Some("snap-001".into()),
            validation_token: None,
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-003", "/home/user/project"),
    )
}

/// `binding.inspect` — accepted shape.
pub fn binding_inspect() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: "DEC-101".into(),
            commit_sha: Some("abc123def456".into()),
            ref_name: Some("main".into()),
            graph_snapshot_id: Some("snap-001".into()),
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-004", "/home/user/project"),
    )
}

/// `review.accept_candidate` — accepted shape.
pub fn review_accept_candidate() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: "CAND-042".into(),
            reason: Some("Source evidence sufficient, no conflicts found".into()),
        }),
        authority_cli("user-reviewer-01", "/home/user/project"),
    )
}

/// `candidate.hide` — accepted shape.
pub fn candidate_hide() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::CandidateHide(CandidateHideParams {
            candidate_id: "CAND-044".into(),
        }),
        authority_cli("user-reviewer-01", "/home/user/project"),
    )
}

/// `candidate.suppress` — accepted shape.
pub fn candidate_suppress() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::CandidateSuppress(CandidateSuppressParams {
            candidate_id: "CAND-045".into(),
            reason: "Duplicate of promoted Decision DEC-201".into(),
            expires_at: None,
        }),
        authority_cli("user-reviewer-01", "/home/user/project"),
    )
}

/// `review.reject_candidate` — accepted shape.
pub fn review_reject_candidate() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ReviewRejectCandidate(ReviewTargetParams {
            target_id: "CAND-043".into(),
            reason: Some("Evidence does not support this claim".into()),
        }),
        authority_cli("user-reviewer-02", "/home/user/project"),
    )
}

/// `review.approve_signoff` — accepted shape (ownership approval only).
pub fn review_approve_signoff() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ReviewApproveSignoff(ReviewTargetParams {
            target_id: "DEC-201".into(),
            reason: Some("Ownership approved after team discussion".into()),
        }),
        authority_dashboard("user-owner-01", "/home/user/project"),
    )
}

/// `review.reject_signoff` — accepted shape (ownership rejection only).
pub fn review_reject_signoff() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ReviewRejectSignoff(ReviewTargetParams {
            target_id: "DEC-202".into(),
            reason: Some("Meaning unclear, needs revision".into()),
        }),
        authority_dashboard("user-owner-02", "/home/user/project"),
    )
}

/// `review.resolve_compliance` — accepted shape (typed unsupported in V1).
pub fn review_resolve_compliance() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ReviewResolveCompliance(ResolveComplianceParams {
            target_id: "DEC-301".into(),
            compliance_verdict: "reflected".into(),
            reason: Some("Code matches decision".into()),
        }),
        authority_cli("user-auditor-01", "/home/user/project"),
    )
}

/// `review.transition_finding` — accepted shape for contradiction resolution.
pub fn review_transition_finding() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
            report_id: "a0000001-0001-4000-8000-000000000001".into(),
            to_status: "acknowledged".into(),
            note: Some("Acknowledging contradiction for review".into()),
        }),
        authority_dashboard("user-owner-01", "/home/user/project"),
    )
}

/// `recall.inspect_evidence` — accepted shape for read-only drill-in.
pub fn recall_inspect_evidence() -> ToolRequest {
    let packet = crate::recall::RecallPacket::new_alpha();
    ToolRequest::new(
        ToolCommand::RecallInspectEvidence(crate::recall::RecallInspectEvidenceParams {
            packet_id: packet.packet_id,
            target_id: Uuid::parse_str("a0000001-0001-4000-8000-000000000111").unwrap(),
            evidence_ref_id: None,
        }),
        authority_dashboard("user-reviewer-01", "/home/user/project"),
    )
}

/// `recall.expand_scope` — accepted shape for explicit scope expansion.
pub fn recall_expand_scope() -> ToolRequest {
    let packet = crate::recall::RecallPacket::new_alpha();
    ToolRequest::new(
        ToolCommand::RecallExpandScope(crate::recall::RecallExpandScopeParams {
            packet_id: packet.packet_id,
            scope_target: "adjacent source scope".into(),
            hints: vec!["look for related bindings".into()],
        }),
        authority_dashboard("user-reviewer-01", "/home/user/project"),
    )
}

/// `recall.promote_decision_candidate` — accepted shape for governed promotion.
pub fn recall_promote_decision_candidate() -> ToolRequest {
    let packet = crate::recall::RecallPacket::new_alpha();
    ToolRequest::new(
        ToolCommand::RecallPromoteDecisionCandidate(crate::recall::RecallPromoteCandidateParams {
            packet_id: packet.packet_id,
            candidate_id: Uuid::parse_str("a0000001-0001-4000-8000-000000000112").unwrap(),
            promotion_outcome: crate::recall::PromotionOutcome::NewConstraint,
            supersedes_decision_id: None,
            scoping_relationship: None,
            approval_proof: Some(crate::recall::ApprovalProof {
                kind: "signoff_token".into(),
                value: "tok_recall_promote_001".into(),
                actor_id: "user-owner-01".into(),
                provided_at: None,
            }),
            confirmation: None,
        }),
        authority_dashboard("user-owner-01", "/home/user/project"),
    )
}

/// `recall.request_correction` — accepted shape for governed correction.
pub fn recall_request_correction() -> ToolRequest {
    let packet = crate::recall::RecallPacket::new_alpha();
    ToolRequest::new(
        ToolCommand::RecallRequestCorrection(crate::recall::RecallRequestCorrectionParams {
            packet_id: packet.packet_id,
            selected_item_ids: vec![
                Uuid::parse_str("a0000001-0001-4000-8000-000000000113").unwrap()
            ],
            correction_kind: crate::recall::CorrectionKind::SourceContradiction,
            rationale: "correction needed for alpha posture alignment".into(),
            approval_proof: crate::recall::ApprovalProof {
                kind: "signoff_token".into(),
                value: "tok_recall_correction_001".into(),
                actor_id: "user-owner-01".into(),
                provided_at: None,
            },
        }),
        authority_dashboard("user-owner-01", "/home/user/project"),
    )
}

/// `history.list` — accepted shape with filters.
pub fn history_list() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::HistoryList(HistoryListParams {
            decision_id: Some("DEC-101".into()),
            include_events: true,
            include_bindings: true,
            since: None,
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-005", "/home/user/project"),
    )
}

/// `search.query` — accepted shape with scope and filters.
pub fn search_query() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::SearchQuery(SearchQueryParams {
            query: "health check endpoint".into(),
            scope: Some(SearchScope::All),
            filters: Some(SearchFilters {
                level: Some("architecture".into()),
                signoff_state: None,
                compliance_state: None,
                file: None,
            }),
            limit: Some(20),
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-006", "/home/user/project"),
    )
}

/// `tracking.untrack_source` — accepted shape.
pub fn tracking_untrack_source() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::TrackingUntrackSource(UntrackSourceParams {
            decision_id: "DEC-401".into(),
            reason: Some("superseded".into()),
            detail: Some("Replaced by DEC-402".into()),
        }),
        authority_cli("user-admin-01", "/home/user/project"),
    )
}

/// `tracking.refresh_query` — accepted shape with default mode.
pub fn tracking_refresh_query() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::TrackingRefreshQuery(RefreshQueryParams {
            mode: "auto".into(),
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-007", "/home/user/project"),
    )
}

/// `privacy.erase_subject` — accepted shape for PII disposal request.
pub fn privacy_erase_subject() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::PrivacyEraseSubject(PrivacyEraseSubjectParams {
            subject_id: "subj-gdpr-001".into(),
            predicate: Some("speaker".into()),
            reason: Some("GDPR Art.17 erasure request".into()),
        }),
        authority_mcp("agent-mcp-001", "sess-privacy-001", "/home/user/project"),
    )
}

/// `evidence.refresh` — accepted shape for manual evidence refresh.
pub fn evidence_refresh(decision_id: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::EvidenceRefresh(EvidenceRefreshParams {
            decision_id: decision_id.into(),
        }),
        authority_dashboard("user-reviewer-01", "/home/user/project"),
    )
}

/// `code.locate` — accepted shape with query and decision context.
pub fn code_locate() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::CodeLocate(CodeLocateParams {
            query: Some("GitHub device flow provider auth".into()),
            decision_id: Some("DEC-501".into()),
            candidate_id: None,
            source_evidence_ids: vec![],
            binding_ids: vec![],
            files: vec!["crates/bicameral-gateway/src/github_device_flow.rs".into()],
            symbols: vec!["start_provider_connection".into()],
            changed_files: vec![],
            diff_context: None,
            workspace_id: Some("/home/user/project".into()),
            commit_sha: None,
            ref_name: Some("HEAD".into()),
            graph_snapshot_id: None,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            scope_kind: Some(LocatorScopeKind::Query),
            limit: 20,
        }),
        authority_mcp("agent-mcp-001", "sess-locate-001", "/home/user/project"),
    )
}

/// `graph.status` — accepted shape for readiness check.
pub fn graph_status() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::GraphStatus(GraphStatusParams {
            workspace_id: Some("/home/user/project".into()),
            ref_name: Some("HEAD".into()),
            commit_sha: None,
            graph_snapshot_id: None,
        }),
        authority_mcp("agent-mcp-001", "sess-graph-001", "/home/user/project"),
    )
}

/// `graph.refresh_snapshot` — accepted shape for manual refresh.
pub fn graph_refresh_snapshot() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::GraphRefreshSnapshot(GraphRefreshParams {
            workspace_id: Some("/home/user/project".into()),
            ref_name: Some("main".into()),
            commit_sha: None,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            scope_kind: Some(RefreshScopeKind::Diff),
            files: vec![],
            symbols: vec![],
            allow_index: AllowIndex::Bounded,
            trigger: RefreshTrigger::Manual,
        }),
        authority_dashboard("user-reviewer-01", "/home/user/project"),
    )
}

// ---------------------------------------------------------------------------
// Negative / rejected fixture constructors
// ---------------------------------------------------------------------------

/// Missing authority: `actor_id` is empty — daemon must reject.
///
/// Expected `ToolResponse` class: `Rejected` with missing-authority reason.
pub fn negative_missing_actor_id() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://example.com/ticket".into(),
            source_type: "manual".into(),
            label: None,
            title: "Test".into(),
            description: "Test description".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        }),
        AuthorityContext {
            actor_id: String::new(),
            auth_method: AuthMethod::McpSession,
            session_id: None,
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        },
    )
}

/// Malformed payload: `ingest.submit_local` with empty required `title`.
///
/// Expected `ToolResponse` class: `Rejected` with malformed-payload reason.
pub fn negative_malformed_ingest_empty_title() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://example.com/ticket".into(),
            source_type: "manual".into(),
            label: None,
            title: String::new(),
            description: "A description".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        }),
        authority_mcp("agent-001", "sess-neg-001", "/repo"),
    )
}

/// `binding.create` with a validation token — daemon must return
/// `validation_token_unsupported` (ADR-0017).
///
/// Expected `ToolResponse` class: `Rejected` with `validation_token_unsupported`.
pub fn negative_validation_token_acceptance() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: "DEC-999".into(),
            bindings: vec![BindingSpec {
                file: "src/main.rs".into(),
                range: None,
                symbol: None,
                from_discovery: None,
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: Some("unsigned-token-abc".into()),
        }),
        authority_mcp("agent-001", "sess-neg-002", "/repo"),
    )
}

/// `search.query` with the supported `bindings` scope — daemon reads accepted,
/// bot-owned `BindingEvidence` from replayed state and returns binding results
/// (read-only; never verified project truth or merge safety).
///
/// Expected `ToolResponse` class: `Ok`.
pub fn search_query_bindings() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::SearchQuery(SearchQueryParams {
            query: "src/main.rs".into(),
            scope: Some(SearchScope::Bindings),
            filters: None,
            limit: None,
        }),
        authority_mcp("agent-001", "sess-lifecycle-006b", "/repo"),
    )
}

/// Unsafe evidence: excerpt claims governance authority (verified state).
///
/// Expected `ToolResponse` class: `Rejected` with `unsafe_evidence`.
pub fn negative_unsafe_evidence() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://example.com/ticket".into(),
            source_type: "manual".into(),
            label: None,
            title: "Fake verified evidence".into(),
            description: "Attempting to inject verified state".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![IngestEvidenceItem {
                excerpt: "GraphEvidenceState::verified binding confirmed".into(),
                span_start: None,
                span_end: None,
                confidence: Some(1.0),
            }],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        }),
        authority_mcp("agent-001", "sess-neg-005", "/repo"),
    )
}

/// Forbidden canonical field in metadata.
///
/// Expected `ToolResponse` class: `Rejected` with `forbidden_canonical_field`.
pub fn negative_forbidden_canonical_field() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://example.com/ticket".into(),
            source_type: "manual".into(),
            label: None,
            title: "Sneaky metadata".into(),
            description: "Attempting to inject decision_id".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: Some(serde_json::json!({
                "decision_id": "DEC-INJECTED",
                "notes": "innocent looking"
            })),
        }),
        authority_mcp("agent-001", "sess-neg-006", "/repo"),
    )
}

/// Legacy MCP-only payload shape — must be rejected.
///
/// Expected `ToolResponse` class: `Rejected` with `legacy_mcp_payload`.
pub fn negative_legacy_mcp_payload() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://example.com/ticket".into(),
            source_type: "manual".into(),
            label: None,
            title: "Legacy shape".into(),
            description: "Has mcp_version in metadata".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: Some(serde_json::json!({
                "mcp_version": "0.2",
                "direct_ledger_write": true
            })),
        }),
        authority_mcp("agent-001", "sess-neg-007", "/repo"),
    )
}

/// External adapter payload routed through local ingest — must be rejected.
///
/// Expected `ToolResponse` class: `Rejected` with `external_adapter_payload`.
pub fn negative_external_adapter_payload() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://github.com/org/repo/issues/1".into(),
            source_type: "github".into(),
            label: None,
            title: "External adapter shape".into(),
            description: "Has source_system in metadata".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: Some(serde_json::json!({
                "source_system": "github",
                "webhook_id": "hook-123"
            })),
        }),
        authority_mcp("agent-001", "sess-neg-008", "/repo"),
    )
}

/// Ingest with candidate drafts — accepted shape demonstrating candidate
/// creation without becoming tracked Decisions.
pub fn ingest_submit_local_with_drafts() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://linear.app/team/issue/PROJ-99".into(),
            source_type: "linear".into(),
            label: Some("PROJ-99: Multi-candidate ingest".into()),
            title: "Add a cache layer".into(),
            description: "We decided to add a cache layer for service responses".into(),
            suggested_level: Some("architecture".into()),
            snapshot_content: Some("We decided to add a cache layer for service responses. We decided to use TTL-based cache invalidation with event-driven purge. We decided to use Redis for distributed caching and in-memory caching for local development.".into()),
            evidence: vec![IngestEvidenceItem {
                excerpt: "We decided to add a cache layer, use TTL-based invalidation, and use Redis for distributed caching".into(),
                span_start: Some(0),
                span_end: Some(22),
                confidence: Some(0.85),
            }],
            candidate_drafts: vec![
                CandidateDraft {
                    title: "Cache invalidation strategy".into(),
                    description: "We decided to use TTL-based cache invalidation with event-driven purge".into(),
                    suggested_level: Some("implementation".into()),
                    tags: vec!["caching".into(), "performance".into()],
                },
                CandidateDraft {
                    title: "Cache storage backend".into(),
                    description: "We decided to use Redis for distributed caching and in-memory caching for local development".into(),
                    suggested_level: None,
                    tags: vec!["caching".into()],
                },
            ],
            binding_hints: vec![BindingHint {
                file: "src/cache/mod.rs".into(),
                range: Some("1:1-50:1".into()),
                symbol: Some("CacheLayer".into()),
            }],
            rationale: Some("LLM extracted these candidates from the ticket discussion".into()),
            metadata: Some(serde_json::json!({"extraction_model": "gpt-4"})),
        }),
        authority_mcp("agent-mcp-001", "sess-lifecycle-010", "/home/user/project"),
    )
}

/// Preflight with mutation attempt — preflight is read-only, so supplying
/// fields that imply mutation intent must be rejected.  This fixture
/// carries otherwise-valid preflight params but the test consumer
/// verifies the command is never treated as a write path.
///
/// Expected `ToolResponse` class: `Ok` (preflight is valid) — the test
/// verifies the protocol shape is accepted but no mutation occurs.
pub fn negative_preflight_readonly_shape() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::PreflightRun(PreflightRunParams {
            files: vec!["src/main.rs".into()],
            symbols: vec![],
            diff_context: Some(
                "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new".into(),
            ),
            branch: Some("main".into()),
            checkpoint_hint: None,
            managed_planning: None,
        }),
        authority_mcp("agent-001", "sess-neg-004", "/repo"),
    )
}

// ---------------------------------------------------------------------------
// ToolResponse fixture constructors
// ---------------------------------------------------------------------------

/// Successful response for an ingest submission.
pub fn response_ingest_accepted(request_id: Uuid) -> ToolResponse {
    ToolResponse::success(
        request_id,
        serde_json::json!({
            "source_id": "SRC-001",
            "snapshot_id": "SNAP-001",
            "evidence_ids": ["EV-001"],
            "candidate_id": "CAND-100"
        }),
    )
}

/// Rejected response: missing authority.
pub fn response_missing_authority(request_id: Uuid) -> ToolResponse {
    ToolResponse::rejected(request_id, "missing_authority: actor_id is required".into())
}

/// Rejected response: malformed payload.
pub fn response_malformed_payload(request_id: Uuid) -> ToolResponse {
    ToolResponse::rejected(
        request_id,
        "malformed_payload: title must not be empty".into(),
    )
}

/// Rejected response: validation token unsupported (ADR-0017).
pub fn response_validation_token_unsupported(request_id: Uuid) -> ToolResponse {
    ToolResponse::rejected(
        request_id,
        "validation_token_unsupported: token acceptance is deferred until signing, expiry, integrity, and revocation are specified".into(),
    )
}

/// Rejected response: unsupported scope from general daemon version
/// negotiation. This is retained for future/unknown scopes only — the
/// `bindings` scope is supported and never returns this.
pub fn response_unsupported_scope(request_id: Uuid) -> ToolResponse {
    ToolResponse::rejected(
        request_id,
        "unsupported_scope: requested search scope is not supported by this daemon version".into(),
    )
}

/// Error response: daemon unavailable.
pub fn response_daemon_error(request_id: Uuid) -> ToolResponse {
    ToolResponse::error(request_id, "daemon_unavailable: runtime not started".into())
}

// ---------------------------------------------------------------------------
// Graph / CodeLocator result fixtures
// ---------------------------------------------------------------------------

/// Locator result: ready graph, verified candidate found.
pub fn locator_result_ready_verified() -> LocatorResult {
    LocatorResult {
        status: LocatorStatus::Found,
        query: Some("GitHub device flow provider auth".into()),
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-fixture-001".into()),
            repo_id: Some("repo-fixture".into()),
            commit_sha: Some("a77b83a".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        scope: Some(LocatorScopeInfo {
            scope_kind: Some("query".into()),
            scope_fingerprint: Some("fp-fixture-001".into()),
            proof_state: Some(ScopeProofState::Sufficient),
        }),
        candidates: vec![LocatorCandidate {
            target: "crates/bicameral-gateway/src/github_device_flow.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "crates/bicameral-gateway/src/github_device_flow.rs".into(),
            range: None,
            symbol: None,
            evidence_state: LocatorEvidenceState::Verified,
            confidence: LocatorConfidence::High,
            reasons: vec![
                LocatorReason {
                    code: LocatorReasonCode::ExactPath,
                    detail: "File exists at snapshot a77b83a".into(),
                },
                LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Matched github/device/provider/auth terms".into(),
                },
            ],
            graph_snapshot_id: Some("gs-fixture-001".into()),
            validated_sha: Some("a77b83a".into()),
            content_hash: None,
            caveats: vec![],
        }],
        warnings: vec![],
        next_actions: vec!["choose_candidate".into()],
    }
}

/// Locator result: stale graph.
pub fn locator_result_stale() -> LocatorResult {
    LocatorResult {
        status: LocatorStatus::Found,
        query: Some("auth module".into()),
        graph_readiness: LocatorGraphReadiness::Stale,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-fixture-stale".into()),
            repo_id: None,
            commit_sha: Some("old-sha".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        scope: None,
        candidates: vec![LocatorCandidate {
            target: "src/auth.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/auth.rs".into(),
            range: None,
            symbol: None,
            evidence_state: LocatorEvidenceState::UnknownStale,
            confidence: LocatorConfidence::Unknown,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::LexicalMatch,
                detail: "Lexical match against stale index".into(),
            }],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            caveats: vec![],
        }],
        warnings: vec!["Graph is stale; results may not reflect current code.".into()],
        next_actions: vec!["refresh_graph".into(), "refresh_latest".into()],
    }
}

/// Locator result: graph not configured.
pub fn locator_result_not_configured() -> LocatorResult {
    LocatorResult {
        status: LocatorStatus::GraphNotConfigured,
        query: Some("any search".into()),
        graph_readiness: LocatorGraphReadiness::NotConfigured,
        snapshot: None,
        scope: None,
        candidates: vec![],
        warnings: vec!["Graph unavailable; configure workspace or continue source-only.".into()],
        next_actions: vec!["continue_source_only".into()],
    }
}

/// Locator result: ambiguous candidates.
pub fn locator_result_ambiguous() -> LocatorResult {
    LocatorResult {
        status: LocatorStatus::Ambiguous,
        query: Some("connection handler".into()),
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-fixture-amb".into()),
            repo_id: None,
            commit_sha: Some("def456".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        scope: Some(LocatorScopeInfo {
            scope_kind: Some("query".into()),
            scope_fingerprint: None,
            proof_state: Some(ScopeProofState::Sufficient),
        }),
        candidates: vec![
            LocatorCandidate {
                target: "src/conn_a.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/conn_a.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::Ambiguous,
                confidence: LocatorConfidence::Medium,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "connection_handler in file".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
                caveats: vec![],
            },
            LocatorCandidate {
                target: "src/conn_b.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/conn_b.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::Ambiguous,
                confidence: LocatorConfidence::Medium,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "connection_handler in file".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
                caveats: vec![],
            },
        ],
        warnings: vec![],
        next_actions: vec!["narrow_query".into(), "choose_candidate".into()],
    }
}

/// Locator result: unsupported language/file type.
pub fn locator_result_unsupported() -> LocatorResult {
    LocatorResult {
        status: LocatorStatus::Unsupported,
        query: Some("config file".into()),
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: None,
        scope: None,
        candidates: vec![LocatorCandidate {
            target: "config/settings.yaml".into(),
            target_type: LocatorTargetType::FilePath,
            file: "config/settings.yaml".into(),
            range: None,
            symbol: None,
            evidence_state: LocatorEvidenceState::Unsupported,
            confidence: LocatorConfidence::Unknown,
            reasons: vec![],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            caveats: vec![],
        }],
        warnings: vec!["YAML files are outside supported exact validation.".into()],
        next_actions: vec!["continue_source_only".into()],
    }
}

/// Locator result: approximate candidate (not verified).
pub fn locator_result_approximate_candidate() -> LocatorResult {
    LocatorResult {
        status: LocatorStatus::Found,
        query: Some("rate limiting".into()),
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-fixture-approx".into()),
            repo_id: None,
            commit_sha: Some("abc789".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        scope: Some(LocatorScopeInfo {
            scope_kind: Some("query".into()),
            scope_fingerprint: None,
            proof_state: Some(ScopeProofState::Insufficient),
        }),
        candidates: vec![LocatorCandidate {
            target: "src/middleware/rate_limit.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/middleware/rate_limit.rs".into(),
            range: None,
            symbol: Some("RateLimiter".into()),
            evidence_state: LocatorEvidenceState::ApproximateCandidate,
            confidence: LocatorConfidence::Low,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::LexicalMatch,
                detail: "rate_limit term match but scope insufficient for exact proof".into(),
            }],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            caveats: vec![],
        }],
        warnings: vec!["Scope insufficient for exact verification.".into()],
        next_actions: vec!["refresh_graph".into()],
    }
}

/// Graph refresh result: ready.
pub fn graph_refresh_result_ready() -> GraphRefreshResult {
    GraphRefreshResult {
        status: GraphRefreshStatus::Ready,
        target: Some(GraphRefreshTarget {
            requested_ref: Some("main".into()),
            resolved_sha: Some("abc123".into()),
            current_head_sha: Some("abc123".into()),
            is_current_for_target: true,
        }),
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-ready-001".into()),
            repo_id: Some("repo-fixture".into()),
            commit_sha: Some("abc123".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        slice: Some(GraphSliceInfo {
            graph_slice_id: Some("sl-ready-001".into()),
            scope_kind: Some("diff".into()),
            scope_fingerprint: None,
            completeness: Some(SliceCompleteness::DiffScoped),
        }),
        job: Some(GraphJobInfo {
            job_id: Some("job-ready-001".into()),
            state: Some(GraphJobState::Ready),
            superseded_by_sha: None,
        }),
        warnings: vec![],
        next_actions: vec![],
    }
}

/// Graph refresh result: stale (target moved during indexing).
pub fn graph_refresh_result_stale() -> GraphRefreshResult {
    GraphRefreshResult {
        status: GraphRefreshStatus::Stale,
        target: Some(GraphRefreshTarget {
            requested_ref: Some("main".into()),
            resolved_sha: Some("c10".into()),
            current_head_sha: Some("c12".into()),
            is_current_for_target: false,
        }),
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-stale-001".into()),
            repo_id: None,
            commit_sha: Some("c10".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        slice: None,
        job: Some(GraphJobInfo {
            job_id: Some("job-stale-001".into()),
            state: Some(GraphJobState::Stale),
            superseded_by_sha: Some("c12".into()),
        }),
        warnings: vec!["Target moved during indexing".into()],
        next_actions: vec!["refresh_latest".into()],
    }
}

/// Graph refresh result: not configured.
pub fn graph_refresh_result_not_configured() -> GraphRefreshResult {
    GraphRefreshResult {
        status: GraphRefreshStatus::NotConfigured,
        target: None,
        snapshot: None,
        slice: None,
        job: None,
        warnings: vec!["Workspace not configured for graph indexing.".into()],
        next_actions: vec!["continue_source_only".into()],
    }
}

/// Graph refresh result: over budget.
pub fn graph_refresh_result_over_budget() -> GraphRefreshResult {
    GraphRefreshResult {
        status: GraphRefreshStatus::OverBudget,
        target: None,
        snapshot: None,
        slice: None,
        job: None,
        warnings: vec!["Requested scope (65 targets) exceeds bounded budget (64); \
             declined to avoid an unbounded rebuild."
            .into()],
        next_actions: vec!["narrow_scope".into()],
    }
}

/// Graph status result: ready.
pub fn graph_status_result_ready() -> GraphStatusResult {
    GraphStatusResult {
        readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-status-001".into()),
            repo_id: None,
            commit_sha: Some("abc123".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        stale_detail: None,
        next_actions: vec![],
    }
}

/// Graph status result: not configured.
pub fn graph_status_result_not_configured() -> GraphStatusResult {
    GraphStatusResult {
        readiness: LocatorGraphReadiness::NotConfigured,
        snapshot: None,
        stale_detail: Some("Workspace not configured for graph indexing".into()),
        next_actions: vec!["continue_source_only".into()],
    }
}

// ---------------------------------------------------------------------------
// All-fixture collection (for bulk validation)
// ---------------------------------------------------------------------------

/// `decision.find_code_impact` — accepted shape with decision reference.
pub fn decision_find_code_impact() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::DecisionFindCodeImpact(FindCodeImpactParams {
            decision_id: "DEC-601".into(),
            hints: vec![
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::File,
                    value: "src/routes/healthz.rs".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Symbol,
                    value: "HealthCheckHandler".into(),
                },
            ],
            workspace_id: Some("/home/user/project".into()),
            ref_name: Some("HEAD".into()),
            commit_sha: None,
            limit: 20,
        }),
        authority_dashboard("user-reviewer-01", "/home/user/project"),
    )
}

/// `decision.find_code_impact` — minimal shape, no hints.
pub fn decision_find_code_impact_minimal() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::DecisionFindCodeImpact(FindCodeImpactParams {
            decision_id: "DEC-602".into(),
            hints: vec![],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        }),
        authority_mcp("agent-mcp-001", "sess-impact-001", "/home/user/project"),
    )
}

// ---------------------------------------------------------------------------
// FindCodeImpactResult fixture constructors
// ---------------------------------------------------------------------------

/// Find-code-impact result: verified candidates found.
pub fn find_code_impact_result_verified() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Found,
        decision_id: "DEC-601".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-impact-001".into()),
            repo_id: Some("repo-fixture".into()),
            commit_sha: Some("a77b83a".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 3,
            hint_seeded: true,
        }),
        candidates: vec![CodeImpactCandidate {
            target: "src/routes/healthz.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/routes/healthz.rs".into(),
            range: Some("10:1-25:1".into()),
            symbol: Some("HealthCheckHandler::handle".into()),
            evidence_state: LocatorEvidenceState::Verified,
            confidence: LocatorConfidence::High,
            reasons: vec![
                LocatorReason {
                    code: LocatorReasonCode::ExactSymbol,
                    detail: "Symbol matches Decision source context".into(),
                },
                LocatorReason {
                    code: LocatorReasonCode::ExactPath,
                    detail: "File exists at snapshot a77b83a".into(),
                },
            ],
            graph_snapshot_id: Some("gs-impact-001".into()),
            validated_sha: Some("a77b83a".into()),
            content_hash: None,
        }],
        caveats: vec![],
        next_actions: vec![FindCodeImpactNextAction {
            action: "binding_create".into(),
            description: "Create binding evidence for verified candidate".into(),
            command_hint: Some("binding.create".into()),
        }],
        semantic_recall: None,
    }
}

/// Find-code-impact result: approximate candidates, needs human selection.
pub fn find_code_impact_result_approximate() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::NeedsHumanSelection,
        decision_id: "DEC-602".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-approx-001".into()),
            repo_id: None,
            commit_sha: Some("def456".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 2,
            hint_seeded: false,
        }),
        candidates: vec![
            CodeImpactCandidate {
                target: "src/conn_a.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/conn_a.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Medium,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Lexical match; scope insufficient for exact proof".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            },
            CodeImpactCandidate {
                target: "src/conn_b.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/conn_b.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Low,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Weaker lexical match".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            },
        ],
        caveats: vec![CodeImpactCaveat {
            kind: CodeImpactCaveatKind::ApproximateCandidates,
            detail: "All candidates are approximate; human selection required".into(),
        }],
        next_actions: vec![
            FindCodeImpactNextAction {
                action: "choose_candidate".into(),
                description: "Select the correct candidate for binding".into(),
                command_hint: None,
            },
            FindCodeImpactNextAction {
                action: "refresh_graph".into(),
                description: "Refresh graph for better precision".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            },
        ],
        semantic_recall: None,
    }
}

/// Find-code-impact result: graph unavailable.
pub fn find_code_impact_result_graph_unavailable() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::GraphUnavailable,
        decision_id: "DEC-603".into(),
        phase: BindingDiscoveryPhase::DiscoveryRequested,
        graph_readiness: LocatorGraphReadiness::NotConfigured,
        snapshot: None,
        search_plan: None,
        candidates: vec![],
        caveats: vec![],
        next_actions: vec![FindCodeImpactNextAction {
            action: "configure_workspace".into(),
            description: "Configure workspace for graph indexing".into(),
            command_hint: Some("graph.refresh_snapshot".into()),
        }],
        semantic_recall: None,
    }
}

/// Find-code-impact result: graph stale.
pub fn find_code_impact_result_graph_stale() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::GraphStale,
        decision_id: "DEC-604".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Stale,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-stale-impact".into()),
            repo_id: None,
            commit_sha: Some("old-sha".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 1,
            hint_seeded: false,
        }),
        candidates: vec![CodeImpactCandidate {
            target: "src/stale.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/stale.rs".into(),
            range: None,
            symbol: None,
            evidence_state: LocatorEvidenceState::UnknownStale,
            confidence: LocatorConfidence::Unknown,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::LexicalMatch,
                detail: "Match against stale index".into(),
            }],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
        }],
        caveats: vec![CodeImpactCaveat {
            kind: CodeImpactCaveatKind::GraphStale,
            detail: "Graph snapshot is behind HEAD".into(),
        }],
        next_actions: vec![FindCodeImpactNextAction {
            action: "refresh_graph".into(),
            description: "Refresh graph snapshot to current HEAD".into(),
            command_hint: Some("graph.refresh_snapshot".into()),
        }],
        semantic_recall: None,
    }
}

/// Find-code-impact result: not found.
pub fn find_code_impact_result_not_found() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::NotFound,
        decision_id: "DEC-605".into(),
        phase: BindingDiscoveryPhase::SearchPlanned,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-nf-001".into()),
            repo_id: None,
            commit_sha: Some("abc123".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 5,
            hint_seeded: false,
        }),
        candidates: vec![],
        caveats: vec![],
        next_actions: vec![FindCodeImpactNextAction {
            action: "provide_hints".into(),
            description: "Provide file/symbol hints to seed a new search".into(),
            command_hint: None,
        }],
        semantic_recall: None,
    }
}

/// Find-code-impact result: unsupported (e.g. Decision not in accepted state).
pub fn find_code_impact_result_unsupported() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Unsupported,
        decision_id: "DEC-606".into(),
        phase: BindingDiscoveryPhase::DiscoveryRequested,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: None,
        search_plan: None,
        candidates: vec![],
        caveats: vec![],
        next_actions: vec![],
        semantic_recall: None,
    }
}

// ---------------------------------------------------------------------------
// Planner quality fixtures — product-language to code-locator translation
// ---------------------------------------------------------------------------

/// Well-formed plan: accepted Decision with rich source context produces a
/// bounded, multi-query search plan with verified and approximate candidates.
pub fn planner_well_formed_plan() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Found,
        decision_id: "DEC-700".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-plan-001".into()),
            repo_id: Some("repo-planner".into()),
            commit_sha: Some("c0ffee1".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 4,
            hint_seeded: false,
        }),
        candidates: vec![
            CodeImpactCandidate {
                target: "src/auth/oauth_provider.rs".into(),
                target_type: LocatorTargetType::Symbol,
                file: "src/auth/oauth_provider.rs".into(),
                range: Some("15:1-42:1".into()),
                symbol: Some("OAuthProvider::exchange_code".into()),
                evidence_state: LocatorEvidenceState::Verified,
                confidence: LocatorConfidence::High,
                reasons: vec![
                    LocatorReason {
                        code: LocatorReasonCode::ExactSymbol,
                        detail: "Symbol matches Decision source: 'OAuth token exchange'".into(),
                    },
                    LocatorReason {
                        code: LocatorReasonCode::ExactPath,
                        detail: "File in auth module at snapshot c0ffee1".into(),
                    },
                ],
                graph_snapshot_id: Some("gs-plan-001".into()),
                validated_sha: Some("c0ffee1".into()),
                content_hash: None,
            },
            CodeImpactCandidate {
                target: "src/auth/token_store.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/auth/token_store.rs".into(),
                range: None,
                symbol: Some("TokenStore::persist".into()),
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Medium,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::ImportEdge,
                    detail: "Import edge from OAuthProvider".into(),
                }],
                graph_snapshot_id: Some("gs-plan-001".into()),
                validated_sha: Some("c0ffee1".into()),
                content_hash: None,
            },
        ],
        caveats: vec![],
        next_actions: vec![FindCodeImpactNextAction {
            action: "binding_create".into(),
            description: "Create binding for verified candidate".into(),
            command_hint: Some("binding.create".into()),
        }],
        semantic_recall: None,
    }
}

/// Ambiguous/broad plan: Decision has vague product language that maps to
/// many candidates, all approximate. The result signals `needs_human_selection`
/// and suggests narrowing.
pub fn planner_ambiguous_broad_plan() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::NeedsHumanSelection,
        decision_id: "DEC-701".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-plan-002".into()),
            repo_id: Some("repo-planner".into()),
            commit_sha: Some("d1ce001".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 5,
            hint_seeded: false,
        }),
        candidates: vec![
            CodeImpactCandidate {
                target: "src/config/settings.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/config/settings.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Low,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Lexical match for 'configuration'".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            },
            CodeImpactCandidate {
                target: "src/config/env.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/config/env.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Low,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Lexical match for 'environment config'".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            },
            CodeImpactCandidate {
                target: "src/config/mod.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/config/mod.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Low,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Lexical match for 'config module root'".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            },
        ],
        caveats: vec![
            CodeImpactCaveat {
                kind: CodeImpactCaveatKind::ApproximateCandidates,
                detail: "All candidates are approximate; source context too broad".into(),
            },
            CodeImpactCaveat {
                kind: CodeImpactCaveatKind::LimitedSourceContext,
                detail: "Decision source context lacks specific symbols or file paths".into(),
            },
        ],
        next_actions: vec![
            FindCodeImpactNextAction {
                action: "narrow_search".into(),
                description: "Provide file/symbol hints to narrow the plan".into(),
                command_hint: None,
            },
            FindCodeImpactNextAction {
                action: "choose_candidate".into(),
                description: "Select the correct candidate for binding".into(),
                command_hint: None,
            },
        ],
        semantic_recall: None,
    }
}

/// Rejected/out-of-scope plan: Decision is not in accepted state, so the
/// planner rejects with `Unsupported`.
pub fn planner_rejected_out_of_scope() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Unsupported,
        decision_id: "DEC-702".into(),
        phase: BindingDiscoveryPhase::DiscoveryRequested,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: None,
        search_plan: None,
        candidates: vec![],
        caveats: vec![],
        next_actions: vec![],
        semantic_recall: None,
    }
}

/// Needs-narrowing plan: planner found candidates but the query exceeded the
/// configured limit, producing a `Truncated` caveat and suggesting narrowing.
pub fn planner_needs_narrowing() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Found,
        decision_id: "DEC-703".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-plan-003".into()),
            repo_id: None,
            commit_sha: Some("beef001".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 8,
            hint_seeded: false,
        }),
        candidates: vec![CodeImpactCandidate {
            target: "src/routes/api.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/routes/api.rs".into(),
            range: Some("1:1-200:1".into()),
            symbol: None,
            evidence_state: LocatorEvidenceState::Verified,
            confidence: LocatorConfidence::Medium,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::ExactPath,
                detail: "File matches Decision source path".into(),
            }],
            graph_snapshot_id: Some("gs-plan-003".into()),
            validated_sha: Some("beef001".into()),
            content_hash: None,
        }],
        caveats: vec![CodeImpactCaveat {
            kind: CodeImpactCaveatKind::Truncated,
            detail: "Results truncated at limit; more candidates may exist".into(),
        }],
        next_actions: vec![
            FindCodeImpactNextAction {
                action: "binding_create".into(),
                description: "Create binding for verified candidate".into(),
                command_hint: Some("binding.create".into()),
            },
            FindCodeImpactNextAction {
                action: "narrow_search".into(),
                description: "Add hints to narrow remaining results".into(),
                command_hint: None,
            },
        ],
        semantic_recall: None,
    }
}

/// Existing bindings plan: planner considers prior bindings and includes a
/// `PriorBinding` reason code on a candidate.
pub fn planner_existing_bindings() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Found,
        decision_id: "DEC-704".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-plan-004".into()),
            repo_id: Some("repo-planner".into()),
            commit_sha: Some("face001".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 2,
            hint_seeded: false,
        }),
        candidates: vec![CodeImpactCandidate {
            target: "src/ledger/replay.rs".into(),
            target_type: LocatorTargetType::Symbol,
            file: "src/ledger/replay.rs".into(),
            range: Some("50:1-80:1".into()),
            symbol: Some("LedgerReplay::apply_event".into()),
            evidence_state: LocatorEvidenceState::Verified,
            confidence: LocatorConfidence::High,
            reasons: vec![
                LocatorReason {
                    code: LocatorReasonCode::PriorBinding,
                    detail: "Prior binding BND-101 covers this symbol".into(),
                },
                LocatorReason {
                    code: LocatorReasonCode::ExactSymbol,
                    detail: "Symbol matches Decision source context".into(),
                },
            ],
            graph_snapshot_id: Some("gs-plan-004".into()),
            validated_sha: Some("face001".into()),
            content_hash: None,
        }],
        caveats: vec![],
        next_actions: vec![FindCodeImpactNextAction {
            action: "binding_create".into(),
            description: "Create binding for verified candidate".into(),
            command_hint: Some("binding.create".into()),
        }],
        semantic_recall: None,
    }
}

/// Weak source evidence plan: planner ran but only found low-confidence
/// approximate candidates because the Decision's source evidence is weak.
pub fn planner_weak_source_evidence() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::NeedsHumanSelection,
        decision_id: "DEC-705".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-plan-005".into()),
            repo_id: None,
            commit_sha: Some("babe001".into()),
            index_version: Some("1".into()),
            ref_name: None,
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "source_context".into(),
            queries_executed: 3,
            hint_seeded: false,
        }),
        candidates: vec![CodeImpactCandidate {
            target: "src/utils/helpers.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/utils/helpers.rs".into(),
            range: None,
            symbol: None,
            evidence_state: LocatorEvidenceState::ApproximateCandidate,
            confidence: LocatorConfidence::Low,
            reasons: vec![LocatorReason {
                code: LocatorReasonCode::LexicalMatch,
                detail: "Weak lexical match from limited source context".into(),
            }],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
        }],
        caveats: vec![
            CodeImpactCaveat {
                kind: CodeImpactCaveatKind::LimitedSourceContext,
                detail: "Decision source evidence is minimal; plan quality limited".into(),
            },
            CodeImpactCaveat {
                kind: CodeImpactCaveatKind::ApproximateCandidates,
                detail: "All candidates are approximate due to weak source evidence".into(),
            },
        ],
        next_actions: vec![
            FindCodeImpactNextAction {
                action: "provide_hints".into(),
                description: "Provide file/symbol hints to improve search".into(),
                command_hint: None,
            },
            FindCodeImpactNextAction {
                action: "choose_candidate".into(),
                description: "Select candidate manually".into(),
                command_hint: None,
            },
        ],
        semantic_recall: None,
    }
}

/// Hint-seeded plan: caller provides hints that constrain the search.
pub fn planner_hint_seeded() -> FindCodeImpactResult {
    FindCodeImpactResult {
        status: FindCodeImpactStatus::Found,
        decision_id: "DEC-706".into(),
        phase: BindingDiscoveryPhase::CandidatesReady,
        graph_readiness: LocatorGraphReadiness::Ready,
        snapshot: Some(LocatorSnapshotInfo {
            graph_snapshot_id: Some("gs-plan-006".into()),
            repo_id: Some("repo-planner".into()),
            commit_sha: Some("cafe001".into()),
            index_version: Some("1".into()),
            ref_name: Some("HEAD".into()),
        }),
        search_plan: Some(SearchPlanSummary {
            strategy: "hint_seeded".into(),
            queries_executed: 2,
            hint_seeded: true,
        }),
        candidates: vec![CodeImpactCandidate {
            target: "src/gateway/routes.rs".into(),
            target_type: LocatorTargetType::Symbol,
            file: "src/gateway/routes.rs".into(),
            range: Some("30:1-55:1".into()),
            symbol: Some("GatewayRouter::handle_request".into()),
            evidence_state: LocatorEvidenceState::Verified,
            confidence: LocatorConfidence::High,
            reasons: vec![
                LocatorReason {
                    code: LocatorReasonCode::ExactSymbol,
                    detail: "Symbol matches caller hint".into(),
                },
                LocatorReason {
                    code: LocatorReasonCode::ExactPath,
                    detail: "File matches caller hint".into(),
                },
            ],
            graph_snapshot_id: Some("gs-plan-006".into()),
            validated_sha: Some("cafe001".into()),
            content_hash: None,
        }],
        caveats: vec![],
        next_actions: vec![FindCodeImpactNextAction {
            action: "binding_create".into(),
            description: "Create binding for verified candidate".into(),
            command_hint: Some("binding.create".into()),
        }],
        semantic_recall: None,
    }
}

/// Request fixture: hint-seeded `decision.find_code_impact` request.
pub fn decision_find_code_impact_hint_seeded() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::DecisionFindCodeImpact(FindCodeImpactParams {
            decision_id: "DEC-706".into(),
            hints: vec![
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::File,
                    value: "src/gateway/routes.rs".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Symbol,
                    value: "GatewayRouter".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Keyword,
                    value: "request routing".into(),
                },
            ],
            workspace_id: Some("/home/user/project".into()),
            ref_name: Some("HEAD".into()),
            commit_sha: None,
            limit: 10,
        }),
        authority_dashboard("user-planner-01", "/home/user/project"),
    )
}

/// Request fixture: broad product-language query with no hints (triggers
/// ambiguous plan).
pub fn decision_find_code_impact_broad_no_hints() -> ToolRequest {
    ToolRequest::new(
        ToolCommand::DecisionFindCodeImpact(FindCodeImpactParams {
            decision_id: "DEC-701".into(),
            hints: vec![],
            workspace_id: Some("/home/user/project".into()),
            ref_name: None,
            commit_sha: None,
            limit: 20,
        }),
        authority_mcp("agent-planner-001", "sess-broad-001", "/home/user/project"),
    )
}

/// Returns all planner quality result fixtures for bulk validation.
pub fn all_planner_quality_fixtures() -> Vec<(&'static str, FindCodeImpactResult)> {
    vec![
        ("well_formed_plan", planner_well_formed_plan()),
        ("ambiguous_broad_plan", planner_ambiguous_broad_plan()),
        ("rejected_out_of_scope", planner_rejected_out_of_scope()),
        ("needs_narrowing", planner_needs_narrowing()),
        ("existing_bindings", planner_existing_bindings()),
        ("weak_source_evidence", planner_weak_source_evidence()),
        ("hint_seeded", planner_hint_seeded()),
        // Include the baseline result fixtures for completeness
        ("verified", find_code_impact_result_verified()),
        ("approximate", find_code_impact_result_approximate()),
        (
            "graph_unavailable",
            find_code_impact_result_graph_unavailable(),
        ),
        ("graph_stale", find_code_impact_result_graph_stale()),
        ("not_found", find_code_impact_result_not_found()),
        ("unsupported", find_code_impact_result_unsupported()),
    ]
}

// ---------------------------------------------------------------------------
// All-fixture collection (for bulk validation)
// ---------------------------------------------------------------------------

/// Returns every accepted lifecycle fixture for bulk round-trip testing.
pub fn all_accepted_fixtures() -> Vec<(&'static str, ToolRequest)> {
    vec![
        ("ingest.submit_local", ingest_submit_local()),
        (
            "ingest.submit_managed",
            ingest_submit_managed("github", "github_repo_primary"),
        ),
        ("preflight.run", preflight_run()),
        ("binding.create", binding_create()),
        ("binding.inspect", binding_inspect()),
        ("review.accept_candidate", review_accept_candidate()),
        ("review.reject_candidate", review_reject_candidate()),
        ("review.approve_signoff", review_approve_signoff()),
        ("review.reject_signoff", review_reject_signoff()),
        ("review.resolve_compliance", review_resolve_compliance()),
        ("history.list", history_list()),
        ("search.query", search_query()),
        ("tracking.untrack_source", tracking_untrack_source()),
        ("tracking.refresh_query", tracking_refresh_query()),
        ("code.locate", code_locate()),
        ("graph.status", graph_status()),
        ("graph.refresh_snapshot", graph_refresh_snapshot()),
        ("decision.find_code_impact", decision_find_code_impact()),
        ("privacy.erase_subject", privacy_erase_subject()),
    ]
}

/// Returns one request for each known command variant.
pub fn one_request_per_command() -> Vec<ToolRequest> {
    let mut requests = all_accepted_fixtures()
        .into_iter()
        .map(|(_, req)| req)
        .collect::<Vec<_>>();
    requests.push(evidence_refresh("DEC-501"));
    requests.push(recall_inspect_evidence());
    requests.push(recall_expand_scope());
    requests.push(recall_promote_decision_candidate());
    requests.push(recall_request_correction());
    requests.push(review_transition_finding());
    requests.push(workspace_bind_request());
    requests.push(workspace_resolve_request());
    requests.push(product_create_request());
    requests.push(product_publish_request());
    requests.push(candidate_hide());
    requests.push(candidate_suppress());
    requests.push(candidate_selection_prepare_request());
    requests.push(candidate_selection_commit_request());
    requests.push(candidate_session_heartbeat_request());
    requests.push(candidate_session_end_request());
    requests
}

/// `candidate_selection.prepare` wire fixture (issues #987 / #989).
pub fn candidate_selection_prepare_request() -> ToolRequest {
    use crate::candidate_selection::{CandidateSelectionV1, CANDIDATE_SELECTION_CONTRACT_VERSION};

    let command = ToolCommand::CandidateSelectionPrepare(CandidateSelectionV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::from_u128(0x301),
        candidate_set_id: Uuid::from_u128(0x101),
        candidate_set_generation: 1,
        candidate_set_digest: format!("sha256:{}", "a".repeat(64)),
        product_id: "prd_fixture".into(),
        host_session_id: "host-fixture".into(),
        session_lease_id: "lease-fixture".into(),
        promote_candidate_ids: vec![Uuid::from_u128(0x201)],
        reject_candidate_ids: vec![],
        actor_id: "user-owner-01".into(),
    });
    ToolRequest::new(
        command,
        authority_mcp("user-owner-01", "sess-selection", "/tmp/fixture"),
    )
}

/// `candidate_selection.commit` wire fixture (issues #987 / #989).
pub fn candidate_selection_commit_request() -> ToolRequest {
    use crate::candidate_selection::{
        CandidateSelectionCommitV1, CANDIDATE_SELECTION_CONTRACT_VERSION,
    };
    use crate::confirmation_challenge::{ChallengeSecret, ConfirmationToken};

    let command = ToolCommand::CandidateSelectionCommit(CandidateSelectionCommitV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::from_u128(0x301),
        selection_digest: format!("sha256:{}", "d".repeat(64)),
        actor_id: "user-owner-01".into(),
        human_confirmed: true,
        confirmation_token: ConfirmationToken {
            challenge_id: Uuid::from_u128(0x401),
            restart_epoch: Uuid::from_u128(0x402),
            secret: ChallengeSecret::from_minted("fixture-opaque-token"),
        },
    });
    ToolRequest::new(
        command,
        authority_mcp("user-owner-01", "sess-selection", "/tmp/fixture"),
    )
}

/// `candidate_session.heartbeat` wire fixture (issues #987 / #989).
pub fn candidate_session_heartbeat_request() -> ToolRequest {
    use crate::candidate_session::{CandidateSessionAccessV1, CandidateSessionHeartbeatRequestV1};

    let command = ToolCommand::CandidateSessionHeartbeat(CandidateSessionHeartbeatRequestV1 {
        access: CandidateSessionAccessV1::new("prd_fixture", "host-fixture", "lease-fixture", 1),
    });
    ToolRequest::new(
        command,
        authority_mcp("user-owner-01", "sess-selection", "/tmp/fixture"),
    )
}

/// `candidate_session.end` wire fixture (issues #987 / #989).
pub fn candidate_session_end_request() -> ToolRequest {
    use crate::candidate_session::{CandidateSessionAccessV1, CandidateSessionEndRequestV1};

    let command = ToolCommand::CandidateSessionEnd(CandidateSessionEndRequestV1 {
        access: CandidateSessionAccessV1::new("prd_fixture", "host-fixture", "lease-fixture", 1),
    });
    ToolRequest::new(
        command,
        authority_mcp("user-owner-01", "sess-selection", "/tmp/fixture"),
    )
}

/// A hosted `product.create` request fixture (bicameral-bot#766).
///
/// Describes the wire shape a paired hosted surface routes to create a
/// canonical Product. The daemon — never the surface — confirms the paired
/// actor, binds the Cloud account, materializes creator authority, and persists
/// the initial Governance Policy Version. Caller-asserted fields are advisory.
pub fn product_create_request() -> ToolRequest {
    use crate::product_creation::{EventStoreHint, ProductCreateParams, RequiredConnection};

    let command = ToolCommand::ProductCreate(ProductCreateParams {
        idempotency_key: "prd-create-idem-1".into(),
        product_display_name: "Acme Checkout".into(),
        event_store_descriptor: None,
        asserted_cloud_account_id: Some("acct_01HZX8PRIYA".into()),
        team_display_name: Some("Acme".into()),
        verified_domain: Some("acme.example".into()),
        event_store_hint: Some(EventStoreHint {
            substrate: Some("git".into()),
            provider: Some("github".into()),
        }),
        required_connection: Some(RequiredConnection {
            provider: Some("github".into()),
        }),
    });

    ToolRequest::new(command, authority_hosted_bridge("acct_01HZX8PRIYA"))
}

/// A hosted `product.publish` request fixture (bicameral-bot#766).
///
/// Describes the wire shape for an independently retryable, non-canonical Cloud
/// publication that references an already-created canonical Product id and its
/// safe creation receipt. Publication never re-runs canonical creation.
pub fn product_publish_request() -> ToolRequest {
    use crate::product_creation::ProductPublishParams;

    let command = ToolCommand::ProductPublish(ProductPublishParams {
        product_id: "prd_01HZX8CHECKOUT".into(),
        idempotency_key: "prd_01HZX8CHECKOUT:publish".into(),
    });

    ToolRequest::new(command, authority_hosted_bridge("acct_01HZX8PRIYA"))
}

/// A confirmed `workspace.bind` request fixture (bicameral-bot#732).
///
/// Describes the wire shape a surface builds to propose a daemon-owned local
/// workspace binding. The daemon — never the surface — validates registration,
/// session scope, candidate path safety, and confirmation before persisting
/// operator-local [`LocalWorkspaceBinding`](crate::workspace_binding) state.
pub fn workspace_bind_request() -> ToolRequest {
    use crate::workspace_binding::{
        ProjectSafeDisplayMetadata, WorkspaceBindRequest, WorkspaceBindingProposal,
        WorkspaceBindingSourceSurface, WorkspaceBindingState,
    };

    let command = ToolCommand::WorkspaceBind(WorkspaceBindRequest {
        proposal: WorkspaceBindingProposal {
            project_id: "proj_ada".into(),
            display: ProjectSafeDisplayMetadata {
                display_name: "Ada Service".into(),
                project_slug: Some("ada-service".into()),
                candidate_label: Some("ada-service".into()),
            },
            candidate_path: "/home/operator/projects/ada-service".into(),
            source_surface: WorkspaceBindingSourceSurface::Mcp,
            reason: "Open editor workspace matches the registered project.".into(),
            confidence: 0.92,
        },
        confirmed: true,
        required_daemon_capability: None,
        expected_current_state: Some(WorkspaceBindingState::LocalWorkspaceUnbound),
    });

    ToolRequest::new(
        command,
        authority_mcp(
            "agent-732",
            "sess-732",
            "/home/operator/projects/ada-service",
        ),
    )
}

/// A `workspace.resolve` request fixture (mcp#831).
///
/// Local-only reverse lookup from a host folder hint to at most one
/// already-materialized binding. Routing fact only — not Product identity
/// and not a bind write.
pub fn workspace_resolve_request() -> ToolRequest {
    use crate::workspace_binding::WorkspaceResolveRequest;

    let command = ToolCommand::WorkspaceResolve(WorkspaceResolveRequest {
        candidate_path: "/home/operator/projects/ada-service".into(),
    });

    ToolRequest::new(
        command,
        authority_mcp(
            "agent-831",
            "sess-831",
            "/home/operator/projects/ada-service",
        ),
    )
}

/// Returns every negative/rejected fixture with its expected rejection class.
pub fn all_negative_fixtures() -> Vec<(&'static str, &'static str, ToolRequest)> {
    vec![
        (
            "missing_authority",
            "Rejected: missing_authority",
            negative_missing_actor_id(),
        ),
        (
            "malformed_payload",
            "Rejected: malformed_payload",
            negative_malformed_ingest_empty_title(),
        ),
        (
            "validation_token_unsupported",
            "Rejected: validation_token_unsupported",
            negative_validation_token_acceptance(),
        ),
        (
            "preflight_readonly_shape",
            "Ok (readonly — verify no mutation)",
            negative_preflight_readonly_shape(),
        ),
        (
            "unsafe_evidence",
            "Rejected: unsafe_evidence",
            negative_unsafe_evidence(),
        ),
        (
            "forbidden_canonical_field",
            "Rejected: forbidden_canonical_field",
            negative_forbidden_canonical_field(),
        ),
        (
            "legacy_mcp_payload",
            "Rejected: legacy_mcp_payload",
            negative_legacy_mcp_payload(),
        ),
        (
            "external_adapter_payload",
            "Rejected: external_adapter_payload",
            negative_external_adapter_payload(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Provider item-envelope fixture constructors
// ---------------------------------------------------------------------------

/// Provider item-envelope fixture: a GitHub issue fetched from a provider.
///
/// This is a **provider fact**, not bot-owned evidence or a decision candidate.
/// It enters the bot lifecycle only after explicit bridge promotion through
/// source/evidence capture paths.
pub fn provider_item_envelope_github_issue() -> crate::provider_acquisition::ProviderItemEnvelope {
    crate::provider_acquisition::ProviderItemEnvelope {
        provider: "github".into(),
        resource_id: "acme/checkout".into(),
        item_id: "issue-123".into(),
        item_type: "issue".into(),
        content: "Add rate limiting to the checkout endpoint".into(),
        content_hash: Some(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        ),
        title: Some("Rate limiting for checkout".into()),
        uri: Some("https://github.com/acme/checkout/issues/123".into()),
        freshness: Some(crate::provider_acquisition::FreshnessMetadata {
            last_modified: Some("2026-01-20T14:30:00Z".into()),
            etag: Some("W/\"abc123\"".into()),
            item_count: None,
        }),
        provider_metadata: serde_json::Map::new(),
        fetched_at: "2026-01-20T15:00:00Z".into(),
    }
}

/// Provider item-envelope fixture: a Slack message fetched from a provider.
pub fn provider_item_envelope_slack_message() -> crate::provider_acquisition::ProviderItemEnvelope {
    crate::provider_acquisition::ProviderItemEnvelope {
        provider: "slack".into(),
        resource_id: "C01234ABCDE".into(),
        item_id: "msg-1706000000.000100".into(),
        item_type: "message".into(),
        content: "We decided to move auth to a separate service".into(),
        content_hash: None,
        title: None,
        uri: Some("https://acme.slack.com/archives/C01234ABCDE/p1706000000000100".into()),
        freshness: None,
        provider_metadata: serde_json::Map::new(),
        fetched_at: "2026-01-20T16:00:00Z".into(),
    }
}

/// Provider item-envelope fixture: minimal required fields only.
pub fn provider_item_envelope_minimal() -> crate::provider_acquisition::ProviderItemEnvelope {
    crate::provider_acquisition::ProviderItemEnvelope {
        provider: "linear".into(),
        resource_id: "team-proj".into(),
        item_id: "LIN-42".into(),
        item_type: "issue".into(),
        content: "Implement caching layer for product catalog".into(),
        content_hash: None,
        title: None,
        uri: None,
        freshness: None,
        provider_metadata: serde_json::Map::new(),
        fetched_at: "2026-01-21T09:00:00Z".into(),
    }
}

// ---------------------------------------------------------------------------
// Recall impact-to-decision fixture constructors (#559)
// ---------------------------------------------------------------------------

/// Build a RecallPacket with accepted binding overlap — the highest-confidence
/// positive recall scenario. The packet surfaces a single canonical decision
/// that has verified binding to the impacted target.
pub fn recall_packet_binding_overlap() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        packet_id: Uuid::parse_str("a0000001-0001-4000-8000-000000000001").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: vec![CanonicalDecisionEntry {
            decision_id: Uuid::parse_str("d0000001-0001-4000-8000-000000000001").unwrap(),
            title: "API handler must validate input before dispatch".into(),
            summary: "All API handler endpoints must validate request payload before routing to domain logic.".into(),
            level: crate::candidate::DecisionLevel::Architecture,
            signoff_state: crate::dashboard::SignoffState::Approved,
            evidence_refs: vec![EvidenceRef {
                id: Uuid::parse_str("e0000001-0001-4000-8000-000000000001").unwrap(),
                label: "Binding to src/api/handler.rs:validate_input".into(),
                kind: EvidenceRefKind::CodeRegion,
            }],
            authority_labels: vec![AuthorityLabel {
                kind: AuthorityLabelKind::HumanRatified,
                description: "Accepted binding overlap in requested scope".into(),
            }],
        }],
        decision_candidates: Vec::new(),
        unknown_scope: Vec::new(),
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
}

/// Build a RecallPacket with graph overlap — tier-2 ranking. The packet
/// surfaces a decision found via graph neighborhood expansion, with an
/// unknown scope entry for areas not fully indexed.
pub fn recall_packet_graph_overlap() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        packet_id: Uuid::parse_str("a0000002-0002-4000-8000-000000000002").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: vec![CanonicalDecisionEntry {
            decision_id: Uuid::parse_str("d0000002-0002-4000-8000-000000000002").unwrap(),
            title: "Auth module must use token-based validation".into(),
            summary: "All authentication flows must validate bearer tokens before granting access."
                .into(),
            level: crate::candidate::DecisionLevel::Architecture,
            signoff_state: crate::dashboard::SignoffState::Approved,
            evidence_refs: vec![EvidenceRef {
                id: Uuid::parse_str("e0000002-0002-4000-8000-000000000002").unwrap(),
                label: "Graph overlap: auth module symbol neighborhood".into(),
                kind: EvidenceRefKind::CodeRegion,
            }],
            authority_labels: vec![AuthorityLabel {
                kind: AuthorityLabelKind::HumanRatified,
                description: "Graph overlap in auth module".into(),
            }],
        }],
        decision_candidates: Vec::new(),
        unknown_scope: vec![UnknownScopeEntry {
            scope_id: "auth-middleware".into(),
            description: "Auth middleware chain not fully indexed in graph snapshot".into(),
            expansion_targets: vec!["src/middleware/auth.rs".into()],
        }],
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
}

/// Build a RecallPacket with corpus search results — tier-4 ranking.
/// Includes both a canonical decision and a surfaced candidate.
pub fn recall_packet_corpus_search() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        // decision_attention is derived below via with_derived_decision_attention.
        packet_id: Uuid::parse_str("a0000003-0003-4000-8000-000000000003").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: vec![CanonicalDecisionEntry {
            decision_id: Uuid::parse_str("d0000003-0003-4000-8000-000000000003").unwrap(),
            title: "Checkout flow must validate payment before order creation".into(),
            summary: "Payment validation is a prerequisite for order materialization.".into(),
            level: crate::candidate::DecisionLevel::Product,
            signoff_state: crate::dashboard::SignoffState::Approved,
            evidence_refs: vec![EvidenceRef {
                id: Uuid::parse_str("e0000003-0003-4000-8000-000000000003").unwrap(),
                label: "Corpus lifecycle tag: checkout".into(),
                kind: EvidenceRefKind::SourceSpan,
            }],
            authority_labels: vec![AuthorityLabel {
                kind: AuthorityLabelKind::HumanRatified,
                description: "Corpus lifecycle tag match".into(),
            }],
        }],
        decision_candidates: vec![SurfacedDecisionCandidate {
            candidate_id: Uuid::parse_str("c0000003-0003-4000-8000-000000000003").unwrap(),
            title: "Cart validation before checkout".into(),
            summary: "Cart contents should be validated before entering checkout flow.".into(),
            source_refs: vec![SourceRef {
                uri: "https://linear.app/team/issue/PROJ-99".into(),
                kind: "linear".into(),
                cited_span: Some("Cart validation is required before checkout".into()),
            }],
            origin_surface: "linear".into(),
            extraction_confidence: crate::candidate::ExtractionConfidence::Medium,
            ambiguities: vec!["Scope of cart validation unclear".into()],
            related_decision_refs: vec![
                Uuid::parse_str("d0000003-0003-4000-8000-000000000003").unwrap()
            ],
            candidate_relationship: CandidateRelationship::KeepsBothWithScope,
            relevance_reasons: vec![CandidateRelevance::SourceTextMatch],
            evidence_freshness: CandidateEvidenceFreshness::Current,
            allowed_outcomes: vec![
                PromotionOutcome::NewConstraint,
                PromotionOutcome::NeedsSignoffReview,
                PromotionOutcome::DismissedAsNoise,
                PromotionOutcome::SupersedesExistingDecision,
                PromotionOutcome::KeepsBothWithScope,
            ],
            code_evidence_refs: Vec::new(),
            review_state: crate::candidate::ReviewLifecycleState::Proposed,
            candidate_state: crate::candidate::CandidateState::Proposed,
        }],
        unknown_scope: Vec::new(),
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
    .with_derived_decision_attention()
}

/// Build a RecallPacket from partial-scope search with no matches.
/// The packet must disclose unknown scope and must not claim global absence.
pub fn recall_packet_partial_scope_no_match() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        packet_id: Uuid::parse_str("a0000004-0004-4000-8000-000000000004").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: Vec::new(),
        decision_candidates: Vec::new(),
        unknown_scope: vec![
            UnknownScopeEntry {
                scope_id: "core-module".into(),
                description: "src/core/ not included in search scope".into(),
                expansion_targets: vec!["src/core/".into()],
            },
            UnknownScopeEntry {
                scope_id: "integrations-module".into(),
                description: "src/integrations/ not included in search scope".into(),
                expansion_targets: vec!["src/integrations/".into()],
            },
            UnknownScopeEntry {
                scope_id: "middleware-module".into(),
                description: "src/middleware/ not included in search scope".into(),
                expansion_targets: vec!["src/middleware/".into()],
            },
        ],
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
}

/// Build a RecallPacket with advisory-only candidates from semantic similarity.
/// These must not carry human_ratified labels or assert compliance.
pub fn recall_packet_semantic_similarity_advisory() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        // decision_attention is derived below via with_derived_decision_attention.
        packet_id: Uuid::parse_str("a0000005-0005-4000-8000-000000000005").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: Vec::new(),
        decision_candidates: vec![SurfacedDecisionCandidate {
            candidate_id: Uuid::parse_str("c0000005-0005-4000-8000-000000000005").unwrap(),
            title: "Semantic match: rate limiting configuration".into(),
            summary: "Corpus text similarity to rate limiting decisions.".into(),
            source_refs: vec![SourceRef {
                uri: "https://docs.internal/rate-limiting".into(),
                kind: "document".into(),
                cited_span: Some("Rate limiting must be applied to all public endpoints".into()),
            }],
            origin_surface: "corpus_search".into(),
            extraction_confidence: crate::candidate::ExtractionConfidence::Low,
            ambiguities: vec!["Similarity-only match — no deterministic verification".into()],
            related_decision_refs: Vec::new(),
            candidate_relationship: CandidateRelationship::NeedsSignoffReview,
            relevance_reasons: vec![CandidateRelevance::SourceTextMatch],
            evidence_freshness: CandidateEvidenceFreshness::Current,
            allowed_outcomes: vec![
                PromotionOutcome::NewConstraint,
                PromotionOutcome::NeedsSignoffReview,
                PromotionOutcome::DismissedAsNoise,
            ],
            code_evidence_refs: Vec::new(),
            review_state: crate::candidate::ReviewLifecycleState::Proposed,
            candidate_state: crate::candidate::CandidateState::Proposed,
        }],
        unknown_scope: Vec::new(),
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
    .with_derived_decision_attention()
}

/// Build a RecallPacket with stale evidence that must be downgraded.
pub fn recall_packet_stale_evidence() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        packet_id: Uuid::parse_str("a0000006-0006-4000-8000-000000000006").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: vec![CanonicalDecisionEntry {
            decision_id: Uuid::parse_str("d0000006-0006-4000-8000-000000000006").unwrap(),
            title: "Stale: DB schema migration requires downtime".into(),
            summary:
                "Decision from outdated graph snapshot — may no longer reflect current schema."
                    .into(),
            level: crate::candidate::DecisionLevel::Implementation,
            signoff_state: crate::dashboard::SignoffState::Approved,
            evidence_refs: vec![EvidenceRef {
                id: Uuid::parse_str("e0000006-0006-4000-8000-000000000006").unwrap(),
                label: "Stale graph snapshot reference".into(),
                kind: EvidenceRefKind::CodeRegion,
            }],
            authority_labels: vec![AuthorityLabel {
                kind: AuthorityLabelKind::Stale,
                description: "Graph snapshot older than freshness threshold".into(),
            }],
        }],
        decision_candidates: Vec::new(),
        unknown_scope: vec![UnknownScopeEntry {
            scope_id: "stale-graph-region".into(),
            description: "Graph snapshot for DB schema region is stale — re-index required".into(),
            expansion_targets: vec!["src/db/migrations/".into()],
        }],
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
}

/// Build a RecallPacket simulating preference-filtered results where an
/// accepted binding overlap decision must survive despite preference suppression.
pub fn recall_packet_preference_must_not_hide() -> crate::recall::RecallPacket {
    use crate::recall::*;
    RecallPacket {
        packet_id: Uuid::parse_str("a0000007-0007-4000-8000-000000000007").unwrap(),
        decision_attention: DecisionAttention::not_warranted(),
        canonical_decisions: vec![CanonicalDecisionEntry {
            decision_id: Uuid::parse_str("d0000007-0007-4000-8000-000000000007").unwrap(),
            title: "Backend API rate limit must not exceed 100 rps per client".into(),
            summary: "Rate limit policy for backend API is a binding constraint.".into(),
            level: crate::candidate::DecisionLevel::Architecture,
            signoff_state: crate::dashboard::SignoffState::Approved,
            evidence_refs: vec![EvidenceRef {
                id: Uuid::parse_str("e0000007-0007-4000-8000-000000000007").unwrap(),
                label: "Accepted binding to src/api/rate_limit.rs".into(),
                kind: EvidenceRefKind::CodeRegion,
            }],
            authority_labels: vec![AuthorityLabel {
                kind: AuthorityLabelKind::HumanRatified,
                description: "Accepted binding overlap — must not be hidden by preferences".into(),
            }],
        }],
        decision_candidates: Vec::new(),
        unknown_scope: Vec::new(),
        allowed_next_actions: RecallAction::alpha_actions(),
        searched_scope: None,
        recall_trace: None,
        freshness: None,
        semantic_recall: None,
    }
}

/// Collect all recall impact fixture packets for batch testing.
pub fn all_recall_impact_fixtures() -> Vec<(&'static str, crate::recall::RecallPacket)> {
    vec![
        ("binding_overlap", recall_packet_binding_overlap()),
        ("graph_overlap", recall_packet_graph_overlap()),
        ("corpus_search", recall_packet_corpus_search()),
        (
            "partial_scope_no_match",
            recall_packet_partial_scope_no_match(),
        ),
        (
            "semantic_similarity_advisory",
            recall_packet_semantic_similarity_advisory(),
        ),
        ("stale_evidence", recall_packet_stale_evidence()),
        (
            "preference_must_not_hide",
            recall_packet_preference_must_not_hide(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Hosted bridge protocol fixtures
// ---------------------------------------------------------------------------

use crate::hosted_bridge::*;

/// Hosted bridge capability discovery — daemon advertises bridge support.
pub fn hosted_bridge_capability_discovery() -> HostedBridgeCapability {
    HostedBridgeCapability {
        bridge_supported: true,
        daemon_build_version: DAEMON_BUILD_VERSION.into(),
        hosted_bridge_protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        bridge_protocol_version: "0.1.0".into(),
        daemon_protocol_version: HOSTED_DASHBOARD_PROTOCOL_VERSION,
        project_list_available: true,
        workspace_binding_available: false,
        note: Some("Bridge ready; workspace binding requires project context.".into()),
    }
}

/// Hosted bridge capability discovery — daemon does not support bridge.
pub fn hosted_bridge_capability_unsupported() -> HostedBridgeCapability {
    HostedBridgeCapability {
        bridge_supported: false,
        daemon_build_version: DAEMON_BUILD_VERSION.into(),
        hosted_bridge_protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        bridge_protocol_version: "0.1.0".into(),
        daemon_protocol_version: HOSTED_DASHBOARD_PROTOCOL_VERSION,
        project_list_available: false,
        workspace_binding_available: false,
        note: Some("This daemon build does not include hosted bridge support.".into()),
    }
}

/// Pairing request from hosted web origin.
pub fn hosted_bridge_pairing_request() -> HostedBridgePairingRequest {
    HostedBridgePairingRequest {
        origin: "https://app.bicameral-ai.com".into(),
        cloud_account_id: "acct-cloud-001".into(),
        csrf_token: "csrf-fixture-token-abc123".into(),
        requested_ttl_seconds: Some(3600),
    }
}

/// Successful pairing response — state transitions to `Paired`.
pub fn hosted_bridge_paired_response() -> HostedBridgePairingResponse {
    HostedBridgePairingResponse {
        state: HostedBridgePairingState::Paired,
        session_token: "bridge-sess-fixture-001".into(),
        expires_at: "2026-07-01T00:00:00Z".into(),
        capabilities: hosted_bridge_capability_discovery(),
    }
}

// ── Pending pairing / local-confirmation flow fixtures (#790) ──────────────
//
// Pinned to `HOSTED_BRIDGE_PROTOCOL_VERSION` (v3). The pending response and the
// non-approved status responses must never carry a session token; only the
// approved status handoff releases one.

/// Pending pairing response returned by `POST /pair` under the local-confirmation
/// flow (202). Carries an opaque handle/proof, expiry, capability/version, and a
/// same-origin confirmation affordance — never a session token.
pub fn hosted_bridge_pending_response() -> HostedBridgePairingPendingResponse {
    HostedBridgePairingPendingResponse {
        status: PendingPairingOutcome::Pending,
        request_handle: "pending-fixture-handle-001".into(),
        poll_proof: "pending-fixture-proof-001".into(),
        pairing_code: "7K2M-PQ8R".into(),
        expires_at: "2026-07-01T00:05:00Z".into(),
        protocol_version: HOSTED_BRIDGE_PROTOCOL_VERSION,
        capabilities: hosted_bridge_capability_discovery(),
        confirmation: LocalConfirmationAffordance {
            local_confirmation_url: Some("/local/pairing?request=pending-fixture-handle-001".into()),
            terminal_command: "bicameral pairing approve".into(),
            message: "Approve this browser in Bicameral on this computer, or run `bicameral pairing approve` and enter this code.".into(),
            requesting_origin: "https://app.bicameral-ai.com".into(),
            cloud_account_id: "acct-cloud-001".into(),
            capability_summary: pairing_capability_summary(),
        },
    }
}

/// Local-surface view of a pending request (served only to the local surface).
pub fn hosted_bridge_pending_local_view() -> PendingPairingLocalView {
    PendingPairingLocalView {
        status: PendingPairingOutcome::Pending,
        request_handle: "pending-fixture-handle-001".into(),
        requesting_origin: "https://app.bicameral-ai.com".into(),
        cloud_account_id: "acct-cloud-001".into(),
        capability_summary: pairing_capability_summary(),
        expires_at: "2026-07-01T00:05:00Z".into(),
        local_confirmation_nonce: "pending-fixture-nonce-001".into(),
    }
}

/// Approved status handoff — the only status outcome that releases a token.
pub fn hosted_bridge_status_approved() -> HostedBridgePairingStatusResponse {
    HostedBridgePairingStatusResponse {
        status: PendingPairingOutcome::Approved,
        session_token: Some("bridge-sess-fixture-001".into()),
        expires_at: Some("2026-07-01T01:00:00Z".into()),
        capabilities: Some(hosted_bridge_capability_discovery()),
        message: Some("Pairing approved.".into()),
    }
}

/// Denied status outcome — terminal, never reveals a token.
pub fn hosted_bridge_status_denied() -> HostedBridgePairingStatusResponse {
    HostedBridgePairingStatusResponse {
        status: PendingPairingOutcome::Denied,
        session_token: None,
        expires_at: None,
        capabilities: None,
        message: Some("Pairing request was denied.".into()),
    }
}

/// Expired status outcome — terminal, never reveals a token.
pub fn hosted_bridge_status_expired() -> HostedBridgePairingStatusResponse {
    HostedBridgePairingStatusResponse {
        status: PendingPairingOutcome::Expired,
        session_token: None,
        expires_at: None,
        capabilities: None,
        message: Some("Pairing request expired before it was confirmed.".into()),
    }
}

/// Typed update-required outcome for an incompatible old immediate-pair
/// consumer/daemon (pinned to the confirmation floor).
pub fn hosted_bridge_pairing_update_required() -> HostedBridgePairingUpdateRequiredResponse {
    pairing_update_required_response(2)
}

/// Unpaired bridge response — no active pairing exists.
pub fn hosted_bridge_unpaired_response() -> HostedBridgeErrorResponse {
    HostedBridgeErrorResponse {
        error: HostedBridgeErrorKind::NotPaired,
        message: "No active bridge pairing. Initiate pairing from the hosted web.".into(),
        retry_after_repair: true,
    }
}

/// Revoked bridge response — prior pairing revoked or expired.
pub fn hosted_bridge_revoked_response() -> HostedBridgeRevokedResponse {
    HostedBridgeRevokedResponse {
        state: HostedBridgePairingState::Revoked,
        reason: "Pairing revoked by operator.".into(),
        revoked_at: "2026-06-25T12:00:00Z".into(),
        re_pair_available: true,
    }
}

/// Operator-home session — post-pairing projection data.
///
/// Projects are listed as non-authoritative projection data. Workspace binding
/// is explicitly unavailable at this level.
pub fn hosted_bridge_operator_home_session() -> OperatorHomeSessionResponse {
    OperatorHomeSessionResponse {
        pairing_state: HostedBridgePairingState::Paired,
        projects: vec![
            ProjectReadinessEntry {
                name: "my-service".into(),
                event_store_healthy: true,
                has_workspace_binding: true,
                setup_state: "normal".into(),
            },
            ProjectReadinessEntry {
                name: "data-pipeline".into(),
                event_store_healthy: true,
                has_workspace_binding: false,
                setup_state: "repair_required".into(),
            },
        ],
        capability_readiness: Vec::new(),
        workspace_binding_available: false,
        status_message: Some("Paired. Select a project to access workspace features.".into()),
    }
}

/// Selected hosted project reference — safe cloud-coordination identity (#719).
pub fn hosted_bridge_selected_project_ref() -> SelectedProjectRef {
    SelectedProjectRef {
        project_id: "my-service".into(),
        team_id: Some("team-acme".into()),
        display_name: Some("My Service".into()),
    }
}

/// Selected-project session — the selected project is bound and ready (#719).
///
/// Proves the selected Cloud project id round-trips through the scoped
/// projection and that a fully-bound local project reports `ready`.
pub fn hosted_bridge_selected_project_ready() -> SelectedProjectSessionResponse {
    resolve_selected_project_readiness(
        HostedBridgePairingState::Paired,
        &hosted_bridge_selected_project_ref(),
        &[ProjectReadinessEntry {
            name: "my-service".into(),
            event_store_healthy: true,
            has_workspace_binding: true,
            setup_state: "normal".into(),
        }],
        "2026-06-25T12:00:00Z",
    )
}

/// Selected-project session — the daemon has no local binding for the selected
/// project, so readiness is `unbound` ("needs binding") (#719).
///
/// The selected project id is NOT in the daemon's local list; the daemon must
/// report `unbound` rather than treat its local list as the Cloud membership
/// source or reject the selection as a non-member.
pub fn hosted_bridge_selected_project_needs_binding() -> SelectedProjectSessionResponse {
    resolve_selected_project_readiness(
        HostedBridgePairingState::Paired,
        &SelectedProjectRef {
            project_id: "unbound-project".into(),
            team_id: Some("team-acme".into()),
            display_name: Some("Unbound Project".into()),
        },
        &[ProjectReadinessEntry {
            name: "my-service".into(),
            event_store_healthy: true,
            has_workspace_binding: true,
            setup_state: "normal".into(),
        }],
        "2026-06-25T12:00:00Z",
    )
}

/// Error: wrong daemon instance.
pub fn hosted_bridge_wrong_daemon() -> HostedBridgeErrorResponse {
    HostedBridgeErrorResponse {
        error: HostedBridgeErrorKind::WrongDaemon,
        message: "Bridge session was paired to a different daemon instance.".into(),
        retry_after_repair: true,
    }
}

/// Error: project not found or not registered locally.
pub fn hosted_bridge_wrong_project() -> HostedBridgeErrorResponse {
    HostedBridgeErrorResponse {
        error: HostedBridgeErrorKind::ProjectNotFound,
        message: "Project is not registered on this daemon.".into(),
        retry_after_repair: false,
    }
}

/// Error: cloud account mismatch.
pub fn hosted_bridge_wrong_account() -> HostedBridgeErrorResponse {
    HostedBridgeErrorResponse {
        error: HostedBridgeErrorKind::AccountMismatch,
        message: "Cloud account does not match the paired account.".into(),
        retry_after_repair: true,
    }
}

/// Error: unsupported bridge command.
pub fn hosted_bridge_unsupported_command() -> HostedBridgeErrorResponse {
    HostedBridgeErrorResponse {
        error: HostedBridgeErrorKind::UnsupportedCommand,
        message: "This bridge command is not supported by the current daemon version.".into(),
        retry_after_repair: false,
    }
}

// ---------------------------------------------------------------------------
// Hosted cloud contract fixtures — compatibility envelope (#612)
// ---------------------------------------------------------------------------

/// Compatibility envelope: current-version match — fully compatible.
pub fn hosted_bridge_compat_current_version() -> ProtocolCompatibilityEnvelope {
    ProtocolCompatibilityEnvelope {
        cloud_protocol_version: "0.1.0".into(),
        daemon_protocol_version: "0.1.0".into(),
        outcome: CompatibilityOutcome::FullyCompatible,
        mutable_controls_available: true,
        read_only_projections_available: true,
        note: Some("Current-version match. All features available.".into()),
    }
}

/// Compatibility envelope: previous-minor version — degraded but compatible.
pub fn hosted_bridge_compat_previous_minor() -> ProtocolCompatibilityEnvelope {
    ProtocolCompatibilityEnvelope {
        cloud_protocol_version: "0.2.0".into(),
        daemon_protocol_version: "0.1.0".into(),
        outcome: CompatibilityOutcome::DegradedCompatible,
        mutable_controls_available: true,
        read_only_projections_available: true,
        note: Some(
            "Previous-minor daemon. Mutable controls available with degraded feature set.".into(),
        ),
    }
}

/// Compatibility envelope: daemon too old — blocked.
pub fn hosted_bridge_compat_too_old_daemon() -> ProtocolCompatibilityEnvelope {
    ProtocolCompatibilityEnvelope {
        cloud_protocol_version: "1.0.0".into(),
        daemon_protocol_version: "0.1.0".into(),
        outcome: CompatibilityOutcome::Blocked,
        mutable_controls_available: false,
        read_only_projections_available: false,
        note: Some("Daemon version too old. Upgrade required.".into()),
    }
}

/// Compatibility envelope: newer daemon — mutable controls blocked.
pub fn hosted_bridge_compat_newer_daemon() -> ProtocolCompatibilityEnvelope {
    ProtocolCompatibilityEnvelope {
        cloud_protocol_version: "0.1.0".into(),
        daemon_protocol_version: "0.3.0".into(),
        outcome: CompatibilityOutcome::NewerDaemonDegraded,
        mutable_controls_available: false,
        read_only_projections_available: true,
        note: Some("Daemon is newer than cloud surface. Read-only projections available; mutable controls blocked until cloud upgrade.".into()),
    }
}

// ---------------------------------------------------------------------------
// Hosted cloud contract fixtures — team-visible workspace discovery (#612)
// ---------------------------------------------------------------------------

/// Team-visible workspace discovery: healthy project with workspace binding.
pub fn hosted_bridge_workspace_discovery_healthy() -> TeamVisibleWorkspaceDiscovery {
    TeamVisibleWorkspaceDiscovery {
        project_name: "my-service".into(),
        event_store_healthy: true,
        has_workspace_binding: true,
        setup_state: "normal".into(),
        code_grounding_ready: true,
    }
}

/// Team-visible workspace discovery: project without workspace binding.
pub fn hosted_bridge_workspace_discovery_unbound() -> TeamVisibleWorkspaceDiscovery {
    TeamVisibleWorkspaceDiscovery {
        project_name: "data-pipeline".into(),
        event_store_healthy: true,
        has_workspace_binding: false,
        setup_state: "code_grounding_not_ready".into(),
        code_grounding_ready: false,
    }
}

/// Team-visible workspace discovery: project needing repair.
pub fn hosted_bridge_workspace_discovery_repair() -> TeamVisibleWorkspaceDiscovery {
    TeamVisibleWorkspaceDiscovery {
        project_name: "legacy-api".into(),
        event_store_healthy: false,
        has_workspace_binding: false,
        setup_state: "repair_required".into(),
        code_grounding_ready: false,
    }
}

// ---------------------------------------------------------------------------
// Hosted cloud contract fixtures — bridge command allowlist (#612)
// ---------------------------------------------------------------------------

/// Allowed bridge command family: list projects (dashboard-intent).
pub fn hosted_bridge_command_allowed_list_projects() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "list_projects".into(),
        allowed: true,
        reason: "Dashboard-intent read operation.".into(),
    }
}

/// Allowed bridge command family: get decision projection (read-only).
pub fn hosted_bridge_command_allowed_decision_projection() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "get_decision_projection".into(),
        allowed: true,
        reason: "Read-only projection of decision state.".into(),
    }
}

/// Allowed bridge command family: get ledger summary (read-only).
pub fn hosted_bridge_command_allowed_ledger_summary() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "get_ledger_summary".into(),
        allowed: true,
        reason: "Read-only projection of ledger state.".into(),
    }
}

/// Forbidden bridge command family: signoff (authority-bearing mutation).
pub fn hosted_bridge_command_forbidden_signoff() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "signoff".into(),
        allowed: false,
        reason: "Signoff is an authority-bearing mutation excluded from bridge.".into(),
    }
}

/// Forbidden bridge command family: decision_write (ledger mutation).
pub fn hosted_bridge_command_forbidden_decision_write() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "decision_write".into(),
        allowed: false,
        reason: "Decision Ledger writes are forbidden through bridge.".into(),
    }
}

/// Forbidden bridge command family: source_bind (source-binding mutation).
pub fn hosted_bridge_command_forbidden_source_bind() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "source_bind".into(),
        allowed: false,
        reason: "Source-binding mutations are forbidden through bridge.".into(),
    }
}

/// Forbidden bridge command family: compliance_resolve (compliance outcome).
pub fn hosted_bridge_command_forbidden_compliance_resolve() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "compliance_resolve".into(),
        allowed: false,
        reason: "Compliance resolution outcomes are forbidden through bridge.".into(),
    }
}

/// Forbidden bridge command family: review_approve (authority mutation).
pub fn hosted_bridge_command_forbidden_review_approve() -> BridgeCommandFamily {
    BridgeCommandFamily {
        name: "review_approve".into(),
        allowed: false,
        reason: "Review approval is an authority-bearing mutation excluded from bridge.".into(),
    }
}

/// Collect all allowed bridge command families.
pub fn all_bridge_allowed_commands() -> Vec<BridgeCommandFamily> {
    vec![
        hosted_bridge_command_allowed_list_projects(),
        hosted_bridge_command_allowed_decision_projection(),
        hosted_bridge_command_allowed_ledger_summary(),
    ]
}

/// Collect all forbidden bridge command families.
pub fn all_bridge_forbidden_commands() -> Vec<BridgeCommandFamily> {
    vec![
        hosted_bridge_command_forbidden_signoff(),
        hosted_bridge_command_forbidden_decision_write(),
        hosted_bridge_command_forbidden_source_bind(),
        hosted_bridge_command_forbidden_compliance_resolve(),
        hosted_bridge_command_forbidden_review_approve(),
    ]
}

/// Collect all compatibility envelope fixtures.
pub fn all_compatibility_envelopes() -> Vec<(&'static str, ProtocolCompatibilityEnvelope)> {
    vec![
        ("current_version", hosted_bridge_compat_current_version()),
        ("previous_minor", hosted_bridge_compat_previous_minor()),
        ("too_old_daemon", hosted_bridge_compat_too_old_daemon()),
        ("newer_daemon", hosted_bridge_compat_newer_daemon()),
    ]
}

/// Collect all team-visible workspace discovery fixtures.
pub fn all_workspace_discovery_fixtures() -> Vec<(&'static str, TeamVisibleWorkspaceDiscovery)> {
    vec![
        ("healthy", hosted_bridge_workspace_discovery_healthy()),
        ("unbound", hosted_bridge_workspace_discovery_unbound()),
        ("repair", hosted_bridge_workspace_discovery_repair()),
    ]
}

/// Negative fixture: payload smuggling `external_ingest_envelope`.
pub fn negative_bridge_smuggle_external_ingest() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "submit_evidence",
        "external_ingest_envelope": {
            "source_uri": "https://example.com/source",
            "source_type": "github_pr",
            "payload": {}
        }
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'external_ingest_envelope' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling review approval.
pub fn negative_bridge_smuggle_review_approval() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "approve_review",
        "review_command": {
            "kind": "approve",
            "candidate_id": "cand-001"
        }
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'review_command' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling signoff approval.
pub fn negative_bridge_smuggle_signoff() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "signoff",
        "signoff_approval": {
            "decision_id": "dec-001",
            "approver": "admin"
        }
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'signoff_approval' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling source authority / source_binding.
pub fn negative_bridge_smuggle_source_authority() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "bind_source",
        "source_binding": {
            "source_uri": "https://github.com/org/repo",
            "canonical": true
        }
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'source_binding' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling workspace path.
pub fn negative_bridge_smuggle_workspace_path() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "bind_workspace",
        "workspace_path": "/home/user/project"
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'workspace_path' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling provider token.
pub fn negative_bridge_smuggle_provider_token() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "configure_provider",
        "provider_token": "REDACTED_FAKE_PROVIDER_TOKEN_FOR_TEST"
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'provider_token' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling authority_context (local actor identity).
pub fn negative_bridge_smuggle_authority_context() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "escalate",
        "authority_context": {
            "actor_id": "admin-001",
            "auth_method": "local_daemon",
            "session_id": "sess-admin"
        }
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'authority_context' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Negative fixture: payload smuggling event_store_write.
pub fn negative_bridge_smuggle_event_store_write() -> AuthoritySmugglingPayload {
    let raw_payload = serde_json::json!({
        "action": "write_event",
        "event_store_write": {
            "event_type": "decision_accepted",
            "payload": {}
        }
    });
    let forbidden = detect_smuggled_fields(&raw_payload);
    AuthoritySmugglingPayload {
        raw_payload,
        forbidden_fields: forbidden,
        rejection: HostedBridgeErrorResponse {
            error: HostedBridgeErrorKind::AuthoritySmugglingRejected,
            message: "Forbidden field 'event_store_write' in bridge payload.".into(),
            retry_after_repair: false,
        },
    }
}

/// Collect all hosted bridge positive fixtures for batch testing.
pub fn all_hosted_bridge_fixtures() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "capability_discovery",
            serde_json::to_value(hosted_bridge_capability_discovery()).unwrap(),
        ),
        (
            "capability_unsupported",
            serde_json::to_value(hosted_bridge_capability_unsupported()).unwrap(),
        ),
        (
            "pairing_request",
            serde_json::to_value(hosted_bridge_pairing_request()).unwrap(),
        ),
        (
            "paired_response",
            serde_json::to_value(hosted_bridge_paired_response()).unwrap(),
        ),
        // Pending pairing / local-confirmation flow fixtures (#790)
        (
            "pending_response",
            serde_json::to_value(hosted_bridge_pending_response()).unwrap(),
        ),
        (
            "pending_local_view",
            serde_json::to_value(hosted_bridge_pending_local_view()).unwrap(),
        ),
        (
            "status_approved",
            serde_json::to_value(hosted_bridge_status_approved()).unwrap(),
        ),
        (
            "status_denied",
            serde_json::to_value(hosted_bridge_status_denied()).unwrap(),
        ),
        (
            "status_expired",
            serde_json::to_value(hosted_bridge_status_expired()).unwrap(),
        ),
        (
            "pairing_update_required",
            serde_json::to_value(hosted_bridge_pairing_update_required()).unwrap(),
        ),
        (
            "unpaired_response",
            serde_json::to_value(hosted_bridge_unpaired_response()).unwrap(),
        ),
        (
            "revoked_response",
            serde_json::to_value(hosted_bridge_revoked_response()).unwrap(),
        ),
        (
            "operator_home_session",
            serde_json::to_value(hosted_bridge_operator_home_session()).unwrap(),
        ),
        // Selected-project scoped readiness fixtures (#719)
        (
            "selected_project_ref",
            serde_json::to_value(hosted_bridge_selected_project_ref()).unwrap(),
        ),
        (
            "selected_project_ready",
            serde_json::to_value(hosted_bridge_selected_project_ready()).unwrap(),
        ),
        (
            "selected_project_needs_binding",
            serde_json::to_value(hosted_bridge_selected_project_needs_binding()).unwrap(),
        ),
        (
            "wrong_daemon",
            serde_json::to_value(hosted_bridge_wrong_daemon()).unwrap(),
        ),
        (
            "wrong_project",
            serde_json::to_value(hosted_bridge_wrong_project()).unwrap(),
        ),
        (
            "wrong_account",
            serde_json::to_value(hosted_bridge_wrong_account()).unwrap(),
        ),
        (
            "unsupported_command",
            serde_json::to_value(hosted_bridge_unsupported_command()).unwrap(),
        ),
        // Compatibility envelope fixtures (#612)
        (
            "compat_current_version",
            serde_json::to_value(hosted_bridge_compat_current_version()).unwrap(),
        ),
        (
            "compat_previous_minor",
            serde_json::to_value(hosted_bridge_compat_previous_minor()).unwrap(),
        ),
        (
            "compat_too_old_daemon",
            serde_json::to_value(hosted_bridge_compat_too_old_daemon()).unwrap(),
        ),
        (
            "compat_newer_daemon",
            serde_json::to_value(hosted_bridge_compat_newer_daemon()).unwrap(),
        ),
        // Team-visible workspace discovery fixtures (#612)
        (
            "workspace_discovery_healthy",
            serde_json::to_value(hosted_bridge_workspace_discovery_healthy()).unwrap(),
        ),
        (
            "workspace_discovery_unbound",
            serde_json::to_value(hosted_bridge_workspace_discovery_unbound()).unwrap(),
        ),
        (
            "workspace_discovery_repair",
            serde_json::to_value(hosted_bridge_workspace_discovery_repair()).unwrap(),
        ),
        // Bridge command allowlist fixtures (#612)
        (
            "command_allowed_list_projects",
            serde_json::to_value(hosted_bridge_command_allowed_list_projects()).unwrap(),
        ),
        (
            "command_allowed_decision_projection",
            serde_json::to_value(hosted_bridge_command_allowed_decision_projection()).unwrap(),
        ),
        (
            "command_allowed_ledger_summary",
            serde_json::to_value(hosted_bridge_command_allowed_ledger_summary()).unwrap(),
        ),
        (
            "command_forbidden_signoff",
            serde_json::to_value(hosted_bridge_command_forbidden_signoff()).unwrap(),
        ),
        (
            "command_forbidden_decision_write",
            serde_json::to_value(hosted_bridge_command_forbidden_decision_write()).unwrap(),
        ),
        (
            "command_forbidden_source_bind",
            serde_json::to_value(hosted_bridge_command_forbidden_source_bind()).unwrap(),
        ),
        (
            "command_forbidden_compliance_resolve",
            serde_json::to_value(hosted_bridge_command_forbidden_compliance_resolve()).unwrap(),
        ),
        (
            "command_forbidden_review_approve",
            serde_json::to_value(hosted_bridge_command_forbidden_review_approve()).unwrap(),
        ),
    ]
}

/// Collect all hosted bridge negative (authority-smuggling) fixtures.
pub fn all_hosted_bridge_negative_fixtures() -> Vec<(&'static str, AuthoritySmugglingPayload)> {
    vec![
        (
            "smuggle_external_ingest",
            negative_bridge_smuggle_external_ingest(),
        ),
        (
            "smuggle_review_approval",
            negative_bridge_smuggle_review_approval(),
        ),
        ("smuggle_signoff", negative_bridge_smuggle_signoff()),
        (
            "smuggle_source_authority",
            negative_bridge_smuggle_source_authority(),
        ),
        (
            "smuggle_workspace_path",
            negative_bridge_smuggle_workspace_path(),
        ),
        (
            "smuggle_provider_token",
            negative_bridge_smuggle_provider_token(),
        ),
        (
            "smuggle_authority_context",
            negative_bridge_smuggle_authority_context(),
        ),
        (
            "smuggle_event_store_write",
            negative_bridge_smuggle_event_store_write(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Commit-driven preflight freshness fixtures (issue #625)
// ---------------------------------------------------------------------------
//
// Daemon-authored freshness read-models for the accepted hybrid freshness
// policy. Each is derived from the touched scope + bounded refresh outcome via
// `PreflightFreshness::derive`, so the fixtures are reproducible contracts.
// Surfaces (MCP/dashboard) render these; they never recompute freshness.

use crate::evidence::GraphReadiness;
use crate::freshness::{FreshnessInputs, PreflightFreshness, RefreshOutcome, TouchedScope};
use crate::tool_request::{GraphSliceInfo, LocatorSnapshotInfo, SliceCompleteness};

const FRESHNESS_SNAPSHOT_ID: &str = "snap-2026-06-12-aaaa1111";
const FRESHNESS_HEAD_SHA: &str = "9f1c2d3e4b5a60718293a4b5c6d7e8f901234567";
const FRESHNESS_REPO_ID: &str = "repo-bicameral-bot";

fn freshness_snapshot() -> LocatorSnapshotInfo {
    LocatorSnapshotInfo {
        graph_snapshot_id: Some(FRESHNESS_SNAPSHOT_ID.to_string()),
        repo_id: Some(FRESHNESS_REPO_ID.to_string()),
        commit_sha: Some(FRESHNESS_HEAD_SHA.to_string()),
        index_version: Some("idx-7".to_string()),
        ref_name: Some("refs/heads/dev".to_string()),
    }
}

fn freshness_scope(
    slice_id: &str,
    scope_kind: &str,
    completeness: SliceCompleteness,
) -> GraphSliceInfo {
    GraphSliceInfo {
        graph_slice_id: Some(slice_id.to_string()),
        scope_kind: Some(scope_kind.to_string()),
        scope_fingerprint: Some(format!("fp-{slice_id}")),
        completeness: Some(completeness),
    }
}

/// Stale graph with touched code in scope.
///
/// A new commit touched a file the cited evidence covers and the *graph itself*
/// is stale: freshness is downgraded to `touched_stale`, currentness is
/// `graph_unavailable`, with a `graph_stale` caveat and a request-touched-
/// refresh next action.
pub fn freshness_stale_graph() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::touched_files(&["crates/bicameral-api/src/recall.rs"]),
        refresh_outcome: RefreshOutcome::Stale,
        graph_readiness: GraphReadiness::Stale,
        graph_snapshot: Some(freshness_snapshot()),
        scope: Some(freshness_scope(
            "slice-stale",
            "file_scoped",
            SliceCompleteness::FileScoped,
        )),
        result_was_empty: false,
    })
}

/// Queued bounded refresh.
///
/// A bounded touched-scope refresh was accepted but is still indexing:
/// freshness is `unknown_stale` with a `refresh_queued` caveat and
/// retry-when-ready / proceed-snapshot-scoped next actions. Never blocks.
pub fn freshness_queued_refresh() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::touched_files(&["crates/bicameral-runtime/src/dispatch.rs"]),
        refresh_outcome: RefreshOutcome::Queued,
        graph_readiness: GraphReadiness::Indexing,
        graph_snapshot: Some(freshness_snapshot()),
        scope: Some(freshness_scope(
            "slice-queued",
            "file_scoped",
            SliceCompleteness::FileScoped,
        )),
        result_was_empty: false,
    })
}

/// Touched file changed on a ready graph.
///
/// A newer commit changed a touched file while the graph is otherwise ready:
/// freshness is `touched_stale` and currentness is `content_changed` (the file
/// content differs from the bound snapshot), with a `graph_stale` caveat and a
/// request-touched-refresh next action.
pub fn freshness_touched_file_changed() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::touched_files(&["crates/bicameral-runtime/src/preflight.rs"]),
        refresh_outcome: RefreshOutcome::Stale,
        graph_readiness: GraphReadiness::Ready,
        graph_snapshot: Some(freshness_snapshot()),
        scope: Some(freshness_scope(
            "slice-touched-file",
            "file_scoped",
            SliceCompleteness::FileScoped,
        )),
        result_was_empty: false,
    })
}

/// Touched symbol changed on a ready graph.
///
/// A newer commit changed a touched symbol while the graph is otherwise ready:
/// same `touched_stale` / `content_changed` downgrade as the file case but
/// scoped to a symbol, proving symbol-level touched detection drives the
/// policy too.
pub fn freshness_touched_symbol_changed() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::touched_symbols(&["recall::RecallPacket::new_alpha"]),
        refresh_outcome: RefreshOutcome::Stale,
        graph_readiness: GraphReadiness::Ready,
        graph_snapshot: Some(freshness_snapshot()),
        scope: Some(freshness_scope(
            "slice-touched-symbol",
            "symbol_scoped",
            SliceCompleteness::SymbolScoped,
        )),
        result_was_empty: false,
    })
}

/// Untouched evidence on a stale graph — snapshot-scoped.
///
/// The cited evidence was confirmed *not* touched by newer commits, so even
/// though the graph is stale the evidence remains usable against its older
/// snapshot identity: `snapshot_scoped` with a `snapshot_scoped_evidence`
/// caveat and a proceed-snapshot-scoped next action.
pub fn freshness_untouched_snapshot_scoped() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::confirmed_untouched(),
        refresh_outcome: RefreshOutcome::Stale,
        graph_readiness: GraphReadiness::Stale,
        graph_snapshot: Some(freshness_snapshot()),
        scope: Some(freshness_scope(
            "slice-untouched",
            "file_scoped",
            SliceCompleteness::FileScoped,
        )),
        result_was_empty: false,
    })
}

/// Unknown touched-scope becoming `unknown_stale`.
///
/// No diff and no explicit targets were supplied, so the touched scope could
/// not be determined while the graph is stale: freshness is `unknown_stale`
/// with a `touched_scope_unknown` caveat and a provide-touched-scope next
/// action. The daemon refuses to claim freshness it cannot confirm.
pub fn freshness_unknown_touched_scope() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::unavailable(),
        refresh_outcome: RefreshOutcome::Stale,
        graph_readiness: GraphReadiness::Stale,
        graph_snapshot: Some(freshness_snapshot()),
        scope: None,
        result_was_empty: false,
    })
}

/// No-result lookup with a stale graph.
///
/// Preflight found no relevant decisions while the graph is stale and the cited
/// evidence was confirmed untouched: freshness is `snapshot_scoped` and an
/// extra `empty_result_stale_graph` caveat is added so the empty result is not
/// mistaken for a confirmed all-clear. Never claims no-conflict or
/// merge-safety.
pub fn freshness_no_result_stale_graph() -> PreflightFreshness {
    PreflightFreshness::derive(FreshnessInputs {
        touched_scope: TouchedScope::confirmed_untouched(),
        refresh_outcome: RefreshOutcome::Stale,
        graph_readiness: GraphReadiness::Stale,
        graph_snapshot: Some(freshness_snapshot()),
        scope: None,
        result_was_empty: true,
    })
}

/// All commit-driven freshness fixtures, keyed by their protocol fixture name.
pub fn all_freshness_fixtures() -> Vec<(&'static str, PreflightFreshness)> {
    vec![
        ("freshness-stale-graph", freshness_stale_graph()),
        ("freshness-queued-refresh", freshness_queued_refresh()),
        (
            "freshness-touched-file-changed",
            freshness_touched_file_changed(),
        ),
        (
            "freshness-touched-symbol-changed",
            freshness_touched_symbol_changed(),
        ),
        (
            "freshness-untouched-snapshot-scoped",
            freshness_untouched_snapshot_scoped(),
        ),
        (
            "freshness-unknown-touched-scope",
            freshness_unknown_touched_scope(),
        ),
        (
            "freshness-no-result-stale-graph",
            freshness_no_result_stale_graph(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Semantic recall contribution fixtures (issue #640)
// ---------------------------------------------------------------------------

use crate::evidence::CodeSourceCurrentness;
use crate::recall::{
    RecallProviderProvenance, RecallProviderStatus, RecallScopeKind, RecallSearchedScope,
    RecallUnknownProviderScope, RecallUnknownProviderScopeReason,
};
use crate::semantic_recall::{SemanticRecallInputs, SemanticRecallView};

const SEMANTIC_PROVIDER_ID: &str = "fake-semantic-provider";
const SEMANTIC_PROVIDER_VERSION: &str = "0.1.0";
const SEMANTIC_INDEX_VERSION: &str = "sem-idx-12";

fn semantic_provenance() -> RecallProviderProvenance {
    RecallProviderProvenance {
        provider_id: SEMANTIC_PROVIDER_ID.to_string(),
        provider_version: SEMANTIC_PROVIDER_VERSION.to_string(),
        index_version: Some(SEMANTIC_INDEX_VERSION.to_string()),
        overlay_id: None,
        model_id: Some("fake-embed".to_string()),
        model_version: Some("v0".to_string()),
    }
}

fn semantic_searched_scope() -> RecallSearchedScope {
    RecallSearchedScope {
        scope_kind: RecallScopeKind::Query,
        scope_fingerprint: Some("fp-semantic-query".to_string()),
        files: Vec::new(),
        symbols: Vec::new(),
        corpus_id: Some("corpus-bicameral-bot".to_string()),
        corpus_version: Some(SEMANTIC_INDEX_VERSION.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn semantic_candidate(
    file: &str,
    symbol: Option<&str>,
    range: Option<&str>,
    evidence_state: LocatorEvidenceState,
    confidence: LocatorConfidence,
    reason_code: LocatorReasonCode,
    reason_detail: &str,
    caveats: &[&str],
) -> LocatorCandidate {
    LocatorCandidate {
        target: file.to_string(),
        target_type: LocatorTargetType::FilePath,
        file: file.to_string(),
        range: range.map(str::to_string),
        symbol: symbol.map(str::to_string),
        evidence_state,
        confidence,
        reasons: vec![LocatorReason {
            code: reason_code,
            detail: reason_detail.to_string(),
        }],
        graph_snapshot_id: None,
        validated_sha: None,
        content_hash: None,
        caveats: caveats.iter().map(|c| c.to_string()).collect(),
    }
}

/// A graph-verified semantic target — fused from the provider but proved by
/// deterministic graph verification.
fn semantic_verified_candidate() -> LocatorCandidate {
    semantic_candidate(
        "crates/bicameral-api/src/recall.rs",
        Some("RecallPacket::with_semantic_recall"),
        Some("60:1-90:1"),
        LocatorEvidenceState::Verified,
        LocatorConfidence::High,
        LocatorReasonCode::SemanticTarget,
        "Semantic candidate confirmed by deterministic graph proof at the bound snapshot.",
        &[],
    )
}

/// An approximate semantic hint — advisory, not graph-verified.
fn semantic_hint_candidate(file: &str) -> LocatorCandidate {
    semantic_candidate(
        file,
        None,
        None,
        LocatorEvidenceState::ApproximateCandidate,
        LocatorConfidence::Low,
        LocatorReasonCode::SemanticHint,
        "Approximate semantic hint; requires deterministic graph proof before binding.",
        &["approximate semantic hint: not graph-verified"],
    )
}

fn semantic_derive(
    status: RecallProviderStatus,
    currentness: CodeSourceCurrentness,
    unknown_scope: Vec<RecallUnknownProviderScope>,
    candidates: &[LocatorCandidate],
) -> SemanticRecallView {
    SemanticRecallView::derive(SemanticRecallInputs {
        provider_status: status,
        currentness,
        searched_scope: semantic_searched_scope(),
        unknown_scope,
        provenance: semantic_provenance(),
        locator_candidates: candidates,
    })
    .expect("configured semantic provider yields a view")
}

/// Searched scope, ready provider: one graph-verified target and one
/// approximate hint kept in **separate** collections so no surface can collapse
/// approximate semantic hints into verified graph evidence.
pub fn semantic_recall_searched_scope() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Ready,
        CodeSourceCurrentness::Current,
        Vec::new(),
        &[
            semantic_verified_candidate(),
            semantic_hint_candidate("crates/bicameral-runtime/src/search.rs"),
        ],
    )
}

/// Unknown scope: a ready provider returned an approximate hint but could not
/// cover part of the requested scope (not indexed). The uncovered scope is
/// disclosed; coverage is partial.
pub fn semantic_recall_unknown_scope() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Ready,
        CodeSourceCurrentness::Current,
        vec![RecallUnknownProviderScope {
            scope: "vendor/".to_string(),
            reason: RecallUnknownProviderScopeReason::NotIndexed,
            detail: Some("Vendored sources are outside the semantic index.".to_string()),
        }],
        &[semantic_hint_candidate(
            "crates/bicameral-gateway/src/routes.rs",
        )],
    )
}

/// Stale index: the semantic index is stale, so even a returned hint is treated
/// as approximate until refreshed.
pub fn semantic_recall_stale_index() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Stale,
        CodeSourceCurrentness::GraphUnavailable,
        Vec::new(),
        &[semantic_hint_candidate(
            "crates/bicameral-api/src/dashboard.rs",
        )],
    )
}

/// Unsupported provider: the provider does not support this language or
/// repository, so no semantic contribution is available for the searched scope.
pub fn semantic_recall_unsupported_provider() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Unsupported,
        CodeSourceCurrentness::GraphUnavailable,
        vec![RecallUnknownProviderScope {
            scope: "repository".to_string(),
            reason: RecallUnknownProviderScopeReason::UnsupportedRepository,
            detail: Some("Semantic provider does not support this repository.".to_string()),
        }],
        &[],
    )
}

/// No result in searched scope: a ready provider searched but found nothing.
/// The empty result is scoped to what was searched — never an all-clear.
pub fn semantic_recall_no_result_in_searched_scope() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Ready,
        CodeSourceCurrentness::Current,
        Vec::new(),
        &[],
    )
}

/// Provider still indexing: coverage is incomplete for the searched scope.
pub fn semantic_recall_indexing() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Indexing,
        CodeSourceCurrentness::GraphUnavailable,
        Vec::new(),
        &[semantic_hint_candidate(
            "crates/bicameral-runtime/src/preflight.rs",
        )],
    )
}

/// Provider failed: no semantic contribution was retrieved for the searched
/// scope.
pub fn semantic_recall_failed() -> SemanticRecallView {
    semantic_derive(
        RecallProviderStatus::Failed,
        CodeSourceCurrentness::GraphUnavailable,
        Vec::new(),
        &[],
    )
}

/// All semantic recall contribution fixtures, keyed by their protocol fixture
/// name. Covers every acceptance state: searched scope, unknown scope, stale
/// index, unsupported provider, and no-result-in-searched-scope (plus indexing
/// and failed provider states).
pub fn all_semantic_recall_fixtures() -> Vec<(&'static str, SemanticRecallView)> {
    vec![
        (
            "semantic-recall-searched-scope",
            semantic_recall_searched_scope(),
        ),
        (
            "semantic-recall-unknown-scope",
            semantic_recall_unknown_scope(),
        ),
        ("semantic-recall-stale-index", semantic_recall_stale_index()),
        (
            "semantic-recall-unsupported-provider",
            semantic_recall_unsupported_provider(),
        ),
        (
            "semantic-recall-no-result-in-searched-scope",
            semantic_recall_no_result_in_searched_scope(),
        ),
        ("semantic-recall-indexing", semantic_recall_indexing()),
        ("semantic-recall-failed", semantic_recall_failed()),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_accepted_fixtures_roundtrip() {
        for (name, req) in all_accepted_fixtures() {
            let json = serde_json::to_string_pretty(&req)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: ToolRequest = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(
                parsed.request_id, req.request_id,
                "{name}: request_id mismatch"
            );

            let re_json = serde_json::to_string_pretty(&parsed)
                .unwrap_or_else(|e| panic!("{name}: re-serialize failed: {e}"));
            assert_eq!(json, re_json, "{name}: round-trip JSON mismatch");
        }
    }

    #[test]
    fn all_negative_fixtures_roundtrip() {
        for (name, _expected_class, req) in all_negative_fixtures() {
            let json = serde_json::to_string_pretty(&req)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: ToolRequest = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(
                parsed.request_id, req.request_id,
                "{name}: request_id mismatch"
            );
        }
    }

    #[test]
    fn accepted_fixture_authority_is_populated() {
        for (name, req) in all_accepted_fixtures() {
            assert!(
                !req.authority.actor_id.is_empty(),
                "{name}: actor_id must not be empty"
            );
        }
    }

    #[test]
    fn one_request_per_command_is_total_and_unique() {
        let requests = one_request_per_command();
        let names: std::collections::HashSet<&'static str> = requests
            .iter()
            .map(|req| req.command.command_name())
            .collect();
        assert_eq!(requests.len(), 35);
        assert_eq!(names.len(), 35);
    }

    #[test]
    fn negative_missing_actor_has_empty_actor_id() {
        let req = negative_missing_actor_id();
        assert!(req.authority.actor_id.is_empty());
    }

    #[test]
    fn negative_malformed_ingest_has_empty_title() {
        let req = negative_malformed_ingest_empty_title();
        if let ToolCommand::IngestSubmitLocal(params) = &req.command {
            assert!(params.title.is_empty());
        } else {
            panic!("expected IngestSubmitLocal");
        }
    }

    #[test]
    fn negative_validation_token_is_present() {
        let req = negative_validation_token_acceptance();
        if let ToolCommand::BindingCreate(params) = &req.command {
            assert!(params.validation_token.is_some());
        } else {
            panic!("expected BindingCreate");
        }
    }

    #[test]
    fn search_query_bindings_scope_is_supported() {
        let req = search_query_bindings();
        if let ToolCommand::SearchQuery(params) = &req.command {
            assert_eq!(params.scope, Some(SearchScope::Bindings));
            assert!(!params.query.is_empty());
        } else {
            panic!("expected SearchQuery");
        }
    }

    #[test]
    fn response_fixtures_roundtrip() {
        let id = Uuid::new_v4();
        let responses: Vec<(&str, ToolResponse)> = vec![
            ("ingest_accepted", response_ingest_accepted(id)),
            ("missing_authority", response_missing_authority(id)),
            ("malformed_payload", response_malformed_payload(id)),
            (
                "validation_token_unsupported",
                response_validation_token_unsupported(id),
            ),
            ("unsupported_scope", response_unsupported_scope(id)),
            ("daemon_error", response_daemon_error(id)),
        ];
        for (name, resp) in &responses {
            let json = serde_json::to_string_pretty(resp)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: ToolResponse = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(
                parsed.request_id, resp.request_id,
                "{name}: request_id mismatch"
            );
            assert_eq!(parsed.status, resp.status, "{name}: status mismatch");
        }
    }

    #[test]
    fn ingest_fixture_contains_expected_command_tag() {
        let req = ingest_submit_local();
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("ingest.submit_local"));
    }

    #[test]
    fn binding_create_fixture_has_multiple_bindings() {
        let req = binding_create();
        if let ToolCommand::BindingCreate(params) = &req.command {
            assert!(params.bindings.len() >= 2);
        } else {
            panic!("expected BindingCreate");
        }
    }

    #[test]
    fn all_accepted_command_tags_are_distinct() {
        let fixtures = all_accepted_fixtures();
        let mut seen = std::collections::HashSet::new();
        for (name, req) in &fixtures {
            let json = serde_json::to_string(&req.command).unwrap();
            let v: serde_json::Value = serde_json::from_str(&json).unwrap();
            let tag = v["name"].as_str().unwrap().to_string();
            assert!(seen.insert(tag.clone()), "{name}: duplicate tag {tag}");
        }
    }

    #[test]
    fn external_ingest_envelope_deserialization_rejects_forbidden_fields() {
        let json = r#"{
            "source_system": "github",
            "source_uri": "https://github.com/org/repo/issues/1",
            "content": "Issue content",
            "evidence": [],
            "candidate_hints": [],
            "authority": { "actor_id": "attacker", "auth_method": "local_daemon" },
            "tool_command": { "name": "review.approve_signoff", "params": { "target_id": "DEC-1" } },
            "review_command": "approve_signoff",
            "canonical_decision": { "id": "DEC-FAKE" }
        }"#;
        let envelope: ExternalIngestEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(envelope.source_system, "github");
        assert!(envelope.evidence.is_empty());
        assert!(envelope.candidate_hints.is_empty());
    }

    #[test]
    fn mcp_direct_authority_attempt_is_structurally_blocked() {
        // MCP cannot produce `GraphEvidenceState::verified` through ToolRequest.
        // The `binding.create` fixture round-trips without any verified field.
        let req = binding_create();
        let json = serde_json::to_string_pretty(&req).unwrap();
        assert!(!json.contains("\"verified\""));
        assert!(!json.contains("GraphEvidenceState"));
    }

    // -- graph / CodeLocator fixture tests --------------------------------

    #[test]
    fn code_locate_fixture_roundtrip() {
        let req = code_locate();
        let json = serde_json::to_string_pretty(&req).unwrap();
        let parsed: ToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
        assert!(json.contains("code.locate"));
    }

    #[test]
    fn graph_status_fixture_roundtrip() {
        let req = graph_status();
        let json = serde_json::to_string_pretty(&req).unwrap();
        let parsed: ToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
        assert!(json.contains("graph.status"));
    }

    #[test]
    fn graph_refresh_snapshot_fixture_roundtrip() {
        let req = graph_refresh_snapshot();
        let json = serde_json::to_string_pretty(&req).unwrap();
        let parsed: ToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
        assert!(json.contains("graph.refresh_snapshot"));
    }

    #[test]
    fn locator_result_fixtures_roundtrip() {
        let fixtures: Vec<(&str, LocatorResult)> = vec![
            ("ready_verified", locator_result_ready_verified()),
            ("stale", locator_result_stale()),
            ("not_configured", locator_result_not_configured()),
            ("ambiguous", locator_result_ambiguous()),
            ("unsupported", locator_result_unsupported()),
            (
                "approximate_candidate",
                locator_result_approximate_candidate(),
            ),
        ];
        for (name, result) in &fixtures {
            let json = serde_json::to_string_pretty(result)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: LocatorResult = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(parsed.status, result.status, "{name}: status mismatch");
            assert_eq!(
                parsed.graph_readiness, result.graph_readiness,
                "{name}: graph_readiness mismatch"
            );
        }
    }

    #[test]
    fn locator_result_non_verified_cannot_bind() {
        let stale = locator_result_stale();
        for c in &stale.candidates {
            assert!(
                !c.evidence_state.allows_binding(),
                "stale candidate must not allow binding"
            );
        }

        let ambiguous = locator_result_ambiguous();
        for c in &ambiguous.candidates {
            assert!(
                !c.evidence_state.allows_binding(),
                "ambiguous candidate must not allow binding"
            );
        }

        let unsupported = locator_result_unsupported();
        for c in &unsupported.candidates {
            assert!(
                !c.evidence_state.allows_binding(),
                "unsupported candidate must not allow binding"
            );
        }

        let approx = locator_result_approximate_candidate();
        for c in &approx.candidates {
            assert!(
                !c.evidence_state.allows_binding(),
                "approximate candidate must not allow binding"
            );
        }

        let not_configured = locator_result_not_configured();
        assert!(
            not_configured.candidates.is_empty(),
            "not_configured result must have no candidates"
        );
    }

    #[test]
    fn locator_result_verified_allows_binding() {
        let ready = locator_result_ready_verified();
        assert_eq!(ready.candidates.len(), 1);
        assert!(ready.candidates[0].evidence_state.allows_binding());
    }

    #[test]
    fn graph_refresh_result_fixtures_roundtrip() {
        let fixtures: Vec<(&str, GraphRefreshResult)> = vec![
            ("ready", graph_refresh_result_ready()),
            ("stale", graph_refresh_result_stale()),
            ("not_configured", graph_refresh_result_not_configured()),
        ];
        for (name, result) in &fixtures {
            let json = serde_json::to_string_pretty(result)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: GraphRefreshResult = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(parsed.status, result.status, "{name}: status mismatch");
        }
    }

    #[test]
    fn graph_status_result_fixtures_roundtrip() {
        let fixtures: Vec<(&str, GraphStatusResult)> = vec![
            ("ready", graph_status_result_ready()),
            ("not_configured", graph_status_result_not_configured()),
        ];
        for (name, result) in &fixtures {
            let json = serde_json::to_string_pretty(result)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: GraphStatusResult = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(
                parsed.readiness, result.readiness,
                "{name}: readiness mismatch"
            );
        }
    }

    #[test]
    fn locator_result_snake_case_serialization() {
        let result = locator_result_ready_verified();
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"found\""));
        assert!(json.contains("\"ready\""));
        assert!(json.contains("\"verified\""));
        assert!(json.contains("\"high\""));
        assert!(json.contains("\"exact_path\""));
        assert!(json.contains("\"file_path\""));
    }

    #[test]
    fn graph_refresh_result_snake_case_serialization() {
        let result = graph_refresh_result_stale();
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"stale\""));
        assert!(json.contains("\"diff_scoped\"") || !json.contains("completeness"));
    }

    // -- decision.find_code_impact fixture tests --------------------------

    #[test]
    fn decision_find_code_impact_fixture_roundtrip() {
        let req = decision_find_code_impact();
        let json = serde_json::to_string_pretty(&req).unwrap();
        let parsed: ToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
        assert!(json.contains("decision.find_code_impact"));
    }

    #[test]
    fn decision_find_code_impact_minimal_fixture_roundtrip() {
        let req = decision_find_code_impact_minimal();
        let json = serde_json::to_string_pretty(&req).unwrap();
        let parsed: ToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
        assert!(json.contains("decision.find_code_impact"));
    }

    #[test]
    fn find_code_impact_result_fixtures_roundtrip() {
        let fixtures: Vec<(&str, FindCodeImpactResult)> = vec![
            ("verified", find_code_impact_result_verified()),
            ("approximate", find_code_impact_result_approximate()),
            (
                "graph_unavailable",
                find_code_impact_result_graph_unavailable(),
            ),
            ("graph_stale", find_code_impact_result_graph_stale()),
            ("not_found", find_code_impact_result_not_found()),
            ("unsupported", find_code_impact_result_unsupported()),
        ];
        for (name, result) in &fixtures {
            let json = serde_json::to_string_pretty(result)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: FindCodeImpactResult = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(parsed.status, result.status, "{name}: status mismatch");
            assert_eq!(parsed.phase, result.phase, "{name}: phase mismatch");
            assert_eq!(
                parsed.graph_readiness, result.graph_readiness,
                "{name}: graph_readiness mismatch"
            );
        }
    }

    #[test]
    fn find_code_impact_result_non_verified_cannot_bind() {
        let approx = find_code_impact_result_approximate();
        for c in &approx.candidates {
            assert!(
                !c.allows_binding(),
                "approximate candidate must not allow binding"
            );
        }

        let stale = find_code_impact_result_graph_stale();
        for c in &stale.candidates {
            assert!(
                !c.allows_binding(),
                "stale candidate must not allow binding"
            );
        }

        let not_found = find_code_impact_result_not_found();
        assert!(not_found.candidates.is_empty());

        let unavailable = find_code_impact_result_graph_unavailable();
        assert!(unavailable.candidates.is_empty());
    }

    #[test]
    fn find_code_impact_result_verified_allows_binding() {
        let verified = find_code_impact_result_verified();
        assert_eq!(verified.candidates.len(), 1);
        assert!(verified.candidates[0].allows_binding());
    }

    #[test]
    fn find_code_impact_result_cannot_encode_compliance() {
        let result = find_code_impact_result_verified();
        let json = serde_json::to_string_pretty(&result).unwrap();
        assert!(!json.contains("compliance"));
        assert!(!json.contains("signoff"));
        assert!(!json.contains("blocking"));
        assert!(!json.contains("no_conflict"));
        assert!(!json.contains("accepted_binding"));
    }

    #[test]
    fn find_code_impact_result_snake_case_serialization() {
        let result = find_code_impact_result_verified();
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"found\""));
        assert!(json.contains("\"candidates_ready\""));
        assert!(json.contains("\"ready\""));
        assert!(json.contains("\"verified\""));
        assert!(json.contains("\"high\""));
        assert!(json.contains("\"exact_symbol\""));
    }

    // -- planner quality fixture tests ------------------------------------

    #[test]
    fn planner_quality_fixtures_roundtrip() {
        for (name, result) in all_planner_quality_fixtures() {
            let json = serde_json::to_string_pretty(&result)
                .unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
            let parsed: FindCodeImpactResult = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: deserialize failed: {e}"));
            assert_eq!(parsed.status, result.status, "{name}: status mismatch");
            assert_eq!(parsed.phase, result.phase, "{name}: phase mismatch");
            assert_eq!(
                parsed.graph_readiness, result.graph_readiness,
                "{name}: graph_readiness mismatch"
            );
        }
    }

    #[test]
    fn planner_well_formed_has_bounded_queries() {
        let result = planner_well_formed_plan();
        let plan = result.search_plan.as_ref().unwrap();
        assert!(plan.queries_executed > 0, "plan must have executed queries");
        assert!(
            plan.queries_executed <= 20,
            "plan queries must be bounded (got {})",
            plan.queries_executed
        );
    }

    #[test]
    fn planner_well_formed_has_mixed_evidence() {
        let result = planner_well_formed_plan();
        assert!(
            result.candidates.len() >= 2,
            "well-formed plan needs multiple candidates"
        );
        let has_verified = result
            .candidates
            .iter()
            .any(|c| c.evidence_state == LocatorEvidenceState::Verified);
        let has_approximate = result
            .candidates
            .iter()
            .any(|c| c.evidence_state == LocatorEvidenceState::ApproximateCandidate);
        assert!(
            has_verified,
            "well-formed plan must include a verified candidate"
        );
        assert!(
            has_approximate,
            "well-formed plan must include an approximate candidate"
        );
    }

    #[test]
    fn planner_ambiguous_all_candidates_approximate() {
        let result = planner_ambiguous_broad_plan();
        assert_eq!(result.status, FindCodeImpactStatus::NeedsHumanSelection);
        for c in &result.candidates {
            assert_eq!(
                c.evidence_state,
                LocatorEvidenceState::ApproximateCandidate,
                "ambiguous plan candidates must all be approximate"
            );
            assert!(
                !c.allows_binding(),
                "ambiguous plan candidates must not allow binding"
            );
        }
    }

    #[test]
    fn planner_ambiguous_has_narrowing_action() {
        let result = planner_ambiguous_broad_plan();
        let has_narrow = result
            .next_actions
            .iter()
            .any(|a| a.action == "narrow_search" || a.action == "choose_candidate");
        assert!(
            has_narrow,
            "ambiguous plan must suggest narrowing or candidate selection"
        );
    }

    #[test]
    fn planner_ambiguous_has_limited_context_caveat() {
        let result = planner_ambiguous_broad_plan();
        let has_caveat = result
            .caveats
            .iter()
            .any(|c| c.kind == CodeImpactCaveatKind::LimitedSourceContext);
        assert!(
            has_caveat,
            "ambiguous plan must flag limited source context"
        );
    }

    #[test]
    fn planner_rejected_has_no_candidates_or_plan() {
        let result = planner_rejected_out_of_scope();
        assert_eq!(result.status, FindCodeImpactStatus::Unsupported);
        assert_eq!(result.phase, BindingDiscoveryPhase::DiscoveryRequested);
        assert!(
            result.search_plan.is_none(),
            "rejected plan must have no search plan"
        );
        assert!(
            result.candidates.is_empty(),
            "rejected plan must have no candidates"
        );
        assert!(
            result.next_actions.is_empty(),
            "rejected plan must have no next actions"
        );
    }

    #[test]
    fn planner_needs_narrowing_has_truncated_caveat() {
        let result = planner_needs_narrowing();
        assert_eq!(result.status, FindCodeImpactStatus::Found);
        let has_truncated = result
            .caveats
            .iter()
            .any(|c| c.kind == CodeImpactCaveatKind::Truncated);
        assert!(
            has_truncated,
            "needs-narrowing plan must have truncated caveat"
        );
    }

    #[test]
    fn planner_needs_narrowing_suggests_narrowing() {
        let result = planner_needs_narrowing();
        let has_narrow = result
            .next_actions
            .iter()
            .any(|a| a.action == "narrow_search");
        assert!(
            has_narrow,
            "needs-narrowing plan must suggest narrow_search"
        );
    }

    #[test]
    fn planner_existing_bindings_has_prior_binding_reason() {
        let result = planner_existing_bindings();
        let has_prior = result.candidates.iter().any(|c| {
            c.reasons
                .iter()
                .any(|r| r.code == LocatorReasonCode::PriorBinding)
        });
        assert!(
            has_prior,
            "existing-bindings plan must include PriorBinding reason code"
        );
    }

    #[test]
    fn planner_weak_evidence_all_approximate_low_confidence() {
        let result = planner_weak_source_evidence();
        for c in &result.candidates {
            assert_eq!(
                c.evidence_state,
                LocatorEvidenceState::ApproximateCandidate,
                "weak-evidence candidates must be approximate"
            );
            assert_eq!(
                c.confidence,
                LocatorConfidence::Low,
                "weak-evidence candidates must have low confidence"
            );
        }
    }

    #[test]
    fn planner_hint_seeded_plan_is_hint_constrained() {
        let result = planner_hint_seeded();
        let plan = result.search_plan.as_ref().unwrap();
        assert!(plan.hint_seeded, "hint-seeded plan must flag hint_seeded");
        assert_eq!(plan.strategy, "hint_seeded");
    }

    #[test]
    fn planner_hint_seeded_request_has_all_hint_kinds() {
        let req = decision_find_code_impact_hint_seeded();
        if let ToolCommand::DecisionFindCodeImpact(params) = &req.command {
            assert_eq!(params.hints.len(), 3);
            let kinds: Vec<_> = params.hints.iter().map(|h| h.kind).collect();
            assert!(kinds.contains(&FindCodeImpactHintKind::File));
            assert!(kinds.contains(&FindCodeImpactHintKind::Symbol));
            assert!(kinds.contains(&FindCodeImpactHintKind::Keyword));
        } else {
            panic!("expected DecisionFindCodeImpact");
        }
    }

    #[test]
    fn planner_request_broad_no_hints_has_empty_hints() {
        let req = decision_find_code_impact_broad_no_hints();
        if let ToolCommand::DecisionFindCodeImpact(params) = &req.command {
            assert!(params.hints.is_empty());
        } else {
            panic!("expected DecisionFindCodeImpact");
        }
    }

    #[test]
    fn planner_output_max_query_bound() {
        let max_queries: usize = 20;
        for (name, result) in all_planner_quality_fixtures() {
            if let Some(plan) = &result.search_plan {
                assert!(
                    plan.queries_executed <= max_queries,
                    "{name}: queries_executed ({}) exceeds max bound ({})",
                    plan.queries_executed,
                    max_queries
                );
            }
        }
    }

    #[test]
    fn planner_output_candidate_limit_bound() {
        let max_candidates: usize = 20;
        for (name, result) in all_planner_quality_fixtures() {
            assert!(
                result.candidates.len() <= max_candidates,
                "{name}: candidates ({}) exceeds max bound ({})",
                result.candidates.len(),
                max_candidates
            );
        }
    }

    #[test]
    fn planner_output_path_scope_constraints() {
        for (name, result) in all_planner_quality_fixtures() {
            for (i, c) in result.candidates.iter().enumerate() {
                assert!(
                    !c.file.is_empty(),
                    "{name}: candidate {i} has empty file path"
                );
                assert!(
                    !c.target.is_empty(),
                    "{name}: candidate {i} has empty target"
                );
                assert!(
                    !c.file.starts_with('/'),
                    "{name}: candidate {i} file path must be relative, got {}",
                    c.file
                );
            }
        }
    }

    #[test]
    fn planner_output_cannot_set_verified_graph_state() {
        for (name, result) in all_planner_quality_fixtures() {
            let json = serde_json::to_string_pretty(&result).unwrap();
            assert!(
                !json.contains("\"graph_evidence_state\""),
                "{name}: planner output must not set graph_evidence_state"
            );
        }
    }

    #[test]
    fn planner_output_cannot_encode_governance() {
        for (name, result) in all_planner_quality_fixtures() {
            let json = serde_json::to_string_pretty(&result).unwrap();
            assert!(
                !json.contains("compliance"),
                "{name}: planner output must not contain compliance"
            );
            assert!(
                !json.contains("signoff"),
                "{name}: planner output must not contain signoff"
            );
            assert!(
                !json.contains("no_conflict"),
                "{name}: planner output must not contain no_conflict"
            );
            assert!(
                !json.contains("accepted_binding"),
                "{name}: planner output must not contain accepted_binding"
            );
        }
    }

    #[test]
    fn planner_output_cannot_encode_blocking() {
        for (name, result) in all_planner_quality_fixtures() {
            let json = serde_json::to_string_pretty(&result).unwrap();
            assert!(
                !json.contains("\"blocking\""),
                "{name}: planner output must not contain blocking claims"
            );
        }
    }

    #[test]
    fn planner_output_non_verified_cannot_bind() {
        for (name, result) in all_planner_quality_fixtures() {
            for (i, c) in result.candidates.iter().enumerate() {
                if c.evidence_state != LocatorEvidenceState::Verified {
                    assert!(
                        !c.allows_binding(),
                        "{name}: non-verified candidate {i} must not allow binding"
                    );
                }
            }
        }
    }

    #[test]
    fn planner_graph_unavailable_visible() {
        let result = find_code_impact_result_graph_unavailable();
        assert_eq!(result.status, FindCodeImpactStatus::GraphUnavailable);
        assert_eq!(result.graph_readiness, LocatorGraphReadiness::NotConfigured);
        assert!(result.candidates.is_empty());
        assert!(result.search_plan.is_none());
    }

    #[test]
    fn planner_graph_stale_visible() {
        let result = find_code_impact_result_graph_stale();
        assert_eq!(result.status, FindCodeImpactStatus::GraphStale);
        assert_eq!(result.graph_readiness, LocatorGraphReadiness::Stale);
        let has_stale_caveat = result
            .caveats
            .iter()
            .any(|c| c.kind == CodeImpactCaveatKind::GraphStale);
        assert!(
            has_stale_caveat,
            "graph-stale result must have GraphStale caveat"
        );
    }

    // -----------------------------------------------------------------------
    // Hosted bridge protocol fixture tests
    // -----------------------------------------------------------------------

    #[test]
    fn hosted_bridge_fixtures_roundtrip() {
        for (name, val) in all_hosted_bridge_fixtures() {
            let json = serde_json::to_string_pretty(&val)
                .unwrap_or_else(|e| panic!("bridge/{name}: serialize failed: {e}"));
            let _: serde_json::Value = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("bridge/{name}: deserialize failed: {e}"));
        }
    }

    #[test]
    fn hosted_bridge_paired_response_is_paired_state() {
        let resp = hosted_bridge_paired_response();
        assert_eq!(resp.state, HostedBridgePairingState::Paired);
    }

    #[test]
    fn hosted_bridge_unpaired_response_is_not_paired() {
        let resp = hosted_bridge_unpaired_response();
        assert_eq!(resp.error, HostedBridgeErrorKind::NotPaired);
        assert!(resp.retry_after_repair);
    }

    #[test]
    fn hosted_bridge_revoked_response_is_revoked_state() {
        let resp = hosted_bridge_revoked_response();
        assert_eq!(resp.state, HostedBridgePairingState::Revoked);
        assert!(resp.re_pair_available);
    }

    #[test]
    fn hosted_bridge_operator_home_workspace_binding_unavailable() {
        let home = hosted_bridge_operator_home_session();
        assert_eq!(home.pairing_state, HostedBridgePairingState::Paired);
        assert!(
            !home.workspace_binding_available,
            "workspace binding must be unavailable at operator-home level"
        );
    }

    #[test]
    fn hosted_bridge_operator_home_projects_are_projection_only() {
        let home = hosted_bridge_operator_home_session();
        assert!(!home.projects.is_empty());
        // Projects listed are projection data, not authority —
        // they do not contain authority_context, tool_command, or workspace path.
        let json = serde_json::to_string(&home).unwrap();
        assert!(!json.contains("authority_context"));
        assert!(!json.contains("tool_command"));
        assert!(!json.contains("\"workspace_path\""));
    }

    #[test]
    fn hosted_bridge_capability_workspace_always_false() {
        let cap = hosted_bridge_capability_discovery();
        assert!(
            !cap.workspace_binding_available,
            "bridge capability must not advertise workspace binding"
        );
    }

    #[test]
    fn hosted_bridge_negative_fixtures_all_detect_forbidden_fields() {
        for (name, payload) in all_hosted_bridge_negative_fixtures() {
            assert!(
                !payload.forbidden_fields.is_empty(),
                "bridge_negative/{name}: must detect at least one forbidden field"
            );
            assert_eq!(
                payload.rejection.error,
                HostedBridgeErrorKind::AuthoritySmugglingRejected,
                "bridge_negative/{name}: rejection must be AuthoritySmugglingRejected"
            );
        }
    }

    #[test]
    fn hosted_bridge_negative_fixtures_cover_required_smuggling_vectors() {
        let payloads = all_hosted_bridge_negative_fixtures();
        let all_forbidden: Vec<String> = payloads
            .iter()
            .flat_map(|(_, p)| p.forbidden_fields.clone())
            .collect();

        // Required coverage per acceptance criteria
        let required = [
            "external_ingest_envelope",
            "review_command",
            "signoff_approval",
            "source_binding",
            "workspace_path",
            "provider_token",
            "authority_context",
            "event_store_write",
        ];
        for field in required {
            assert!(
                all_forbidden.contains(&field.to_string()),
                "negative fixtures must cover forbidden field '{field}'"
            );
        }
    }

    #[test]
    fn hosted_bridge_negative_payload_roundtrip() {
        for (name, payload) in all_hosted_bridge_negative_fixtures() {
            let json = serde_json::to_string_pretty(&payload)
                .unwrap_or_else(|e| panic!("bridge_negative/{name}: serialize failed: {e}"));
            let rt: AuthoritySmugglingPayload = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("bridge_negative/{name}: deserialize failed: {e}"));
            assert_eq!(rt.forbidden_fields, payload.forbidden_fields);
        }
    }

    #[test]
    fn hosted_bridge_detect_smuggled_fields_clean_payload() {
        let clean = serde_json::json!({
            "action": "list_projects",
            "session_token": "bridge-sess-001"
        });
        let found = detect_smuggled_fields(&clean);
        assert!(
            found.is_empty(),
            "clean payload must not trigger smuggling detection"
        );
    }

    // -----------------------------------------------------------------------
    // Hosted cloud contract fixture tests (#612)
    // -----------------------------------------------------------------------

    #[test]
    fn compat_current_version_is_fully_compatible() {
        let env = hosted_bridge_compat_current_version();
        assert_eq!(env.outcome, CompatibilityOutcome::FullyCompatible);
        assert!(env.mutable_controls_available);
        assert!(env.read_only_projections_available);
        assert_eq!(env.cloud_protocol_version, env.daemon_protocol_version);
    }

    #[test]
    fn compat_previous_minor_is_degraded() {
        let env = hosted_bridge_compat_previous_minor();
        assert_eq!(env.outcome, CompatibilityOutcome::DegradedCompatible);
        assert!(env.mutable_controls_available);
        assert!(env.read_only_projections_available);
        assert_ne!(env.cloud_protocol_version, env.daemon_protocol_version);
    }

    #[test]
    fn compat_too_old_daemon_is_blocked() {
        let env = hosted_bridge_compat_too_old_daemon();
        assert_eq!(env.outcome, CompatibilityOutcome::Blocked);
        assert!(!env.mutable_controls_available);
        assert!(!env.read_only_projections_available);
    }

    #[test]
    fn compat_newer_daemon_blocks_mutable_controls() {
        let env = hosted_bridge_compat_newer_daemon();
        assert_eq!(env.outcome, CompatibilityOutcome::NewerDaemonDegraded);
        assert!(!env.mutable_controls_available);
        assert!(env.read_only_projections_available);
    }

    #[test]
    fn all_compatibility_envelopes_roundtrip() {
        for (name, env) in all_compatibility_envelopes() {
            let json = serde_json::to_string_pretty(&env)
                .unwrap_or_else(|e| panic!("compat/{name}: serialize failed: {e}"));
            let rt: ProtocolCompatibilityEnvelope = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("compat/{name}: deserialize failed: {e}"));
            assert_eq!(env, rt, "compat/{name}: round-trip mismatch");
        }
    }

    #[test]
    fn workspace_discovery_healthy_has_binding() {
        let ws = hosted_bridge_workspace_discovery_healthy();
        assert!(ws.has_workspace_binding);
        assert!(ws.code_grounding_ready);
        assert!(ws.event_store_healthy);
        assert_eq!(ws.setup_state, "normal");
    }

    #[test]
    fn workspace_discovery_unbound_not_ready() {
        let ws = hosted_bridge_workspace_discovery_unbound();
        assert!(!ws.has_workspace_binding);
        assert!(!ws.code_grounding_ready);
        assert_eq!(ws.setup_state, "code_grounding_not_ready");
    }

    #[test]
    fn workspace_discovery_repair_unhealthy() {
        let ws = hosted_bridge_workspace_discovery_repair();
        assert!(!ws.event_store_healthy);
        assert!(!ws.has_workspace_binding);
        assert_eq!(ws.setup_state, "repair_required");
    }

    #[test]
    fn workspace_discovery_contains_no_sensitive_fields() {
        for (name, ws) in all_workspace_discovery_fixtures() {
            let json = serde_json::to_string(&ws).unwrap();
            assert!(
                !json.contains("workspace_path"),
                "workspace_discovery/{name}: must not contain workspace_path"
            );
            assert!(
                !json.contains("provider_token"),
                "workspace_discovery/{name}: must not contain provider_token"
            );
            assert!(
                !json.contains("provider_config"),
                "workspace_discovery/{name}: must not contain provider_config"
            );
        }
    }

    #[test]
    fn all_workspace_discovery_roundtrip() {
        for (name, ws) in all_workspace_discovery_fixtures() {
            let json = serde_json::to_string_pretty(&ws)
                .unwrap_or_else(|e| panic!("workspace_discovery/{name}: serialize failed: {e}"));
            let rt: TeamVisibleWorkspaceDiscovery = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("workspace_discovery/{name}: deserialize failed: {e}"));
            assert_eq!(ws, rt, "workspace_discovery/{name}: round-trip mismatch");
        }
    }

    #[test]
    fn bridge_allowed_commands_are_allowed() {
        for cmd in all_bridge_allowed_commands() {
            assert!(cmd.allowed, "bridge command '{}' must be allowed", cmd.name);
        }
    }

    #[test]
    fn bridge_forbidden_commands_are_denied() {
        for cmd in all_bridge_forbidden_commands() {
            assert!(
                !cmd.allowed,
                "bridge command '{}' must be forbidden",
                cmd.name
            );
        }
    }

    #[test]
    fn bridge_forbidden_command_names_match_forbidden_commands_const() {
        let cmds = all_bridge_forbidden_commands();
        let forbidden_names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        for name in &forbidden_names {
            assert!(
                BRIDGE_FORBIDDEN_COMMANDS.contains(name),
                "forbidden fixture command '{}' not in BRIDGE_FORBIDDEN_COMMANDS",
                name
            );
        }
    }

    #[test]
    fn bridge_allowed_command_names_match_allowed_commands_const() {
        let allowed_names: Vec<String> = all_bridge_allowed_commands()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        for name in &allowed_names {
            assert!(
                BRIDGE_ALLOWED_COMMANDS.contains(&name.as_str()),
                "allowed fixture command '{}' not in BRIDGE_ALLOWED_COMMANDS",
                name
            );
        }
    }

    #[test]
    fn bridge_allowed_and_forbidden_do_not_overlap() {
        let allowed: std::collections::HashSet<String> = all_bridge_allowed_commands()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let forbidden: std::collections::HashSet<String> = all_bridge_forbidden_commands()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let overlap: Vec<_> = allowed.intersection(&forbidden).collect();
        assert!(
            overlap.is_empty(),
            "allowed and forbidden command sets must not overlap: {:?}",
            overlap
        );
    }
}
