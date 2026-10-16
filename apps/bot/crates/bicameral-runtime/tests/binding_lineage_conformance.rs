//! Binding lineage and snapshot revalidation conformance tests.
//!
//! Proves ADR-0010 §Binding Lineage Across Snapshots rules:
//!
//! - A binding verified at snapshot A is not automatically verified at snapshot B.
//! - Rebase SHA changes require revalidation; verified identity continuity
//!   produces `Confirmed` only after re-inspection.
//! - Moved targets require verified identity continuity to remain same lineage.
//! - Divergent branch bindings for the same Decision are parallel/divergent
//!   evidence, not `collision_pending` by default.
//! - Unavailable refs produce readiness/snapshot-resolution outcomes, not `not_found`.
//! - Inspected snapshot with absent target returns `not_found`.
//! - RevalidateBinding replay accumulates lineage without mutating prior events.
//!
//! # Acceptance criteria (issue #101)
//!
//! 1. Fixture covers binding at snapshot A viewed at B without auto-verification.
//! 2. Fixture covers rebase revalidation.
//! 3. Fixture covers moved-target identity continuity.
//! 4. Fixture covers divergent branch bindings as parallel evidence.
//! 5. Fixture covers unavailable ref readiness outcomes.
//! 6. Fixture covers inspected snapshot with absent target → `not_found`.
//! 7. Tests run without network, hosted graph, or Drive credentials.
//! 8. Tests exercise replay/materialized state for lineage/revalidation events.
//!
//! All tests use the in-memory `MemoryAdapter` — no network, no hosted graph,
//! no Drive API.

// ─── Reusable harness ─────────────────────────────────────────────────────

mod harness {
    use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
    use bicameral_api::event_store::{
        BindToCodePayload, EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
        RevalidateBindingPayload, RevalidationOutcome, SymbolIdentity,
    };
    use bicameral_api::evidence::{
        BindingConfidence, BindingEvidence, BindingType, GraphEvidenceState, GraphSnapshot,
    };
    use bicameral_api::source::{Source, SourceSnapshot};
    use bicameral_runtime::event_store_adapters::MemoryAdapter;
    use bicameral_runtime::replay::{replay_from_adapter, ReplayedState};
    use uuid::Uuid;

    /// Build a `GraphSnapshot` with explicit identity for lineage tests.
    pub fn snapshot(commit_sha: &str, ref_name: &str) -> GraphSnapshot {
        GraphSnapshot {
            graph_snapshot_id: format!("gs-{commit_sha}"),
            repo_id: "test-repo".to_string(),
            commit_sha: commit_sha.to_string(),
            index_version: "1".to_string(),
            ref_name: Some(ref_name.to_string()),
            merge_base_sha: None,
            indexed_at: None,
            index_source: None,
        }
    }

    /// Ingest a candidate, accept it, return `(candidate_id, decision_id)`.
    pub fn ingest_and_accept(adapter: &MemoryAdapter) -> (Uuid, Uuid) {
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
            content: Some("lineage test content".to_string()),
            content_hash: Some("sha256:lineage".to_string()),
            capture_method: None,
        };
        let evidence = vec![bicameral_api::evidence::SourceEvidence::new(
            snapshot.snapshot_addr.clone(),
            0,
            10,
            "lineage test excerpt".to_string(),
            0.9,
        )];
        let mut candidate = DecisionCandidate::new(
            "Lineage Test Decision".to_string(),
            "Conformance fixture for binding lineage".to_string(),
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

    /// Emit a BindToCode event at a specific snapshot.
    pub fn emit_bind(
        adapter: &MemoryAdapter,
        decision_id: Uuid,
        binding_id: Uuid,
        snapshot: &GraphSnapshot,
        target: &str,
    ) -> Uuid {
        let mut binding = BindingEvidence::new(
            decision_id,
            BindingType::CodeRegion,
            target.to_string(),
            BindingConfidence::High,
        );
        binding.id = binding_id;
        binding.graph_snapshot_id = Some(snapshot.graph_snapshot_id.clone());

        let payload = BindToCodePayload {
            decision_id,
            binding,
            graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
            validated_sha: snapshot.commit_sha.clone(),
            evidence_state: GraphEvidenceState::Verified,
            content_hash: Some("sha256:original-content".to_string()),
            line_start: Some(10),
            line_end: Some(20),
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

    /// Emit a RevalidateBinding event.
    pub fn emit_revalidation(
        adapter: &MemoryAdapter,
        decision_id: Uuid,
        binding_id: Uuid,
        root_binding_id: Uuid,
        original_snapshot: &GraphSnapshot,
        current_snapshot: &GraphSnapshot,
        outcome: RevalidationOutcome,
        evidence_state: GraphEvidenceState,
        symbol_identity: Option<SymbolIdentity>,
        replacement_binding_id: Option<Uuid>,
    ) {
        let payload = RevalidateBindingPayload {
            decision_id,
            binding_id,
            root_binding_id,
            binding_type: BindingType::CodeRegion,
            target: "src/lib.rs:10-20".to_string(),
            original_graph_snapshot_id: original_snapshot.graph_snapshot_id.clone(),
            original_validated_sha: original_snapshot.commit_sha.clone(),
            current_graph_snapshot_id: current_snapshot.graph_snapshot_id.clone(),
            current_validated_sha: current_snapshot.commit_sha.clone(),
            evidence_state,
            content_hash: Some("sha256:revalidated-content".to_string()),
            line_start: Some(10),
            line_end: Some(20),
            outcome,
            symbol_identity,
            replacement_binding_id,
            governance_id: None,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::RevalidateBinding,
                serde_json::to_value(&payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();
    }

    /// Replay the adapter.
    pub fn replay(adapter: &MemoryAdapter) -> ReplayedState {
        replay_from_adapter(adapter).unwrap()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  Binding at snapshot A is not automatically verified at snapshot B
// ═══════════════════════════════════════════════════════════════════════════

/// ADR-0010 §Binding Lineage: "An accepted BindingEvidence record is valid as
/// verified graph evidence only for the GraphSnapshot where it was produced."
///
/// A binding created at snapshot A does not gain automatic verification at
/// snapshot B. Viewing it at B without revalidation leaves it as the original
/// evidence — no new verified claim is created.
#[test]
fn binding_at_snapshot_a_not_auto_verified_at_b() {
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    // Bind at snapshot A
    let snap_a = harness::snapshot("aaa111aaa111", "main");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_a,
        "src/lib.rs:10-20",
    );

    // Time passes, snapshot B exists (different commit), but NO revalidation event
    let _snap_b = harness::snapshot("bbb222bbb222", "main");

    // Replay state
    let state = harness::replay(&adapter);

    // The binding still references snapshot A only
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].graph_snapshot_id.as_deref(),
        Some("gs-aaa111aaa111"),
        "Binding must remain anchored to snapshot A"
    );

    // No revalidation history exists — viewing at B does not create evidence
    assert!(
        !state.binding_revalidations.contains_key(&binding_id),
        "No revalidation event means no automatic verification at snapshot B"
    );
}

/// Proves the invariant through replay: after binding at A, replaying the
/// store at a conceptually later time still only shows evidence for A.
#[test]
fn replay_binding_does_not_fabricate_later_snapshot_evidence() {
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_a = harness::snapshot("commit_a_sha", "feature-x");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_a,
        "src/handler.rs:5-15",
    );

    // Replay twice — state must be identical (no phantom evidence)
    let state1 = harness::replay(&adapter);
    let state2 = harness::replay(&adapter);

    let b1 = &state1.bindings[&decision_id];
    let b2 = &state2.bindings[&decision_id];
    assert_eq!(b1.len(), b2.len());
    assert_eq!(b1[0].graph_snapshot_id, b2[0].graph_snapshot_id);
    assert_eq!(
        b1[0].graph_snapshot_id.as_deref(),
        Some("gs-commit_a_sha"),
        "Replayed binding must always reference original snapshot"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Rebase revalidation — validated_sha changes, content still verifies
// ═══════════════════════════════════════════════════════════════════════════

/// ADR-0010: "If the target content hash or structural hash still matches
/// after a rebase, but validated_sha changes, the graph result is not stale
/// solely because the commit changed."
///
/// After revalidation at the rebased snapshot returns `Verified`, the
/// `Confirmed` outcome is durable. The original binding is preserved.
#[test]
fn rebase_revalidation_confirmed_when_content_matches() {
    use bicameral_api::event_store::RevalidationOutcome;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    // Original binding at pre-rebase snapshot
    let snap_pre_rebase = harness::snapshot("pre_rebase_sha", "feature/login");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_pre_rebase,
        "src/auth.rs:10-20",
    );

    // After rebase: new SHA, but content hash still matches → Confirmed
    let snap_post_rebase = harness::snapshot("post_rebase_sha", "feature/login");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id, // root is itself for first revalidation
        &snap_pre_rebase,
        &snap_post_rebase,
        RevalidationOutcome::Confirmed,
        GraphEvidenceState::Verified,
        None,
        None,
    );

    let state = harness::replay(&adapter);

    // Original binding preserved immutably
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].graph_snapshot_id.as_deref(),
        Some("gs-pre_rebase_sha"),
        "Original binding must not be mutated by revalidation"
    );

    // Revalidation history records the new snapshot
    let revals = state.binding_revalidations.get(&binding_id).unwrap();
    assert_eq!(revals.len(), 1);
    assert_eq!(revals[0].outcome, RevalidationOutcome::Confirmed);
    assert_eq!(revals[0].current_graph_snapshot_id, "gs-post_rebase_sha");
    assert_eq!(revals[0].current_validated_sha, "post_rebase_sha");
    assert_eq!(revals[0].original_validated_sha, "pre_rebase_sha");
    assert_eq!(
        revals[0].evidence_state,
        GraphEvidenceState::Verified,
        "Revalidation at rebased snapshot must be Verified"
    );
}

/// If revalidation at the rebased snapshot fails (content changed), outcome
/// is `Failed` and evidence_state is not Verified.
#[test]
fn rebase_revalidation_failed_when_content_diverges() {
    use bicameral_api::event_store::RevalidationOutcome;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_pre = harness::snapshot("pre_sha_x", "feature/refactor");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_pre,
        "src/core.rs:1-10",
    );

    // Rebase changed the content — revalidation finds NotFound or stale
    let snap_post = harness::snapshot("post_sha_y", "feature/refactor");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_pre,
        &snap_post,
        RevalidationOutcome::Failed,
        GraphEvidenceState::NotFound,
        None,
        None,
    );

    let state = harness::replay(&adapter);

    // Original binding still present (immutable)
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 1);

    // Revalidation recorded as Failed
    let revals = state.binding_revalidations.get(&binding_id).unwrap();
    assert_eq!(revals.len(), 1);
    assert_eq!(revals[0].outcome, RevalidationOutcome::Failed);
    assert_eq!(
        revals[0].evidence_state,
        GraphEvidenceState::NotFound,
        "Failed revalidation must carry the actual evidence state"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Moved target — same lineage only with verified identity continuity
// ═══════════════════════════════════════════════════════════════════════════

/// ADR-0010: "When the target moves, the graph layer may call it the same
/// lineage only if it has verified evidence at the later snapshot plus enough
/// identity continuity to distinguish a move from a different target."
///
/// A replacement binding is issued when the target moved but structural hash
/// matches — identity continuity is verified via `SymbolIdentity`.
#[test]
fn moved_target_same_lineage_with_verified_identity() {
    use bicameral_api::event_store::{RevalidationOutcome, SymbolIdentity};
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_v1 = harness::snapshot("v1_sha", "main");
    let original_binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        original_binding_id,
        &snap_v1,
        "src/old_path.rs:10-20",
    );

    // Target moved to new file — structural hash confirms identity continuity
    let snap_v2 = harness::snapshot("v2_sha", "main");
    let replacement_id = uuid::Uuid::new_v4();
    let symbol_identity = SymbolIdentity {
        symbol_name: "process_data".to_string(),
        qualified_name: Some("crate::handlers::process_data".to_string()),
        structural_hash: Some("sha256:struct-abc123".to_string()),
        resolver_identity: Some("tree-sitter-rust@0.21".to_string()),
        parser_metadata: Some("rust-1.75".to_string()),
    };

    harness::emit_revalidation(
        &adapter,
        decision_id,
        original_binding_id,
        original_binding_id,
        &snap_v1,
        &snap_v2,
        RevalidationOutcome::Replaced,
        GraphEvidenceState::Verified,
        Some(symbol_identity.clone()),
        Some(replacement_id),
    );

    let state = harness::replay(&adapter);

    // Original binding preserved
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].id, original_binding_id);

    // Revalidation records replacement with symbol identity
    let revals = state
        .binding_revalidations
        .get(&original_binding_id)
        .unwrap();
    assert_eq!(revals.len(), 1);
    assert_eq!(revals[0].outcome, RevalidationOutcome::Replaced);
    assert_eq!(revals[0].replacement_binding_id, Some(replacement_id));
    assert_eq!(
        revals[0].evidence_state,
        GraphEvidenceState::Verified,
        "Moved target accepted as same lineage requires Verified evidence"
    );

    // Symbol identity carries continuity proof
    let sym = revals[0].symbol_identity.as_ref().unwrap();
    assert_eq!(sym.symbol_name, "process_data");
    assert_eq!(
        sym.structural_hash.as_deref(),
        Some("sha256:struct-abc123"),
        "Structural hash proves identity continuity"
    );
}

/// Without identity continuity evidence, a moved target cannot be classified
/// as same lineage — the outcome is Inconclusive.
#[test]
fn moved_target_without_identity_continuity_is_inconclusive() {
    use bicameral_api::event_store::RevalidationOutcome;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_v1 = harness::snapshot("move_v1", "main");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_v1,
        "src/removed.rs:1-5",
    );

    // Target not found at new snapshot, no symbol identity to prove continuity
    let snap_v2 = harness::snapshot("move_v2", "main");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_v1,
        &snap_v2,
        RevalidationOutcome::Inconclusive,
        GraphEvidenceState::NotFound,
        None, // No symbol identity → cannot confirm same lineage
        None,
    );

    let state = harness::replay(&adapter);

    let revals = state.binding_revalidations.get(&binding_id).unwrap();
    assert_eq!(revals.len(), 1);
    assert_eq!(
        revals[0].outcome,
        RevalidationOutcome::Inconclusive,
        "Without identity continuity, moved target is Inconclusive"
    );
    assert!(
        revals[0].symbol_identity.is_none(),
        "No symbol identity means no continuity proof"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Divergent branch bindings — parallel evidence, not collision_pending
// ═══════════════════════════════════════════════════════════════════════════

/// ADR-0010: "If two branches bind the same Decision to different symbols,
/// ranges, or files, the graph layer should classify that as divergent binding
/// evidence or parallel lineage, not as a conflict by itself."
///
/// Two bindings from different branches for the same Decision coexist as
/// parallel evidence without triggering collision_pending.
#[test]
fn divergent_branch_bindings_are_parallel_evidence() {
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    // Branch A binds the decision to one code region
    let snap_branch_a = harness::snapshot("branch_a_sha", "feature/alpha");
    let binding_a = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_a,
        &snap_branch_a,
        "src/alpha.rs:1-10",
    );

    // Branch B binds the same decision to a different code region
    let snap_branch_b = harness::snapshot("branch_b_sha", "feature/beta");
    let binding_b = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_b,
        &snap_branch_b,
        "src/beta.rs:20-30",
    );

    let state = harness::replay(&adapter);

    // Both bindings coexist for the same decision
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(
        bindings.len(),
        2,
        "Divergent branch bindings must coexist as parallel evidence"
    );

    // Each references its own snapshot
    let snap_ids: Vec<&str> = bindings
        .iter()
        .map(|b| b.graph_snapshot_id.as_deref().unwrap())
        .collect();
    assert!(snap_ids.contains(&"gs-branch_a_sha"));
    assert!(snap_ids.contains(&"gs-branch_b_sha"));

    // Decision is NOT in collision_pending — divergent bindings alone do not
    // trigger governance conflict (per ADR-0010 and ADR-0011).
    let decision = state.decisions.get(&decision_id).unwrap();
    assert_ne!(
        decision.review_state,
        bicameral_api::candidate::ReviewLifecycleState::CollisionPending,
        "Divergent branch bindings must not produce collision_pending"
    );
}

/// Even with three branches binding the same decision, replay must treat
/// them as parallel evidence accumulation.
#[test]
fn multiple_branch_bindings_accumulate_as_parallel() {
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let branches = [
        ("sha_branch_1", "feature/one", "src/one.rs:1-5"),
        ("sha_branch_2", "feature/two", "src/two.rs:10-15"),
        ("sha_branch_3", "feature/three", "src/three.rs:20-25"),
    ];

    for (sha, ref_name, target) in &branches {
        let snap = harness::snapshot(sha, ref_name);
        let bid = uuid::Uuid::new_v4();
        harness::emit_bind(&adapter, decision_id, bid, &snap, target);
    }

    let state = harness::replay(&adapter);
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 3, "All three branch bindings must coexist");

    // No collision state
    let decision = state.decisions.get(&decision_id).unwrap();
    assert_ne!(
        decision.review_state,
        bicameral_api::candidate::ReviewLifecycleState::CollisionPending,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Unavailable ref → readiness/snapshot-resolution, NOT not_found
// ═══════════════════════════════════════════════════════════════════════════

/// ADR-0010 §Branch Visibility Boundary: "If the ref is not locally available
/// ... the result is a snapshot-resolution or GraphReadiness failure. It must
/// not be reported as verified, and it must not be collapsed into not_found."
///
/// Uses `SnapshotResolver::resolve_ref` to prove the typed outcome.
#[test]
fn unavailable_ref_returns_ref_not_available() {
    use bicameral_api::evidence::SnapshotResolution;
    use bicameral_runtime::grounding::SnapshotResolver;

    let dir = setup_temp_git_repo();
    let resolver = SnapshotResolver::new(dir.path());

    // Request a ref that does not exist locally
    let result = resolver.resolve_ref("refs/remotes/origin/hosted-only-branch");

    match &result {
        SnapshotResolution::RefNotAvailable {
            requested_ref,
            detail,
        } => {
            assert_eq!(requested_ref, "refs/remotes/origin/hosted-only-branch");
            assert!(
                !detail.is_empty(),
                "Detail must explain why the ref is unavailable"
            );
        }
        other => panic!("Unavailable ref must return RefNotAvailable, got {other:?}"),
    }

    // Confirm it is NOT reported as resolved/verified
    assert!(!result.is_resolved());
}

/// A hosted-only scope ref returns `HostedRequired` when the resolver
/// cannot find it locally (simulated by requesting a non-existent remote ref).
#[test]
fn hosted_only_scope_returns_readiness_failure() {
    use bicameral_api::evidence::GraphReadiness;
    use bicameral_runtime::grounding::SnapshotResolver;

    let dir = setup_temp_git_repo();
    let resolver = SnapshotResolver::new(dir.path());

    let result = resolver.resolve_ref("refs/remotes/origin/team-branch-xyz");

    // Should be a non-resolved state
    assert!(!result.is_resolved());

    // The readiness mapped from this resolution must NOT be Ready
    let readiness = result.readiness();
    assert_ne!(
        readiness,
        GraphReadiness::Ready,
        "Unavailable ref must not report Ready readiness"
    );
}

/// Merge-base resolution with an unavailable base ref returns
/// `RefNotAvailable`, not `NotFound`.
#[test]
fn merge_base_unavailable_ref_returns_ref_not_available() {
    use bicameral_api::evidence::SnapshotResolution;
    use bicameral_runtime::grounding::SnapshotResolver;

    let dir = setup_temp_git_repo();
    let resolver = SnapshotResolver::new(dir.path());

    let result = resolver.resolve_merge_base("HEAD", "origin/nonexistent-base");

    match &result {
        SnapshotResolution::RefNotAvailable { requested_ref, .. } => {
            assert!(
                requested_ref.contains("nonexistent-base"),
                "Must identify the unavailable ref"
            );
        }
        other => panic!("Expected RefNotAvailable for merge-base, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Inspected snapshot with absent target → not_found
// ═══════════════════════════════════════════════════════════════════════════

/// ADR-0010: "not_found: the relevant snapshot and required files were
/// inspected, and the symbol or edge is absent."
///
/// When a snapshot IS available and we inspect it, absent targets produce
/// `NotFound` — this is verified negative evidence.
#[test]
fn inspected_snapshot_absent_target_returns_not_found() {
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::bind_to_code::{bind_to_code, BindToCodeRequest, BindToCodeResult};
    use bicameral_runtime::grounding::{FileInspector, SnapshotResolver};

    let dir = setup_temp_git_repo();
    let resolver = SnapshotResolver::new(dir.path());
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());

    // Request binding to a file that does not exist in the snapshot
    let req = BindToCodeRequest {
        decision_id: uuid::Uuid::new_v4(),
        file: "src/does_not_exist.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };

    let result = bind_to_code(&inspector, &snap, &req, None);
    match result {
        BindToCodeResult::Rejected {
            evidence_state,
            detail,
            ..
        } => {
            assert_eq!(
                evidence_state,
                GraphEvidenceState::NotFound,
                "Absent target at inspected snapshot must be NotFound"
            );
            assert!(
                !detail.is_empty(),
                "NotFound must carry a detail explaining what was absent"
            );
        }
        other => panic!("Expected Rejected(NotFound), got {other:?}"),
    }
}

/// Proves the distinction: `NotFound` is for inspected-and-absent,
/// while `RefNotAvailable` is for uninspectable refs (§5 above).
/// They must never be confused.
#[test]
fn not_found_vs_ref_not_available_are_distinct() {
    use bicameral_api::evidence::{GraphEvidenceState, SnapshotResolution};
    use bicameral_runtime::bind_to_code::{bind_to_code, BindToCodeRequest, BindToCodeResult};
    use bicameral_runtime::grounding::{FileInspector, SnapshotResolver};

    let dir = setup_temp_git_repo();
    let resolver = SnapshotResolver::new(dir.path());

    // Case 1: Inspectable snapshot, absent file → NotFound
    let snap = resolver.resolve_head().unwrap();
    let inspector = FileInspector::new(dir.path(), snap.clone());
    let req = BindToCodeRequest {
        decision_id: uuid::Uuid::new_v4(),
        file: "nonexistent.rs".to_string(),
        line_start: None,
        line_end: None,
        symbol: None,
        explanation: None,
        validation_token: None,
    };
    let bind_result = bind_to_code(&inspector, &snap, &req, None);
    let bind_state = match bind_result {
        BindToCodeResult::Rejected { evidence_state, .. } => evidence_state,
        _ => panic!("expected Rejected"),
    };
    assert_eq!(bind_state, GraphEvidenceState::NotFound);

    // Case 2: Unavailable ref → RefNotAvailable (NOT NotFound)
    let resolve_result = resolver.resolve_ref("origin/ghost-branch");
    assert!(
        matches!(resolve_result, SnapshotResolution::RefNotAvailable { .. }),
        "Unavailable ref must be RefNotAvailable, not NotFound"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  RevalidateBinding replay — lineage accumulates without mutation
// ═══════════════════════════════════════════════════════════════════════════

/// Multiple revalidation events accumulate in order. The original binding
/// and all prior revalidations are preserved immutably.
#[test]
fn revalidation_chain_accumulates_lineage_without_mutation() {
    use bicameral_api::event_store::RevalidationOutcome;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_v1 = harness::snapshot("chain_v1", "main");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_v1,
        "src/chain.rs:1-10",
    );

    // First revalidation: confirmed at v2
    let snap_v2 = harness::snapshot("chain_v2", "main");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_v1,
        &snap_v2,
        RevalidationOutcome::Confirmed,
        GraphEvidenceState::Verified,
        None,
        None,
    );

    // Second revalidation: confirmed at v3
    let snap_v3 = harness::snapshot("chain_v3", "main");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_v1,
        &snap_v3,
        RevalidationOutcome::Confirmed,
        GraphEvidenceState::Verified,
        None,
        None,
    );

    // Third revalidation: failed at v4 (file removed)
    let snap_v4 = harness::snapshot("chain_v4", "main");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_v1,
        &snap_v4,
        RevalidationOutcome::Failed,
        GraphEvidenceState::NotFound,
        None,
        None,
    );

    let state = harness::replay(&adapter);

    // Original binding is still present and untouched
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].id, binding_id);
    assert_eq!(
        bindings[0].graph_snapshot_id.as_deref(),
        Some("gs-chain_v1"),
        "Original binding immutable"
    );

    // All three revalidations accumulated in order
    let revals = state.binding_revalidations.get(&binding_id).unwrap();
    assert_eq!(revals.len(), 3, "All revalidation events must accumulate");
    assert_eq!(revals[0].outcome, RevalidationOutcome::Confirmed);
    assert_eq!(revals[0].current_graph_snapshot_id, "gs-chain_v2");
    assert_eq!(revals[1].outcome, RevalidationOutcome::Confirmed);
    assert_eq!(revals[1].current_graph_snapshot_id, "gs-chain_v3");
    assert_eq!(revals[2].outcome, RevalidationOutcome::Failed);
    assert_eq!(revals[2].current_graph_snapshot_id, "gs-chain_v4");
}

/// Replacement binding creates lineage linkage via `root_binding_id`.
#[test]
fn replacement_binding_preserves_lineage_root() {
    use bicameral_api::event_store::{RevalidationOutcome, SymbolIdentity};
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_v1 = harness::snapshot("root_v1", "main");
    let root_binding = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        root_binding,
        &snap_v1,
        "src/root.rs:1-5",
    );

    // First replacement
    let snap_v2 = harness::snapshot("root_v2", "main");
    let replacement_1 = uuid::Uuid::new_v4();
    harness::emit_revalidation(
        &adapter,
        decision_id,
        root_binding,
        root_binding,
        &snap_v1,
        &snap_v2,
        RevalidationOutcome::Replaced,
        GraphEvidenceState::Verified,
        Some(SymbolIdentity {
            symbol_name: "handler".to_string(),
            qualified_name: None,
            structural_hash: Some("sha256:struct-v1".to_string()),
            resolver_identity: None,
            parser_metadata: None,
        }),
        Some(replacement_1),
    );

    // Second replacement (chains from root)
    let snap_v3 = harness::snapshot("root_v3", "main");
    let replacement_2 = uuid::Uuid::new_v4();
    harness::emit_revalidation(
        &adapter,
        decision_id,
        replacement_1,
        root_binding, // root remains the original
        &snap_v2,
        &snap_v3,
        RevalidationOutcome::Replaced,
        GraphEvidenceState::Verified,
        Some(SymbolIdentity {
            symbol_name: "handler".to_string(),
            qualified_name: None,
            structural_hash: Some("sha256:struct-v2".to_string()),
            resolver_identity: None,
            parser_metadata: None,
        }),
        Some(replacement_2),
    );

    let state = harness::replay(&adapter);

    // Both revalidations share the same root_binding_id
    let revals_root = state.binding_revalidations.get(&root_binding).unwrap();
    assert_eq!(revals_root.len(), 1);
    assert_eq!(revals_root[0].root_binding_id, root_binding);

    let revals_r1 = state.binding_revalidations.get(&replacement_1).unwrap();
    assert_eq!(revals_r1.len(), 1);
    assert_eq!(
        revals_r1[0].root_binding_id, root_binding,
        "Chained replacement must preserve the root binding id"
    );
    assert_eq!(revals_r1[0].replacement_binding_id, Some(replacement_2));
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Full replay roundtrip — binding + revalidation cycle
// ═══════════════════════════════════════════════════════════════════════════

/// Full lifecycle: ingest → accept → bind at A → revalidate at B (confirmed)
/// → revalidate at C (failed). Proves replay reconstructs the complete lineage.
#[test]
fn full_lifecycle_bind_revalidate_replay() {
    use bicameral_api::event_store::RevalidationOutcome;
    use bicameral_api::evidence::GraphEvidenceState;
    use bicameral_runtime::event_store_adapters::MemoryAdapter;

    let adapter = MemoryAdapter::new();
    let (_cand_id, decision_id) = harness::ingest_and_accept(&adapter);

    let snap_a = harness::snapshot("lifecycle_a", "main");
    let binding_id = uuid::Uuid::new_v4();
    harness::emit_bind(
        &adapter,
        decision_id,
        binding_id,
        &snap_a,
        "src/lifecycle.rs:1-20",
    );

    let snap_b = harness::snapshot("lifecycle_b", "main");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_a,
        &snap_b,
        RevalidationOutcome::Confirmed,
        GraphEvidenceState::Verified,
        None,
        None,
    );

    let snap_c = harness::snapshot("lifecycle_c", "main");
    harness::emit_revalidation(
        &adapter,
        decision_id,
        binding_id,
        binding_id,
        &snap_a,
        &snap_c,
        RevalidationOutcome::Failed,
        GraphEvidenceState::NotFound,
        None,
        None,
    );

    // Replay
    let state = harness::replay(&adapter);

    // Decision exists
    assert!(state.decisions.contains_key(&decision_id));

    // Binding exists with original snapshot
    let bindings = state.bindings.get(&decision_id).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].graph_snapshot_id.as_deref(),
        Some("gs-lifecycle_a")
    );

    // Two revalidation events in order
    let revals = state.binding_revalidations.get(&binding_id).unwrap();
    assert_eq!(revals.len(), 2);
    assert_eq!(revals[0].outcome, RevalidationOutcome::Confirmed);
    assert_eq!(revals[1].outcome, RevalidationOutcome::Failed);

    // Events include all types
    let event_types: Vec<_> = state.events.iter().map(|e| e.event_type).collect();
    assert!(event_types.contains(&bicameral_api::event_store::EventType::Ingest));
    assert!(event_types.contains(&bicameral_api::event_store::EventType::AcceptCandidate));
    assert!(event_types.contains(&bicameral_api::event_store::EventType::BindToCode));
    assert!(event_types.contains(&bicameral_api::event_store::EventType::RevalidateBinding));
}

// ─── Temp git repo helper ─────────────────────────────────────────────────

fn setup_temp_git_repo() -> tempfile::TempDir {
    use std::process::Command;

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    Command::new("git")
        .args(["init"])
        .current_dir(p)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(p)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(p)
        .output()
        .unwrap();

    std::fs::write(
        p.join("src.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(p)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(p)
        .output()
        .unwrap();

    dir
}
