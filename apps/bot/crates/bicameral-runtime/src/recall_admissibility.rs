//! Bot-owned recall admissibility gate.
//!
//! The gate is a deterministic filter between provider output and any later
//! RecallPacket, GraphSlice, CodeLocator, or fusion layer. It can admit,
//! downgrade, reject, or defer provider signals, but it never creates authority
//! facts. Semantic, ownership, CodeGenome-style, stale, unscoped, or approximate
//! signals remain recall hints until later deterministic verification proves a
//! concrete target through the governed runtime paths.

use bicameral_api::evidence::CodeSourceCurrentness;
use bicameral_api::recall::{
    RecallProviderSignal, RecallProviderSignalKind, RecallProviderStatus, RecallSearchedScope,
    RecallSignalResult, SemanticCodeCandidate, SemanticCodeSearchResult, SemanticCodeTargetType,
};
use serde::{Deserialize, Serialize};

/// Deterministic recall-admissibility outcome for a provider item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallAdmissibilityOutcome {
    /// The item may influence allowed recall-only uses.
    Admit,
    /// The item may influence recall-only uses, but with lower weight/caveats.
    Downgrade,
    /// The item must not influence recall or locator planning.
    Reject,
    /// The item cannot be evaluated until a later index/graph/provider state.
    Defer,
}

/// Stable reason codes emitted by the admissibility gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallAdmissibilityReasonCode {
    ProviderReady,
    ProviderStale,
    ProviderIndexing,
    ProviderUnavailable,
    ProviderUnsupported,
    MissingProviderIdentity,
    MissingProviderVersion,
    MissingIndexVersion,
    MissingSourceRef,
    MissingSnapshotRef,
    MissingSearchedScopeProof,
    UnknownScopePresent,
    Current,
    StaleCurrentness,
    UnknownCurrentness,
    MissingTargetIdentity,
    ApproximateTargetIdentity,
    SemanticSignalRankOnly,
    OwnershipSignalRankOnly,
    CodegenomeSignalRankOnly,
    AdvisorySignalRankOnly,
}

/// Recall-only uses that an admitted or downgraded provider item may influence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallAdmissibleUse {
    /// Candidate ordering within later deterministic fusion.
    RankOnly,
    /// Human-readable explanation of why something was surfaced.
    ExplainOnly,
    /// Non-authoritative slice/search hint before deterministic verification.
    SliceHint,
    /// Offline/runtime evaluation metrics.
    Evaluate,
}

/// Authority effects that are forbidden for recall provider output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallAuthorityEffects {
    pub creates_decision: bool,
    pub creates_decision_candidate: bool,
    pub creates_source_evidence: bool,
    pub creates_binding_evidence: bool,
    pub approves_signoff: bool,
    pub asserts_compliance: bool,
    pub enforces_policy: bool,
    pub asserts_no_conflict: bool,
    pub asserts_merge_safety: bool,
}

impl RecallAuthorityEffects {
    pub const fn none() -> Self {
        Self {
            creates_decision: false,
            creates_decision_candidate: false,
            creates_source_evidence: false,
            creates_binding_evidence: false,
            approves_signoff: false,
            asserts_compliance: false,
            enforces_policy: false,
            asserts_no_conflict: false,
            asserts_merge_safety: false,
        }
    }

    pub const fn any(self) -> bool {
        self.creates_decision
            || self.creates_decision_candidate
            || self.creates_source_evidence
            || self.creates_binding_evidence
            || self.approves_signoff
            || self.asserts_compliance
            || self.enforces_policy
            || self.asserts_no_conflict
            || self.asserts_merge_safety
    }
}

/// Gate decision for one provider signal or candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallAdmissibilityDecision {
    pub outcome: RecallAdmissibilityOutcome,
    pub reason_codes: Vec<RecallAdmissibilityReasonCode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_uses: Vec<RecallAdmissibleUse>,
    pub authority_effects: RecallAuthorityEffects,
}

impl RecallAdmissibilityDecision {
    fn new(
        outcome: RecallAdmissibilityOutcome,
        reason_codes: Vec<RecallAdmissibilityReasonCode>,
        caveats: Vec<String>,
        allowed_uses: Vec<RecallAdmissibleUse>,
    ) -> Self {
        Self {
            outcome,
            reason_codes,
            caveats,
            allowed_uses,
            authority_effects: RecallAuthorityEffects::none(),
        }
    }
}

/// Deterministic gate for provider recall output.
#[derive(Debug, Clone, Default)]
pub struct RecallAdmissibilityGate;

impl RecallAdmissibilityGate {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a generic provider signal.
    pub fn evaluate_signal(
        &self,
        result: &RecallSignalResult,
        signal: &RecallProviderSignal,
    ) -> RecallAdmissibilityDecision {
        let mut ctx = GateContext::new();
        evaluate_provider_status(result.status, &mut ctx);
        evaluate_provenance(
            &result.provenance.provider_id,
            &result.provenance.provider_version,
            result.provenance.index_version.as_deref(),
            &mut ctx,
        );
        evaluate_scope(
            &result.searched_scope,
            !result.unknown_scope.is_empty(),
            &mut ctx,
        );
        evaluate_currentness(signal.currentness, &mut ctx);
        evaluate_snapshot(
            signal
                .snapshot_ref
                .as_deref()
                .or(result.graph_snapshot_id.as_deref()),
            &mut ctx,
        );
        evaluate_signal_identity(signal, &mut ctx);
        evaluate_signal_kind(signal.signal_kind, &mut ctx);
        ctx.finish()
    }

    /// Evaluate a semantic code provider candidate.
    pub fn evaluate_semantic_candidate(
        &self,
        result: &SemanticCodeSearchResult,
        candidate: &SemanticCodeCandidate,
    ) -> RecallAdmissibilityDecision {
        let mut ctx = GateContext::new();
        evaluate_provider_status(result.status, &mut ctx);
        evaluate_provenance(
            &result.provenance.provider_id,
            &result.provenance.provider_version,
            result.provenance.index_version.as_deref(),
            &mut ctx,
        );
        evaluate_scope(
            &result.searched_scope,
            !result.unknown_scope.is_empty(),
            &mut ctx,
        );
        evaluate_currentness(candidate.currentness, &mut ctx);
        evaluate_snapshot(result.graph_snapshot_id.as_deref(), &mut ctx);
        evaluate_semantic_identity(candidate, &mut ctx);
        ctx.downgrade(
            RecallAdmissibilityReasonCode::SemanticSignalRankOnly,
            "semantic code candidates are approximate until deterministic CodeLocator verification",
        );
        ctx.restrict_to_rank_and_explain();
        ctx.finish()
    }
}

#[derive(Debug)]
struct GateContext {
    outcome: RecallAdmissibilityOutcome,
    reason_codes: Vec<RecallAdmissibilityReasonCode>,
    caveats: Vec<String>,
    allowed_uses: Vec<RecallAdmissibleUse>,
}

impl GateContext {
    fn new() -> Self {
        Self {
            outcome: RecallAdmissibilityOutcome::Admit,
            reason_codes: Vec::new(),
            caveats: Vec::new(),
            allowed_uses: vec![
                RecallAdmissibleUse::RankOnly,
                RecallAdmissibleUse::ExplainOnly,
                RecallAdmissibleUse::SliceHint,
                RecallAdmissibleUse::Evaluate,
            ],
        }
    }

    fn add_reason(&mut self, reason: RecallAdmissibilityReasonCode) {
        if !self.reason_codes.contains(&reason) {
            self.reason_codes.push(reason);
        }
    }

    fn add_caveat(&mut self, caveat: impl Into<String>) {
        let caveat = caveat.into();
        if !self.caveats.contains(&caveat) {
            self.caveats.push(caveat);
        }
    }

    fn downgrade(&mut self, reason: RecallAdmissibilityReasonCode, caveat: impl Into<String>) {
        self.add_reason(reason);
        self.add_caveat(caveat);
        if self.outcome == RecallAdmissibilityOutcome::Admit {
            self.outcome = RecallAdmissibilityOutcome::Downgrade;
        }
    }

    fn defer(&mut self, reason: RecallAdmissibilityReasonCode, caveat: impl Into<String>) {
        self.add_reason(reason);
        self.add_caveat(caveat);
        if !matches!(self.outcome, RecallAdmissibilityOutcome::Reject) {
            self.outcome = RecallAdmissibilityOutcome::Defer;
        }
    }

    fn reject(&mut self, reason: RecallAdmissibilityReasonCode, caveat: impl Into<String>) {
        self.add_reason(reason);
        self.add_caveat(caveat);
        self.outcome = RecallAdmissibilityOutcome::Reject;
        self.allowed_uses.clear();
    }

    fn restrict_to_rank_and_explain(&mut self) {
        self.allowed_uses.retain(|use_kind| {
            matches!(
                use_kind,
                RecallAdmissibleUse::RankOnly
                    | RecallAdmissibleUse::ExplainOnly
                    | RecallAdmissibleUse::Evaluate
            )
        });
    }

    fn finish(mut self) -> RecallAdmissibilityDecision {
        if self.outcome == RecallAdmissibilityOutcome::Reject {
            self.allowed_uses.clear();
        }
        RecallAdmissibilityDecision::new(
            self.outcome,
            self.reason_codes,
            self.caveats,
            self.allowed_uses,
        )
    }
}

fn evaluate_provider_status(status: RecallProviderStatus, ctx: &mut GateContext) {
    match status {
        RecallProviderStatus::Ready => ctx.add_reason(RecallAdmissibilityReasonCode::ProviderReady),
        RecallProviderStatus::Stale => ctx.downgrade(
            RecallAdmissibilityReasonCode::ProviderStale,
            "provider reported a stale index",
        ),
        RecallProviderStatus::Indexing => ctx.defer(
            RecallAdmissibilityReasonCode::ProviderIndexing,
            "provider index is still building",
        ),
        RecallProviderStatus::NotConfigured | RecallProviderStatus::Failed => ctx.reject(
            RecallAdmissibilityReasonCode::ProviderUnavailable,
            "provider is unavailable for recall admission",
        ),
        RecallProviderStatus::Unsupported => ctx.reject(
            RecallAdmissibilityReasonCode::ProviderUnsupported,
            "provider does not support this recall request",
        ),
    }
}

fn evaluate_provenance(
    provider_id: &str,
    provider_version: &str,
    index_version: Option<&str>,
    ctx: &mut GateContext,
) {
    if provider_id.trim().is_empty() {
        ctx.reject(
            RecallAdmissibilityReasonCode::MissingProviderIdentity,
            "provider id is required",
        );
    }
    if provider_version.trim().is_empty() {
        ctx.reject(
            RecallAdmissibilityReasonCode::MissingProviderVersion,
            "provider version is required",
        );
    }
    if option_is_blank(index_version) {
        ctx.downgrade(
            RecallAdmissibilityReasonCode::MissingIndexVersion,
            "provider index version is missing",
        );
    }
}

fn evaluate_scope(scope: &RecallSearchedScope, has_unknown_scope: bool, ctx: &mut GateContext) {
    if !has_scope_proof(scope) {
        ctx.reject(
            RecallAdmissibilityReasonCode::MissingSearchedScopeProof,
            "provider output lacks searched-scope proof",
        );
    }
    if has_unknown_scope {
        ctx.downgrade(
            RecallAdmissibilityReasonCode::UnknownScopePresent,
            "provider reported unknown or uncovered scope",
        );
    }
}

fn has_scope_proof(scope: &RecallSearchedScope) -> bool {
    scope
        .scope_fingerprint
        .as_deref()
        .is_some_and(|fingerprint| !fingerprint.trim().is_empty())
        || !scope.files.is_empty()
        || !scope.symbols.is_empty()
        || (scope
            .corpus_id
            .as_deref()
            .is_some_and(|corpus| !corpus.trim().is_empty())
            && scope
                .corpus_version
                .as_deref()
                .is_some_and(|version| !version.trim().is_empty()))
}

fn evaluate_currentness(currentness: CodeSourceCurrentness, ctx: &mut GateContext) {
    match currentness {
        CodeSourceCurrentness::Current => ctx.add_reason(RecallAdmissibilityReasonCode::Current),
        CodeSourceCurrentness::ContentChanged | CodeSourceCurrentness::TargetMissing => {
            ctx.downgrade(
                RecallAdmissibilityReasonCode::StaleCurrentness,
                "code source currentness is stale or target is missing",
            );
        }
        CodeSourceCurrentness::GraphUnavailable | CodeSourceCurrentness::GraphNotConfigured => {
            ctx.defer(
                RecallAdmissibilityReasonCode::UnknownCurrentness,
                "graph currentness is unavailable",
            );
        }
    }
}

fn evaluate_snapshot(snapshot_ref: Option<&str>, ctx: &mut GateContext) {
    if option_is_blank(snapshot_ref) {
        ctx.downgrade(
            RecallAdmissibilityReasonCode::MissingSnapshotRef,
            "graph snapshot or explicit snapshot reference is missing",
        );
    }
}

fn evaluate_signal_identity(signal: &RecallProviderSignal, ctx: &mut GateContext) {
    if signal.target_ref.trim().is_empty() {
        ctx.reject(
            RecallAdmissibilityReasonCode::MissingTargetIdentity,
            "signal target identity is missing",
        );
    }
    if option_is_blank(signal.source_ref.as_deref()) {
        ctx.reject(
            RecallAdmissibilityReasonCode::MissingSourceRef,
            "signal source reference is required",
        );
    }
}

fn evaluate_signal_kind(kind: RecallProviderSignalKind, ctx: &mut GateContext) {
    match kind {
        RecallProviderSignalKind::Semantic => {
            ctx.downgrade(
                RecallAdmissibilityReasonCode::SemanticSignalRankOnly,
                "semantic similarity is rank/explain-only until deterministic verification",
            );
            ctx.restrict_to_rank_and_explain();
        }
        RecallProviderSignalKind::Ownership => {
            ctx.downgrade(
                RecallAdmissibilityReasonCode::OwnershipSignalRankOnly,
                "ownership overlap is a recall signal, not signoff authority",
            );
            ctx.restrict_to_rank_and_explain();
        }
        RecallProviderSignalKind::Codegenome => {
            ctx.downgrade(
                RecallAdmissibilityReasonCode::CodegenomeSignalRankOnly,
                "CodeGenome-style signals are advisory until deterministic verification",
            );
            ctx.restrict_to_rank_and_explain();
        }
        RecallProviderSignalKind::Lexical | RecallProviderSignalKind::SourceOverlap => {
            ctx.downgrade(
                RecallAdmissibilityReasonCode::AdvisorySignalRankOnly,
                "text/source-overlap signals require corroboration before slice hints",
            );
            ctx.restrict_to_rank_and_explain();
        }
        RecallProviderSignalKind::Exact
        | RecallProviderSignalKind::Graph
        | RecallProviderSignalKind::ChangedFile
        | RecallProviderSignalKind::PriorBinding => {}
    }
}

fn evaluate_semantic_identity(candidate: &SemanticCodeCandidate, ctx: &mut GateContext) {
    if candidate.file.trim().is_empty() {
        ctx.reject(
            RecallAdmissibilityReasonCode::MissingTargetIdentity,
            "semantic candidate file identity is missing",
        );
        return;
    }

    let has_precise_identity = match candidate.target_type {
        SemanticCodeTargetType::Symbol => candidate
            .symbol
            .as_deref()
            .is_some_and(|symbol| !symbol.trim().is_empty()),
        SemanticCodeTargetType::CodeRegion => {
            candidate
                .range
                .as_deref()
                .is_some_and(|range| !range.trim().is_empty())
                || candidate
                    .chunk_id
                    .as_deref()
                    .is_some_and(|chunk| !chunk.trim().is_empty())
                || candidate
                    .content_hash
                    .as_deref()
                    .is_some_and(|hash| !hash.trim().is_empty())
        }
        SemanticCodeTargetType::FilePath => candidate
            .content_hash
            .as_deref()
            .is_some_and(|hash| !hash.trim().is_empty()),
        SemanticCodeTargetType::Dependency | SemanticCodeTargetType::DeploySurface => false,
    };

    if !has_precise_identity {
        ctx.downgrade(
            RecallAdmissibilityReasonCode::ApproximateTargetIdentity,
            "semantic candidate target identity is approximate",
        );
    }
}

fn option_is_blank(value: Option<&str>) -> bool {
    match value {
        Some(value) => value.trim().is_empty(),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::recall::{
        RecallProviderProvenance, RecallRankingMetadata, RecallScopeKind,
        RecallUnknownProviderScope, RecallUnknownProviderScopeReason, SemanticScoreKind,
    };

    fn provenance() -> RecallProviderProvenance {
        RecallProviderProvenance {
            provider_id: "fixture-provider".to_string(),
            provider_version: "1.0.0".to_string(),
            index_version: Some("idx-1".to_string()),
            overlay_id: None,
            model_id: None,
            model_version: None,
        }
    }

    fn searched_scope() -> RecallSearchedScope {
        RecallSearchedScope {
            scope_kind: RecallScopeKind::FileSet,
            scope_fingerprint: Some("scope-abc".to_string()),
            files: vec!["src/lib.rs".to_string()],
            symbols: Vec::new(),
            corpus_id: Some("workspace-a".to_string()),
            corpus_version: Some("graph-1".to_string()),
        }
    }

    fn signal_result() -> RecallSignalResult {
        RecallSignalResult {
            status: RecallProviderStatus::Ready,
            provenance: provenance(),
            graph_snapshot_id: Some("graph-1".to_string()),
            searched_scope: searched_scope(),
            unknown_scope: Vec::new(),
            currentness: CodeSourceCurrentness::Current,
            signals: Vec::new(),
        }
    }

    fn provider_signal(kind: RecallProviderSignalKind) -> RecallProviderSignal {
        RecallProviderSignal {
            target_ref: "decision:123".to_string(),
            source_ref: Some("source:git-docs".to_string()),
            snapshot_ref: Some("graph-1".to_string()),
            signal_kind: kind,
            signal_weight: 0.98,
            relevance_reason: "fixture reason".to_string(),
            currentness: CodeSourceCurrentness::Current,
            confidence: Some(0.99),
            ranking_metadata: vec![RecallRankingMetadata {
                key: "route".to_string(),
                value: "fixture".to_string(),
            }],
            provenance: provenance(),
        }
    }

    #[test]
    fn exact_current_scoped_signal_is_admitted_without_authority_effects() {
        let gate = RecallAdmissibilityGate::new();
        let decision = gate.evaluate_signal(
            &signal_result(),
            &provider_signal(RecallProviderSignalKind::Graph),
        );

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Admit);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::ProviderReady));
        assert!(decision
            .allowed_uses
            .contains(&RecallAdmissibleUse::SliceHint));
        assert!(!decision.authority_effects.any());
    }

    #[test]
    fn missing_searched_scope_proof_rejects_signal() {
        let gate = RecallAdmissibilityGate::new();
        let mut result = signal_result();
        result.searched_scope = RecallSearchedScope::query();

        let decision =
            gate.evaluate_signal(&result, &provider_signal(RecallProviderSignalKind::Exact));

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Reject);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::MissingSearchedScopeProof));
        assert!(decision.allowed_uses.is_empty());
        assert!(!decision.authority_effects.any());
    }

    #[test]
    fn unknown_scope_and_stale_currentness_downgrade_with_caveats() {
        let gate = RecallAdmissibilityGate::new();
        let mut result = signal_result();
        result.unknown_scope.push(RecallUnknownProviderScope {
            scope: "src/generated".to_string(),
            reason: RecallUnknownProviderScopeReason::NotIndexed,
            detail: Some("generated code not indexed".to_string()),
        });
        let mut signal = provider_signal(RecallProviderSignalKind::PriorBinding);
        signal.currentness = CodeSourceCurrentness::ContentChanged;

        let decision = gate.evaluate_signal(&result, &signal);

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Downgrade);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::UnknownScopePresent));
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::StaleCurrentness));
        assert!(!decision.caveats.is_empty());
        assert!(!decision.authority_effects.any());
    }

    #[test]
    fn unavailable_currentness_defers_signal() {
        let gate = RecallAdmissibilityGate::new();
        let mut signal = provider_signal(RecallProviderSignalKind::Graph);
        signal.currentness = CodeSourceCurrentness::GraphUnavailable;

        let decision = gate.evaluate_signal(&signal_result(), &signal);

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Defer);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::UnknownCurrentness));
        assert!(!decision.authority_effects.any());
    }

    #[test]
    fn semantic_ownership_and_codegenome_signals_are_rank_only() {
        let gate = RecallAdmissibilityGate::new();
        for kind in [
            RecallProviderSignalKind::Semantic,
            RecallProviderSignalKind::Ownership,
            RecallProviderSignalKind::Codegenome,
        ] {
            let decision = gate.evaluate_signal(&signal_result(), &provider_signal(kind));

            assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Downgrade);
            assert!(decision
                .allowed_uses
                .contains(&RecallAdmissibleUse::RankOnly));
            assert!(decision
                .allowed_uses
                .contains(&RecallAdmissibleUse::ExplainOnly));
            assert!(!decision
                .allowed_uses
                .contains(&RecallAdmissibleUse::SliceHint));
            assert!(!decision.authority_effects.any());
        }
    }

    fn semantic_result() -> SemanticCodeSearchResult {
        SemanticCodeSearchResult {
            status: RecallProviderStatus::Ready,
            provenance: provenance(),
            graph_snapshot_id: Some("graph-1".to_string()),
            searched_scope: searched_scope(),
            unknown_scope: Vec::new(),
            currentness: CodeSourceCurrentness::Current,
            candidates: Vec::new(),
        }
    }

    fn semantic_candidate() -> SemanticCodeCandidate {
        SemanticCodeCandidate {
            file: "src/lib.rs".to_string(),
            range: None,
            symbol: None,
            chunk_id: None,
            content_hash: None,
            target_type: SemanticCodeTargetType::FilePath,
            score: 0.99,
            score_kind: SemanticScoreKind::SemanticSimilarity,
            reasons: vec!["high semantic similarity".to_string()],
            matched_terms: vec!["admissibility".to_string()],
            matched_excerpt: Some("fn admissibility".to_string()),
            currentness: CodeSourceCurrentness::Current,
            provenance: provenance(),
        }
    }

    #[test]
    fn approximate_semantic_candidate_is_downgraded_and_never_authoritative() {
        let gate = RecallAdmissibilityGate::new();

        let decision = gate.evaluate_semantic_candidate(&semantic_result(), &semantic_candidate());

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Downgrade);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::ApproximateTargetIdentity));
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::SemanticSignalRankOnly));
        assert!(!decision.authority_effects.creates_decision);
        assert!(!decision.authority_effects.creates_decision_candidate);
        assert!(!decision.authority_effects.creates_source_evidence);
        assert!(!decision.authority_effects.creates_binding_evidence);
        assert!(!decision.authority_effects.approves_signoff);
        assert!(!decision.authority_effects.asserts_compliance);
        assert!(!decision.authority_effects.enforces_policy);
        assert!(!decision.authority_effects.asserts_no_conflict);
        assert!(!decision.authority_effects.asserts_merge_safety);
    }

    #[test]
    fn missing_source_ref_rejects_provider_signal() {
        let gate = RecallAdmissibilityGate::new();
        let mut signal = provider_signal(RecallProviderSignalKind::Graph);
        signal.source_ref = None;

        let decision = gate.evaluate_signal(&signal_result(), &signal);

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Reject);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::MissingSourceRef));
        assert!(!decision.authority_effects.any());
    }
}
