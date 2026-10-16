use crate::{
    ArtifactAuthorityV1, AtomBinding, HypergraphRelationKind, LoweredClaimTraceV1, LoweringStatus,
    Modality, OccurrenceKind, Polarity, Quantifier, SourceSpan,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const ALLOY_SNAPSHOT_SERIALIZER_VERSION: &str = "candidate-promotion-alloy-snapshot-v1";
pub(crate) const ALLOY_META_MODEL: &str = include_str!("../alloy/smr_meta_model.als");
pub(crate) const ALLOY_CHECKS: &str = include_str!("../alloy/smr_checks.als");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlloyCommandKind {
    Run,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlloyCommand {
    SnapshotInstance,
    NoSemanticIdentityConflict,
}

impl AlloyCommand {
    pub(crate) const fn alloy_name(self) -> &'static str {
        match self {
            Self::SnapshotInstance => "SnapshotInstance",
            Self::NoSemanticIdentityConflict => "NoSemanticIdentityConflict",
        }
    }

    pub(crate) const fn kind(self) -> AlloyCommandKind {
        match self {
            Self::SnapshotInstance => AlloyCommandKind::Run,
            Self::NoSemanticIdentityConflict => AlloyCommandKind::Check,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlloyCapacity {
    pub max_model_atoms: usize,
    pub max_claim_assertions: usize,
    pub max_atom_relations: usize,
}

impl AlloyCapacity {
    pub const fn alpha_default() -> Self {
        Self {
            max_model_atoms: 5,
            max_claim_assertions: 5,
            max_atom_relations: 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlloyExactCardinalitiesV1 {
    pub model_atoms: usize,
    pub claim_assertions: usize,
    pub atom_relations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlloyProvenanceEntryV1 {
    pub alloy_symbol: String,
    pub semantic_id: String,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlloyCheckEvidenceMapV1 {
    pub command: AlloyCommand,
    pub semantic_ids: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidatePromotionAlloySnapshotV1 {
    pub snapshot_id: String,
    pub authority: ArtifactAuthorityV1,
    pub source_graph_id: String,
    pub serializer_version: String,
    pub meta_model_digest: String,
    pub checks_digest: String,
    pub exact_cardinalities: AlloyExactCardinalitiesV1,
    pub provenance: Vec<AlloyProvenanceEntryV1>,
    pub check_evidence_map: Vec<AlloyCheckEvidenceMapV1>,
    pub alloy_module: String,
    pub result_digest: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AlloySerializationError {
    #[error("cannot serialize semantic record {semantic_id}: lowering status is {status:?}")]
    IncompleteLowering {
        semantic_id: String,
        status: LoweringStatus,
    },
    #[error("{collection} capacity exceeded: {actual} > {maximum}")]
    CapacityExceeded {
        collection: &'static str,
        actual: usize,
        maximum: usize,
    },
}

pub fn serialize_alloy_snapshot(
    lowered: &LoweredClaimTraceV1,
    capacity: AlloyCapacity,
) -> Result<CandidatePromotionAlloySnapshotV1, AlloySerializationError> {
    for entry in &lowered.loss_manifest {
        if entry.status != LoweringStatus::Lowered {
            return Err(AlloySerializationError::IncompleteLowering {
                semantic_id: entry.semantic_id.clone(),
                status: entry.status,
            });
        }
    }

    enforce_capacity(
        "model_atoms",
        lowered.hypergraph.nodes.len(),
        capacity.max_model_atoms,
    )?;
    enforce_capacity(
        "claim_assertions",
        lowered.hypergraph.assertions.len(),
        capacity.max_claim_assertions,
    )?;
    enforce_capacity(
        "atom_relations",
        lowered.hypergraph.relations.len(),
        capacity.max_atom_relations,
    )?;

    let node_symbols = lowered
        .hypergraph
        .nodes
        .iter()
        .map(|node| (node.node_id.clone(), stable_symbol("N", &node.node_id)))
        .collect::<BTreeMap<_, _>>();
    let assertion_symbols = lowered
        .hypergraph
        .assertions
        .iter()
        .map(|assertion| {
            (
                assertion.assertion_id.clone(),
                stable_symbol("A", &assertion.assertion_id),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let relation_symbols = lowered
        .hypergraph
        .relations
        .iter()
        .map(|relation| {
            (
                relation.relation_id.clone(),
                stable_symbol("R", &relation.relation_id),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut binding_symbols = BTreeMap::<String, (String, &'static str)>::new();
    for node in &lowered.hypergraph.nodes {
        let (binding_key, binding_kind) = match &node.atom_binding {
            AtomBinding::ExactRef { atom_id } => (format!("exact:{atom_id}"), "ExactRef"),
            AtomBinding::ProposedIdentity {
                normalized_identity,
            } => (
                format!("proposed:{normalized_identity}"),
                "ProposedIdentity",
            ),
            AtomBinding::Alternatives { .. } | AtomBinding::Unresolved => {
                unreachable!("non-exact lowering is rejected before serialization")
            }
        };
        binding_symbols
            .entry(binding_key.clone())
            .or_insert_with(|| (stable_symbol("B", &binding_key), binding_kind));
    }

    let mut provenance = Vec::new();
    provenance.extend(
        lowered
            .hypergraph
            .nodes
            .iter()
            .map(|node| AlloyProvenanceEntryV1 {
                alloy_symbol: node_symbols[&node.node_id].clone(),
                semantic_id: node.node_id.clone(),
                source_spans: node.source_spans.clone(),
            }),
    );
    provenance.extend(lowered.hypergraph.assertions.iter().map(|assertion| {
        AlloyProvenanceEntryV1 {
            alloy_symbol: assertion_symbols[&assertion.assertion_id].clone(),
            semantic_id: assertion.assertion_id.clone(),
            source_spans: assertion.source_spans.clone(),
        }
    }));
    provenance.extend(
        lowered
            .hypergraph
            .relations
            .iter()
            .map(|relation| AlloyProvenanceEntryV1 {
                alloy_symbol: relation_symbols[&relation.relation_id].clone(),
                semantic_id: relation.relation_id.clone(),
                source_spans: relation.source_spans.clone(),
            }),
    );
    provenance.sort_by(|left, right| left.semantic_id.cmp(&right.semantic_id));
    let check_evidence_map = identity_conflict_evidence(lowered);

    let exact_cardinalities = AlloyExactCardinalitiesV1 {
        model_atoms: node_symbols.len(),
        claim_assertions: assertion_symbols.len(),
        atom_relations: relation_symbols.len(),
    };
    let alloy_module = render_module(
        lowered,
        &node_symbols,
        &assertion_symbols,
        &relation_symbols,
        &binding_symbols,
    );
    let mut snapshot = CandidatePromotionAlloySnapshotV1 {
        snapshot_id: String::new(),
        authority: ArtifactAuthorityV1::NonCanonicalProjection,
        source_graph_id: lowered.hypergraph.graph_id.clone(),
        serializer_version: ALLOY_SNAPSHOT_SERIALIZER_VERSION.into(),
        meta_model_digest: digest_text(ALLOY_META_MODEL),
        checks_digest: digest_text(ALLOY_CHECKS),
        exact_cardinalities,
        provenance,
        check_evidence_map,
        alloy_module,
        result_digest: String::new(),
    };
    snapshot.result_digest = compute_snapshot_digest(&snapshot);
    snapshot.snapshot_id = stable_snapshot_id(&snapshot.source_graph_id, &snapshot.result_digest);
    Ok(snapshot)
}

fn render_module(
    lowered: &LoweredClaimTraceV1,
    node_symbols: &BTreeMap<String, String>,
    assertion_symbols: &BTreeMap<String, String>,
    relation_symbols: &BTreeMap<String, String>,
    binding_symbols: &BTreeMap<String, (String, &'static str)>,
) -> String {
    let graph = &lowered.hypergraph;
    let mut output = String::from(
        "module candidate_promotion_snapshot\n\n\
         open smr_meta_model\n\
         open smr_checks\n\n",
    );

    for (symbol, binding_kind) in binding_symbols.values() {
        output.push_str(&format!("one sig {symbol} extends BindingIdentity {{}}\n"));
        output.push_str(&format!(
            "fact {{ {symbol}.bindingKind = {binding_kind} }}\n"
        ));
    }
    for node in &graph.nodes {
        let symbol = &node_symbols[&node.node_id];
        output.push_str(&format!("one sig {symbol} extends ModelAtom {{}}\n"));
    }
    for assertion in &graph.assertions {
        let symbol = &assertion_symbols[&assertion.assertion_id];
        output.push_str(&format!("one sig {symbol} extends ClaimAssertion {{}}\n"));
    }
    for relation in &graph.relations {
        let symbol = &relation_symbols[&relation.relation_id];
        output.push_str(&format!("one sig {symbol} extends AtomRelation {{}}\n"));
    }

    output.push_str("\nfact ExactSnapshot {\n");
    output.push_str(&format!(
        "  ModelAtom = {}\n",
        render_union(node_symbols.values())
    ));
    output.push_str(&format!(
        "  ClaimAssertion = {}\n",
        render_union(assertion_symbols.values())
    ));
    output.push_str(&format!(
        "  AtomRelation = {}\n",
        render_union(relation_symbols.values())
    ));
    output.push_str(&format!(
        "  BindingIdentity = {}\n",
        render_union(binding_symbols.values().map(|(symbol, _)| symbol))
    ));
    output.push_str("}\n\nfact SnapshotBindings {\n");

    for node in &graph.nodes {
        let symbol = &node_symbols[&node.node_id];
        let binding_key = match &node.atom_binding {
            AtomBinding::ExactRef { atom_id } => format!("exact:{atom_id}"),
            AtomBinding::ProposedIdentity {
                normalized_identity,
            } => format!("proposed:{normalized_identity}"),
            AtomBinding::Alternatives { .. } | AtomBinding::Unresolved => unreachable!(),
        };
        output.push_str(&format!(
            "  {symbol}.occurrenceKind = {}\n  {symbol}.bindingIdentity = {}\n",
            occurrence_kind(node.kind),
            binding_symbols[&binding_key].0
        ));
    }

    for assertion in &graph.assertions {
        let symbol = &assertion_symbols[&assertion.assertion_id];
        output.push_str(&format!(
            "  {symbol}.actorNode = {}\n\
             \x20 {symbol}.actionNode = {}\n\
             \x20 {symbol}.targetNode = {}\n\
             \x20 {symbol}.preStateNode = {}\n\
             \x20 {symbol}.postStateNode = {}\n\
         \x20 {symbol}.guardNodes = {}\n\
         \x20 {symbol}.effectNodes = {}\n\
         \x20 {symbol}.exceptionAssertions = {}\n\
         \x20 {symbol}.modality = {}\n\
             \x20 {symbol}.polarity = {}\n\
             \x20 {symbol}.quantifier = {}\n",
            optional_symbol(assertion.actor_node_id.as_deref(), node_symbols),
            node_symbols[&assertion.action_node_id],
            optional_symbol(assertion.target_node_id.as_deref(), node_symbols),
            optional_symbol(assertion.pre_state_node_id.as_deref(), node_symbols),
            optional_symbol(assertion.post_state_node_id.as_deref(), node_symbols),
            symbol_set(&assertion.guard_node_ids, node_symbols),
            symbol_set(&assertion.effect_node_ids, node_symbols),
            symbol_set(&assertion.exception_assertion_ids, assertion_symbols),
            modality(assertion.modality),
            polarity(assertion.polarity),
            quantifier(assertion.quantifier),
        ));
    }

    let semantic_symbols = node_symbols
        .iter()
        .chain(assertion_symbols.iter())
        .map(|(semantic_id, symbol)| (semantic_id.clone(), symbol.clone()))
        .collect::<BTreeMap<_, _>>();
    for relation in &graph.relations {
        let symbol = &relation_symbols[&relation.relation_id];
        output.push_str(&format!(
            "  {symbol}.fromElement = {}\n\
             \x20 {symbol}.toElement = {}\n\
             \x20 {symbol}.relationKind = {}\n",
            semantic_symbols[&relation.from_id],
            semantic_symbols[&relation.to_id],
            relation_kind(relation.relation),
        ));
    }
    output.push_str("}\n\n");
    let exact_scope = format!(
        "for exactly {} ModelAtom, exactly {} ClaimAssertion, exactly {} AtomRelation",
        node_symbols.len(),
        assertion_symbols.len(),
        relation_symbols.len(),
    );
    output.push_str(&format!("run SnapshotInstance {exact_scope}\n"));
    output.push_str(&format!("check NoSemanticIdentityConflict {exact_scope}\n"));
    output
}

fn identity_conflict_evidence(lowered: &LoweredClaimTraceV1) -> Vec<AlloyCheckEvidenceMapV1> {
    let equivalent = lowered
        .hypergraph
        .relations
        .iter()
        .filter(|relation| relation.relation == HypergraphRelationKind::EquivalentTo);
    let distinct = lowered
        .hypergraph
        .relations
        .iter()
        .filter(|relation| relation.relation == HypergraphRelationKind::DistinctFrom)
        .collect::<Vec<_>>();
    let mut evidence = Vec::new();

    for equivalent_relation in equivalent {
        for distinct_relation in &distinct {
            let same_direction = equivalent_relation.from_id == distinct_relation.from_id
                && equivalent_relation.to_id == distinct_relation.to_id;
            let reverse_direction = equivalent_relation.from_id == distinct_relation.to_id
                && equivalent_relation.to_id == distinct_relation.from_id;
            if !same_direction && !reverse_direction {
                continue;
            }

            let mut semantic_ids = vec![
                equivalent_relation.relation_id.clone(),
                distinct_relation.relation_id.clone(),
            ];
            semantic_ids.sort();
            let source_spans = equivalent_relation
                .source_spans
                .iter()
                .chain(&distinct_relation.source_spans)
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            evidence.push(AlloyCheckEvidenceMapV1 {
                command: AlloyCommand::NoSemanticIdentityConflict,
                semantic_ids,
                source_spans,
            });
        }
    }

    evidence.sort_by(|left, right| left.semantic_ids.cmp(&right.semantic_ids));
    evidence
}

fn occurrence_kind(kind: OccurrenceKind) -> &'static str {
    match kind {
        OccurrenceKind::Actor => "ActorKind",
        OccurrenceKind::Role => "RoleKind",
        OccurrenceKind::Action => "ActionKind",
        OccurrenceKind::Target => "TargetKind",
        OccurrenceKind::State => "StateKind",
        OccurrenceKind::Condition => "ConditionKind",
        OccurrenceKind::Value => "ValueKind",
    }
}

fn modality(value: Modality) -> &'static str {
    match value {
        Modality::Asserted => "Asserted",
        Modality::Permitted => "Permitted",
        Modality::Required => "Required",
        Modality::Prohibited => "Prohibited",
        Modality::Conditional => "Conditional",
        Modality::Unknown => unreachable!("unknown modality is rejected before serialization"),
    }
}

fn polarity(value: Polarity) -> &'static str {
    match value {
        Polarity::Positive => "Positive",
        Polarity::Negative => "Negative",
        Polarity::Unknown => unreachable!("unknown polarity is rejected before serialization"),
    }
}

fn quantifier(value: Quantifier) -> &'static str {
    match value {
        Quantifier::All => "All",
        Quantifier::Some => "Some",
        Quantifier::One => "One",
        Quantifier::Lone => "Lone",
        Quantifier::None => "NoneQ",
        Quantifier::Unspecified => {
            unreachable!("unspecified quantifier is rejected before serialization")
        }
    }
}

fn relation_kind(value: HypergraphRelationKind) -> &'static str {
    match value {
        HypergraphRelationKind::Precedes => "Precedes",
        HypergraphRelationKind::Requires => "RequiresRel",
        HypergraphRelationKind::Enables => "Enables",
        HypergraphRelationKind::CausesTransition => "CausesTransition",
        HypergraphRelationKind::ConditionalOn => "ConditionalOn",
        HypergraphRelationKind::AlternativeTo => "AlternativeTo",
        HypergraphRelationKind::Excludes => "Excludes",
        HypergraphRelationKind::EquivalentTo => "EquivalentTo",
        HypergraphRelationKind::DistinctFrom => "DistinctFrom",
    }
}

fn optional_symbol(id: Option<&str>, symbols: &BTreeMap<String, String>) -> String {
    id.map(|id| symbols[id].clone())
        .unwrap_or_else(|| "none".into())
}

fn symbol_set(ids: &[String], symbols: &BTreeMap<String, String>) -> String {
    render_union(ids.iter().map(|id| &symbols[id]))
}

fn render_union<'a, I>(symbols: I) -> String
where
    I: IntoIterator<Item = &'a String>,
{
    let joined = symbols
        .into_iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" + ");
    if joined.is_empty() {
        "none".into()
    } else {
        joined
    }
}

fn stable_symbol(prefix: &str, semantic_id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(semantic_id));
    format!("{prefix}_{}", &digest[..16])
}

fn stable_snapshot_id(graph_id: &str, result_digest: &str) -> String {
    let digest = Sha256::digest(format!(
        "{ALLOY_SNAPSHOT_SERIALIZER_VERSION}:{graph_id}:{result_digest}"
    ));
    format!("ALLOY-SNAPSHOT-{digest:x}")
}

pub(crate) fn compute_snapshot_digest(snapshot: &CandidatePromotionAlloySnapshotV1) -> String {
    let bytes = serde_json::to_vec(&(
        snapshot.source_graph_id.as_str(),
        snapshot.authority,
        snapshot.serializer_version.as_str(),
        snapshot.meta_model_digest.as_str(),
        snapshot.checks_digest.as_str(),
        snapshot.exact_cardinalities,
        &snapshot.provenance,
        &snapshot.check_evidence_map,
        snapshot.alloy_module.as_str(),
    ))
    .expect("snapshot identity serialization is infallible");
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn digest_text(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

fn enforce_capacity(
    collection: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), AlloySerializationError> {
    if actual > maximum {
        Err(AlloySerializationError::CapacityExceeded {
            collection,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}
