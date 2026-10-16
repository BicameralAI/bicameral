//! Deterministic State Machine Resolver boundary.
//!
//! The crate accepts a source-bound [`ClaimSemanticTraceV1`] proposed by an
//! external reasoning provider. It does not invoke a model, persist Product
//! state, or grant authority. Validation rejects malformed semantic content
//! instead of repairing it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

mod alloy;
mod analyzer;
mod experiment_board;
mod feedback;
mod feedback_alloy;
mod feedback_analyzer;
mod feedback_evaluation;
mod feedback_mapping;
mod hypergraph;

pub use alloy::{
    serialize_alloy_snapshot, AlloyCapacity, AlloyCheckEvidenceMapV1, AlloyCommand,
    AlloyCommandKind, AlloyExactCardinalitiesV1, AlloyProvenanceEntryV1, AlloySerializationError,
    CandidatePromotionAlloySnapshotV1,
};
pub use analyzer::{
    analyze_alloy_snapshot, AlloyAnalysisError, AlloyAnalysisResultV1, AlloyAnalysisStatus,
    AlloyAnalyzerConfig, AlloyAnalyzerReceiptV1, AlloyFindingV1,
};
pub use experiment_board::{
    load_bundled_reviewed_experiment_board, load_experiment_board, load_reviewed_experiment_board,
    reviewed_experiment_board_pin, ExperimentBoardError, ExperimentBoardPinV1,
    ReviewedExperimentBoardV1, REVIEWED_BOARD_CLAIM_SET_DIGEST, REVIEWED_BOARD_FILE_SHA256,
    REVIEWED_BOARD_HEAD_COMMIT, REVIEWED_BOARD_MERGED_COMMIT,
};
pub use feedback::{
    map_feedback_verdict, BoundedCheckOutcome, FeedbackVerdict, InconclusiveVerdictChecks,
};
pub use feedback_alloy::{
    serialize_feedback_alloy_snapshots, FeedbackAlloySerializationError,
    FeedbackAlloySnapshotPairV1, FeedbackAlloySnapshotV1, FeedbackBooleanPropositionV1,
    FEEDBACK_ALLOY_SERIALIZER_VERSION,
};
pub use feedback_analyzer::{
    analyze_feedback_snapshot_pair, FeedbackAlloyAnalysisError, FeedbackAlloyAnalyzerConfig,
    FeedbackAnalyzerReceiptV1, FeedbackBoundedCheckV1, FeedbackBoundedEvaluationV1,
    FEEDBACK_ALLOY_DISTRIBUTION_SHA256,
};
pub use feedback_evaluation::{
    evaluate_feedback_proposition, FeedbackEvaluationAttemptV1, FeedbackUnevaluatedReasonV1,
    FeedbackUnevaluatedV1,
};
pub use feedback_mapping::{
    plan_feedback_check_pair, FeedbackAssertionWitnessRequestV1, FeedbackBaselineAuthorityV1,
    FeedbackCheckPairPlanV1, FeedbackCheckRoleV1, FeedbackMappingError,
};
pub use hypergraph::{
    lower_claim_trace, CandidateHypergraphV1, HypergraphAssertionV1, HypergraphNodeV1,
    HypergraphRelationKind, HypergraphRelationV1, LossManifestEntryV1, LoweredClaimTraceV1,
    LoweringStatus,
};

pub const ALPHA_EXTRACTION_PROFILE_ID: &str = "smr-claim-trace-alpha-v1";
pub const ALPHA_PROMPT_TEMPLATE_ID: &str = "smr-claim-trace-alpha-v1";
pub const CLAIM_TRACE_SCHEMA_ID: &str = "ClaimSemanticTraceV1";
pub const STRICT_DECODING_PROFILE_ID: &str = "strict-structured-output-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAuthorityV1 {
    NonCanonicalCandidate,
    NonCanonicalProjection,
    BoundedEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub artifact_id: String,
    pub version: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedModelBase {
    pub model_id: String,
    pub version: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionPlacement {
    Caller,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionProfileV1 {
    pub profile_id: String,
    pub placement: ExtractionPlacement,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_template_id: String,
    pub output_schema_id: String,
    pub decoding_profile_id: String,
    pub provider_config_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceKind {
    Actor,
    Role,
    Action,
    Target,
    State,
    Condition,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AtomBinding {
    ExactRef { atom_id: String },
    ProposedIdentity { normalized_identity: String },
    Alternatives { candidate_refs: Vec<String> },
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimOccurrenceV1 {
    pub occurrence_id: String,
    pub kind: OccurrenceKind,
    pub source_unit_refs: Vec<String>,
    pub atom_binding: AtomBinding,
    #[serde(default)]
    pub unknown_dimensions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMode {
    ExactTuple,
    CompatibleSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Asserted,
    Permitted,
    Required,
    Prohibited,
    Conditional,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Positive,
    Negative,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantifier {
    All,
    Some,
    One,
    Lone,
    None,
    Unspecified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimAssertionV1 {
    pub assertion_id: String,
    pub source_unit_refs: Vec<String>,
    #[serde(default)]
    pub actor_occurrence_id: Option<String>,
    pub action_occurrence_id: String,
    #[serde(default)]
    pub target_occurrence_id: Option<String>,
    pub binding_mode: BindingMode,
    pub modality: Modality,
    pub polarity: Polarity,
    pub quantifier: Quantifier,
    #[serde(default)]
    pub pre_state_occurrence_id: Option<String>,
    #[serde(default)]
    pub post_state_occurrence_id: Option<String>,
    #[serde(default)]
    pub guard_occurrence_ids: Vec<String>,
    #[serde(default)]
    pub effect_occurrence_ids: Vec<String>,
    #[serde(default)]
    pub exception_assertion_ids: Vec<String>,
    #[serde(default)]
    pub unknown_dimensions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimEdgeKind {
    Precedes,
    Requires,
    Enables,
    CausesTransition,
    ConditionalOn,
    AlternativeTo,
    Excludes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimEdgeV1 {
    pub edge_id: String,
    pub from_assertion_id: String,
    pub to_assertion_id: String,
    pub relation: ClaimEdgeKind,
    pub source_unit_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRelationKind {
    EquivalentTo,
    DistinctFrom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticRelationV1 {
    pub relation_id: String,
    pub left_occurrence_id: String,
    pub right_occurrence_id: String,
    pub relation: SemanticRelationKind,
    pub source_unit_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Complete,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceCoverageV1 {
    pub status: CoverageStatus,
    pub source_unit_refs: Vec<String>,
    #[serde(default)]
    pub unknown_dimensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimSemanticTraceV1 {
    pub trace_id: String,
    pub source_binding: SourceBinding,
    pub accepted_model_base: AcceptedModelBase,
    pub extraction_profile: ExtractionProfileV1,
    pub occurrences: Vec<ClaimOccurrenceV1>,
    pub assertions: Vec<ClaimAssertionV1>,
    #[serde(default)]
    pub edges: Vec<ClaimEdgeV1>,
    #[serde(default)]
    pub semantic_relations: Vec<SemanticRelationV1>,
    pub coverage: TraceCoverageV1,
    pub result_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceUnit {
    pub source_unit_id: String,
    pub span: SourceSpan,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCatalog {
    units: BTreeMap<String, SourceUnit>,
}

impl SourceCatalog {
    pub fn new<I>(units: I) -> Result<Self, TraceValidationError>
    where
        I: IntoIterator<Item = SourceUnit>,
    {
        let mut indexed = BTreeMap::new();
        for unit in units {
            require_text(&unit.source_unit_id, "source_unit_id")?;
            require_digest(&unit.content_digest, "source unit content_digest")?;
            if unit.span.byte_start >= unit.span.byte_end {
                return Err(TraceValidationError::InvalidSourceUnit {
                    source_unit_id: unit.source_unit_id,
                    reason: "byte_start must be less than byte_end".into(),
                });
            }
            if unit.span.line_start == 0 || unit.span.line_start > unit.span.line_end {
                return Err(TraceValidationError::InvalidSourceUnit {
                    source_unit_id: unit.source_unit_id,
                    reason: "line range must be one-based and ordered".into(),
                });
            }
            let unit_id = unit.source_unit_id.clone();
            if indexed.insert(unit_id.clone(), unit).is_some() {
                return Err(TraceValidationError::DuplicateId(unit_id));
            }
        }
        if indexed.is_empty() {
            return Err(TraceValidationError::EmptyCollection("source catalog"));
        }
        Ok(Self { units: indexed })
    }

    fn resolve(&self, source_unit_id: &str) -> Option<&SourceUnit> {
        self.units.get(source_unit_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceLimits {
    pub max_occurrences: usize,
    pub max_assertions: usize,
    pub max_edges: usize,
    pub max_semantic_relations: usize,
}

impl Default for TraceLimits {
    fn default() -> Self {
        Self {
            max_occurrences: 256,
            max_assertions: 256,
            max_edges: 512,
            max_semantic_relations: 256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedClaimTrace {
    pub trace: ClaimSemanticTraceV1,
    source_spans: BTreeMap<String, Vec<SourceSpan>>,
}

impl ValidatedClaimTrace {
    pub fn source_spans_for(&self, semantic_id: &str) -> Option<&[SourceSpan]> {
        self.source_spans.get(semantic_id).map(Vec::as_slice)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TraceValidationError {
    #[error("{0} must not be empty")]
    EmptyText(&'static str),
    #[error("{0} must use a content-addressed digest")]
    InvalidDigest(&'static str),
    #[error("{0} must contain at least one item")]
    EmptyCollection(&'static str),
    #[error("duplicate semantic identifier: {0}")]
    DuplicateId(String),
    #[error("{owner_id} references unknown {reference_kind} {reference_id}")]
    UnknownReference {
        owner_id: String,
        reference_kind: &'static str,
        reference_id: String,
    },
    #[error("{semantic_id} must cite at least one source unit")]
    MissingSourceProvenance { semantic_id: String },
    #[error("unsupported extraction profile field {field}: {actual}")]
    UnsupportedExtractionProfile { field: &'static str, actual: String },
    #[error("{collection} capacity exceeded: {actual} > {maximum}")]
    CapacityExceeded {
        collection: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("invalid source unit {source_unit_id}: {reason}")]
    InvalidSourceUnit {
        source_unit_id: String,
        reason: String,
    },
    #[error("declared trace digest does not match deterministic digest")]
    DigestMismatch,
    #[error("{semantic_id} omits {dimension} without declaring it in unknown_dimensions")]
    UnmarkedUnknownDimension {
        semantic_id: String,
        dimension: String,
    },
}

pub fn validate_claim_trace(
    mut trace: ClaimSemanticTraceV1,
    catalog: &SourceCatalog,
    limits: TraceLimits,
) -> Result<ValidatedClaimTrace, TraceValidationError> {
    validate_identity(&trace)?;
    validate_profile(&trace.extraction_profile)?;
    enforce_capacity(
        "occurrences",
        trace.occurrences.len(),
        limits.max_occurrences,
    )?;
    enforce_capacity("assertions", trace.assertions.len(), limits.max_assertions)?;
    enforce_capacity("edges", trace.edges.len(), limits.max_edges)?;
    enforce_capacity(
        "semantic_relations",
        trace.semantic_relations.len(),
        limits.max_semantic_relations,
    )?;

    if trace.occurrences.is_empty() {
        return Err(TraceValidationError::EmptyCollection("occurrences"));
    }
    if trace.assertions.is_empty() {
        return Err(TraceValidationError::EmptyCollection("assertions"));
    }

    let occurrence_ids = unique_ids(
        trace
            .occurrences
            .iter()
            .map(|occurrence| occurrence.occurrence_id.as_str()),
    )?;
    let assertion_ids = unique_ids(
        trace
            .assertions
            .iter()
            .map(|assertion| assertion.assertion_id.as_str()),
    )?;
    let mut all_ids = occurrence_ids.clone();
    extend_unique(
        &mut all_ids,
        trace.edges.iter().map(|edge| edge.edge_id.as_str()),
    )?;
    extend_unique(
        &mut all_ids,
        trace
            .semantic_relations
            .iter()
            .map(|relation| relation.relation_id.as_str()),
    )?;
    for assertion_id in &assertion_ids {
        if !all_ids.insert(assertion_id.clone()) {
            return Err(TraceValidationError::DuplicateId(assertion_id.clone()));
        }
    }

    let mut spans = BTreeMap::new();
    for occurrence in &trace.occurrences {
        spans.insert(
            occurrence.occurrence_id.clone(),
            resolve_source_refs(
                &occurrence.occurrence_id,
                &occurrence.source_unit_refs,
                catalog,
            )?,
        );
        validate_atom_binding(&occurrence.occurrence_id, &occurrence.atom_binding)?;
        if occurrence.atom_binding == AtomBinding::Unresolved
            && !occurrence
                .unknown_dimensions
                .iter()
                .any(|dimension| dimension == "atom_binding")
        {
            return Err(TraceValidationError::UnmarkedUnknownDimension {
                semantic_id: occurrence.occurrence_id.clone(),
                dimension: "atom_binding".into(),
            });
        }
    }

    for assertion in &trace.assertions {
        spans.insert(
            assertion.assertion_id.clone(),
            resolve_source_refs(
                &assertion.assertion_id,
                &assertion.source_unit_refs,
                catalog,
            )?,
        );
        require_reference(
            &assertion.assertion_id,
            "action occurrence",
            &assertion.action_occurrence_id,
            &occurrence_ids,
        )?;
        for reference in [
            assertion.actor_occurrence_id.as_ref(),
            assertion.target_occurrence_id.as_ref(),
            assertion.pre_state_occurrence_id.as_ref(),
            assertion.post_state_occurrence_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        .chain(assertion.guard_occurrence_ids.iter())
        .chain(assertion.effect_occurrence_ids.iter())
        {
            require_reference(
                &assertion.assertion_id,
                "occurrence",
                reference,
                &occurrence_ids,
            )?;
        }
        for exception in &assertion.exception_assertion_ids {
            require_reference(
                &assertion.assertion_id,
                "assertion",
                exception,
                &assertion_ids,
            )?;
        }
        validate_assertion_semantics(assertion)?;
    }

    for edge in &trace.edges {
        spans.insert(
            edge.edge_id.clone(),
            resolve_source_refs(&edge.edge_id, &edge.source_unit_refs, catalog)?,
        );
        require_reference(
            &edge.edge_id,
            "assertion",
            &edge.from_assertion_id,
            &assertion_ids,
        )?;
        require_reference(
            &edge.edge_id,
            "assertion",
            &edge.to_assertion_id,
            &assertion_ids,
        )?;
    }

    for relation in &trace.semantic_relations {
        spans.insert(
            relation.relation_id.clone(),
            resolve_source_refs(&relation.relation_id, &relation.source_unit_refs, catalog)?,
        );
        require_reference(
            &relation.relation_id,
            "occurrence",
            &relation.left_occurrence_id,
            &occurrence_ids,
        )?;
        require_reference(
            &relation.relation_id,
            "occurrence",
            &relation.right_occurrence_id,
            &occurrence_ids,
        )?;
    }

    resolve_source_refs("coverage", &trace.coverage.source_unit_refs, catalog)?;

    let declared_digest = std::mem::take(&mut trace.result_digest);
    canonicalize_trace(&mut trace);
    let digest = deterministic_trace_digest(&trace);
    if !declared_digest.is_empty() && declared_digest != digest {
        return Err(TraceValidationError::DigestMismatch);
    }
    trace.result_digest = digest;

    Ok(ValidatedClaimTrace {
        trace,
        source_spans: spans,
    })
}

fn validate_identity(trace: &ClaimSemanticTraceV1) -> Result<(), TraceValidationError> {
    require_text(&trace.trace_id, "trace_id")?;
    require_text(&trace.source_binding.artifact_id, "source artifact_id")?;
    require_text(&trace.source_binding.version, "source version")?;
    require_digest(
        &trace.source_binding.content_digest,
        "source content_digest",
    )?;
    require_text(&trace.accepted_model_base.model_id, "accepted model_id")?;
    require_text(&trace.accepted_model_base.version, "accepted model version")?;
    require_digest(
        &trace.accepted_model_base.content_digest,
        "accepted model content_digest",
    )
}

fn validate_profile(profile: &ExtractionProfileV1) -> Result<(), TraceValidationError> {
    for (field, actual, expected) in [
        (
            "profile_id",
            profile.profile_id.as_str(),
            ALPHA_EXTRACTION_PROFILE_ID,
        ),
        (
            "prompt_template_id",
            profile.prompt_template_id.as_str(),
            ALPHA_PROMPT_TEMPLATE_ID,
        ),
        (
            "output_schema_id",
            profile.output_schema_id.as_str(),
            CLAIM_TRACE_SCHEMA_ID,
        ),
        (
            "decoding_profile_id",
            profile.decoding_profile_id.as_str(),
            STRICT_DECODING_PROFILE_ID,
        ),
    ] {
        if actual != expected {
            return Err(TraceValidationError::UnsupportedExtractionProfile {
                field,
                actual: actual.into(),
            });
        }
    }
    require_text(&profile.provider_id, "provider_id")?;
    require_text(&profile.model_id, "provider model_id")?;
    require_digest(&profile.provider_config_digest, "provider_config_digest")
}

fn validate_atom_binding(
    owner_id: &str,
    binding: &AtomBinding,
) -> Result<(), TraceValidationError> {
    match binding {
        AtomBinding::ExactRef { atom_id } => require_text(atom_id, "atom_id"),
        AtomBinding::ProposedIdentity {
            normalized_identity,
        } => require_text(normalized_identity, "normalized_identity"),
        AtomBinding::Alternatives { candidate_refs } => {
            if candidate_refs.is_empty() {
                return Err(TraceValidationError::EmptyCollection(
                    "atom binding alternatives",
                ));
            }
            let mut seen = BTreeSet::new();
            for candidate in candidate_refs {
                require_text(candidate, "candidate_ref")?;
                if !seen.insert(candidate) {
                    return Err(TraceValidationError::DuplicateId(format!(
                        "{owner_id}/{candidate}"
                    )));
                }
            }
            Ok(())
        }
        AtomBinding::Unresolved => Ok(()),
    }
}

fn validate_assertion_semantics(assertion: &ClaimAssertionV1) -> Result<(), TraceValidationError> {
    if assertion.binding_mode != BindingMode::CompatibleSelector {
        return Ok(());
    }

    let declared_unknowns = assertion
        .unknown_dimensions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let omitted_dimensions = [
        ("actor", assertion.actor_occurrence_id.is_none()),
        ("target", assertion.target_occurrence_id.is_none()),
        ("pre_state", assertion.pre_state_occurrence_id.is_none()),
        ("post_state", assertion.post_state_occurrence_id.is_none()),
        ("modality", assertion.modality == Modality::Unknown),
        ("polarity", assertion.polarity == Polarity::Unknown),
        (
            "quantifier",
            assertion.quantifier == Quantifier::Unspecified,
        ),
    ];

    for (dimension, omitted) in omitted_dimensions {
        if omitted && !declared_unknowns.contains(dimension) {
            return Err(TraceValidationError::UnmarkedUnknownDimension {
                semantic_id: assertion.assertion_id.clone(),
                dimension: dimension.into(),
            });
        }
    }
    Ok(())
}

fn deterministic_trace_digest(trace_without_digest: &ClaimSemanticTraceV1) -> String {
    let serialized =
        serde_json::to_vec(trace_without_digest).expect("ClaimTrace serialization is infallible");
    format!("sha256:{:x}", Sha256::digest(serialized))
}

fn canonicalize_trace(trace: &mut ClaimSemanticTraceV1) {
    for occurrence in &mut trace.occurrences {
        occurrence.source_unit_refs.sort();
        occurrence.unknown_dimensions.sort();
        if let AtomBinding::Alternatives { candidate_refs } = &mut occurrence.atom_binding {
            candidate_refs.sort();
        }
    }
    trace
        .occurrences
        .sort_by(|left, right| left.occurrence_id.cmp(&right.occurrence_id));

    for assertion in &mut trace.assertions {
        assertion.source_unit_refs.sort();
        assertion.guard_occurrence_ids.sort();
        assertion.effect_occurrence_ids.sort();
        assertion.exception_assertion_ids.sort();
        assertion.unknown_dimensions.sort();
    }
    trace
        .assertions
        .sort_by(|left, right| left.assertion_id.cmp(&right.assertion_id));

    for edge in &mut trace.edges {
        edge.source_unit_refs.sort();
    }
    trace
        .edges
        .sort_by(|left, right| left.edge_id.cmp(&right.edge_id));

    for relation in &mut trace.semantic_relations {
        relation.source_unit_refs.sort();
    }
    trace
        .semantic_relations
        .sort_by(|left, right| left.relation_id.cmp(&right.relation_id));

    trace.coverage.source_unit_refs.sort();
    trace.coverage.unknown_dimensions.sort();
}

fn resolve_source_refs(
    semantic_id: &str,
    source_refs: &[String],
    catalog: &SourceCatalog,
) -> Result<Vec<SourceSpan>, TraceValidationError> {
    if source_refs.is_empty() {
        return Err(TraceValidationError::MissingSourceProvenance {
            semantic_id: semantic_id.into(),
        });
    }
    let mut spans = Vec::with_capacity(source_refs.len());
    let mut seen = BTreeSet::new();
    for source_ref in source_refs {
        if !seen.insert(source_ref) {
            return Err(TraceValidationError::DuplicateId(format!(
                "{semantic_id}/{source_ref}"
            )));
        }
        let unit =
            catalog
                .resolve(source_ref)
                .ok_or_else(|| TraceValidationError::UnknownReference {
                    owner_id: semantic_id.into(),
                    reference_kind: "source unit",
                    reference_id: source_ref.clone(),
                })?;
        spans.push(unit.span);
    }
    Ok(spans)
}

fn require_reference(
    owner_id: &str,
    reference_kind: &'static str,
    reference_id: &str,
    valid_ids: &BTreeSet<String>,
) -> Result<(), TraceValidationError> {
    if valid_ids.contains(reference_id) {
        Ok(())
    } else {
        Err(TraceValidationError::UnknownReference {
            owner_id: owner_id.into(),
            reference_kind,
            reference_id: reference_id.into(),
        })
    }
}

fn unique_ids<'a, I>(ids: I) -> Result<BTreeSet<String>, TraceValidationError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut unique = BTreeSet::new();
    extend_unique(&mut unique, ids)?;
    Ok(unique)
}

fn extend_unique<'a, I>(unique: &mut BTreeSet<String>, ids: I) -> Result<(), TraceValidationError>
where
    I: IntoIterator<Item = &'a str>,
{
    for id in ids {
        require_text(id, "semantic identifier")?;
        if !unique.insert(id.into()) {
            return Err(TraceValidationError::DuplicateId(id.into()));
        }
    }
    Ok(())
}

fn enforce_capacity(
    collection: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), TraceValidationError> {
    if actual > maximum {
        Err(TraceValidationError::CapacityExceeded {
            collection,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn require_text(value: &str, field: &'static str) -> Result<(), TraceValidationError> {
    if value.trim().is_empty() {
        Err(TraceValidationError::EmptyText(field))
    } else {
        Ok(())
    }
}

fn require_digest(value: &str, field: &'static str) -> Result<(), TraceValidationError> {
    if value.starts_with("sha256:") && value.len() > "sha256:".len() {
        Ok(())
    } else {
        Err(TraceValidationError::InvalidDigest(field))
    }
}
