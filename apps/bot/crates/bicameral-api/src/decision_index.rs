//! DecisionIndexRecord — versioned metadata compiler for impact recall.
//!
//! Defines the `DecisionIndexRecord` type that holds compiled metadata for
//! accepted Decisions and source-backed candidates used by the recall subsystem.
//!
//! Each metadata field carries full provenance (source type/ref, proposer,
//! acceptance fields, extraction method, taxonomy version) and quality labels
//! so the recall resolver can distinguish accepted from provisional metadata
//! and downgrade no-result claims when coverage is incomplete.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::candidate::{DecisionLevel, ExtractionConfidence};
use crate::dashboard::SignoffState;

// ---------------------------------------------------------------------------
// Metadata schema version
// ---------------------------------------------------------------------------

/// Current metadata schema version for DecisionIndexRecord.
///
/// Bump this when the shape of `MetadataEntry`, quality labels, or compilation
/// semantics change in a backward-incompatible way.
pub const METADATA_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// DecisionIndexRecord
// ---------------------------------------------------------------------------

/// A compiled index record for an accepted Decision or source-backed candidate.
///
/// Used by the recall subsystem to perform deterministic metadata matching.
/// Each field carries provenance and quality labels so the resolver can:
/// - Distinguish accepted metadata from auto-compiled/provisional metadata.
/// - Downgrade no-result claims when metadata coverage is incomplete.
/// - Report coverage/debt by field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionIndexRecord {
    /// Unique record identifier.
    pub record_id: Uuid,

    /// The Decision or candidate this record indexes.
    pub source_ref: IndexSourceRef,

    /// Schema version of this record's metadata structure.
    pub schema_version: u32,

    /// Compiled metadata entries, keyed by field name.
    pub metadata: Vec<MetadataEntry>,

    /// Overall compilation state of this record.
    pub compilation_state: CompilationState,

    /// Timestamp at which this record was last compiled.
    pub compiled_at: DateTime<Utc>,

    /// Coverage report for this record's metadata fields.
    pub coverage: CoverageReport,
}

impl DecisionIndexRecord {
    /// Create a new record for an accepted Decision.
    pub fn for_decision(decision_id: Uuid, title: String, level: DecisionLevel) -> Self {
        Self {
            record_id: Uuid::new_v4(),
            source_ref: IndexSourceRef::Decision {
                decision_id,
                title,
                level,
                signoff_state: SignoffState::Proposed,
            },
            schema_version: METADATA_SCHEMA_VERSION,
            metadata: Vec::new(),
            compilation_state: CompilationState::Empty,
            compiled_at: Utc::now(),
            coverage: CoverageReport::empty(),
        }
    }

    /// Create a new record for a source-backed candidate.
    pub fn for_candidate(
        candidate_id: Uuid,
        title: String,
        level: DecisionLevel,
        extraction_confidence: ExtractionConfidence,
    ) -> Self {
        Self {
            record_id: Uuid::new_v4(),
            source_ref: IndexSourceRef::Candidate {
                candidate_id,
                title,
                level,
                extraction_confidence,
            },
            schema_version: METADATA_SCHEMA_VERSION,
            metadata: Vec::new(),
            compilation_state: CompilationState::Empty,
            compiled_at: Utc::now(),
            coverage: CoverageReport::empty(),
        }
    }

    /// Add a metadata entry and recompile state.
    pub fn add_metadata(&mut self, entry: MetadataEntry) {
        self.metadata.push(entry);
        self.recompile();
    }

    /// Recompile the record's compilation state and coverage from its entries.
    pub fn recompile(&mut self) {
        self.compiled_at = Utc::now();
        self.compilation_state = Self::derive_compilation_state(&self.metadata);
        self.coverage = Self::derive_coverage(&self.metadata);
    }

    /// Derive the overall compilation state from metadata entries.
    fn derive_compilation_state(entries: &[MetadataEntry]) -> CompilationState {
        if entries.is_empty() {
            return CompilationState::Empty;
        }

        let has_accepted = entries
            .iter()
            .any(|e| e.acceptance_state == MetadataAcceptanceState::Accepted);
        let has_rejected = entries
            .iter()
            .any(|e| e.acceptance_state == MetadataAcceptanceState::Rejected);
        let has_stale = entries
            .iter()
            .any(|e| e.acceptance_state == MetadataAcceptanceState::Stale);
        let has_superseded = entries
            .iter()
            .any(|e| e.acceptance_state == MetadataAcceptanceState::Superseded);
        let has_auto = entries
            .iter()
            .any(|e| e.acceptance_state == MetadataAcceptanceState::AutoCompiled);
        let all_accepted = entries
            .iter()
            .all(|e| e.acceptance_state == MetadataAcceptanceState::Accepted);

        if all_accepted {
            CompilationState::Accepted
        } else if has_rejected && !has_accepted && !has_auto {
            CompilationState::Rejected
        } else if has_superseded && !has_accepted && !has_auto {
            CompilationState::Superseded
        } else if has_stale && !has_accepted {
            CompilationState::Stale
        } else {
            CompilationState::Partial
        }
    }

    /// Derive coverage report from metadata entries.
    fn derive_coverage(entries: &[MetadataEntry]) -> CoverageReport {
        let total_fields = entries.len() as u32;
        let accepted_fields = entries
            .iter()
            .filter(|e| e.acceptance_state == MetadataAcceptanceState::Accepted)
            .count() as u32;
        let auto_compiled_fields = entries
            .iter()
            .filter(|e| e.acceptance_state == MetadataAcceptanceState::AutoCompiled)
            .count() as u32;
        let stale_fields = entries
            .iter()
            .filter(|e| e.acceptance_state == MetadataAcceptanceState::Stale)
            .count() as u32;
        let missing_fields = REQUIRED_METADATA_FIELDS
            .iter()
            .filter(|required| !entries.iter().any(|e| e.key == **required))
            .count() as u32;

        let debt_fields: Vec<String> = REQUIRED_METADATA_FIELDS
            .iter()
            .filter(|required| !entries.iter().any(|e| e.key == **required))
            .map(|s| s.to_string())
            .collect();

        CoverageReport {
            total_fields,
            accepted_fields,
            auto_compiled_fields,
            stale_fields,
            missing_fields,
            debt_fields,
            coverage_ratio: if total_fields + missing_fields > 0 {
                accepted_fields as f64 / (total_fields + missing_fields) as f64
            } else {
                0.0
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Index source reference
// ---------------------------------------------------------------------------

/// What the index record refers to: an accepted Decision or a source-backed candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndexSourceRef {
    /// An accepted Decision in the ledger.
    Decision {
        decision_id: Uuid,
        title: String,
        level: DecisionLevel,
        signoff_state: SignoffState,
    },
    /// A source-backed candidate (not yet promoted).
    Candidate {
        candidate_id: Uuid,
        title: String,
        level: DecisionLevel,
        extraction_confidence: ExtractionConfidence,
    },
}

impl IndexSourceRef {
    /// The identifier of the source entity.
    pub fn entity_id(&self) -> Uuid {
        match self {
            IndexSourceRef::Decision { decision_id, .. } => *decision_id,
            IndexSourceRef::Candidate { candidate_id, .. } => *candidate_id,
        }
    }

    /// Whether this is a Decision (accepted) source.
    pub fn is_decision(&self) -> bool {
        matches!(self, IndexSourceRef::Decision { .. })
    }

    /// Whether this is a Candidate source.
    pub fn is_candidate(&self) -> bool {
        matches!(self, IndexSourceRef::Candidate { .. })
    }
}

// ---------------------------------------------------------------------------
// Metadata entry with provenance
// ---------------------------------------------------------------------------

/// A single metadata field with full provenance chain.
///
/// Each metadata entry records where the value came from, who proposed it,
/// how it was accepted, and any quality caveats that constrain how the recall
/// resolver should weight it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataEntry {
    /// Field key (e.g. "domain", "component", "risk_level").
    pub key: String,

    /// Field value.
    pub value: serde_json::Value,

    /// Provenance chain for this metadata value.
    pub provenance: MetadataProvenance,

    /// Current acceptance state of this entry.
    pub acceptance_state: MetadataAcceptanceState,

    /// Quality labels/caveats constraining how this entry participates in recall.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quality_labels: Vec<MetadataQualityLabel>,

    /// Taxonomy version under which this entry was classified.
    pub taxonomy_version: String,

    /// When this entry was last compiled/updated.
    pub compiled_at: DateTime<Utc>,
}

impl MetadataEntry {
    /// Whether this entry carries any quality caveats.
    pub fn has_caveats(&self) -> bool {
        !self.quality_labels.is_empty()
    }

    /// Whether this entry participates fully in recall matching (accepted, no caveats).
    pub fn is_full_participant(&self) -> bool {
        self.acceptance_state == MetadataAcceptanceState::Accepted && self.quality_labels.is_empty()
    }

    /// Whether this entry participates with caveats in recall matching.
    pub fn is_caveat_participant(&self) -> bool {
        self.acceptance_state == MetadataAcceptanceState::AutoCompiled
            || (self.acceptance_state == MetadataAcceptanceState::Accepted
                && !self.quality_labels.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Metadata provenance
// ---------------------------------------------------------------------------

/// Full provenance chain for a metadata value.
///
/// Records the source, extraction method, proposer, and acceptance context
/// so the recall resolver can trace any metadata value back to its origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataProvenance {
    /// Classification of the source that produced this metadata value.
    pub source_type: MetadataSourceType,

    /// URI or stable reference to the source (e.g. issue URL, transcript ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,

    /// Who proposed this metadata value.
    pub proposer: String,

    /// How the value was extracted from the source.
    pub extraction_method: MetadataExtractionMethod,

    /// Who accepted this metadata value (when acceptance_state is Accepted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,

    /// When acceptance was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<DateTime<Utc>>,

    /// The review/signoff event that accepted this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_event_id: Option<Uuid>,
}

/// Source type classification for metadata provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSourceType {
    /// Extracted from a human-authored decision or review.
    HumanAuthored,
    /// Auto-compiled from source evidence by the daemon.
    DaemonCompiled,
    /// Extracted from an external integration source.
    ExternalSource,
    /// Derived from code analysis (tree-sitter, graph).
    CodeAnalysis,
    /// Inherited from a related or parent Decision.
    Inherited,
}

/// Method used to extract a metadata value from its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataExtractionMethod {
    /// Manually entered by a human.
    Manual,
    /// Extracted via LLM/reasoning projection.
    LlmExtraction,
    /// Extracted via deterministic pattern matching.
    PatternMatch,
    /// Copied from an existing canonical source.
    CopyFromCanonical,
    /// Inferred from code structure/AST analysis.
    CodeInference,
    /// Auto-compiled by the metadata compiler with no explicit source.
    AutoCompiled,
}

// ---------------------------------------------------------------------------
// Acceptance state
// ---------------------------------------------------------------------------

/// Acceptance state of a metadata entry.
///
/// Distinguishes accepted (human-reviewed) from auto-compiled (provisional)
/// metadata. The recall resolver uses this to weight results:
/// - Accepted: full-confidence participation.
/// - AutoCompiled: participates only with quality caveats.
/// - Stale/Superseded/Rejected: excluded or downgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataAcceptanceState {
    /// Human-reviewed and accepted metadata.
    Accepted,
    /// Auto-compiled by the daemon — provisional, participates with caveats.
    AutoCompiled,
    /// Partial information — some source data missing or incomplete.
    Partial,
    /// Rejected during review — excluded from recall matching.
    Rejected,
    /// Superseded by a newer value — historical only.
    Superseded,
    /// Source evidence has gone stale — freshness check failed.
    Stale,
}

// ---------------------------------------------------------------------------
// Quality labels
// ---------------------------------------------------------------------------

/// Quality label/caveat constraining how a metadata entry participates in recall.
///
/// Auto-compiled metadata always carries at least one quality caveat.
/// Accepted metadata may also carry caveats when the acceptance was conditional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataQualityLabel {
    /// Kind of quality caveat.
    pub kind: QualityLabelKind,

    /// Human-readable explanation of the caveat.
    pub detail: String,
}

/// Kind of quality label/caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityLabelKind {
    /// Value was auto-compiled without human review.
    AutoCompiledNoReview,
    /// Source evidence is stale or may be outdated.
    StaleSource,
    /// Low extraction confidence.
    LowConfidence,
    /// Partial coverage — some expected subfields are missing.
    PartialCoverage,
    /// Taxonomy version mismatch — value was classified under an older schema.
    TaxonomyMismatch,
    /// Conditional acceptance — accepted with documented restrictions.
    ConditionalAcceptance,
    /// Inherited from a related Decision — not directly sourced.
    InheritedValue,
}

// ---------------------------------------------------------------------------
// Compilation state
// ---------------------------------------------------------------------------

/// Overall compilation state of a DecisionIndexRecord.
///
/// Derived from the collective acceptance states of all metadata entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilationState {
    /// No metadata entries yet.
    Empty,
    /// All metadata is accepted (full confidence).
    Accepted,
    /// Mix of accepted and auto-compiled/partial metadata.
    Partial,
    /// All metadata has been rejected.
    Rejected,
    /// Record has been superseded by a newer version.
    Superseded,
    /// Source evidence has gone stale.
    Stale,
}

// ---------------------------------------------------------------------------
// Coverage report
// ---------------------------------------------------------------------------

/// Required metadata field keys for coverage reporting.
///
/// The recall resolver uses these to determine whether a record has sufficient
/// metadata for confident matching. Missing required fields downgrade no-result
/// claims.
pub const REQUIRED_METADATA_FIELDS: &[&str] = &[
    "domain",
    "component",
    "decision_level",
    "summary",
    "keywords",
];

/// Coverage/debt report for a DecisionIndexRecord.
///
/// Quantifies how much of the expected metadata is present and accepted,
/// enabling the recall resolver to downgrade no-result claims when coverage
/// is incomplete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Total metadata fields present in the record.
    pub total_fields: u32,

    /// Fields with accepted (human-reviewed) state.
    pub accepted_fields: u32,

    /// Fields that were auto-compiled (provisional).
    pub auto_compiled_fields: u32,

    /// Fields whose source evidence has gone stale.
    pub stale_fields: u32,

    /// Count of required fields that are missing entirely.
    pub missing_fields: u32,

    /// Names of required fields that are missing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub debt_fields: Vec<String>,

    /// Ratio of accepted fields to total expected (0.0–1.0).
    pub coverage_ratio: f64,
}

impl CoverageReport {
    /// An empty coverage report (no fields present).
    pub fn empty() -> Self {
        Self {
            total_fields: 0,
            accepted_fields: 0,
            auto_compiled_fields: 0,
            stale_fields: 0,
            missing_fields: REQUIRED_METADATA_FIELDS.len() as u32,
            debt_fields: REQUIRED_METADATA_FIELDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            coverage_ratio: 0.0,
        }
    }

    /// Whether coverage is sufficient for confident recall matching.
    ///
    /// Requires at least 60% accepted coverage and no missing required fields.
    pub fn is_sufficient(&self) -> bool {
        self.missing_fields == 0 && self.coverage_ratio >= 0.6
    }

    /// Whether coverage gaps should downgrade no-result claims.
    pub fn should_downgrade_no_result(&self) -> bool {
        self.missing_fields > 0 || self.coverage_ratio < 0.6
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Record shape ─────────────────────────────────────────────────────

    #[test]
    fn decision_record_has_correct_schema_version() {
        let record = DecisionIndexRecord::for_decision(
            Uuid::nil(),
            "Test decision".into(),
            DecisionLevel::Architecture,
        );
        assert_eq!(record.schema_version, METADATA_SCHEMA_VERSION);
    }

    #[test]
    fn candidate_record_has_correct_schema_version() {
        let record = DecisionIndexRecord::for_candidate(
            Uuid::nil(),
            "Test candidate".into(),
            DecisionLevel::Product,
            ExtractionConfidence::High,
        );
        assert_eq!(record.schema_version, METADATA_SCHEMA_VERSION);
    }

    #[test]
    fn decision_source_ref_is_decision() {
        let record = DecisionIndexRecord::for_decision(
            Uuid::nil(),
            "Test".into(),
            DecisionLevel::Implementation,
        );
        assert!(record.source_ref.is_decision());
        assert!(!record.source_ref.is_candidate());
    }

    #[test]
    fn candidate_source_ref_is_candidate() {
        let record = DecisionIndexRecord::for_candidate(
            Uuid::nil(),
            "Test".into(),
            DecisionLevel::Product,
            ExtractionConfidence::Medium,
        );
        assert!(record.source_ref.is_candidate());
        assert!(!record.source_ref.is_decision());
    }

    #[test]
    fn entity_id_returns_correct_id() {
        let id = Uuid::new_v4();
        let record = DecisionIndexRecord::for_decision(id, "T".into(), DecisionLevel::Architecture);
        assert_eq!(record.source_ref.entity_id(), id);
    }

    #[test]
    fn new_record_starts_empty() {
        let record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        assert_eq!(record.compilation_state, CompilationState::Empty);
        assert!(record.metadata.is_empty());
    }

    #[test]
    fn record_serializes_and_deserializes() {
        let record = DecisionIndexRecord::for_decision(
            Uuid::nil(),
            "Serialization test".into(),
            DecisionLevel::Architecture,
        );
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: DecisionIndexRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.record_id, record.record_id);
        assert_eq!(deserialized.schema_version, METADATA_SCHEMA_VERSION);
    }

    // ── Provenance ───────────────────────────────────────────────────────

    #[test]
    fn metadata_entry_carries_full_provenance() {
        let entry = MetadataEntry {
            key: "domain".into(),
            value: serde_json::Value::String("payments".into()),
            provenance: MetadataProvenance {
                source_type: MetadataSourceType::HumanAuthored,
                source_ref: Some("jira://PROJ-123".into()),
                proposer: "user-alice".into(),
                extraction_method: MetadataExtractionMethod::Manual,
                accepted_by: Some("user-bob".into()),
                accepted_at: Some(Utc::now()),
                acceptance_event_id: Some(Uuid::new_v4()),
            },
            acceptance_state: MetadataAcceptanceState::Accepted,
            quality_labels: vec![],
            taxonomy_version: "v1".into(),
            compiled_at: Utc::now(),
        };
        assert_eq!(
            entry.provenance.source_type,
            MetadataSourceType::HumanAuthored
        );
        assert_eq!(entry.provenance.proposer, "user-alice");
        assert!(entry.provenance.accepted_by.is_some());
        assert!(entry.provenance.acceptance_event_id.is_some());
    }

    #[test]
    fn provenance_source_types_serialize() {
        let types = vec![
            (MetadataSourceType::HumanAuthored, "\"human_authored\""),
            (MetadataSourceType::DaemonCompiled, "\"daemon_compiled\""),
            (MetadataSourceType::ExternalSource, "\"external_source\""),
            (MetadataSourceType::CodeAnalysis, "\"code_analysis\""),
            (MetadataSourceType::Inherited, "\"inherited\""),
        ];
        for (variant, expected) in types {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn extraction_methods_serialize() {
        let methods = vec![
            (MetadataExtractionMethod::Manual, "\"manual\""),
            (
                MetadataExtractionMethod::LlmExtraction,
                "\"llm_extraction\"",
            ),
            (MetadataExtractionMethod::PatternMatch, "\"pattern_match\""),
            (
                MetadataExtractionMethod::CopyFromCanonical,
                "\"copy_from_canonical\"",
            ),
            (
                MetadataExtractionMethod::CodeInference,
                "\"code_inference\"",
            ),
            (MetadataExtractionMethod::AutoCompiled, "\"auto_compiled\""),
        ];
        for (variant, expected) in methods {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    // ── Quality labels ───────────────────────────────────────────────────

    #[test]
    fn auto_compiled_entry_has_quality_caveat() {
        let entry = MetadataEntry {
            key: "component".into(),
            value: serde_json::Value::String("auth".into()),
            provenance: MetadataProvenance {
                source_type: MetadataSourceType::DaemonCompiled,
                source_ref: None,
                proposer: "daemon".into(),
                extraction_method: MetadataExtractionMethod::AutoCompiled,
                accepted_by: None,
                accepted_at: None,
                acceptance_event_id: None,
            },
            acceptance_state: MetadataAcceptanceState::AutoCompiled,
            quality_labels: vec![MetadataQualityLabel {
                kind: QualityLabelKind::AutoCompiledNoReview,
                detail: "Auto-compiled without human review".into(),
            }],
            taxonomy_version: "v1".into(),
            compiled_at: Utc::now(),
        };
        assert!(entry.has_caveats());
        assert!(!entry.is_full_participant());
        assert!(entry.is_caveat_participant());
    }

    #[test]
    fn accepted_entry_without_caveats_is_full_participant() {
        let entry = MetadataEntry {
            key: "domain".into(),
            value: serde_json::Value::String("billing".into()),
            provenance: MetadataProvenance {
                source_type: MetadataSourceType::HumanAuthored,
                source_ref: Some("linear://ISS-42".into()),
                proposer: "user-1".into(),
                extraction_method: MetadataExtractionMethod::Manual,
                accepted_by: Some("user-2".into()),
                accepted_at: Some(Utc::now()),
                acceptance_event_id: None,
            },
            acceptance_state: MetadataAcceptanceState::Accepted,
            quality_labels: vec![],
            taxonomy_version: "v1".into(),
            compiled_at: Utc::now(),
        };
        assert!(entry.is_full_participant());
        assert!(!entry.is_caveat_participant());
        assert!(!entry.has_caveats());
    }

    #[test]
    fn quality_label_kinds_serialize() {
        let kinds = vec![
            (
                QualityLabelKind::AutoCompiledNoReview,
                "\"auto_compiled_no_review\"",
            ),
            (QualityLabelKind::StaleSource, "\"stale_source\""),
            (QualityLabelKind::LowConfidence, "\"low_confidence\""),
            (QualityLabelKind::PartialCoverage, "\"partial_coverage\""),
            (QualityLabelKind::TaxonomyMismatch, "\"taxonomy_mismatch\""),
            (
                QualityLabelKind::ConditionalAcceptance,
                "\"conditional_acceptance\"",
            ),
            (QualityLabelKind::InheritedValue, "\"inherited_value\""),
        ];
        for (variant, expected) in kinds {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    // ── Versioning ───────────────────────────────────────────────────────

    #[test]
    fn schema_version_is_stable_constant() {
        assert_eq!(METADATA_SCHEMA_VERSION, 1);
    }

    #[test]
    fn taxonomy_version_preserved_through_serialization() {
        let entry = MetadataEntry {
            key: "keywords".into(),
            value: serde_json::json!(["auth", "rbac"]),
            provenance: MetadataProvenance {
                source_type: MetadataSourceType::HumanAuthored,
                source_ref: None,
                proposer: "user-1".into(),
                extraction_method: MetadataExtractionMethod::Manual,
                accepted_by: None,
                accepted_at: None,
                acceptance_event_id: None,
            },
            acceptance_state: MetadataAcceptanceState::Accepted,
            quality_labels: vec![],
            taxonomy_version: "v2.1".into(),
            compiled_at: Utc::now(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deser: MetadataEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.taxonomy_version, "v2.1");
    }

    // ── Compiler behavior ────────────────────────────────────────────────

    #[test]
    fn all_accepted_entries_yield_accepted_state() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Architecture);
        for key in REQUIRED_METADATA_FIELDS {
            record.add_metadata(make_entry(key, MetadataAcceptanceState::Accepted));
        }
        assert_eq!(record.compilation_state, CompilationState::Accepted);
    }

    #[test]
    fn mixed_accepted_and_auto_yields_partial() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Architecture);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::Accepted));
        record.add_metadata(make_entry(
            "component",
            MetadataAcceptanceState::AutoCompiled,
        ));
        assert_eq!(record.compilation_state, CompilationState::Partial);
    }

    #[test]
    fn all_rejected_entries_yield_rejected_state() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::Rejected));
        record.add_metadata(make_entry("component", MetadataAcceptanceState::Rejected));
        assert_eq!(record.compilation_state, CompilationState::Rejected);
    }

    #[test]
    fn all_superseded_entries_yield_superseded_state() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::Superseded));
        assert_eq!(record.compilation_state, CompilationState::Superseded);
    }

    #[test]
    fn stale_without_accepted_yields_stale() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::Stale));
        record.add_metadata(make_entry("component", MetadataAcceptanceState::Stale));
        assert_eq!(record.compilation_state, CompilationState::Stale);
    }

    // ── Coverage / debt ──────────────────────────────────────────────────

    #[test]
    fn empty_record_reports_all_required_as_debt() {
        let record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        assert_eq!(
            record.coverage.missing_fields,
            REQUIRED_METADATA_FIELDS.len() as u32
        );
        assert_eq!(record.coverage.coverage_ratio, 0.0);
        assert!(record.coverage.should_downgrade_no_result());
        assert!(!record.coverage.is_sufficient());
    }

    #[test]
    fn full_accepted_coverage_is_sufficient() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Architecture);
        for key in REQUIRED_METADATA_FIELDS {
            record.add_metadata(make_entry(key, MetadataAcceptanceState::Accepted));
        }
        assert_eq!(record.coverage.missing_fields, 0);
        assert!(record.coverage.is_sufficient());
        assert!(!record.coverage.should_downgrade_no_result());
    }

    #[test]
    fn partial_coverage_downgrades_no_result() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::Accepted));
        record.add_metadata(make_entry("component", MetadataAcceptanceState::Accepted));
        // Missing: decision_level, summary, keywords
        assert!(record.coverage.missing_fields > 0);
        assert!(record.coverage.should_downgrade_no_result());
    }

    #[test]
    fn coverage_report_tracks_auto_compiled_fields() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::AutoCompiled));
        assert_eq!(record.coverage.auto_compiled_fields, 1);
    }

    #[test]
    fn coverage_report_tracks_stale_fields() {
        let mut record =
            DecisionIndexRecord::for_decision(Uuid::nil(), "T".into(), DecisionLevel::Product);
        record.add_metadata(make_entry("domain", MetadataAcceptanceState::Stale));
        assert_eq!(record.coverage.stale_fields, 1);
    }

    // ── Acceptance state serialization ───────────────────────────────────

    #[test]
    fn acceptance_states_serialize() {
        let states = vec![
            (MetadataAcceptanceState::Accepted, "\"accepted\""),
            (MetadataAcceptanceState::AutoCompiled, "\"auto_compiled\""),
            (MetadataAcceptanceState::Partial, "\"partial\""),
            (MetadataAcceptanceState::Rejected, "\"rejected\""),
            (MetadataAcceptanceState::Superseded, "\"superseded\""),
            (MetadataAcceptanceState::Stale, "\"stale\""),
        ];
        for (variant, expected) in states {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn compilation_states_serialize() {
        let states = vec![
            (CompilationState::Empty, "\"empty\""),
            (CompilationState::Accepted, "\"accepted\""),
            (CompilationState::Partial, "\"partial\""),
            (CompilationState::Rejected, "\"rejected\""),
            (CompilationState::Superseded, "\"superseded\""),
            (CompilationState::Stale, "\"stale\""),
        ];
        for (variant, expected) in states {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    // ── Helper ───────────────────────────────────────────────────────────

    fn make_entry(key: &str, acceptance_state: MetadataAcceptanceState) -> MetadataEntry {
        MetadataEntry {
            key: key.into(),
            value: serde_json::Value::String("test_value".into()),
            provenance: MetadataProvenance {
                source_type: MetadataSourceType::HumanAuthored,
                source_ref: None,
                proposer: "test".into(),
                extraction_method: MetadataExtractionMethod::Manual,
                accepted_by: None,
                accepted_at: None,
                acceptance_event_id: None,
            },
            acceptance_state,
            quality_labels: vec![],
            taxonomy_version: "v1".into(),
            compiled_at: Utc::now(),
        }
    }
}
