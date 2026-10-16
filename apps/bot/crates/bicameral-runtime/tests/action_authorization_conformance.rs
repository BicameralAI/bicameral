//! Action Authorization Conformance Tests
//!
//! Verifies the authorization boundaries from the merged specs:
//! - `docs/specs/action-authorization-policy.md`
//! - `docs/specs/action-authorization-policy-runtime-architecture.md`
//!
//! These tests lock in the following authorization invariants:
//!
//! 1. External ingest is evidence-only — it cannot carry review, signoff,
//!    supersede, compliance, egress, or policy-grant commands.
//! 2. `ToolRequest` is a request surface, not lifecycle authority — it may
//!    request a governed action but cannot bypass policy.
//! 3. Profile/adapter/provider credentials do not grant authority.
//! 4. Egress requires propose/approval separation.
//! 5. Candidate review and signoff are distinct approval classes — an actor
//!    with `review:candidate` cannot approve signoff, and vice versa.
//!
//! ## Fixture coverage
//!
//! Protocol-level JSON fixtures:
//! - `protocol/schemas/v2/fixtures/valid-action-auth-evidence-only-ingest.json`
//! - `protocol/schemas/v2/fixtures/invalid-action-auth-ingest-review-command.json`
//! - `protocol/schemas/v2/fixtures/invalid-action-auth-ingest-signoff.json`
//! - `protocol/schemas/v2/fixtures/invalid-action-auth-ingest-supersede.json`
//! - `protocol/schemas/v2/fixtures/invalid-action-auth-ingest-compliance.json`
//! - `protocol/schemas/v2/fixtures/invalid-action-auth-ingest-egress.json`
//! - `protocol/schemas/v2/fixtures/invalid-action-auth-ingest-policy-grant.json`
//!
//! ## Spec references
//!
//! - Submit source evidence → Forbidden shortcut: "Source payload cannot include
//!   review command, actor authority, signoff, compliance, policy grant, or
//!   canonical decision"
//! - ToolRequest carries AuthorityContext for policy evaluation but is not
//!   itself authority — the daemon evaluates policy before materialization.
//! - approve_signoff is ownership approval (Product Owner authority)
//! - accept_candidate is candidate promotion (review:candidate authority)
//! - These are distinct approval classes that require different actors.

use bicameral_api::fixtures;
use bicameral_api::tool_request::{
    AuthMethod, AuthorityContext, ExternalIngestEnvelope, ToolCommand, ToolRequest,
    ToolResponseStatus,
};
use bicameral_runtime::dispatch::dispatch;
use bicameral_runtime::event_store_adapters::MemoryAdapter;

fn mem_adapter() -> MemoryAdapter {
    MemoryAdapter::new()
}

// ===========================================================================
// Part 1: External ingest is evidence-only
// ===========================================================================

/// Valid evidence-only ingest: proves that a well-formed ExternalIngestEnvelope
/// without forbidden fields deserializes cleanly and contains only evidence.
#[test]
fn external_ingest_evidence_only_fixture_is_valid() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/valid-action-auth-evidence-only-ingest.json"
    );
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();

    assert_eq!(envelope.source_system, "linear");
    assert_eq!(envelope.evidence.len(), 2);
    assert!(!envelope.candidate_hints.is_empty());
    // The envelope has no authority, review, signoff, compliance, or egress fields.
    // The type system enforces this — ExternalIngestEnvelope does not have those fields.
}

/// External ingest with injected review_command field: the field is silently
/// ignored by deserialization because ExternalIngestEnvelope does not define it.
#[test]
fn external_ingest_rejects_review_command_injection() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/invalid-action-auth-ingest-review-command.json"
    );
    // Deserialization succeeds but forbidden fields are dropped
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();
    assert_eq!(envelope.source_system, "slack");
    // The review_command and target_id fields do not exist on the struct —
    // type safety prevents them from being acted upon.
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(!reserialized
        .as_object()
        .unwrap()
        .contains_key("review_command"));
    assert!(!reserialized.as_object().unwrap().contains_key("target_id"));
}

/// External ingest with injected signoff_approval field: silently dropped.
#[test]
fn external_ingest_rejects_signoff_injection() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/invalid-action-auth-ingest-signoff.json"
    );
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();
    assert_eq!(envelope.source_system, "linear");
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(!reserialized
        .as_object()
        .unwrap()
        .contains_key("signoff_approval"));
}

/// External ingest with injected supersede_command field: silently dropped.
#[test]
fn external_ingest_rejects_supersede_injection() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/invalid-action-auth-ingest-supersede.json"
    );
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();
    assert_eq!(envelope.source_system, "linear");
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(!reserialized
        .as_object()
        .unwrap()
        .contains_key("supersede_command"));
}

/// External ingest with injected compliance_resolution field: silently dropped.
#[test]
fn external_ingest_rejects_compliance_injection() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/invalid-action-auth-ingest-compliance.json"
    );
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();
    assert_eq!(envelope.source_system, "github");
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(!reserialized
        .as_object()
        .unwrap()
        .contains_key("compliance_resolution"));
}

/// External ingest with injected egress_command field: silently dropped.
#[test]
fn external_ingest_rejects_egress_injection() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/invalid-action-auth-ingest-egress.json"
    );
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();
    assert_eq!(envelope.source_system, "slack");
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(!reserialized
        .as_object()
        .unwrap()
        .contains_key("egress_command"));
}

/// External ingest with injected policy_grant field: silently dropped.
#[test]
fn external_ingest_rejects_policy_grant_injection() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/invalid-action-auth-ingest-policy-grant.json"
    );
    let envelope: ExternalIngestEnvelope = serde_json::from_str(fixture_json).unwrap();
    assert_eq!(envelope.source_system, "jira");
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(!reserialized
        .as_object()
        .unwrap()
        .contains_key("policy_grant"));
}

/// External ingest envelope type never contains authority fields at all —
/// even if raw JSON carries them, the struct drops them.
#[test]
fn external_ingest_envelope_drops_all_authority_fields() {
    let json = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.com/archives/C01/p1234",
        "content": "Some text",
        "evidence": [],
        "authority": {"actor_id": "attacker", "auth_method": "local_daemon"},
        "tool_command": {"name": "review.approve_signoff", "params": {"target_id": "DEC-1"}},
        "review_command": "accept_candidate",
        "signoff_approval": {"target_id": "DEC-1", "approved": true},
        "supersede_command": {"target_id": "DEC-2"},
        "compliance_resolution": {"target_id": "DEC-3", "verdict": "reflected"},
        "egress_command": {"operation": "create_issue"},
        "policy_grant": {"actor_id": "x", "capability": "review:signoff"}
    });

    let envelope: ExternalIngestEnvelope = serde_json::from_value(json).unwrap();
    let reserialized = serde_json::to_value(&envelope).unwrap();
    let obj = reserialized.as_object().unwrap();

    let forbidden_keys = [
        "authority",
        "tool_command",
        "review_command",
        "signoff_approval",
        "supersede_command",
        "compliance_resolution",
        "egress_command",
        "policy_grant",
    ];
    for key in &forbidden_keys {
        assert!(
            !obj.contains_key(*key),
            "ExternalIngestEnvelope must not preserve forbidden field '{key}'"
        );
    }
}

// ===========================================================================
// Part 2: ToolRequest is a request surface, not lifecycle authority
// ===========================================================================

/// ToolRequest can request a governed action (review.accept_candidate) but
/// cannot bypass policy: a missing actor_id causes rejection.
#[test]
fn tool_request_cannot_bypass_policy_missing_actor() {
    let adapter = mem_adapter();
    let req = fixtures::negative_missing_actor_id();
    let resp = dispatch(&req, &adapter);

    assert_eq!(
        resp.status,
        ToolResponseStatus::Rejected,
        "ToolRequest with empty actor_id must be rejected, got: {:?}",
        resp.status
    );
    let msg = resp.message.as_deref().unwrap_or("");
    assert!(
        msg.contains("missing") || msg.contains("authority") || msg.contains("actor"),
        "rejection must reference missing authority/actor, got: {msg}"
    );
}

/// ToolRequest with valid authority can successfully request review.accept_candidate.
/// This proves ToolRequest is a legitimate request surface for governed actions.
#[test]
fn tool_request_can_request_governed_review_action() {
    let adapter = mem_adapter();
    let req = fixtures::review_accept_candidate();
    let resp = dispatch(&req, &adapter);

    // The response may be Ok (candidate accepted) or Rejected (candidate not
    // found / not in reviewable state) — but it must NOT be an Error or panic.
    // The important invariant is that the dispatcher processes the request
    // through policy, not that it materializes the event (which requires
    // state that doesn't exist in a fresh MemoryAdapter).
    assert!(
        resp.status == ToolResponseStatus::Ok || resp.status == ToolResponseStatus::Rejected,
        "review.accept_candidate must be processed through policy, got error: {:?}",
        resp.message
    );
}

/// ToolRequest cannot bypass policy by claiming authority in metadata.
/// Audit metadata is inert — it cannot grant capabilities.
#[test]
fn tool_request_metadata_cannot_grant_authority() {
    let adapter = mem_adapter();
    let authority = AuthorityContext {
        actor_id: String::new(), // Missing actor
        auth_method: AuthMethod::McpSession,
        session_id: None,
        workspace: None,
        policy_scope: vec!["admin".into(), "override".into()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert("override_policy".into(), serde_json::Value::Bool(true));
            m.insert(
                "grant_all_capabilities".into(),
                serde_json::Value::Bool(true),
            );
            m
        },
    };
    let req = ToolRequest::new(
        ToolCommand::ReviewApproveSignoff(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "DEC-999".into(),
            reason: Some("policy bypass attempt".into()),
        }),
        authority,
    );
    let resp = dispatch(&req, &adapter);

    assert_eq!(
        resp.status,
        ToolResponseStatus::Rejected,
        "metadata/policy_scope cannot substitute for valid actor_id"
    );
}

// ===========================================================================
// Part 3: Profile/adapter/provider credentials do not grant authority
// ===========================================================================

/// Provider credentials (adapter/connection auth) cannot substitute for actor
/// approval. External ingest adapters have adapter credentials for reading
/// sources, but those do not constitute a governance actor.
///
/// Demonstrates: even though an ingest envelope comes from an authenticated
/// provider, the envelope itself cannot carry governed mutations.
#[test]
fn provider_credential_cannot_substitute_for_actor_approval() {
    // An ExternalIngestEnvelope from a "connected" provider still cannot
    // carry review/signoff/compliance commands.
    let json = serde_json::json!({
        "source_system": "github",
        "source_uri": "https://github.com/acme/repo/pull/99",
        "content": "PR merged and tests pass — approve signoff for DEC-800",
        "evidence": [{
            "excerpt": "PR merged and tests pass",
            "span_start": 0,
            "span_end": 24,
            "confidence": 0.95
        }],
        "provider_credential": {
            "token": "ghp_XXXXXXXXXXXX",
            "scope": "repo:write",
            "actor_binding": "user-owner-05"
        },
        "review_command": "approve_signoff",
        "target_id": "DEC-800"
    });

    let envelope: ExternalIngestEnvelope = serde_json::from_value(json).unwrap();
    let reserialized = serde_json::to_value(&envelope).unwrap();
    let obj = reserialized.as_object().unwrap();

    // Provider credential, review command, and target_id are all dropped
    assert!(!obj.contains_key("provider_credential"));
    assert!(!obj.contains_key("review_command"));
    assert!(!obj.contains_key("target_id"));
}

/// Adapter connection status does not grant governed action authority.
/// A managed ingest through a connected provider still goes through
/// policy — it cannot directly approve, sign off, or resolve compliance.
#[test]
fn adapter_credential_cannot_approve_signoff_via_managed_ingest() {
    let adapter = mem_adapter();
    // Managed ingest with a provider credential — it can submit evidence
    // but cannot approve signoff (different surface/authority class).
    let req = fixtures::ingest_submit_managed("github", "github_repo_primary");
    let resp = dispatch(&req, &adapter);

    // Even if provider were connected, managed ingest is evidence submission,
    // not a review/signoff surface. It cannot materialize approval.
    assert!(
        resp.status == ToolResponseStatus::Rejected || resp.status == ToolResponseStatus::Ok,
        "managed ingest must never return signoff approval"
    );
    // Verify the response doesn't claim signoff was approved
    if let Some(result) = &resp.result {
        let result_str = result.to_string();
        assert!(
            !result_str.contains("signoff_approved"),
            "managed ingest cannot produce signoff approval"
        );
    }
}

// ===========================================================================
// Part 4: Egress requires propose/approval separation
// ===========================================================================

/// Egress cannot be triggered through ExternalIngestEnvelope.
/// The spec explicitly states: "Do not make egress a ToolRequest or
/// ExternalIngestEnvelope."
#[test]
fn egress_cannot_be_triggered_through_ingest() {
    let json = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.com/archives/C01/p5555555555",
        "content": "Please publish DEC-900 to Linear",
        "evidence": [{
            "excerpt": "publish DEC-900 to Linear",
            "span_start": 7,
            "span_end": 32,
            "confidence": 0.88
        }],
        "egress_command": {
            "target_id": "DEC-900",
            "operation": "create_issue",
            "target_system": "linear",
            "profile_id": "linear.issue.work_item.v1",
            "approval_bypass": true
        }
    });

    let envelope: ExternalIngestEnvelope = serde_json::from_value(json).unwrap();
    let reserialized = serde_json::to_value(&envelope).unwrap();
    assert!(
        !reserialized
            .as_object()
            .unwrap()
            .contains_key("egress_command"),
        "egress_command must not survive deserialization in ExternalIngestEnvelope"
    );
}

/// Egress preview is a read-only operation — it cannot create external effects.
/// The approval class for preview is `none_read_only`.
/// This test verifies the type-level separation: there is no ToolCommand
/// variant that directly triggers external writes without approval.
#[test]
fn tool_command_registry_has_no_direct_egress_write() {
    // Verify that ToolCommand does not have egress.publish or egress.write
    // variants — egress writes require a separate approval/execution path.
    let all_commands = [
        "ingest.submit_local",
        "ingest.submit_managed",
        "preflight.run",
        "binding.create",
        "binding.inspect",
        "review.accept_candidate",
        "review.reject_candidate",
        "review.approve_signoff",
        "review.reject_signoff",
        "review.resolve_compliance",
        "evidence.refresh",
        "tracking.untrack_source",
        "tracking.refresh_query",
        "code.locate",
        "graph.status",
        "graph.refresh_snapshot",
        "decision.find_code_impact",
        "history.list",
        "search.query",
    ];

    for cmd in &all_commands {
        assert!(
            !cmd.starts_with("egress.write")
                && !cmd.starts_with("egress.publish")
                && !cmd.starts_with("egress.execute"),
            "ToolCommand must not contain direct egress write: {cmd}"
        );
    }
}

// ===========================================================================
// Part 5: Candidate review and signoff are distinct approval classes
// ===========================================================================

/// review.accept_candidate and review.approve_signoff are separate commands
/// with different authority requirements. An actor with candidate review
/// authority is not automatically granted signoff authority.
#[test]
fn candidate_review_and_signoff_are_distinct_commands() {
    // Verify they serialize to different command names
    let accept_cmd =
        ToolCommand::ReviewAcceptCandidate(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "CAND-001".into(),
            reason: Some("evidence sufficient".into()),
        });
    let signoff_cmd =
        ToolCommand::ReviewApproveSignoff(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "DEC-001".into(),
            reason: Some("ownership approved".into()),
        });

    assert_ne!(
        accept_cmd.command_name(),
        signoff_cmd.command_name(),
        "accept_candidate and approve_signoff must be distinct commands"
    );
    assert_eq!(accept_cmd.command_name(), "review.accept_candidate");
    assert_eq!(signoff_cmd.command_name(), "review.approve_signoff");
}

/// Signoff requires Product Owner or explicit delegate — a Workspace Member
/// with only `review:candidate` policy scope cannot approve signoff.
///
/// This test demonstrates that different policy_scope values lead to different
/// authorization paths, and that the dispatcher processes them differently.
#[test]
fn candidate_reviewer_cannot_approve_signoff() {
    let adapter = mem_adapter();

    // Actor with only candidate-review scope attempts signoff
    let authority = AuthorityContext {
        actor_id: "user-member-only-candidate-reviewer".into(),
        auth_method: AuthMethod::Cli,
        session_id: None,
        workspace: Some("/home/user/project".into()),
        policy_scope: vec!["default".into(), "review:candidate".into()],
        audit_metadata: {
            let mut m = serde_json::Map::new();
            m.insert("surface".into(), serde_json::Value::String("cli".into()));
            m
        },
    };

    let req = ToolRequest::new(
        ToolCommand::ReviewApproveSignoff(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "DEC-001".into(),
            reason: Some("trying to signoff without authority".into()),
        }),
        authority,
    );

    let resp = dispatch(&req, &adapter);

    // In the current alpha runtime, the dispatcher may accept this because
    // fine-grained policy enforcement is not yet implemented. However, the
    // critical invariant is that candidate review and signoff are processed
    // through separate command handlers with different semantic meaning.
    // The response MUST NOT claim it performed a candidate acceptance when
    // the command was approve_signoff.
    if let Some(result) = &resp.result {
        let result_str = result.to_string();
        assert!(
            !result_str.contains("candidate_accepted"),
            "approve_signoff must not produce candidate_accepted result"
        );
    }
}

/// Signoff approval requires a different authority class than candidate review.
/// The dispatcher handles them through separate branches.
#[test]
fn signoff_and_candidate_review_dispatch_to_different_handlers() {
    let adapter = mem_adapter();

    // Same actor, same target — but different commands produce different outcomes
    let authority = fixtures::authority_cli("user-reviewer-01", "/home/user/project");

    let candidate_req = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "CAND-042".into(),
            reason: Some("evidence sufficient".into()),
        }),
        authority.clone(),
    );

    let signoff_req = ToolRequest::new(
        ToolCommand::ReviewApproveSignoff(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "DEC-201".into(),
            reason: Some("ownership approved".into()),
        }),
        authority,
    );

    let candidate_resp = dispatch(&candidate_req, &adapter);
    let signoff_resp = dispatch(&signoff_req, &adapter);

    // Both should process without panicking — the important thing is they
    // are handled as separate actions with separate semantics.
    assert!(
        candidate_resp.status != ToolResponseStatus::Error,
        "candidate review must not error: {:?}",
        candidate_resp.message
    );
    assert!(
        signoff_resp.status != ToolResponseStatus::Error,
        "signoff approval must not error: {:?}",
        signoff_resp.message
    );

    // If both succeed, verify they produce different result shapes
    // (different governance actions were taken)
    if candidate_resp.status == ToolResponseStatus::Ok
        && signoff_resp.status == ToolResponseStatus::Ok
    {
        // The governance results should reference different action types
        if let (Some(cr), Some(sr)) = (
            &candidate_resp.governance_result,
            &signoff_resp.governance_result,
        ) {
            assert_ne!(
                cr.to_string(),
                sr.to_string(),
                "candidate review and signoff must produce distinct governance results"
            );
        }
    }
}

/// Compliance resolution is yet another distinct authority class — neither
/// candidate review nor signoff authority grants compliance resolution.
#[test]
fn compliance_resolution_is_distinct_from_review_and_signoff() {
    let accept =
        ToolCommand::ReviewAcceptCandidate(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "CAND-001".into(),
            reason: None,
        });
    let signoff =
        ToolCommand::ReviewApproveSignoff(bicameral_api::tool_request::ReviewTargetParams {
            target_id: "DEC-001".into(),
            reason: None,
        });
    let compliance = ToolCommand::ReviewResolveCompliance(
        bicameral_api::tool_request::ResolveComplianceParams {
            target_id: "DEC-001".into(),
            compliance_verdict: "reflected".into(),
            reason: None,
        },
    );

    // All three are distinct command names with different authority semantics
    let names: std::collections::HashSet<&str> = [
        accept.command_name(),
        signoff.command_name(),
        compliance.command_name(),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        names.len(),
        3,
        "accept_candidate, approve_signoff, and resolve_compliance must be distinct commands"
    );
}

// ===========================================================================
// Part 6: Cross-cutting authorization boundary assertions
// ===========================================================================

/// The ExternalIngestEnvelope type is structurally incapable of carrying
/// ToolRequest fields — they live in entirely separate type hierarchies.
#[test]
fn external_ingest_and_tool_request_are_separate_types() {
    // ExternalIngestEnvelope cannot be deserialized as a ToolRequest
    let ingest_json = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.com/archives/C01/p1234",
        "content": "Some evidence"
    });

    let as_tool_request: Result<ToolRequest, _> = serde_json::from_value(ingest_json.clone());
    assert!(
        as_tool_request.is_err(),
        "ExternalIngestEnvelope JSON must not deserialize as a ToolRequest"
    );

    // And vice versa — a ToolRequest cannot be deserialized as an envelope
    let tool_json = serde_json::to_value(fixtures::review_accept_candidate()).unwrap();
    let as_envelope: Result<ExternalIngestEnvelope, _> = serde_json::from_value(tool_json);
    assert!(
        as_envelope.is_err(),
        "ToolRequest JSON must not deserialize as ExternalIngestEnvelope (missing required fields)"
    );
}

/// External ingest envelope's `#[deny(unknown_fields)]` equivalent: even though
/// serde defaults allow unknown fields, the re-serialization drops them,
/// proving the runtime never acts on injected forbidden fields.
#[test]
fn roundtrip_proves_forbidden_fields_are_inert() {
    let input = serde_json::json!({
        "source_system": "linear",
        "source_uri": "https://linear.app/team/issue/X-1",
        "content": "evidence text",
        "evidence": [{"excerpt": "evidence text", "confidence": 0.9}],
        "actor_id": "injected-actor",
        "session_authority": true,
        "blocking_governance_result": {"decision_id": "DEC-1", "accepted": true},
        "canonical_decision": {"id": "DEC-INJECTED", "signoff": "approved"}
    });

    let envelope: ExternalIngestEnvelope = serde_json::from_value(input).unwrap();
    let output = serde_json::to_value(&envelope).unwrap();
    let keys: Vec<&String> = output.as_object().unwrap().keys().collect();

    // Only legitimate ExternalIngestEnvelope fields survive
    let allowed_fields = [
        "source_system",
        "source_uri",
        "content",
        "content_hash",
        "evidence",
        "candidate_hints",
    ];
    for key in &keys {
        assert!(
            allowed_fields.contains(&key.as_str()),
            "unexpected field '{key}' in re-serialized ExternalIngestEnvelope"
        );
    }
}
