//! Exact Alloy snapshots for one source-grounded Boolean feedback proposition.

use crate::{
    AlloyExactCardinalitiesV1, ArtifactAuthorityV1, FeedbackBaselineAuthorityV1,
    FeedbackCheckPairPlanV1, FeedbackCheckRoleV1, ReviewedExperimentBoardV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const FEEDBACK_ALLOY_SERIALIZER_VERSION: &str = "smr-feedback-alloy-snapshot-v1";
pub(crate) const FEEDBACK_ALLOY_META_MODEL: &str =
    include_str!("../alloy/smr_feedback_meta_model.als");
pub(crate) const FEEDBACK_ALLOY_CHECKS: &str = include_str!("../alloy/smr_feedback_checks.als");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackBooleanPropositionV1 {
    pub accepted_assertion_ref: String,
    pub asserted_value: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackAlloySnapshotV1 {
    pub snapshot_id: String,
    pub authority: ArtifactAuthorityV1,
    pub serializer_version: String,
    pub meta_model_digest: String,
    pub checks_digest: String,
    pub baseline_fixture_file_sha256: String,
    pub baseline_head_commit: String,
    pub baseline_merged_commit: String,
    pub baseline_claim_set_digest: String,
    pub scope_id: String,
    pub scope_completeness: String,
    pub coverage_limitations: Vec<String>,
    pub feedback_assertion_id: String,
    pub check_role: FeedbackCheckRoleV1,
    pub proposition_atom_id: String,
    pub proposition_value: bool,
    pub accepted_claim_refs: Vec<String>,
    pub accepted_atom_refs: Vec<String>,
    pub accepted_relation_refs: Vec<String>,
    pub source_anchor_refs: Vec<String>,
    pub exact_cardinalities: AlloyExactCardinalitiesV1,
    pub alloy_module: String,
    pub result_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackAlloySnapshotPairV1 {
    pub assertion: FeedbackAlloySnapshotV1,
    pub negated_assertion: FeedbackAlloySnapshotV1,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FeedbackAlloySerializationError {
    #[error("feedback check plan does not match reviewed board field {field}")]
    PlanIdentityMismatch { field: &'static str },
    #[error("accepted assertion reference is unknown: {0}")]
    UnknownAssertion(String),
    #[error("accepted assertion {0} is outside the source-grounded witness plan")]
    PropositionOutsideWitness(String),
    #[error("accepted assertion {0} is not a supported Boolean proposition")]
    UnsupportedProposition(String),
    #[error("reviewed experiment board projection is malformed: {0}")]
    MalformedBoard(String),
}

#[derive(Deserialize)]
struct Projection {
    model_atoms: Vec<ModelAtom>,
    claim_assertions: Vec<ClaimAssertion>,
    atom_relations: Vec<AtomRelation>,
}

#[derive(Deserialize)]
struct ModelAtom {
    atom_id: String,
    atom_kind: String,
}

#[derive(Deserialize)]
struct ClaimAssertion {
    assertion_id: String,
    claim_id: String,
    atom_id: String,
    polarity: String,
    value: AssertionValue,
}

#[derive(Deserialize)]
struct AssertionValue {
    kind: String,
    boolean: Option<bool>,
}

#[derive(Deserialize)]
struct AtomRelation {
    relation_id: String,
    relation_kind: String,
    from_atom_id: String,
    to_atom_id: String,
}

pub fn serialize_feedback_alloy_snapshots(
    board: &ReviewedExperimentBoardV1,
    plan: &FeedbackCheckPairPlanV1,
    proposition: FeedbackBooleanPropositionV1,
) -> Result<FeedbackAlloySnapshotPairV1, FeedbackAlloySerializationError> {
    validate_plan_identity(board, plan)?;
    let mut projection: Projection =
        serde_json::from_value(board.normalized_claim_set().clone())
            .map_err(|error| FeedbackAlloySerializationError::MalformedBoard(error.to_string()))?;
    projection
        .model_atoms
        .sort_by(|a, b| a.atom_id.cmp(&b.atom_id));
    projection
        .claim_assertions
        .sort_by(|a, b| a.assertion_id.cmp(&b.assertion_id));
    projection
        .atom_relations
        .sort_by(|a, b| a.relation_id.cmp(&b.relation_id));

    let assertion = projection
        .claim_assertions
        .iter()
        .find(|item| item.assertion_id == proposition.accepted_assertion_ref)
        .ok_or_else(|| {
            FeedbackAlloySerializationError::UnknownAssertion(
                proposition.accepted_assertion_ref.clone(),
            )
        })?;
    if !plan.accepted_claim_refs.contains(&assertion.claim_id)
        || !plan.accepted_atom_refs.contains(&assertion.atom_id)
    {
        return Err(FeedbackAlloySerializationError::PropositionOutsideWitness(
            assertion.assertion_id.clone(),
        ));
    }
    if assertion.polarity != "asserts"
        || assertion.value.kind != "boolean"
        || assertion.value.boolean.is_none()
    {
        return Err(FeedbackAlloySerializationError::UnsupportedProposition(
            assertion.assertion_id.clone(),
        ));
    }

    let assertion_snapshot = render_snapshot(
        board,
        plan,
        &projection,
        &assertion.atom_id,
        FeedbackCheckRoleV1::Assertion,
        proposition.asserted_value,
    )?;
    let negated_snapshot = render_snapshot(
        board,
        plan,
        &projection,
        &assertion.atom_id,
        FeedbackCheckRoleV1::NegatedAssertion,
        !proposition.asserted_value,
    )?;
    Ok(FeedbackAlloySnapshotPairV1 {
        assertion: assertion_snapshot,
        negated_assertion: negated_snapshot,
    })
}

fn validate_plan_identity(
    board: &ReviewedExperimentBoardV1,
    plan: &FeedbackCheckPairPlanV1,
) -> Result<(), FeedbackAlloySerializationError> {
    let matches = [
        (
            "baseline_fixture_file_sha256",
            plan.baseline_fixture_file_sha256 == board.fixture_file_sha256(),
        ),
        (
            "baseline_head_commit",
            plan.baseline_head_commit == board.fixture_head_commit(),
        ),
        (
            "baseline_merged_commit",
            plan.baseline_merged_commit == board.merged_commit(),
        ),
        (
            "baseline_claim_set_digest",
            plan.baseline_claim_set_digest == board.claim_set_digest(),
        ),
        ("scope_id", plan.scope_id == board.scope_id()),
        (
            "scope_completeness",
            plan.scope_completeness == board.scope_completeness(),
        ),
        (
            "coverage_limitations",
            plan.coverage_limitations == board.scope_exclusions(),
        ),
        (
            "baseline_authority",
            plan.baseline_authority == FeedbackBaselineAuthorityV1::NonAuthoritativeExperiment,
        ),
        (
            "checks",
            plan.checks
                == [
                    FeedbackCheckRoleV1::Assertion,
                    FeedbackCheckRoleV1::NegatedAssertion,
                ],
        ),
    ];
    for (field, matches) in matches {
        if !matches {
            return Err(FeedbackAlloySerializationError::PlanIdentityMismatch { field });
        }
    }
    Ok(())
}

fn render_snapshot(
    board: &ReviewedExperimentBoardV1,
    plan: &FeedbackCheckPairPlanV1,
    projection: &Projection,
    proposition_atom_id: &str,
    check_role: FeedbackCheckRoleV1,
    proposition_value: bool,
) -> Result<FeedbackAlloySnapshotV1, FeedbackAlloySerializationError> {
    let alloy_module = render_module(projection, proposition_atom_id, proposition_value)?;
    let mut snapshot = FeedbackAlloySnapshotV1 {
        snapshot_id: String::new(),
        authority: ArtifactAuthorityV1::NonCanonicalProjection,
        serializer_version: FEEDBACK_ALLOY_SERIALIZER_VERSION.into(),
        meta_model_digest: digest_text(FEEDBACK_ALLOY_META_MODEL),
        checks_digest: digest_text(FEEDBACK_ALLOY_CHECKS),
        baseline_fixture_file_sha256: board.fixture_file_sha256().into(),
        baseline_head_commit: board.fixture_head_commit().into(),
        baseline_merged_commit: board.merged_commit().into(),
        baseline_claim_set_digest: board.claim_set_digest().into(),
        scope_id: board.scope_id().into(),
        scope_completeness: board.scope_completeness().into(),
        coverage_limitations: board.scope_exclusions().to_vec(),
        feedback_assertion_id: plan.assertion_id.clone(),
        check_role,
        proposition_atom_id: proposition_atom_id.into(),
        proposition_value,
        accepted_claim_refs: plan.accepted_claim_refs.clone(),
        accepted_atom_refs: plan.accepted_atom_refs.clone(),
        accepted_relation_refs: plan.accepted_relation_refs.clone(),
        source_anchor_refs: plan.source_anchor_refs.clone(),
        exact_cardinalities: AlloyExactCardinalitiesV1 {
            model_atoms: projection.model_atoms.len(),
            claim_assertions: projection.claim_assertions.len(),
            atom_relations: projection.atom_relations.len(),
        },
        alloy_module,
        result_digest: String::new(),
    };
    snapshot.result_digest = compute_feedback_snapshot_digest(&snapshot);
    snapshot.snapshot_id = format!(
        "SMR-FEEDBACK-SNAPSHOT-{}",
        snapshot.result_digest.trim_start_matches("sha256:")
    );
    Ok(snapshot)
}

fn render_module(
    projection: &Projection,
    proposition_atom_id: &str,
    proposition_value: bool,
) -> Result<String, FeedbackAlloySerializationError> {
    let atoms = projection
        .model_atoms
        .iter()
        .map(|atom| (atom.atom_id.clone(), stable_symbol("N", &atom.atom_id)))
        .collect::<BTreeMap<_, _>>();
    let assertions = projection
        .claim_assertions
        .iter()
        .map(|item| {
            (
                item.assertion_id.clone(),
                stable_symbol("A", &item.assertion_id),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let relations = projection
        .atom_relations
        .iter()
        .map(|item| {
            (
                item.relation_id.clone(),
                stable_symbol("R", &item.relation_id),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let proposed = atoms.get(proposition_atom_id).ok_or_else(|| {
        FeedbackAlloySerializationError::MalformedBoard(format!(
            "proposition references missing atom {proposition_atom_id}"
        ))
    })?;
    let mut output = String::from(
        "module smr_feedback_snapshot\n\nopen smr_feedback_meta_model\nopen smr_feedback_checks\n\n",
    );
    for atom in &projection.model_atoms {
        output.push_str(&format!(
            "one sig {} extends ModelAtom {{}}\n",
            atoms[&atom.atom_id]
        ));
    }
    for item in &projection.claim_assertions {
        output.push_str(&format!(
            "one sig {} extends AcceptedAssertion {{}}\n",
            assertions[&item.assertion_id]
        ));
    }
    for item in &projection.atom_relations {
        output.push_str(&format!(
            "one sig {} extends AtomRelation {{}}\n",
            relations[&item.relation_id]
        ));
    }
    output.push_str("\nfact ExactFeedbackSnapshot {\n");
    output.push_str(&format!("  ModelAtom = {}\n", union(atoms.values())));
    output.push_str(&format!(
        "  AcceptedAssertion = {}\n",
        union(assertions.values())
    ));
    output.push_str(&format!("  AtomRelation = {}\n", union(relations.values())));
    for atom in &projection.model_atoms {
        output.push_str(&format!(
            "  {}.atomKind = {}\n",
            atoms[&atom.atom_id],
            atom_kind(&atom.atom_kind)?
        ));
    }
    for item in &projection.claim_assertions {
        let value = item.value.boolean.ok_or_else(|| {
            FeedbackAlloySerializationError::UnsupportedProposition(item.assertion_id.clone())
        })?;
        if item.polarity != "asserts" || item.value.kind != "boolean" {
            return Err(FeedbackAlloySerializationError::UnsupportedProposition(
                item.assertion_id.clone(),
            ));
        }
        output.push_str(&format!(
            "  {}.subject = {}\n  {}.truth = {}\n",
            assertions[&item.assertion_id],
            atoms[&item.atom_id],
            assertions[&item.assertion_id],
            truth(value)
        ));
    }
    for item in &projection.atom_relations {
        output.push_str(&format!(
            "  {}.fromAtom = {}\n  {}.toAtom = {}\n  {}.relationKind = {}\n",
            relations[&item.relation_id],
            atoms[&item.from_atom_id],
            relations[&item.relation_id],
            atoms[&item.to_atom_id],
            relations[&item.relation_id],
            relation_kind(&item.relation_kind)?
        ));
    }
    output.push_str(&format!(
        "  ProposedAssertion.subject = {proposed}\n  ProposedAssertion.truth = {}\n}}\n\n",
        truth(proposition_value)
    ));
    output.push_str(&format!(
        "run FeedbackCompatibility for exactly {} ModelAtom, exactly {} AcceptedAssertion, exactly {} AtomRelation\n",
        atoms.len(), assertions.len(), relations.len()
    ));
    Ok(output)
}

fn atom_kind(value: &str) -> Result<&'static str, FeedbackAlloySerializationError> {
    match value {
        "action" => Ok("ActionKind"),
        "event" => Ok("EventKind"),
        "invariant" => Ok("InvariantKind"),
        "state" => Ok("StateKind"),
        "term" => Ok("TermKind"),
        "transition" => Ok("TransitionKind"),
        other => Err(FeedbackAlloySerializationError::MalformedBoard(format!(
            "unsupported atom kind {other}"
        ))),
    }
}

fn relation_kind(value: &str) -> Result<&'static str, FeedbackAlloySerializationError> {
    match value {
        "precedes" => Ok("Precedes"),
        "guards" => Ok("Guards"),
        "triggers" => Ok("Triggers"),
        "produces" => Ok("Produces"),
        "part_of" => Ok("PartOf"),
        other => Err(FeedbackAlloySerializationError::MalformedBoard(format!(
            "unsupported relation kind {other}"
        ))),
    }
}

fn truth(value: bool) -> &'static str {
    if value {
        "TrueValue"
    } else {
        "FalseValue"
    }
}

fn stable_symbol(prefix: &str, semantic_id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(semantic_id));
    format!("{prefix}_{}", &digest[..16])
}

fn digest_text(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

fn union<'a>(symbols: impl IntoIterator<Item = &'a String>) -> String {
    symbols
        .into_iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" + ")
}

pub(crate) fn compute_feedback_snapshot_digest(snapshot: &FeedbackAlloySnapshotV1) -> String {
    let mut digest_input = snapshot.clone();
    digest_input.snapshot_id.clear();
    digest_input.result_digest.clear();
    let bytes = serde_json::to_vec(&digest_input)
        .expect("feedback snapshot identity serialization is infallible");
    format!("sha256:{:x}", Sha256::digest(bytes))
}
