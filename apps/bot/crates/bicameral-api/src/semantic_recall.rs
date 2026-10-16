//! Semantic recall contribution read-model — issue #640.
//!
//! `code.locate` already fuses semantic-provider candidates into its ranked
//! [`LocatorResult`] (issue #639 / PR #654), distinguishing graph-verified
//! targets ([`LocatorReasonCode::SemanticTarget`]) from approximate hints
//! ([`LocatorReasonCode::SemanticHint`]). This module renders that fused
//! contribution — plus the provider's searched/unknown scope and status — as a
//! single daemon-authored read-model that surfaces (`search.query`,
//! `decision.find_code_impact`, `preflight.run`, the `RecallPacket`/projection,
//! and the dashboard) render verbatim.
//!
//! ## Authority boundary
//!
//! Everything in this module is a **derived, evidence-only read-model**. A
//! semantic recall view, entry, caveat, or next action never:
//!
//! - mutates accepted `BindingEvidence`;
//! - mutates `Decision` state, signoff, or compliance;
//! - asserts no-conflict, merge-safety, enforcement, or safe-to-proceed;
//! - blocks the operation.
//!
//! ## Approximate vs verified
//!
//! The view keeps approximate semantic hints and graph-verified targets in
//! **separate** collections ([`SemanticRecallView::approximate_hints`] and
//! [`SemanticRecallView::verified_targets`]) so no surface can collapse them
//! into one state. A semantic-only contribution is always presented as
//! approximate/advisory unless deterministic graph verification proved it.
//!
//! Raw provider scores are intentionally **not** exposed as user authority:
//! this read-model carries only the reused locator confidence/evidence-state
//! vocabulary, never a numeric provider score.
//!
//! ## Quiet no-op
//!
//! [`SemanticRecallView::derive`] returns [`None`] when no semantic provider is
//! configured ([`RecallProviderStatus::NotConfigured`]). The optional field is
//! then omitted entirely, so existing no-provider behavior stays deterministic
//! and quiet.

use serde::{Deserialize, Serialize};

use crate::evidence::CodeSourceCurrentness;
use crate::recall::{
    RecallProviderProvenance, RecallProviderStatus, RecallSearchedScope, RecallUnknownProviderScope,
};
use crate::tool_request::{
    LocatorCandidate, LocatorConfidence, LocatorEvidenceState, LocatorReasonCode, LocatorResult,
};

// ── Evidence tier ───────────────────────────────────────────────────────────

/// Evidence tier of a semantic recall entry.
///
/// The tier is the load-bearing distinction of this read-model: a
/// [`SemanticEvidenceTier::GraphVerified`] entry was proved by deterministic
/// graph verification, while a [`SemanticEvidenceTier::ApproximateHint`] entry
/// is an advisory provider hint that has not been graph-verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticEvidenceTier {
    /// A semantic candidate verified through deterministic graph proof.
    GraphVerified,
    /// An approximate semantic/provider hint without deterministic graph proof.
    /// Advisory only — never binding-authoritative.
    ApproximateHint,
}

impl SemanticEvidenceTier {
    /// Classify a locator candidate's semantic tier, if it carries one.
    ///
    /// Returns `None` for candidates that did not originate from the semantic
    /// provider fusion (i.e. carry neither [`LocatorReasonCode::SemanticTarget`]
    /// nor [`LocatorReasonCode::SemanticHint`]).
    pub fn from_candidate(candidate: &LocatorCandidate) -> Option<Self> {
        let mut has_target = false;
        let mut has_hint = false;
        for reason in &candidate.reasons {
            match reason.code {
                LocatorReasonCode::SemanticTarget => has_target = true,
                LocatorReasonCode::SemanticHint => has_hint = true,
                _ => {}
            }
        }
        if has_target {
            Some(SemanticEvidenceTier::GraphVerified)
        } else if has_hint {
            Some(SemanticEvidenceTier::ApproximateHint)
        } else {
            None
        }
    }

    /// Whether this tier is advisory-only (never binding-authoritative).
    pub fn is_advisory(self) -> bool {
        matches!(self, SemanticEvidenceTier::ApproximateHint)
    }
}

// ── Semantic recall entry ────────────────────────────────────────────────────

/// A single semantic recall contribution rendered on a surface.
///
/// Carries the fused locator candidate's identity, evidence state, ranking
/// reasons, and per-candidate caveats — but never a raw provider score.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticRecallEntry {
    /// Evidence tier — keeps approximate hints distinct from verified targets.
    pub tier: SemanticEvidenceTier,
    /// File path of the contributed target.
    pub file: String,
    /// Symbol name, when the target is a symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Line range within the file, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// Graph evidence state for this candidate (reused locator vocabulary).
    pub evidence_state: LocatorEvidenceState,
    /// Confidence ranking (reused locator vocabulary).
    pub confidence: LocatorConfidence,
    /// Human-readable ranking reasons explaining the contribution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    /// Admissibility / graph-proof caveats carried from the fused candidate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
}

impl SemanticRecallEntry {
    /// Build a semantic recall entry from a fused locator candidate.
    ///
    /// Returns `None` when the candidate did not originate from semantic
    /// provider fusion (no `SemanticTarget`/`SemanticHint` reason).
    pub fn from_candidate(candidate: &LocatorCandidate) -> Option<Self> {
        let tier = SemanticEvidenceTier::from_candidate(candidate)?;
        Some(Self {
            tier,
            file: candidate.file.clone(),
            symbol: candidate.symbol.clone(),
            range: candidate.range.clone(),
            evidence_state: candidate.evidence_state,
            confidence: candidate.confidence,
            reasons: candidate.reasons.iter().map(|r| r.detail.clone()).collect(),
            caveats: candidate.caveats.clone(),
        })
    }
}

// ── Daemon-authored caveats ──────────────────────────────────────────────────

/// Typed code for a daemon-authored semantic recall caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRecallCaveatCode {
    /// Approximate semantic hints are present and are advisory only.
    ApproximateHintsPresent,
    /// The semantic index is stale; contributions may not reflect current code.
    StaleIndex,
    /// The semantic provider is still indexing; coverage is incomplete.
    ProviderIndexing,
    /// The semantic provider does not support this language/repository.
    ProviderUnsupported,
    /// The semantic provider failed; no semantic contribution was retrieved.
    ProviderFailed,
    /// Part of the requested scope was not covered by the provider.
    UnknownScope,
    /// No semantic contribution was found within the searched scope.
    /// The empty result is scoped to what was searched — not an all-clear.
    NoResultInSearchedScope,
}

/// A daemon-authored semantic recall caveat.
///
/// Caveats are advisory observations rendered verbatim by surfaces. They never
/// carry authority or block the operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticRecallCaveat {
    /// Typed caveat code — surfaces render this directly.
    pub code: SemanticRecallCaveatCode,
    /// Human-readable explanation of the caveat.
    pub message: String,
}

impl SemanticRecallCaveat {
    fn new(code: SemanticRecallCaveatCode, message: &str) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

// ── Daemon-authored next actions ─────────────────────────────────────────────

/// A daemon-authored next action a caller may take in response to a semantic
/// recall caveat.
///
/// Next actions are advisory routing hints. None mutate canonical state and
/// none block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRecallNextAction {
    /// Verify an approximate hint with deterministic graph proof before binding.
    VerifyWithGraphProof,
    /// Refresh the semantic index so contributions reflect current code.
    RefreshSemanticIndex,
    /// Narrow the query/scope to improve provider coverage.
    NarrowScope,
    /// Inspect a candidate manually to assess relevance.
    InspectCandidate,
    /// Configure a semantic provider so semantic recall can contribute.
    ConfigureProvider,
    /// Retry the lookup once the provider finishes indexing.
    RetryWhenReady,
}

// ── Semantic recall view ─────────────────────────────────────────────────────

/// Daemon-authored semantic recall contribution attached to a surface.
///
/// Carries the provider status and searched/unknown scope, the fused semantic
/// contribution split into graph-verified targets and approximate hints, and
/// the caveats + next actions surfaces should render. It is evidence-only and
/// never mutates authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticRecallView {
    /// Runtime status of the semantic provider.
    pub provider_status: RecallProviderStatus,
    /// Reused currentness label for the provider's contribution.
    pub currentness: CodeSourceCurrentness,
    /// The exact bounded scope the provider actually searched.
    pub searched_scope: RecallSearchedScope,
    /// Scope the provider could not cover (not indexed, stale, unsupported…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_scope: Vec<RecallUnknownProviderScope>,
    /// Semantic candidates verified through deterministic graph proof.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_targets: Vec<SemanticRecallEntry>,
    /// Approximate semantic hints without deterministic graph proof (advisory).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approximate_hints: Vec<SemanticRecallEntry>,
    /// Whether the lookup returned no semantic contribution within the searched
    /// scope. The empty result is scoped to what was searched.
    pub no_result_in_searched_scope: bool,
    /// Daemon-authored caveats for surfaces to render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<SemanticRecallCaveat>,
    /// Daemon-authored next actions for surfaces to render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<SemanticRecallNextAction>,
    /// Stable provider provenance for reproducibility.
    pub provenance: RecallProviderProvenance,
}

/// Deterministic inputs to [`SemanticRecallView::derive`].
#[derive(Debug, Clone)]
pub struct SemanticRecallInputs<'a> {
    /// Provider status, searched/unknown scope, currentness, and provenance.
    pub provider_status: RecallProviderStatus,
    /// Currentness label for the provider contribution.
    pub currentness: CodeSourceCurrentness,
    /// The exact bounded scope the provider searched.
    pub searched_scope: RecallSearchedScope,
    /// Scope the provider could not cover.
    pub unknown_scope: Vec<RecallUnknownProviderScope>,
    /// Provider provenance.
    pub provenance: RecallProviderProvenance,
    /// The fused locator candidates carrying the semantic contribution. The
    /// `SemanticTarget`/`SemanticHint` reason on each candidate decides its
    /// tier; non-semantic candidates are ignored.
    pub locator_candidates: &'a [LocatorCandidate],
}

impl<'a> SemanticRecallInputs<'a> {
    /// Build inputs from a fused [`LocatorResult`] and the semantic provider's
    /// status/scope context.
    pub fn from_locator(
        locator: &'a LocatorResult,
        provider_status: RecallProviderStatus,
        currentness: CodeSourceCurrentness,
        searched_scope: RecallSearchedScope,
        unknown_scope: Vec<RecallUnknownProviderScope>,
        provenance: RecallProviderProvenance,
    ) -> Self {
        Self {
            provider_status,
            currentness,
            searched_scope,
            unknown_scope,
            provenance,
            locator_candidates: &locator.candidates,
        }
    }
}

impl SemanticRecallView {
    /// Derive the daemon-authored semantic recall view from deterministic
    /// inputs.
    ///
    /// Returns `None` when no semantic provider is configured
    /// ([`RecallProviderStatus::NotConfigured`]) so the no-op stays quiet.
    /// This is the single source of truth for the read-model mapping: it is
    /// pure and deterministic so surfaces render — never recompute — the view.
    pub fn derive(inputs: SemanticRecallInputs<'_>) -> Option<Self> {
        let SemanticRecallInputs {
            provider_status,
            currentness,
            searched_scope,
            unknown_scope,
            provenance,
            locator_candidates,
        } = inputs;

        if provider_status == RecallProviderStatus::NotConfigured {
            return None;
        }

        let mut verified_targets = Vec::new();
        let mut approximate_hints = Vec::new();
        for candidate in locator_candidates {
            if let Some(entry) = SemanticRecallEntry::from_candidate(candidate) {
                match entry.tier {
                    SemanticEvidenceTier::GraphVerified => verified_targets.push(entry),
                    SemanticEvidenceTier::ApproximateHint => approximate_hints.push(entry),
                }
            }
        }

        let no_result_in_searched_scope =
            verified_targets.is_empty() && approximate_hints.is_empty();

        let mut caveats = Vec::new();
        let mut next_actions = Vec::new();

        match provider_status {
            RecallProviderStatus::Stale => {
                caveats.push(SemanticRecallCaveat::new(
                    SemanticRecallCaveatCode::StaleIndex,
                    "The semantic index is stale; semantic contributions may not reflect \
                     the current code. Treat them as approximate until refreshed.",
                ));
                next_actions.push(SemanticRecallNextAction::RefreshSemanticIndex);
            }
            RecallProviderStatus::Indexing => {
                caveats.push(SemanticRecallCaveat::new(
                    SemanticRecallCaveatCode::ProviderIndexing,
                    "The semantic provider is still indexing; semantic coverage is \
                     incomplete for the searched scope.",
                ));
                next_actions.push(SemanticRecallNextAction::RetryWhenReady);
            }
            RecallProviderStatus::Unsupported => {
                caveats.push(SemanticRecallCaveat::new(
                    SemanticRecallCaveatCode::ProviderUnsupported,
                    "The semantic provider does not support this language or repository; \
                     no semantic contribution is available for the searched scope.",
                ));
            }
            RecallProviderStatus::Failed => {
                caveats.push(SemanticRecallCaveat::new(
                    SemanticRecallCaveatCode::ProviderFailed,
                    "The semantic provider failed; no semantic contribution was retrieved \
                     for the searched scope.",
                ));
            }
            RecallProviderStatus::Ready | RecallProviderStatus::NotConfigured => {}
        }

        if !approximate_hints.is_empty() {
            caveats.push(SemanticRecallCaveat::new(
                SemanticRecallCaveatCode::ApproximateHintsPresent,
                "Approximate semantic hints are advisory and require deterministic graph \
                 proof before they can be treated as verified targets.",
            ));
            next_actions.push(SemanticRecallNextAction::VerifyWithGraphProof);
            next_actions.push(SemanticRecallNextAction::InspectCandidate);
        }

        if !unknown_scope.is_empty() {
            caveats.push(SemanticRecallCaveat::new(
                SemanticRecallCaveatCode::UnknownScope,
                "Part of the requested scope was not covered by the semantic provider; \
                 coverage is partial for the searched scope.",
            ));
            next_actions.push(SemanticRecallNextAction::NarrowScope);
        }

        if no_result_in_searched_scope && provider_status == RecallProviderStatus::Ready {
            caveats.push(SemanticRecallCaveat::new(
                SemanticRecallCaveatCode::NoResultInSearchedScope,
                "No semantic contribution was found within the searched scope. This is \
                 scoped to what was searched and does not describe code outside it.",
            ));
        }

        Some(Self {
            provider_status,
            currentness,
            searched_scope,
            unknown_scope,
            verified_targets,
            approximate_hints,
            no_result_in_searched_scope,
            caveats,
            next_actions,
            provenance,
        })
    }

    /// Whether the view carries any approximate (non-graph-verified) hint.
    pub fn has_approximate_hints(&self) -> bool {
        !self.approximate_hints.is_empty()
    }

    /// Whether the view carries any graph-verified target.
    pub fn has_verified_targets(&self) -> bool {
        !self.verified_targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_request::{LocatorReason, LocatorTargetType};

    fn provenance() -> RecallProviderProvenance {
        RecallProviderProvenance {
            provider_id: "fake-semantic".into(),
            provider_version: "0.1.0".into(),
            index_version: Some("idx-1".into()),
            overlay_id: None,
            model_id: None,
            model_version: None,
        }
    }

    fn candidate(
        file: &str,
        evidence_state: LocatorEvidenceState,
        reason: LocatorReasonCode,
        caveats: Vec<String>,
    ) -> LocatorCandidate {
        LocatorCandidate {
            target: file.to_string(),
            target_type: LocatorTargetType::FilePath,
            file: file.to_string(),
            range: None,
            symbol: None,
            evidence_state,
            confidence: LocatorConfidence::Medium,
            reasons: vec![LocatorReason {
                code: reason,
                detail: "semantic provider contribution".into(),
            }],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
            caveats,
        }
    }

    fn inputs<'a>(
        status: RecallProviderStatus,
        candidates: &'a [LocatorCandidate],
    ) -> SemanticRecallInputs<'a> {
        SemanticRecallInputs {
            provider_status: status,
            currentness: CodeSourceCurrentness::Current,
            searched_scope: RecallSearchedScope::query(),
            unknown_scope: Vec::new(),
            provenance: provenance(),
            locator_candidates: candidates,
        }
    }

    #[test]
    fn not_configured_is_quiet() {
        let view = SemanticRecallView::derive(inputs(RecallProviderStatus::NotConfigured, &[]));
        assert!(view.is_none());
    }

    #[test]
    fn approximate_and_verified_stay_separate() {
        let candidates = vec![
            candidate(
                "src/a.rs",
                LocatorEvidenceState::Verified,
                LocatorReasonCode::SemanticTarget,
                vec![],
            ),
            candidate(
                "src/b.rs",
                LocatorEvidenceState::ApproximateCandidate,
                LocatorReasonCode::SemanticHint,
                vec!["approximate semantic candidate: requires deterministic graph proof".into()],
            ),
        ];
        let view = SemanticRecallView::derive(inputs(RecallProviderStatus::Ready, &candidates))
            .expect("configured provider yields a view");
        assert_eq!(view.verified_targets.len(), 1);
        assert_eq!(view.approximate_hints.len(), 1);
        assert_eq!(
            view.verified_targets[0].tier,
            SemanticEvidenceTier::GraphVerified
        );
        assert_eq!(
            view.approximate_hints[0].tier,
            SemanticEvidenceTier::ApproximateHint
        );
        assert!(!view.no_result_in_searched_scope);
        assert!(view
            .caveats
            .iter()
            .any(|c| c.code == SemanticRecallCaveatCode::ApproximateHintsPresent));
        assert!(view
            .next_actions
            .contains(&SemanticRecallNextAction::VerifyWithGraphProof));
    }

    #[test]
    fn stale_provider_emits_refresh() {
        let view = SemanticRecallView::derive(inputs(RecallProviderStatus::Stale, &[]))
            .expect("configured provider yields a view");
        assert!(view
            .caveats
            .iter()
            .any(|c| c.code == SemanticRecallCaveatCode::StaleIndex));
        assert!(view
            .next_actions
            .contains(&SemanticRecallNextAction::RefreshSemanticIndex));
    }

    #[test]
    fn ready_with_no_candidates_is_scoped_no_result() {
        let view = SemanticRecallView::derive(inputs(RecallProviderStatus::Ready, &[]))
            .expect("configured provider yields a view");
        assert!(view.no_result_in_searched_scope);
        assert!(view
            .caveats
            .iter()
            .any(|c| c.code == SemanticRecallCaveatCode::NoResultInSearchedScope));
    }

    #[test]
    fn no_forbidden_authority_language() {
        // Build a view exercising every status + tier so all caveat copy is
        // present, then assert none of it implies authority it does not have.
        let candidates = vec![
            candidate(
                "src/a.rs",
                LocatorEvidenceState::Verified,
                LocatorReasonCode::SemanticTarget,
                vec![],
            ),
            candidate(
                "src/b.rs",
                LocatorEvidenceState::ApproximateCandidate,
                LocatorReasonCode::SemanticHint,
                vec![],
            ),
        ];
        for status in [
            RecallProviderStatus::Ready,
            RecallProviderStatus::Stale,
            RecallProviderStatus::Indexing,
            RecallProviderStatus::Unsupported,
            RecallProviderStatus::Failed,
        ] {
            let mut i = inputs(status, &candidates);
            i.unknown_scope = vec![RecallUnknownProviderScope {
                scope: "vendor/".into(),
                reason: crate::recall::RecallUnknownProviderScopeReason::NotIndexed,
                detail: None,
            }];
            let view = SemanticRecallView::derive(i).expect("configured");
            let json = serde_json::to_string(&view).unwrap().to_lowercase();
            for forbidden in [
                "complete recall",
                "no conflict",
                "no-conflict",
                "compliance",
                "signoff",
                "merge safe",
                "merge-safe",
                "safe to proceed",
                "safe-to-proceed",
                "enforcement",
            ] {
                assert!(
                    !json.contains(forbidden),
                    "status {status:?} caveat copy must not contain {forbidden:?}: {json}"
                );
            }
        }
    }
}
