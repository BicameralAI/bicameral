//! Recall taxonomy registry — governed controlled values for recall-relevant metadata.
//!
//! Every recall-relevant enum value must be registered in the taxonomy registry.
//! Ad-hoc values are rejected at validation time. The registry provides:
//!
//! - **Versioning**: every registry snapshot carries a [`TaxonomyVersion`].
//! - **Aliasing**: legacy or shorthand names resolve to a canonical value.
//! - **Deprecation**: values can be deprecated with a reason and successor.
//! - **Canonicalization**: alias resolution always yields the canonical name.
//! - **Ownership**: each value declares an owner (team, module, or spec).
//! - **Change policy**: controls whether a value is immutable, governed, or extensible.
//! - **Migration**: deprecated values carry an optional migration target version.
//!
//! The registry shape is consumed by `RecallTrace` (issue #556) and downstream
//! recall resolvers. This module defines the shape and initial controlled values
//! only — it does not implement the recall resolver.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Taxonomy version
// ---------------------------------------------------------------------------

/// Semantic version for a taxonomy registry snapshot.
///
/// `RecallTrace` references this version to ensure recall queries operate
/// against a known, governed set of controlled values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaxonomyVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl TaxonomyVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The initial alpha taxonomy version.
    pub fn alpha() -> Self {
        Self::new(0, 1, 0)
    }
}

impl std::fmt::Display for TaxonomyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ---------------------------------------------------------------------------
// Taxonomy kind — the 16+ recall-relevant dimensions
// ---------------------------------------------------------------------------

/// Which taxonomy dimension a controlled value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyKind {
    DecisionType,
    LifecycleTag,
    CapabilityFamily,
    ExternalTarget,
    AuthorityBoundary,
    AffectedSurface,
    RiskClass,
    ReasonCode,
    ResultClass,
    ScopeLevel,
    FreshnessState,
    EvidenceState,
    MetadataQuality,
    ApplicabilityStatus,
    SourceClass,
    RouteClass,
    PreferenceGuard,
}

impl TaxonomyKind {
    /// All taxonomy kinds in declaration order.
    pub fn all() -> &'static [TaxonomyKind] {
        &[
            TaxonomyKind::DecisionType,
            TaxonomyKind::LifecycleTag,
            TaxonomyKind::CapabilityFamily,
            TaxonomyKind::ExternalTarget,
            TaxonomyKind::AuthorityBoundary,
            TaxonomyKind::AffectedSurface,
            TaxonomyKind::RiskClass,
            TaxonomyKind::ReasonCode,
            TaxonomyKind::ResultClass,
            TaxonomyKind::ScopeLevel,
            TaxonomyKind::FreshnessState,
            TaxonomyKind::EvidenceState,
            TaxonomyKind::MetadataQuality,
            TaxonomyKind::ApplicabilityStatus,
            TaxonomyKind::SourceClass,
            TaxonomyKind::RouteClass,
            TaxonomyKind::PreferenceGuard,
        ]
    }
}

// ---------------------------------------------------------------------------
// Change policy
// ---------------------------------------------------------------------------

/// How a controlled value may be changed after registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangePolicy {
    /// Value is permanent — removal requires a major version bump.
    Immutable,
    /// Value changes require explicit governance review.
    Governed,
    /// New values in this kind can be added without review (minor bump).
    Extensible,
}

// ---------------------------------------------------------------------------
// Deprecation metadata
// ---------------------------------------------------------------------------

/// Deprecation metadata for a controlled value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeprecationInfo {
    /// Version in which this value was deprecated.
    pub deprecated_since: TaxonomyVersion,
    /// Canonical name of the replacement value, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    /// Version at which this value may be removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removal_target: Option<TaxonomyVersion>,
    /// Human-readable reason for deprecation.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Controlled value entry
// ---------------------------------------------------------------------------

/// A single registered value in the taxonomy registry.
///
/// Every value has a canonical name, taxonomy kind, optional aliases,
/// deprecation metadata, an owner, a change policy, and the version
/// in which it was introduced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlledValue {
    /// The canonical (primary) name used in recall queries and storage.
    pub canonical_name: String,
    /// Which taxonomy dimension this value belongs to.
    pub kind: TaxonomyKind,
    /// Alternative names that resolve to this canonical value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Deprecation metadata, if this value is deprecated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecation: Option<DeprecationInfo>,
    /// Owner of this value definition (team, module, or spec reference).
    pub owner: String,
    /// How this value may be changed after registration.
    pub change_policy: ChangePolicy,
    /// Human-readable description of this value.
    pub description: String,
    /// Version in which this value was introduced.
    pub introduced_in: TaxonomyVersion,
}

// ---------------------------------------------------------------------------
// Validation result
// ---------------------------------------------------------------------------

/// Result of resolving a value name against the taxonomy registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult<'a> {
    /// Resolved to a canonical (non-deprecated) value.
    Canonical(&'a ControlledValue),
    /// Resolved to a deprecated value (still valid but flagged).
    Deprecated(&'a ControlledValue),
    /// Resolved via alias to a canonical value.
    Alias {
        canonical: &'a ControlledValue,
        alias_used: String,
    },
    /// Resolved via alias to a deprecated value.
    DeprecatedAlias {
        canonical: &'a ControlledValue,
        alias_used: String,
    },
    /// Value not found in the registry — ad-hoc value rejected.
    Unregistered,
}

impl ResolveResult<'_> {
    /// Whether the resolution found a registered value (canonical or alias).
    pub fn is_registered(&self) -> bool {
        !matches!(self, ResolveResult::Unregistered)
    }

    /// Whether the resolved value is deprecated.
    pub fn is_deprecated(&self) -> bool {
        matches!(
            self,
            ResolveResult::Deprecated(_) | ResolveResult::DeprecatedAlias { .. }
        )
    }

    /// The canonical name if resolved, `None` if unregistered.
    pub fn canonical_name(&self) -> Option<&str> {
        match self {
            ResolveResult::Canonical(v)
            | ResolveResult::Deprecated(v)
            | ResolveResult::Alias { canonical: v, .. }
            | ResolveResult::DeprecatedAlias { canonical: v, .. } => Some(&v.canonical_name),
            ResolveResult::Unregistered => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Taxonomy registry
// ---------------------------------------------------------------------------

/// The top-level versioned taxonomy registry.
///
/// Holds all registered controlled values across all taxonomy kinds.
/// Provides lookup, alias resolution, and validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyRegistry {
    /// Version of this registry snapshot.
    pub version: TaxonomyVersion,
    /// All registered controlled values.
    pub values: Vec<ControlledValue>,
    /// Precomputed alias-to-canonical index (kind, alias) → index into `values`.
    /// Rebuilt on construction; not serialized.
    #[serde(skip)]
    alias_index: HashMap<(TaxonomyKind, String), usize>,
    /// Precomputed canonical-name index (kind, canonical) → index into `values`.
    #[serde(skip)]
    canonical_index: HashMap<(TaxonomyKind, String), usize>,
}

impl TaxonomyRegistry {
    /// Build a registry from a version and a set of controlled values.
    ///
    /// Rebuilds internal lookup indices.
    pub fn new(version: TaxonomyVersion, values: Vec<ControlledValue>) -> Self {
        let mut registry = Self {
            version,
            values,
            alias_index: HashMap::new(),
            canonical_index: HashMap::new(),
        };
        registry.rebuild_indices();
        registry
    }

    /// Rebuild internal lookup indices from the current `values`.
    fn rebuild_indices(&mut self) {
        self.canonical_index.clear();
        self.alias_index.clear();
        for (i, val) in self.values.iter().enumerate() {
            self.canonical_index
                .insert((val.kind, val.canonical_name.clone()), i);
            for alias in &val.aliases {
                self.alias_index.insert((val.kind, alias.clone()), i);
            }
        }
    }

    /// Resolve a name within a given taxonomy kind.
    ///
    /// Checks canonical names first, then aliases. Returns [`ResolveResult`]
    /// indicating whether the value is canonical, deprecated, aliased, or
    /// unregistered.
    pub fn resolve(&self, kind: TaxonomyKind, name: &str) -> ResolveResult<'_> {
        // Check canonical name first
        if let Some(&idx) = self.canonical_index.get(&(kind, name.to_owned())) {
            let val = &self.values[idx];
            if val.deprecation.is_some() {
                return ResolveResult::Deprecated(val);
            }
            return ResolveResult::Canonical(val);
        }
        // Check aliases
        if let Some(&idx) = self.alias_index.get(&(kind, name.to_owned())) {
            let val = &self.values[idx];
            if val.deprecation.is_some() {
                return ResolveResult::DeprecatedAlias {
                    canonical: val,
                    alias_used: name.to_owned(),
                };
            }
            return ResolveResult::Alias {
                canonical: val,
                alias_used: name.to_owned(),
            };
        }
        ResolveResult::Unregistered
    }

    /// Validate that a name is registered for the given kind.
    ///
    /// Returns `true` if the name resolves to any registered value
    /// (canonical or alias, even if deprecated).
    pub fn is_registered(&self, kind: TaxonomyKind, name: &str) -> bool {
        self.resolve(kind, name).is_registered()
    }

    /// List all registered values for a given taxonomy kind.
    pub fn values_for_kind(&self, kind: TaxonomyKind) -> Vec<&ControlledValue> {
        self.values.iter().filter(|v| v.kind == kind).collect()
    }

    /// List all non-deprecated values for a given taxonomy kind.
    pub fn active_values_for_kind(&self, kind: TaxonomyKind) -> Vec<&ControlledValue> {
        self.values
            .iter()
            .filter(|v| v.kind == kind && v.deprecation.is_none())
            .collect()
    }

    /// The number of registered values across all kinds.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The number of taxonomy kinds that have at least one registered value.
    pub fn populated_kind_count(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for val in &self.values {
            seen.insert(val.kind);
        }
        seen.len()
    }
}

// ---------------------------------------------------------------------------
// Alpha registry builder
// ---------------------------------------------------------------------------

/// Build the alpha (v0.1.0) taxonomy registry with initial controlled values.
///
/// This is the canonical starting point for recall taxonomy. All values
/// are governed and owned by `bicameral-api/recall`.
pub fn alpha_registry() -> TaxonomyRegistry {
    let v = TaxonomyVersion::alpha();
    let owner = "bicameral-api/recall".to_owned();

    let mut values = Vec::new();

    // -- DecisionType --
    for (name, aliases, desc) in [
        (
            "product",
            vec!["product_decision", "l1"],
            "Product-level decision (what and why)",
        ),
        (
            "architecture",
            vec!["arch", "architectural", "l2"],
            "Architecture-level decision (structural how)",
        ),
        (
            "implementation",
            vec!["impl", "tactical", "l3"],
            "Implementation-level decision (tactical how)",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::DecisionType,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- LifecycleTag --
    for (name, aliases, desc) in [
        ("proposed", vec!["new"], "Newly proposed, awaiting review"),
        ("active", vec!["tracked"], "Actively tracked and governed"),
        (
            "superseded",
            vec!["replaced"],
            "Replaced by a newer decision",
        ),
        ("retired", vec!["archived"], "No longer in governance scope"),
        (
            "collision_pending",
            vec!["conflicted"],
            "Pending collision resolution",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::LifecycleTag,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- CapabilityFamily --
    for (name, aliases, desc) in [
        (
            "grounding",
            vec!["code_grounding"],
            "Code grounding and binding capabilities",
        ),
        (
            "ingestion",
            vec!["evidence_ingestion"],
            "Evidence ingestion from external sources",
        ),
        (
            "projection",
            vec!["reasoning_projection"],
            "Reasoning projection and candidate extraction",
        ),
        (
            "governance",
            vec!["review", "signoff"],
            "Governance review and signoff",
        ),
        (
            "recall",
            vec!["decision_recall"],
            "Decision recall and lookup",
        ),
        (
            "compliance",
            vec!["drift_detection"],
            "Compliance checking and drift detection",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::CapabilityFamily,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- ExternalTarget --
    for (name, aliases, desc) in [
        ("github", vec!["gh"], "GitHub repository or issue"),
        ("slack", vec![], "Slack channel or thread"),
        ("jira", vec![], "Jira ticket or board"),
        ("linear", vec![], "Linear issue or project"),
        (
            "meeting",
            vec!["meeting_notes"],
            "Meeting notes or transcript",
        ),
        ("document", vec!["doc", "gdoc"], "External document"),
        ("mcp_session", vec!["mcp"], "MCP tool session"),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::ExternalTarget,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Extensible,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- AuthorityBoundary --
    for (name, aliases, desc) in [
        (
            "retrieval_only",
            vec!["a0", "read_only"],
            "May appear in inspection/recall only",
        ),
        (
            "recommendation",
            vec!["a1", "suggest"],
            "May influence rankings and suggestions",
        ),
        (
            "draft",
            vec!["a2"],
            "May shape drafts or proposed bindings for review",
        ),
        (
            "local_mutation",
            vec!["a3"],
            "May drive local workflow mutation",
        ),
        (
            "external_action",
            vec!["a4"],
            "May trigger external system actions",
        ),
        (
            "governance_change",
            vec!["a5"],
            "May modify governance policy",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::AuthorityBoundary,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Immutable,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- AffectedSurface --
    for (name, aliases, desc) in [
        (
            "ledger",
            vec!["decision_ledger"],
            "Decision ledger and canonical state",
        ),
        ("graph", vec!["code_graph"], "Code graph and symbol index"),
        ("dashboard", vec!["ui"], "Dashboard and user-facing views"),
        ("gateway", vec!["api"], "Gateway API and HTTP surface"),
        (
            "event_store",
            vec!["store"],
            "Event store and persistence layer",
        ),
        (
            "integration",
            vec!["connector"],
            "External integration connectors",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::AffectedSurface,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- RiskClass --
    for (name, aliases, desc) in [
        (
            "authority_escalation",
            vec!["escalation"],
            "Risk of unintended authority escalation",
        ),
        (
            "data_loss",
            vec!["loss"],
            "Risk of canonical data loss or corruption",
        ),
        (
            "drift",
            vec!["semantic_drift"],
            "Risk of semantic drift between decision and code",
        ),
        (
            "stale_evidence",
            vec!["staleness"],
            "Risk from evidence that is no longer current",
        ),
        (
            "collision",
            vec!["conflict"],
            "Risk from conflicting decisions on the same scope",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::RiskClass,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- ReasonCode --
    for (name, aliases, desc) in [
        (
            "evidence_cited",
            vec!["cited"],
            "Decision supported by cited evidence",
        ),
        (
            "scope_match",
            vec!["scope"],
            "Decision matches the recall scope",
        ),
        (
            "authority_verified",
            vec!["authority"],
            "Authority chain has been verified",
        ),
        (
            "collision_detected",
            vec!["collision"],
            "Collision with another decision detected",
        ),
        (
            "supersession",
            vec!["superseded"],
            "Decision supersedes or is superseded by another",
        ),
        (
            "freshness_lapse",
            vec!["stale"],
            "Evidence freshness has lapsed",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::ReasonCode,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- ResultClass --
    for (name, aliases, desc) in [
        ("included", vec!["hit"], "Value included in recall results"),
        (
            "excluded",
            vec!["miss"],
            "Value excluded from recall results",
        ),
        (
            "deferred",
            vec!["pending"],
            "Evaluation deferred to a later stage",
        ),
        (
            "ambiguous",
            vec!["uncertain"],
            "Result is ambiguous and requires human review",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::ResultClass,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- ScopeLevel --
    for (name, aliases, desc) in [
        (
            "workspace",
            vec!["repo"],
            "Entire workspace or repository scope",
        ),
        ("module", vec!["crate"], "Module or crate scope"),
        ("file", vec!["source_file"], "Single file scope"),
        (
            "symbol",
            vec!["function", "struct"],
            "Symbol-level (function, struct, etc.) scope",
        ),
        ("region", vec!["range", "span"], "Code region or line range"),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::ScopeLevel,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- FreshnessState --
    for (name, aliases, desc) in [
        (
            "current",
            vec!["fresh"],
            "Evidence or source is current and verified",
        ),
        (
            "possibly_stale",
            vec!["aging"],
            "Evidence may be stale but has not been confirmed",
        ),
        ("stale", vec!["expired"], "Evidence is confirmed stale"),
        (
            "unknown",
            vec!["unverified"],
            "Freshness cannot be determined",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::FreshnessState,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- EvidenceState --
    for (name, aliases, desc) in [
        (
            "verified",
            vec!["confirmed"],
            "Evidence verified against current graph snapshot",
        ),
        (
            "approximate",
            vec!["unverified"],
            "Evidence is approximate, not graph-verified",
        ),
        ("missing", vec!["absent"], "Expected evidence is missing"),
        (
            "conflicting",
            vec!["contradictory"],
            "Evidence conflicts with other evidence",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::EvidenceState,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- MetadataQuality --
    for (name, aliases, desc) in [
        (
            "complete",
            vec!["full"],
            "All required metadata fields present and valid",
        ),
        (
            "partial",
            vec!["incomplete"],
            "Some metadata fields missing or unvalidated",
        ),
        (
            "minimal",
            vec!["sparse"],
            "Only minimal required fields present",
        ),
        (
            "degraded",
            vec!["corrupted"],
            "Metadata is present but quality is degraded",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::MetadataQuality,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- ApplicabilityStatus --
    for (name, aliases, desc) in [
        (
            "applicable",
            vec!["relevant"],
            "Decision is applicable to the current recall context",
        ),
        (
            "not_applicable",
            vec!["irrelevant"],
            "Decision is not applicable to the current context",
        ),
        (
            "conditionally_applicable",
            vec!["conditional"],
            "Applicability depends on runtime or scope conditions",
        ),
        (
            "undetermined",
            vec!["unknown"],
            "Applicability has not been evaluated",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::ApplicabilityStatus,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- SourceClass --
    for (name, aliases, desc) in [
        (
            "operational",
            vec!["ops"],
            "Source from operational tooling (Jira, Slack, etc.)",
        ),
        (
            "code",
            vec!["source_code"],
            "Source from code or code review",
        ),
        (
            "meeting",
            vec!["transcript"],
            "Source from a meeting or transcript",
        ),
        (
            "document",
            vec!["spec", "adr"],
            "Source from a document, spec, or ADR",
        ),
        (
            "agent",
            vec!["mcp", "ai"],
            "Source from an AI agent or MCP session",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::SourceClass,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Extensible,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- RouteClass --
    for (name, aliases, desc) in [
        (
            "signoff",
            vec!["review"],
            "Route to signoff/review pipeline",
        ),
        (
            "grounding",
            vec!["bind"],
            "Route to grounding/binding pipeline",
        ),
        (
            "compliance",
            vec!["verify"],
            "Route to compliance verification",
        ),
        (
            "advisory",
            vec!["info"],
            "Advisory route — no governance action required",
        ),
        ("escalation", vec!["escalate"], "Route to human escalation"),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::RouteClass,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    // -- PreferenceGuard --
    for (name, aliases, desc) in [
        (
            "require_human_review",
            vec!["human_gate"],
            "Requires human review before proceeding",
        ),
        (
            "require_evidence",
            vec!["evidence_gate"],
            "Requires cited evidence before acceptance",
        ),
        (
            "require_scope_match",
            vec!["scope_gate"],
            "Requires scope match verification",
        ),
        (
            "allow_advisory",
            vec!["advisory_ok"],
            "Advisory results are permitted without gating",
        ),
        (
            "block_deprecated",
            vec!["no_deprecated"],
            "Deprecated values are blocked in recall results",
        ),
    ] {
        values.push(ControlledValue {
            canonical_name: name.into(),
            kind: TaxonomyKind::PreferenceGuard,
            aliases: aliases.into_iter().map(String::from).collect(),
            deprecation: None,
            owner: owner.clone(),
            change_policy: ChangePolicy::Governed,
            description: desc.into(),
            introduced_in: v.clone(),
        });
    }

    TaxonomyRegistry::new(v, values)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Registry shape tests --

    #[test]
    fn alpha_registry_version_is_0_1_0() {
        let reg = alpha_registry();
        assert_eq!(reg.version, TaxonomyVersion::new(0, 1, 0));
        assert_eq!(reg.version.to_string(), "0.1.0");
    }

    #[test]
    fn alpha_registry_covers_all_taxonomy_kinds() {
        let reg = alpha_registry();
        assert_eq!(reg.populated_kind_count(), TaxonomyKind::all().len());
        for kind in TaxonomyKind::all() {
            assert!(
                !reg.values_for_kind(*kind).is_empty(),
                "Taxonomy kind {:?} has no registered values",
                kind
            );
        }
    }

    #[test]
    fn alpha_registry_is_not_empty() {
        let reg = alpha_registry();
        assert!(!reg.is_empty());
        assert!(reg.len() > 0);
    }

    #[test]
    fn taxonomy_kind_all_returns_17_kinds() {
        assert_eq!(TaxonomyKind::all().len(), 17);
    }

    // -- Versioning tests --

    #[test]
    fn taxonomy_version_display() {
        let v = TaxonomyVersion::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn taxonomy_version_alpha() {
        let v = TaxonomyVersion::alpha();
        assert_eq!(v, TaxonomyVersion::new(0, 1, 0));
    }

    #[test]
    fn taxonomy_version_serializes() {
        let v = TaxonomyVersion::new(0, 1, 0);
        let json = serde_json::to_string(&v).unwrap();
        let deserialized: TaxonomyVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, deserialized);
    }

    #[test]
    fn all_values_reference_alpha_version() {
        let reg = alpha_registry();
        let alpha = TaxonomyVersion::alpha();
        for val in &reg.values {
            assert_eq!(
                val.introduced_in, alpha,
                "Value {} introduced_in does not match alpha version",
                val.canonical_name
            );
        }
    }

    // -- Canonical resolution tests --

    #[test]
    fn canonical_name_resolves_directly() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::DecisionType, "product");
        assert!(result.is_registered());
        assert!(!result.is_deprecated());
        assert_eq!(result.canonical_name(), Some("product"));
    }

    #[test]
    fn all_canonical_names_resolve() {
        let reg = alpha_registry();
        for val in &reg.values {
            let result = reg.resolve(val.kind, &val.canonical_name);
            assert!(
                result.is_registered(),
                "Canonical name '{}' for {:?} did not resolve",
                val.canonical_name,
                val.kind
            );
        }
    }

    // -- Alias resolution tests --

    #[test]
    fn alias_resolves_to_canonical() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::DecisionType, "arch");
        assert!(result.is_registered());
        assert_eq!(result.canonical_name(), Some("architecture"));
        assert!(matches!(result, ResolveResult::Alias { .. }));
    }

    #[test]
    fn decision_type_l1_alias_resolves_to_product() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::DecisionType, "l1");
        assert!(result.is_registered());
        assert_eq!(result.canonical_name(), Some("product"));
    }

    #[test]
    fn decision_type_l2_alias_resolves_to_architecture() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::DecisionType, "l2");
        assert!(result.is_registered());
        assert_eq!(result.canonical_name(), Some("architecture"));
    }

    #[test]
    fn external_target_gh_alias_resolves() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::ExternalTarget, "gh");
        assert!(result.is_registered());
        assert_eq!(result.canonical_name(), Some("github"));
    }

    #[test]
    fn authority_boundary_a0_alias_resolves() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::AuthorityBoundary, "a0");
        assert!(result.is_registered());
        assert_eq!(result.canonical_name(), Some("retrieval_only"));
    }

    // -- Unregistered value rejection --

    #[test]
    fn unregistered_value_is_rejected() {
        let reg = alpha_registry();
        let result = reg.resolve(TaxonomyKind::DecisionType, "nonexistent_type");
        assert!(!result.is_registered());
        assert!(matches!(result, ResolveResult::Unregistered));
        assert_eq!(result.canonical_name(), None);
    }

    #[test]
    fn ad_hoc_value_rejected_for_every_kind() {
        let reg = alpha_registry();
        for kind in TaxonomyKind::all() {
            let result = reg.resolve(*kind, "__ad_hoc_not_registered__");
            assert!(
                !result.is_registered(),
                "Ad-hoc value should be rejected for {:?}",
                kind
            );
        }
    }

    #[test]
    fn wrong_kind_rejects_valid_name() {
        let reg = alpha_registry();
        // "product" is a valid DecisionType but not a valid RiskClass
        let result = reg.resolve(TaxonomyKind::RiskClass, "product");
        assert!(!result.is_registered());
    }

    // -- Deprecation tests --

    #[test]
    fn deprecated_value_is_flagged() {
        let v = TaxonomyVersion::alpha();
        let deprecated_val = ControlledValue {
            canonical_name: "old_value".into(),
            kind: TaxonomyKind::DecisionType,
            aliases: vec!["legacy".into()],
            deprecation: Some(DeprecationInfo {
                deprecated_since: v.clone(),
                superseded_by: Some("product".into()),
                removal_target: Some(TaxonomyVersion::new(1, 0, 0)),
                reason: "Replaced by product".into(),
            }),
            owner: "test".into(),
            change_policy: ChangePolicy::Governed,
            description: "Deprecated test value".into(),
            introduced_in: v.clone(),
        };

        let reg = TaxonomyRegistry::new(v, vec![deprecated_val]);
        let result = reg.resolve(TaxonomyKind::DecisionType, "old_value");
        assert!(result.is_registered());
        assert!(result.is_deprecated());
        assert!(matches!(result, ResolveResult::Deprecated(_)));
    }

    #[test]
    fn deprecated_alias_is_flagged() {
        let v = TaxonomyVersion::alpha();
        let deprecated_val = ControlledValue {
            canonical_name: "old_value".into(),
            kind: TaxonomyKind::DecisionType,
            aliases: vec!["legacy".into()],
            deprecation: Some(DeprecationInfo {
                deprecated_since: v.clone(),
                superseded_by: Some("product".into()),
                removal_target: None,
                reason: "Replaced by product".into(),
            }),
            owner: "test".into(),
            change_policy: ChangePolicy::Governed,
            description: "Deprecated test value".into(),
            introduced_in: v.clone(),
        };

        let reg = TaxonomyRegistry::new(v, vec![deprecated_val]);
        let result = reg.resolve(TaxonomyKind::DecisionType, "legacy");
        assert!(result.is_registered());
        assert!(result.is_deprecated());
        assert!(matches!(result, ResolveResult::DeprecatedAlias { .. }));
        assert_eq!(result.canonical_name(), Some("old_value"));
    }

    #[test]
    fn deprecation_info_serializes() {
        let info = DeprecationInfo {
            deprecated_since: TaxonomyVersion::new(0, 1, 0),
            superseded_by: Some("new_value".into()),
            removal_target: Some(TaxonomyVersion::new(1, 0, 0)),
            reason: "Test deprecation".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: DeprecationInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, deserialized);
    }

    // -- Value constraint tests --

    #[test]
    fn every_value_has_nonempty_canonical_name() {
        let reg = alpha_registry();
        for val in &reg.values {
            assert!(
                !val.canonical_name.is_empty(),
                "Empty canonical name found in {:?}",
                val.kind
            );
        }
    }

    #[test]
    fn every_value_has_nonempty_description() {
        let reg = alpha_registry();
        for val in &reg.values {
            assert!(
                !val.description.is_empty(),
                "Empty description for {} in {:?}",
                val.canonical_name,
                val.kind
            );
        }
    }

    #[test]
    fn every_value_has_nonempty_owner() {
        let reg = alpha_registry();
        for val in &reg.values {
            assert!(
                !val.owner.is_empty(),
                "Empty owner for {} in {:?}",
                val.canonical_name,
                val.kind
            );
        }
    }

    #[test]
    fn no_duplicate_canonical_names_within_kind() {
        let reg = alpha_registry();
        for kind in TaxonomyKind::all() {
            let values = reg.values_for_kind(*kind);
            let mut seen = std::collections::HashSet::new();
            for val in values {
                assert!(
                    seen.insert(&val.canonical_name),
                    "Duplicate canonical name '{}' in {:?}",
                    val.canonical_name,
                    kind
                );
            }
        }
    }

    #[test]
    fn no_alias_collides_with_canonical_name_in_same_kind() {
        let reg = alpha_registry();
        for kind in TaxonomyKind::all() {
            let values = reg.values_for_kind(*kind);
            let canonical_names: std::collections::HashSet<&str> =
                values.iter().map(|v| v.canonical_name.as_str()).collect();
            for val in &values {
                for alias in &val.aliases {
                    assert!(
                        !canonical_names.contains(alias.as_str()),
                        "Alias '{}' collides with a canonical name in {:?}",
                        alias,
                        kind
                    );
                }
            }
        }
    }

    #[test]
    fn no_duplicate_aliases_within_kind() {
        let reg = alpha_registry();
        for kind in TaxonomyKind::all() {
            let values = reg.values_for_kind(*kind);
            let mut seen = std::collections::HashSet::new();
            for val in &values {
                for alias in &val.aliases {
                    assert!(
                        seen.insert(alias.clone()),
                        "Duplicate alias '{}' in {:?}",
                        alias,
                        kind
                    );
                }
            }
        }
    }

    // -- Serialization tests --

    #[test]
    fn taxonomy_kind_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&TaxonomyKind::DecisionType).unwrap(),
            "\"decision_type\""
        );
        assert_eq!(
            serde_json::to_string(&TaxonomyKind::LifecycleTag).unwrap(),
            "\"lifecycle_tag\""
        );
        assert_eq!(
            serde_json::to_string(&TaxonomyKind::PreferenceGuard).unwrap(),
            "\"preference_guard\""
        );
    }

    #[test]
    fn taxonomy_kind_roundtrips() {
        for kind in TaxonomyKind::all() {
            let json = serde_json::to_string(kind).unwrap();
            let deserialized: TaxonomyKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, deserialized);
        }
    }

    #[test]
    fn change_policy_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&ChangePolicy::Immutable).unwrap(),
            "\"immutable\""
        );
        assert_eq!(
            serde_json::to_string(&ChangePolicy::Governed).unwrap(),
            "\"governed\""
        );
        assert_eq!(
            serde_json::to_string(&ChangePolicy::Extensible).unwrap(),
            "\"extensible\""
        );
    }

    #[test]
    fn controlled_value_roundtrips() {
        let val = ControlledValue {
            canonical_name: "test".into(),
            kind: TaxonomyKind::DecisionType,
            aliases: vec!["t".into()],
            deprecation: None,
            owner: "test-owner".into(),
            change_policy: ChangePolicy::Governed,
            description: "A test value".into(),
            introduced_in: TaxonomyVersion::alpha(),
        };
        let json = serde_json::to_string(&val).unwrap();
        let deserialized: ControlledValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, deserialized);
    }

    #[test]
    fn registry_serializes_and_deserializes() {
        let reg = alpha_registry();
        let json = serde_json::to_string(&reg).unwrap();
        let mut deserialized: TaxonomyRegistry = serde_json::from_str(&json).unwrap();
        // Indices are skipped on serialization; rebuild them
        deserialized.rebuild_indices();
        assert_eq!(deserialized.version, reg.version);
        assert_eq!(deserialized.values.len(), reg.values.len());
        // Verify lookup still works after deserialization + rebuild
        assert!(deserialized.is_registered(TaxonomyKind::DecisionType, "product"));
        assert!(deserialized.is_registered(TaxonomyKind::DecisionType, "arch"));
    }

    // -- is_registered convenience tests --

    #[test]
    fn is_registered_returns_true_for_canonical() {
        let reg = alpha_registry();
        assert!(reg.is_registered(TaxonomyKind::DecisionType, "product"));
    }

    #[test]
    fn is_registered_returns_true_for_alias() {
        let reg = alpha_registry();
        assert!(reg.is_registered(TaxonomyKind::DecisionType, "impl"));
    }

    #[test]
    fn is_registered_returns_false_for_unknown() {
        let reg = alpha_registry();
        assert!(!reg.is_registered(TaxonomyKind::DecisionType, "unknown"));
    }

    // -- values_for_kind and active_values_for_kind --

    #[test]
    fn values_for_kind_returns_correct_subset() {
        let reg = alpha_registry();
        let dt_values = reg.values_for_kind(TaxonomyKind::DecisionType);
        assert_eq!(dt_values.len(), 3); // product, architecture, implementation
        for v in &dt_values {
            assert_eq!(v.kind, TaxonomyKind::DecisionType);
        }
    }

    #[test]
    fn active_values_excludes_deprecated() {
        let v = TaxonomyVersion::alpha();
        let values = vec![
            ControlledValue {
                canonical_name: "active_val".into(),
                kind: TaxonomyKind::DecisionType,
                aliases: vec![],
                deprecation: None,
                owner: "test".into(),
                change_policy: ChangePolicy::Governed,
                description: "Active value".into(),
                introduced_in: v.clone(),
            },
            ControlledValue {
                canonical_name: "deprecated_val".into(),
                kind: TaxonomyKind::DecisionType,
                aliases: vec![],
                deprecation: Some(DeprecationInfo {
                    deprecated_since: v.clone(),
                    superseded_by: Some("active_val".into()),
                    removal_target: None,
                    reason: "Test".into(),
                }),
                owner: "test".into(),
                change_policy: ChangePolicy::Governed,
                description: "Deprecated value".into(),
                introduced_in: v.clone(),
            },
        ];
        let reg = TaxonomyRegistry::new(v, values);
        let all = reg.values_for_kind(TaxonomyKind::DecisionType);
        assert_eq!(all.len(), 2);
        let active = reg.active_values_for_kind(TaxonomyKind::DecisionType);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].canonical_name, "active_val");
    }

    // -- Authority boundary immutability policy --

    #[test]
    fn authority_boundary_values_are_immutable() {
        let reg = alpha_registry();
        for val in reg.values_for_kind(TaxonomyKind::AuthorityBoundary) {
            assert_eq!(
                val.change_policy,
                ChangePolicy::Immutable,
                "AuthorityBoundary value '{}' should be Immutable",
                val.canonical_name
            );
        }
    }

    // -- External target and source class are extensible --

    #[test]
    fn external_target_values_are_extensible() {
        let reg = alpha_registry();
        for val in reg.values_for_kind(TaxonomyKind::ExternalTarget) {
            assert_eq!(
                val.change_policy,
                ChangePolicy::Extensible,
                "ExternalTarget value '{}' should be Extensible",
                val.canonical_name
            );
        }
    }

    #[test]
    fn source_class_values_are_extensible() {
        let reg = alpha_registry();
        for val in reg.values_for_kind(TaxonomyKind::SourceClass) {
            assert_eq!(
                val.change_policy,
                ChangePolicy::Extensible,
                "SourceClass value '{}' should be Extensible",
                val.canonical_name
            );
        }
    }

    // -- Empty registry edge case --

    #[test]
    fn empty_registry_rejects_everything() {
        let reg = TaxonomyRegistry::new(TaxonomyVersion::alpha(), vec![]);
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(!reg.is_registered(TaxonomyKind::DecisionType, "product"));
    }

    // -- rebuild_indices after deserialization --

    #[test]
    fn deserialized_registry_needs_rebuild_for_lookup() {
        let reg = alpha_registry();
        let json = serde_json::to_string(&reg).unwrap();
        let raw: TaxonomyRegistry = serde_json::from_str(&json).unwrap();
        // Before rebuild, indices are empty — lookups fail
        assert!(!raw.is_registered(TaxonomyKind::DecisionType, "product"));
        // After rebuild, lookups succeed
        let rebuilt = TaxonomyRegistry::new(raw.version, raw.values);
        assert!(rebuilt.is_registered(TaxonomyKind::DecisionType, "product"));
    }
}
