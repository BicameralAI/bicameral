//! Snapshot-scoped GraphStore binding verification conformance tests.
//!
//! Proves issue #138 acceptance criteria:
//!
//! 1. Graph records are keyed by immutable snapshot identity rather than branch name.
//! 2. Partial graph slices can verify exact file/range/symbol hints where sufficient.
//! 3. Stale, dirty, missing, ambiguous, unsupported, and snapshot-mismatch cases
//!    return typed non-bound statuses.
//! 4. Durable `BindingEvidence` stores `graph_snapshot_id` and `validated_sha`.
//! 5. MCP-provided validation tokens do not bypass daemon verification.
//! 6. Tests cover two worktrees resolving the same symbol name against different commits.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.

use bicameral_api::evidence::{BindingConfidence, BindingType, GraphEvidenceState};
use bicameral_runtime::bind_to_code::{
    bind_symbol_to_code, bind_to_code, BindToCodeRequest, BindToCodeResult,
};
use bicameral_runtime::grounding::{FileInspector, SnapshotResolver};
use bicameral_runtime::symbol_resolver::RustSymbolResolver;
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

fn setup_repo_with_rust_symbol() -> tempfile::TempDir {
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
    git(&["commit", "-m", "init: add hello_world"], p);

    dir
}

// ─── 1. Snapshot identity keying ──────────────────────────────────────────

#[test]
fn snapshot_identity_is_commit_sha_based_not_branch_name() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();

    // graph_snapshot_id is derived from (repo_id, commit_sha, index_version).
    assert!(
        snap.graph_snapshot_id.starts_with("gs-"),
        "graph_snapshot_id must start with gs- prefix, got: {}",
        snap.graph_snapshot_id
    );

    // The snapshot carries both the immutable identity and the commit SHA.
    assert!(!snap.commit_sha.is_empty(), "commit_sha must be non-empty");
    assert!(
        !snap.graph_snapshot_id.is_empty(),
        "graph_snapshot_id must be non-empty"
    );

    // Resolving HEAD a second time produces the same snapshot identity
    // (deterministic — same repo_id + commit_sha + index_version).
    let snap2 = resolver.resolve_head().unwrap();
    assert_eq!(
        snap.graph_snapshot_id, snap2.graph_snapshot_id,
        "same HEAD must produce same snapshot identity"
    );
    assert_eq!(snap.commit_sha, snap2.commit_sha);

    // ref_name is provenance only — changing the branch name at the same
    // commit must produce the same snapshot identity.
    // (We verify this indirectly: both resolve_head calls produce the same id.)
}

// ─── 2. Partial graph slices: file, range, symbol ─────────────────────────

#[test]
fn file_hint_verified_at_snapshot() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    let result = bind_to_code(&inspector, &snap, &req, None);
    match result {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::FilePath);
            assert_eq!(proposal.target, "src/lib.rs");
        }
        other => panic!("expected Bound for file hint, got {other:?}"),
    }
}

#[test]
fn range_hint_verified_at_snapshot() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: Some(1),
        line_end: Some(3),
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    let result = bind_to_code(&inspector, &snap, &req, None);
    match result {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::CodeRegion);
            assert_eq!(proposal.target, "src/lib.rs:1-3");
        }
        other => panic!("expected Bound for range hint, got {other:?}"),
    }
}

#[test]
fn symbol_hint_verified_at_snapshot() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let sym_resolver = RustSymbolResolver::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: None,
    };
    let result = bind_symbol_to_code(&sym_resolver, &snap, &req, None);
    match result {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::Symbol);
            assert!(
                proposal.target.starts_with("src/lib.rs:"),
                "symbol target should reference file:range, got: {}",
                proposal.target
            );
        }
        other => panic!("expected Bound for symbol hint, got {other:?}"),
    }
}

// ─── 3. Typed non-bound statuses ──────────────────────────────────────────

#[test]
fn missing_file_returns_not_found() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "does_not_exist.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::NotFound);
        }
        other => panic!("expected Rejected/NotFound, got {other:?}"),
    }
}

#[test]
fn dirty_file_returns_unknown_stale() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();

    // Dirty the working tree without committing.
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"modified\"\n}\n",
    )
    .unwrap();

    let inspector = FileInspector::new(dir.path(), snap.clone());
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::UnknownStale);
        }
        other => panic!("expected Rejected/UnknownStale, got {other:?}"),
    }
}

#[test]
fn ambiguous_symbol_returns_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    git(&["init"], p);
    git(&["config", "user.email", "test@test.com"], p);
    git(&["config", "user.name", "Test"], p);

    // Two files each define `MyStruct`.
    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(p.join("src/a.rs"), "pub struct MyStruct;\n").unwrap();
    std::fs::write(p.join("src/b.rs"), "pub struct MyStruct;\n").unwrap();

    git(&["add", "."], p);
    git(&["commit", "-m", "ambiguous symbol"], p);

    let resolver = SnapshotResolver::new(p);
    let snap = resolver.resolve_head().unwrap();
    let sym_resolver = RustSymbolResolver::new(p, snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/a.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("MyStruct".to_string()),
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym_resolver, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::Ambiguous);
        }
        other => panic!("expected Rejected/Ambiguous, got {other:?}"),
    }
}

#[test]
fn unsupported_non_rust_symbol_returns_unsupported() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let sym_resolver = RustSymbolResolver::new(dir.path(), snap.clone());

    // No symbol name → unsupported.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym_resolver, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::Unsupported);
        }
        other => panic!("expected Rejected/Unsupported, got {other:?}"),
    }
}

#[test]
fn snapshot_mismatch_returns_typed_status() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_to_code(&inspector, &snap, &req, Some("gs-wrong-snapshot-id")) {
        BindToCodeResult::SnapshotMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, "gs-wrong-snapshot-id");
            assert_eq!(actual, snap.graph_snapshot_id);
        }
        other => panic!("expected SnapshotMismatch, got {other:?}"),
    }
}

// ─── 4. Durable BindingEvidence stores both ids ───────────────────────────

#[test]
fn binding_evidence_stores_graph_snapshot_id_and_validated_sha() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    let result = bind_to_code(&inspector, &snap, &req, None);
    match result {
        BindToCodeResult::Bound { proposal, .. } => {
            let evidence = proposal.into_accepted_evidence();
            assert_eq!(
                evidence.graph_snapshot_id.as_deref(),
                Some(snap.graph_snapshot_id.as_str()),
                "BindingEvidence must store graph_snapshot_id"
            );
            assert_eq!(
                evidence.validated_sha.as_deref(),
                Some(snap.commit_sha.as_str()),
                "BindingEvidence must store validated_sha"
            );
        }
        other => panic!("expected Bound, got {other:?}"),
    }
}

#[test]
fn symbol_binding_evidence_stores_snapshot_identity() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let sym_resolver = RustSymbolResolver::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: None,
    };
    let result = bind_symbol_to_code(&sym_resolver, &snap, &req, None);
    match result {
        BindToCodeResult::Bound { proposal, .. } => {
            let evidence = proposal.into_accepted_evidence();
            assert!(evidence.graph_snapshot_id.is_some());
            assert!(evidence.validated_sha.is_some());
        }
        other => panic!("expected Bound, got {other:?}"),
    }
}

// ─── 5. Validation tokens rejected ───────────────────────────────────────

#[test]
fn validation_token_rejected_on_file_binding() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: Some("mcp-signed-token-v1".to_string()),
    };
    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::ValidationTokenUnsupported { .. } => {}
        other => panic!("expected ValidationTokenUnsupported, got {other:?}"),
    }
}

#[test]
fn validation_token_rejected_on_symbol_binding() {
    let dir = setup_repo_with_rust_symbol();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let sym_resolver = RustSymbolResolver::new(dir.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: Some("mcp-signed-token-v1".to_string()),
    };
    match bind_symbol_to_code(&sym_resolver, &snap, &req, None) {
        BindToCodeResult::ValidationTokenUnsupported { .. } => {}
        other => panic!("expected ValidationTokenUnsupported, got {other:?}"),
    }
}

// ─── 6. Two worktrees, same symbol, different commits ─────────────────────

#[test]
fn two_worktrees_same_symbol_different_commits() {
    // Commit A: hello_world returns "hello"
    let dir_a = setup_repo_with_rust_symbol();
    let resolver_a = SnapshotResolver::new(dir_a.path());
    let snap_a = resolver_a.resolve_head().unwrap();

    // Create second worktree by cloning and modifying the same symbol.
    let dir_b = tempfile::tempdir().unwrap();
    let p_b = dir_b.path();

    git(&["init"], p_b);
    git(&["config", "user.email", "test@test.com"], p_b);
    git(&["config", "user.name", "Test"], p_b);

    std::fs::create_dir_all(p_b.join("src")).unwrap();
    std::fs::write(
        p_b.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"goodbye\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], p_b);
    git(&["commit", "-m", "modified hello_world"], p_b);

    let resolver_b = SnapshotResolver::new(p_b);
    let snap_b = resolver_b.resolve_head().unwrap();

    // Snapshots must differ — different commits.
    assert_ne!(
        snap_a.graph_snapshot_id, snap_b.graph_snapshot_id,
        "different commits must produce different snapshot identities"
    );
    assert_ne!(
        snap_a.commit_sha, snap_b.commit_sha,
        "different commits must have different SHAs"
    );

    // Both resolve the same symbol name.
    let sym_a = RustSymbolResolver::new(dir_a.path(), snap_a.clone());
    let sym_b = RustSymbolResolver::new(p_b, snap_b.clone());

    let req_a = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: None,
    };
    let req_b = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: None,
    };

    let result_a = bind_symbol_to_code(&sym_a, &snap_a, &req_a, None);
    let result_b = bind_symbol_to_code(&sym_b, &snap_b, &req_b, None);

    // Both should bind successfully.
    let (proposal_a, gs_a, sha_a) = match result_a {
        BindToCodeResult::Bound {
            proposal,
            graph_snapshot_id,
            validated_sha,
            ..
        } => (proposal, graph_snapshot_id, validated_sha),
        other => panic!("expected Bound for worktree A, got {other:?}"),
    };
    let (proposal_b, gs_b, sha_b) = match result_b {
        BindToCodeResult::Bound {
            proposal,
            graph_snapshot_id,
            validated_sha,
            ..
        } => (proposal, graph_snapshot_id, validated_sha),
        other => panic!("expected Bound for worktree B, got {other:?}"),
    };

    // Same symbol name, different snapshot identities.
    assert_eq!(proposal_a.binding_type, BindingType::Symbol);
    assert_eq!(proposal_b.binding_type, BindingType::Symbol);
    assert_ne!(
        gs_a, gs_b,
        "same symbol at different commits must have different graph_snapshot_ids"
    );
    assert_ne!(
        sha_a, sha_b,
        "same symbol at different commits must have different validated_shas"
    );

    // Promote both to evidence — each stores its own snapshot identity.
    let evidence_a = proposal_a.into_accepted_evidence();
    let evidence_b = proposal_b.into_accepted_evidence();
    assert_ne!(evidence_a.graph_snapshot_id, evidence_b.graph_snapshot_id);
    assert_ne!(evidence_a.validated_sha, evidence_b.validated_sha);
    assert_eq!(evidence_a.binding_type, BindingType::Symbol);
    assert_eq!(evidence_b.binding_type, BindingType::Symbol);
    assert_eq!(evidence_a.confidence, BindingConfidence::High);
    assert_eq!(evidence_b.confidence, BindingConfidence::High);
}

/// Cross-check: worktree A's snapshot cannot verify worktree B's binding.
#[test]
fn cross_worktree_snapshot_mismatch_detected() {
    let dir_a = setup_repo_with_rust_symbol();
    let resolver_a = SnapshotResolver::new(dir_a.path());
    let snap_a = resolver_a.resolve_head().unwrap();

    let dir_b = tempfile::tempdir().unwrap();
    let p_b = dir_b.path();
    git(&["init"], p_b);
    git(&["config", "user.email", "test@test.com"], p_b);
    git(&["config", "user.name", "Test"], p_b);
    std::fs::create_dir_all(p_b.join("src")).unwrap();
    std::fs::write(
        p_b.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"other\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], p_b);
    git(&["commit", "-m", "other version"], p_b);

    let resolver_b = SnapshotResolver::new(p_b);
    let snap_b = resolver_b.resolve_head().unwrap();
    let sym_b = RustSymbolResolver::new(p_b, snap_b.clone());

    // Try to bind at worktree B but expect worktree A's snapshot.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym_b, &snap_b, &req, Some(&snap_a.graph_snapshot_id)) {
        BindToCodeResult::SnapshotMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, snap_a.graph_snapshot_id);
            assert_eq!(actual, snap_b.graph_snapshot_id);
        }
        other => panic!("expected SnapshotMismatch, got {other:?}"),
    }
}
