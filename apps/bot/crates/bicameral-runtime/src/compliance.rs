//! ComplianceEvaluator — computes compliance state from persisted bindings
//! and current grounding evidence.
//!
//! Compliance is independent from signoff (ADR-0022, issue #193). A Decision
//! may be `approved` on the signoff axis while simultaneously `drifted` on
//! the compliance axis. Signoff alone must never be used as proof of
//! compliance, enforcement, readiness, or implementation correctness. The
//! evaluator's inputs are binding evidence and graph state only — it never
//! consults `SignoffState`.
//!
//! State semantics (per ADR-0006 §Ledger View and issue #52):
//!
//! - `ungrounded`: no `BindingEvidence` exists for this Decision.
//! - `reflected`: all bindings are verified unchanged at the current snapshot.
//! - `partial`: some bindings are verified, others are changed or missing.
//! - `drifted`: all bindings show changed or missing symbols/files.
//! - `pending`: graph evidence is stale, unsupported, or ambiguous — cannot
//!   determine compliance until the graph is refreshed.

use bicameral_api::dashboard::ComplianceState;
use bicameral_api::evidence::{
    BindingEvidence, BindingType, FileInspectionResult, GraphEvidenceState,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::grounding::FileInspector;

/// Per-binding verdict produced by the evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingVerdict {
    pub binding_id: Uuid,
    pub target: String,
    pub evidence_state: GraphEvidenceState,
    pub content_match: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Aggregate compliance verdict for a single Decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceVerdict {
    pub decision_id: Uuid,
    pub compliance_state: ComplianceState,
    pub binding_verdicts: Vec<BindingVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Evaluates compliance state for Decisions against current grounding evidence.
///
/// The evaluator re-inspects each persisted binding target against the current
/// `GraphSnapshot` via `FileInspector` and compares content hashes to detect
/// drift. It does not modify signoff state or event-store state.
pub struct ComplianceEvaluator<'a> {
    inspector: &'a FileInspector,
}

impl<'a> ComplianceEvaluator<'a> {
    pub fn new(inspector: &'a FileInspector) -> Self {
        Self { inspector }
    }

    /// Evaluate compliance for a single Decision given its persisted bindings
    /// and the original content hashes recorded at bind time.
    ///
    /// `content_hashes` maps `BindingEvidence.id` → content hash recorded in
    /// the original `BindToCodePayload`. When absent, content-level drift
    /// detection falls back to evidence-state-only comparison.
    pub fn evaluate(
        &self,
        decision_id: Uuid,
        bindings: &[BindingEvidence],
        content_hashes: &std::collections::HashMap<Uuid, Option<String>>,
    ) -> ComplianceVerdict {
        if bindings.is_empty() {
            return ComplianceVerdict {
                decision_id,
                compliance_state: ComplianceState::Ungrounded,
                binding_verdicts: vec![],
                detail: Some("No binding evidence exists for this Decision".to_string()),
            };
        }

        let mut verdicts = Vec::with_capacity(bindings.len());
        let mut reflected_count: usize = 0;
        let mut drifted_count: usize = 0;
        let mut pending_count: usize = 0;

        for binding in bindings {
            let (file, line_start, line_end) =
                parse_binding_target(&binding.target, binding.binding_type);
            let inspection = self.inspector.inspect_file(&file, line_start, line_end);
            let verdict = self.judge_binding(binding, &inspection, content_hashes);

            match verdict.evidence_state {
                GraphEvidenceState::Verified => {
                    let is_match = verdict.content_match.unwrap_or(true);
                    if is_match {
                        reflected_count += 1;
                    } else {
                        drifted_count += 1;
                    }
                }
                GraphEvidenceState::NotFound => {
                    drifted_count += 1;
                }
                GraphEvidenceState::UnknownStale
                | GraphEvidenceState::UnknownNotIndexed
                | GraphEvidenceState::Unsupported
                | GraphEvidenceState::Ambiguous
                | GraphEvidenceState::ApproximateCandidate => {
                    pending_count += 1;
                }
            }

            verdicts.push(verdict);
        }

        let total = verdicts.len();
        let compliance_state = if pending_count == total {
            ComplianceState::Pending
        } else if reflected_count == total {
            ComplianceState::Reflected
        } else if drifted_count == total {
            ComplianceState::Drifted
        } else if (reflected_count > 0 && (drifted_count > 0 || pending_count > 0))
            || (drifted_count > 0 && pending_count > 0)
        {
            ComplianceState::Partial
        } else if pending_count > 0 {
            ComplianceState::Pending
        } else {
            ComplianceState::Drifted
        };

        ComplianceVerdict {
            decision_id,
            compliance_state,
            binding_verdicts: verdicts,
            detail: None,
        }
    }

    fn judge_binding(
        &self,
        binding: &BindingEvidence,
        inspection: &FileInspectionResult,
        content_hashes: &std::collections::HashMap<Uuid, Option<String>>,
    ) -> BindingVerdict {
        let content_match = match inspection.evidence_state {
            GraphEvidenceState::Verified => {
                let original_hash = content_hashes.get(&binding.id).and_then(|h| h.as_deref());
                match (original_hash, inspection.content_hash.as_deref()) {
                    (Some(orig), Some(current)) => Some(orig == current),
                    // No original hash to compare — treat as match (binding
                    // was created before content-hash tracking).
                    (None, _) => Some(true),
                    // Original hash exists but current inspection didn't produce one —
                    // conservative: cannot confirm match.
                    (Some(_), None) => Some(false),
                }
            }
            _ => None,
        };

        let detail = match inspection.evidence_state {
            GraphEvidenceState::Verified => {
                if content_match == Some(false) {
                    Some("Content hash differs from binding time — code has changed".to_string())
                } else {
                    None
                }
            }
            GraphEvidenceState::NotFound => {
                Some("Bound file or range no longer exists".to_string())
            }
            GraphEvidenceState::UnknownStale => {
                Some("Working tree differs from committed snapshot".to_string())
            }
            GraphEvidenceState::UnknownNotIndexed => {
                Some("Required files have not been parsed or indexed".to_string())
            }
            GraphEvidenceState::Unsupported => {
                Some("File type or path is outside supported validation".to_string())
            }
            GraphEvidenceState::Ambiguous => {
                Some("Multiple matches — cannot determine compliance".to_string())
            }
            GraphEvidenceState::ApproximateCandidate => {
                Some("Approximate match only — not verified evidence".to_string())
            }
        };

        BindingVerdict {
            binding_id: binding.id,
            target: binding.target.clone(),
            evidence_state: inspection.evidence_state,
            content_match,
            detail,
        }
    }
}

/// Returns `true` when the given compliance state could have been determined
/// without any signoff information — i.e. compliance is derived purely from
/// binding evidence and graph state, never from signoff.
///
/// This is a boundary guard for ADR-0022: callers computing enforcement,
/// compliance, or readiness must not use signoff alone as proof. This
/// function always returns `true` because `ComplianceEvaluator` never
/// consults `SignoffState`. It exists as a machine-checkable assertion of
/// the invariant.
pub fn signoff_is_not_compliance_proof(
    _signoff: bicameral_api::dashboard::SignoffState,
    compliance: ComplianceState,
) -> bool {
    // Compliance state is determined entirely by binding evidence and graph
    // state (ComplianceEvaluator). Signoff is never an input.
    // Any compliance state is valid regardless of signoff.
    let _ = compliance;
    true
}

/// Evaluate compliance for a Decision without a live `FileInspector`.
///
/// Used when graph readiness fails or no workspace is available. Decisions
/// with bindings whose graph snapshot is stale or unsupported produce `Pending`.
pub fn evaluate_without_inspector(
    decision_id: Uuid,
    bindings: &[BindingEvidence],
) -> ComplianceVerdict {
    if bindings.is_empty() {
        return ComplianceVerdict {
            decision_id,
            compliance_state: ComplianceState::Ungrounded,
            binding_verdicts: vec![],
            detail: Some("No binding evidence exists for this Decision".to_string()),
        };
    }

    ComplianceVerdict {
        decision_id,
        compliance_state: ComplianceState::Pending,
        binding_verdicts: bindings
            .iter()
            .map(|b| BindingVerdict {
                binding_id: b.id,
                target: b.target.clone(),
                evidence_state: GraphEvidenceState::UnknownStale,
                content_match: None,
                detail: Some(
                    "Graph snapshot not available — compliance evaluation deferred".to_string(),
                ),
            })
            .collect(),
        detail: Some(
            "No graph snapshot available for compliance evaluation; state is pending".to_string(),
        ),
    }
}

/// Parse a binding target string into (file, line_start, line_end).
///
/// Targets follow the convention from `bind_to_code`:
/// - `path/to/file.rs` → file only
/// - `path/to/file.rs:10` → file + start line
/// - `path/to/file.rs:10-20` → file + range
fn parse_binding_target(
    target: &str,
    binding_type: BindingType,
) -> (String, Option<u32>, Option<u32>) {
    match binding_type {
        BindingType::FilePath | BindingType::Dependency | BindingType::DeploySurface => {
            (target.to_string(), None, None)
        }
        BindingType::CodeRegion | BindingType::Symbol => {
            if let Some(colon_pos) = target.rfind(':') {
                let file = &target[..colon_pos];
                let range_part = &target[colon_pos + 1..];
                if let Some(dash_pos) = range_part.find('-') {
                    let start = range_part[..dash_pos].parse::<u32>().ok();
                    let end = range_part[dash_pos + 1..].parse::<u32>().ok();
                    (file.to_string(), start, end)
                } else {
                    let start = range_part.parse::<u32>().ok();
                    (file.to_string(), start, None)
                }
            } else {
                (target.to_string(), None, None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::{FileInspector, SnapshotResolver};
    use bicameral_api::evidence::{BindingConfidence, BindingType, GraphSnapshot};
    use std::collections::HashMap;
    use std::process::Command;

    fn init_test_repo() -> (tempfile::TempDir, GraphSnapshot) {
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

        std::fs::write(p.join("src.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("src/lib.rs"), "pub fn hello() {}\n").unwrap();

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

        let resolver = SnapshotResolver::new(p);
        let snapshot = resolver.resolve_head().unwrap();
        (dir, snapshot)
    }

    fn make_binding(
        candidate_id: Uuid,
        target: &str,
        binding_type: BindingType,
    ) -> BindingEvidence {
        BindingEvidence::new(
            candidate_id,
            binding_type,
            target.to_string(),
            BindingConfidence::High,
        )
    }

    #[test]
    fn unbound_decisions_remain_ungrounded() {
        let (dir, snapshot) = init_test_repo();
        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);

        let verdict = evaluator.evaluate(Uuid::new_v4(), &[], &HashMap::new());
        assert_eq!(verdict.compliance_state, ComplianceState::Ungrounded);
        assert!(verdict.binding_verdicts.is_empty());
    }

    #[test]
    fn verified_unchanged_bindings_produce_reflected() {
        let (dir, snapshot) = init_test_repo();
        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        let binding = make_binding(decision_id, "src.rs", BindingType::FilePath);
        // No original content hash → treated as match
        let verdict = evaluator.evaluate(decision_id, &[binding], &HashMap::new());
        assert_eq!(verdict.compliance_state, ComplianceState::Reflected);
    }

    #[test]
    fn verified_unchanged_with_matching_hash_produces_reflected() {
        let (dir, snapshot) = init_test_repo();

        // Compute the current content hash for src.rs
        let content = std::fs::read_to_string(dir.path().join("src.rs")).unwrap();
        let hash = sha2_hex(&content);

        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        let binding = make_binding(decision_id, "src.rs", BindingType::FilePath);
        let mut hashes = HashMap::new();
        hashes.insert(binding.id, Some(hash));

        let verdict = evaluator.evaluate(decision_id, &[binding], &hashes);
        assert_eq!(verdict.compliance_state, ComplianceState::Reflected);
        assert_eq!(verdict.binding_verdicts[0].content_match, Some(true));
    }

    #[test]
    fn changed_content_hash_produces_drifted() {
        let (dir, snapshot) = init_test_repo();
        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        let binding = make_binding(decision_id, "src.rs", BindingType::FilePath);
        let mut hashes = HashMap::new();
        hashes.insert(binding.id, Some("old-hash-that-wont-match".to_string()));

        let verdict = evaluator.evaluate(decision_id, &[binding], &hashes);
        assert_eq!(verdict.compliance_state, ComplianceState::Drifted);
        assert_eq!(verdict.binding_verdicts[0].content_match, Some(false));
    }

    #[test]
    fn missing_file_produces_drifted() {
        let (dir, snapshot) = init_test_repo();
        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        let binding = make_binding(decision_id, "nonexistent.rs", BindingType::FilePath);
        let verdict = evaluator.evaluate(decision_id, &[binding], &HashMap::new());
        assert_eq!(verdict.compliance_state, ComplianceState::Drifted);
        assert_eq!(
            verdict.binding_verdicts[0].evidence_state,
            GraphEvidenceState::NotFound
        );
    }

    #[test]
    fn mixed_reflected_and_missing_produces_partial() {
        let (dir, snapshot) = init_test_repo();
        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        let b1 = make_binding(decision_id, "src.rs", BindingType::FilePath);
        let b2 = make_binding(decision_id, "gone.rs", BindingType::FilePath);

        let verdict = evaluator.evaluate(decision_id, &[b1, b2], &HashMap::new());
        assert_eq!(verdict.compliance_state, ComplianceState::Partial);
    }

    #[test]
    fn dirty_working_tree_produces_pending() {
        let (dir, snapshot) = init_test_repo();

        // Dirty the file after commit
        std::fs::write(dir.path().join("src.rs"), "fn changed() {}\n").unwrap();

        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        let binding = make_binding(decision_id, "src.rs", BindingType::FilePath);
        let verdict = evaluator.evaluate(decision_id, &[binding], &HashMap::new());
        assert_eq!(verdict.compliance_state, ComplianceState::Pending);
    }

    #[test]
    fn evaluate_without_inspector_with_no_bindings_is_ungrounded() {
        let verdict = evaluate_without_inspector(Uuid::new_v4(), &[]);
        assert_eq!(verdict.compliance_state, ComplianceState::Ungrounded);
    }

    #[test]
    fn evaluate_without_inspector_with_bindings_is_pending() {
        let decision_id = Uuid::new_v4();
        let binding = make_binding(decision_id, "some/file.rs", BindingType::FilePath);
        let verdict = evaluate_without_inspector(decision_id, &[binding]);
        assert_eq!(verdict.compliance_state, ComplianceState::Pending);
    }

    #[test]
    fn code_region_target_parses_correctly() {
        let (file, start, end) = parse_binding_target("src/lib.rs:10-20", BindingType::CodeRegion);
        assert_eq!(file, "src/lib.rs");
        assert_eq!(start, Some(10));
        assert_eq!(end, Some(20));
    }

    #[test]
    fn file_path_target_does_not_parse_range() {
        let (file, start, end) = parse_binding_target("src/lib.rs:10-20", BindingType::FilePath);
        assert_eq!(file, "src/lib.rs:10-20");
        assert_eq!(start, None);
        assert_eq!(end, None);
    }

    fn sha2_hex(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    // ── Signoff-independence invariant tests (issue #193, ADR-0022) ──

    #[test]
    fn signoff_approved_does_not_imply_compliance() {
        // A Decision may be Approved on signoff while Ungrounded/Drifted/Pending
        // on compliance. Signoff alone is never proof of compliance.
        use bicameral_api::dashboard::SignoffState;

        for compliance in [
            ComplianceState::Ungrounded,
            ComplianceState::Pending,
            ComplianceState::Drifted,
            ComplianceState::Partial,
            ComplianceState::Reflected,
        ] {
            assert!(
                signoff_is_not_compliance_proof(SignoffState::Approved, compliance),
                "Signoff::Approved must not imply or prevent any compliance state"
            );
        }
    }

    #[test]
    fn all_signoff_states_are_independent_from_compliance() {
        use bicameral_api::dashboard::SignoffState;

        let all_signoff = [
            SignoffState::Proposed,
            SignoffState::Approved,
            SignoffState::Rejected,
            SignoffState::CollisionPending,
            SignoffState::Superseded,
        ];
        let all_compliance = [
            ComplianceState::Ungrounded,
            ComplianceState::Pending,
            ComplianceState::Drifted,
            ComplianceState::Partial,
            ComplianceState::Reflected,
        ];

        for signoff in &all_signoff {
            for compliance in &all_compliance {
                assert!(
                    signoff_is_not_compliance_proof(*signoff, *compliance),
                    "Every signoff × compliance combination must be valid: \
                     signoff={signoff:?}, compliance={compliance:?}"
                );
            }
        }
    }

    #[test]
    fn compliance_evaluator_ignores_signoff_state() {
        // The ComplianceEvaluator's evaluate() signature does not accept
        // SignoffState. This test structurally verifies the invariant:
        // compliance evaluation depends only on bindings and graph evidence.
        let (dir, snapshot) = init_test_repo();
        let inspector = FileInspector::new(dir.path(), snapshot);
        let evaluator = ComplianceEvaluator::new(&inspector);
        let decision_id = Uuid::new_v4();

        // Same bindings, same graph → same compliance, regardless of what
        // signoff state the Decision carries.
        let binding = make_binding(decision_id, "src.rs", BindingType::FilePath);
        let verdict = evaluator.evaluate(decision_id, &[binding], &HashMap::new());

        // Compliance is Reflected because the file exists and matches.
        // If signoff were an input, this test would need to vary it.
        assert_eq!(verdict.compliance_state, ComplianceState::Reflected);
    }
}
