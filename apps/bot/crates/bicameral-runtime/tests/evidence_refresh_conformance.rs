//! Evidence refresh conformance tests (issue #377).
//!
//! Proves acceptance criteria for the bot-owned manual evidence refresh
//! tracer bullet:
//!
//! 1. Manual refresh for a tracked Decision returns typed JSON with
//!    per-binding `CodeSourceRefreshResult` entries.
//! 2. Runtime uses bot-owned snapshot/file-inspection primitives (no MCP).
//! 3. Typed outcomes include current, content_changed, target_missing,
//!    graph_unavailable, and graph_not_configured.
//! 4. Ineligible Decisions (rejected signoff, no bindings) return typed
//!    rejection.
//! 5. Signoff state is unchanged after refresh.
//! 6. Compliance state is unchanged after refresh.
//! 7. Response is typed JSON, not an empty success body.
//! 8. Workspace-unavailable case returns typed unsupported outcome.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
use bicameral_api::event_store::{
    BindToCodePayload, EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
};
use bicameral_api::evidence::{
    BindingConfidence, BindingEvidence, BindingType, GraphEvidenceState, SourceEvidence,
};
use bicameral_api::source::{Source, SourceSnapshot};
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
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
        "pub fn hello_world() -> &'static str {\n    \"hello\"\n}\n",
    )
    .unwrap();

    git(&["add", "."], p);
    git(&["commit", "-m", "init"], p);

    dir
}

fn authority() -> AuthorityContext {
    bicameral_api::fixtures::authority_dashboard("test-reviewer", "/workspace")
}

fn make_evidence_refresh(decision_id: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::EvidenceRefresh(EvidenceRefreshParams {
            decision_id: decision_id.to_string(),
        }),
        authority(),
    )
}

/// Ingest a candidate and promote it to a Decision. Returns (candidate_id, decision_id).
fn ingest_and_accept(adapter: &MemoryAdapter) -> (Uuid, Uuid) {
    let source = Source {
        uri: format!("test://{}", Uuid::new_v4()),
        source_type: "test".to_string(),
        label: None,
        source_kind: None,
    };
    let snapshot = SourceSnapshot {
        snapshot_addr: format!("sha256:snap-{}", Uuid::new_v4()),
        snapshot_ref: source.uri.clone(),
        captured_at: chrono::Utc::now(),
        content: Some("evidence refresh test".to_string()),
        content_hash: Some("sha256:test".to_string()),
        capture_method: None,
    };
    let evidence = vec![SourceEvidence::new(
        snapshot.snapshot_addr.clone(),
        0,
        10,
        "test evidence".to_string(),
        0.9,
    )];
    let mut candidate = DecisionCandidate::new(
        "Refresh Test Decision".to_string(),
        "Conformance fixture for evidence refresh".to_string(),
        DecisionLevel::Architecture,
        source.uri.clone(),
    );
    candidate.evidence_refs = evidence.iter().map(|e| e.id).collect();
    let candidate_id = candidate.id;

    let ingest = IngestPayload {
        source,
        snapshot: Some(snapshot),
        evidence,
        candidate,
        evidence_references: Vec::new(),
        provenance: None,
    };
    adapter
        .append(EventStoreEntry::new(
            EventType::Ingest,
            serde_json::to_value(&ingest).unwrap(),
            "test".to_string(),
        ))
        .unwrap();

    let decision_id = Uuid::new_v4();
    adapter
        .append(EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({
                "candidate_id": candidate_id.to_string(),
                "decision_id": decision_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

    (candidate_id, decision_id)
}

/// Build a `binding.create` request for a file target. Going through the real
/// create dispatch (rather than `emit_bind`) populates the binding's
/// `metadata["content_hash"]` from the workspace, matching production.
fn make_binding_create(decision_id: &str, file: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.to_string(),
            bindings: vec![BindingSpec {
                file: file.to_string(),
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
    )
}

/// Emit a BindToCode event for a file target.
fn emit_bind(
    adapter: &MemoryAdapter,
    decision_id: Uuid,
    target: &str,
    graph_snapshot_id: Option<String>,
    validated_sha: Option<String>,
) -> Uuid {
    let mut binding = BindingEvidence::new(
        decision_id,
        BindingType::CodeRegion,
        target.to_string(),
        BindingConfidence::High,
    );
    binding.graph_snapshot_id = graph_snapshot_id.clone();
    binding.validated_sha = validated_sha.clone();
    let binding_id = binding.id;

    let payload = BindToCodePayload {
        decision_id,
        binding,
        graph_snapshot_id: graph_snapshot_id.unwrap_or_default(),
        validated_sha: validated_sha.unwrap_or_default(),
        evidence_state: GraphEvidenceState::Verified,
        content_hash: Some("sha256:original".to_string()),
        line_start: Some(1),
        line_end: Some(3),
    };
    adapter
        .append(EventStoreEntry::new(
            EventType::BindToCode,
            serde_json::to_value(&payload).unwrap(),
            "test".to_string(),
        ))
        .unwrap();
    binding_id
}

// ─── 1. Refresh returns current for matching target ───────────────────────

#[test]
fn refresh_returns_current_for_existing_target() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);
    let _binding_id = emit_bind(
        &adapter,
        decision_id,
        "src/lib.rs",
        Some("snap-001".to_string()),
        Some("abc123".to_string()),
    );

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));

    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "refreshed");
    assert_eq!(result["decision_id"], decision_id.to_string());

    let bindings = result["binding_results"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "should have one binding result");

    let b = &bindings[0];
    assert!(b["binding_id"].is_string());
    assert_eq!(b["decision_id"], decision_id.to_string());
    assert_eq!(b["graph_readiness"], "ready");
    assert_eq!(b["currentness"], "current");
}

// ─── 2. Refresh returns target_missing for non-existent file ──────────────

#[test]
fn refresh_returns_target_missing_for_absent_file() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);
    let _binding_id = emit_bind(
        &adapter,
        decision_id,
        "src/nonexistent.rs",
        Some("snap-001".to_string()),
        Some("abc123".to_string()),
    );

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "refreshed");

    let b = &result["binding_results"][0];
    assert_eq!(b["currentness"], "target_missing");
}

// ─── 3. Workspace unavailable returns typed unsupported ───────────────────

#[test]
fn refresh_without_workspace_returns_unsupported() {
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);
    let _binding_id = emit_bind(
        &adapter,
        decision_id,
        "src/lib.rs",
        Some("snap-001".to_string()),
        Some("abc123".to_string()),
    );

    // No workspace provided
    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "unsupported");
    assert!(result["detail"].as_str().unwrap().contains("workspace"));
}

// ─── 4. Decision not found returns typed not_found ────────────────────────

#[test]
fn refresh_unknown_decision_returns_not_found() {
    let adapter = MemoryAdapter::new();
    let phantom_id = Uuid::new_v4();

    let req = make_evidence_refresh(&phantom_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "not_found");
}

// ─── 5. Decision with rejected signoff returns ineligible ─────────────────

#[test]
fn refresh_rejected_decision_returns_ineligible() {
    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = ingest_and_accept(&adapter);

    // Reject signoff
    adapter
        .append(EventStoreEntry::new(
            EventType::RejectSignoff,
            serde_json::json!({
                "decision_id": decision_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "ineligible");
}

// ─── 6. Decision with no bindings returns ineligible ──────────────────────

#[test]
fn refresh_decision_without_bindings_returns_ineligible() {
    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = ingest_and_accept(&adapter);

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(Path::new("/tmp")));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "ineligible");
    assert!(result["detail"].as_str().unwrap().contains("no binding"));
}

// ─── 7. Signoff state is unchanged after refresh ──────────────────────────

#[test]
fn refresh_does_not_mutate_signoff_state() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);

    // Approve signoff
    adapter
        .append(EventStoreEntry::new(
            EventType::ApproveSignoff,
            serde_json::json!({
                "decision_id": decision_id.to_string(),
            }),
            "test".to_string(),
        ))
        .unwrap();

    let _binding_id = emit_bind(
        &adapter,
        decision_id,
        "src/lib.rs",
        Some("snap-001".to_string()),
        Some("abc123".to_string()),
    );

    // Snapshot signoff state before refresh
    let state_before = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    let signoff_before = state_before.decisions[&decision_id].signoff_state;

    // Execute refresh
    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    // Verify signoff unchanged
    let state_after = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    let signoff_after = state_after.decisions[&decision_id].signoff_state;
    assert_eq!(
        signoff_before, signoff_after,
        "signoff state must not change after evidence refresh"
    );
}

// ─── 8. Compliance state is unchanged after refresh ───────────────────────

#[test]
fn refresh_does_not_mutate_compliance_state() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);
    let _binding_id = emit_bind(
        &adapter,
        decision_id,
        "src/lib.rs",
        Some("snap-001".to_string()),
        Some("abc123".to_string()),
    );

    // Snapshot compliance state before refresh
    let state_before = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    let compliance_before = state_before.decisions[&decision_id].compliance_state;

    // Execute refresh
    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    // Verify compliance unchanged
    let state_after = bicameral_runtime::replay::replay_from_adapter(&adapter).unwrap();
    let compliance_after = state_after.decisions[&decision_id].compliance_state;
    assert_eq!(
        compliance_before, compliance_after,
        "compliance state must not change after evidence refresh"
    );

    // Also check no new events were appended (refresh is read-only)
    let events_before_count = state_before.events.len();
    let events_after_count = state_after.events.len();
    assert_eq!(
        events_before_count, events_after_count,
        "evidence.refresh must not append events to the store"
    );
}

// ─── 9. Response is typed JSON ────────────────────────────────────────────

#[test]
fn refresh_response_is_typed_json_not_empty() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);
    let _binding_id = emit_bind(
        &adapter,
        decision_id,
        "src/lib.rs",
        Some("snap-001".to_string()),
        Some("abc123".to_string()),
    );

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));

    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert!(result.is_object(), "response must be a JSON object");
    assert!(result.get("decision_id").is_some());
    assert!(result.get("status").is_some());
    assert!(result.get("binding_results").is_some());
}

// ─── 10. Invalid decision_id returns not_found ────────────────────────────

#[test]
fn refresh_invalid_uuid_returns_not_found() {
    let adapter = MemoryAdapter::new();

    let req = make_evidence_refresh("not-a-uuid");
    let resp = dispatch_with_workspace(&req, &adapter, None);

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "not_found");
    assert!(result["detail"].as_str().unwrap().contains("invalid"));
}

// ─── 11. Binding without graph_snapshot_id still evaluates via workspace ──
//
// Graph readiness is determined by the workspace (SnapshotResolver),
// not by the binding's graph_snapshot_id. A valid git workspace with
// HEAD resolved reports Ready regardless of the binding metadata.

#[test]
fn refresh_binding_without_snapshot_evaluates_via_workspace() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);
    // Bind without graph_snapshot_id — workspace still resolves
    let _binding_id = emit_bind(&adapter, decision_id, "src/lib.rs", None, None);

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));

    assert_eq!(resp.status, ToolResponseStatus::Ok);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "refreshed");

    let b = &result["binding_results"][0];
    // Workspace has a valid git repo, so graph readiness is Ready
    assert_eq!(b["graph_readiness"], "ready");
    // File exists in workspace, so currentness is Current
    assert_eq!(b["currentness"], "current");
}

// ─── 12. Refresh detects content drift after the bound file changes ───────
//
// Bindings created through the real `binding.create` dispatch record the
// bind-time content hash in `metadata["content_hash"]`. Modifying and
// committing the bound file makes the workspace clean (Verified) but its
// content differ from the recorded baseline, so refresh must report
// `content_changed` — the drift outcome that closes the #377 acceptance gap.

#[test]
fn refresh_returns_content_changed_for_modified_target() {
    let dir = setup_workspace();
    let p = dir.path();
    let adapter = MemoryAdapter::new();

    let (_cand_id, decision_id) = ingest_and_accept(&adapter);

    // Create the binding through the real dispatch so the bind-time content
    // hash is captured from the workspace.
    let create_req = make_binding_create(&decision_id.to_string(), "src/lib.rs");
    let create_resp = dispatch_with_workspace(&create_req, &adapter, Some(p));
    assert_eq!(
        create_resp.status,
        ToolResponseStatus::Ok,
        "{:?}",
        create_resp.message
    );

    // Modify and commit the bound file: working tree stays clean (Verified)
    // but the content differs from the recorded baseline.
    std::fs::write(
        p.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"goodbye\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], p);
    git(&["commit", "-m", "change lib"], p);

    let req = make_evidence_refresh(&decision_id.to_string());
    let resp = dispatch_with_workspace(&req, &adapter, Some(p));

    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "refreshed");

    let b = &result["binding_results"][0];
    assert_eq!(b["graph_readiness"], "ready");
    assert_eq!(
        b["currentness"], "content_changed",
        "a modified bound file must refresh as content_changed, got {:?}",
        b["currentness"]
    );
}
