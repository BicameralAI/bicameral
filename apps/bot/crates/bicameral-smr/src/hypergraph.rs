use crate::{
    AcceptedModelBase, ArtifactAuthorityV1, AtomBinding, ClaimEdgeKind, CoverageStatus, Modality,
    OccurrenceKind, Polarity, Quantifier, SemanticRelationKind, SourceSpan, ValidatedClaimTrace,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const HYPERGRAPH_LOWERING_VERSION: &str = "claim-trace-hypergraph-alpha-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateHypergraphV1 {
    pub graph_id: String,
    pub authority: ArtifactAuthorityV1,
    pub source_trace_digest: String,
    pub accepted_model_base: AcceptedModelBase,
    pub nodes: Vec<HypergraphNodeV1>,
    pub assertions: Vec<HypergraphAssertionV1>,
    pub relations: Vec<HypergraphRelationV1>,
    pub coverage_status: CoverageStatus,
    pub coverage_unknown_dimensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HypergraphNodeV1 {
    pub node_id: String,
    pub kind: OccurrenceKind,
    pub atom_binding: AtomBinding,
    pub unknown_dimensions: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HypergraphAssertionV1 {
    pub assertion_id: String,
    pub actor_node_id: Option<String>,
    pub action_node_id: String,
    pub target_node_id: Option<String>,
    pub modality: Modality,
    pub polarity: Polarity,
    pub quantifier: Quantifier,
    pub pre_state_node_id: Option<String>,
    pub post_state_node_id: Option<String>,
    pub guard_node_ids: Vec<String>,
    pub effect_node_ids: Vec<String>,
    pub exception_assertion_ids: Vec<String>,
    pub unknown_dimensions: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypergraphRelationKind {
    Precedes,
    Requires,
    Enables,
    CausesTransition,
    ConditionalOn,
    AlternativeTo,
    Excludes,
    EquivalentTo,
    DistinctFrom,
}

impl From<ClaimEdgeKind> for HypergraphRelationKind {
    fn from(value: ClaimEdgeKind) -> Self {
        match value {
            ClaimEdgeKind::Precedes => Self::Precedes,
            ClaimEdgeKind::Requires => Self::Requires,
            ClaimEdgeKind::Enables => Self::Enables,
            ClaimEdgeKind::CausesTransition => Self::CausesTransition,
            ClaimEdgeKind::ConditionalOn => Self::ConditionalOn,
            ClaimEdgeKind::AlternativeTo => Self::AlternativeTo,
            ClaimEdgeKind::Excludes => Self::Excludes,
        }
    }
}

impl From<SemanticRelationKind> for HypergraphRelationKind {
    fn from(value: SemanticRelationKind) -> Self {
        match value {
            SemanticRelationKind::EquivalentTo => Self::EquivalentTo,
            SemanticRelationKind::DistinctFrom => Self::DistinctFrom,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HypergraphRelationV1 {
    pub relation_id: String,
    pub from_id: String,
    pub to_id: String,
    pub relation: HypergraphRelationKind,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoweringStatus {
    Lowered,
    LoweredWithUnknowns,
    Ambiguous,
    Unsupported,
    OmittedOutOfScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LossManifestEntryV1 {
    pub semantic_id: String,
    pub status: LoweringStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoweredClaimTraceV1 {
    pub hypergraph: CandidateHypergraphV1,
    pub loss_manifest: Vec<LossManifestEntryV1>,
}

pub fn lower_claim_trace(validated: &ValidatedClaimTrace) -> LoweredClaimTraceV1 {
    let trace = &validated.trace;

    let mut nodes = trace
        .occurrences
        .iter()
        .map(|occurrence| HypergraphNodeV1 {
            node_id: occurrence.occurrence_id.clone(),
            kind: occurrence.kind,
            atom_binding: occurrence.atom_binding.clone(),
            unknown_dimensions: occurrence.unknown_dimensions.clone(),
            source_spans: validated
                .source_spans_for(&occurrence.occurrence_id)
                .expect("validated occurrences always have provenance")
                .to_vec(),
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));

    let mut assertions = trace
        .assertions
        .iter()
        .map(|assertion| HypergraphAssertionV1 {
            assertion_id: assertion.assertion_id.clone(),
            actor_node_id: assertion.actor_occurrence_id.clone(),
            action_node_id: assertion.action_occurrence_id.clone(),
            target_node_id: assertion.target_occurrence_id.clone(),
            modality: assertion.modality,
            polarity: assertion.polarity,
            quantifier: assertion.quantifier,
            pre_state_node_id: assertion.pre_state_occurrence_id.clone(),
            post_state_node_id: assertion.post_state_occurrence_id.clone(),
            guard_node_ids: assertion.guard_occurrence_ids.clone(),
            effect_node_ids: assertion.effect_occurrence_ids.clone(),
            exception_assertion_ids: assertion.exception_assertion_ids.clone(),
            unknown_dimensions: assertion.unknown_dimensions.clone(),
            source_spans: validated
                .source_spans_for(&assertion.assertion_id)
                .expect("validated assertions always have provenance")
                .to_vec(),
        })
        .collect::<Vec<_>>();
    assertions.sort_by(|left, right| left.assertion_id.cmp(&right.assertion_id));

    let mut relations = trace
        .edges
        .iter()
        .map(|edge| HypergraphRelationV1 {
            relation_id: edge.edge_id.clone(),
            from_id: edge.from_assertion_id.clone(),
            to_id: edge.to_assertion_id.clone(),
            relation: edge.relation.into(),
            source_spans: validated
                .source_spans_for(&edge.edge_id)
                .expect("validated edges always have provenance")
                .to_vec(),
        })
        .chain(trace.semantic_relations.iter().map(|relation| {
            HypergraphRelationV1 {
                relation_id: relation.relation_id.clone(),
                from_id: relation.left_occurrence_id.clone(),
                to_id: relation.right_occurrence_id.clone(),
                relation: relation.relation.into(),
                source_spans: validated
                    .source_spans_for(&relation.relation_id)
                    .expect("validated semantic relations always have provenance")
                    .to_vec(),
            }
        }))
        .collect::<Vec<_>>();
    relations.sort_by(|left, right| left.relation_id.cmp(&right.relation_id));

    let mut loss_manifest = trace
        .occurrences
        .iter()
        .map(|occurrence| {
            let (status, reason) = match &occurrence.atom_binding {
                AtomBinding::Unresolved => (
                    LoweringStatus::Unsupported,
                    Some("atom binding is unresolved".into()),
                ),
                AtomBinding::Alternatives { .. } => (
                    LoweringStatus::Ambiguous,
                    Some("atom binding retains alternatives".into()),
                ),
                _ if !occurrence.unknown_dimensions.is_empty() => (
                    LoweringStatus::LoweredWithUnknowns,
                    Some("occurrence retains typed unknown dimensions".into()),
                ),
                _ => (LoweringStatus::Lowered, None),
            };
            LossManifestEntryV1 {
                semantic_id: occurrence.occurrence_id.clone(),
                status,
                reason,
            }
        })
        .chain(trace.assertions.iter().map(|assertion| {
            let has_unknowns = !assertion.unknown_dimensions.is_empty()
                || assertion.modality == Modality::Unknown
                || assertion.polarity == Polarity::Unknown
                || assertion.quantifier == Quantifier::Unspecified;
            LossManifestEntryV1 {
                semantic_id: assertion.assertion_id.clone(),
                status: if has_unknowns {
                    LoweringStatus::LoweredWithUnknowns
                } else {
                    LoweringStatus::Lowered
                },
                reason: has_unknowns.then(|| "assertion retains typed unknown dimensions".into()),
            }
        }))
        .chain(trace.edges.iter().map(|edge| LossManifestEntryV1 {
            semantic_id: edge.edge_id.clone(),
            status: LoweringStatus::Lowered,
            reason: None,
        }))
        .chain(
            trace
                .semantic_relations
                .iter()
                .map(|relation| LossManifestEntryV1 {
                    semantic_id: relation.relation_id.clone(),
                    status: LoweringStatus::Lowered,
                    reason: None,
                }),
        )
        .chain(std::iter::once({
            let has_unknowns = trace.coverage.status != CoverageStatus::Complete
                || !trace.coverage.unknown_dimensions.is_empty();
            LossManifestEntryV1 {
                semantic_id: "coverage".into(),
                status: if has_unknowns {
                    LoweringStatus::LoweredWithUnknowns
                } else {
                    LoweringStatus::Lowered
                },
                reason: has_unknowns.then(|| {
                    format!(
                        "coverage is {:?}; unknown dimensions: {}",
                        trace.coverage.status,
                        trace.coverage.unknown_dimensions.join(", ")
                    )
                }),
            }
        }))
        .collect::<Vec<_>>();
    loss_manifest.sort_by(|left, right| left.semantic_id.cmp(&right.semantic_id));

    let graph_id = graph_id(&trace.result_digest);
    LoweredClaimTraceV1 {
        hypergraph: CandidateHypergraphV1 {
            graph_id,
            authority: ArtifactAuthorityV1::NonCanonicalCandidate,
            source_trace_digest: trace.result_digest.clone(),
            accepted_model_base: trace.accepted_model_base.clone(),
            nodes,
            assertions,
            relations,
            coverage_status: trace.coverage.status,
            coverage_unknown_dimensions: trace.coverage.unknown_dimensions.clone(),
        },
        loss_manifest,
    }
}

fn graph_id(trace_digest: &str) -> String {
    let digest = Sha256::digest(format!("{HYPERGRAPH_LOWERING_VERSION}:{trace_digest}"));
    format!("SMR-HG-{digest:x}")
}
