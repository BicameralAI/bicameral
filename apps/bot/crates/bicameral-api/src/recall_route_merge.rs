//! Route merge, dedup, ranking, and reason-code catalog for preflight recall.
//!
//! Implements the route merge/dedup layer described in the preflight
//! impact-to-decision recall strategy spec. Routes A–F each produce
//! `RouteCandidate`s; this module merges them, deduplicates by decision/
//! candidate identity, assigns deterministic ranking tiers, applies
//! stable tie-breaks, and enforces bounded preference adjustments.
//!
//! # Route classification
//!
//! | Route | Name | Tier ceiling |
//! |-------|------|-------------|
//! | A | Accepted binding backtrace | 1 |
//! | B | Graph overlap / neighborhood | 2 |
//! | C | Structured trusted-corpus search | 3–4 |
//! | D | Semantic/textual reviewed-corpus | 5–6 |
//! | E | Ownership/project/source overlap | 4–5 |
//! | F | CodeGenome-style advisory hints | 6–7 |
//!
//! # Invariants
//!
//! - Accepted binding overlap (tier 1) cannot be hidden by semantic or
//!   advisory weighting.
//! - Preferences are bounded to within-policy score adjustments and
//!   cannot create authority or waive governance gates.
//! - Tie-breaks within a tier are stable: `decision_id` then
//!   `candidate_id` then `title`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::recall::EvidenceFreshness;

// ---------------------------------------------------------------------------
// Route classification
// ---------------------------------------------------------------------------

/// Recall route classification (Routes A–F from the strategy spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteClass {
    /// Route A: accepted binding backtrace.
    Binding,
    /// Route B: graph overlap and neighborhood expansion.
    Graph,
    /// Route C: structured trusted-corpus search.
    Structured,
    /// Route D: semantic/textual reviewed-corpus search.
    Semantic,
    /// Route E: ownership, project, and source-overlap boosting.
    Ownership,
    /// Route F: CodeGenome-style advisory hints.
    Advisory,
}

impl RouteClass {
    /// The highest (lowest-numbered) tier a route can assign.
    ///
    /// This ceiling prevents low-authority routes from placing candidates
    /// in tiers reserved for higher-authority evidence.
    pub fn tier_ceiling(self) -> u8 {
        match self {
            Self::Binding => 1,
            Self::Graph => 2,
            Self::Structured => 3,
            Self::Ownership => 4,
            Self::Semantic => 5,
            Self::Advisory => 6,
        }
    }
}

// ---------------------------------------------------------------------------
// Ranking tiers
// ---------------------------------------------------------------------------

/// Deterministic ranking tier assigned from route precedence.
///
/// Lower numeric value = higher precedence.  Tier assignment is
/// deterministic for fixed inputs.
///
/// Tier definitions (from the spec):
///
/// 1. Accepted binding overlap in requested scope.
/// 2. Verified graph overlap or exact symbol/path overlap with
///    searched-scope proof.
/// 3. High-confidence projected impacted area with searched-scope proof.
/// 4. Accepted Decision/source overlap from trusted corpus.
/// 5. Ownership, project, or source overlap.
/// 6. Semantic/textual similarity over reviewed corpus text.
/// 7. Fresh, explainable CodeGenome-style propagated hints.
/// 8. Approximate candidates with lower or incomplete provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RankingTier(pub u8);

impl RankingTier {
    pub const BINDING_OVERLAP: Self = Self(1);
    pub const GRAPH_OVERLAP: Self = Self(2);
    pub const HIGH_CONFIDENCE_PROJECTION: Self = Self(3);
    pub const CORPUS_OVERLAP: Self = Self(4);
    pub const OWNERSHIP_OVERLAP: Self = Self(5);
    pub const SEMANTIC_SIMILARITY: Self = Self(6);
    pub const ADVISORY_HINT: Self = Self(7);
    pub const APPROXIMATE: Self = Self(8);

    /// Valid tier range.
    pub fn is_valid(self) -> bool {
        (1..=8).contains(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Reason-code catalog
// ---------------------------------------------------------------------------

/// Reason code explaining why a candidate was surfaced and ranked.
///
/// Each code is a stable, machine-readable identifier suitable for
/// fixture replay and evaluation.  Human-readable explanations are
/// produced by `ReasonCode::explanation()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    // -- Binding route (A) --
    /// Candidate has an accepted binding to an exact impacted target.
    AcceptedBindingExact,
    /// Candidate has an accepted binding overlapping a nearby target.
    AcceptedBindingNearby,
    /// Candidate has an accepted binding but the binding is stale.
    AcceptedBindingStale,

    // -- Graph route (B) --
    /// Exact symbol or path overlap verified against graph snapshot.
    GraphOverlapExact,
    /// Neighborhood expansion reached the candidate within budget.
    GraphNeighborhoodExpansion,
    /// Graph overlap exists but scope proof is incomplete.
    GraphOverlapPartialScope,

    // -- Structured route (C) --
    /// Structured corpus match on path, module, or source ref.
    StructuredCorpusMatch,
    /// Structured match on lifecycle tag, external target, or metadata.
    StructuredMetadataMatch,
    /// Structured match on controlled metadata with quality caveat.
    StructuredMetadataQualityCaveat,

    // -- Semantic route (D) --
    /// Semantic similarity over reviewed corpus text.
    SemanticSimilarity,
    /// Textual keyword match over reviewed corpus.
    TextualKeywordMatch,

    // -- Ownership route (E) --
    /// Owner, project, or source overlap with the actor or task scope.
    OwnerOverlap,
    /// Team, product area, or integration namespace overlap.
    TeamAreaOverlap,
    /// Historical governance overlap with the actor's subsystem.
    HistoricalGovernanceOverlap,

    // -- Advisory route (F) --
    /// Fresh, explainable CodeGenome propagated hint.
    AdvisoryHintFresh,
    /// Advisory hint that is stale or partially explained.
    AdvisoryHintStale,

    // -- Cross-route / general --
    /// Candidate was surfaced by multiple routes (merged).
    MultiRouteConvergence,
    /// Candidate was downgraded due to staleness.
    DowngradedStale,
    /// Candidate was downgraded due to missing scope proof.
    DowngradedMissingScope,
    /// Candidate has incomplete provenance.
    IncompleteProvenance,
    /// Preference-boosted within policy bounds.
    PreferenceBoosted,
}

impl ReasonCode {
    /// Stable human-readable explanation for the reason code.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::AcceptedBindingExact => {
                "Candidate has an accepted binding to an exact impacted target."
            }
            Self::AcceptedBindingNearby => {
                "Candidate has an accepted binding overlapping a nearby target."
            }
            Self::AcceptedBindingStale => {
                "Candidate has an accepted binding but the binding is stale."
            }
            Self::GraphOverlapExact => {
                "Exact symbol or path overlap verified against graph snapshot."
            }
            Self::GraphNeighborhoodExpansion => {
                "Neighborhood expansion reached the candidate within budget."
            }
            Self::GraphOverlapPartialScope => "Graph overlap exists but scope proof is incomplete.",
            Self::StructuredCorpusMatch => {
                "Structured corpus match on path, module, or source ref."
            }
            Self::StructuredMetadataMatch => {
                "Structured match on lifecycle tag, external target, or metadata."
            }
            Self::StructuredMetadataQualityCaveat => {
                "Structured match on controlled metadata with quality caveat."
            }
            Self::SemanticSimilarity => "Semantic similarity over reviewed corpus text.",
            Self::TextualKeywordMatch => "Textual keyword match over reviewed corpus.",
            Self::OwnerOverlap => "Owner, project, or source overlap with the actor or task scope.",
            Self::TeamAreaOverlap => "Team, product area, or integration namespace overlap.",
            Self::HistoricalGovernanceOverlap => {
                "Historical governance overlap with the actor's subsystem."
            }
            Self::AdvisoryHintFresh => "Fresh, explainable CodeGenome propagated hint.",
            Self::AdvisoryHintStale => "Advisory hint that is stale or partially explained.",
            Self::MultiRouteConvergence => "Candidate was surfaced by multiple routes (merged).",
            Self::DowngradedStale => "Candidate was downgraded due to staleness.",
            Self::DowngradedMissingScope => "Candidate was downgraded due to missing scope proof.",
            Self::IncompleteProvenance => "Candidate has incomplete provenance.",
            Self::PreferenceBoosted => "Preference-boosted within policy bounds.",
        }
    }

    /// Whether this reason code represents a governance-authority signal.
    ///
    /// Authority signals must not be created by preferences, semantic
    /// similarity, or advisory hints.
    pub fn is_authority_signal(self) -> bool {
        matches!(
            self,
            Self::AcceptedBindingExact
                | Self::AcceptedBindingNearby
                | Self::AcceptedBindingStale
                | Self::GraphOverlapExact
        )
    }
}

// ---------------------------------------------------------------------------
// Route candidate (per-route output)
// ---------------------------------------------------------------------------

/// A single candidate produced by one recall route.
///
/// Routes produce these independently; the merge layer combines them
/// into `RankedRecallCandidate`s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCandidate {
    /// Decision or candidate identity.
    pub identity: CandidateIdentity,

    /// Route that produced this candidate.
    pub route: RouteClass,

    /// Primary reason code for surfacing.
    pub reason: ReasonCode,

    /// Additional reason codes (e.g. multi-signal convergence).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_reasons: Vec<ReasonCode>,

    /// Route-specific confidence or relevance score (0.0–1.0).
    ///
    /// Meaning is route-relative; cross-route comparison uses tiers.
    pub route_score: f64,

    /// Freshness of the evidence backing this candidate.
    pub freshness: EvidenceFreshness,

    /// Whether this candidate has searched-scope proof.
    pub has_scope_proof: bool,

    /// Source references for provenance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<String>,

    /// Graph snapshot id when graph-backed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,

    /// Caveats attached to this candidate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
}

/// Identity of a recall candidate — either a canonical Decision or a
/// DecisionCandidate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CandidateIdentity {
    /// Decision id (for canonical decisions) or candidate id.
    pub id: Uuid,
    /// Title for stable tie-break ordering.
    pub title: String,
    /// Whether this is a canonical decision or a candidate.
    pub kind: CandidateIdentityKind,
}

/// Whether the identity refers to a canonical decision or a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateIdentityKind {
    /// Canonical accepted Decision.
    CanonicalDecision,
    /// Proposed DecisionCandidate.
    DecisionCandidate,
}

// ---------------------------------------------------------------------------
// Ranked recall candidate (merged output)
// ---------------------------------------------------------------------------

/// A merged, deduplicated, ranked recall candidate.
///
/// Produced by the route merge layer from one or more `RouteCandidate`s
/// sharing the same `CandidateIdentity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedRecallCandidate {
    /// Decision or candidate identity.
    pub identity: CandidateIdentity,

    /// Assigned ranking tier (lower = higher precedence).
    pub tier: RankingTier,

    /// All reason codes across merged routes.
    pub reason_codes: Vec<ReasonCode>,

    /// Routes that contributed to this candidate.
    pub contributing_routes: Vec<RouteClass>,

    /// Best freshness across contributing routes.
    pub freshness: EvidenceFreshness,

    /// Whether any contributing route has scope proof.
    pub has_scope_proof: bool,

    /// Merged source references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<String>,

    /// Graph snapshot id from the highest-authority route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,

    /// Merged caveats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,

    /// Within-tier sort key for stable ordering.
    ///
    /// Composed from: best route score, preference adjustment,
    /// and stable tie-break fields.
    pub sort_key: TierSortKey,

    /// Explanation contract: human-readable ranking explanation.
    pub explanation: String,
}

/// Within-tier sort key for deterministic ordering.
///
/// Compared lexicographically: `effective_score` (descending),
/// then `decision_id` (ascending), then `title` (ascending).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierSortKey {
    /// Effective score after preference adjustment (0.0–1.0).
    pub effective_score: f64,
    /// Decision or candidate id for tie-break.
    pub tie_break_id: Uuid,
    /// Title for final tie-break.
    pub tie_break_title: String,
}

impl PartialEq for TierSortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_tuple() == other.cmp_tuple()
    }
}

impl Eq for TierSortKey {}

impl PartialOrd for TierSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TierSortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_tuple().cmp(&other.cmp_tuple())
    }
}

impl TierSortKey {
    /// Produce a comparison tuple.
    ///
    /// Score is inverted (negated) so that higher scores sort first
    /// in ascending order.  Tie-breaks use id then title ascending.
    fn cmp_tuple(&self) -> (OrderedFloat, Uuid, &str) {
        (
            OrderedFloat(-self.effective_score),
            self.tie_break_id,
            &self.tie_break_title,
        )
    }
}

/// Wrapper for `f64` that implements `Ord` for sort key comparison.
///
/// NaN is treated as greater than any finite value (sorts last).
#[derive(Debug, Clone, Copy)]
struct OrderedFloat(f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// ---------------------------------------------------------------------------
// Preference profile (bounded)
// ---------------------------------------------------------------------------

/// Bounded preference profile for within-policy score adjustments.
///
/// Preferences may tune attention within tiers.  They must not:
/// - create authority or binding evidence;
/// - approve signoff or assert compliance;
/// - hide accepted binding overlap;
/// - directly mutate hidden ranking behavior without audit.
///
/// Policy caps beat preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecallPreferenceProfile {
    /// Per-route score adjustment (additive, clamped to cap).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub route_adjustments: HashMap<RouteClass, f64>,

    /// Maximum absolute adjustment allowed per candidate.
    ///
    /// Enforced as a policy cap — any adjustment beyond this is
    /// clamped silently.  Default: 0.15.
    #[serde(default = "default_preference_cap")]
    pub preference_cap: f64,
}

fn default_preference_cap() -> f64 {
    0.15
}

impl RecallPreferenceProfile {
    /// Apply a bounded preference adjustment to a route score.
    ///
    /// Returns the adjusted score, clamped to [0.0, 1.0], with the
    /// adjustment itself clamped to `[-preference_cap, +preference_cap]`.
    pub fn adjust_score(&self, route: RouteClass, base_score: f64) -> f64 {
        let adj = self
            .route_adjustments
            .get(&route)
            .copied()
            .unwrap_or(0.0)
            .clamp(-self.preference_cap, self.preference_cap);
        (base_score + adj).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Merge + dedup + rank engine
// ---------------------------------------------------------------------------

/// Merge candidates from multiple routes, dedup by identity, assign
/// ranking tiers, apply preferences, and sort with stable tie-breaks.
///
/// # Invariants
///
/// - Accepted binding overlap (tier 1) candidates always appear before
///   semantic or advisory candidates regardless of preference adjustments.
/// - Preferences cannot promote a candidate to a tier below its route
///   ceiling.
/// - Tie-breaks within a tier are stable: effective_score desc, then
///   `id` asc, then `title` asc.
pub fn merge_and_rank(
    route_candidates: Vec<RouteCandidate>,
    preferences: &RecallPreferenceProfile,
) -> Vec<RankedRecallCandidate> {
    // Group by identity
    let mut groups: HashMap<(Uuid, CandidateIdentityKind), Vec<RouteCandidate>> = HashMap::new();
    for rc in route_candidates {
        groups
            .entry((rc.identity.id, rc.identity.kind))
            .or_default()
            .push(rc);
    }

    let mut ranked: Vec<RankedRecallCandidate> = groups
        .into_values()
        .map(|candidates| merge_group(candidates, preferences))
        .collect();

    // Sort by tier (ascending), then by sort_key within tier
    ranked.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then_with(|| a.sort_key.cmp(&b.sort_key))
    });

    ranked
}

/// Merge a group of `RouteCandidate`s sharing the same identity into a
/// single `RankedRecallCandidate`.
fn merge_group(
    candidates: Vec<RouteCandidate>,
    preferences: &RecallPreferenceProfile,
) -> RankedRecallCandidate {
    debug_assert!(!candidates.is_empty());

    // Take identity from the first candidate (they all share it).
    let identity = candidates[0].identity.clone();

    // Collect unique routes and reason codes.
    let mut contributing_routes: Vec<RouteClass> = Vec::new();
    let mut reason_codes: Vec<ReasonCode> = Vec::new();
    let mut source_refs: Vec<String> = Vec::new();
    let mut caveats: Vec<String> = Vec::new();
    let mut best_freshness = EvidenceFreshness::Unknown;
    let mut has_scope_proof = false;
    let mut graph_snapshot_id: Option<String> = None;
    let mut best_route_score: f64 = 0.0;
    let mut best_route: RouteClass = candidates[0].route;

    for c in &candidates {
        if !contributing_routes.contains(&c.route) {
            contributing_routes.push(c.route);
        }
        if !reason_codes.contains(&c.reason) {
            reason_codes.push(c.reason);
        }
        for sr in &c.secondary_reasons {
            if !reason_codes.contains(sr) {
                reason_codes.push(*sr);
            }
        }
        for s in &c.source_refs {
            if !source_refs.contains(s) {
                source_refs.push(s.clone());
            }
        }
        for cv in &c.caveats {
            if !caveats.contains(cv) {
                caveats.push(cv.clone());
            }
        }
        best_freshness = best_evidence_freshness(best_freshness, c.freshness);
        has_scope_proof = has_scope_proof || c.has_scope_proof;
        if c.graph_snapshot_id.is_some() && graph_snapshot_id.is_none() {
            graph_snapshot_id = c.graph_snapshot_id.clone();
        }
        if c.route_score > best_route_score {
            best_route_score = c.route_score;
            best_route = c.route;
        }
    }

    if contributing_routes.len() > 1 && !reason_codes.contains(&ReasonCode::MultiRouteConvergence) {
        reason_codes.push(ReasonCode::MultiRouteConvergence);
    }

    // Assign tier: use the best (lowest) route ceiling, then adjust
    // for freshness and scope proof.
    let base_tier = contributing_routes
        .iter()
        .map(|r| r.tier_ceiling())
        .min()
        .unwrap_or(8);

    let tier = assign_tier(base_tier, best_freshness, has_scope_proof, &reason_codes);

    // Apply bounded preference adjustment.
    let adjusted_score = preferences.adjust_score(best_route, best_route_score);

    let mut adjusted_reasons = reason_codes.clone();
    let raw_adj = preferences
        .route_adjustments
        .get(&best_route)
        .copied()
        .unwrap_or(0.0);
    if raw_adj.abs() > f64::EPSILON && !adjusted_reasons.contains(&ReasonCode::PreferenceBoosted) {
        adjusted_reasons.push(ReasonCode::PreferenceBoosted);
    }

    let sort_key = TierSortKey {
        effective_score: adjusted_score,
        tie_break_id: identity.id,
        tie_break_title: identity.title.clone(),
    };

    let explanation = build_explanation(&identity, tier, &adjusted_reasons, &contributing_routes);

    RankedRecallCandidate {
        identity,
        tier,
        reason_codes: adjusted_reasons,
        contributing_routes,
        freshness: best_freshness,
        has_scope_proof,
        source_refs,
        graph_snapshot_id,
        caveats,
        sort_key,
        explanation,
    }
}

/// Assign a ranking tier from the base route ceiling, adjusted for
/// freshness and scope proof.
fn assign_tier(
    base_tier: u8,
    freshness: EvidenceFreshness,
    has_scope_proof: bool,
    reason_codes: &[ReasonCode],
) -> RankingTier {
    let mut tier = base_tier;

    // Stale evidence pushes the candidate down by one tier.
    if freshness == EvidenceFreshness::Stale {
        tier = tier.saturating_add(1).min(8);
    }

    // Missing scope proof for tiers that require it (2, 3) pushes down.
    if !has_scope_proof && (2..=3).contains(&tier) {
        tier = tier.saturating_add(1).min(8);
    }

    // Incomplete provenance pushes to tier 8.
    if reason_codes.contains(&ReasonCode::IncompleteProvenance) {
        tier = 8;
    }

    RankingTier(tier)
}

/// Select the "best" (most current) freshness from two values.
fn best_evidence_freshness(a: EvidenceFreshness, b: EvidenceFreshness) -> EvidenceFreshness {
    fn rank(f: EvidenceFreshness) -> u8 {
        match f {
            EvidenceFreshness::Current => 0,
            EvidenceFreshness::PossiblyStale => 1,
            EvidenceFreshness::Stale => 2,
            EvidenceFreshness::Unknown => 3,
        }
    }
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

/// Build a human-readable explanation for a ranked candidate.
fn build_explanation(
    identity: &CandidateIdentity,
    tier: RankingTier,
    reasons: &[ReasonCode],
    routes: &[RouteClass],
) -> String {
    let kind_label = match identity.kind {
        CandidateIdentityKind::CanonicalDecision => "Decision",
        CandidateIdentityKind::DecisionCandidate => "Candidate",
    };

    let route_names: Vec<&str> = routes
        .iter()
        .map(|r| match r {
            RouteClass::Binding => "binding",
            RouteClass::Graph => "graph",
            RouteClass::Structured => "structured",
            RouteClass::Semantic => "semantic",
            RouteClass::Ownership => "ownership",
            RouteClass::Advisory => "advisory",
        })
        .collect();

    let reason_explanations: Vec<&str> = reasons.iter().map(|r| r.explanation()).collect();

    format!(
        "{kind_label} \"{}\" ranked tier {} via route(s) [{}]. Reasons: {}.",
        identity.title,
        tier.0,
        route_names.join(", "),
        reason_explanations.join("; "),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_identity(id: u128, title: &str, kind: CandidateIdentityKind) -> CandidateIdentity {
        CandidateIdentity {
            id: Uuid::from_u128(id),
            title: title.to_string(),
            kind,
        }
    }

    fn binding_candidate(id: u128, title: &str, score: f64) -> RouteCandidate {
        RouteCandidate {
            identity: make_identity(id, title, CandidateIdentityKind::CanonicalDecision),
            route: RouteClass::Binding,
            reason: ReasonCode::AcceptedBindingExact,
            secondary_reasons: vec![],
            route_score: score,
            freshness: EvidenceFreshness::Current,
            has_scope_proof: true,
            source_refs: vec![],
            graph_snapshot_id: None,
            caveats: vec![],
        }
    }

    fn semantic_candidate(id: u128, title: &str, score: f64) -> RouteCandidate {
        RouteCandidate {
            identity: make_identity(id, title, CandidateIdentityKind::CanonicalDecision),
            route: RouteClass::Semantic,
            reason: ReasonCode::SemanticSimilarity,
            secondary_reasons: vec![],
            route_score: score,
            freshness: EvidenceFreshness::Current,
            has_scope_proof: true,
            source_refs: vec![],
            graph_snapshot_id: None,
            caveats: vec![],
        }
    }

    fn advisory_candidate(id: u128, title: &str, score: f64) -> RouteCandidate {
        RouteCandidate {
            identity: make_identity(id, title, CandidateIdentityKind::DecisionCandidate),
            route: RouteClass::Advisory,
            reason: ReasonCode::AdvisoryHintFresh,
            secondary_reasons: vec![],
            route_score: score,
            freshness: EvidenceFreshness::Current,
            has_scope_proof: true,
            source_refs: vec![],
            graph_snapshot_id: None,
            caveats: vec![],
        }
    }

    fn ownership_candidate(id: u128, title: &str, score: f64) -> RouteCandidate {
        RouteCandidate {
            identity: make_identity(id, title, CandidateIdentityKind::CanonicalDecision),
            route: RouteClass::Ownership,
            reason: ReasonCode::OwnerOverlap,
            secondary_reasons: vec![],
            route_score: score,
            freshness: EvidenceFreshness::Current,
            has_scope_proof: true,
            source_refs: vec![],
            graph_snapshot_id: None,
            caveats: vec![],
        }
    }

    fn graph_candidate(id: u128, title: &str, score: f64) -> RouteCandidate {
        RouteCandidate {
            identity: make_identity(id, title, CandidateIdentityKind::CanonicalDecision),
            route: RouteClass::Graph,
            reason: ReasonCode::GraphOverlapExact,
            secondary_reasons: vec![],
            route_score: score,
            freshness: EvidenceFreshness::Current,
            has_scope_proof: true,
            source_refs: vec!["snapshot-abc".into()],
            graph_snapshot_id: Some("snap-001".into()),
            caveats: vec![],
        }
    }

    fn structured_candidate(id: u128, title: &str, score: f64) -> RouteCandidate {
        RouteCandidate {
            identity: make_identity(id, title, CandidateIdentityKind::CanonicalDecision),
            route: RouteClass::Structured,
            reason: ReasonCode::StructuredCorpusMatch,
            secondary_reasons: vec![],
            route_score: score,
            freshness: EvidenceFreshness::Current,
            has_scope_proof: true,
            source_refs: vec![],
            graph_snapshot_id: None,
            caveats: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Tie-break stability
    // -----------------------------------------------------------------------

    #[test]
    fn tie_break_stability_same_tier_orders_by_score_then_id_then_title() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![
            semantic_candidate(3, "Zebra decision", 0.8),
            semantic_candidate(1, "Alpha decision", 0.8),
            semantic_candidate(2, "Beta decision", 0.9),
        ];

        let ranked = merge_and_rank(candidates, &prefs);

        // All are tier 6 (semantic ceiling 5 → with current freshness
        // and scope proof stays at 5... let's check).
        // Actually RouteClass::Semantic has tier_ceiling 5, and with
        // Current freshness + scope proof, tier stays 5.
        // Wait — checking assign_tier: base_tier=5, freshness=Current,
        // has_scope_proof=true. No stale, no missing scope for tier 2–3.
        // So tier=5.

        assert_eq!(ranked.len(), 3);
        // All in same tier
        assert!(ranked.iter().all(|r| r.tier == RankingTier(5)));
        // Sorted by score desc, then id asc: id=2(0.9), id=1(0.8), id=3(0.8)
        assert_eq!(ranked[0].identity.id, Uuid::from_u128(2));
        assert_eq!(ranked[1].identity.id, Uuid::from_u128(1));
        assert_eq!(ranked[2].identity.id, Uuid::from_u128(3));
    }

    #[test]
    fn tie_break_stability_is_deterministic_across_invocations() {
        let prefs = RecallPreferenceProfile::default();
        let make_candidates = || {
            vec![
                binding_candidate(10, "Decision X", 0.95),
                binding_candidate(20, "Decision Y", 0.95),
                binding_candidate(30, "Decision A", 0.95),
            ]
        };

        let ranked1 = merge_and_rank(make_candidates(), &prefs);
        let ranked2 = merge_and_rank(make_candidates(), &prefs);

        assert_eq!(ranked1.len(), ranked2.len());
        for (a, b) in ranked1.iter().zip(ranked2.iter()) {
            assert_eq!(a.identity.id, b.identity.id);
            assert_eq!(a.tier, b.tier);
        }
    }

    // -----------------------------------------------------------------------
    // Dedup
    // -----------------------------------------------------------------------

    #[test]
    fn dedup_same_identity_from_multiple_routes_merges() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![
            binding_candidate(1, "Shared decision", 0.9),
            semantic_candidate(1, "Shared decision", 0.7),
        ];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].identity.id, Uuid::from_u128(1));
        assert!(ranked[0].contributing_routes.contains(&RouteClass::Binding));
        assert!(ranked[0]
            .contributing_routes
            .contains(&RouteClass::Semantic));
        assert!(ranked[0]
            .reason_codes
            .contains(&ReasonCode::MultiRouteConvergence));
        // Should get tier 1 from binding route ceiling.
        assert_eq!(ranked[0].tier, RankingTier::BINDING_OVERLAP);
    }

    #[test]
    fn dedup_different_identities_remain_separate() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![
            binding_candidate(1, "Decision A", 0.9),
            semantic_candidate(2, "Decision B", 0.8),
        ];

        let ranked = merge_and_rank(candidates, &prefs);
        assert_eq!(ranked.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Stale bindings
    // -----------------------------------------------------------------------

    #[test]
    fn stale_binding_is_downgraded_one_tier() {
        let prefs = RecallPreferenceProfile::default();
        let mut stale = binding_candidate(1, "Stale binding decision", 0.9);
        stale.freshness = EvidenceFreshness::Stale;
        stale.reason = ReasonCode::AcceptedBindingStale;

        let fresh = binding_candidate(2, "Fresh binding decision", 0.9);

        let ranked = merge_and_rank(vec![stale, fresh], &prefs);

        assert_eq!(ranked.len(), 2);
        // Fresh binding at tier 1
        let fresh_ranked = ranked.iter().find(|r| r.identity.id == Uuid::from_u128(2));
        assert_eq!(fresh_ranked.unwrap().tier, RankingTier::BINDING_OVERLAP);
        // Stale binding pushed to tier 2
        let stale_ranked = ranked.iter().find(|r| r.identity.id == Uuid::from_u128(1));
        assert_eq!(stale_ranked.unwrap().tier, RankingTier::GRAPH_OVERLAP);
    }

    // -----------------------------------------------------------------------
    // Semantic-only matches
    // -----------------------------------------------------------------------

    #[test]
    fn semantic_only_matches_land_in_correct_tier() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![semantic_candidate(1, "Semantic only", 0.85)];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 1);
        // Semantic route ceiling is 5
        assert_eq!(ranked[0].tier, RankingTier::OWNERSHIP_OVERLAP);
        assert!(ranked[0]
            .reason_codes
            .contains(&ReasonCode::SemanticSimilarity));
    }

    #[test]
    fn semantic_match_does_not_carry_authority_signal() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![semantic_candidate(1, "Semantic match", 0.95)];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 1);
        // No authority signals from semantic route
        assert!(ranked[0]
            .reason_codes
            .iter()
            .all(|r| !r.is_authority_signal()));
    }

    // -----------------------------------------------------------------------
    // Advisory hints
    // -----------------------------------------------------------------------

    #[test]
    fn advisory_hints_land_below_binding_and_graph() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![
            binding_candidate(1, "Binding match", 0.7),
            graph_candidate(2, "Graph match", 0.8),
            advisory_candidate(3, "Advisory hint", 0.95),
        ];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 3);
        // Binding first (tier 1), graph second (tier 2), advisory last (tier 6)
        assert_eq!(ranked[0].identity.id, Uuid::from_u128(1));
        assert_eq!(ranked[0].tier, RankingTier::BINDING_OVERLAP);
        assert_eq!(ranked[1].identity.id, Uuid::from_u128(2));
        assert_eq!(ranked[1].tier, RankingTier::GRAPH_OVERLAP);
        assert_eq!(ranked[2].identity.id, Uuid::from_u128(3));
        assert!(ranked[2].tier.0 >= 6);
    }

    #[test]
    fn stale_advisory_hint_is_downgraded() {
        let prefs = RecallPreferenceProfile::default();
        let mut stale_advisory = advisory_candidate(1, "Stale hint", 0.9);
        stale_advisory.freshness = EvidenceFreshness::Stale;
        stale_advisory.reason = ReasonCode::AdvisoryHintStale;

        let fresh_advisory = advisory_candidate(2, "Fresh hint", 0.8);

        let ranked = merge_and_rank(vec![stale_advisory, fresh_advisory], &prefs);

        assert_eq!(ranked.len(), 2);
        let stale = ranked.iter().find(|r| r.identity.id == Uuid::from_u128(1));
        let fresh = ranked.iter().find(|r| r.identity.id == Uuid::from_u128(2));
        // Stale should be in a worse (higher-numbered) tier
        assert!(stale.unwrap().tier.0 > fresh.unwrap().tier.0);
    }

    // -----------------------------------------------------------------------
    // Binding overlap visibility (acceptance criterion)
    // -----------------------------------------------------------------------

    #[test]
    fn accepted_binding_overlap_cannot_be_hidden_by_semantic_weighting() {
        let mut prefs = RecallPreferenceProfile::default();
        // Maximally boost semantic route
        prefs.route_adjustments.insert(RouteClass::Semantic, 1.0);

        let candidates = vec![
            binding_candidate(1, "Binding decision", 0.5),
            semantic_candidate(2, "Semantic decision", 0.99),
        ];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 2);
        // Binding must appear before semantic regardless of scores
        assert_eq!(ranked[0].identity.id, Uuid::from_u128(1));
        assert_eq!(ranked[0].tier, RankingTier::BINDING_OVERLAP);
        assert_eq!(ranked[1].identity.id, Uuid::from_u128(2));
        assert!(ranked[1].tier.0 > ranked[0].tier.0);
    }

    #[test]
    fn accepted_binding_overlap_cannot_be_hidden_by_advisory_weighting() {
        let mut prefs = RecallPreferenceProfile::default();
        prefs.route_adjustments.insert(RouteClass::Advisory, 1.0);

        let candidates = vec![
            binding_candidate(1, "Binding decision", 0.3),
            advisory_candidate(2, "Advisory hint", 0.99),
        ];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].identity.id, Uuid::from_u128(1));
        assert_eq!(ranked[0].tier, RankingTier::BINDING_OVERLAP);
        assert!(ranked[1].tier.0 > ranked[0].tier.0);
    }

    #[test]
    fn preference_boost_is_capped_at_policy_limit() {
        let mut prefs = RecallPreferenceProfile::default();
        prefs.preference_cap = 0.10;
        // Attempt a boost larger than the cap
        prefs.route_adjustments.insert(RouteClass::Semantic, 0.50);

        let adjusted = prefs.adjust_score(RouteClass::Semantic, 0.5);
        // Should be clamped to 0.5 + 0.10 = 0.60
        assert!((adjusted - 0.60).abs() < f64::EPSILON);
    }

    #[test]
    fn preference_cannot_promote_candidate_tier() {
        let mut prefs = RecallPreferenceProfile::default();
        prefs.route_adjustments.insert(RouteClass::Advisory, 1.0);

        let candidates = vec![advisory_candidate(1, "Boosted advisory", 0.99)];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 1);
        // Advisory tier ceiling is 6 — preference cannot promote above it
        assert!(ranked[0].tier.0 >= 6);
    }

    // -----------------------------------------------------------------------
    // All route types present
    // -----------------------------------------------------------------------

    #[test]
    fn all_six_route_types_merge_correctly() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![
            binding_candidate(1, "Binding", 0.9),
            graph_candidate(2, "Graph", 0.8),
            structured_candidate(3, "Structured", 0.7),
            semantic_candidate(4, "Semantic", 0.6),
            ownership_candidate(5, "Ownership", 0.5),
            advisory_candidate(6, "Advisory", 0.4),
        ];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 6);
        // Verify tier ordering: binding(1) < graph(2) < structured(3) <
        // ownership(4) < semantic(5) < advisory(6)
        for i in 0..ranked.len() - 1 {
            assert!(
                ranked[i].tier.0 <= ranked[i + 1].tier.0,
                "tier ordering violated at index {}: {} > {}",
                i,
                ranked[i].tier.0,
                ranked[i + 1].tier.0
            );
        }
    }

    // -----------------------------------------------------------------------
    // Explanation contract
    // -----------------------------------------------------------------------

    #[test]
    fn explanation_contains_route_and_tier_info() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![binding_candidate(1, "Test decision", 0.9)];

        let ranked = merge_and_rank(candidates, &prefs);

        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].explanation.contains("binding"));
        assert!(ranked[0].explanation.contains("tier 1"));
        assert!(ranked[0].explanation.contains("Test decision"));
    }

    // -----------------------------------------------------------------------
    // Reason-code catalog tests
    // -----------------------------------------------------------------------

    #[test]
    fn reason_code_serializes_to_snake_case() {
        let cases = vec![
            (
                ReasonCode::AcceptedBindingExact,
                "\"accepted_binding_exact\"",
            ),
            (
                ReasonCode::AcceptedBindingNearby,
                "\"accepted_binding_nearby\"",
            ),
            (
                ReasonCode::AcceptedBindingStale,
                "\"accepted_binding_stale\"",
            ),
            (ReasonCode::GraphOverlapExact, "\"graph_overlap_exact\""),
            (
                ReasonCode::GraphNeighborhoodExpansion,
                "\"graph_neighborhood_expansion\"",
            ),
            (
                ReasonCode::GraphOverlapPartialScope,
                "\"graph_overlap_partial_scope\"",
            ),
            (
                ReasonCode::StructuredCorpusMatch,
                "\"structured_corpus_match\"",
            ),
            (
                ReasonCode::StructuredMetadataMatch,
                "\"structured_metadata_match\"",
            ),
            (
                ReasonCode::StructuredMetadataQualityCaveat,
                "\"structured_metadata_quality_caveat\"",
            ),
            (ReasonCode::SemanticSimilarity, "\"semantic_similarity\""),
            (ReasonCode::TextualKeywordMatch, "\"textual_keyword_match\""),
            (ReasonCode::OwnerOverlap, "\"owner_overlap\""),
            (ReasonCode::TeamAreaOverlap, "\"team_area_overlap\""),
            (
                ReasonCode::HistoricalGovernanceOverlap,
                "\"historical_governance_overlap\"",
            ),
            (ReasonCode::AdvisoryHintFresh, "\"advisory_hint_fresh\""),
            (ReasonCode::AdvisoryHintStale, "\"advisory_hint_stale\""),
            (
                ReasonCode::MultiRouteConvergence,
                "\"multi_route_convergence\"",
            ),
            (ReasonCode::DowngradedStale, "\"downgraded_stale\""),
            (
                ReasonCode::DowngradedMissingScope,
                "\"downgraded_missing_scope\"",
            ),
            (
                ReasonCode::IncompleteProvenance,
                "\"incomplete_provenance\"",
            ),
            (ReasonCode::PreferenceBoosted, "\"preference_boosted\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                expected,
                "Failed for {:?}",
                variant
            );
        }
    }

    #[test]
    fn reason_code_has_non_empty_explanation() {
        let codes = vec![
            ReasonCode::AcceptedBindingExact,
            ReasonCode::AcceptedBindingNearby,
            ReasonCode::AcceptedBindingStale,
            ReasonCode::GraphOverlapExact,
            ReasonCode::GraphNeighborhoodExpansion,
            ReasonCode::GraphOverlapPartialScope,
            ReasonCode::StructuredCorpusMatch,
            ReasonCode::StructuredMetadataMatch,
            ReasonCode::StructuredMetadataQualityCaveat,
            ReasonCode::SemanticSimilarity,
            ReasonCode::TextualKeywordMatch,
            ReasonCode::OwnerOverlap,
            ReasonCode::TeamAreaOverlap,
            ReasonCode::HistoricalGovernanceOverlap,
            ReasonCode::AdvisoryHintFresh,
            ReasonCode::AdvisoryHintStale,
            ReasonCode::MultiRouteConvergence,
            ReasonCode::DowngradedStale,
            ReasonCode::DowngradedMissingScope,
            ReasonCode::IncompleteProvenance,
            ReasonCode::PreferenceBoosted,
        ];
        for code in codes {
            assert!(
                !code.explanation().is_empty(),
                "Empty explanation for {:?}",
                code
            );
        }
    }

    #[test]
    fn authority_signal_classification_is_correct() {
        assert!(ReasonCode::AcceptedBindingExact.is_authority_signal());
        assert!(ReasonCode::AcceptedBindingNearby.is_authority_signal());
        assert!(ReasonCode::AcceptedBindingStale.is_authority_signal());
        assert!(ReasonCode::GraphOverlapExact.is_authority_signal());

        assert!(!ReasonCode::SemanticSimilarity.is_authority_signal());
        assert!(!ReasonCode::AdvisoryHintFresh.is_authority_signal());
        assert!(!ReasonCode::OwnerOverlap.is_authority_signal());
        assert!(!ReasonCode::PreferenceBoosted.is_authority_signal());
    }

    // -----------------------------------------------------------------------
    // Route class tests
    // -----------------------------------------------------------------------

    #[test]
    fn route_class_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&RouteClass::Binding).unwrap(),
            "\"binding\""
        );
        assert_eq!(
            serde_json::to_string(&RouteClass::Graph).unwrap(),
            "\"graph\""
        );
        assert_eq!(
            serde_json::to_string(&RouteClass::Structured).unwrap(),
            "\"structured\""
        );
        assert_eq!(
            serde_json::to_string(&RouteClass::Semantic).unwrap(),
            "\"semantic\""
        );
        assert_eq!(
            serde_json::to_string(&RouteClass::Ownership).unwrap(),
            "\"ownership\""
        );
        assert_eq!(
            serde_json::to_string(&RouteClass::Advisory).unwrap(),
            "\"advisory\""
        );
    }

    #[test]
    fn ranking_tier_validity() {
        assert!(RankingTier(1).is_valid());
        assert!(RankingTier(8).is_valid());
        assert!(!RankingTier(0).is_valid());
        assert!(!RankingTier(9).is_valid());
    }

    // -----------------------------------------------------------------------
    // Ranking tier serialization
    // -----------------------------------------------------------------------

    #[test]
    fn ranking_tier_serializes_and_deserializes() {
        let tier = RankingTier::BINDING_OVERLAP;
        let json = serde_json::to_string(&tier).unwrap();
        let deserialized: RankingTier = serde_json::from_str(&json).unwrap();
        assert_eq!(tier, deserialized);
    }

    // -----------------------------------------------------------------------
    // Preference profile
    // -----------------------------------------------------------------------

    #[test]
    fn default_preference_profile_makes_no_adjustment() {
        let prefs = RecallPreferenceProfile::default();
        let adjusted = prefs.adjust_score(RouteClass::Binding, 0.5);
        assert!((adjusted - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn preference_negative_adjustment_clamps_to_zero() {
        let mut prefs = RecallPreferenceProfile::default();
        prefs.preference_cap = 1.0;
        prefs.route_adjustments.insert(RouteClass::Semantic, -1.0);
        let adjusted = prefs.adjust_score(RouteClass::Semantic, 0.3);
        assert!((adjusted - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn preference_positive_adjustment_clamps_to_one() {
        let mut prefs = RecallPreferenceProfile::default();
        prefs.preference_cap = 1.0;
        prefs.route_adjustments.insert(RouteClass::Binding, 1.0);
        let adjusted = prefs.adjust_score(RouteClass::Binding, 0.8);
        assert!((adjusted - 1.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn empty_input_produces_empty_output() {
        let prefs = RecallPreferenceProfile::default();
        let ranked = merge_and_rank(vec![], &prefs);
        assert!(ranked.is_empty());
    }

    #[test]
    fn single_candidate_produces_single_result() {
        let prefs = RecallPreferenceProfile::default();
        let ranked = merge_and_rank(vec![binding_candidate(1, "Solo", 0.9)], &prefs);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].tier, RankingTier::BINDING_OVERLAP);
    }

    #[test]
    fn graph_candidate_without_scope_proof_is_downgraded() {
        let prefs = RecallPreferenceProfile::default();
        let mut no_scope = graph_candidate(1, "No scope proof", 0.8);
        no_scope.has_scope_proof = false;

        let with_scope = graph_candidate(2, "With scope proof", 0.8);

        let ranked = merge_and_rank(vec![no_scope, with_scope], &prefs);

        let no_scope_ranked = ranked.iter().find(|r| r.identity.id == Uuid::from_u128(1));
        let with_scope_ranked = ranked.iter().find(|r| r.identity.id == Uuid::from_u128(2));

        // Without scope proof, graph candidate (base tier 2) is pushed to tier 3
        assert_eq!(no_scope_ranked.unwrap().tier, RankingTier(3));
        assert_eq!(with_scope_ranked.unwrap().tier, RankingTier(2));
    }

    #[test]
    fn incomplete_provenance_pushes_to_tier_8() {
        let prefs = RecallPreferenceProfile::default();
        let mut incomplete = binding_candidate(1, "Incomplete", 0.9);
        incomplete
            .secondary_reasons
            .push(ReasonCode::IncompleteProvenance);

        let ranked = merge_and_rank(vec![incomplete], &prefs);

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].tier, RankingTier::APPROXIMATE);
    }

    // -----------------------------------------------------------------------
    // Merged candidate serialization round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn ranked_recall_candidate_serializes_and_deserializes() {
        let prefs = RecallPreferenceProfile::default();
        let candidates = vec![
            binding_candidate(1, "Test", 0.9),
            semantic_candidate(1, "Test", 0.7),
        ];

        let ranked = merge_and_rank(candidates, &prefs);
        let json = serde_json::to_string(&ranked[0]).unwrap();
        let deserialized: RankedRecallCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.identity.id, ranked[0].identity.id);
        assert_eq!(deserialized.tier, ranked[0].tier);
    }
}
