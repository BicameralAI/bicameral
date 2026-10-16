//! bind_to_code — verified file/range/symbol binding proposal.
//!
//! Implements the ADR-0010 and ADR-0017 contracts: only verified local graph
//! evidence may produce a `BindingProposal`. The proposal is a graph fact,
//! not accepted `BindingEvidence`. Accepted `BindingEvidence` is materialized
//! only at the gateway/event-store boundary via the `BindToCode` governance
//! event.
//!
//! Unverified, ambiguous, unsupported, or stale bindings are rejected with a
//! typed reason. Snapshot mismatch between the inspection snapshot and the
//! requested snapshot is explicit.
//!
//! Validation tokens are represented in the protocol but always return
//! `ValidationTokenUnsupported` in the first implementation slice (ADR-0017).

use bicameral_api::evidence::{
    BindingConfidence, BindingProposal, BindingType, FileInspectionResult, GraphEvidenceState,
    GraphSnapshot, SymbolResolutionResult,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::grounding::FileInspector;
use crate::symbol_resolver::RustSymbolResolver;

/// Request to bind a decision to a file (and optional line range or symbol).
#[derive(Debug, Clone, Deserialize)]
pub struct BindToCodeRequest {
    pub decision_id: Uuid,
    pub file: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    /// Optional symbol name for exact symbol binding.
    pub symbol: Option<String>,
    pub explanation: Option<String>,
    /// Optional validation token (ADR-0017). Always returns
    /// `ValidationTokenUnsupported` in the first implementation slice.
    pub validation_token: Option<String>,
}

/// Outcome of a `bind_to_code` attempt.
///
/// Returns a `BindingProposal` (graph fact) on success — not accepted
/// `BindingEvidence`. The gateway/event-store path converts the proposal
/// into accepted evidence via `BindingProposal::into_accepted_evidence()`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BindToCodeResult {
    /// Verified graph fact — proposed binding ready for gateway acceptance.
    Bound {
        proposal: BindingProposal,
        graph_snapshot_id: String,
        validated_sha: String,
        content_hash: Option<String>,
    },
    /// Binding rejected — evidence state is not verified.
    Rejected {
        evidence_state: GraphEvidenceState,
        detail: String,
        graph_snapshot_id: String,
    },
    /// Snapshot mismatch — the requested snapshot differs from the inspector's.
    SnapshotMismatch {
        expected: String,
        actual: String,
        detail: String,
    },
    /// Validation token provided but unsupported in this implementation slice.
    ValidationTokenUnsupported { detail: String },
}

/// Reject early if a validation token is provided (unsupported in first slice).
fn check_validation_token(req: &BindToCodeRequest) -> Option<BindToCodeResult> {
    if req.validation_token.is_some() {
        return Some(BindToCodeResult::ValidationTokenUnsupported {
            detail: "Validation tokens are represented in the protocol but not yet \
                     accepted. Submit direct binding hints for daemon-side revalidation \
                     instead (ADR-0017)."
                .to_string(),
        });
    }
    None
}

/// Reject early if the expected snapshot does not match the resolved snapshot.
fn check_snapshot_mismatch(
    expected_snapshot_id: Option<&str>,
    snapshot: &GraphSnapshot,
) -> Option<BindToCodeResult> {
    if let Some(expected) = expected_snapshot_id {
        if expected != snapshot.graph_snapshot_id {
            return Some(BindToCodeResult::SnapshotMismatch {
                expected: expected.to_string(),
                actual: snapshot.graph_snapshot_id.clone(),
                detail: "Requested snapshot does not match the inspector's resolved snapshot. \
                         Re-resolve the snapshot or re-validate at the current HEAD."
                    .to_string(),
            });
        }
    }
    None
}

/// Validate a file/range binding against the local graph snapshot and
/// produce a `BindingProposal` only when the evidence state is `Verified`.
///
/// The returned proposal is a graph fact. It must be sent to the gateway
/// for acceptance before it becomes canonical `BindingEvidence`.
pub fn bind_to_code(
    inspector: &FileInspector,
    snapshot: &GraphSnapshot,
    req: &BindToCodeRequest,
    expected_snapshot_id: Option<&str>,
) -> BindToCodeResult {
    // Gate: validation tokens are unsupported in this slice (ADR-0017).
    if let Some(result) = check_validation_token(req) {
        return result;
    }

    // Gate: snapshot identity mismatch.
    if let Some(result) = check_snapshot_mismatch(expected_snapshot_id, snapshot) {
        return result;
    }

    let inspection: FileInspectionResult =
        inspector.inspect_file(&req.file, req.line_start, req.line_end);

    // Gate: only verified evidence may materialize.
    if inspection.evidence_state != GraphEvidenceState::Verified {
        return BindToCodeResult::Rejected {
            evidence_state: inspection.evidence_state,
            detail: inspection
                .detail
                .unwrap_or_else(|| rejection_detail(inspection.evidence_state)),
            graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
        };
    }

    let binding_type = if req.line_start.is_some() {
        BindingType::CodeRegion
    } else {
        BindingType::FilePath
    };

    let target = match (req.line_start, req.line_end) {
        (Some(s), Some(e)) => format!("{}:{}-{}", req.file, s, e),
        (Some(s), None) => format!("{}:{}", req.file, s),
        _ => req.file.clone(),
    };

    let proposal = BindingProposal {
        candidate_id: req.decision_id,
        binding_type,
        target,
        confidence: BindingConfidence::High,
        explanation: req.explanation.clone(),
        graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
        validated_sha: snapshot.commit_sha.clone(),
        content_hash: inspection.content_hash.clone(),
    };

    BindToCodeResult::Bound {
        proposal,
        graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
        validated_sha: snapshot.commit_sha.clone(),
        content_hash: inspection.content_hash,
    }
}

/// Validate a symbol binding against the local graph snapshot via the
/// `RustSymbolResolver`. Produces a `BindingProposal` only when exactly one
/// verified definition is found.
///
/// Symbol binding follows the same snapshot-scoped evidence contract as
/// file/range binding: only `Verified` evidence may materialize, and
/// ambiguous / stale / not-found / unsupported results return typed non-bound
/// statuses.
pub fn bind_symbol_to_code(
    resolver: &RustSymbolResolver,
    snapshot: &GraphSnapshot,
    req: &BindToCodeRequest,
    expected_snapshot_id: Option<&str>,
) -> BindToCodeResult {
    // Gate: validation tokens are unsupported in this slice (ADR-0017).
    if let Some(result) = check_validation_token(req) {
        return result;
    }

    // Gate: snapshot identity mismatch.
    if let Some(result) = check_snapshot_mismatch(expected_snapshot_id, snapshot) {
        return result;
    }

    let symbol_name = match &req.symbol {
        Some(name) => name.as_str(),
        None => {
            return BindToCodeResult::Rejected {
                evidence_state: GraphEvidenceState::Unsupported,
                detail: "bind_symbol_to_code requires a symbol name".to_string(),
                graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
            };
        }
    };

    let resolution: SymbolResolutionResult = resolver.find_symbol(symbol_name);

    // Gate: only verified evidence may materialize.
    if resolution.evidence_state != GraphEvidenceState::Verified {
        return BindToCodeResult::Rejected {
            evidence_state: resolution.evidence_state,
            detail: resolution
                .detail
                .unwrap_or_else(|| rejection_detail(resolution.evidence_state)),
            graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
        };
    }

    // Exactly one verified occurrence — build the proposal.
    let occ = &resolution.occurrences[0];
    let target = format!("{}:{}-{}", occ.file_path, occ.line_start, occ.line_end);

    let proposal = BindingProposal {
        candidate_id: req.decision_id,
        binding_type: BindingType::Symbol,
        target,
        confidence: BindingConfidence::High,
        explanation: req.explanation.clone(),
        graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
        validated_sha: snapshot.commit_sha.clone(),
        content_hash: Some(occ.content_hash.clone()),
    };

    BindToCodeResult::Bound {
        proposal,
        graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
        validated_sha: snapshot.commit_sha.clone(),
        content_hash: Some(occ.content_hash.clone()),
    }
}

fn rejection_detail(state: GraphEvidenceState) -> String {
    match state {
        GraphEvidenceState::NotFound => "File or range not found at the requested snapshot".into(),
        GraphEvidenceState::UnknownStale => {
            "Working tree differs from committed snapshot — binding would be stale".into()
        }
        GraphEvidenceState::UnknownNotIndexed => {
            "Required files have not been parsed or indexed".into()
        }
        GraphEvidenceState::Ambiguous => {
            "Multiple matches found — cannot select without more input".into()
        }
        GraphEvidenceState::Unsupported => {
            "File type or path is outside supported validation".into()
        }
        GraphEvidenceState::ApproximateCandidate => {
            "Approximate match only — not verified graph evidence".into()
        }
        GraphEvidenceState::Verified => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::SnapshotResolver;
    use std::process::Command;

    fn setup_temp_repo() -> tempfile::TempDir {
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

        std::fs::write(p.join("src.rs"), "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        std::fs::create_dir_all(p.join("lib")).unwrap();
        std::fs::write(
            p.join("lib/mod.rs"),
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
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

    #[test]
    fn verified_file_produces_proposal() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "src.rs".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: Some("main entrypoint".to_string()),
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Bound {
                proposal,
                graph_snapshot_id,
                validated_sha,
                content_hash,
            } => {
                assert_eq!(proposal.target, "src.rs");
                assert_eq!(proposal.binding_type, BindingType::FilePath);
                assert_eq!(proposal.confidence, BindingConfidence::High);
                assert_eq!(proposal.graph_snapshot_id, graph_snapshot_id);
                assert_eq!(validated_sha, snap.commit_sha);
                assert!(content_hash.is_some());
            }
            other => panic!("expected Bound, got {other:?}"),
        }
    }

    #[test]
    fn verified_range_produces_proposal() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "lib/mod.rs".to_string(),
            line_start: Some(1),
            line_end: Some(3),
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Bound { proposal, .. } => {
                assert_eq!(proposal.target, "lib/mod.rs:1-3");
                assert_eq!(proposal.binding_type, BindingType::CodeRegion);
            }
            other => panic!("expected Bound, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_rejected() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "nonexistent.rs".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Rejected { evidence_state, .. } => {
                assert_eq!(evidence_state, GraphEvidenceState::NotFound);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn stale_dirty_file_rejected() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();

        // Dirty the file without committing
        std::fs::write(dir.path().join("src.rs"), "fn main() {}\n").unwrap();

        let inspector = FileInspector::new(dir.path(), snap.clone());
        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "src.rs".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Rejected { evidence_state, .. } => {
                assert_eq!(evidence_state, GraphEvidenceState::UnknownStale);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn out_of_range_rejected() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "src.rs".to_string(),
            line_start: Some(999),
            line_end: Some(1000),
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Rejected { evidence_state, .. } => {
                assert_eq!(evidence_state, GraphEvidenceState::NotFound);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_mismatch_explicit() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "src.rs".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, Some("gs-stale-snapshot-id"));
        match result {
            BindToCodeResult::SnapshotMismatch {
                expected, actual, ..
            } => {
                assert_eq!(expected, "gs-stale-snapshot-id");
                assert_eq!(actual, snap.graph_snapshot_id);
            }
            other => panic!("expected SnapshotMismatch, got {other:?}"),
        }
    }

    #[test]
    fn directory_path_rejected_as_unsupported() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "lib".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: None,
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Rejected { evidence_state, .. } => {
                assert_eq!(evidence_state, GraphEvidenceState::Unsupported);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn proposal_survives_serialization_roundtrip() {
        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let req = BindToCodeRequest {
            decision_id: Uuid::new_v4(),
            file: "src.rs".to_string(),
            line_start: Some(1),
            line_end: Some(2),
            symbol: None,
            explanation: Some("entrypoint".to_string()),
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "bound");
        assert!(parsed["content_hash"].is_string());
        assert_eq!(parsed["proposal"]["binding_type"], "code_region");
    }

    #[test]
    fn proposal_promotes_to_accepted_evidence() {
        use bicameral_api::evidence::BindingEvidence;

        let dir = setup_temp_repo();
        let resolver = SnapshotResolver::new(dir.path());
        let snap = resolver.resolve_head().unwrap();
        let inspector = FileInspector::new(dir.path(), snap.clone());

        let decision_id = Uuid::new_v4();
        let req = BindToCodeRequest {
            decision_id,
            file: "src.rs".to_string(),
            line_start: None,
            line_end: None,
            symbol: None,
            explanation: Some("test promotion".to_string()),
            validation_token: None,
        };

        let result = bind_to_code(&inspector, &snap, &req, None);
        match result {
            BindToCodeResult::Bound { proposal, .. } => {
                let evidence: BindingEvidence = proposal.into_accepted_evidence();
                assert_eq!(evidence.candidate_id, decision_id);
                assert_eq!(evidence.target, "src.rs");
                assert_eq!(evidence.binding_type, BindingType::FilePath);
                assert_eq!(evidence.confidence, BindingConfidence::High);
                assert!(evidence.graph_snapshot_id.is_some());
            }
            other => panic!("expected Bound, got {other:?}"),
        }
    }
}
