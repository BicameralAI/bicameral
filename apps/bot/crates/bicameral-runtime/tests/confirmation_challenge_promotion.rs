//! Contract/component tests for the daemon-issued `ConfirmationChallenge`
//! candidate-promotion flow (issue #829).
//!
//! These exercise the real dispatch entrypoint
//! [`dispatch_with_confirmation_challenges`] against an in-memory event store
//! and challenge store. They cover the two-phase protocol, its security
//! properties (theft/replay, cross-workspace/actor/outcome misuse, stale-packet
//! promotion, duplicate materialization), lifecycle transitions, idempotent
//! lost-response recovery, and the no-secret-leakage guarantee.

use bicameral_api::confirmation_challenge::{
    ConfirmationOutcome, ConfirmationRejection, ConfirmationToken,
};
use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::fixtures;
use bicameral_api::recall::{
    PromotionOutcome, PromotionRequestOutcome, RecallPromoteCandidateParams,
    RecallPromoteCandidateResult,
};
use bicameral_api::tool_request::{AuthorityContext, ToolCommand, ToolRequest, ToolResponseStatus};
use bicameral_runtime::confirmation_challenge_store::ConfirmationChallengeStore;
use bicameral_runtime::dispatch::dispatch_with_confirmation_challenges;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::replay;
use chrono::Utc;
use uuid::Uuid;

const WORKSPACE: &str = "/home/user/project";
const ACTOR: &str = "user-reviewer-01";
const SESSION: &str = "sess-1";

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

fn authority() -> AuthorityContext {
    fixtures::authority_mcp(ACTOR, SESSION, WORKSPACE)
}

/// Ingest a local candidate and return its id.
fn ingest_candidate(adapter: &MemoryAdapter, store: &ConfirmationChallengeStore) -> Uuid {
    let ingest = fixtures::ingest_submit_local();
    let resp = dispatch_with_confirmation_challenges(&ingest, adapter, None, None, store);
    assert_eq!(resp.status, ToolResponseStatus::Ok, "ingest must succeed");
    let state = replay::replay_from_adapter(adapter).unwrap();
    *state
        .candidates
        .keys()
        .next()
        .expect("ingest should produce a candidate")
}

fn promote_params(candidate_id: Uuid, outcome: PromotionOutcome) -> RecallPromoteCandidateParams {
    RecallPromoteCandidateParams {
        packet_id: Uuid::from_u128(0xACE),
        candidate_id,
        promotion_outcome: outcome,
        supersedes_decision_id: None,
        scoping_relationship: None,
        approval_proof: None,
        confirmation: None,
    }
}

/// Build a promotion `ToolRequest` with an explicit `request_id` so phase 1 and
/// phase 2 can share it (matching how MCP reuses the id across a confirmation).
fn request_with_id(
    request_id: Uuid,
    params: RecallPromoteCandidateParams,
    authority: AuthorityContext,
) -> ToolRequest {
    ToolRequest {
        request_id,
        product_id: None,
        command: ToolCommand::RecallPromoteDecisionCandidate(params),
        authority,
        issued_at: Utc::now(),
    }
}

fn parse_result(resp: &bicameral_api::tool_request::ToolResponse) -> RecallPromoteCandidateResult {
    serde_json::from_value(resp.result.clone().expect("result payload")).unwrap()
}

fn parse_rejection(resp: &bicameral_api::tool_request::ToolResponse) -> ConfirmationRejection {
    serde_json::from_str(resp.message.as_deref().expect("rejection message")).unwrap()
}

fn token_from(result: &RecallPromoteCandidateResult) -> ConfirmationToken {
    let challenge = result
        .confirmation_required
        .as_ref()
        .expect("confirmation_required challenge");
    ConfirmationToken {
        challenge_id: challenge.challenge_id,
        restart_epoch: challenge.restart_epoch,
        secret: challenge.secret.clone(),
    }
}

/// Drive phase 1 and return (request_id, phase-1 result).
fn issue_challenge(
    adapter: &MemoryAdapter,
    store: &ConfirmationChallengeStore,
    candidate_id: Uuid,
    outcome: PromotionOutcome,
) -> (Uuid, RecallPromoteCandidateResult) {
    let request_id = Uuid::new_v4();
    let req = request_with_id(
        request_id,
        promote_params(candidate_id, outcome),
        authority(),
    );
    let resp = dispatch_with_confirmation_challenges(&req, adapter, None, None, store);
    assert_eq!(resp.status, ToolResponseStatus::Ok);
    (request_id, parse_result(&resp))
}

// ---------------------------------------------------------------------------
// Phase 1
// ---------------------------------------------------------------------------

#[test]
fn initial_request_returns_confirmation_and_writes_no_event() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let events_before = adapter.replay().unwrap().len();

    let (_req_id, result) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    assert_eq!(
        result.outcome,
        PromotionRequestOutcome::ConfirmationRequired
    );
    assert!(result.decision_id.is_none());
    let challenge = result.confirmation_required.expect("challenge issued");
    assert_eq!(challenge.binding.candidate_id, candidate_id);
    assert_eq!(challenge.binding.workspace, WORKSPACE);
    assert_eq!(challenge.binding.actor_id, ACTOR);

    // No canonical event was appended by the initial request.
    assert_eq!(
        adapter.replay().unwrap().len(),
        events_before,
        "initial governed request must not append canonical events"
    );
    let state = replay::replay_from_adapter(&adapter).unwrap();
    assert!(state.decisions.is_empty());
}

#[test]
fn free_floating_candidate_is_refused_before_any_challenge() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let req = request_with_id(
        Uuid::new_v4(),
        promote_params(Uuid::new_v4(), PromotionOutcome::NewConstraint),
        authority(),
    );
    let resp = dispatch_with_confirmation_challenges(&req, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert!(resp
        .message
        .as_deref()
        .unwrap_or("")
        .contains("free_floating_claim_refused"));
}

#[test]
fn dismiss_needs_no_challenge() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (_id, result) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::DismissedAsNoise,
    );
    assert_eq!(result.outcome, PromotionRequestOutcome::Dismissed);
    assert!(result.confirmation_required.is_none());
}

#[test]
fn route_to_signoff_needs_no_challenge() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (_id, result) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NeedsSignoffReview,
    );
    assert_eq!(result.outcome, PromotionRequestOutcome::RoutedToSignoff);
    assert!(result.confirmation_required.is_none());
}

// ---------------------------------------------------------------------------
// Phase 2 — happy path & idempotency
// ---------------------------------------------------------------------------

#[test]
fn confirmed_resubmission_promotes_and_materializes_decision() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&phase1));
    let confirm = request_with_id(request_id, params, authority());
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);

    let result = parse_result(&resp);
    assert_eq!(result.outcome, PromotionRequestOutcome::Promoted);
    let decision_id = result.decision_id.expect("decision_id");

    let state = replay::replay_from_adapter(&adapter).unwrap();
    assert!(state.decisions.contains_key(&decision_id));
}

#[test]
fn lost_response_retry_is_idempotent_and_does_not_duplicate() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&phase1));

    let confirm = request_with_id(request_id, params.clone(), authority());
    let first = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    let first_result = parse_result(&first);
    let events_after_first = adapter.replay().unwrap().len();

    // Retry with the same request id (lost-response recovery).
    let retry = request_with_id(request_id, params, authority());
    let second = dispatch_with_confirmation_challenges(&retry, &adapter, None, None, &store);
    let second_result = parse_result(&second);

    assert_eq!(second.status, ToolResponseStatus::Ok);
    assert_eq!(
        first_result.decision_id, second_result.decision_id,
        "retry must return the same canonical decision id"
    );
    assert_eq!(
        adapter.replay().unwrap().len(),
        events_after_first,
        "retry must not append a duplicate canonical event"
    );
    let state = replay::replay_from_adapter(&adapter).unwrap();
    assert_eq!(
        state.decisions.len(),
        1,
        "exactly one decision materialized"
    );
}

// ---------------------------------------------------------------------------
// Phase 2 — fail-closed security & lifecycle
// ---------------------------------------------------------------------------

#[test]
fn cross_workspace_confirmation_fails_closed() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&phase1));
    // Attacker presents the stolen token from a different workspace.
    let evil = fixtures::authority_mcp(ACTOR, SESSION, "/evil/workspace");
    let confirm = request_with_id(request_id, params, evil);
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::WorkspaceMismatch
    );
    assert!(replay::replay_from_adapter(&adapter)
        .unwrap()
        .decisions
        .is_empty());
}

#[test]
fn actor_mismatch_confirmation_fails_closed() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&phase1));
    let other = fixtures::authority_mcp("user-thief", SESSION, WORKSPACE);
    let confirm = request_with_id(request_id, params, other);
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::ActorMismatch
    );
}

#[test]
fn session_mismatch_confirmation_is_unauthorized() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&phase1));
    let other = fixtures::authority_mcp(ACTOR, "sess-other", WORKSPACE);
    let confirm = request_with_id(request_id, params, other);
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::Unauthorized
    );
}

#[test]
fn outcome_mismatch_confirmation_fails_closed() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    // Confirm with a different requested outcome than was challenged.
    let mut params = promote_params(candidate_id, PromotionOutcome::KeepsBothWithScope);
    params.scoping_relationship = Some("scoped".into());
    params.confirmation = Some(token_from(&phase1));
    let confirm = request_with_id(request_id, params, authority());
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::OutcomeMismatch
    );
}

#[test]
fn forged_secret_confirmation_is_unauthorized() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    let real = token_from(&phase1);
    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(ConfirmationToken {
        challenge_id: real.challenge_id,
        restart_epoch: real.restart_epoch,
        secret: bicameral_api::confirmation_challenge::ChallengeSecret::from_minted("forged00"),
    });
    let confirm = request_with_id(request_id, params, authority());
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::Unauthorized
    );
}

#[test]
fn replayed_challenge_after_consume_does_not_duplicate() {
    // A stolen token replayed with a *fresh* request id (so idempotency does
    // not short-circuit it) must not mint a second decision.
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );
    let token = token_from(&phase1);

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token.clone());
    let confirm = request_with_id(request_id, params, authority());
    let ok = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(ok.status, ToolResponseStatus::Ok);
    let decisions_after = replay::replay_from_adapter(&adapter)
        .unwrap()
        .decisions
        .len();

    // Replay the same token under a new request id — the challenge is bound to
    // the original request id, so this is rejected and mints nothing.
    let mut replay_params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    replay_params.confirmation = Some(token);
    let replay_req = request_with_id(Uuid::new_v4(), replay_params, authority());
    let replayed = dispatch_with_confirmation_challenges(&replay_req, &adapter, None, None, &store);
    assert_eq!(replayed.status, ToolResponseStatus::Rejected);
    assert_eq!(
        replay::replay_from_adapter(&adapter)
            .unwrap()
            .decisions
            .len(),
        decisions_after,
        "replayed challenge must not duplicate a decision"
    );
}

#[test]
fn stale_packet_promotion_fails_closed() {
    // Issue a challenge, promote the candidate through a separate confirmed
    // flow, then confirm the first challenge: the candidate has moved on.
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);

    let (stale_req_id, stale_phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );
    let stale_token = token_from(&stale_phase1);

    // Promote via a second, independent confirmed flow.
    let (req2, phase2) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );
    let mut p2 = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    p2.confirmation = Some(token_from(&phase2));
    let confirm2 = request_with_id(req2, p2, authority());
    assert_eq!(
        dispatch_with_confirmation_challenges(&confirm2, &adapter, None, None, &store).status,
        ToolResponseStatus::Ok
    );

    // Now confirm the first (stale) challenge.
    let mut stale_params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    stale_params.confirmation = Some(stale_token);
    let stale_confirm = request_with_id(stale_req_id, stale_params, authority());
    let resp = dispatch_with_confirmation_challenges(&stale_confirm, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::StalePacket
    );
    assert_eq!(
        replay::replay_from_adapter(&adapter)
            .unwrap()
            .decisions
            .len(),
        1,
        "stale confirmation must not add a second decision"
    );
}

#[test]
fn rejected_candidate_confirmation_is_ineligible() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );

    // Reject the candidate between issue and confirm.
    let reject = ToolRequest::new(
        ToolCommand::ReviewRejectCandidate(bicameral_api::tool_request::ReviewTargetParams {
            target_id: candidate_id.to_string(),
            reason: Some("not supported".into()),
        }),
        authority(),
    );
    assert_eq!(
        dispatch_with_confirmation_challenges(&reject, &adapter, None, None, &store).status,
        ToolResponseStatus::Ok
    );

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&phase1));
    let confirm = request_with_id(request_id, params, authority());
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::IneligibleTarget
    );
}

#[test]
fn restart_invalidates_pending_challenge_at_dispatch() {
    let adapter = mem_adapter();
    let mut store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    let (request_id, phase1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );
    let token = token_from(&phase1);

    store.restart();

    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token);
    let confirm = request_with_id(request_id, params, authority());
    let resp = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::ChallengeUnavailableAfterRestart
    );
}

#[test]
fn missing_session_confirmation_is_unauthorized() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);
    // CLI authority has no session id.
    let cli = fixtures::authority_cli(ACTOR, WORKSPACE);
    let req = request_with_id(
        Uuid::new_v4(),
        promote_params(candidate_id, PromotionOutcome::NewConstraint),
        cli,
    );
    let resp = dispatch_with_confirmation_challenges(&req, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Rejected);
    assert_eq!(
        parse_rejection(&resp).outcome,
        ConfirmationOutcome::Unauthorized
    );
}

// ---------------------------------------------------------------------------
// Supersession lineage & scoped coexistence
// ---------------------------------------------------------------------------

#[test]
fn supersession_records_explicit_lineage() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);

    // First, materialize a decision to supersede.
    let (r1, p1) = issue_challenge(
        &adapter,
        &store,
        candidate_id,
        PromotionOutcome::NewConstraint,
    );
    let mut cp1 = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    cp1.confirmation = Some(token_from(&p1));
    let confirm1 = request_with_id(r1, cp1, authority());
    let first = parse_result(&dispatch_with_confirmation_challenges(
        &confirm1, &adapter, None, None, &store,
    ));
    let superseded = first.decision_id.unwrap();

    // Ingest a second candidate that supersedes the first decision.
    let cand2 = {
        let ingest = fixtures::ingest_submit_local();
        dispatch_with_confirmation_challenges(&ingest, &adapter, None, None, &store);
        let state = replay::replay_from_adapter(&adapter).unwrap();
        *state
            .candidates
            .keys()
            .find(|k| **k != candidate_id)
            .expect("second candidate")
    };

    let request_id = Uuid::new_v4();
    let mut p2 = promote_params(cand2, PromotionOutcome::SupersedesExistingDecision);
    p2.supersedes_decision_id = Some(superseded);
    let issue = request_with_id(request_id, p2.clone(), authority());
    let issued = parse_result(&dispatch_with_confirmation_challenges(
        &issue, &adapter, None, None, &store,
    ));
    assert_eq!(
        issued.outcome,
        PromotionRequestOutcome::ConfirmationRequired
    );

    p2.confirmation = Some(token_from(&issued));
    let confirm2 = request_with_id(request_id, p2, authority());
    let resp = dispatch_with_confirmation_challenges(&confirm2, &adapter, None, None, &store);
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);
    assert_eq!(
        parse_result(&resp).outcome,
        PromotionRequestOutcome::Promoted
    );

    // The lineage is persisted in the canonical event stream.
    let events = adapter.replay().unwrap();
    let has_lineage = events.iter().any(|e| {
        e.payload
            .get("supersedes_decision_id")
            .and_then(|v| v.as_str())
            == Some(superseded.to_string().as_str())
    });
    assert!(
        has_lineage,
        "supersession event must record explicit lineage"
    );
}

// ---------------------------------------------------------------------------
// No secret leakage
// ---------------------------------------------------------------------------

#[test]
fn challenge_secret_never_appears_in_trace_or_rejections() {
    let adapter = mem_adapter();
    let store = ConfirmationChallengeStore::new();
    let candidate_id = ingest_candidate(&adapter, &store);

    let request_id = Uuid::new_v4();
    let req = request_with_id(
        request_id,
        promote_params(candidate_id, PromotionOutcome::NewConstraint),
        authority(),
    );
    let resp = dispatch_with_confirmation_challenges(&req, &adapter, None, None, &store);
    let result = parse_result(&resp);
    let secret = result
        .confirmation_required
        .as_ref()
        .unwrap()
        .secret
        .expose_for_transport()
        .to_string();
    assert!(!secret.is_empty());

    // The runtime trace attached to the response must not carry the secret.
    let trace_dbg = format!("{:?}", resp.trace);
    assert!(
        !trace_dbg.contains(&secret),
        "trace must not contain the secret"
    );

    // A subsequent mismatched confirmation's rejection must not carry it.
    let mut params = promote_params(candidate_id, PromotionOutcome::NewConstraint);
    params.confirmation = Some(token_from(&result));
    let evil = fixtures::authority_mcp("user-thief", SESSION, WORKSPACE);
    let confirm = request_with_id(request_id, params, evil);
    let rej = dispatch_with_confirmation_challenges(&confirm, &adapter, None, None, &store);
    assert!(!rej.message.as_deref().unwrap_or("").contains(&secret));
    assert!(!format!("{:?}", rej.trace).contains(&secret));
}
