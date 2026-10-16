//! Typed orchestration boundary for one feedback assertion evaluation attempt.

use crate::{
    analyze_feedback_snapshot_pair, plan_feedback_check_pair, serialize_feedback_alloy_snapshots,
    FeedbackAlloyAnalysisError, FeedbackAlloyAnalyzerConfig, FeedbackAlloySerializationError,
    FeedbackAssertionWitnessRequestV1, FeedbackBooleanPropositionV1, FeedbackBoundedEvaluationV1,
    FeedbackMappingError, ReviewedExperimentBoardV1,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackUnevaluatedReasonV1 {
    MappingUnavailable,
    UnsupportedSemantics,
    AnalyzerUnavailable,
    AnalyzerPinMismatch,
    AnalyzerTimeout,
    AnalyzerFailed,
    IntegrityFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackUnevaluatedV1 {
    pub assertion_id: String,
    pub baseline_fixture_file_sha256: String,
    pub baseline_head_commit: String,
    pub baseline_merged_commit: String,
    pub baseline_claim_set_digest: String,
    pub scope_id: String,
    pub scope_completeness: String,
    pub coverage_limitations: Vec<String>,
    pub accepted_claim_refs: Vec<String>,
    pub accepted_atom_refs: Vec<String>,
    pub accepted_relation_refs: Vec<String>,
    pub source_anchor_refs: Vec<String>,
    pub reason: FeedbackUnevaluatedReasonV1,
    pub recovery: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FeedbackEvaluationAttemptV1 {
    BoundedResult {
        evaluation: Box<FeedbackBoundedEvaluationV1>,
    },
    Indeterminate {
        unevaluated: FeedbackUnevaluatedV1,
    },
    Unavailable {
        unevaluated: FeedbackUnevaluatedV1,
    },
    Timeout {
        unevaluated: FeedbackUnevaluatedV1,
    },
    Malformed {
        unevaluated: FeedbackUnevaluatedV1,
    },
}

#[derive(Clone, Copy)]
enum UnevaluatedStatus {
    Indeterminate,
    Unavailable,
    Timeout,
    Malformed,
}

pub fn evaluate_feedback_proposition(
    board: &ReviewedExperimentBoardV1,
    request: FeedbackAssertionWitnessRequestV1,
    proposition: FeedbackBooleanPropositionV1,
    analyzer: &FeedbackAlloyAnalyzerConfig,
) -> FeedbackEvaluationAttemptV1 {
    let mut unevaluated = FeedbackUnevaluatedV1 {
        assertion_id: request.assertion_id.clone(),
        baseline_fixture_file_sha256: board.fixture_file_sha256().into(),
        baseline_head_commit: board.fixture_head_commit().into(),
        baseline_merged_commit: board.merged_commit().into(),
        baseline_claim_set_digest: board.claim_set_digest().into(),
        scope_id: board.scope_id().into(),
        scope_completeness: board.scope_completeness().into(),
        coverage_limitations: board.scope_exclusions().to_vec(),
        accepted_claim_refs: request.accepted_claim_refs.clone(),
        accepted_atom_refs: request.accepted_atom_refs.clone(),
        accepted_relation_refs: request.accepted_relation_refs.clone(),
        source_anchor_refs: request.source_anchor_refs.clone(),
        reason: FeedbackUnevaluatedReasonV1::MappingUnavailable,
        recovery: String::new(),
    };

    let plan = match plan_feedback_check_pair(board, request) {
        Ok(plan) => plan,
        Err(error) => return mapping_failure(unevaluated, error),
    };
    unevaluated.accepted_claim_refs = plan.accepted_claim_refs.clone();
    unevaluated.accepted_atom_refs = plan.accepted_atom_refs.clone();
    unevaluated.accepted_relation_refs = plan.accepted_relation_refs.clone();
    unevaluated.source_anchor_refs = plan.source_anchor_refs.clone();

    let snapshots = match serialize_feedback_alloy_snapshots(board, &plan, proposition) {
        Ok(snapshots) => snapshots,
        Err(error) => return serialization_failure(unevaluated, error),
    };
    match analyze_feedback_snapshot_pair(&snapshots, analyzer) {
        Ok(evaluation) => FeedbackEvaluationAttemptV1::BoundedResult {
            evaluation: Box::new(evaluation),
        },
        Err(error) => analysis_failure(unevaluated, error),
    }
}

fn mapping_failure(
    unevaluated: FeedbackUnevaluatedV1,
    error: FeedbackMappingError,
) -> FeedbackEvaluationAttemptV1 {
    match error {
        FeedbackMappingError::UnsupportedRelation { .. } => finish(
            UnevaluatedStatus::Indeterminate,
            unevaluated,
            FeedbackUnevaluatedReasonV1::UnsupportedSemantics,
            "Use a relation kind supported by the frozen SMR feedback profile.",
        ),
        FeedbackMappingError::MalformedBoard(_) => integrity_failure(unevaluated),
        _ => finish(
            UnevaluatedStatus::Indeterminate,
            unevaluated,
            FeedbackUnevaluatedReasonV1::MappingUnavailable,
            "Supply one complete, unambiguous set of accepted Claim, atom, relation, and source-anchor witnesses.",
        ),
    }
}

fn serialization_failure(
    unevaluated: FeedbackUnevaluatedV1,
    error: FeedbackAlloySerializationError,
) -> FeedbackEvaluationAttemptV1 {
    match error {
        FeedbackAlloySerializationError::UnsupportedProposition(_) => finish(
            UnevaluatedStatus::Indeterminate,
            unevaluated,
            FeedbackUnevaluatedReasonV1::UnsupportedSemantics,
            "Use an exact Boolean accepted assertion supported by the v0.1 evaluator.",
        ),
        FeedbackAlloySerializationError::UnknownAssertion(_)
        | FeedbackAlloySerializationError::PropositionOutsideWitness(_) => finish(
            UnevaluatedStatus::Indeterminate,
            unevaluated,
            FeedbackUnevaluatedReasonV1::MappingUnavailable,
            "Bind the proposition to an accepted assertion inside the validated witness set.",
        ),
        FeedbackAlloySerializationError::PlanIdentityMismatch { .. }
        | FeedbackAlloySerializationError::MalformedBoard(_) => integrity_failure(unevaluated),
    }
}

fn analysis_failure(
    unevaluated: FeedbackUnevaluatedV1,
    error: FeedbackAlloyAnalysisError,
) -> FeedbackEvaluationAttemptV1 {
    match error {
        FeedbackAlloyAnalysisError::TimedOut { .. } => finish(
            UnevaluatedStatus::Timeout,
            unevaluated,
            FeedbackUnevaluatedReasonV1::AnalyzerTimeout,
            "Retry once with the same bound inputs and analyzer pins.",
        ),
        FeedbackAlloyAnalysisError::Io(_) => finish(
            UnevaluatedStatus::Unavailable,
            unevaluated,
            FeedbackUnevaluatedReasonV1::AnalyzerUnavailable,
            "Install or restore the pinned Java 17 and Alloy 6.2.0 runtime.",
        ),
        FeedbackAlloyAnalysisError::DistributionDigestMismatch { .. }
        | FeedbackAlloyAnalysisError::AlloyVersionMismatch { .. }
        | FeedbackAlloyAnalysisError::JavaVersionMismatch { .. } => finish(
            UnevaluatedStatus::Unavailable,
            unevaluated,
            FeedbackUnevaluatedReasonV1::AnalyzerPinMismatch,
            "Restore the exact analyzer versions and distribution digest required by v0.1.",
        ),
        FeedbackAlloyAnalysisError::AnalyzerFailed { .. } => finish(
            UnevaluatedStatus::Unavailable,
            unevaluated,
            FeedbackUnevaluatedReasonV1::AnalyzerFailed,
            "Inspect the analyzer installation and retry with identical bound inputs.",
        ),
        FeedbackAlloyAnalysisError::PairMismatch { .. }
        | FeedbackAlloyAnalysisError::SnapshotDigestMismatch { .. }
        | FeedbackAlloyAnalysisError::StableArtifactDigestMismatch { .. } => {
            integrity_failure(unevaluated)
        }
    }
}

fn integrity_failure(unevaluated: FeedbackUnevaluatedV1) -> FeedbackEvaluationAttemptV1 {
    finish(
        UnevaluatedStatus::Malformed,
        unevaluated,
        FeedbackUnevaluatedReasonV1::IntegrityFailure,
        "Rebuild the evaluation from the exact reviewed baseline and bound artifacts.",
    )
}

fn finish(
    status: UnevaluatedStatus,
    mut unevaluated: FeedbackUnevaluatedV1,
    reason: FeedbackUnevaluatedReasonV1,
    recovery: &'static str,
) -> FeedbackEvaluationAttemptV1 {
    unevaluated.reason = reason;
    unevaluated.recovery = recovery.into();
    match status {
        UnevaluatedStatus::Indeterminate => {
            FeedbackEvaluationAttemptV1::Indeterminate { unevaluated }
        }
        UnevaluatedStatus::Unavailable => FeedbackEvaluationAttemptV1::Unavailable { unevaluated },
        UnevaluatedStatus::Timeout => FeedbackEvaluationAttemptV1::Timeout { unevaluated },
        UnevaluatedStatus::Malformed => FeedbackEvaluationAttemptV1::Malformed { unevaluated },
    }
}
