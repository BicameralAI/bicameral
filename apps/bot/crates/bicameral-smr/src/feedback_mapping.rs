//! Explicit witness mapping for one advisory feedback assertion.

use crate::ReviewedExperimentBoardV1;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackAssertionWitnessRequestV1 {
    pub assertion_id: String,
    pub accepted_claim_refs: Vec<String>,
    pub accepted_atom_refs: Vec<String>,
    pub accepted_relation_refs: Vec<String>,
    pub source_anchor_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackCheckRoleV1 {
    Assertion,
    NegatedAssertion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackBaselineAuthorityV1 {
    NonAuthoritativeExperiment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackCheckPairPlanV1 {
    pub assertion_id: String,
    pub baseline_fixture_file_sha256: String,
    pub baseline_head_commit: String,
    pub baseline_merged_commit: String,
    pub baseline_claim_set_digest: String,
    pub baseline_authority: FeedbackBaselineAuthorityV1,
    pub scope_id: String,
    pub scope_completeness: String,
    pub coverage_limitations: Vec<String>,
    pub accepted_claim_refs: Vec<String>,
    pub accepted_atom_refs: Vec<String>,
    pub accepted_relation_refs: Vec<String>,
    pub source_anchor_refs: Vec<String>,
    pub checks: [FeedbackCheckRoleV1; 2],
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FeedbackMappingError {
    #[error("feedback assertion_id must use a sha256 content identity")]
    InvalidAssertionId,
    #[error("{0} must contain at least one exact reference")]
    EmptyReferences(&'static str),
    #[error("duplicate {collection} reference: {reference}")]
    DuplicateReference {
        collection: &'static str,
        reference: String,
    },
    #[error("unknown {collection} reference: {reference}")]
    UnknownReference {
        collection: &'static str,
        reference: String,
    },
    #[error("unsupported accepted relation kind {relation_kind} on {relation_id}")]
    UnsupportedRelation {
        relation_id: String,
        relation_kind: String,
    },
    #[error("relation {relation_id} requires mapped {collection} reference {reference}")]
    IncompleteRelationWitness {
        relation_id: String,
        collection: &'static str,
        reference: String,
    },
    #[error("Claim {claim_id} requires mapped {collection} reference {reference}")]
    IncompleteClaimWitness {
        claim_id: String,
        collection: &'static str,
        reference: String,
    },
    #[error("mapped {collection} reference is not connected to a selected Claim or relation: {reference}")]
    UnconnectedReference {
        collection: &'static str,
        reference: String,
    },
    #[error("reviewed experiment board projection is malformed: {0}")]
    MalformedBoard(String),
}

#[derive(Deserialize)]
struct Projection {
    claims: Vec<Claim>,
    model_atoms: Vec<ModelAtom>,
    claim_assertions: Vec<ClaimAssertion>,
    atom_relations: Vec<AtomRelation>,
    source_anchors: Vec<SourceAnchor>,
}

#[derive(Deserialize)]
struct Claim {
    claim_id: String,
    assertion_ids: Vec<String>,
    source_anchor_ids: Vec<String>,
}

#[derive(Deserialize)]
struct ModelAtom {
    atom_id: String,
}

#[derive(Deserialize)]
struct ClaimAssertion {
    assertion_id: String,
    atom_id: String,
}

#[derive(Deserialize)]
struct AtomRelation {
    relation_id: String,
    relation_kind: String,
    from_atom_id: String,
    to_atom_id: String,
    supporting_claim_ids: Vec<String>,
    supporting_source_anchor_ids: Vec<String>,
}

#[derive(Deserialize)]
struct SourceAnchor {
    anchor_id: String,
}

pub fn plan_feedback_check_pair(
    board: &ReviewedExperimentBoardV1,
    request: FeedbackAssertionWitnessRequestV1,
) -> Result<FeedbackCheckPairPlanV1, FeedbackMappingError> {
    if !is_sha256(&request.assertion_id) {
        return Err(FeedbackMappingError::InvalidAssertionId);
    }

    let claim_refs = exact_refs("accepted_claim_refs", request.accepted_claim_refs, true)?;
    let atom_refs = exact_refs("accepted_atom_refs", request.accepted_atom_refs, true)?;
    let relation_refs = exact_refs(
        "accepted_relation_refs",
        request.accepted_relation_refs,
        false,
    )?;
    let anchor_refs = exact_refs("source_anchor_refs", request.source_anchor_refs, true)?;
    let projection: Projection = serde_json::from_value(board.normalized_claim_set().clone())
        .map_err(|error| FeedbackMappingError::MalformedBoard(error.to_string()))?;

    let claims = projection
        .claims
        .iter()
        .map(|claim| (claim.claim_id.as_str(), claim))
        .collect::<BTreeMap<_, _>>();
    let assertions = projection
        .claim_assertions
        .iter()
        .map(|assertion| (assertion.assertion_id.as_str(), assertion))
        .collect::<BTreeMap<_, _>>();
    let relations = projection
        .atom_relations
        .iter()
        .map(|relation| (relation.relation_id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    let known_atoms = projection
        .model_atoms
        .iter()
        .map(|atom| atom.atom_id.as_str())
        .collect::<BTreeSet<_>>();
    let known_anchors = projection
        .source_anchors
        .iter()
        .map(|anchor| anchor.anchor_id.as_str())
        .collect::<BTreeSet<_>>();

    require_known("accepted_claim_refs", &claim_refs, claims.keys().copied())?;
    require_known(
        "accepted_atom_refs",
        &atom_refs,
        known_atoms.iter().copied(),
    )?;
    require_known(
        "accepted_relation_refs",
        &relation_refs,
        relations.keys().copied(),
    )?;
    require_known(
        "source_anchor_refs",
        &anchor_refs,
        known_anchors.iter().copied(),
    )?;

    let mut connected_atoms = BTreeSet::new();
    let mut connected_anchors = BTreeSet::new();
    for claim_ref in &claim_refs {
        let claim = claims[claim_ref.as_str()];
        for assertion_id in &claim.assertion_ids {
            let assertion = assertions.get(assertion_id.as_str()).ok_or_else(|| {
                FeedbackMappingError::MalformedBoard(format!(
                    "Claim {claim_ref} references missing assertion {assertion_id}"
                ))
            })?;
            if !atom_refs.contains(&assertion.atom_id) {
                return Err(FeedbackMappingError::IncompleteClaimWitness {
                    claim_id: claim.claim_id.clone(),
                    collection: "accepted_atom_refs",
                    reference: assertion.atom_id.clone(),
                });
            }
            connected_atoms.insert(assertion.atom_id.as_str());
        }
        for anchor_id in &claim.source_anchor_ids {
            if !anchor_refs.contains(anchor_id) {
                return Err(FeedbackMappingError::IncompleteClaimWitness {
                    claim_id: claim.claim_id.clone(),
                    collection: "source_anchor_refs",
                    reference: anchor_id.clone(),
                });
            }
        }
        connected_anchors.extend(claim.source_anchor_ids.iter().map(String::as_str));
    }

    for relation_ref in &relation_refs {
        let relation = relations[relation_ref.as_str()];
        if !matches!(
            relation.relation_kind.as_str(),
            "precedes" | "guards" | "triggers" | "produces" | "part_of"
        ) {
            return Err(FeedbackMappingError::UnsupportedRelation {
                relation_id: relation.relation_id.clone(),
                relation_kind: relation.relation_kind.clone(),
            });
        }
        require_relation_refs(relation, &claim_refs, &atom_refs, &anchor_refs)?;
        connected_atoms.insert(&relation.from_atom_id);
        connected_atoms.insert(&relation.to_atom_id);
        connected_anchors.extend(
            relation
                .supporting_source_anchor_ids
                .iter()
                .map(String::as_str),
        );
    }

    require_connected("accepted_atom_refs", &atom_refs, &connected_atoms)?;
    require_connected("source_anchor_refs", &anchor_refs, &connected_anchors)?;

    Ok(FeedbackCheckPairPlanV1 {
        assertion_id: request.assertion_id,
        baseline_fixture_file_sha256: board.fixture_file_sha256().into(),
        baseline_head_commit: board.fixture_head_commit().into(),
        baseline_merged_commit: board.merged_commit().into(),
        baseline_claim_set_digest: board.claim_set_digest().into(),
        baseline_authority: FeedbackBaselineAuthorityV1::NonAuthoritativeExperiment,
        scope_id: board.scope_id().into(),
        scope_completeness: board.scope_completeness().into(),
        coverage_limitations: board.scope_exclusions().to_vec(),
        accepted_claim_refs: claim_refs,
        accepted_atom_refs: atom_refs,
        accepted_relation_refs: relation_refs,
        source_anchor_refs: anchor_refs,
        checks: [
            FeedbackCheckRoleV1::Assertion,
            FeedbackCheckRoleV1::NegatedAssertion,
        ],
    })
}

fn require_relation_refs(
    relation: &AtomRelation,
    claims: &[String],
    atoms: &[String],
    anchors: &[String],
) -> Result<(), FeedbackMappingError> {
    for (collection, required, selected) in [
        (
            "accepted_atom_refs",
            vec![relation.from_atom_id.as_str(), relation.to_atom_id.as_str()],
            atoms,
        ),
        (
            "accepted_claim_refs",
            relation
                .supporting_claim_ids
                .iter()
                .map(String::as_str)
                .collect(),
            claims,
        ),
        (
            "source_anchor_refs",
            relation
                .supporting_source_anchor_ids
                .iter()
                .map(String::as_str)
                .collect(),
            anchors,
        ),
    ] {
        for reference in required {
            if !selected.iter().any(|selected| selected == reference) {
                return Err(FeedbackMappingError::IncompleteRelationWitness {
                    relation_id: relation.relation_id.clone(),
                    collection,
                    reference: reference.into(),
                });
            }
        }
    }
    Ok(())
}

fn exact_refs(
    collection: &'static str,
    mut references: Vec<String>,
    required: bool,
) -> Result<Vec<String>, FeedbackMappingError> {
    if required && references.is_empty() {
        return Err(FeedbackMappingError::EmptyReferences(collection));
    }
    references.sort();
    for pair in references.windows(2) {
        if pair[0] == pair[1] {
            return Err(FeedbackMappingError::DuplicateReference {
                collection,
                reference: pair[0].clone(),
            });
        }
    }
    Ok(references)
}

fn require_known<'a>(
    collection: &'static str,
    selected: &[String],
    known: impl IntoIterator<Item = &'a str>,
) -> Result<(), FeedbackMappingError> {
    let known = known.into_iter().collect::<BTreeSet<_>>();
    for reference in selected {
        if !known.contains(reference.as_str()) {
            return Err(FeedbackMappingError::UnknownReference {
                collection,
                reference: reference.clone(),
            });
        }
    }
    Ok(())
}

fn require_connected(
    collection: &'static str,
    selected: &[String],
    connected: &BTreeSet<&str>,
) -> Result<(), FeedbackMappingError> {
    for reference in selected {
        if !connected.contains(reference.as_str()) {
            return Err(FeedbackMappingError::UnconnectedReference {
                collection,
                reference: reference.clone(),
            });
        }
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
