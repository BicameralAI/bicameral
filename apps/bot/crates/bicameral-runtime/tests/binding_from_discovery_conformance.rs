//! Binding-from-discovery conformance tests (issue #446).
//!
//! Proves Stage 2 acceptance criteria for explicit binding acceptance from
//! `decision.find_code_impact` discovery results:
//!
//! 1. `binding.create` accepts a verified discovery candidate and creates
//!    accepted `BindingEvidence`.
//! 2. Approximate, ambiguous, stale, unindexed, unsupported, and not-found
//!    discovery candidates are rejected before re-verification.
//! 3. Confidence/ranking never upgrades a non-verified candidate into a
//!    bindable candidate.
//! 4. Discovery snapshot mismatch is rejected with a typed response.
//! 5. Accepted bindings from discovery can be inspected through
//!    `binding.inspect`.
//! 6. `from_discovery` is optional — direct `binding.create` without
//!    discovery remains backward compatible.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::event_store::EventStoreAdapter;
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
    std::fs::write(
        p.join("src/main.rs"),
        "fn main() {\n    println!(\"hi\");\n}\n",
    )
    .unwrap();

    git(&["add", "."], p);
    git(&["commit", "-m", "init"], p);

    dir
}

fn authority() -> AuthorityContext {
    bicameral_api::fixtures::authority_mcp("test-agent", "sess-test", "/workspace")
}

/// Resolve the current graph snapshot id for the workspace.
fn resolve_snapshot_id(dir: &Path) -> String {
    use bicameral_runtime::grounding::SnapshotResolver;
    let resolver = SnapshotResolver::new(dir);
    let snap = resolver.resolve_head().unwrap();
    snap.graph_snapshot_id
}

fn make_binding_create_from_discovery(
    decision_id: &str,
    file: &str,
    discovery: DiscoverySource,
) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: decision_id.to_string(),
            bindings: vec![BindingSpec {
                file: file.to_string(),
                range: None,
                symbol: None,
                from_discovery: Some(discovery),
            }],
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            validation_token: None,
        }),
        authority(),
    )
}

fn make_binding_inspect(decision_id: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::BindingInspect(BindingInspectParams {
            decision_or_candidate_id: decision_id.to_string(),
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
        }),
        authority(),
    )
}

// ─── 1. Verified discovery candidate creates BindingEvidence ──────────────

#[test]
fn verified_discovery_candidate_creates_binding_evidence() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();
    let snap_id = resolve_snapshot_id(dir.path());

    let decision_id = Uuid::new_v4().to_string();
    let req = make_binding_create_from_discovery(
        &decision_id,
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::Verified,
            graph_snapshot_id: Some(snap_id.clone()),
            validated_sha: None,
            content_hash: None,
            candidate_count: Some(1),
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");

    let bound = &result["bound"];
    assert_eq!(bound.as_array().unwrap().len(), 1);
    assert_eq!(bound[0]["evidence_state"], "verified");
    assert!(bound[0]["binding_id"].is_string());
    assert!(bound[0]["graph_snapshot_id"].is_string());

    let events = adapter.replay().unwrap();
    assert_eq!(
        events.len(),
        1,
        "should append exactly one BindToCode event"
    );
}

// ─── 2. Approximate candidate is rejected ─────────────────────────────────

#[test]
fn approximate_candidate_rejected_from_discovery() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();
    let snap_id = resolve_snapshot_id(dir.path());

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::ApproximateCandidate,
            graph_snapshot_id: Some(snap_id),
            validated_sha: None,
            content_hash: None,
            candidate_count: Some(1),
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");

    let rejected = &result["rejected"];
    assert_eq!(rejected.as_array().unwrap().len(), 1);
    assert_eq!(
        rejected[0]["evidence_state"], "approximate_candidate",
        "approximate candidates must not produce BindingEvidence"
    );
    assert_eq!(rejected[0]["source"], "discovery_gate");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty(), "rejected binding must not append events");
}

// ─── 3. Ambiguous candidate is rejected ───────────────────────────────────

#[test]
fn ambiguous_candidate_rejected_from_discovery() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();
    let snap_id = resolve_snapshot_id(dir.path());

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::Ambiguous,
            graph_snapshot_id: Some(snap_id),
            validated_sha: None,
            content_hash: None,
            candidate_count: Some(3),
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "ambiguous");
    assert_eq!(result["rejected"][0]["source"], "discovery_gate");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 4. Stale candidate is rejected ───────────────────────────────────────

#[test]
fn stale_candidate_rejected_from_discovery() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::UnknownStale,
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            candidate_count: None,
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "unknown_stale");
    assert_eq!(result["rejected"][0]["source"], "discovery_gate");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 5. Unindexed candidate is rejected ───────────────────────────────────

#[test]
fn unindexed_candidate_rejected_from_discovery() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::UnknownNotIndexed,
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            candidate_count: None,
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(
        result["rejected"][0]["evidence_state"],
        "unknown_not_indexed"
    );

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 6. Unsupported candidate is rejected ─────────────────────────────────

#[test]
fn unsupported_candidate_rejected_from_discovery() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::Unsupported,
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            candidate_count: None,
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "unsupported");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 7. Not-found candidate is rejected ───────────────────────────────────

#[test]
fn not_found_candidate_rejected_from_discovery() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::NotFound,
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            candidate_count: None,
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["rejected"][0]["evidence_state"], "not_found");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 8. Discovery snapshot mismatch is rejected ───────────────────────────

#[test]
fn discovery_snapshot_mismatch_rejected() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::Verified,
            graph_snapshot_id: Some("stale-discovery-snapshot".to_string()),
            validated_sha: None,
            content_hash: None,
            candidate_count: Some(1),
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "rejected");

    let rejected = &result["rejected"][0];
    assert_eq!(rejected["evidence_state"], "snapshot_mismatch");
    assert_eq!(
        rejected["discovery_snapshot_id"],
        "stale-discovery-snapshot"
    );
    assert!(rejected["resolved_snapshot_id"].is_string());
    assert_eq!(rejected["source"], "discovery_gate");

    let events = adapter.replay().unwrap();
    assert!(events.is_empty());
}

// ─── 9. Confidence never upgrades non-verified candidate ──────────────────

#[test]
fn confidence_never_upgrades_non_verified_to_bindable() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();
    let snap_id = resolve_snapshot_id(dir.path());

    // Even though the file exists and would pass graph re-verification,
    // an approximate discovery candidate must be rejected.
    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::ApproximateCandidate,
            graph_snapshot_id: Some(snap_id),
            validated_sha: None,
            content_hash: None,
            candidate_count: Some(1),
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok);

    let result = resp.result.unwrap();
    assert_eq!(
        result["status"], "rejected",
        "approximate discovery candidate must be rejected even when \
         the file exists and would pass graph re-verification"
    );

    let events = adapter.replay().unwrap();
    assert!(
        events.is_empty(),
        "confidence/ranking must never upgrade non-verified candidate"
    );
}

// ─── 10. Accepted binding from discovery can be inspected ─────────────────

#[test]
fn accepted_binding_from_discovery_inspectable() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();
    let snap_id = resolve_snapshot_id(dir.path());

    let decision_id = Uuid::new_v4().to_string();
    let create_req = make_binding_create_from_discovery(
        &decision_id,
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::Verified,
            graph_snapshot_id: Some(snap_id),
            validated_sha: None,
            content_hash: None,
            candidate_count: Some(1),
        },
    );

    let create_resp = dispatch_with_workspace(&create_req, &adapter, Some(dir.path()));
    assert_eq!(create_resp.status, ToolResponseStatus::Ok);
    assert_eq!(create_resp.result.as_ref().unwrap()["status"], "bound");

    // Inspect the binding through binding.inspect.
    let inspect_req = make_binding_inspect(&decision_id);
    let inspect_resp = dispatch_with_workspace(&inspect_req, &adapter, Some(dir.path()));
    assert_eq!(
        inspect_resp.status,
        ToolResponseStatus::Ok,
        "{:?}",
        inspect_resp.message
    );

    let inspect_result = inspect_resp.result.unwrap();
    assert_eq!(inspect_result["binding_count"], 1);

    let bindings = inspect_result["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    assert!(bindings[0]["binding_id"].is_string());
    assert_eq!(bindings[0]["evidence_state"], "verified");
}

// ─── 11. Direct binding.create without discovery remains compatible ────────

#[test]
fn direct_binding_create_without_discovery_still_works() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let decision_id = Uuid::new_v4().to_string();
    let req = ToolRequest::new(
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

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");

    let events = adapter.replay().unwrap();
    assert_eq!(events.len(), 1);
}

// ─── 12. Mixed verified/non-verified from discovery ───────────────────────

#[test]
fn mixed_discovery_candidates_produce_partial_status() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();
    let snap_id = resolve_snapshot_id(dir.path());

    let req = ToolRequest::new(
        ToolCommand::BindingCreate(BindingCreateParams {
            decision_or_candidate_id: Uuid::new_v4().to_string(),
            bindings: vec![
                BindingSpec {
                    file: "src/lib.rs".to_string(),
                    range: None,
                    symbol: None,
                    from_discovery: Some(DiscoverySource {
                        evidence_state: LocatorEvidenceState::Verified,
                        graph_snapshot_id: Some(snap_id.clone()),
                        validated_sha: None,
                        content_hash: None,
                        candidate_count: Some(2),
                    }),
                },
                BindingSpec {
                    file: "src/main.rs".to_string(),
                    range: None,
                    symbol: None,
                    from_discovery: Some(DiscoverySource {
                        evidence_state: LocatorEvidenceState::ApproximateCandidate,
                        graph_snapshot_id: Some(snap_id),
                        validated_sha: None,
                        content_hash: None,
                        candidate_count: Some(2),
                    }),
                },
            ],
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
    assert_eq!(result["status"], "partial");
    assert_eq!(result["bound"].as_array().unwrap().len(), 1);
    assert_eq!(result["rejected"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["rejected"][0]["evidence_state"],
        "approximate_candidate"
    );
}

// ─── 13. All non-verified states are exhaustively rejected ────────────────

#[test]
fn all_non_verified_discovery_states_rejected() {
    let non_verified = [
        (LocatorEvidenceState::NotFound, "not_found"),
        (
            LocatorEvidenceState::UnknownNotIndexed,
            "unknown_not_indexed",
        ),
        (LocatorEvidenceState::UnknownStale, "unknown_stale"),
        (LocatorEvidenceState::Ambiguous, "ambiguous"),
        (LocatorEvidenceState::Unsupported, "unsupported"),
        (
            LocatorEvidenceState::ApproximateCandidate,
            "approximate_candidate",
        ),
    ];

    for (state, expected_json) in &non_verified {
        let dir = setup_workspace();
        let adapter = MemoryAdapter::new();

        let req = make_binding_create_from_discovery(
            &Uuid::new_v4().to_string(),
            "src/lib.rs",
            DiscoverySource {
                evidence_state: *state,
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
                candidate_count: None,
            },
        );

        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let result = resp.result.unwrap();
        assert_eq!(
            result["status"], "rejected",
            "discovery state {:?} must be rejected",
            state
        );
        assert_eq!(
            result["rejected"][0]["evidence_state"]
                .as_str()
                .unwrap_or(""),
            *expected_json,
            "evidence_state mismatch for {:?}",
            state
        );

        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "non-verified state {:?} must not append events",
            state
        );
    }
}

// ─── 14. Discovery with no snapshot_id passes through to re-verification ──

#[test]
fn verified_discovery_without_snapshot_id_still_binds() {
    let dir = setup_workspace();
    let adapter = MemoryAdapter::new();

    let req = make_binding_create_from_discovery(
        &Uuid::new_v4().to_string(),
        "src/lib.rs",
        DiscoverySource {
            evidence_state: LocatorEvidenceState::Verified,
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            candidate_count: None,
        },
    );

    let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
    assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);

    let result = resp.result.unwrap();
    assert_eq!(result["status"], "bound");

    let events = adapter.replay().unwrap();
    assert_eq!(events.len(), 1);
}
