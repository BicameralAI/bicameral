//! V1 code-grounding fixture conformance tests.
//!
//! Uses the committed fixture workspace under `tests/fixtures/code-grounding/`
//! to exercise every `GraphEvidenceState` variant without network access or
//! external repository dependencies.
//!
//! All outputs are **evidence states** and **graph-readiness observations**,
//! not compliance verdicts, enforcement decisions, or feature-readiness claims
//! (ADR-0010, ADR-0015).

use bicameral_api::evidence::{BindingType, GraphEvidenceState};
use bicameral_runtime::bind_to_code::{
    bind_symbol_to_code, bind_to_code, BindToCodeRequest, BindToCodeResult,
};
use bicameral_runtime::grounding::{FileInspector, SnapshotResolver};
use bicameral_runtime::symbol_resolver::RustSymbolResolver;
use bicameral_runtime::test_fixtures::FixtureWorktree;
use uuid::Uuid;

// ── Verified scenarios ────────────────────────────────────────────────────

#[test]
fn fixture_verified_file_binding() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(wt.path(), snap.clone());

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
            assert_eq!(proposal.target, "src/lib.rs");
        }
        other => panic!("expected Bound for fixture file, got {other:?}"),
    }
}

#[test]
fn fixture_verified_range_binding() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(wt.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: Some(1),
        line_end: Some(5),
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::CodeRegion);
            assert_eq!(proposal.target, "src/lib.rs:1-5");
        }
        other => panic!("expected Bound for fixture range, got {other:?}"),
    }
}

#[test]
fn fixture_verified_symbol_binding() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let sym = RustSymbolResolver::new(wt.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("process_order".to_string()),
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym, &snap, &req, None) {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::Symbol);
            assert!(
                proposal.target.starts_with("src/lib.rs:"),
                "symbol target should reference file:range, got: {}",
                proposal.target
            );
        }
        other => panic!("expected Bound for fixture symbol, got {other:?}"),
    }
}

#[test]
fn fixture_verified_file_types() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(wt.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/types.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::FilePath);
            assert_eq!(proposal.target, "src/types.rs");
        }
        other => panic!("expected Bound for types.rs file, got {other:?}"),
    }
}

// ── Not found ─────────────────────────────────────────────────────────────

#[test]
fn fixture_not_found_missing_file() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(wt.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/nonexistent.rs".to_string(),
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

// ── Ambiguous ─────────────────────────────────────────────────────────────

#[test]
fn fixture_ambiguous_symbol() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let sym = RustSymbolResolver::new(wt.path(), snap.clone());

    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/ambiguous.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("SharedName".to_string()),
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::Ambiguous);
        }
        other => panic!("expected Rejected/Ambiguous, got {other:?}"),
    }
}

// ── Unsupported ───────────────────────────────────────────────────────────

#[test]
fn fixture_unsupported_no_symbol_name() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let sym = RustSymbolResolver::new(wt.path(), snap.clone());

    // No symbol name → Unsupported (bind_symbol_to_code requires a symbol name).
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/lib.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::Unsupported);
        }
        other => panic!("expected Rejected/Unsupported, got {other:?}"),
    }
}

#[test]
fn fixture_non_rust_file_symbol_not_found() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let sym = RustSymbolResolver::new(wt.path(), snap.clone());

    // Symbol search in non-Rust file returns NotFound (resolver only indexes .rs).
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "data/config.json".to_string(),
        line_start: None,
        line_end: None,
        symbol: Some("name".to_string()),
        explanation: None,
        validation_token: None,
    };
    match bind_symbol_to_code(&sym, &snap, &req, None) {
        BindToCodeResult::Rejected { evidence_state, .. } => {
            assert_eq!(evidence_state, GraphEvidenceState::NotFound);
        }
        other => panic!("expected Rejected/NotFound for non-Rust symbol, got {other:?}"),
    }
}

// ── Stale / changed target ────────────────────────────────────────────────

#[test]
fn fixture_stale_dirty_file() {
    let wt = FixtureWorktree::new();
    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();

    // Dirty the file after snapshot resolution.
    wt.dirty_file("src/lib.rs", "// modified after snapshot\npub fn process_order(id: u64) -> String { format!(\"changed-{id}\") }\n");

    let inspector = FileInspector::new(wt.path(), snap.clone());
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

// ── Moved / renamed target ────────────────────────────────────────────────

#[test]
fn fixture_moved_target_old_path_not_found() {
    let wt = FixtureWorktree::new();

    // Rename the file and commit.
    wt.rename_and_commit(
        "src/moved_target.rs",
        "src/renamed.rs",
        "rename moved_target -> renamed",
    );

    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(wt.path(), snap.clone());

    // Old path should be not found.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/moved_target.rs".to_string(),
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
        other => panic!("expected Rejected/NotFound for old path, got {other:?}"),
    }
}

#[test]
fn fixture_moved_target_new_path_verified() {
    let wt = FixtureWorktree::new();

    // Rename the file and commit.
    wt.rename_and_commit(
        "src/moved_target.rs",
        "src/renamed.rs",
        "rename moved_target -> renamed",
    );

    let resolver = SnapshotResolver::new(wt.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(wt.path(), snap.clone());

    // New path should be verified.
    let req = BindToCodeRequest {
        decision_id: Uuid::new_v4(),
        file: "src/renamed.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    match bind_to_code(&inspector, &snap, &req, None) {
        BindToCodeResult::Bound { proposal, .. } => {
            assert_eq!(proposal.binding_type, BindingType::FilePath);
            assert_eq!(proposal.target, "src/renamed.rs");
        }
        other => panic!("expected Bound for new path after rename, got {other:?}"),
    }
}

// ── Fixture reuse: two worktrees from same fixture ────────────────────────

#[test]
fn fixture_two_worktrees_independent_snapshots() {
    let wt_a = FixtureWorktree::new();
    let wt_b = FixtureWorktree::new();

    // Mutate B only.
    wt_b.add_file_and_commit(
        "src/extra.rs",
        "pub fn extra() {}\n",
        "add extra file to worktree B",
    );

    let resolver_a = SnapshotResolver::new(wt_a.path());
    let resolver_b = SnapshotResolver::new(wt_b.path());
    let snap_a = resolver_a.resolve_head().unwrap();
    let snap_b = resolver_b.resolve_head().unwrap();

    // Different commits → different snapshot identities.
    assert_ne!(
        snap_a.graph_snapshot_id, snap_b.graph_snapshot_id,
        "independent worktrees with different commits must produce different snapshot ids"
    );
}
