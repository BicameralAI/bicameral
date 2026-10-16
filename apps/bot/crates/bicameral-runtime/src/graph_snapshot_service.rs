//! GraphSnapshotService — bot-owned runtime boundary for graph snapshot
//! resolution, readiness reporting, and binding currentness evaluation.
//!
//! Per the graphstore-snapshot-service-runtime-architecture spec (#378, #435):
//!
//! - The service owns resolve, readiness, and currentness operations.
//! - It must not append governance events, accept BindingEvidence, or
//!   mutate signoff/compliance/review/Decision state.
//! - Provider adapters map existing resolvers into the trait boundary.
//!
//! The `LocalGraphProvider` wraps `SnapshotResolver`, `FileInspector`,
//! `RustSymbolResolver`, and `code_source_refresh` as the initial Stage 1
//! provider for local git-backed workspaces.

use bicameral_api::evidence::{
    BindingEvidence, CodeSourceCurrentness, CodeSourceRefreshResult, FileInspectionResult,
    GraphReadiness, GraphReadinessView, GraphSnapshot, SnapshotResolution, SymbolResolutionResult,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::code_source_refresh::{currentness_for_readiness, evaluate_binding_currentness};
use crate::grounding::{FileInspector, GroundingError, SnapshotResolver};
use crate::symbol_resolver::RustSymbolResolver;

/// Errors that can occur within the `GraphSnapshotService`.
#[derive(Debug, thiserror::Error)]
pub enum GraphSnapshotServiceError {
    #[error("workspace not configured: {0}")]
    WorkspaceNotConfigured(String),

    #[error("unsupported workspace: {0}")]
    UnsupportedWorkspace(String),

    #[error("grounding error: {0}")]
    Grounding(#[from] GroundingError),
}

/// Bot-owned runtime boundary for graph snapshot resolution, readiness
/// reporting, file/symbol inspection, and binding currentness evaluation.
///
/// Implementations must not:
/// - append governance events to the event store;
/// - accept or mutate `BindingEvidence`;
/// - mutate signoff, compliance, conflict, review, or Decision state;
/// - compute "safe" or "blocked" conclusions from missing/stale graph data.
pub trait GraphSnapshotService {
    /// Resolve an arbitrary ref to a `SnapshotResolution`.
    fn resolve_snapshot(&self, requested_ref: &str) -> SnapshotResolution;

    /// Resolve HEAD to a `GraphSnapshot`.
    fn resolve_head(&self) -> Result<GraphSnapshot, GraphSnapshotServiceError>;

    /// Check overall graph readiness for a set of resolutions.
    fn check_readiness(&self, resolutions: &[&SnapshotResolution]) -> GraphReadiness;

    /// Build a `GraphReadinessView` for the current workspace.
    fn readiness_view(&self) -> GraphReadinessView;

    /// Inspect a file (optionally a line range) against a resolved snapshot.
    fn inspect_file(
        &self,
        snapshot: &GraphSnapshot,
        path: &str,
        line_start: Option<u32>,
        line_end: Option<u32>,
    ) -> FileInspectionResult;

    /// Resolve symbols in a file against a resolved snapshot.
    fn resolve_symbols(&self, snapshot: &GraphSnapshot, rel_path: &str) -> SymbolResolutionResult;

    /// Evaluate currentness of a single binding against the current workspace.
    fn evaluate_currentness(
        &self,
        decision_id: Uuid,
        binding: &BindingEvidence,
    ) -> CodeSourceRefreshResult;

    /// Evaluate currentness for all bindings of a Decision.
    fn evaluate_decision_currentness(
        &self,
        decision_id: Uuid,
        bindings: &[BindingEvidence],
    ) -> Vec<CodeSourceRefreshResult>;

    /// Map a `GraphReadiness` to the `CodeSourceCurrentness` that prevents
    /// stale/unavailable graph states from being interpreted as governance.
    fn currentness_for_readiness(&self, readiness: GraphReadiness) -> CodeSourceCurrentness;
}

/// Local git-backed provider adapter for `GraphSnapshotService`.
///
/// Wraps `SnapshotResolver`, `FileInspector`, `RustSymbolResolver`, and
/// `code_source_refresh` evaluators behind the service boundary.
pub struct LocalGraphProvider {
    workspace_root: PathBuf,
}

impl LocalGraphProvider {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    /// Returns the workspace root used by this provider.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn resolver(&self) -> SnapshotResolver {
        SnapshotResolver::new(&self.workspace_root)
    }
}

impl GraphSnapshotService for LocalGraphProvider {
    fn resolve_snapshot(&self, requested_ref: &str) -> SnapshotResolution {
        self.resolver().resolve_ref(requested_ref)
    }

    fn resolve_head(&self) -> Result<GraphSnapshot, GraphSnapshotServiceError> {
        self.resolver().resolve_head().map_err(Into::into)
    }

    fn check_readiness(&self, resolutions: &[&SnapshotResolution]) -> GraphReadiness {
        SnapshotResolver::check_readiness(resolutions)
    }

    fn readiness_view(&self) -> GraphReadinessView {
        let resolution = self.resolver().resolve_ref("HEAD");
        GraphReadinessView::from_resolution(&resolution)
    }

    fn inspect_file(
        &self,
        snapshot: &GraphSnapshot,
        path: &str,
        line_start: Option<u32>,
        line_end: Option<u32>,
    ) -> FileInspectionResult {
        let inspector = FileInspector::new(&self.workspace_root, snapshot.clone());
        inspector.inspect_file(path, line_start, line_end)
    }

    fn resolve_symbols(&self, snapshot: &GraphSnapshot, rel_path: &str) -> SymbolResolutionResult {
        let resolver = RustSymbolResolver::new(&self.workspace_root, snapshot.clone());
        resolver.resolve_file(rel_path)
    }

    fn evaluate_currentness(
        &self,
        decision_id: Uuid,
        binding: &BindingEvidence,
    ) -> CodeSourceRefreshResult {
        evaluate_binding_currentness(&self.resolver(), decision_id, binding)
    }

    fn evaluate_decision_currentness(
        &self,
        decision_id: Uuid,
        bindings: &[BindingEvidence],
    ) -> Vec<CodeSourceRefreshResult> {
        let resolver = self.resolver();
        bindings
            .iter()
            .map(|b| evaluate_binding_currentness(&resolver, decision_id, b))
            .collect()
    }

    fn currentness_for_readiness(&self, readiness: GraphReadiness) -> CodeSourceCurrentness {
        currentness_for_readiness(readiness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::evidence::{BindingConfidence, BindingType, GraphEvidenceState};
    use std::process::Command;

    /// Helper: create a temporary git repo with committed files.
    fn setup_temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();

        Command::new("git")
            .args(["init", "-b", "master"])
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
            p.join("hello.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(p.join("sub")).unwrap();
        std::fs::write(p.join("sub/nested.txt"), "line1\nline2\nline3\n").unwrap();

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

    /// Helper: create a minimal binding for test purposes.
    fn test_binding(target: &str) -> BindingEvidence {
        BindingEvidence::new(
            Uuid::new_v4(),
            BindingType::FilePath,
            target.to_string(),
            BindingConfidence::High,
        )
    }

    // ── Trait interface: resolve ──────────────────────────────────────

    #[test]
    fn graph_snapshot_service_resolve_head_returns_snapshot() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let snap = svc.resolve_head().unwrap();
        assert!(!snap.graph_snapshot_id.is_empty());
        assert!(!snap.commit_sha.is_empty());
    }

    #[test]
    fn graph_snapshot_service_resolve_snapshot_ready() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let resolution = svc.resolve_snapshot("HEAD");
        assert!(resolution.is_resolved());
        assert_eq!(resolution.readiness(), GraphReadiness::Ready);
    }

    #[test]
    fn graph_snapshot_service_resolve_snapshot_ref_not_available() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let resolution = svc.resolve_snapshot("nonexistent-ref");
        assert!(!resolution.is_resolved());
        assert_eq!(resolution.readiness(), GraphReadiness::Failed);
    }

    // ── Trait interface: readiness ────────────────────────────────────

    #[test]
    fn graph_snapshot_service_readiness_view_ready() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let view = svc.readiness_view();
        assert_eq!(view.readiness, GraphReadiness::Ready);
        assert!(view.graph_snapshot_id.is_some());
    }

    #[test]
    fn graph_snapshot_service_check_readiness_all_ready() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let r1 = svc.resolve_snapshot("HEAD");
        let readiness = svc.check_readiness(&[&r1]);
        assert_eq!(readiness, GraphReadiness::Ready);
    }

    #[test]
    fn graph_snapshot_service_check_readiness_with_failed() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let r_ok = svc.resolve_snapshot("HEAD");
        let r_fail = svc.resolve_snapshot("nonexistent-ref");
        let readiness = svc.check_readiness(&[&r_ok, &r_fail]);
        assert_eq!(readiness, GraphReadiness::Failed);
    }

    // ── Trait interface: file inspection ──────────────────────────────

    #[test]
    fn graph_snapshot_service_inspect_file_verified() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());
        let snap = svc.resolve_head().unwrap();

        let result = svc.inspect_file(&snap, "hello.rs", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::Verified);
        assert!(result.content_hash.is_some());
    }

    #[test]
    fn graph_snapshot_service_inspect_file_not_found() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());
        let snap = svc.resolve_head().unwrap();

        let result = svc.inspect_file(&snap, "missing.rs", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::NotFound);
    }

    // ── Trait interface: symbol resolution ────────────────────────────

    #[test]
    fn graph_snapshot_service_resolve_symbols_verified() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());
        let snap = svc.resolve_head().unwrap();

        let result = svc.resolve_symbols(&snap, "hello.rs");
        assert_eq!(result.evidence_state, GraphEvidenceState::Verified);
    }

    #[test]
    fn graph_snapshot_service_resolve_symbols_unsupported_language() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());
        let snap = svc.resolve_head().unwrap();

        let result = svc.resolve_symbols(&snap, "sub/nested.txt");
        assert_eq!(result.evidence_state, GraphEvidenceState::Unsupported);
    }

    // ── Trait interface: currentness evaluation ──────────────────────

    #[test]
    fn graph_snapshot_service_evaluate_currentness_current() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let binding = test_binding("hello.rs");
        let decision_id = Uuid::new_v4();
        let result = svc.evaluate_currentness(decision_id, &binding);

        assert_eq!(result.graph_readiness, GraphReadiness::Ready);
        assert_eq!(result.currentness, CodeSourceCurrentness::Current);
        assert!(result.evaluated_snapshot_id.is_some());
    }

    #[test]
    fn graph_snapshot_service_evaluate_currentness_target_missing() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let binding = test_binding("does_not_exist.rs");
        let decision_id = Uuid::new_v4();
        let result = svc.evaluate_currentness(decision_id, &binding);

        assert_eq!(result.graph_readiness, GraphReadiness::Ready);
        assert_eq!(result.currentness, CodeSourceCurrentness::TargetMissing);
    }

    #[test]
    fn graph_snapshot_service_evaluate_decision_currentness_multiple() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let b1 = test_binding("hello.rs");
        let b2 = test_binding("missing.rs");
        let decision_id = Uuid::new_v4();
        let results = svc.evaluate_decision_currentness(decision_id, &[b1, b2]);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].currentness, CodeSourceCurrentness::Current);
        assert_eq!(results[1].currentness, CodeSourceCurrentness::TargetMissing);
    }

    // ── Currentness-for-readiness mapping ────────────────────────────

    #[test]
    fn graph_snapshot_service_currentness_for_stale_readiness() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        assert_eq!(
            svc.currentness_for_readiness(GraphReadiness::Stale),
            CodeSourceCurrentness::GraphUnavailable
        );
    }

    #[test]
    fn graph_snapshot_service_currentness_for_not_configured() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        assert_eq!(
            svc.currentness_for_readiness(GraphReadiness::NotConfigured),
            CodeSourceCurrentness::GraphNotConfigured
        );
    }

    #[test]
    fn graph_snapshot_service_currentness_for_ready() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        assert_eq!(
            svc.currentness_for_readiness(GraphReadiness::Ready),
            CodeSourceCurrentness::Current
        );
    }

    // ── Unsupported workspace ────────────────────────────────────────

    #[test]
    fn graph_snapshot_service_non_git_workspace() {
        let dir = tempfile::tempdir().unwrap();
        // No git init — this is an unsupported workspace.
        let svc = LocalGraphProvider::new(dir.path());

        let view = svc.readiness_view();
        assert_ne!(view.readiness, GraphReadiness::Ready);
    }

    // ── No governance mutation ────────────────────────────────────────

    #[test]
    fn graph_snapshot_service_does_not_mutate_binding() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let binding = test_binding("hello.rs");
        let original_id = binding.id;
        let original_target = binding.target.clone();
        let original_confidence = binding.confidence;
        let decision_id = Uuid::new_v4();

        let _result = svc.evaluate_currentness(decision_id, &binding);

        // Binding must be unchanged after evaluation.
        assert_eq!(binding.id, original_id);
        assert_eq!(binding.target, original_target);
        assert_eq!(binding.confidence, original_confidence);
    }

    #[test]
    fn graph_snapshot_service_evaluate_does_not_produce_governance_state() {
        let dir = setup_temp_repo();
        let svc = LocalGraphProvider::new(dir.path());

        let binding = test_binding("hello.rs");
        let decision_id = Uuid::new_v4();
        let result = svc.evaluate_currentness(decision_id, &binding);

        // Result is a derived read-model, not a governance event.
        // It carries graph_readiness and currentness, never signoff/compliance.
        assert!(matches!(
            result.currentness,
            CodeSourceCurrentness::Current
                | CodeSourceCurrentness::ContentChanged
                | CodeSourceCurrentness::TargetMissing
                | CodeSourceCurrentness::GraphUnavailable
                | CodeSourceCurrentness::GraphNotConfigured
        ));
    }
}
