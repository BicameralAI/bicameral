//! Daemon-owned journey review composition and HTTP query surface.

use crate::live_projection_work::LiveProjectionWorkError;
use crate::projection_work_repository::{
    ProjectionOutcomeKind, ProjectionRequestStatus, ProjectionWorkOutcome,
    ProjectionWorkRepository, ProjectionWorkRequest,
};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use bicameral_api::candidate::{Decision, DecisionCandidate};
use bicameral_api::event_store::{EventStoreError, EventType};
use bicameral_api::evidence_acceptance::EvidenceAcceptedPayload;
use bicameral_api::journey_review::{
    JourneyActionKind, JourneyAllowedAction, JourneyAuthorityClass, JourneyAuthorityTransition,
    JourneyCaveat, JourneyPersistenceEffect, JourneyPersistenceOperation, JourneyPersistencePlane,
    JourneyPhaseStatus, JourneyPhaseView, JourneyReviewView, JourneySourceView,
    JOURNEY_REVIEW_SCHEMA_VERSION,
};
use bicameral_api::recall::{
    CandidateRelevance, CanonicalDecisionEntry, RecallPacket, SurfacedDecisionCandidate,
    UnknownScopeEntry,
};
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JourneyReviewBuildError {
    #[error("journey not found: {0}")]
    NotFound(String),
    #[error("canonical event replay failed: {0}")]
    EventStore(#[from] EventStoreError),
    #[error("journey working-state query failed: {0}")]
    WorkingState(#[from] LiveProjectionWorkError),
    #[error("journey acceptance payload is invalid: {0}")]
    InvalidAcceptance(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
struct JourneyReviewErrorResponse {
    code: &'static str,
    detail: String,
}

impl JourneyReviewBuildError {
    fn response(self) -> Response {
        let (status, code) = match &self {
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "journey_not_found"),
            Self::InvalidAcceptance(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_acceptance_payload",
            ),
            Self::EventStore(_) | Self::WorkingState(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "journey_query_unavailable")
            }
        };
        (
            status,
            Json(JourneyReviewErrorResponse {
                code,
                detail: self.to_string(),
            }),
        )
            .into_response()
    }
}

pub fn journey_review_routes() -> Router<AppState> {
    Router::new().route("/api/v1/journeys/:journey_id", get(get_journey_review))
}

async fn get_journey_review(
    State(state): State<AppState>,
    Path(journey_id): Path<String>,
) -> Response {
    match build_journey_review(&state, &journey_id).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => error.response(),
    }
}

pub async fn build_journey_review(
    state: &AppState,
    journey_id: &str,
) -> Result<JourneyReviewView, JourneyReviewBuildError> {
    let events = state.event_store.replay()?;
    let acceptance = events
        .iter()
        .filter(|event| event.event_type == EventType::EvidenceAccepted)
        .find_map(|event| {
            let payload =
                serde_json::from_value::<EvidenceAcceptedPayload>(event.payload.clone()).ok()?;
            (payload.idempotency_key == journey_id).then_some(payload)
        })
        .ok_or_else(|| JourneyReviewBuildError::NotFound(journey_id.to_string()))?;

    let evidence_body_verified = state
        .evidence_repository
        .read(&acceptance.content_locator)
        .map(|bytes| bytes.len() == acceptance.content_byte_length)
        .unwrap_or(false);

    let (request, outcomes, candidates) = if let Some(store) = state.projection_work_store.as_ref()
    {
        if let Some(request) = store.request_by_acceptance_key(journey_id)? {
            let outcomes = store
                .repository()
                .list_outcomes(request.request_id)
                .map_err(LiveProjectionWorkError::from)?;
            let candidates = store.candidate_payloads(request.request_id)?;
            (Some(request), outcomes, candidates)
        } else {
            (None, Vec::new(), Vec::new())
        }
    } else {
        (None, Vec::new(), Vec::new())
    };

    let decisions = state
        .decisions
        .read()
        .await
        .values()
        .filter(|decision| decision_belongs_to_journey(decision, &acceptance))
        .cloned()
        .collect::<Vec<_>>();

    Ok(compose_journey_review(JourneyReviewInput {
        acceptance,
        evidence_body_verified,
        request,
        outcomes,
        candidates,
        decisions,
        working_store_configured: state.projection_work_store.is_some(),
    }))
}

#[derive(Debug)]
struct JourneyReviewInput {
    acceptance: EvidenceAcceptedPayload,
    evidence_body_verified: bool,
    request: Option<ProjectionWorkRequest>,
    outcomes: Vec<ProjectionWorkOutcome>,
    candidates: Vec<DecisionCandidate>,
    decisions: Vec<Decision>,
    working_store_configured: bool,
}

fn compose_journey_review(input: JourneyReviewInput) -> JourneyReviewView {
    let mut caveats = vec![JourneyCaveat {
        code: "integrations_trace_not_attached".into(),
        detail: "Integrations phase records are not yet attached to the daemon journey store; the view begins at daemon admission.".into(),
    }];
    let mut phases = vec![unavailable_integrations_phase()];

    phases.push(JourneyPhaseView {
        phase_id: "d3.evidence_body".into(),
        component: "bicameral-gateway".into(),
        function: "EvidenceRepository::read".into(),
        status: if input.evidence_body_verified {
            JourneyPhaseStatus::Accepted
        } else {
            caveats.push(JourneyCaveat {
                code: "evidence_body_unavailable".into(),
                detail: "The canonical acceptance fact exists, but the referenced local evidence body could not be verified.".into(),
            });
            JourneyPhaseStatus::Failed
        },
        reason_codes: if !input.evidence_body_verified {
            vec!["evidence_body_unavailable".into()]
        } else {
            Vec::new()
        },
        persistence: persistence(
            JourneyPersistencePlane::EvidenceBody,
            JourneyPersistenceOperation::Read,
        ),
        authority: authority(
            JourneyAuthorityClass::Evidence,
            JourneyAuthorityClass::Evidence,
            "verified_screened_evidence_body",
        ),
        request_id: None,
        attempt: None,
        retryable: None,
        occurred_at: Some(input.acceptance.accepted_at),
        sanitized_summary: json!({
            "content_digest": input.acceptance.content_digest,
            "byte_length": input.acceptance.content_byte_length,
            "verified": input.evidence_body_verified,
        }),
    });

    phases.push(JourneyPhaseView {
        phase_id: "d3.evidence_accepted".into(),
        component: "bicameral-runtime".into(),
        function: "EventStoreAdapter::append(EvidenceAccepted)".into(),
        status: JourneyPhaseStatus::Accepted,
        reason_codes: Vec::new(),
        persistence: persistence(
            JourneyPersistencePlane::CanonicalEvent,
            JourneyPersistenceOperation::Append,
        ),
        authority: authority(
            JourneyAuthorityClass::Evidence,
            JourneyAuthorityClass::Canonical,
            "canonical_evidence_acceptance_fact",
        ),
        request_id: None,
        attempt: None,
        retryable: None,
        occurred_at: Some(input.acceptance.accepted_at),
        sanitized_summary: json!({
            "evidence_identity": input.acceptance.evidence_identity,
            "evidence_count": input.acceptance.evidence_ids.len(),
            "redaction_receipt_present": input.acceptance.redaction_receipt.is_some(),
        }),
    });

    if let Some(request) = input.request.as_ref() {
        phases.push(request_phase(request));
        for outcome in &input.outcomes {
            phases.push(outcome_phase(outcome));
        }
    } else {
        if !input.working_store_configured {
            caveats.push(JourneyCaveat {
                code: "projection_work_store_unavailable".into(),
                detail: "No daemon-local projection working store is configured for this process."
                    .into(),
            });
        } else {
            caveats.push(JourneyCaveat {
                code: "projection_request_not_found".into(),
                detail: "Accepted evidence has no surviving local projection request; canonical evidence remains unaffected.".into(),
            });
        }
        phases.push(JourneyPhaseView {
            phase_id: "d3.projection_work".into(),
            component: "bicameral-gateway".into(),
            function: "LiveProjectionWorkStore::request_by_acceptance_key".into(),
            status: JourneyPhaseStatus::Unavailable,
            reason_codes: vec![if input.working_store_configured {
                "projection_request_not_found".into()
            } else {
                "projection_work_store_unavailable".into()
            }],
            persistence: persistence(
                JourneyPersistencePlane::LocalWork,
                JourneyPersistenceOperation::Read,
            ),
            authority: authority(
                JourneyAuthorityClass::Evidence,
                JourneyAuthorityClass::Evidence,
                "no_authority_transition",
            ),
            request_id: None,
            attempt: None,
            retryable: None,
            occurred_at: None,
            sanitized_summary: json!({}),
        });
    }

    if !input.candidates.is_empty() {
        phases.push(JourneyPhaseView {
            phase_id: "d3.candidate_exposure".into(),
            component: "bicameral-gateway".into(),
            function: "LiveProjectionWorkStore::candidate_payloads".into(),
            status: JourneyPhaseStatus::Completed,
            reason_codes: Vec::new(),
            persistence: persistence(
                JourneyPersistencePlane::LocalWork,
                JourneyPersistenceOperation::Read,
            ),
            authority: authority(
                JourneyAuthorityClass::Evidence,
                JourneyAuthorityClass::Proposal,
                "local_candidate_projection",
            ),
            request_id: input.request.as_ref().map(|request| request.request_id),
            attempt: input.request.as_ref().map(|request| request.attempt),
            retryable: None,
            occurred_at: input
                .candidates
                .iter()
                .map(|candidate| candidate.extracted_at)
                .max(),
            sanitized_summary: json!({
                "candidate_count": input.candidates.len(),
                "candidate_ids": input.candidates.iter().map(|candidate| candidate.id).collect::<Vec<_>>(),
            }),
        });
    }

    if !input.decisions.is_empty() {
        phases.push(JourneyPhaseView {
            phase_id: "d3.decision_promotion".into(),
            component: "bicameral-runtime".into(),
            function: "dispatch_review_mutation(AcceptCandidate)".into(),
            status: JourneyPhaseStatus::Completed,
            reason_codes: Vec::new(),
            persistence: persistence(
                JourneyPersistencePlane::CanonicalEvent,
                JourneyPersistenceOperation::Append,
            ),
            authority: authority(
                JourneyAuthorityClass::Proposal,
                JourneyAuthorityClass::Canonical,
                "governed_candidate_promotion",
            ),
            request_id: input.request.as_ref().map(|request| request.request_id),
            attempt: None,
            retryable: None,
            occurred_at: input.decisions.iter().map(|decision| decision.promoted_at).max(),
            sanitized_summary: json!({
                "decision_count": input.decisions.len(),
                "decision_ids": input.decisions.iter().map(|decision| decision.id).collect::<Vec<_>>(),
            }),
        });
    }

    let mut packet = RecallPacket::new_alpha();
    packet.canonical_decisions = input
        .decisions
        .iter()
        .map(|decision| CanonicalDecisionEntry {
            decision_id: decision.id,
            title: decision.title.clone(),
            summary: decision.body.clone(),
            level: decision.level,
            signoff_state: decision.signoff_state,
            evidence_refs: Vec::new(),
            authority_labels: Vec::new(),
        })
        .collect();
    packet.decision_candidates = input
        .candidates
        .iter()
        .map(|candidate| {
            let related = input
                .decisions
                .iter()
                .filter(|decision| decision.candidate_id == candidate.id)
                .map(|decision| decision.id)
                .collect();
            SurfacedDecisionCandidate::from_candidate(
                candidate,
                vec![CandidateRelevance::FullLedgerScan],
                related,
            )
        })
        .collect();
    packet.unknown_scope.push(UnknownScopeEntry {
        scope_id: "integrations_phase_trace".into(),
        description: "The daemon does not yet retain the upstream Integrations transformation phases for this journey.".into(),
        expansion_targets: vec!["integrations_transformation_trace".into()],
    });
    packet = packet.with_derived_decision_attention();

    let allowed_actions = input
        .candidates
        .iter()
        .flat_map(|candidate| {
            [
                JourneyAllowedAction {
                    command: JourneyActionKind::CandidateHide,
                    target_id: candidate.id,
                    requires_authority: false,
                },
                JourneyAllowedAction {
                    command: JourneyActionKind::CandidateSuppress,
                    target_id: candidate.id,
                    requires_authority: true,
                },
                JourneyAllowedAction {
                    command: JourneyActionKind::CandidateReject,
                    target_id: candidate.id,
                    requires_authority: true,
                },
                JourneyAllowedAction {
                    command: JourneyActionKind::CandidatePromote,
                    target_id: candidate.id,
                    requires_authority: true,
                },
            ]
        })
        .collect();

    JourneyReviewView {
        schema_version: JOURNEY_REVIEW_SCHEMA_VERSION,
        journey_id: input.acceptance.idempotency_key,
        product_scope: input.acceptance.product_scope,
        source: JourneySourceView {
            provider: input.acceptance.source_system,
            evidence_identity: input.acceptance.evidence_identity,
            source_uri: input.acceptance.source_uri,
            source_version: input.acceptance.source_version,
            content_digest: input.acceptance.content_digest,
            accepted_at: input.acceptance.accepted_at,
        },
        phases,
        recall_packet: packet,
        allowed_actions,
        caveats,
        generated_at: Utc::now(),
    }
}

fn decision_belongs_to_journey(decision: &Decision, acceptance: &EvidenceAcceptedPayload) -> bool {
    decision
        .evidence_refs
        .iter()
        .any(|id| acceptance.evidence_ids.contains(id))
        || decision.source_id.as_deref() == Some(acceptance.source_uri.as_str())
}

fn unavailable_integrations_phase() -> JourneyPhaseView {
    JourneyPhaseView {
        phase_id: "d1_d2.integrations_trace".into(),
        component: "bicameral-integrations".into(),
        function: "transformation_trace".into(),
        status: JourneyPhaseStatus::Unavailable,
        reason_codes: vec!["integrations_trace_not_attached".into()],
        persistence: persistence(
            JourneyPersistencePlane::None,
            JourneyPersistenceOperation::None,
        ),
        authority: authority(
            JourneyAuthorityClass::Evidence,
            JourneyAuthorityClass::Evidence,
            "upstream_trace_unavailable",
        ),
        request_id: None,
        attempt: None,
        retryable: None,
        occurred_at: None,
        sanitized_summary: json!({}),
    }
}

fn request_phase(request: &ProjectionWorkRequest) -> JourneyPhaseView {
    let status = match request.status {
        ProjectionRequestStatus::Deferred => JourneyPhaseStatus::Deferred,
        ProjectionRequestStatus::Failed => JourneyPhaseStatus::Failed,
        ProjectionRequestStatus::Completed => JourneyPhaseStatus::Completed,
        ProjectionRequestStatus::Pending | ProjectionRequestStatus::Running => {
            JourneyPhaseStatus::Accepted
        }
    };
    JourneyPhaseView {
        phase_id: "d3.projection_work".into(),
        component: "bicameral-gateway".into(),
        function: "LiveProjectionWorkStore::request_by_acceptance_key".into(),
        status,
        reason_codes: request.last_error_code.clone().into_iter().collect(),
        persistence: persistence(
            JourneyPersistencePlane::LocalWork,
            JourneyPersistenceOperation::Read,
        ),
        authority: authority(
            JourneyAuthorityClass::Evidence,
            JourneyAuthorityClass::Evidence,
            "noncanonical_work_tracking",
        ),
        request_id: Some(request.request_id),
        attempt: Some(request.attempt),
        retryable: Some(matches!(request.status, ProjectionRequestStatus::Deferred)),
        occurred_at: Some(request.updated_at),
        sanitized_summary: json!({
            "status": request.status,
            "source_system": request.source_system,
            "source_uri": request.source_uri,
        }),
    }
}

fn outcome_phase(outcome: &ProjectionWorkOutcome) -> JourneyPhaseView {
    let status = match outcome.kind {
        ProjectionOutcomeKind::Deferred => JourneyPhaseStatus::Deferred,
        ProjectionOutcomeKind::Failed => JourneyPhaseStatus::Failed,
        ProjectionOutcomeKind::Candidate | ProjectionOutcomeKind::NoCandidate => {
            JourneyPhaseStatus::Completed
        }
    };
    JourneyPhaseView {
        phase_id: format!("d3.projection_outcome.{}", outcome.attempt),
        component: "bicameral-gateway".into(),
        function: "ProjectionWorkRepository::record_outcome".into(),
        status,
        reason_codes: outcome.reason_code.clone().into_iter().collect(),
        persistence: persistence(
            JourneyPersistencePlane::LocalWork,
            JourneyPersistenceOperation::Read,
        ),
        authority: authority(
            JourneyAuthorityClass::Evidence,
            if outcome.kind == ProjectionOutcomeKind::Candidate {
                JourneyAuthorityClass::Proposal
            } else {
                JourneyAuthorityClass::Evidence
            },
            if outcome.kind == ProjectionOutcomeKind::Candidate {
                "candidate_proposal_created"
            } else {
                "no_canonical_authority_transition"
            },
        ),
        request_id: Some(outcome.request_id),
        attempt: Some(outcome.attempt),
        retryable: Some(outcome.retryable),
        occurred_at: Some(outcome.completed_at),
        sanitized_summary: json!({
            "kind": outcome.kind,
            "reason_code": outcome.reason_code,
            "retryable": outcome.retryable,
        }),
    }
}

fn persistence(
    plane: JourneyPersistencePlane,
    operation: JourneyPersistenceOperation,
) -> JourneyPersistenceEffect {
    JourneyPersistenceEffect { plane, operation }
}

fn authority(
    before: JourneyAuthorityClass,
    after: JourneyAuthorityClass,
    transition: &str,
) -> JourneyAuthorityTransition {
    JourneyAuthorityTransition {
        before,
        after,
        transition: transition.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::candidate::{DecisionLevel, ExtractionConfidence};
    use bicameral_api::dashboard::{ComplianceState, SignoffState};
    use bicameral_api::evidence::SubmissionProvenance;
    use bicameral_api::external_ingest::ExternalRedactionReceipt;
    use bicameral_api::{CandidateState, ReviewLifecycleState, TrackingState};
    use chrono::Duration;
    use serde_json::Map;
    use uuid::Uuid;

    fn acceptance(now: chrono::DateTime<Utc>, evidence_id: Uuid) -> EvidenceAcceptedPayload {
        EvidenceAcceptedPayload {
            idempotency_key: "journey-1".into(),
            evidence_identity: "github:issue:42".into(),
            source_system: "github".into(),
            source_uri: "https://github.example/issues/42".into(),
            product_scope: Some("product-1".into()),
            source_version: Some("sha-1".into()),
            snapshot_addr: "sha256:snapshot".into(),
            content_digest: "sha256:content".into(),
            content_locator: "sha256/ab/abcdef".into(),
            content_byte_length: 17,
            evidence_ids: vec![evidence_id],
            provenance: SubmissionProvenance {
                submitting_surface: Some("external_adapter".into()),
                source_kind: Some("github".into()),
                actor_id: None,
                session_id: None,
                request_id: None,
                source_uri: Some("https://github.example/issues/42".into()),
                submitted_at: now,
            },
            redaction_receipt: None::<ExternalRedactionReceipt>,
            accepted_at: now,
        }
    }

    #[test]
    fn composition_preserves_candidate_and_decision_authority() {
        let now = Utc::now();
        let evidence_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let candidate_id = Uuid::new_v4();
        let decision_id = Uuid::new_v4();
        let candidate = DecisionCandidate {
            id: candidate_id,
            title: "Use a local event store".into(),
            description: "The daemon owns canonical Product events.".into(),
            level: DecisionLevel::Architecture,
            source: "https://github.example/issues/42".into(),
            extraction_confidence: ExtractionConfidence::High,
            extracted_at: now,
            extracted_by: "deterministic_projection".into(),
            tags: Vec::new(),
            evidence_refs: vec![evidence_id],
            evidence_reference_ids: Vec::new(),
            candidate_state: CandidateState::Proposed,
            review_state: ReviewLifecycleState::Proposed,
            metadata: serde_json::Value::Null,
            provenance: None,
        };
        let decision = Decision {
            id: decision_id,
            candidate_id,
            title: candidate.title.clone(),
            body: candidate.description.clone(),
            level: candidate.level,
            signoff_state: SignoffState::Proposed,
            compliance_state: ComplianceState::Ungrounded,
            review_state: ReviewLifecycleState::Proposed,
            evidence_refs: vec![evidence_id],
            evidence_reference_ids: Vec::new(),
            source_id: Some(candidate.source.clone()),
            tracking_state: TrackingState::Tracked,
            promoted_at: now + Duration::seconds(3),
            promoted_by: "operator-1".into(),
            provenance: None,
        };
        let request = ProjectionWorkRequest {
            request_id,
            evidence_acceptance_key: "journey-1".into(),
            product_scope: Some("product-1".into()),
            source_system: "github".into(),
            source_uri: candidate.source.clone(),
            status: ProjectionRequestStatus::Completed,
            attempt: 1,
            available_at: now,
            lease_expires_at: None,
            last_error_code: None,
            created_at: now,
            updated_at: now + Duration::seconds(1),
        };
        let outcome = ProjectionWorkOutcome {
            request_id,
            attempt: 1,
            kind: ProjectionOutcomeKind::Candidate,
            reason_code: Some("candidate_projected".into()),
            retryable: false,
            details: serde_json::Value::Object(Map::new()),
            completed_at: now + Duration::seconds(2),
        };

        let view = compose_journey_review(JourneyReviewInput {
            acceptance: acceptance(now, evidence_id),
            evidence_body_verified: true,
            request: Some(request),
            outcomes: vec![outcome],
            candidates: vec![candidate],
            decisions: vec![decision],
            working_store_configured: true,
        });

        assert_eq!(view.schema_version, JOURNEY_REVIEW_SCHEMA_VERSION);
        assert_eq!(view.recall_packet.decision_candidates.len(), 1);
        assert_eq!(view.recall_packet.canonical_decisions.len(), 1);
        assert_eq!(view.allowed_actions.len(), 4);
        assert!(view.phases.iter().any(|phase| {
            phase.authority.before == JourneyAuthorityClass::Evidence
                && phase.authority.after == JourneyAuthorityClass::Proposal
        }));
        assert!(view.phases.iter().any(|phase| {
            phase.authority.before == JourneyAuthorityClass::Proposal
                && phase.authority.after == JourneyAuthorityClass::Canonical
        }));
        assert!(view
            .caveats
            .iter()
            .any(|caveat| caveat.code == "integrations_trace_not_attached"));
    }
}
