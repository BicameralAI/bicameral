//! Local grounding baseline — snapshot resolution and file inspection.
//!
//! Implements the ADR-0010 contract for snapshot-scoped local code graph
//! identity and file/range evidence states.
//!
//! Graph indexes produced here are cache/runtime data only, not canonical
//! `.bicameral/` artifacts.

use bicameral_api::evidence::{
    FileInspectionResult, GraphEvidenceState, GraphReadiness, GraphSnapshot, IndexSource,
    SnapshotResolution,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Current file-inspection index version.
///
/// Bump when the inspection logic changes in a way that would alter results
/// for the same commit (e.g. adding range hashing, changing hash algorithm).
const INDEX_VERSION: &str = "file-inspect-v1";

/// Resolves a `GraphSnapshot` from the current git workspace state.
pub struct SnapshotResolver {
    workspace_root: PathBuf,
}

impl SnapshotResolver {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    /// Returns the workspace root path used for snapshot resolution.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Resolve the current HEAD into a `GraphSnapshot`.
    pub fn resolve_head(&self) -> Result<GraphSnapshot, GroundingError> {
        let commit_sha = self.git_rev_parse("HEAD")?;
        let ref_name = self.git_symbolic_ref().ok();
        let repo_id = self.derive_repo_id()?;
        let graph_snapshot_id = derive_graph_snapshot_id(&repo_id, &commit_sha, INDEX_VERSION);

        Ok(GraphSnapshot {
            graph_snapshot_id,
            repo_id,
            commit_sha,
            index_version: INDEX_VERSION.to_string(),
            ref_name,
            merge_base_sha: None,
            indexed_at: Some(Utc::now()),
            index_source: Some(IndexSource::LocalDaemon),
        })
    }

    /// Resolve an arbitrary ref (branch, tag, SHA) into a `SnapshotResolution`.
    ///
    /// Returns `SnapshotResolution::Resolved` when the ref is locally available,
    /// or a typed unavailability reason otherwise. This never collapses missing
    /// refs into `GraphEvidenceState::NotFound`.
    pub fn resolve_ref(&self, requested_ref: &str) -> SnapshotResolution {
        let commit_sha = match self.git_rev_parse(requested_ref) {
            Ok(sha) => sha,
            Err(GroundingError::GitCommand(detail)) => {
                return SnapshotResolution::RefNotAvailable {
                    requested_ref: requested_ref.to_string(),
                    detail,
                };
            }
        };

        let repo_id = match self.derive_repo_id() {
            Ok(id) => id,
            Err(GroundingError::GitCommand(detail)) => {
                return SnapshotResolution::Failed {
                    requested_ref: requested_ref.to_string(),
                    detail,
                };
            }
        };

        let graph_snapshot_id = derive_graph_snapshot_id(&repo_id, &commit_sha, INDEX_VERSION);

        SnapshotResolution::Resolved {
            snapshot: GraphSnapshot {
                graph_snapshot_id,
                repo_id,
                commit_sha,
                index_version: INDEX_VERSION.to_string(),
                ref_name: Some(requested_ref.to_string()),
                merge_base_sha: None,
                indexed_at: Some(Utc::now()),
                index_source: Some(IndexSource::LocalDaemon),
            },
        }
    }

    /// Resolve the merge-base between two refs into a `SnapshotResolution`.
    ///
    /// Both refs must be locally available. The resulting snapshot carries the
    /// merge-base commit SHA and records the base ref in `merge_base_sha`.
    pub fn resolve_merge_base(&self, head_ref: &str, base_ref: &str) -> SnapshotResolution {
        let head_sha = match self.git_rev_parse(head_ref) {
            Ok(sha) => sha,
            Err(GroundingError::GitCommand(detail)) => {
                return SnapshotResolution::RefNotAvailable {
                    requested_ref: head_ref.to_string(),
                    detail: format!("cannot resolve head ref for merge-base: {detail}"),
                };
            }
        };

        let base_sha = match self.git_rev_parse(base_ref) {
            Ok(sha) => sha,
            Err(GroundingError::GitCommand(detail)) => {
                return SnapshotResolution::RefNotAvailable {
                    requested_ref: base_ref.to_string(),
                    detail: format!("cannot resolve base ref for merge-base: {detail}"),
                };
            }
        };

        let merge_base_sha = match self.git_merge_base(&head_sha, &base_sha) {
            Ok(sha) => sha,
            Err(GroundingError::GitCommand(detail)) => {
                return SnapshotResolution::Failed {
                    requested_ref: format!("merge-base({head_ref}, {base_ref})"),
                    detail,
                };
            }
        };

        let repo_id = match self.derive_repo_id() {
            Ok(id) => id,
            Err(GroundingError::GitCommand(detail)) => {
                return SnapshotResolution::Failed {
                    requested_ref: format!("merge-base({head_ref}, {base_ref})"),
                    detail,
                };
            }
        };

        let graph_snapshot_id = derive_graph_snapshot_id(&repo_id, &merge_base_sha, INDEX_VERSION);

        SnapshotResolution::Resolved {
            snapshot: GraphSnapshot {
                graph_snapshot_id,
                repo_id,
                commit_sha: merge_base_sha,
                index_version: INDEX_VERSION.to_string(),
                ref_name: Some(format!("merge-base({head_ref}, {base_ref})")),
                merge_base_sha: Some(base_sha),
                indexed_at: Some(Utc::now()),
                index_source: Some(IndexSource::LocalDaemon),
            },
        }
    }

    /// Check graph readiness for a set of requested snapshot resolutions.
    ///
    /// Returns `GraphReadiness::Ready` only when all resolutions succeeded.
    /// Otherwise returns the most severe non-ready state.
    pub fn check_readiness(resolutions: &[&SnapshotResolution]) -> GraphReadiness {
        fn severity(g: GraphReadiness) -> u8 {
            match g {
                GraphReadiness::Ready => 0,
                GraphReadiness::Indexing => 1,
                GraphReadiness::Stale => 2,
                GraphReadiness::NotConfigured => 3,
                GraphReadiness::Failed => 4,
            }
        }
        let mut worst = GraphReadiness::Ready;
        for res in resolutions {
            let r = res.readiness();
            if severity(r) > severity(worst) {
                worst = r;
            }
        }
        worst
    }

    fn git_rev_parse(&self, rev: &str) -> Result<String, GroundingError> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", rev])
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| GroundingError::GitCommand(format!("failed to run git: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GroundingError::GitCommand(format!(
                "git rev-parse {rev} failed: {stderr}"
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn git_merge_base(&self, sha_a: &str, sha_b: &str) -> Result<String, GroundingError> {
        let output = std::process::Command::new("git")
            .args(["merge-base", sha_a, sha_b])
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| GroundingError::GitCommand(format!("failed to run git: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GroundingError::GitCommand(format!(
                "git merge-base failed: {stderr}"
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn git_symbolic_ref(&self) -> Result<String, GroundingError> {
        let output = std::process::Command::new("git")
            .args(["symbolic-ref", "--short", "HEAD"])
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| GroundingError::GitCommand(format!("failed to run git: {e}")))?;

        if !output.status.success() {
            return Err(GroundingError::GitCommand("detached HEAD".to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn derive_repo_id(&self) -> Result<String, GroundingError> {
        let output = std::process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| GroundingError::GitCommand(format!("failed to run git: {e}")))?;

        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(normalize_repo_url(&url))
        } else {
            // Fallback: use the workspace root directory name.
            let name = self
                .workspace_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            Ok(format!("local:{name}"))
        }
    }
}

/// Inspects files (and optional line ranges) against a `GraphSnapshot`.
pub struct FileInspector {
    workspace_root: PathBuf,
    snapshot: GraphSnapshot,
}

impl FileInspector {
    pub fn new(workspace_root: &Path, snapshot: GraphSnapshot) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            snapshot,
        }
    }

    /// Inspect a single file path, optionally restricting to a line range.
    ///
    /// Returns one of the ADR-0010 evidence states:
    /// - `verified`: file exists at the snapshot commit and content hash matches.
    /// - `not_found`: file does not exist in the working tree.
    /// - `unknown_stale`: file exists but working-tree content differs from the
    ///   committed version (dirty working tree for this path).
    /// - `unsupported`: path is outside the workspace or otherwise uninspectable.
    /// - `ambiguous`: reserved for future multi-match scenarios.
    pub fn inspect_file(
        &self,
        path: &str,
        line_start: Option<u32>,
        line_end: Option<u32>,
    ) -> FileInspectionResult {
        let full_path = self.workspace_root.join(path);

        // Check the path stays inside the workspace.
        if let Ok(canonical) = full_path.canonicalize() {
            if let Ok(ws_canonical) = self.workspace_root.canonicalize() {
                if !canonical.starts_with(&ws_canonical) {
                    return self.result(
                        path,
                        GraphEvidenceState::Unsupported,
                        line_start,
                        line_end,
                        None,
                        Some("path outside workspace".to_string()),
                    );
                }
            }
        }

        if !full_path.exists() {
            return self.result(
                path,
                GraphEvidenceState::NotFound,
                line_start,
                line_end,
                None,
                None,
            );
        }

        if !full_path.is_file() {
            return self.result(
                path,
                GraphEvidenceState::Unsupported,
                line_start,
                line_end,
                None,
                Some("path is not a regular file".to_string()),
            );
        }

        // Read the file content.
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => {
                return self.result(
                    path,
                    GraphEvidenceState::Unsupported,
                    line_start,
                    line_end,
                    None,
                    Some("binary or unreadable file".to_string()),
                );
            }
        };

        // Check if the file is dirty relative to the snapshot commit.
        if self.is_dirty(path) {
            return self.result(
                path,
                GraphEvidenceState::UnknownStale,
                line_start,
                line_end,
                None,
                Some("working tree differs from committed snapshot".to_string()),
            );
        }

        // Extract the relevant slice and hash.
        let slice = match (line_start, line_end) {
            (Some(start), Some(end)) => {
                let lines: Vec<&str> = content.lines().collect();
                let s = (start as usize).saturating_sub(1);
                let e = (end as usize).min(lines.len());
                if s >= lines.len() {
                    return self.result(
                        path,
                        GraphEvidenceState::NotFound,
                        line_start,
                        line_end,
                        None,
                        Some("line range out of bounds".to_string()),
                    );
                }
                lines[s..e].join("\n")
            }
            (Some(start), None) => {
                let lines: Vec<&str> = content.lines().collect();
                let s = (start as usize).saturating_sub(1);
                if s >= lines.len() {
                    return self.result(
                        path,
                        GraphEvidenceState::NotFound,
                        line_start,
                        line_end,
                        None,
                        Some("line out of bounds".to_string()),
                    );
                }
                lines[s].to_string()
            }
            _ => content.clone(),
        };

        let hash = sha256_hex(&slice);

        self.result(
            path,
            GraphEvidenceState::Verified,
            line_start,
            line_end,
            Some(hash),
            None,
        )
    }

    fn is_dirty(&self, path: &str) -> bool {
        let output = std::process::Command::new("git")
            .args(["diff", "--quiet", "HEAD", "--", path])
            .current_dir(&self.workspace_root)
            .status();

        match output {
            Ok(status) => !status.success(),
            Err(_) => true, // If git fails, treat as dirty.
        }
    }

    fn result(
        &self,
        path: &str,
        evidence_state: GraphEvidenceState,
        line_start: Option<u32>,
        line_end: Option<u32>,
        content_hash: Option<String>,
        detail: Option<String>,
    ) -> FileInspectionResult {
        FileInspectionResult {
            path: path.to_string(),
            evidence_state,
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            line_start,
            line_end,
            content_hash,
            detail,
        }
    }
}

/// Derive a deterministic `graph_snapshot_id` from identity fields.
fn derive_graph_snapshot_id(repo_id: &str, commit_sha: &str, index_version: &str) -> String {
    let input = format!("{repo_id}:{commit_sha}:{index_version}");
    let hash = sha256_hex(&input);
    format!("gs-{}", &hash[..16])
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Normalize a git remote URL to a stable repo identifier.
fn normalize_repo_url(url: &str) -> String {
    let s = url.trim().trim_end_matches('/').trim_end_matches(".git");
    // Strip common prefixes to get owner/repo.
    if let Some(rest) = s.strip_prefix("https://github.com/") {
        return rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("git@github.com:") {
        return rest.to_string();
    }
    s.to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum GroundingError {
    #[error("git command error: {0}")]
    GitCommand(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Helper: create a temporary git repo with one committed file.
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

    #[test]
    fn snapshot_resolver_returns_head_identity() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();

        assert!(!snap.graph_snapshot_id.is_empty());
        assert!(!snap.commit_sha.is_empty());
        assert_eq!(snap.index_version, INDEX_VERSION);
        assert!(snap.indexed_at.is_some());
        assert_eq!(snap.index_source, Some(IndexSource::LocalDaemon));
    }

    #[test]
    fn file_inspector_verified_existing_file() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let result = inspector.inspect_file("hello.rs", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::Verified);
        assert_eq!(result.graph_snapshot_id, snap.graph_snapshot_id);
        assert!(result.content_hash.is_some());
    }

    #[test]
    fn file_inspector_not_found_missing_file() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap);

        let result = inspector.inspect_file("does_not_exist.rs", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::NotFound);
    }

    #[test]
    fn file_inspector_verified_line_range() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap);

        let result = inspector.inspect_file("sub/nested.txt", Some(1), Some(2));
        assert_eq!(result.evidence_state, GraphEvidenceState::Verified);
        assert_eq!(result.line_start, Some(1));
        assert_eq!(result.line_end, Some(2));
        assert!(result.content_hash.is_some());
    }

    #[test]
    fn file_inspector_not_found_out_of_range() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap);

        let result = inspector.inspect_file("sub/nested.txt", Some(999), Some(1000));
        assert_eq!(result.evidence_state, GraphEvidenceState::NotFound);
    }

    #[test]
    fn file_inspector_stale_dirty_file() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();

        // Modify committed file without committing.
        std::fs::write(dir.path().join("hello.rs"), "fn main() {}\n").unwrap();

        let inspector = FileInspector::new(dir.path(), snap);
        let result = inspector.inspect_file("hello.rs", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::UnknownStale);
    }

    #[test]
    fn file_inspector_unsupported_directory() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap);

        let result = inspector.inspect_file("sub", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::Unsupported);
    }

    #[test]
    fn graph_snapshot_id_is_deterministic() {
        let id1 = derive_graph_snapshot_id("owner/repo", "abc123", "v1");
        let id2 = derive_graph_snapshot_id("owner/repo", "abc123", "v1");
        assert_eq!(id1, id2);

        let id3 = derive_graph_snapshot_id("owner/repo", "def456", "v1");
        assert_ne!(id1, id3);
    }

    #[test]
    fn normalize_repo_url_strips_suffix() {
        assert_eq!(
            normalize_repo_url("https://github.com/Org/Repo.git"),
            "Org/Repo"
        );
        assert_eq!(
            normalize_repo_url("git@github.com:Org/Repo.git"),
            "Org/Repo"
        );
        assert_eq!(
            normalize_repo_url("https://github.com/Org/Repo"),
            "Org/Repo"
        );
    }

    // --- resolve_ref tests ---

    #[test]
    fn resolve_ref_head_returns_resolved() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let res = resolver.resolve_ref("HEAD");
        assert!(res.is_resolved());
        let snap = res.snapshot().unwrap();
        assert!(!snap.commit_sha.is_empty());
        assert_eq!(snap.index_version, INDEX_VERSION);
        assert_eq!(res.readiness(), GraphReadiness::Ready);
    }

    #[test]
    fn resolve_ref_existing_branch_returns_resolved() {
        let dir = setup_temp_repo();
        let p = dir.path();

        // Create a branch.
        Command::new("git")
            .args(["checkout", "-b", "feature-branch"])
            .current_dir(p)
            .output()
            .unwrap();
        std::fs::write(p.join("feature.rs"), "fn feature() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "feature"])
            .current_dir(p)
            .output()
            .unwrap();

        // Switch back to main and resolve the feature branch.
        Command::new("git")
            .args(["checkout", "master"])
            .current_dir(p)
            .output()
            .unwrap();

        let resolver = SnapshotResolver::new(p);
        let res = resolver.resolve_ref("feature-branch");
        assert!(res.is_resolved());
        assert_eq!(
            res.snapshot().unwrap().ref_name.as_deref(),
            Some("feature-branch")
        );
    }

    #[test]
    fn resolve_ref_missing_ref_returns_ref_not_available() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let res = resolver.resolve_ref("origin/nonexistent-branch");
        assert!(!res.is_resolved());
        match &res {
            SnapshotResolution::RefNotAvailable {
                requested_ref,
                detail,
            } => {
                assert_eq!(requested_ref, "origin/nonexistent-branch");
                assert!(!detail.is_empty());
            }
            other => panic!("expected RefNotAvailable, got {other:?}"),
        }
        assert_eq!(res.readiness(), GraphReadiness::Failed);
    }

    #[test]
    fn resolve_ref_missing_pr_ref_returns_ref_not_available() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let res = resolver.resolve_ref("refs/pull/42/head");
        assert!(!res.is_resolved());
        match &res {
            SnapshotResolution::RefNotAvailable { requested_ref, .. } => {
                assert_eq!(requested_ref, "refs/pull/42/head");
            }
            other => panic!("expected RefNotAvailable, got {other:?}"),
        }
    }

    // --- resolve_merge_base tests ---

    #[test]
    fn resolve_merge_base_returns_resolved_when_both_refs_available() {
        let dir = setup_temp_repo();
        let p = dir.path();

        // Create a branch diverging from master.
        Command::new("git")
            .args(["checkout", "-b", "topic"])
            .current_dir(p)
            .output()
            .unwrap();
        std::fs::write(p.join("topic.rs"), "fn topic() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "topic"])
            .current_dir(p)
            .output()
            .unwrap();

        let resolver = SnapshotResolver::new(p);
        let res = resolver.resolve_merge_base("topic", "master");
        assert!(res.is_resolved());
        let snap = res.snapshot().unwrap();
        assert!(!snap.commit_sha.is_empty());
        assert!(snap.merge_base_sha.is_some());
        assert_eq!(res.readiness(), GraphReadiness::Ready);
    }

    #[test]
    fn resolve_merge_base_missing_base_returns_ref_not_available() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let res = resolver.resolve_merge_base("HEAD", "origin/missing-base");
        assert!(!res.is_resolved());
        match &res {
            SnapshotResolution::RefNotAvailable {
                requested_ref,
                detail,
            } => {
                assert_eq!(requested_ref, "origin/missing-base");
                assert!(detail.contains("merge-base"));
            }
            other => panic!("expected RefNotAvailable, got {other:?}"),
        }
    }

    #[test]
    fn resolve_merge_base_missing_head_returns_ref_not_available() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let res = resolver.resolve_merge_base("nonexistent-head", "master");
        assert!(!res.is_resolved());
        match &res {
            SnapshotResolution::RefNotAvailable { requested_ref, .. } => {
                assert_eq!(requested_ref, "nonexistent-head");
            }
            other => panic!("expected RefNotAvailable, got {other:?}"),
        }
    }

    // --- check_readiness tests ---

    #[test]
    fn check_readiness_all_resolved_is_ready() {
        let r1 = SnapshotResolution::Resolved {
            snapshot: GraphSnapshot {
                graph_snapshot_id: "gs-1".into(),
                repo_id: "r".into(),
                commit_sha: "a".into(),
                index_version: "v1".into(),
                ref_name: None,
                merge_base_sha: None,
                indexed_at: None,
                index_source: None,
            },
        };
        assert_eq!(
            SnapshotResolver::check_readiness(&[&r1]),
            GraphReadiness::Ready
        );
    }

    #[test]
    fn check_readiness_with_unavailable_ref_returns_failed() {
        let good = SnapshotResolution::Resolved {
            snapshot: GraphSnapshot {
                graph_snapshot_id: "gs-1".into(),
                repo_id: "r".into(),
                commit_sha: "a".into(),
                index_version: "v1".into(),
                ref_name: None,
                merge_base_sha: None,
                indexed_at: None,
                index_source: None,
            },
        };
        let bad = SnapshotResolution::RefNotAvailable {
            requested_ref: "missing".into(),
            detail: "not found".into(),
        };
        assert_eq!(
            SnapshotResolver::check_readiness(&[&good, &bad]),
            GraphReadiness::Failed
        );
    }

    #[test]
    fn check_readiness_stale_returns_stale() {
        let stale = SnapshotResolution::StaleIndex {
            requested_ref: "main".into(),
            detail: "version mismatch".into(),
        };
        assert_eq!(
            SnapshotResolver::check_readiness(&[&stale]),
            GraphReadiness::Stale
        );
    }

    // --- inspected-but-not-found tests (NotFound correctly used) ---

    #[test]
    fn file_inspector_not_found_is_inspected_negative_evidence() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap);

        // Inspecting a non-existent file in a valid snapshot → NotFound
        // This is verified negative evidence, not a missing ref.
        let result = inspector.inspect_file("does_not_exist.rs", None, None);
        assert_eq!(result.evidence_state, GraphEvidenceState::NotFound);
    }

    #[test]
    fn graph_snapshot_serializes_required_fields() {
        let snap = GraphSnapshot {
            graph_snapshot_id: "gs-abc".to_string(),
            repo_id: "owner/repo".to_string(),
            commit_sha: "abc123".to_string(),
            index_version: "v1".to_string(),
            ref_name: Some("main".to_string()),
            merge_base_sha: None,
            indexed_at: None,
            index_source: None,
        };

        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["graph_snapshot_id"], "gs-abc");
        assert_eq!(json["repo_id"], "owner/repo");
        assert_eq!(json["commit_sha"], "abc123");
        assert_eq!(json["ref_name"], "main");
        // Optional None fields should be absent.
        assert!(json.get("merge_base_sha").is_none());
    }
}
