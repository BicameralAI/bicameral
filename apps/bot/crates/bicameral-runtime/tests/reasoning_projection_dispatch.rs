//! Integration tests for reasoning projection through the dispatch path.
//!
//! Verifies that `ingest.submit_local` routes through daemon-owned
//! projection rather than directly creating candidates.

use bicameral_api::reasoning_projection::{
    AlphaProjectionOutcome, CandidateHintInput, ProjectionRequest, ReasoningProjectionService,
    UnavailableProjectionProvider,
};
use bicameral_api::tool_request::{
    AuthMethod, AuthorityContext, CandidateDraft, IngestEvidenceItem, IngestSubmitLocalParams,
    ToolCommand, ToolRequest, ToolResponseStatus,
};
use bicameral_runtime::dispatch;
use bicameral_runtime::event_store_adapters::MemoryAdapter;

fn test_authority() -> AuthorityContext {
    AuthorityContext {
        actor_id: "agent-test-projection".into(),
        auth_method: AuthMethod::McpSession,
        session_id: Some("sess-projection-test".into()),
        workspace: None,
        policy_scope: vec![],
        audit_metadata: Default::default(),
    }
}

// ── Local ingest with title+description: candidate_projected ──────

#[test]
fn local_ingest_projects_candidate_from_title_description() {
    let adapter = MemoryAdapter::new();
    let authority = test_authority();

    let request = ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "test://projection/basic".into(),
            source_type: "manual".into(),
            label: None,
            title: "Use Redis for session cache".into(),
            description: "Architecture decision: Redis for ephemeral session data".into(),
            suggested_level: Some("architecture".into()),
            snapshot_content: Some("Meeting notes about Redis decision".into()),
            evidence: vec![IngestEvidenceItem {
                excerpt: "Redis for ephemeral session data".into(),
                span_start: Some(0),
                span_end: Some(31),
                confidence: Some(0.9),
            }],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        }),
        authority,
    );

    let response = dispatch(&request, &adapter);
    assert_eq!(response.status, ToolResponseStatus::Ok);

    let result = response.result.unwrap();
    let candidate_ids = result["candidate_ids"].as_array().unwrap();
    assert_eq!(candidate_ids.len(), 1, "One projected candidate expected");
    assert_eq!(result["projection_outcome"], "candidate_projected");
    assert!(result["projection_id"].is_string());
}

// ── Local ingest with title+description+drafts: multiple projected ──

#[test]
fn local_ingest_projects_primary_plus_drafts() {
    let adapter = MemoryAdapter::new();
    let authority = test_authority();

    let request = ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "test://projection/drafts".into(),
            source_type: "meeting_transcript".into(),
            label: None,
            title: "Primary decision: use event sourcing".into(),
            description: "Team decided to use event sourcing for order domain".into(),
            suggested_level: None,
            snapshot_content: None,
            evidence: vec![IngestEvidenceItem {
                excerpt: "use event sourcing for order domain".into(),
                span_start: None,
                span_end: None,
                confidence: Some(0.85),
            }],
            candidate_drafts: vec![CandidateDraft {
                title: "Secondary: validate order schema".into(),
                description: "Input validation is required for order creation".into(),
                suggested_level: Some("implementation".into()),
                tags: vec!["validation".into()],
            }],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        }),
        authority,
    );

    let response = dispatch(&request, &adapter);
    assert_eq!(response.status, ToolResponseStatus::Ok);

    let result = response.result.unwrap();
    let candidate_ids = result["candidate_ids"].as_array().unwrap();
    assert_eq!(
        candidate_ids.len(),
        2,
        "Primary + one draft = two projected candidates"
    );
    assert_eq!(result["projection_outcome"], "candidate_projected");
}

// ── Unit: deterministic provider no_candidate ─────────────────────

#[test]
fn deterministic_provider_no_candidate_on_evidence_only() {
    let svc = ReasoningProjectionService::deterministic();
    let req = ProjectionRequest {
        source_uri: "test://evidence-only".into(),
        source_type: "slack".into(),
        title: None,
        description: None,
        snapshot_content: Some("Some chat transcript".into()),
        evidence_excerpts: vec!["mentioned a library".into()],
        candidate_hints: vec![],
        candidate_drafts: vec![],
    };
    let result = svc.project(&req);
    assert_eq!(result.outcome, AlphaProjectionOutcome::NoCandidate);
    assert!(result.candidates.is_empty());
}

// ── Unit: unavailable provider defers ─────────────────────────────

#[test]
fn unavailable_provider_returns_projection_deferred() {
    let svc = ReasoningProjectionService::new(Box::new(UnavailableProjectionProvider));
    let req = ProjectionRequest {
        source_uri: "test://unavailable".into(),
        source_type: "manual".into(),
        title: Some("A decision".into()),
        description: Some("Some description".into()),
        snapshot_content: None,
        evidence_excerpts: vec![],
        candidate_hints: vec![],
        candidate_drafts: vec![],
    };
    let result = svc.project(&req);
    assert!(result.outcome.is_deferred());
    assert!(result.candidates.is_empty());
    assert!(result.detail.is_some());
}

// ── Unit: hard_rejected construction ──────────────────────────────

#[test]
fn hard_rejected_has_correct_fields() {
    let result = bicameral_api::reasoning_projection::ProjectionResult::hard_rejected(
        "sensitive content detected".into(),
    );
    assert!(
        matches!(result.outcome, AlphaProjectionOutcome::HardRejected { .. }),
        "expected HardRejected"
    );
    assert!(result.candidates.is_empty());
    assert_eq!(result.detail.as_deref(), Some("sensitive content detected"));
}

// ── Unit: no fallback on empty hints with content ─────────────────

#[test]
fn no_fallback_candidate_manufactured_on_content_only() {
    let svc = ReasoningProjectionService::deterministic();
    let req = ProjectionRequest {
        source_uri: "test://content-only".into(),
        source_type: "github".into(),
        title: None,
        description: None,
        snapshot_content: Some("Long issue body with lots of discussion".into()),
        evidence_excerpts: vec!["excerpt one".into(), "excerpt two".into()],
        candidate_hints: vec![],
        candidate_drafts: vec![],
    };
    let result = svc.project(&req);
    assert_eq!(
        result.outcome,
        AlphaProjectionOutcome::NoCandidate,
        "Content/evidence without title+description+hints must not manufacture a fallback"
    );
    assert!(
        result.candidates.is_empty(),
        "No generic fallback candidates permitted"
    );
}

// ── Unit: deterministic projects from hints ───────────────────────

#[test]
fn deterministic_projects_from_external_hints() {
    let svc = ReasoningProjectionService::deterministic();
    let req = ProjectionRequest {
        source_uri: "test://hints".into(),
        source_type: "jira".into(),
        title: None,
        description: None,
        snapshot_content: Some(
            "Team sync outcome: We will use Postgres in place of SQLite for the primary              store. We will use PgBouncer for connection pooling."
                .into(),
        ),
        evidence_excerpts: vec![],
        candidate_hints: vec![
            CandidateHintInput {
                title: "Migrate to Postgres".into(),
                body: "We will use Postgres in place of SQLite for the primary store".into(),
                level: Some("architecture".into()),
                labels: vec!["infra".into()],
            },
            CandidateHintInput {
                title: "Add connection pooling".into(),
                body: "We will use PgBouncer for connection pooling".into(),
                level: Some("implementation".into()),
                labels: vec![],
            },
        ],
        candidate_drafts: vec![],
    };
    let result = svc.project(&req);
    assert!(result.outcome.candidates_materialized());
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(result.candidates[0].title, "Migrate to Postgres");
    assert_eq!(result.candidates[1].title, "Add connection pooling");
}

// ── Unit: empty hint title filtered out ───────────────────────────

#[test]
fn deterministic_filters_empty_hint_titles() {
    let svc = ReasoningProjectionService::deterministic();
    let req = ProjectionRequest {
        source_uri: "test://empty-hint".into(),
        source_type: "slack".into(),
        title: None,
        description: None,
        snapshot_content: None,
        evidence_excerpts: vec![],
        candidate_hints: vec![CandidateHintInput {
            title: "  ".into(),
            body: "Some body".into(),
            level: None,
            labels: vec![],
        }],
        candidate_drafts: vec![],
    };
    let result = svc.project(&req);
    assert_eq!(result.outcome, AlphaProjectionOutcome::NoCandidate);
}
