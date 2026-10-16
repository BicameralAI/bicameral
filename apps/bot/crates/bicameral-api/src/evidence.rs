//! Evidence types — linking decisions to sources and code.
//!
//! Aligned with MCP v0.2 spec §5 (source evidence pipeline).

use crate::source::SourceKind;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Broad citation pointer into a `SourceSnapshot`.
///
/// `EvidenceReference` is the V1 citation abstraction per ADR-0007 and
/// ADR-0022. It can point at source excerpts, agent session turns, tool
/// calls, command output, file ranges, diff hunks, or code snapshot regions.
/// Candidates, Decisions, bindings, and governance results may cite multiple
/// evidence references when a claim depends on multiple source regions or
/// sources.
///
/// `SourceEvidence` remains the concrete source-backed span evidence object;
/// `EvidenceReference` is the broader abstraction (CONTEXT.md).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceReference {
    pub id: Uuid,
    /// The snapshot this reference points into.
    pub snapshot_id: String,
    /// Classification of the source surface this reference cites.
    pub source_kind: SourceKind,
    /// Kind-specific locator into the snapshot (e.g. byte offset, line range,
    /// JSON path, tool-call ID). Interpretation depends on `source_kind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// Canonical excerpt of the cited content.
    pub excerpt: String,
    pub captured_at: DateTime<Utc>,
    /// Optional metadata for kind-specific details (e.g. diff hunk header,
    /// MCP session turn index, command exit code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl EvidenceReference {
    pub fn new(snapshot_id: String, source_kind: SourceKind, excerpt: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            snapshot_id,
            source_kind,
            pointer: None,
            excerpt,
            captured_at: Utc::now(),
            metadata: None,
        }
    }
}

/// Source evidence — a span pointer into a `SourceSnapshot`.
///
/// Per spec §5.2 stage 3: `SourceEvidence` is a span pointer into a snapshot
/// — the specific excerpt that supports a claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub id: Uuid,
    /// The snapshot this evidence points into.
    pub snapshot_id: String,
    pub span_start: usize,
    pub span_end: usize,
    pub excerpt: String,
    pub confidence: f64,
    #[serde(default)]
    pub extraction_method: Option<ExtractionMethod>,
    pub captured_at: DateTime<Utc>,
}

impl SourceEvidence {
    pub fn new(
        snapshot_id: String,
        span_start: usize,
        span_end: usize,
        excerpt: String,
        confidence: f64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            snapshot_id,
            span_start,
            span_end,
            excerpt,
            confidence,
            extraction_method: None,
            captured_at: Utc::now(),
        }
    }
}

/// How evidence was extracted from a source snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMethod {
    LlmExtract,
    Regex,
    Manual,
}

/// Evidence binding a decision to specific code symbols, files, or paths.
///
/// Binding evidence answers "what code does this decision constrain?" — a file
/// path, function symbol, dependency, deploy surface, or code region.
///
/// Binding confidence is intentionally separate from extraction confidence and
/// compliance confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingEvidence {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub binding_type: BindingType,
    pub target: String,
    pub confidence: BindingConfidence,
    pub explanation: Option<String>,
    pub bound_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Snapshot identity at which the binding was validated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    /// Commit SHA at which the binding was validated.
    ///
    /// Together with `graph_snapshot_id`, this forms the durable immutable
    /// identity anchor for binding evidence per ADR-0017. Branch names are
    /// provenance only — `validated_sha` is the canonical durable key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validated_sha: Option<String>,
    /// Provenance ref name at which the binding was resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// Classification of the binding target (e.g. "file", "symbol", "code_region").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    /// Canonical identifier for the binding target within the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_identifier: Option<String>,
    /// Graph evidence state at the time of validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_state: Option<GraphEvidenceState>,
    /// Readiness warnings surfaced during binding validation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness_warnings: Vec<String>,
    /// The `ToolRequest.request_id` that created this binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_from_request_id: Option<Uuid>,
}

impl BindingEvidence {
    pub fn new(
        candidate_id: Uuid,
        binding_type: BindingType,
        target: String,
        confidence: BindingConfidence,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            candidate_id,
            binding_type,
            target,
            confidence,
            explanation: None,
            bound_at: Utc::now(),
            metadata: serde_json::Value::Null,
            graph_snapshot_id: None,
            validated_sha: None,
            ref_name: None,
            target_kind: None,
            target_identifier: None,
            evidence_state: None,
            readiness_warnings: Vec::new(),
            created_from_request_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingType {
    FilePath,
    Symbol,
    Dependency,
    DeploySurface,
    CodeRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingConfidence {
    Low,
    Medium,
    High,
}

/// A verified graph fact proposed by runtime prevalidation.
///
/// `BindingProposal` is a graph-level output — it records that the codegraph
/// verified a file, range, or symbol at a specific snapshot. It is **not**
/// accepted `BindingEvidence`. The gateway/event-store path is the first
/// boundary where a proposal becomes accepted `BindingEvidence` via the
/// `BindToCode` governance event.
///
/// Callers must not treat a `BindingProposal` as canonical authority. The
/// proposal carries enough information for the gateway to materialize
/// `BindingEvidence` after the event-store acceptance gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingProposal {
    pub candidate_id: Uuid,
    pub binding_type: BindingType,
    pub target: String,
    pub confidence: BindingConfidence,
    pub explanation: Option<String>,
    pub graph_snapshot_id: String,
    pub validated_sha: String,
    pub content_hash: Option<String>,
}

impl BindingProposal {
    /// Promote this proposal into accepted `BindingEvidence`.
    ///
    /// This should only be called at the gateway/event-store acceptance
    /// boundary — the point where the governance event is emitted.
    pub fn into_accepted_evidence(self) -> BindingEvidence {
        let mut binding = BindingEvidence::new(
            self.candidate_id,
            self.binding_type,
            self.target,
            self.confidence,
        );
        binding.explanation = self.explanation;
        binding.graph_snapshot_id = Some(self.graph_snapshot_id);
        binding.validated_sha = Some(self.validated_sha);
        binding
    }
}

/// Graph readiness state from ADR-0010 §GraphReadiness.
///
/// This is **graph/view state**, not a Decision lifecycle event. Graph
/// readiness describes whether the local code graph can answer queries for a
/// requested snapshot. It never directly changes signoff, compliance, review
/// state, or ledger materialization.
///
/// Checked before a Ledger View compliance or grounded-conflict read. Applies
/// to the requested snapshot scope for the view, not to every Decision row
/// individually.
///
/// Downstream consumers (`bind_to_code`, `preflight`, `compliance`) should
/// inspect this state before attempting graph operations. Only `Ready` permits
/// verified graph claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphReadiness {
    /// Required graph snapshot is available for the requested compliance or
    /// grounded read.
    Ready,
    /// Snapshot or graph neighborhood is being built.
    Indexing,
    /// Index exists but does not match the requested commit, index version,
    /// parser version, or resolver version.
    Stale,
    /// Indexing or revalidation failed.
    Failed,
    /// Local graph support is unavailable for this workspace.
    NotConfigured,
}

/// Result of resolving a requested ref to a `GraphSnapshot`.
///
/// When the ref is locally available and resolves to a commit, the result is
/// `Resolved` with the snapshot. Otherwise, the result describes why the
/// ref could not be resolved. This is distinct from `GraphEvidenceState::NotFound`,
/// which means the snapshot *was* inspected and the target symbol/file/range was
/// absent.
///
/// Downstream consumers (`bind_to_code`, `preflight`, `compliance`) can pattern-match
/// on this type to distinguish a usable snapshot from a typed unavailability reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SnapshotResolution {
    /// Ref resolved to a locally inspectable snapshot.
    Resolved { snapshot: GraphSnapshot },
    /// The requested ref does not exist in the local repository.
    RefNotAvailable {
        requested_ref: String,
        detail: String,
    },
    /// The ref exists but the index is stale (wrong commit, parser version,
    /// or index version).
    StaleIndex {
        requested_ref: String,
        detail: String,
    },
    /// Resolution requires hosted or team graph visibility that is not
    /// available locally.
    HostedRequired {
        requested_ref: String,
        detail: String,
    },
    /// Resolution failed for an unexpected reason (e.g. git command error).
    Failed {
        requested_ref: String,
        detail: String,
    },
}

impl SnapshotResolution {
    /// Returns the resolved snapshot if available, or `None`.
    pub fn snapshot(&self) -> Option<&GraphSnapshot> {
        match self {
            SnapshotResolution::Resolved { snapshot } => Some(snapshot),
            _ => None,
        }
    }

    /// Returns `true` when the resolution succeeded.
    pub fn is_resolved(&self) -> bool {
        matches!(self, SnapshotResolution::Resolved { .. })
    }

    /// Returns the `GraphReadiness` that corresponds to this resolution outcome.
    pub fn readiness(&self) -> GraphReadiness {
        match self {
            SnapshotResolution::Resolved { .. } => GraphReadiness::Ready,
            SnapshotResolution::StaleIndex { .. } => GraphReadiness::Stale,
            SnapshotResolution::RefNotAvailable { .. } => GraphReadiness::Failed,
            SnapshotResolution::HostedRequired { .. } => GraphReadiness::NotConfigured,
            SnapshotResolution::Failed { .. } => GraphReadiness::Failed,
        }
    }
}

/// Graph evidence state from ADR-0010 §Evidence States.
///
/// This is **graph/view state**, not a governance event. Non-verified values
/// (`UnknownStale`, `UnknownNotIndexed`, `Ambiguous`, `Unsupported`,
/// `ApproximateCandidate`) must not directly change signoff, compliance,
/// review state, or ledger materialization. They surface honestly as graph
/// limitations. Only `Verified` graph claims may materialize bindings,
/// support compliance conclusions, or produce blocking governance results.
///
/// Unresolved graph grounding must not produce `collision_pending` (per
/// ADR-0011). Graph facts become governance facts only when accepted through
/// the event-store path (per ADR-0015).
///
/// **Important**: `NotFound` is verified negative evidence — the snapshot was
/// inspected and the target was absent. It must not be used for unavailable
/// refs or unresolved snapshots. Use `SnapshotResolution::RefNotAvailable` for
/// those cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEvidenceState {
    /// Proven against the requested snapshot.
    Verified,
    /// The relevant snapshot and required files were inspected; symbol/edge absent.
    NotFound,
    /// Required files or neighborhoods have not been parsed or resolved.
    UnknownNotIndexed,
    /// Available index data does not match the requested snapshot, parser version, or index version.
    UnknownStale,
    /// Multiple occurrences match; API cannot select one without more caller input.
    Ambiguous,
    /// Language, file type, generated artifact, or resolver behavior is outside supported exact validation.
    Unsupported,
    /// Search or ranking found a possible match, but it is not verified graph evidence.
    ApproximateCandidate,
}

impl GraphEvidenceState {
    /// Authority boundary: only `Verified` evidence may materialize bindings.
    ///
    /// Per ADR-0010 §Evidence States and ADR-0017: non-verified graph claims
    /// must not produce `BindingEvidence`, support compliance conclusions, or
    /// generate blocking governance results. This method is the canonical
    /// single-point authority check — callers should use it instead of
    /// pattern-matching on `Verified` directly.
    pub fn allows_binding(&self) -> bool {
        matches!(self, GraphEvidenceState::Verified)
    }

    /// Complement of `allows_binding`: the state is informational only.
    ///
    /// Review-only states surface honestly as graph limitations for human
    /// review, but must not directly change signoff, compliance, review
    /// state, or ledger materialization.
    pub fn is_review_only(&self) -> bool {
        !self.allows_binding()
    }

    /// Project the internal seven-state model to the five-state public model.
    ///
    /// The public model from the 2026-06-09 team sync uses five states:
    /// `Verified`, `NotFound`, `Unknown`, `NotSupported`, `Ambiguous`.
    ///
    /// Internal subdivisions (`UnknownNotIndexed`, `UnknownStale`,
    /// `ApproximateCandidate`) carry diagnostic detail for surfaces and
    /// remediation but collapse to coarser public states for external
    /// consumers and wire schemas.
    ///
    /// The mapping preserves the authority boundary: no non-verified
    /// internal state maps to `PublicGraphEvidenceState::Verified`.
    pub fn to_public(&self) -> PublicGraphEvidenceState {
        match self {
            GraphEvidenceState::Verified => PublicGraphEvidenceState::Verified,
            GraphEvidenceState::NotFound => PublicGraphEvidenceState::NotFound,
            GraphEvidenceState::UnknownNotIndexed
            | GraphEvidenceState::UnknownStale
            | GraphEvidenceState::ApproximateCandidate => PublicGraphEvidenceState::Unknown,
            GraphEvidenceState::Ambiguous => PublicGraphEvidenceState::Ambiguous,
            GraphEvidenceState::Unsupported => PublicGraphEvidenceState::NotSupported,
        }
    }
}

/// Five-state public graph evidence model.
///
/// This is the external/wire-level projection of `GraphEvidenceState` from
/// the 2026-06-09 team sync. It collapses internal diagnostic subdivisions
/// into five coarse states that external consumers and UI surfaces use.
///
/// The authority boundary is identical: only `Verified` allows binding;
/// all other states are review-only.
///
/// Internal states map as follows:
/// - `Verified` → `Verified`
/// - `NotFound` → `NotFound`
/// - `UnknownNotIndexed` / `UnknownStale` / `ApproximateCandidate` → `Unknown`
/// - `Ambiguous` → `Ambiguous`
/// - `Unsupported` → `NotSupported`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicGraphEvidenceState {
    /// Proven against the requested snapshot — binding-eligible.
    Verified,
    /// Snapshot inspected; target absent — verified negative evidence.
    NotFound,
    /// Graph state is insufficient to determine presence (not indexed,
    /// stale, or only an approximate candidate was found).
    Unknown,
    /// Language, file type, or resolver is outside supported validation.
    NotSupported,
    /// Multiple occurrences match; disambiguation required before binding.
    Ambiguous,
}

impl PublicGraphEvidenceState {
    /// Authority boundary: only `Verified` evidence may materialize bindings.
    pub fn allows_binding(&self) -> bool {
        matches!(self, PublicGraphEvidenceState::Verified)
    }

    /// Complement of `allows_binding`: the state is informational only.
    pub fn is_review_only(&self) -> bool {
        !self.allows_binding()
    }
}

/// Coverage scope for a graph readiness view.
///
/// Indicates whether the graph snapshot covers all files in the worktree
/// (full branch coverage) or only a subset (e.g. only Rust files when
/// other languages are present). Consumers must not treat `Partial`
/// scope as full branch coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphScopeCoverage {
    /// Full branch/worktree coverage — all files are indexed.
    Full,
    /// Only a subset of files or languages are indexed.
    Partial,
}

/// View-layer graph readiness summary for dashboard and surfaces.
///
/// Surfaces graph readiness honestly without being part of the Decision
/// lifecycle. Surfaces can show `stale`, `not_configured`, `indexing`,
/// `failed`, or `ambiguous` states using this type. These values never
/// directly mutate signoff, compliance, review state, or ledger
/// materialization.
///
/// Per ADR-0010/ADR-0011/ADR-0015: graph facts are not governance facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphReadinessView {
    pub readiness: GraphReadiness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Scope coverage for the graph snapshot. `None` when coverage has
    /// not been evaluated. `Some(Partial)` when only a subset of files
    /// or languages are indexed — this must not be treated as full
    /// branch coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_coverage: Option<GraphScopeCoverage>,
}

impl GraphReadinessView {
    /// Build a view from a `SnapshotResolution`.
    pub fn from_resolution(resolution: &SnapshotResolution) -> Self {
        let readiness = resolution.readiness();
        let graph_snapshot_id = resolution.snapshot().map(|s| s.graph_snapshot_id.clone());
        let detail = match resolution {
            SnapshotResolution::Resolved { .. } => None,
            SnapshotResolution::RefNotAvailable { detail, .. }
            | SnapshotResolution::StaleIndex { detail, .. }
            | SnapshotResolution::HostedRequired { detail, .. }
            | SnapshotResolution::Failed { detail, .. } => Some(detail.clone()),
        };
        Self {
            readiness,
            graph_snapshot_id,
            detail,
            scope_coverage: None,
        }
    }

    /// Build a view directly from a `GraphReadiness` value.
    pub fn from_readiness(readiness: GraphReadiness, detail: Option<String>) -> Self {
        Self {
            readiness,
            graph_snapshot_id: None,
            detail,
            scope_coverage: None,
        }
    }

    /// Set the scope coverage for this readiness view.
    pub fn with_scope_coverage(mut self, coverage: GraphScopeCoverage) -> Self {
        self.scope_coverage = Some(coverage);
        self
    }
}

/// Identifies the code snapshot against which graph claims are made.
///
/// Per ADR-0010 §Definitions: a `GraphSnapshot` carries identity fields
/// (`graph_snapshot_id`, `repo_id`, `commit_sha`, `index_version`) and
/// optional provenance fields (`ref_name`, `merge_base_sha`, `indexed_at`,
/// `index_source`).
///
/// Graph indexes are cache/runtime data only, not canonical `.bicameral/`
/// artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub graph_snapshot_id: String,
    pub repo_id: String,
    pub commit_sha: String,
    pub index_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_source: Option<IndexSource>,
}

/// Where the graph index was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexSource {
    LocalDaemon,
    Hosted,
    Imported,
}

/// Result of inspecting a file (or file range) against a `GraphSnapshot`.
///
/// Returns one of the ADR-0010 evidence states for file-level grounding.
/// Only `verified` results may feed into binding materialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInspectionResult {
    pub path: String,
    pub evidence_state: GraphEvidenceState,
    pub graph_snapshot_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A symbol observed at a specific `GraphSnapshot`.
///
/// Per ADR-0010: a `SymbolOccurrence` carries graph snapshot identity, file
/// path, line/range, content evidence, and parser/resolver metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolOccurrence {
    pub graph_snapshot_id: String,
    pub symbol_name: String,
    pub qualified_name: Option<String>,
    pub symbol_kind: SymbolKind,
    pub language: SymbolLanguage,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub content_hash: String,
    pub structural_hash: String,
    pub evidence_state: GraphEvidenceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_kind: Option<RustCaptureKind>,
}

/// Mapped Rust capture kinds from tree-sitter `tags.scm`.
///
/// Audited from `tree-sitter-rust` TAGS_QUERY. Each variant corresponds to a
/// concrete `@definition.*` or `@reference.*` capture in the upstream
/// `tags.scm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustCaptureKind {
    /// `struct_item`, `enum_item`, `union_item`, `type_item` → `@definition.class`
    DefinitionClass,
    /// `function_item` inside `declaration_list` → `@definition.method`
    DefinitionMethod,
    /// `function_item` (top-level) → `@definition.function`
    DefinitionFunction,
    /// `trait_item` → `@definition.interface`
    DefinitionInterface,
    /// `mod_item` → `@definition.module`
    DefinitionModule,
    /// `macro_definition` → `@definition.macro`
    DefinitionMacro,
    /// `call_expression` → `@reference.call`
    ReferenceCall,
    /// `impl_item` → `@reference.implementation`
    ReferenceImplementation,
}

/// Currentness of code-source evidence after a refresh against GraphStore.
///
/// `CodeSourceCurrentness` is a derived read-model state — it describes
/// whether accepted `BindingEvidence` is still current at the latest
/// available `GraphSnapshot`. It never mutates accepted bindings, signoff,
/// compliance, or ledger materialization.
///
/// Accepted `BindingEvidence` remains immutable. A later refresh that
/// finds stale or unknown graph state produces a derived currentness
/// annotation; it does not rewrite the original binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeSourceCurrentness {
    /// Binding target verified at the current snapshot — content hash matches.
    Current,
    /// Binding target exists but content has changed since the binding was created.
    ContentChanged,
    /// Binding target was not found at the current snapshot.
    TargetMissing,
    /// Graph snapshot is stale, indexing, or failed — currentness cannot be determined.
    GraphUnavailable,
    /// Graph is not configured for this workspace.
    GraphNotConfigured,
}

/// Result of refreshing code-source evidence against GraphStore readiness.
///
/// This is a read-model result that delegates to `GraphReadiness` and
/// `SnapshotResolution` concepts. It preserves the graph-fact vs
/// governance-fact boundary: refresh results are evidence observations,
/// not signoff/compliance mutations.
///
/// Accepted `BindingEvidence` remains immutable regardless of the refresh
/// result. The `currentness` field describes the derived state; downstream
/// consumers may surface it for review but must not use it to block
/// governance results in V1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSourceRefreshResult {
    /// The binding that was evaluated.
    pub binding_id: uuid::Uuid,
    /// The decision that owns this binding.
    pub decision_id: uuid::Uuid,
    /// Graph readiness at the time of refresh.
    pub graph_readiness: GraphReadiness,
    /// Derived currentness of the binding target.
    pub currentness: CodeSourceCurrentness,
    /// The snapshot used for evaluation, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluated_snapshot_id: Option<String>,
    /// Detail message for surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Outcome of an `evidence.refresh` command for one tracked Decision.
///
/// This is a derived read-model result. It describes the currentness of
/// each binding for the requested Decision without mutating signoff,
/// compliance, or accepted `BindingEvidence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRefreshOutcome {
    /// The Decision that was refreshed.
    pub decision_id: uuid::Uuid,
    /// Overall status of the refresh.
    pub status: EvidenceRefreshStatus,
    /// Per-binding currentness results (empty when status is a rejection).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binding_results: Vec<CodeSourceRefreshResult>,
    /// Human-readable detail for the outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// High-level status of an `evidence.refresh` outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRefreshStatus {
    /// Refresh completed — binding results available.
    Refreshed,
    /// Decision is ineligible (not tracked, rejected, superseded, or no bindings).
    Ineligible,
    /// Decision not found in the ledger.
    NotFound,
    /// Workspace/graph is unavailable or unsupported.
    Unsupported,
}

/// Symbol kind in the Bicameral domain vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Union,
    TypeAlias,
    Trait,
    Module,
    Macro,
    Impl,
    Unknown,
}

/// Language of a symbol occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolLanguage {
    Rust,
    Unsupported,
}

/// Result of resolving symbols within a `GraphSnapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolResolutionResult {
    pub graph_snapshot_id: String,
    pub language: SymbolLanguage,
    pub occurrences: Vec<SymbolOccurrence>,
    pub evidence_state: GraphEvidenceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Submission provenance — informational metadata only
// ---------------------------------------------------------------------------

/// Informational metadata capturing the submitting surface, source kind,
/// actor/session/request IDs, source URI, and timestamp for evidence
/// submissions.
///
/// Provenance is **informational only** — it never creates review, signoff,
/// compliance, or governance authority. All fields except `submitted_at` are
/// `Option` so callers with incomplete context degrade gracefully without
/// failing ingest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmissionProvenance {
    /// Which surface submitted this evidence (e.g. "mcp", "cli", "gateway",
    /// "external_adapter").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitting_surface: Option<String>,

    /// Classification of the source that produced the evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,

    /// Actor identity that submitted the evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,

    /// Session context in which the submission occurred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Request identity for correlation with dispatch audit trails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// Source URI from which the evidence originated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,

    /// Timestamp at which the submission was received by the daemon.
    pub submitted_at: DateTime<Utc>,
}

impl SubmissionProvenance {
    /// Derive provenance from a local `ToolRequest` context.
    pub fn from_tool_request(
        request_id: uuid::Uuid,
        authority: &crate::tool_request::AuthorityContext,
        source_uri: Option<&str>,
        source_kind: Option<&str>,
    ) -> Self {
        Self {
            submitting_surface: Some("tool_request".to_string()),
            source_kind: source_kind.map(|s| s.to_string()),
            actor_id: Some(authority.actor_id.clone()),
            session_id: authority.session_id.clone(),
            request_id: Some(request_id.to_string()),
            source_uri: source_uri.map(|s| s.to_string()),
            submitted_at: Utc::now(),
        }
    }

    /// Derive provenance from an external adapter submission.
    pub fn from_external_adapter(source_system: &str, source_uri: &str) -> Self {
        Self {
            submitting_surface: Some("external_adapter".to_string()),
            source_kind: Some(source_system.to_string()),
            actor_id: None,
            session_id: None,
            request_id: None,
            source_uri: Some(source_uri.to_string()),
            submitted_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_evidence_state_serializes_to_snake_case() {
        let cases = vec![
            (GraphEvidenceState::Verified, "\"verified\""),
            (GraphEvidenceState::NotFound, "\"not_found\""),
            (
                GraphEvidenceState::UnknownNotIndexed,
                "\"unknown_not_indexed\"",
            ),
            (GraphEvidenceState::UnknownStale, "\"unknown_stale\""),
            (GraphEvidenceState::Ambiguous, "\"ambiguous\""),
            (GraphEvidenceState::Unsupported, "\"unsupported\""),
            (
                GraphEvidenceState::ApproximateCandidate,
                "\"approximate_candidate\"",
            ),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn graph_evidence_state_deserializes_from_snake_case() {
        let cases = vec![
            ("\"verified\"", GraphEvidenceState::Verified),
            ("\"not_found\"", GraphEvidenceState::NotFound),
            (
                "\"unknown_not_indexed\"",
                GraphEvidenceState::UnknownNotIndexed,
            ),
            ("\"unknown_stale\"", GraphEvidenceState::UnknownStale),
            ("\"ambiguous\"", GraphEvidenceState::Ambiguous),
            ("\"unsupported\"", GraphEvidenceState::Unsupported),
            (
                "\"approximate_candidate\"",
                GraphEvidenceState::ApproximateCandidate,
            ),
        ];
        for (json, expected) in cases {
            assert_eq!(
                serde_json::from_str::<GraphEvidenceState>(json).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn graph_evidence_state_rejects_unknown_variant() {
        assert!(serde_json::from_str::<GraphEvidenceState>("\"ratified\"").is_err());
    }

    // ── Authority boundary tests ─────────────────────────────────────

    #[test]
    fn only_verified_allows_binding() {
        assert!(GraphEvidenceState::Verified.allows_binding());
    }

    #[test]
    fn not_found_cannot_bind() {
        assert!(!GraphEvidenceState::NotFound.allows_binding());
        assert!(GraphEvidenceState::NotFound.is_review_only());
    }

    #[test]
    fn unknown_not_indexed_cannot_bind() {
        assert!(!GraphEvidenceState::UnknownNotIndexed.allows_binding());
        assert!(GraphEvidenceState::UnknownNotIndexed.is_review_only());
    }

    #[test]
    fn unknown_stale_cannot_bind() {
        assert!(!GraphEvidenceState::UnknownStale.allows_binding());
        assert!(GraphEvidenceState::UnknownStale.is_review_only());
    }

    #[test]
    fn ambiguous_cannot_bind() {
        assert!(!GraphEvidenceState::Ambiguous.allows_binding());
        assert!(GraphEvidenceState::Ambiguous.is_review_only());
    }

    #[test]
    fn unsupported_cannot_bind() {
        assert!(!GraphEvidenceState::Unsupported.allows_binding());
        assert!(GraphEvidenceState::Unsupported.is_review_only());
    }

    #[test]
    fn approximate_candidate_cannot_bind() {
        assert!(!GraphEvidenceState::ApproximateCandidate.allows_binding());
        assert!(GraphEvidenceState::ApproximateCandidate.is_review_only());
    }

    #[test]
    fn exhaustive_non_verified_states_cannot_bind() {
        let non_verified = [
            GraphEvidenceState::NotFound,
            GraphEvidenceState::UnknownNotIndexed,
            GraphEvidenceState::UnknownStale,
            GraphEvidenceState::Ambiguous,
            GraphEvidenceState::Unsupported,
            GraphEvidenceState::ApproximateCandidate,
        ];
        for state in &non_verified {
            assert!(
                !state.allows_binding(),
                "{:?} must not allow binding",
                state
            );
            assert!(state.is_review_only(), "{:?} must be review-only", state);
        }
    }

    #[test]
    fn verified_is_not_review_only() {
        assert!(!GraphEvidenceState::Verified.is_review_only());
    }

    // ── Public model projection tests ────────────────────────────────

    #[test]
    fn to_public_maps_verified() {
        assert_eq!(
            GraphEvidenceState::Verified.to_public(),
            PublicGraphEvidenceState::Verified
        );
    }

    #[test]
    fn to_public_maps_not_found() {
        assert_eq!(
            GraphEvidenceState::NotFound.to_public(),
            PublicGraphEvidenceState::NotFound
        );
    }

    #[test]
    fn to_public_collapses_unknown_variants() {
        assert_eq!(
            GraphEvidenceState::UnknownNotIndexed.to_public(),
            PublicGraphEvidenceState::Unknown
        );
        assert_eq!(
            GraphEvidenceState::UnknownStale.to_public(),
            PublicGraphEvidenceState::Unknown
        );
        assert_eq!(
            GraphEvidenceState::ApproximateCandidate.to_public(),
            PublicGraphEvidenceState::Unknown
        );
    }

    #[test]
    fn to_public_maps_ambiguous() {
        assert_eq!(
            GraphEvidenceState::Ambiguous.to_public(),
            PublicGraphEvidenceState::Ambiguous
        );
    }

    #[test]
    fn to_public_maps_unsupported_to_not_supported() {
        assert_eq!(
            GraphEvidenceState::Unsupported.to_public(),
            PublicGraphEvidenceState::NotSupported
        );
    }

    #[test]
    fn to_public_preserves_authority_boundary() {
        for state in [
            GraphEvidenceState::Verified,
            GraphEvidenceState::NotFound,
            GraphEvidenceState::UnknownNotIndexed,
            GraphEvidenceState::UnknownStale,
            GraphEvidenceState::Ambiguous,
            GraphEvidenceState::Unsupported,
            GraphEvidenceState::ApproximateCandidate,
        ] {
            let public = state.to_public();
            assert_eq!(
                state.allows_binding(),
                public.allows_binding(),
                "authority boundary mismatch for {:?} -> {:?}",
                state,
                public
            );
        }
    }

    #[test]
    fn public_model_only_verified_allows_binding() {
        assert!(PublicGraphEvidenceState::Verified.allows_binding());
        for state in [
            PublicGraphEvidenceState::NotFound,
            PublicGraphEvidenceState::Unknown,
            PublicGraphEvidenceState::NotSupported,
            PublicGraphEvidenceState::Ambiguous,
        ] {
            assert!(
                !state.allows_binding(),
                "public {:?} must not allow binding",
                state
            );
            assert!(
                state.is_review_only(),
                "public {:?} must be review-only",
                state
            );
        }
    }

    #[test]
    fn public_graph_evidence_state_serializes_to_snake_case() {
        let cases = vec![
            (PublicGraphEvidenceState::Verified, "\"verified\""),
            (PublicGraphEvidenceState::NotFound, "\"not_found\""),
            (PublicGraphEvidenceState::Unknown, "\"unknown\""),
            (PublicGraphEvidenceState::NotSupported, "\"not_supported\""),
            (PublicGraphEvidenceState::Ambiguous, "\"ambiguous\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn public_graph_evidence_state_round_trips() {
        for variant in [
            PublicGraphEvidenceState::Verified,
            PublicGraphEvidenceState::NotFound,
            PublicGraphEvidenceState::Unknown,
            PublicGraphEvidenceState::NotSupported,
            PublicGraphEvidenceState::Ambiguous,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: PublicGraphEvidenceState = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn graph_readiness_serializes_to_snake_case() {
        let cases = vec![
            (GraphReadiness::Ready, "\"ready\""),
            (GraphReadiness::Indexing, "\"indexing\""),
            (GraphReadiness::Stale, "\"stale\""),
            (GraphReadiness::Failed, "\"failed\""),
            (GraphReadiness::NotConfigured, "\"not_configured\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn graph_readiness_round_trips() {
        for variant in [
            GraphReadiness::Ready,
            GraphReadiness::Indexing,
            GraphReadiness::Stale,
            GraphReadiness::Failed,
            GraphReadiness::NotConfigured,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: GraphReadiness = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn snapshot_resolution_resolved_serializes_with_tag() {
        let snap = GraphSnapshot {
            graph_snapshot_id: "gs-abc".into(),
            repo_id: "owner/repo".into(),
            commit_sha: "abc123".into(),
            index_version: "v1".into(),
            ref_name: None,
            merge_base_sha: None,
            indexed_at: None,
            index_source: None,
        };
        let res = SnapshotResolution::Resolved {
            snapshot: snap.clone(),
        };
        let json = serde_json::to_value(&res).unwrap();
        assert_eq!(json["status"], "resolved");
        assert_eq!(json["snapshot"]["graph_snapshot_id"], "gs-abc");
        assert!(res.is_resolved());
        assert_eq!(res.snapshot().unwrap().commit_sha, "abc123");
        assert_eq!(res.readiness(), GraphReadiness::Ready);
    }

    #[test]
    fn snapshot_resolution_ref_not_available_serializes() {
        let res = SnapshotResolution::RefNotAvailable {
            requested_ref: "origin/feature".into(),
            detail: "ref not found locally".into(),
        };
        let json = serde_json::to_value(&res).unwrap();
        assert_eq!(json["status"], "ref_not_available");
        assert_eq!(json["requested_ref"], "origin/feature");
        assert!(!res.is_resolved());
        assert!(res.snapshot().is_none());
        assert_eq!(res.readiness(), GraphReadiness::Failed);
    }

    #[test]
    fn snapshot_resolution_stale_index_readiness() {
        let res = SnapshotResolution::StaleIndex {
            requested_ref: "main".into(),
            detail: "index version mismatch".into(),
        };
        assert_eq!(res.readiness(), GraphReadiness::Stale);
    }

    #[test]
    fn snapshot_resolution_hosted_required_readiness() {
        let res = SnapshotResolution::HostedRequired {
            requested_ref: "pr/42".into(),
            detail: "requires team graph".into(),
        };
        assert_eq!(res.readiness(), GraphReadiness::NotConfigured);
    }

    // ── EvidenceReference serialization ──────────────────────────────

    #[test]
    fn evidence_reference_serializes_source_kind() {
        let er = EvidenceReference::new(
            "snap-001".into(),
            SourceKind::Tracker,
            "must use HTTPS".into(),
        );
        let json = serde_json::to_value(&er).unwrap();
        assert_eq!(json["source_kind"], "tracker");
        assert_eq!(json["snapshot_id"], "snap-001");
        assert_eq!(json["excerpt"], "must use HTTPS");
        assert!(json.get("pointer").is_none());
        assert!(json.get("metadata").is_none());
    }

    #[test]
    fn evidence_reference_round_trips_with_pointer_and_metadata() {
        let mut er = EvidenceReference::new(
            "snap-002".into(),
            SourceKind::Diff,
            "+fn new_handler()".into(),
        );
        er.pointer = Some("@@ -10,3 +10,5 @@".into());
        er.metadata = Some(serde_json::json!({"hunk_index": 0}));

        let json = serde_json::to_string(&er).unwrap();
        let back: EvidenceReference = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_kind, SourceKind::Diff);
        assert_eq!(back.pointer.as_deref(), Some("@@ -10,3 +10,5 @@"));
        assert_eq!(back.metadata.unwrap()["hunk_index"], 0);
    }

    #[test]
    fn evidence_reference_all_source_kinds_round_trip() {
        let kinds = vec![
            (SourceKind::Text, "text"),
            (SourceKind::Tracker, "tracker"),
            (SourceKind::McpSession, "mcp_session"),
            (SourceKind::CommandRun, "command_run"),
            (SourceKind::PullRequest, "pull_request"),
            (SourceKind::File, "file"),
            (SourceKind::Diff, "diff"),
            (SourceKind::CodeSnapshot, "code_snapshot"),
        ];
        for (kind, expected_str) in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", expected_str));
            let back: SourceKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn evidence_reference_omits_optional_fields_when_none() {
        let er = EvidenceReference::new(
            "snap-003".into(),
            SourceKind::McpSession,
            "session turn 5".into(),
        );
        let json = serde_json::to_value(&er).unwrap();
        assert!(json.get("pointer").is_none());
        assert!(json.get("metadata").is_none());
    }

    #[test]
    fn evidence_reference_deserializes_without_optional_fields() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "snapshot_id": "snap-min",
            "source_kind": "command_run",
            "excerpt": "exit 0",
            "captured_at": "2026-01-01T00:00:00Z"
        }"#;
        let er: EvidenceReference = serde_json::from_str(json).unwrap();
        assert_eq!(er.source_kind, SourceKind::CommandRun);
        assert_eq!(er.excerpt, "exit 0");
        assert!(er.pointer.is_none());
        assert!(er.metadata.is_none());
    }

    #[test]
    fn submission_provenance_from_tool_request_populates_fields() {
        let authority = crate::tool_request::AuthorityContext {
            actor_id: "agent-42".to_string(),
            auth_method: crate::tool_request::AuthMethod::McpSession,
            session_id: Some("sess-99".to_string()),
            workspace: Some("/repo".to_string()),
            policy_scope: Vec::new(),
            audit_metadata: Default::default(),
        };
        let req_id = uuid::Uuid::new_v4();
        let prov = SubmissionProvenance::from_tool_request(
            req_id,
            &authority,
            Some("https://jira.example.com/PROJ-1"),
            Some("jira"),
        );
        assert_eq!(prov.submitting_surface.as_deref(), Some("tool_request"));
        assert_eq!(prov.source_kind.as_deref(), Some("jira"));
        assert_eq!(prov.actor_id.as_deref(), Some("agent-42"));
        assert_eq!(prov.session_id.as_deref(), Some("sess-99"));
        assert_eq!(
            prov.request_id.as_deref(),
            Some(req_id.to_string().as_str())
        );
        assert_eq!(
            prov.source_uri.as_deref(),
            Some("https://jira.example.com/PROJ-1")
        );
    }

    #[test]
    fn submission_provenance_from_external_adapter() {
        let prov = SubmissionProvenance::from_external_adapter("slack", "https://slack.com/msg/1");
        assert_eq!(prov.submitting_surface.as_deref(), Some("external_adapter"));
        assert_eq!(prov.source_kind.as_deref(), Some("slack"));
        assert!(prov.actor_id.is_none());
        assert!(prov.session_id.is_none());
        assert!(prov.request_id.is_none());
        assert_eq!(prov.source_uri.as_deref(), Some("https://slack.com/msg/1"));
    }

    #[test]
    fn submission_provenance_serializes_with_optional_fields_omitted() {
        let prov = SubmissionProvenance::from_external_adapter("github", "https://github.com/pr/1");
        let json = serde_json::to_value(&prov).unwrap();
        // Optional None fields should be absent
        assert!(json.get("actor_id").is_none());
        assert!(json.get("session_id").is_none());
        assert!(json.get("request_id").is_none());
        // Present fields should exist
        assert_eq!(json["submitting_surface"], "external_adapter");
        assert_eq!(json["source_kind"], "github");
        assert!(json.get("submitted_at").is_some());
    }

    #[test]
    fn submission_provenance_deserializes_without_optional_fields() {
        let json = r#"{
            "submitted_at": "2026-01-01T00:00:00Z"
        }"#;
        let prov: SubmissionProvenance = serde_json::from_str(json).unwrap();
        assert!(prov.submitting_surface.is_none());
        assert!(prov.source_kind.is_none());
        assert!(prov.actor_id.is_none());
        assert!(prov.session_id.is_none());
        assert!(prov.request_id.is_none());
        assert!(prov.source_uri.is_none());
    }
}
