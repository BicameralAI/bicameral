//! Binding alpha path conformance — issue #391.
//!
//! Proves the alpha binding lifecycle through the canonical ToolRequest
//! dispatcher path:
//!
//! 1. `binding.create` is usable through the alpha ToolRequest/API path.
//! 2. `binding.inspect` returns accepted binding evidence with stable identity.
//! 3. Binding evidence includes `graph_snapshot_id` and `validated_sha` where
//!    available.
//! 4. Missing graph/snapshot support returns typed state, not silent freshness
//!    or compliance claim.
//! 5. Tests prove signoff and compliance state do not change after binding
//!    operations.
//!
//! The test exercises the full lifecycle:
//!   ingest.submit_local → review.accept_candidate → review.approve_signoff
//!   → binding.create → binding.inspect
//!
//! All through `dispatch_with_workspace` — the canonical alpha ToolRequest
//! dispatcher. This is the release proof that the alpha binding path works
//! end-to-end without modifying governance state.
//!
//! Non-goals:
//!   - Does not implement manual currentness refresh (#377).
//!   - Does not implement GraphStore rebuild lifecycle (#378).
//!   - Does not implement compliance resolution.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::replay;
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

// ─── Helpers ──────────────────────────────────────────────────────────────

fn git(args: &[&str], dir: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn setup_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    git(&["init"], p);
    git(&["config", "user.email", "test@test.com"], p);
    git(&["config", "user.name", "Test"], p);

    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(
        p.join("src/lib.rs"),
        "pub fn process_order(id: u64) -> bool {\n    id > 0\n}\n",
    )
    .unwrap();

    git(&["add", "."], p);
    git(&["commit", "-m", "init: add process_order"], p);

    dir
}

fn authority() -> AuthorityContext {
    bicameral_api::fixtures::authority_mcp("test-agent", "sess-alpha-binding", "/workspace")
}

/// Owner-level authority for signoff actions (requires non-MCP surface).
fn owner_authority() -> AuthorityContext {
    bicameral_api::fixtures::authority_dashboard("test-owner", "/workspace")
}

// ─── Full alpha binding lifecycle ─────────────────────────────────────────

/// Proves all five acceptance criteria for issue #391 through the alpha
/// ToolRequest dispatcher:
///
/// 1. binding.create is usable through the alpha ToolRequest/API path.
/// 2. binding.inspect returns accepted binding evidence with stable identity.
/// 3. Binding evidence includes graph_snapshot_id and validated_sha.
/// 4. Missing graph/snapshot returns typed state (tested separately below).
/// 5. Signoff and compliance state do not change after binding operations.
#[test]
fn alpha_binding_lifecycle_full_proof() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // ── Step 1: Ingest source evidence ────────────────────────────────────
    let ingest_req = ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "jira://PROJ-391".to_string(),
            source_type: "jira".to_string(),
            label: Some("Alpha binding proof".to_string()),
            title: "All order processing must use process_order".to_string(),
            description: "Decision: route all order processing through process_order".to_string(),
            suggested_level: Some("product".to_string()),
            snapshot_content: Some(
                "Meeting 2026-06-15: All order logic must go through process_order.".to_string(),
            ),
            evidence: vec![IngestEvidenceItem {
                excerpt: "All order logic must go through process_order.".to_string(),
                span_start: Some(0),
                span_end: Some(48),
                confidence: Some(0.95),
            }],
            candidate_drafts: vec![],
            binding_hints: vec![BindingHint {
                file: "src/lib.rs".to_string(),
                range: None,
                symbol: Some("process_order".to_string()),
            }],
            rationale: None,
            metadata: None,
        }),
        authority(),
    );

    let ingest_resp = dispatch_with_workspace(&ingest_req, &adapter, Some(dir.path()));
    assert_eq!(
        ingest_resp.status,
        ToolResponseStatus::Ok,
        "ingest must succeed: {:?}",
        ingest_resp.message
    );

    let ingest_result = ingest_resp.result.unwrap();
    let candidate_ids = ingest_result["candidate_ids"]
        .as_array()
        .expect("ingest must return candidate_ids");
    assert!(
        !candidate_ids.is_empty(),
        "must have at least one candidate"
    );
    let candidate_id = candidate_ids[0].as_str().unwrap().to_string();

    // ── Step 2: Accept candidate → Decision(proposed) ─────────────────────
    let accept_req = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id.clone(),
            reason: Some("governance review".to_string()),
        }),
        authority(),
    );

    let accept_resp = dispatch_with_workspace(&accept_req, &adapter, Some(dir.path()));
    assert_eq!(
        accept_resp.status,
        ToolResponseStatus::Ok,
        "accept_candidate must succeed: {:?}",
        accept_resp.message
    );

    // The decision_id is generated internally during accept — retrieve from replay.
    let decision_id = {
        let state = replay::replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.decisions.len(),
            1,
            "exactly one Decision after acceptance"
        );
        let (id, decision) = state.decisions.iter().next().unwrap();
        assert_eq!(
            format!("{:?}", decision.signoff_state),
            "Proposed",
            "Step 2: signoff must be Proposed after acceptance"
        );
        assert_eq!(
            format!("{:?}", decision.compliance_state),
            "Ungrounded",
            "Step 2: compliance must be Ungrounded after acceptance"
        );
        id.to_string()
    };

    // ── Step 3: Approve signoff → Decision(approved) ──────────────────────
    // Signoff requires owner authority (non-MCP surface per ActionAuthorizationPolicy).
    let signoff_req = ToolRequest::new(
        ToolCommand::ReviewApproveSignoff(ReviewTargetParams {
            target_id: decision_id.clone(),
            reason: Some("tech-lead approval".to_string()),
        }),
        owner_authority(),
    );

    let signoff_resp = dispatch_with_workspace(&signoff_req, &adapter, Some(dir.path()));
    assert_eq!(
        signoff_resp.status,
        ToolResponseStatus::Ok,
        "approve_signoff must succeed: {:?}",
        signoff_resp.message
    );

    // Capture signoff/compliance state BEFORE binding.
    let (signoff_before, compliance_before) = {
        let state = replay::replay_from_adapter(&adapter).unwrap();
        let decision_uuid = Uuid::parse_str(&decision_id).unwrap();
        let decision = state.decisions.get(&decision_uuid).unwrap();
        (
            format!("{:?}", decision.signoff_state),
            format!("{:?}", decision.compliance_state),
        )
    };
    assert_eq!(signoff_before, "Approved", "signoff must be Approved");
    assert_eq!(
        compliance_before, "Ungrounded",
        "compliance must remain Ungrounded (binding does not resolve compliance)"
    );

    // ── Step 4: binding.create through ToolRequest dispatcher ─────────────
    //
    // Acceptance criteria #1: binding.create is usable through the alpha
    // ToolRequest/API path.
    let binding_create_req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.clone(),
            bindings: vec![BindingSpec {
                file: "src/lib.rs".to_string(),
                range: Some("1-3".to_string()),
                symbol: None,
                from_discovery: None,
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: None,
        }),
        authority(),
    );

    let bind_resp = dispatch_with_workspace(&binding_create_req, &adapter, Some(dir.path()));
    assert_eq!(
        bind_resp.status,
        ToolResponseStatus::Ok,
        "binding.create must succeed: {:?}",
        bind_resp.message
    );

    let bind_result = bind_resp.result.unwrap();
    assert_eq!(
        bind_result["status"], "bound",
        "binding.create must return status=bound"
    );

    let bound_entries = bind_result["bound"].as_array().unwrap();
    assert_eq!(bound_entries.len(), 1, "exactly one binding created");

    let bound_entry = &bound_entries[0];

    // Acceptance criteria #3: Binding evidence includes graph_snapshot_id
    // and validated_sha where available.
    let graph_snapshot_id = bound_entry["graph_snapshot_id"]
        .as_str()
        .expect("binding must include graph_snapshot_id");
    assert!(
        graph_snapshot_id.starts_with("gs-"),
        "graph_snapshot_id must use gs- prefix: {graph_snapshot_id}"
    );

    let validated_sha = bound_entry["validated_sha"]
        .as_str()
        .expect("binding must include validated_sha");
    assert!(!validated_sha.is_empty(), "validated_sha must be non-empty");

    let binding_id = bound_entry["binding_id"]
        .as_str()
        .expect("binding must include binding_id");
    assert!(!binding_id.is_empty(), "binding_id must be non-empty");

    assert_eq!(
        bound_entry["evidence_state"], "verified",
        "evidence_state must be verified"
    );

    // Also verify top-level response includes graph_snapshot_id and validated_sha.
    assert_eq!(
        bind_result["graph_snapshot_id"].as_str().unwrap(),
        graph_snapshot_id,
        "top-level graph_snapshot_id must match"
    );
    assert_eq!(
        bind_result["validated_sha"].as_str().unwrap(),
        validated_sha,
        "top-level validated_sha must match"
    );

    // Acceptance criteria #5: Signoff and compliance state must NOT change
    // after binding.create.
    {
        let state = replay::replay_from_adapter(&adapter).unwrap();
        let decision_uuid = Uuid::parse_str(&decision_id).unwrap();
        let decision = state.decisions.get(&decision_uuid).unwrap();
        assert_eq!(
            format!("{:?}", decision.signoff_state),
            signoff_before,
            "CRITICAL: signoff_state must not change after binding.create"
        );
        assert_eq!(
            format!("{:?}", decision.compliance_state),
            compliance_before,
            "CRITICAL: compliance_state must not change after binding.create"
        );
    }

    // ── Step 5: binding.inspect through ToolRequest dispatcher ────────────
    //
    // Acceptance criteria #2: binding.inspect returns accepted binding
    // evidence with stable identity.
    let inspect_req = ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: decision_id.clone(),
            commit_sha: Some(validated_sha.to_string()),
            ref_name: None,
            graph_snapshot_id: Some(graph_snapshot_id.to_string()),
        }),
        authority(),
    );

    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(
        inspect_resp.status,
        ToolResponseStatus::Ok,
        "binding.inspect must succeed: {:?}",
        inspect_resp.message
    );

    let inspect_result = inspect_resp.result.unwrap();
    assert_eq!(inspect_result["binding_count"], 1);

    let inspect_bindings = inspect_result["bindings"].as_array().unwrap();
    assert_eq!(inspect_bindings.len(), 1);

    let inspected = &inspect_bindings[0];

    // Stable identity: binding_id matches the one from create.
    assert_eq!(
        inspected["binding_id"].as_str().unwrap(),
        binding_id,
        "binding.inspect must return same binding_id (stable identity)"
    );

    // Evidence fields preserved.
    assert_eq!(
        inspected["graph_snapshot_id"].as_str().unwrap(),
        graph_snapshot_id,
        "inspect must preserve graph_snapshot_id"
    );
    assert_eq!(
        inspected["validated_sha"].as_str().unwrap(),
        validated_sha,
        "inspect must preserve validated_sha"
    );
    assert_eq!(inspected["evidence_state"], "verified");

    // Currentness check with matching SHA should return Current.
    assert_eq!(
        inspected["currentness"], "current",
        "currentness must be Current when commit_sha matches validated_sha"
    );

    // Acceptance criteria #5: Signoff and compliance state must NOT change
    // after binding.inspect.
    {
        let state = replay::replay_from_adapter(&adapter).unwrap();
        let decision_uuid = Uuid::parse_str(&decision_id).unwrap();
        let decision = state.decisions.get(&decision_uuid).unwrap();
        assert_eq!(
            format!("{:?}", decision.signoff_state),
            signoff_before,
            "CRITICAL: signoff_state must not change after binding.inspect"
        );
        assert_eq!(
            format!("{:?}", decision.compliance_state),
            compliance_before,
            "CRITICAL: compliance_state must not change after binding.inspect"
        );
    }
}

// ─── Acceptance criteria #4: Missing graph returns typed state ─────────────

/// When workspace root is not configured (graph unavailable), binding.create
/// must return a typed error state, NOT a silent freshness or compliance claim.
#[test]
fn binding_create_without_workspace_returns_typed_graph_unavailable() {
    let adapter = MemoryAdapter::new();

    let req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: Uuid::new_v4().to_string(),
            bindings: vec![BindingSpec {
                file: "src/lib.rs".to_string(),
                range: None,
                symbol: None,
                from_discovery: None,
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: None,
        }),
        authority(),
    );

    // No workspace → graph unavailable.
    let resp = dispatch_with_workspace(&req, &adapter, None);
    assert_eq!(
        resp.status,
        ToolResponseStatus::Error,
        "must return Error status when graph is unavailable"
    );
    assert!(
        resp.message
            .as_deref()
            .unwrap_or("")
            .contains("graph_unavailable"),
        "must return typed graph_unavailable, not silent claim: {:?}",
        resp.message
    );

    // No events appended — no silent freshness or compliance mutation.
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "graph_unavailable must not append events or mutate state"
    );
}

/// When workspace exists but the target file does not, binding.create must
/// return typed not_found state — not a freshness or compliance claim.
#[test]
fn binding_create_missing_file_returns_typed_not_found() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: Uuid::new_v4().to_string(),
            bindings: vec![BindingSpec {
                file: "nonexistent/file.rs".to_string(),
                range: None,
                symbol: None,
                from_discovery: None,
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: None,
        }),
        authority(),
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");

    let rejected = result["rejected"].as_array().unwrap();
    assert_eq!(rejected.len(), 1);

    let evidence_state = rejected[0]["evidence_state"].as_str().unwrap();
    assert_eq!(
        evidence_state, "not_found",
        "missing file must return typed not_found, not silent compliance claim"
    );

    // No events appended.
    let events = adapter.replay().unwrap();
    assert!(events.is_empty(), "not_found must not append events");
}

/// When graph snapshot ID doesn't match, binding.create must return typed
/// snapshot_mismatch state — not silent freshness claim.
#[test]
fn binding_create_snapshot_mismatch_returns_typed_state() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: Uuid::new_v4().to_string(),
            bindings: vec![BindingSpec {
                file: "src/lib.rs".to_string(),
                range: None,
                symbol: None,
                from_discovery: None,
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: Some("gs-nonexistent-snapshot".to_string()),
            validation_token: None,
        }),
        authority(),
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(
        result["status"], "snapshot_mismatch",
        "mismatched snapshot must return typed snapshot_mismatch"
    );
    assert!(
        result["expected_snapshot_id"].is_string(),
        "must include expected_snapshot_id"
    );
    assert!(
        result["actual_snapshot_id"].is_string(),
        "must include actual_snapshot_id"
    );

    // No events appended — no silent freshness mutation.
    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "snapshot_mismatch must not append events"
    );
}

/// binding.inspect with no prior bindings returns typed empty state —
/// not a freshness or compliance claim.
#[test]
fn binding_inspect_empty_returns_typed_state_not_claim() {
    let adapter = MemoryAdapter::new();

    let req = ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: Uuid::new_v4().to_string(),
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
        }),
        authority(),
    );

    let resp = dispatch_with_workspace(&req, &adapter, None);
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["binding_count"], 0);
    assert!(result["bindings"].as_array().unwrap().is_empty());

    // No events appended — inspect is read-only.
    let events = adapter.replay().unwrap();
    assert!(events.is_empty(), "binding.inspect must not append events");
}

/// binding.inspect returns GraphUnavailable currentness when no commit_sha
/// is provided — typed state, not a freshness claim.
#[test]
fn binding_inspect_without_commit_sha_returns_graph_unavailable_currentness() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    // Create a binding first.
    let decision_id = Uuid::new_v4().to_string();
    let create_req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.clone(),
            bindings: vec![BindingSpec {
                file: "src/lib.rs".to_string(),
                range: None,
                symbol: None,
                from_discovery: None,
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: None,
        }),
        authority(),
    );

    let create_resp = dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));
    assert_eq!(create_resp.status, ToolResponseStatus::Ok);

    // Inspect WITHOUT commit_sha — with workspace available, binding.inspect
    // routes through GraphSnapshotService (issue #438) and evaluates
    // currentness against the live graph state instead of requiring a
    // caller-supplied SHA. The file hasn't changed, so currentness is
    // "current".
    let inspect_req = ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: decision_id,
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
        }),
        authority(),
    );

    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(inspect_resp.status, ToolResponseStatus::Ok);

    let result = inspect_resp.result.unwrap();
    assert_eq!(result["binding_count"], 1);

    let binding = &result["bindings"].as_array().unwrap()[0];
    assert_eq!(
        binding["currentness"], "current",
        "with workspace, service-backed inspect evaluates currentness against \
         the live graph (issue #438)"
    );
}

/// Multiple bindings for the same decision preserve stable identity on inspect.
#[test]
fn multiple_bindings_preserve_stable_identity_on_inspect() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let decision_id = Uuid::new_v4().to_string();

    // Create two bindings.
    let create_req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.clone(),
            bindings: vec![
                BindingSpec {
                    file: "src/lib.rs".to_string(),
                    range: Some("1-1".to_string()),
                    symbol: None,
                    from_discovery: None,
                },
                BindingSpec {
                    file: "src/lib.rs".to_string(),
                    range: Some("2-3".to_string()),
                    symbol: None,
                    from_discovery: None,
                },
            ],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: None,
        }),
        authority(),
    );

    let create_resp = dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));
    assert_eq!(create_resp.status, ToolResponseStatus::Ok);

    let create_result = create_resp.result.unwrap();
    assert_eq!(create_result["status"], "bound");
    let bound = create_result["bound"].as_array().unwrap();
    assert_eq!(bound.len(), 2);

    let id_0 = bound[0]["binding_id"].as_str().unwrap().to_string();
    let id_1 = bound[1]["binding_id"].as_str().unwrap().to_string();
    assert_ne!(id_0, id_1, "each binding must have a unique id");

    // Inspect must return both with same IDs (stable identity).
    let inspect_req = ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: decision_id,
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
        }),
        authority(),
    );

    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(inspect_resp.status, ToolResponseStatus::Ok);

    let inspect_result = inspect_resp.result.unwrap();
    assert_eq!(inspect_result["binding_count"], 2);

    let inspected = inspect_result["bindings"].as_array().unwrap();
    let inspected_ids: Vec<&str> = inspected
        .iter()
        .map(|b| b["binding_id"].as_str().unwrap())
        .collect();

    assert!(
        inspected_ids.contains(&id_0.as_str()),
        "first binding_id must be stable"
    );
    assert!(
        inspected_ids.contains(&id_1.as_str()),
        "second binding_id must be stable"
    );
}
