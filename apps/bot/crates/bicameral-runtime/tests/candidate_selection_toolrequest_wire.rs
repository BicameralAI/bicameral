//! ToolRequest wire for Managed MCP CandidateSet selection (#987 / #989).
//!
//! Proves the four commands are advertised, deserialize, and route through
//! `dispatch_with_candidate_selection` into the production session/selection
//! services with MCP-compatible result keys.

use std::sync::Arc;

use bicameral_api::candidate_selection::{
    CandidatePromotionPolicyV1, CandidateSelectionAuthorityV1, CandidateSelectionCommitV1,
    CandidateSelectionV1, CANDIDATE_SELECTION_COMMIT_COMMAND, CANDIDATE_SELECTION_CONTRACT_VERSION,
    CANDIDATE_SELECTION_PREPARE_COMMAND,
};
use bicameral_api::candidate_session::{
    CandidateDisposition, CandidateSessionAccessV1, CandidateSessionEndRequestV1,
    CandidateSessionHeartbeatRequestV1, CandidateSetBindingV1, SessionCandidateDraftV1,
    CANDIDATE_SESSION_CONTRACT_VERSION, CANDIDATE_SESSION_END_COMMAND,
    CANDIDATE_SESSION_HEARTBEAT_COMMAND,
};
use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::tool_request::{
    AuthMethod, AuthorityContext, ToolCommand, ToolRequest, ToolResponseStatus,
};
use bicameral_runtime::candidate_selection_authority::CandidateSelectionAuthorityRegistry;
use bicameral_runtime::candidate_selection_service::CandidateSelectionService;
use bicameral_runtime::candidate_session_store::{
    CandidateSessionStore, CreateCandidateSetRequest,
};
use bicameral_runtime::dispatch::{
    capability_report, dispatch_with_candidate_selection, CandidateSelectionDispatchContext,
    SUPPORTED_COMMANDS,
};
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use chrono::{Duration, TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PRODUCT: &str = "product-wire";
const SESSION: &str = "host-session-wire";
const LEASE: &str = "lease-wire";
const OWNER: &str = "owner-wire";
const CANDIDATE_A: Uuid = Uuid::from_u128(0xA11);
const CANDIDATE_B: Uuid = Uuid::from_u128(0xB22);

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()
}

fn digest(seed: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(seed.as_bytes()))
}

fn authority_ctx() -> AuthorityContext {
    AuthorityContext {
        actor_id: OWNER.into(),
        auth_method: AuthMethod::McpSession,
        session_id: Some(SESSION.into()),
        workspace: None,
        policy_scope: vec![],
        audit_metadata: Default::default(),
    }
}

fn binding() -> CandidateSetBindingV1 {
    CandidateSetBindingV1 {
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        governing_spec_binding_digest: digest("governing-spec-wire"),
        plan_digest: digest("completed-plan-wire"),
        accepted_model_base_position: 11,
        accepted_model_base_digest: digest("accepted-base-wire"),
    }
}

fn draft(id: Uuid) -> SessionCandidateDraftV1 {
    SessionCandidateDraftV1 {
        candidate_id: id,
        proposed_decision: format!("Decision for {id}"),
        source_spans: Vec::new(),
        contradiction_refs: Vec::new(),
        dependency_candidate_ids: Vec::new(),
        claim_trace_digest: digest(&format!("claim-{id}")),
        proposal_digest: digest(&format!("proposal-{id}")),
    }
}

struct WireHarness {
    service: Arc<CandidateSelectionService>,
    set: bicameral_api::candidate_session::CandidateSetV1,
    events: Arc<MemoryAdapter>,
}

fn harness() -> WireHarness {
    let events = Arc::new(MemoryAdapter::new());
    let sessions = Arc::new(CandidateSessionStore::new());
    // Seed with wall clock: dispatch prepare/heartbeat/end use Utc::now() and
    // leases expire after CANDIDATE_SESSION_RECONNECT_GRACE_SECONDS.
    let set = sessions
        .create(
            CreateCandidateSetRequest {
                contract_version: CANDIDATE_SESSION_CONTRACT_VERSION,
                binding: binding(),
                session_lease_id: LEASE.into(),
                candidates: vec![draft(CANDIDATE_A), draft(CANDIDATE_B)],
                coverage_limitations: Vec::new(),
            },
            Utc::now(),
        )
        .expect("seed CandidateSet");
    let authority = Arc::new(CandidateSelectionAuthorityRegistry::new());
    authority.project_current(CandidateSelectionAuthorityV1 {
        product_id: PRODUCT.into(),
        actor_id: OWNER.into(),
        may_promote: true,
        is_current_product_owner: true,
        governing_spec_binding_digest: binding().governing_spec_binding_digest,
        accepted_model_base_position: binding().accepted_model_base_position,
        accepted_model_base_digest: binding().accepted_model_base_digest,
        promotion_policy: CandidatePromotionPolicyV1 {
            enabled: false,
            policy_revision: "policy-wire-1".into(),
        },
    });
    let service = Arc::new(CandidateSelectionService::new(
        sessions,
        authority,
        events.clone(),
    ));
    WireHarness {
        service,
        set,
        events,
    }
}

fn dispatch(h: &WireHarness, command: ToolCommand) -> bicameral_api::tool_request::ToolResponse {
    let request = ToolRequest::new(command, authority_ctx()).for_product(PRODUCT);
    let ctx = CandidateSelectionDispatchContext {
        service: h.service.as_ref(),
        atomic_event_store: Some(h.events.as_ref()),
    };
    dispatch_with_candidate_selection(&request, h.events.as_ref(), None, None, &ctx)
}

#[test]
fn supported_commands_advertise_selection_and_session() {
    for name in [
        CANDIDATE_SELECTION_PREPARE_COMMAND,
        CANDIDATE_SELECTION_COMMIT_COMMAND,
        CANDIDATE_SESSION_HEARTBEAT_COMMAND,
        CANDIDATE_SESSION_END_COMMAND,
    ] {
        assert!(
            SUPPORTED_COMMANDS.contains(&name),
            "{name} must be in SUPPORTED_COMMANDS"
        );
    }
    let report = capability_report();
    assert!(report
        .supported_commands
        .contains(&CANDIDATE_SELECTION_PREPARE_COMMAND));
}

#[test]
fn toolcommand_serde_tags_match_mcp_command_names() {
    let prepare = ToolCommand::CandidateSelectionPrepare(CandidateSelectionV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::from_u128(1),
        candidate_set_id: Uuid::from_u128(2),
        candidate_set_generation: 1,
        candidate_set_digest: digest("set"),
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        session_lease_id: LEASE.into(),
        promote_candidate_ids: vec![CANDIDATE_A],
        reject_candidate_ids: vec![],
        actor_id: OWNER.into(),
    });
    assert_eq!(prepare.command_name(), "candidate_selection.prepare");
    let json = serde_json::to_value(&prepare).unwrap();
    assert_eq!(json["name"], "candidate_selection.prepare");

    let heartbeat = ToolCommand::CandidateSessionHeartbeat(CandidateSessionHeartbeatRequestV1 {
        access: CandidateSessionAccessV1::new(PRODUCT, SESSION, LEASE, 1),
    });
    assert_eq!(heartbeat.command_name(), "candidate_session.heartbeat");
}

#[test]
fn prepare_commit_heartbeat_end_through_toolrequest() {
    let h = harness();

    // prepare
    let prepare_cmd = ToolCommand::CandidateSelectionPrepare(CandidateSelectionV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::new_v4(),
        candidate_set_id: h.set.candidate_set_id,
        candidate_set_generation: h.set.candidate_set_generation,
        candidate_set_digest: h.set.candidate_set_digest.clone(),
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        session_lease_id: h.set.session_lease_id.clone(),
        promote_candidate_ids: vec![CANDIDATE_A],
        reject_candidate_ids: vec![CANDIDATE_B],
        actor_id: OWNER.into(),
    });
    let prepare_resp = dispatch(&h, prepare_cmd);
    assert_eq!(prepare_resp.status, ToolResponseStatus::Ok);
    let preview = prepare_resp
        .result
        .as_ref()
        .and_then(|v| v.get("candidate_selection_preview"))
        .cloned()
        .expect("preview key for MCP render_prepare_response");
    assert_eq!(preview["contract_version"], 1);
    assert!(preview.get("confirmation_challenge").is_some());
    assert_eq!(preview["promote_effects"].as_array().unwrap().len(), 1);
    assert_eq!(preview["reject_effects"].as_array().unwrap().len(), 1);

    // commit with human confirmation + daemon token
    let token = preview["confirmation_challenge"].clone();
    let commit_cmd = ToolCommand::CandidateSelectionCommit(CandidateSelectionCommitV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: preview["selection_operation_id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        selection_digest: preview["selection_digest"].as_str().unwrap().into(),
        actor_id: OWNER.into(),
        human_confirmed: true,
        confirmation_token: serde_json::from_value(token).expect("token shape"),
    });
    let commit_resp = dispatch(&h, commit_cmd);
    assert_eq!(
        commit_resp.status,
        ToolResponseStatus::Ok,
        "{commit_resp:?}"
    );
    let receipt = commit_resp
        .result
        .as_ref()
        .and_then(|v| v.get("candidate_selection_receipt"))
        .cloned()
        .expect("receipt key for MCP render_commit_response");
    assert_eq!(
        receipt["promoted_candidate_ids"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        receipt["rejected_candidate_ids"].as_array().unwrap().len(),
        1
    );
    assert_eq!(receipt["replay_verified"], true);
    assert_eq!(receipt["session_reconciled"], true);
    assert_eq!(h.events.replay().unwrap().len(), 1);

    // heartbeat
    let hb_cmd = ToolCommand::CandidateSessionHeartbeat(CandidateSessionHeartbeatRequestV1 {
        access: CandidateSessionAccessV1::new(
            PRODUCT,
            SESSION,
            &h.set.session_lease_id,
            h.set.candidate_set_generation,
        ),
    });
    let hb_resp = dispatch(&h, hb_cmd);
    assert_eq!(hb_resp.status, ToolResponseStatus::Ok);
    let hb = hb_resp
        .result
        .as_ref()
        .and_then(|v| v.get("candidate_session_heartbeat_result"))
        .cloned()
        .expect("heartbeat result key");
    assert_eq!(hb["promoted_count"], 1);
    assert_eq!(hb["rejected_count"], 1);

    // end
    let end_cmd = ToolCommand::CandidateSessionEnd(CandidateSessionEndRequestV1 {
        access: CandidateSessionAccessV1::new(
            PRODUCT,
            SESSION,
            &h.set.session_lease_id,
            h.set.candidate_set_generation,
        ),
    });
    let end_resp = dispatch(&h, end_cmd);
    assert_eq!(end_resp.status, ToolResponseStatus::Ok);
    let end = end_resp
        .result
        .as_ref()
        .and_then(|v| v.get("candidate_session_end_result"))
        .cloned()
        .expect("end result key");
    assert_eq!(end["erased_candidate_count"], 2);
    assert_eq!(end["erased_promoted_disposition_count"], 1);

    // end is erasure-not-rejection: canonical event remains
    assert_eq!(h.events.replay().unwrap().len(), 1);
    assert_eq!(h.service.sessions().active_set_count(), 0);
}

#[test]
fn prepare_without_selection_context_fails_closed() {
    let events = MemoryAdapter::new();
    let request = ToolRequest::new(
        ToolCommand::CandidateSelectionPrepare(CandidateSelectionV1 {
            contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
            selection_operation_id: Uuid::new_v4(),
            candidate_set_id: Uuid::new_v4(),
            candidate_set_generation: 1,
            candidate_set_digest: digest("missing"),
            product_id: PRODUCT.into(),
            host_session_id: SESSION.into(),
            session_lease_id: LEASE.into(),
            promote_candidate_ids: vec![CANDIDATE_A],
            reject_candidate_ids: vec![],
            actor_id: OWNER.into(),
        }),
        authority_ctx(),
    );
    // Pure-runtime entrypoint has no selection context.
    let response = bicameral_runtime::dispatch::dispatch_with_workspace(&request, &events, None);
    assert_eq!(response.status, ToolResponseStatus::Rejected);
    let message = response.message.unwrap_or_default();
    assert!(
        message.contains("unsupported_capability") || message.contains("candidate_selection"),
        "{message}"
    );
}

#[test]
fn unknown_session_returns_typed_failure_object() {
    let h = harness();
    let response = dispatch(
        &h,
        ToolCommand::CandidateSessionHeartbeat(CandidateSessionHeartbeatRequestV1 {
            access: CandidateSessionAccessV1::new(
                PRODUCT,
                "other-session",
                &h.set.session_lease_id,
                1,
            ),
        }),
    );
    assert_eq!(response.status, ToolResponseStatus::Rejected);
    let failure = response
        .result
        .as_ref()
        .and_then(|v| v.get("failure"))
        .cloned()
        .expect("typed failure object for MCP render_failure");
    assert_eq!(failure["code"], "candidate_set_not_found");
}

#[test]
fn selection_does_not_mutate_dispositions_on_prepare_only() {
    let h = harness();
    let prepare_cmd = ToolCommand::CandidateSelectionPrepare(CandidateSelectionV1 {
        contract_version: CANDIDATE_SELECTION_CONTRACT_VERSION,
        selection_operation_id: Uuid::new_v4(),
        candidate_set_id: h.set.candidate_set_id,
        candidate_set_generation: h.set.candidate_set_generation,
        candidate_set_digest: h.set.candidate_set_digest.clone(),
        product_id: PRODUCT.into(),
        host_session_id: SESSION.into(),
        session_lease_id: h.set.session_lease_id.clone(),
        promote_candidate_ids: vec![CANDIDATE_A, CANDIDATE_B],
        reject_candidate_ids: vec![],
        actor_id: OWNER.into(),
    });
    let resp = dispatch(&h, prepare_cmd);
    assert_eq!(resp.status, ToolResponseStatus::Ok);
    assert!(h.events.replay().unwrap().is_empty());
    let set = h
        .service
        .sessions()
        .enumerate(
            &CandidateSessionAccessV1::new(
                PRODUCT,
                SESSION,
                &h.set.session_lease_id,
                h.set.candidate_set_generation,
            ),
            now() + Duration::seconds(1),
        )
        .unwrap();
    assert_eq!(set.count_with(CandidateDisposition::Pending), 2);
}
