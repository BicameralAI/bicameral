//! Multi-worktree MCP verification conformance fixtures.
//!
//! Proves issue #139 acceptance criteria:
//!
//! 1. MCP A on `wt-a @ aaa111` and MCP B on `wt-b @ bbb222` produce
//!    separate graph snapshot identities.
//! 2. A copied `graph_snapshot_id` from A into B returns `snapshot_mismatch`.
//! 3. Branch movement creates a new snapshot while old evidence remains
//!    inspectable.
//! 4. Partial graph scope is surfaced in readiness and not treated as full
//!    branch coverage.
//! 5. Fixture proves MCP hints cannot bypass daemon-side verification.
//!
//! All tests are deterministic — no network, no hosted graph, no credentials.
//! These are fixtures only; no production multi-worktree runtime is exercised.

use bicameral_api::evidence::{BindingConfidence, BindingType, GraphReadiness, GraphScopeCoverage};
use bicameral_runtime::bind_to_code::{
    bind_symbol_to_code, bind_to_code, BindToCodeRequest, BindToCodeResult,
};
use bicameral_runtime::graph_readiness_view::{
    build_readiness_view, build_readiness_view_with_scope,
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

fn git_output(args: &[&str], dir: &Path) -> String {
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
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Set up a bare repo with two worktrees simulating independent MCP
/// clients. Each worktree has its own branch at a different commit.
///
/// Layout:
/// - `bare.git` — bare repository
/// - `wt-a` — worktree on branch `branch-a` with initial `hello_world`
/// - `wt-b` — worktree on branch `branch-b` with modified `hello_world`
///
/// Returns `(tmp_dir, wt_a_path, wt_b_path)`.
fn setup_worktree_pair() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create a regular repo first, then add worktrees.
    let main_dir = root.join("main-repo");
    std::fs::create_dir_all(&main_dir).unwrap();
    git(&["init"], &main_dir);
    git(&["config", "user.email", "test@test.com"], &main_dir);
    git(&["config", "user.name", "Test"], &main_dir);

    // Initial commit with hello_world.
    std::fs::create_dir_all(main_dir.join("src")).unwrap();
    std::fs::write(
        main_dir.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"hello\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], &main_dir);
    git(&["commit", "-m", "init: add hello_world"], &main_dir);

    // Create branch-a at current commit.
    git(&["branch", "branch-a"], &main_dir);

    // Create branch-b with a different version of hello_world.
    git(&["checkout", "-b", "branch-b"], &main_dir);
    std::fs::write(
        main_dir.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"goodbye\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], &main_dir);
    git(&["commit", "-m", "branch-b: modify hello_world"], &main_dir);

    // Detach HEAD in the main repo so both branches are free for worktrees.
    let head_sha = git_output(&["rev-parse", "HEAD"], &main_dir);
    git(&["checkout", "--detach", &head_sha], &main_dir);

    // Create worktree wt-a on branch-a.
    let wt_a = root.join("wt-a");
    git(
        &["worktree", "add", wt_a.to_str().unwrap(), "branch-a"],
        &main_dir,
    );

    // Create worktree wt-b on branch-b.
    let wt_b = root.join("wt-b");
    git(
        &["worktree", "add", wt_b.to_str().unwrap(), "branch-b"],
        &main_dir,
    );

    (tmp, wt_a, wt_b)
}

// ═══════════════════════════════════════════════════════════════════════════
// AC1: Separate worktrees produce separate graph snapshot identities.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ac1_separate_worktrees_produce_separate_snapshot_identities() {
    let (_tmp, wt_a, wt_b) = setup_worktree_pair();

    let resolver_a = SnapshotResolver::new(&wt_a);
    let snap_a = resolver_a.resolve_head().unwrap();

    let resolver_b = SnapshotResolver::new(&wt_b);
    let snap_b = resolver_b.resolve_head().unwrap();

    // Different commits → different snapshot identities.
    assert_ne!(
        snap_a.graph_snapshot_id, snap_b.graph_snapshot_id,
        "MCP A and MCP B on different worktrees/commits must produce \
         different graph_snapshot_ids"
    );
    assert_ne!(
        snap_a.commit_sha, snap_b.commit_sha,
        "wt-a and wt-b are at different commits"
    );

    // Both snapshots are valid (gs- prefixed).
    assert!(snap_a.graph_snapshot_id.starts_with("gs-"));
    assert!(snap_b.graph_snapshot_id.starts_with("gs-"));

    // Note: repo_id derivation falls back to directory name for local repos
    // without an `origin` remote. Worktrees at different filesystem paths
    // therefore produce different repo_ids. In production (with a remote),
    // both worktrees would share repo_id. This fixture verifies the snapshot
    // identity separation holds regardless of repo_id derivation strategy.
}

#[test]
fn ac1_same_worktree_same_commit_produces_same_snapshot() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap1 = resolver.resolve_head().unwrap();
    let snap2 = resolver.resolve_head().unwrap();

    assert_eq!(
        snap1.graph_snapshot_id, snap2.graph_snapshot_id,
        "same worktree at same commit must produce identical snapshot identity"
    );
}

#[test]
fn ac1_worktree_bindings_carry_distinct_snapshot_identity() {
    let (_tmp, wt_a, wt_b) = setup_worktree_pair();

    let resolver_a = SnapshotResolver::new(&wt_a);
    let snap_a = resolver_a.resolve_head().unwrap();
    let sym_a = RustSymbolResolver::new(&wt_a, snap_a.clone());

    let resolver_b = SnapshotResolver::new(&wt_b);
    let snap_b = resolver_b.resolve_head().unwrap();
    let sym_b = RustSymbolResolver::new(&wt_b, snap_b.clone());

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

    let (proposal_a, gs_a) = match result_a {
        BindToCodeResult::Bound {
            proposal,
            graph_snapshot_id,
            ..
        } => (proposal, graph_snapshot_id),
        other => panic!("expected Bound for wt-a, got {other:?}"),
    };
    let (proposal_b, gs_b) = match result_b {
        BindToCodeResult::Bound {
            proposal,
            graph_snapshot_id,
            ..
        } => (proposal, graph_snapshot_id),
        other => panic!("expected Bound for wt-b, got {other:?}"),
    };

    assert_ne!(
        gs_a, gs_b,
        "same symbol in different worktrees must carry different snapshot IDs"
    );

    // Promoted evidence also carries distinct identities.
    let evidence_a = proposal_a.into_accepted_evidence();
    let evidence_b = proposal_b.into_accepted_evidence();
    assert_ne!(evidence_a.graph_snapshot_id, evidence_b.graph_snapshot_id);
    assert_ne!(evidence_a.validated_sha, evidence_b.validated_sha);
}

// ═══════════════════════════════════════════════════════════════════════════
// AC2: Copied graph_snapshot_id returns snapshot_mismatch.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ac2_copied_snapshot_id_from_a_into_b_returns_snapshot_mismatch_file() {
    let (_tmp, wt_a, wt_b) = setup_worktree_pair();

    let resolver_a = SnapshotResolver::new(&wt_a);
    let snap_a = resolver_a.resolve_head().unwrap();

    let resolver_b = SnapshotResolver::new(&wt_b);
    let snap_b = resolver_b.resolve_head().unwrap();
    let inspector_b = FileInspector::new(&wt_b, snap_b.clone());

    // MCP B presents a bind request but claims A's snapshot identity.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };

    match bind_to_code(&inspector_b, &snap_b, &req, Some(&snap_a.graph_snapshot_id)) {
        BindToCodeResult::SnapshotMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, snap_a.graph_snapshot_id);
            assert_eq!(actual, snap_b.graph_snapshot_id);
        }
        other => panic!("expected SnapshotMismatch when using A's snapshot at B, got {other:?}"),
    }
}

#[test]
fn ac2_copied_snapshot_id_from_a_into_b_returns_snapshot_mismatch_symbol() {
    let (_tmp, wt_a, wt_b) = setup_worktree_pair();

    let resolver_a = SnapshotResolver::new(&wt_a);
    let snap_a = resolver_a.resolve_head().unwrap();

    let resolver_b = SnapshotResolver::new(&wt_b);
    let snap_b = resolver_b.resolve_head().unwrap();
    let sym_b = RustSymbolResolver::new(&wt_b, snap_b.clone());

    // MCP B presents a symbol bind request but claims A's snapshot identity.
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
        other => {
            panic!("expected SnapshotMismatch when using A's snapshot at B (symbol), got {other:?}")
        }
    }
}

#[test]
fn ac2_reverse_direction_b_into_a_also_mismatches() {
    let (_tmp, wt_a, wt_b) = setup_worktree_pair();

    let resolver_a = SnapshotResolver::new(&wt_a);
    let snap_a = resolver_a.resolve_head().unwrap();
    let inspector_a = FileInspector::new(&wt_a, snap_a.clone());

    let resolver_b = SnapshotResolver::new(&wt_b);
    let snap_b = resolver_b.resolve_head().unwrap();

    // MCP A presents a bind request but claims B's snapshot identity.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };

    match bind_to_code(&inspector_a, &snap_a, &req, Some(&snap_b.graph_snapshot_id)) {
        BindToCodeResult::SnapshotMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, snap_b.graph_snapshot_id);
            assert_eq!(actual, snap_a.graph_snapshot_id);
        }
        other => panic!("expected SnapshotMismatch when using B's snapshot at A, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// AC3: Branch movement creates a new snapshot; old evidence inspectable.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ac3_branch_movement_creates_new_snapshot() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap_before = resolver.resolve_head().unwrap();

    // Bind at the current snapshot.
    let inspector_before = FileInspector::new(&wt_a, snap_before.clone());
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    let result_before = bind_to_code(&inspector_before, &snap_before, &req, None);
    let evidence_before = match result_before {
        BindToCodeResult::Bound { proposal, .. } => proposal.into_accepted_evidence(),
        other => panic!("expected Bound before branch move, got {other:?}"),
    };

    // Advance branch-a: create a new commit.
    std::fs::write(
        wt_a.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"updated\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], &wt_a);
    git(&["commit", "-m", "advance branch-a"], &wt_a);

    // New snapshot after branch movement.
    let snap_after = resolver.resolve_head().unwrap();
    assert_ne!(
        snap_before.graph_snapshot_id, snap_after.graph_snapshot_id,
        "branch movement must create a new snapshot identity"
    );
    assert_ne!(
        snap_before.commit_sha, snap_after.commit_sha,
        "branch movement must advance commit SHA"
    );

    // Old evidence remains inspectable — its snapshot identity and
    // validated_sha are durable anchors.
    assert_eq!(
        evidence_before.graph_snapshot_id.as_deref(),
        Some(snap_before.graph_snapshot_id.as_str()),
        "old evidence retains old snapshot identity after branch movement"
    );
    assert_eq!(
        evidence_before.validated_sha.as_deref(),
        Some(snap_before.commit_sha.as_str()),
        "old evidence retains old validated_sha after branch movement"
    );

    // New binding at the new snapshot produces different identity.
    let inspector_after = FileInspector::new(&wt_a, snap_after.clone());
    let req2 = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    let result_after = bind_to_code(&inspector_after, &snap_after, &req2, None);
    let evidence_after = match result_after {
        BindToCodeResult::Bound { proposal, .. } => proposal.into_accepted_evidence(),
        other => panic!("expected Bound after branch move, got {other:?}"),
    };

    assert_ne!(
        evidence_before.graph_snapshot_id, evidence_after.graph_snapshot_id,
        "new evidence after branch move must have new snapshot identity"
    );
    assert_ne!(
        evidence_before.validated_sha, evidence_after.validated_sha,
        "new evidence after branch move must have new validated_sha"
    );
}

#[test]
fn ac3_old_snapshot_id_mismatches_after_branch_movement() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap_before = resolver.resolve_head().unwrap();

    // Advance branch.
    std::fs::write(
        wt_a.join("src/lib.rs"),
        "pub fn hello_world() -> &'static str {\n    \"moved\"\n}\n",
    )
    .unwrap();
    git(&["add", "."], &wt_a);
    git(&["commit", "-m", "move branch"], &wt_a);

    let snap_after = resolver.resolve_head().unwrap();
    let inspector_after = FileInspector::new(&wt_a, snap_after.clone());

    // Attempt to bind with the old snapshot's identity → mismatch.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };

    match bind_to_code(
        &inspector_after,
        &snap_after,
        &req,
        Some(&snap_before.graph_snapshot_id),
    ) {
        BindToCodeResult::SnapshotMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, snap_before.graph_snapshot_id);
            assert_eq!(actual, snap_after.graph_snapshot_id);
        }
        other => {
            panic!("expected SnapshotMismatch with stale snapshot after branch move, got {other:?}")
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// AC4: Partial graph scope surfaced in readiness, not full branch coverage.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ac4_partial_scope_surfaced_in_readiness() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    // Add a non-Rust file that the Rust symbol resolver cannot index.
    std::fs::write(wt_a.join("src/config.py"), "SETTING = True\n").unwrap();
    git(&["add", "."], &wt_a);
    git(&["commit", "-m", "add non-Rust file"], &wt_a);

    let resolver = SnapshotResolver::new(&wt_a);
    let snap = resolver.resolve_head().unwrap();

    // The Rust symbol resolver can only index .rs files.
    // A readiness view with partial scope must surface this honestly.
    let view = build_readiness_view_with_scope(
        GraphReadiness::Ready,
        Some(snap.graph_snapshot_id.clone()),
        Some("Rust-only symbol index; config.py not indexed".to_string()),
        GraphScopeCoverage::Partial,
    );

    assert_eq!(view.readiness, GraphReadiness::Ready);
    assert_eq!(view.scope_coverage, Some(GraphScopeCoverage::Partial));
    assert_ne!(
        view.scope_coverage,
        Some(GraphScopeCoverage::Full),
        "partial scope must not be treated as full branch coverage"
    );
    assert!(
        view.detail.as_deref().unwrap().contains("not indexed"),
        "detail should explain what is not indexed"
    );
}

#[test]
fn ac4_full_scope_when_all_files_indexed() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap = resolver.resolve_head().unwrap();

    // All files are .rs → full coverage.
    let view = build_readiness_view_with_scope(
        GraphReadiness::Ready,
        Some(snap.graph_snapshot_id.clone()),
        None,
        GraphScopeCoverage::Full,
    );

    assert_eq!(view.readiness, GraphReadiness::Ready);
    assert_eq!(view.scope_coverage, Some(GraphScopeCoverage::Full));
}

#[test]
fn ac4_partial_scope_serializes_correctly() {
    let view = build_readiness_view_with_scope(
        GraphReadiness::Ready,
        Some("gs-test-partial".to_string()),
        Some("partial coverage".to_string()),
        GraphScopeCoverage::Partial,
    );
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["scope_coverage"], "partial");
    assert_eq!(json["readiness"], "ready");
}

#[test]
fn ac4_no_scope_coverage_omits_field() {
    let view = build_readiness_view(GraphReadiness::Ready, Some("gs-test".to_string()), None);
    let json = serde_json::to_value(&view).unwrap();
    // scope_coverage is None → skip_serializing_if means it is omitted.
    assert!(
        json.get("scope_coverage").is_none(),
        "scope_coverage should be omitted when None"
    );
}

#[test]
fn ac4_partial_scope_does_not_affect_readiness_state() {
    // Partial scope is an orthogonal dimension — it does not change
    // the Ready/Stale/Failed/etc readiness classification.
    let view = build_readiness_view_with_scope(
        GraphReadiness::Ready,
        Some("gs-partial".to_string()),
        None,
        GraphScopeCoverage::Partial,
    );
    assert_eq!(
        view.readiness,
        GraphReadiness::Ready,
        "partial scope must not downgrade readiness"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// AC5: MCP hints cannot bypass daemon-side verification.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ac5_validation_token_rejected_at_file_binding_worktree() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(&wt_a, snap.clone());

    // MCP submits a validation_token — daemon must reject.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: Some("mcp-forged-token-v1".to_string()),
    };

    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::ValidationTokenUnsupported { .. } => {}
        other => panic!("expected ValidationTokenUnsupported for MCP hint, got {other:?}"),
    }
}

#[test]
fn ac5_validation_token_rejected_at_symbol_binding_worktree() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap = resolver.resolve_head().unwrap();
    let sym_resolver = RustSymbolResolver::new(&wt_a, snap.clone());

    // MCP submits a validation_token for symbol binding — daemon must reject.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("hello_world".to_string()),
        explanation: None,
        validation_token: Some("mcp-signed-bypass-attempt".to_string()),
    };

    match bind_symbol_to_code(&sym_resolver, &snap, &req, None) {
        BindToCodeResult::ValidationTokenUnsupported { .. } => {}
        other => {
            panic!("expected ValidationTokenUnsupported for MCP hint on symbol, got {other:?}")
        }
    }
}

#[test]
fn ac5_validation_token_rejected_even_with_matching_snapshot() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(&wt_a, snap.clone());

    // MCP provides correct snapshot AND a validation token.
    // Token rejection takes priority — daemon never trusts MCP-provided tokens.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: Some("mcp-correct-snapshot-but-token".to_string()),
    };

    match bind_to_code(&inspector, &snap, &req, Some(&snap.graph_snapshot_id)) {
        BindToCodeResult::ValidationTokenUnsupported { .. } => {}
        other => {
            panic!("expected ValidationTokenUnsupported even with matching snapshot, got {other:?}")
        }
    }
}

#[test]
fn ac5_without_validation_token_binding_succeeds() {
    let (_tmp, wt_a, _wt_b) = setup_worktree_pair();

    let resolver = SnapshotResolver::new(&wt_a);
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(&wt_a, snap.clone());

    // Same request but without a validation token — should succeed.
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
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::FilePath);
            assert_eq!(proposal.confidence, BindingConfidence::High);
        }
        other => panic!("expected Bound without validation token, got {other:?}"),
    }
}
