use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::event_store::{EventStoreEntry, EventType};
use bicameral_api::evidence::SubmissionProvenance;
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_api::journey_review::{
    JourneyAuthorityClass, JourneyPhaseStatus, JourneyReviewView, JOURNEY_REVIEW_SCHEMA_VERSION,
};
use bicameral_audit::store::AuditStore;
use bicameral_gateway::journey_review::journey_review_routes;
use bicameral_gateway::live_projection_work::LiveProjectionWorkStore;
use bicameral_gateway::projection_work_repository::{
    ProjectionOutcomeKind, ProjectionWorkOutcome, ProjectionWorkRepository, ProjectionWorkRequest,
    SqliteProjectionWorkRepository,
};
use bicameral_gateway::AppState;
use chrono::{Duration, Utc};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn endpoint_composes_canonical_acceptance_with_sqlite_no_candidate_outcome() {
    let dir = tempfile::tempdir().expect("tempdir");
    let audit = AuditStore::open(&dir.path().join("audit")).expect("audit store");
    let repository = Arc::new(
        SqliteProjectionWorkRepository::open(dir.path().join("projection.sqlite3"))
            .expect("projection repository"),
    );
    let live_store =
        Arc::new(LiveProjectionWorkStore::open(repository.clone()).expect("live projection store"));
    let state = AppState::new(audit).with_projection_work_store(live_store);

    let now = Utc::now();
    let stored = state
        .evidence_repository
        .put(b"screened github evidence")
        .expect("evidence body");
    let acceptance = EvidenceAcceptedPayload {
        idempotency_key: "journey-http-1".into(),
        evidence_identity: "github:issue:42".into(),
        source_system: "github".into(),
        source_uri: "https://github.example/issues/42".into(),
        product_scope: Some("product-1".into()),
        source_version: Some("sha-1".into()),
        snapshot_addr: "sha256:snapshot".into(),
        content_digest: stored.digest,
        content_locator: stored.locator,
        content_byte_length: stored.byte_length,
        evidence_ids: Vec::new(),
        provenance: SubmissionProvenance::from_external_adapter(
            "github",
            "https://github.example/issues/42",
        ),
        redaction_receipt: None,
        accepted_at: now,
    };
    state
        .event_store
        .append(EventStoreEntry::new(
            EventType::EvidenceAccepted,
            serde_json::to_value(&acceptance).expect("acceptance payload"),
            "gateway".into(),
        ))
        .expect("append acceptance");

    let pending = ProjectionWorkRequest::pending(
        acceptance.idempotency_key.clone(),
        acceptance.source_system.clone(),
        acceptance.source_uri.clone(),
        now,
    );
    assert!(repository.enqueue(&pending).expect("enqueue"));
    let claimed = repository
        .claim_next(now, now + Duration::minutes(5))
        .expect("claim")
        .expect("claimed request");
    repository
        .record_outcome(
            &ProjectionWorkOutcome {
                request_id: claimed.request_id,
                attempt: claimed.attempt,
                kind: ProjectionOutcomeKind::NoCandidate,
                reason_code: Some("content_not_decision_like".into()),
                retryable: false,
                details: json!({"outcome_label": "no_candidate"}),
                completed_at: now + Duration::seconds(1),
            },
            None,
        )
        .expect("record no-candidate outcome");

    let app = Router::new()
        .merge(journey_review_routes())
        .with_state(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/journeys/journey-http-1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response bytes");
    let view: JourneyReviewView = serde_json::from_slice(&bytes).expect("journey view");
    assert_eq!(view.schema_version, JOURNEY_REVIEW_SCHEMA_VERSION);
    assert_eq!(view.journey_id, "journey-http-1");
    assert!(view.recall_packet.decision_candidates.is_empty());
    assert!(view.recall_packet.canonical_decisions.is_empty());
    assert!(view.allowed_actions.is_empty());
    assert!(view.phases.iter().any(|phase| {
        phase.phase_id == "d3.evidence_accepted"
            && phase.status == JourneyPhaseStatus::Accepted
            && phase.authority.after == JourneyAuthorityClass::Canonical
    }));
    assert!(view.phases.iter().any(|phase| {
        phase.phase_id == "d3.projection_outcome.1"
            && phase.status == JourneyPhaseStatus::Completed
            && phase.reason_codes == vec!["content_not_decision_like"]
    }));
    assert!(view
        .caveats
        .iter()
        .any(|caveat| caveat.code == "integrations_trace_not_attached"));
}
