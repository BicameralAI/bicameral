//! Commit-driven code-evidence freshness read-model — issue #625 (slice 1).
//!
//! Response-contract types for the accepted hybrid freshness policy:
//!
//! - A new commit downgrades freshness for code evidence whose touched scope it
//!   overlaps. Touched evidence is reported as [`CommitFreshness::TouchedStale`]
//!   and needs a refresh before it can be treated as current.
//! - Untouched evidence stays usable only when explicitly tied to its older
//!   `GraphSnapshot`/commit identity, reported as
//!   [`CommitFreshness::SnapshotScoped`].
//! - When the touched scope cannot be determined or a bounded refresh is queued
//!   / over budget, freshness is [`CommitFreshness::UnknownStale`].
//! - `preflight.run` may (in a later slice) request a *bounded* touched-scope
//!   refresh/check from the bot-owned `GraphSnapshotService`. When that refresh
//!   is stale, queued, unavailable, or over budget, the daemon returns the
//!   freshness caveats and next actions in this module instead of blocking or
//!   strengthening claims.
//!
//! This slice ships the **API/types + fixtures** only — no runtime graph
//! refresh. The runtime detection/refresh wiring lands in a later slice.
//!
//! ## Reused vocabulary
//!
//! The read-model deliberately reuses existing graph vocabulary instead of
//! inventing a parallel set:
//!
//! - graph snapshot identity → [`LocatorSnapshotInfo`]
//! - scope identity → [`GraphSliceInfo`]
//! - currentness label → [`CodeSourceCurrentness`]
//! - readiness labels → [`GraphReadiness`] and [`GraphEvidenceState`]
//!
//! Only the commit-driven freshness label ([`CommitFreshness`]), the bounded
//! refresh outcome ([`RefreshOutcome`]), and the daemon-authored caveats/next
//! actions are new — there is no existing vocabulary for those concepts.
//!
//! ## Authority boundary
//!
//! Everything in this module is a **derived, evidence-only read-model**. A
//! freshness label, caveat, or next action never:
//!
//! - mutates accepted `BindingEvidence`;
//! - mutates `Decision` state, signoff, or compliance;
//! - asserts no-conflict or merge-safety;
//! - blocks the operation.
//!
//! Computation is daemon-authored. MCP, the dashboard, and hosted surfaces
//! render the [`PreflightFreshness`] they receive; they must not recompute
//! freshness.

use serde::{Deserialize, Serialize};

use crate::evidence::{CodeSourceCurrentness, GraphEvidenceState, GraphReadiness};
use crate::tool_request::{GraphSliceInfo, LocatorSnapshotInfo};

// ── Commit freshness ───────────────────────────────────────────────────────

/// Freshness of code evidence relative to the current commit/snapshot identity.
///
/// This is an evidence observation, never a governance verdict. In particular
/// it does not strengthen or weaken signoff, compliance, no-conflict, or
/// merge-safety — those remain owned by their existing authority paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitFreshness {
    /// Evidence is current for the resolved HEAD snapshot.
    Fresh,
    /// A new commit touched code in this evidence's scope; freshness is
    /// downgraded and a touched-scope refresh is needed before it is current.
    TouchedStale,
    /// Evidence was not touched by newer commits and remains usable, but only
    /// when explicitly interpreted against its older `GraphSnapshot`/commit
    /// identity rather than current HEAD.
    SnapshotScoped,
    /// The graph could not confirm whether the evidence scope was touched
    /// (e.g. scope undetectable, refresh queued, or over budget); treat as
    /// stale until confirmed.
    UnknownStale,
    /// No graph was available to evaluate freshness.
    GraphUnavailable,
}

impl CommitFreshness {
    /// Whether the evidence is current for the resolved HEAD snapshot.
    ///
    /// Only [`CommitFreshness::Fresh`] is current for HEAD.
    /// [`CommitFreshness::SnapshotScoped`] is usable but only against its
    /// older snapshot identity, so it is not current for HEAD.
    pub fn is_current_for_head(self) -> bool {
        matches!(self, CommitFreshness::Fresh)
    }

    /// Whether a touched-scope refresh is needed to restore currentness.
    pub fn needs_refresh(self) -> bool {
        matches!(
            self,
            CommitFreshness::TouchedStale | CommitFreshness::UnknownStale
        )
    }

    /// Map the freshness label to the reused [`CodeSourceCurrentness`]
    /// vocabulary, given the resolved graph readiness.
    pub fn currentness(self, readiness: GraphReadiness) -> CodeSourceCurrentness {
        match readiness {
            GraphReadiness::NotConfigured => CodeSourceCurrentness::GraphNotConfigured,
            GraphReadiness::Indexing | GraphReadiness::Stale | GraphReadiness::Failed => {
                CodeSourceCurrentness::GraphUnavailable
            }
            GraphReadiness::Ready => match self {
                CommitFreshness::Fresh | CommitFreshness::SnapshotScoped => {
                    CodeSourceCurrentness::Current
                }
                CommitFreshness::TouchedStale => CodeSourceCurrentness::ContentChanged,
                CommitFreshness::UnknownStale | CommitFreshness::GraphUnavailable => {
                    CodeSourceCurrentness::GraphUnavailable
                }
            },
        }
    }

    /// Map the freshness label to the reused [`GraphEvidenceState`] vocabulary.
    pub fn evidence_state(self) -> GraphEvidenceState {
        match self {
            CommitFreshness::Fresh => GraphEvidenceState::Verified,
            CommitFreshness::TouchedStale | CommitFreshness::SnapshotScoped => {
                GraphEvidenceState::UnknownStale
            }
            CommitFreshness::UnknownStale | CommitFreshness::GraphUnavailable => {
                GraphEvidenceState::UnknownNotIndexed
            }
        }
    }
}

// ── Touched scope ──────────────────────────────────────────────────────────

/// How the touched code scope for a preflight request was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchedDetectionMethod {
    /// Touched files were parsed from a supplied unified diff.
    DiffContext,
    /// Touched targets were taken from explicit file/symbol query terms.
    ExplicitTargets,
    /// The cited evidence was confirmed *not* touched by newer commits.
    ConfirmedUntouched,
    /// Touched scope could not be detected (no diff and no targets).
    Unavailable,
}

/// The bounded set of code touched by the change under preflight.
///
/// Drives the hybrid policy: evidence overlapping the touched scope is
/// downgraded; evidence outside it may remain snapshot-scoped; an undetectable
/// scope yields unknown-stale.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TouchedScope {
    /// How the touched scope was detected.
    pub detection: Option<TouchedDetectionMethod>,
    /// Touched file paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Touched symbols.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
}

impl TouchedScope {
    /// A touched scope that could not be detected (unknown whether touched).
    pub fn unavailable() -> Self {
        Self {
            detection: Some(TouchedDetectionMethod::Unavailable),
            files: Vec::new(),
            symbols: Vec::new(),
        }
    }

    /// A scope where the cited evidence was confirmed *not* touched.
    pub fn confirmed_untouched() -> Self {
        Self {
            detection: Some(TouchedDetectionMethod::ConfirmedUntouched),
            files: Vec::new(),
            symbols: Vec::new(),
        }
    }

    /// Touched files detected from a diff.
    pub fn touched_files(files: &[&str]) -> Self {
        Self {
            detection: Some(TouchedDetectionMethod::DiffContext),
            files: files.iter().map(|s| s.to_string()).collect(),
            symbols: Vec::new(),
        }
    }

    /// Touched symbols detected from explicit targets.
    pub fn touched_symbols(symbols: &[&str]) -> Self {
        Self {
            detection: Some(TouchedDetectionMethod::ExplicitTargets),
            files: Vec::new(),
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Number of touched targets (files + symbols).
    pub fn target_count(&self) -> usize {
        self.files.len() + self.symbols.len()
    }

    /// Whether any concrete code target was detected as touched.
    pub fn has_targets(&self) -> bool {
        !self.files.is_empty() || !self.symbols.is_empty()
    }

    /// Whether the scope is explicitly unknown (undetectable).
    pub fn is_unknown(&self) -> bool {
        self.detection == Some(TouchedDetectionMethod::Unavailable)
    }
}

// ── Bounded refresh outcome ────────────────────────────────────────────────

/// Outcome of the bounded touched-scope refresh/check requested from the
/// bot-owned `GraphSnapshotService`.
///
/// `preflight.run` never blocks waiting for a graph rebuild: each non-current
/// outcome is reported as a caveat + next action rather than a stall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// No touched scope was detected, so no refresh was requested.
    NotRequested,
    /// Refresh/check completed within budget; graph is current for the scope.
    RefreshedWithinBudget,
    /// Refresh is running asynchronously and is not yet ready.
    Queued,
    /// Graph is stale and was not refreshed within this request.
    Stale,
    /// Graph/workspace is unavailable or not configured; cannot refresh.
    Unavailable,
    /// Touched scope exceeds the bounded budget; refresh was declined to avoid
    /// an unbounded/blocking rebuild.
    OverBudget,
}

// ── Daemon-authored caveats ────────────────────────────────────────────────

/// Typed code for a daemon-authored freshness caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessCaveatCode {
    /// Graph is stale for the touched scope.
    GraphStale,
    /// A bounded refresh is queued and not yet ready.
    RefreshQueued,
    /// Graph/workspace is unavailable; freshness could not be evaluated.
    RefreshUnavailable,
    /// Touched scope exceeds the bounded refresh budget.
    RefreshOverBudget,
    /// Touched scope could not be determined; freshness is unknown.
    TouchedScopeUnknown,
    /// Evidence is usable only against its older snapshot identity.
    SnapshotScopedEvidence,
    /// Lookup returned no result while the graph was stale — the empty result
    /// is not an all-clear.
    EmptyResultStaleGraph,
}

/// A daemon-authored freshness caveat.
///
/// Caveats are advisory observations. They are rendered by MCP/dashboard/hosted
/// surfaces verbatim and never carry authority or block the operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FreshnessCaveat {
    /// Typed caveat code — surfaces render this directly.
    pub code: FreshnessCaveatCode,
    /// Human-readable explanation of the caveat.
    pub message: String,
    /// Snapshot identity the evidence is scoped to, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// Current resolved HEAD commit sha, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_head_sha: Option<String>,
}

// ── Daemon-authored next actions ───────────────────────────────────────────

/// A daemon-authored next action a caller may take in response to a freshness
/// caveat.
///
/// Next actions are advisory routing hints. None of them mutate canonical
/// state on their own and none of them block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessNextAction {
    /// Request a bounded touched-scope refresh from the GraphSnapshotService.
    RequestTouchedRefresh,
    /// Retry the lookup once the queued refresh is ready.
    RetryWhenReady,
    /// Narrow the touched scope so a bounded refresh fits the budget.
    NarrowScope,
    /// Provide a diff or explicit targets so the touched scope can be detected.
    ProvideTouchedScope,
    /// Proceed using evidence interpreted against its older snapshot identity.
    ProceedSnapshotScoped,
    /// Inspect the cited evidence to assess relevance manually.
    InspectEvidence,
    /// Configure a workspace/graph so freshness can be evaluated.
    ConfigureWorkspace,
}

// ── Preflight freshness block ──────────────────────────────────────────────

/// Daemon-authored freshness block attached to a `preflight.run` response and
/// optionally to a `RecallPacket`.
///
/// Carries the derived commit freshness alongside the reused currentness and
/// readiness vocabulary, the detected touched scope, the snapshot/scope
/// identity it was evaluated against, and the caveats + next actions surfaces
/// should render. It is evidence-only and never mutates authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightFreshness {
    /// Derived commit freshness for the touched code evidence.
    pub commit_freshness: CommitFreshness,
    /// Reused currentness label (graph-fact vocabulary).
    pub currentness: CodeSourceCurrentness,
    /// Reused readiness label for the resolved graph.
    pub graph_readiness: GraphReadiness,
    /// Reused evidence-state label for the cited evidence.
    pub evidence_state: GraphEvidenceState,
    /// Outcome of the bounded touched-scope refresh/check.
    pub refresh_outcome: RefreshOutcome,
    /// The touched code scope detected for this request.
    pub touched_scope: TouchedScope,
    /// Graph snapshot identity the freshness was evaluated against, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot: Option<LocatorSnapshotInfo>,
    /// Scope identity (graph slice) the freshness was evaluated against, when
    /// known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<GraphSliceInfo>,
    /// Daemon-authored caveats for surfaces to render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<FreshnessCaveat>,
    /// Daemon-authored next actions for surfaces to render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<FreshnessNextAction>,
}

/// Deterministic inputs to [`PreflightFreshness::derive`].
///
/// Grouped into a struct so the derivation has a single, named, order-free
/// contract rather than a long positional signature.
#[derive(Debug, Clone)]
pub struct FreshnessInputs {
    /// The touched code scope detected for this request.
    pub touched_scope: TouchedScope,
    /// Outcome of the bounded touched-scope refresh/check.
    pub refresh_outcome: RefreshOutcome,
    /// Resolved readiness of the graph.
    pub graph_readiness: GraphReadiness,
    /// Graph snapshot identity, when known.
    pub graph_snapshot: Option<LocatorSnapshotInfo>,
    /// Scope identity (graph slice), when known.
    pub scope: Option<GraphSliceInfo>,
    /// Whether the lookup returned no relevant result. When true and the graph
    /// is not fresh, the daemon adds an explicit caveat so the empty result is
    /// not read as an all-clear.
    pub result_was_empty: bool,
}

impl PreflightFreshness {
    /// Derive the daemon-authored freshness block from deterministic inputs.
    ///
    /// This is the single source of truth for the hybrid policy mapping. It is
    /// pure and deterministic so the read-model is reproducible and so MCP,
    /// the dashboard, and hosted surfaces render — never recompute — the
    /// result.
    pub fn derive(inputs: FreshnessInputs) -> Self {
        let FreshnessInputs {
            touched_scope,
            refresh_outcome,
            graph_readiness,
            graph_snapshot,
            scope,
            result_was_empty,
        } = inputs;

        let commit_freshness = match refresh_outcome {
            RefreshOutcome::NotRequested | RefreshOutcome::RefreshedWithinBudget => {
                CommitFreshness::Fresh
            }
            RefreshOutcome::Stale => {
                if touched_scope.has_targets() {
                    CommitFreshness::TouchedStale
                } else if touched_scope.is_unknown() {
                    CommitFreshness::UnknownStale
                } else {
                    CommitFreshness::SnapshotScoped
                }
            }
            RefreshOutcome::Queued | RefreshOutcome::OverBudget => CommitFreshness::UnknownStale,
            RefreshOutcome::Unavailable => CommitFreshness::GraphUnavailable,
        };

        let currentness = commit_freshness.currentness(graph_readiness);
        let evidence_state = commit_freshness.evidence_state();

        let snapshot_id = graph_snapshot
            .as_ref()
            .and_then(|s| s.graph_snapshot_id.clone());
        let current_head_sha = graph_snapshot.as_ref().and_then(|s| s.commit_sha.clone());

        let mut caveats = Vec::new();
        let mut next_actions = Vec::new();

        let caveat = |code: FreshnessCaveatCode, message: &str| FreshnessCaveat {
            code,
            message: message.to_string(),
            snapshot_id: snapshot_id.clone(),
            current_head_sha: current_head_sha.clone(),
        };

        match commit_freshness {
            CommitFreshness::Fresh => {}
            CommitFreshness::TouchedStale => {
                caveats.push(caveat(
                    FreshnessCaveatCode::GraphStale,
                    "A newer commit touched code in this evidence's scope; freshness is \
                     downgraded until a bounded touched-scope refresh completes.",
                ));
                next_actions.push(FreshnessNextAction::RequestTouchedRefresh);
                next_actions.push(FreshnessNextAction::InspectEvidence);
            }
            CommitFreshness::SnapshotScoped => {
                caveats.push(caveat(
                    FreshnessCaveatCode::SnapshotScopedEvidence,
                    "Graph is stale, but the cited evidence was not touched by newer \
                     commits; it remains usable only against its older snapshot identity.",
                ));
                next_actions.push(FreshnessNextAction::ProceedSnapshotScoped);
            }
            CommitFreshness::UnknownStale => match refresh_outcome {
                RefreshOutcome::Queued => {
                    caveats.push(caveat(
                        FreshnessCaveatCode::RefreshQueued,
                        "A bounded touched-scope refresh is queued and not yet ready; \
                         freshness cannot be confirmed for HEAD.",
                    ));
                    next_actions.push(FreshnessNextAction::RetryWhenReady);
                    next_actions.push(FreshnessNextAction::ProceedSnapshotScoped);
                }
                RefreshOutcome::OverBudget => {
                    caveats.push(caveat(
                        FreshnessCaveatCode::RefreshOverBudget,
                        "Touched scope exceeds the bounded refresh budget; the daemon \
                         declined an unbounded rebuild rather than blocking.",
                    ));
                    next_actions.push(FreshnessNextAction::NarrowScope);
                    next_actions.push(FreshnessNextAction::RequestTouchedRefresh);
                }
                _ => {
                    caveats.push(caveat(
                        FreshnessCaveatCode::TouchedScopeUnknown,
                        "The touched code scope could not be determined, so freshness \
                         cannot be confirmed; treat the evidence as stale until checked.",
                    ));
                    next_actions.push(FreshnessNextAction::ProvideTouchedScope);
                    next_actions.push(FreshnessNextAction::InspectEvidence);
                }
            },
            CommitFreshness::GraphUnavailable => {
                caveats.push(caveat(
                    FreshnessCaveatCode::RefreshUnavailable,
                    "Graph or workspace is unavailable; touched-code freshness could not \
                     be evaluated.",
                ));
                next_actions.push(FreshnessNextAction::ConfigureWorkspace);
            }
        }

        // A no-result lookup on a non-fresh graph gets an explicit caveat so an
        // empty result is never mistaken for a confirmed all-clear.
        if result_was_empty && !commit_freshness.is_current_for_head() {
            caveats.push(caveat(
                FreshnessCaveatCode::EmptyResultStaleGraph,
                "Lookup returned no relevant decisions while the graph was not fresh; \
                 do not treat this empty result as a confirmed all-clear.",
            ));
            if !next_actions.contains(&FreshnessNextAction::InspectEvidence) {
                next_actions.push(FreshnessNextAction::InspectEvidence);
            }
        }

        Self {
            commit_freshness,
            currentness,
            graph_readiness,
            evidence_state,
            refresh_outcome,
            touched_scope,
            graph_snapshot,
            scope,
            caveats,
            next_actions,
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_request::SliceCompleteness;

    fn snap() -> LocatorSnapshotInfo {
        LocatorSnapshotInfo {
            graph_snapshot_id: Some("snap-1".into()),
            repo_id: None,
            commit_sha: Some("headsha".into()),
            index_version: None,
            ref_name: None,
        }
    }

    fn inputs(
        touched: TouchedScope,
        outcome: RefreshOutcome,
        readiness: GraphReadiness,
    ) -> FreshnessInputs {
        FreshnessInputs {
            touched_scope: touched,
            refresh_outcome: outcome,
            graph_readiness: readiness,
            graph_snapshot: Some(snap()),
            scope: None,
            result_was_empty: false,
        }
    }

    #[test]
    fn refreshed_within_budget_is_fresh_no_caveats() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::touched_files(&["src/a.rs"]),
            RefreshOutcome::RefreshedWithinBudget,
            GraphReadiness::Ready,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::Fresh);
        assert_eq!(f.currentness, CodeSourceCurrentness::Current);
        assert_eq!(f.evidence_state, GraphEvidenceState::Verified);
        assert!(f.caveats.is_empty());
        assert!(f.next_actions.is_empty());
    }

    #[test]
    fn touched_file_changed_on_ready_graph_is_content_changed() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::touched_files(&["src/a.rs"]),
            RefreshOutcome::Stale,
            GraphReadiness::Ready,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::TouchedStale);
        assert_eq!(f.currentness, CodeSourceCurrentness::ContentChanged);
        assert_eq!(f.evidence_state, GraphEvidenceState::UnknownStale);
        assert_eq!(f.caveats[0].code, FreshnessCaveatCode::GraphStale);
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::RequestTouchedRefresh));
    }

    #[test]
    fn stale_graph_currentness_is_graph_unavailable() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::touched_files(&["src/a.rs"]),
            RefreshOutcome::Stale,
            GraphReadiness::Stale,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::TouchedStale);
        assert_eq!(f.currentness, CodeSourceCurrentness::GraphUnavailable);
        assert_eq!(f.graph_readiness, GraphReadiness::Stale);
    }

    #[test]
    fn confirmed_untouched_is_snapshot_scoped() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::confirmed_untouched(),
            RefreshOutcome::Stale,
            GraphReadiness::Stale,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::SnapshotScoped);
        assert_eq!(
            f.caveats[0].code,
            FreshnessCaveatCode::SnapshotScopedEvidence
        );
        assert_eq!(
            f.next_actions,
            vec![FreshnessNextAction::ProceedSnapshotScoped]
        );
        assert!(!f.commit_freshness.is_current_for_head());
    }

    #[test]
    fn unknown_touched_scope_is_unknown_stale() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::unavailable(),
            RefreshOutcome::Stale,
            GraphReadiness::Stale,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::UnknownStale);
        assert_eq!(f.caveats[0].code, FreshnessCaveatCode::TouchedScopeUnknown);
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::ProvideTouchedScope));
    }

    #[test]
    fn queued_is_unknown_stale_with_retry() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::touched_files(&["src/a.rs"]),
            RefreshOutcome::Queued,
            GraphReadiness::Indexing,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::UnknownStale);
        assert_eq!(f.caveats[0].code, FreshnessCaveatCode::RefreshQueued);
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::RetryWhenReady));
    }

    #[test]
    fn over_budget_is_unknown_stale_with_narrow_scope() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::touched_files(&["a.rs", "b.rs", "c.rs"]),
            RefreshOutcome::OverBudget,
            GraphReadiness::Ready,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::UnknownStale);
        assert_eq!(f.caveats[0].code, FreshnessCaveatCode::RefreshOverBudget);
        assert!(f.next_actions.contains(&FreshnessNextAction::NarrowScope));
    }

    #[test]
    fn unavailable_is_graph_unavailable() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::unavailable(),
            RefreshOutcome::Unavailable,
            GraphReadiness::NotConfigured,
        ));
        assert_eq!(f.commit_freshness, CommitFreshness::GraphUnavailable);
        assert_eq!(f.currentness, CodeSourceCurrentness::GraphNotConfigured);
        assert_eq!(f.caveats[0].code, FreshnessCaveatCode::RefreshUnavailable);
        assert_eq!(
            f.next_actions,
            vec![FreshnessNextAction::ConfigureWorkspace]
        );
    }

    #[test]
    fn empty_result_on_stale_graph_adds_caveat() {
        let mut i = inputs(
            TouchedScope::confirmed_untouched(),
            RefreshOutcome::Stale,
            GraphReadiness::Stale,
        );
        i.result_was_empty = true;
        let f = PreflightFreshness::derive(i);
        assert!(f
            .caveats
            .iter()
            .any(|c| c.code == FreshnessCaveatCode::EmptyResultStaleGraph));
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::InspectEvidence));
    }

    #[test]
    fn scope_identity_is_carried_through() {
        let mut i = inputs(
            TouchedScope::touched_symbols(&["auth::verify"]),
            RefreshOutcome::Stale,
            GraphReadiness::Ready,
        );
        i.scope = Some(GraphSliceInfo {
            graph_slice_id: Some("slice-1".into()),
            scope_kind: Some("symbol_scoped".into()),
            scope_fingerprint: Some("fp-abc".into()),
            completeness: Some(SliceCompleteness::SymbolScoped),
        });
        let f = PreflightFreshness::derive(i);
        assert_eq!(
            f.scope.as_ref().unwrap().scope_kind.as_deref(),
            Some("symbol_scoped")
        );
    }

    #[test]
    fn never_emits_authority_or_governance_fields() {
        for outcome in [
            RefreshOutcome::NotRequested,
            RefreshOutcome::RefreshedWithinBudget,
            RefreshOutcome::Queued,
            RefreshOutcome::Stale,
            RefreshOutcome::Unavailable,
            RefreshOutcome::OverBudget,
        ] {
            for touched in [
                TouchedScope::touched_files(&["src/a.rs"]),
                TouchedScope::touched_symbols(&["m::f"]),
                TouchedScope::confirmed_untouched(),
                TouchedScope::unavailable(),
            ] {
                let mut i = inputs(touched, outcome, GraphReadiness::Stale);
                i.result_was_empty = true;
                let f = PreflightFreshness::derive(i);
                let json = serde_json::to_string(&f).unwrap();
                for forbidden in [
                    "signoff",
                    "compliance",
                    "compliant",
                    "approved",
                    "no_conflict",
                    "no-conflict",
                    "merge_safe",
                    "merge-safety",
                    "binding_evidence",
                    "\"safe\"",
                    "\"blocked\"",
                ] {
                    assert!(
                        !json.contains(forbidden),
                        "freshness read-model must not contain '{forbidden}': {json}"
                    );
                }
            }
        }
    }

    #[test]
    fn roundtrips_through_json() {
        let f = PreflightFreshness::derive(inputs(
            TouchedScope::touched_files(&["src/a.rs"]),
            RefreshOutcome::Stale,
            GraphReadiness::Stale,
        ));
        let json = serde_json::to_string(&f).unwrap();
        let parsed: PreflightFreshness = serde_json::from_str(&json).unwrap();
        assert_eq!(f, parsed);
    }
}
