//! Runtime touched-scope detection and bounded graph refresh/check for
//! `preflight.run` — issue #625 (slice 2).
//!
//! Slice 1 (PR #633) shipped the response-contract types and fixtures: the
//! daemon-authored [`PreflightFreshness`] read-model and its pure
//! [`PreflightFreshness::derive`] mapping. This slice wires the runtime:
//!
//! 1. Derive the touched code scope from the preflight request inputs
//!    (`diff_context`, `files`, `symbols`, and the resolved branch).
//! 2. Request a *bounded* daemon-owned graph readiness/refresh check from the
//!    bot-owned [`GraphSnapshotService`], expressed as a [`GraphRefreshParams`]
//!    with [`RefreshTrigger::Preflight`], [`AllowIndex::Bounded`], and a
//!    [`RefreshScopeKind`] of `Diff`, `FileSet`, or `SymbolSet`.
//! 3. Map the bounded outcome + readiness + snapshot/scope identity into
//!    deterministic [`FreshnessInputs`] and `derive` the freshness block.
//!
//! ## Non-blocking contract
//!
//! `preflight.run` never blocks waiting for a full graph rebuild. Each
//! non-current readiness (indexing, stale, unavailable) or an over-budget
//! touched scope is reported as a [`RefreshOutcome`] that `derive` turns into
//! daemon-authored caveats + next actions — never a stall and never a stronger
//! claim.
//!
//! ## Authority boundary
//!
//! Everything here produces a derived, evidence-only read-model. Touched-scope
//! detection and the bounded refresh/check never mutate `BindingEvidence`,
//! `Decision` state, signoff, compliance, no-conflict, or merge-safety
//! authority. Freshness is daemon-authored; MCP/dashboard/hosted only render
//! the result.

use bicameral_api::evidence::GraphReadiness;
use bicameral_api::freshness::{
    FreshnessInputs, PreflightFreshness, RefreshOutcome, TouchedDetectionMethod, TouchedScope,
};
use bicameral_api::tool_request::{
    AllowIndex, GraphRefreshParams, GraphSliceInfo, LocatorSnapshotInfo, RefreshScopeKind,
    RefreshTrigger, SliceCompleteness,
};

use crate::graph_snapshot_service::GraphSnapshotService;
use crate::preflight::PreflightQuery;

/// Upper bound on the number of touched targets a single bounded preflight
/// refresh will attempt. Beyond this the daemon declines the refresh
/// ([`RefreshOutcome::OverBudget`]) rather than risk an unbounded rebuild.
pub const BOUNDED_SCOPE_BUDGET: usize = 64;

// ── Touched-scope detection ─────────────────────────────────────────────────

/// Derive the touched code scope from a preflight request.
///
/// Precedence:
/// 1. A supplied unified `diff_context` whose headers name touched files →
///    [`TouchedDetectionMethod::DiffContext`] (any explicit `symbols` are
///    carried alongside the diff-derived files).
/// 2. Explicit `files`/`symbols` query terms →
///    [`TouchedDetectionMethod::ExplicitTargets`].
/// 3. Neither → [`TouchedDetectionMethod::Unavailable`] (scope unknown).
///
/// `base/head` planned-edit context is folded in where already available: the
/// resolved `branch` is recorded on the bounded refresh request, not on the
/// touched scope itself.
pub fn derive_touched_scope(query: &PreflightQuery) -> TouchedScope {
    let diff_files = query
        .diff_context
        .as_deref()
        .map(parse_touched_files_from_diff)
        .unwrap_or_default();

    if !diff_files.is_empty() {
        return TouchedScope {
            detection: Some(TouchedDetectionMethod::DiffContext),
            files: diff_files,
            symbols: dedup_preserving_order(&query.symbols),
        };
    }

    let files = dedup_preserving_order(&query.files);
    let symbols = dedup_preserving_order(&query.symbols);
    if !files.is_empty() || !symbols.is_empty() {
        return TouchedScope {
            detection: Some(TouchedDetectionMethod::ExplicitTargets),
            files,
            symbols,
        };
    }

    TouchedScope::unavailable()
}

/// Parse touched file paths from a unified diff.
///
/// Recognizes both `diff --git a/<path> b/<path>` headers and `+++ b/<path>` /
/// `--- a/<path>` file markers, stripping the conventional `a/` and `b/`
/// prefixes. `/dev/null` markers (added/deleted file placeholders) are ignored.
/// Results are de-duplicated while preserving first-seen order so detection is
/// deterministic.
pub fn parse_touched_files_from_diff(diff: &str) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();

    let mut push = |path: &str| {
        let path = path.trim();
        if path.is_empty() || path == "/dev/null" {
            return;
        }
        let cleaned = strip_diff_prefix(path);
        if !cleaned.is_empty() && !files.iter().any(|f| f == cleaned) {
            files.push(cleaned.to_string());
        }
    };

    for line in diff.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // `a/<path> b/<path>` — take the destination (b/) side.
            if let Some((_, b)) = rest.split_once(' ') {
                push(b);
            }
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            push(rest);
        } else if let Some(rest) = line.strip_prefix("--- ") {
            push(rest);
        }
    }

    files
}

/// Strip a leading `a/` or `b/` diff prefix from a file path.
fn strip_diff_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

fn dedup_preserving_order(items: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !out.iter().any(|x| x == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

// ── Bounded refresh request ─────────────────────────────────────────────────

/// Build the bounded graph refresh/check request for a preflight touched scope.
///
/// Always sets [`RefreshTrigger::Preflight`] and [`AllowIndex::Bounded`] so the
/// graph service performs a scope-limited check, never a full repo rebuild. The
/// [`RefreshScopeKind`] follows how the scope was detected: a diff yields
/// `Diff`, explicit symbols yield `SymbolSet`, explicit files yield `FileSet`,
/// and an undetectable scope leaves `scope_kind` unset.
pub fn build_preflight_refresh_request(
    touched: &TouchedScope,
    branch: Option<&str>,
) -> GraphRefreshParams {
    let scope_kind = match touched.detection {
        Some(TouchedDetectionMethod::DiffContext) => Some(RefreshScopeKind::Diff),
        Some(TouchedDetectionMethod::ExplicitTargets) => {
            if !touched.symbols.is_empty() {
                Some(RefreshScopeKind::SymbolSet)
            } else {
                Some(RefreshScopeKind::FileSet)
            }
        }
        Some(TouchedDetectionMethod::ConfirmedUntouched)
        | Some(TouchedDetectionMethod::Unavailable)
        | None => None,
    };

    GraphRefreshParams {
        workspace_id: None,
        ref_name: branch.map(str::to_string),
        commit_sha: None,
        base_ref: None,
        base_sha: None,
        head_ref: branch.map(str::to_string),
        head_sha: None,
        scope_kind,
        files: touched.files.clone(),
        symbols: touched.symbols.clone(),
        allow_index: AllowIndex::Bounded,
        trigger: RefreshTrigger::Preflight,
    }
}

// ── Bounded refresh/check ───────────────────────────────────────────────────

/// Result of a bounded preflight graph readiness/refresh check.
#[derive(Debug, Clone)]
pub struct BoundedRefresh {
    /// Outcome reported back into the freshness derivation.
    pub outcome: RefreshOutcome,
    /// Resolved graph readiness for the workspace.
    pub graph_readiness: GraphReadiness,
    /// Snapshot identity the check resolved against, when available.
    pub graph_snapshot: Option<LocatorSnapshotInfo>,
    /// Scope identity (graph slice) the bounded check was scoped to.
    pub scope: Option<GraphSliceInfo>,
}

/// Run the bounded graph readiness/refresh check against the bot-owned
/// [`GraphSnapshotService`].
///
/// This is a *check*, not an unbounded rebuild: it resolves current readiness
/// and snapshot identity and maps them — together with the touched-scope budget
/// — to a non-blocking [`RefreshOutcome`]. It never appends events or mutates
/// governance state.
pub fn bounded_refresh_check(
    svc: &dyn GraphSnapshotService,
    refresh: &GraphRefreshParams,
    touched: &TouchedScope,
) -> BoundedRefresh {
    let view = svc.readiness_view();
    let readiness = view.readiness;

    let graph_snapshot = svc.resolve_head().ok().map(|s| LocatorSnapshotInfo {
        graph_snapshot_id: Some(s.graph_snapshot_id),
        repo_id: Some(s.repo_id),
        commit_sha: Some(s.commit_sha),
        index_version: Some(s.index_version),
        ref_name: s.ref_name,
    });

    let scope = scope_info(refresh, touched, readiness);
    let outcome = decide_bounded_outcome(readiness, touched);

    BoundedRefresh {
        outcome,
        graph_readiness: readiness,
        graph_snapshot,
        scope,
    }
}

/// Map readiness + touched-scope budget to a non-blocking [`RefreshOutcome`].
///
/// The budget guard runs first so an over-large scope is declined regardless of
/// readiness. A `Ready` graph is current for HEAD, so touched targets are
/// covered ([`RefreshOutcome::RefreshedWithinBudget`]) and an empty scope needs
/// no refresh ([`RefreshOutcome::NotRequested`]). Non-`Ready` readiness maps to
/// the queued/stale/unavailable outcomes that `derive` turns into caveats.
pub fn decide_bounded_outcome(readiness: GraphReadiness, touched: &TouchedScope) -> RefreshOutcome {
    if touched.target_count() > BOUNDED_SCOPE_BUDGET {
        return RefreshOutcome::OverBudget;
    }

    match readiness {
        GraphReadiness::Ready => {
            if touched.has_targets() {
                RefreshOutcome::RefreshedWithinBudget
            } else {
                RefreshOutcome::NotRequested
            }
        }
        GraphReadiness::Indexing => RefreshOutcome::Queued,
        GraphReadiness::Stale => RefreshOutcome::Stale,
        GraphReadiness::Failed | GraphReadiness::NotConfigured => RefreshOutcome::Unavailable,
    }
}

/// Build the scope identity ([`GraphSliceInfo`]) describing the bounded slice
/// the check was scoped to.
fn scope_info(
    refresh: &GraphRefreshParams,
    touched: &TouchedScope,
    readiness: GraphReadiness,
) -> Option<GraphSliceInfo> {
    let scope_kind = refresh.scope_kind?;

    let completeness = match readiness {
        GraphReadiness::Ready => match scope_kind {
            RefreshScopeKind::Diff => SliceCompleteness::DiffScoped,
            RefreshScopeKind::FileSet => SliceCompleteness::FileScoped,
            RefreshScopeKind::SymbolSet => SliceCompleteness::SymbolScoped,
            RefreshScopeKind::Query => SliceCompleteness::QueryScoped,
            RefreshScopeKind::BindingRevalidation => SliceCompleteness::RepairScoped,
        },
        _ => SliceCompleteness::NotAvailable,
    };

    Some(GraphSliceInfo {
        graph_slice_id: None,
        scope_kind: Some(scope_kind_label(scope_kind).to_string()),
        scope_fingerprint: Some(scope_fingerprint(touched)),
        completeness: Some(completeness),
    })
}

fn scope_kind_label(kind: RefreshScopeKind) -> &'static str {
    match kind {
        RefreshScopeKind::Diff => "diff",
        RefreshScopeKind::FileSet => "file_set",
        RefreshScopeKind::SymbolSet => "symbol_set",
        RefreshScopeKind::Query => "query",
        RefreshScopeKind::BindingRevalidation => "binding_revalidation",
    }
}

/// A deterministic, order-independent fingerprint of the touched targets.
fn scope_fingerprint(touched: &TouchedScope) -> String {
    let mut parts: Vec<String> = Vec::new();
    for f in &touched.files {
        parts.push(format!("f:{f}"));
    }
    for s in &touched.symbols {
        parts.push(format!("s:{s}"));
    }
    parts.sort();
    format!("{:016x}", fnv1a(&parts.join("\n")))
}

pub(crate) fn scope_fingerprint_for_watcher(touched: &TouchedScope) -> String {
    scope_fingerprint(touched)
}

fn fnv1a(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ── Top-level derivation ────────────────────────────────────────────────────

/// Compute the daemon-authored [`PreflightFreshness`] read-model for a
/// preflight request.
///
/// When no graph service is available (no workspace), freshness is reported as
/// [`RefreshOutcome::Unavailable`] / [`GraphReadiness::NotConfigured`] so the
/// caller still receives an honest, source-only read-model rather than a
/// fabricated all-clear. When a service is available, the touched scope drives
/// a bounded readiness/refresh check whose outcome feeds the pure derivation.
pub fn compute_preflight_freshness(
    query: &PreflightQuery,
    svc: Option<&dyn GraphSnapshotService>,
    result_was_empty: bool,
) -> PreflightFreshness {
    let touched_scope = derive_touched_scope(query);

    let inputs = match svc {
        Some(svc) => {
            let refresh = build_preflight_refresh_request(&touched_scope, query.branch.as_deref());
            let bounded = bounded_refresh_check(svc, &refresh, &touched_scope);
            FreshnessInputs {
                touched_scope,
                refresh_outcome: bounded.outcome,
                graph_readiness: bounded.graph_readiness,
                graph_snapshot: bounded.graph_snapshot,
                scope: bounded.scope,
                result_was_empty,
            }
        }
        None => FreshnessInputs {
            touched_scope,
            refresh_outcome: RefreshOutcome::Unavailable,
            graph_readiness: GraphReadiness::NotConfigured,
            graph_snapshot: None,
            scope: None,
            result_was_empty,
        },
    };

    PreflightFreshness::derive(inputs)
}

/// Compute preflight freshness, preferring a daemon-owned commit watcher
/// observation when available and falling back to caller-provided diff/target
/// context otherwise.
pub fn compute_preflight_freshness_with_watcher(
    query: &PreflightQuery,
    svc: Option<&dyn GraphSnapshotService>,
    result_was_empty: bool,
    watcher_observation: Option<&crate::commit_watcher::CommitWatcherObservation>,
) -> PreflightFreshness {
    crate::commit_watcher::compute_watcher_preflight_freshness(
        query,
        svc,
        result_was_empty,
        watcher_observation,
    )
    .unwrap_or_else(|| compute_preflight_freshness(query, svc, result_was_empty))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::evidence::{
        BindingEvidence, CodeSourceCurrentness, CodeSourceRefreshResult,
    };
    use bicameral_api::evidence::{
        FileInspectionResult, GraphReadinessView, GraphSnapshot, SnapshotResolution,
        SymbolResolutionResult,
    };
    use bicameral_api::freshness::{CommitFreshness, FreshnessCaveatCode, FreshnessNextAction};
    use uuid::Uuid;

    fn query(
        files: &[&str],
        symbols: &[&str],
        diff: Option<&str>,
        branch: Option<&str>,
    ) -> PreflightQuery {
        PreflightQuery {
            files: files.iter().map(|s| s.to_string()).collect(),
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
            diff_context: diff.map(str::to_string),
            branch: branch.map(str::to_string),
        }
    }

    /// A fake graph service that reports a fixed readiness — lets the bounded
    /// check be driven deterministically without a live git workspace.
    struct FakeGraphService {
        readiness: GraphReadiness,
        head: Option<GraphSnapshot>,
    }

    impl FakeGraphService {
        fn with_readiness(readiness: GraphReadiness) -> Self {
            Self {
                readiness,
                head: Some(GraphSnapshot {
                    graph_snapshot_id: "snap-head".into(),
                    repo_id: "repo-1".into(),
                    commit_sha: "headsha".into(),
                    index_version: "v1".into(),
                    ref_name: Some("HEAD".into()),
                    merge_base_sha: None,
                    indexed_at: None,
                    index_source: None,
                }),
            }
        }
    }

    impl GraphSnapshotService for FakeGraphService {
        fn resolve_snapshot(&self, _requested_ref: &str) -> SnapshotResolution {
            match &self.head {
                Some(s) => SnapshotResolution::Resolved {
                    snapshot: s.clone(),
                },
                None => SnapshotResolution::Failed {
                    requested_ref: _requested_ref.to_string(),
                    detail: "no head".into(),
                },
            }
        }
        fn resolve_head(
            &self,
        ) -> Result<GraphSnapshot, crate::graph_snapshot_service::GraphSnapshotServiceError>
        {
            self.head.clone().ok_or_else(|| {
                crate::graph_snapshot_service::GraphSnapshotServiceError::WorkspaceNotConfigured(
                    "no head".into(),
                )
            })
        }
        fn check_readiness(&self, _resolutions: &[&SnapshotResolution]) -> GraphReadiness {
            self.readiness
        }
        fn readiness_view(&self) -> GraphReadinessView {
            GraphReadinessView::from_readiness(self.readiness, None)
        }
        fn inspect_file(
            &self,
            _snapshot: &GraphSnapshot,
            _path: &str,
            _line_start: Option<u32>,
            _line_end: Option<u32>,
        ) -> FileInspectionResult {
            unreachable!("freshness check does not inspect files")
        }
        fn resolve_symbols(
            &self,
            _snapshot: &GraphSnapshot,
            _rel_path: &str,
        ) -> SymbolResolutionResult {
            unreachable!("freshness check does not resolve symbols")
        }
        fn evaluate_currentness(
            &self,
            _decision_id: Uuid,
            _binding: &BindingEvidence,
        ) -> CodeSourceRefreshResult {
            unreachable!("freshness check does not evaluate bindings")
        }
        fn evaluate_decision_currentness(
            &self,
            _decision_id: Uuid,
            _bindings: &[BindingEvidence],
        ) -> Vec<CodeSourceRefreshResult> {
            unreachable!("freshness check does not evaluate bindings")
        }
        fn currentness_for_readiness(&self, _readiness: GraphReadiness) -> CodeSourceCurrentness {
            CodeSourceCurrentness::GraphUnavailable
        }
    }

    // ── Touched-scope detection ──────────────────────────────────────────

    #[test]
    fn diff_context_yields_diff_derived_touched_files() {
        let diff = "diff --git a/src/foo.rs b/src/foo.rs\n\
                    --- a/src/foo.rs\n\
                    +++ b/src/foo.rs\n\
                    @@ -1 +1 @@\n\
                    -old\n+new\n\
                    diff --git a/src/bar.rs b/src/bar.rs\n\
                    +++ b/src/bar.rs\n";
        let scope = derive_touched_scope(&query(&[], &[], Some(diff), None));
        assert_eq!(scope.detection, Some(TouchedDetectionMethod::DiffContext));
        assert_eq!(scope.files, vec!["src/foo.rs", "src/bar.rs"]);
        assert!(scope.symbols.is_empty());
    }

    #[test]
    fn explicit_files_and_symbols_yield_explicit_touched_scope() {
        let scope = derive_touched_scope(&query(
            &["src/a.rs", "src/a.rs"],
            &["module::do_thing"],
            None,
            None,
        ));
        assert_eq!(
            scope.detection,
            Some(TouchedDetectionMethod::ExplicitTargets)
        );
        // de-duplicated, order preserved
        assert_eq!(scope.files, vec!["src/a.rs"]);
        assert_eq!(scope.symbols, vec!["module::do_thing"]);
    }

    #[test]
    fn no_inputs_yield_unavailable_touched_scope() {
        let scope = derive_touched_scope(&query(&[], &[], None, None));
        assert!(scope.is_unknown());
        assert!(!scope.has_targets());
    }

    #[test]
    fn diff_with_dev_null_marker_ignores_placeholder() {
        let diff = "diff --git a/new.rs b/new.rs\n--- /dev/null\n+++ b/new.rs\n";
        let scope = derive_touched_scope(&query(&[], &[], Some(diff), None));
        assert_eq!(scope.files, vec!["new.rs"]);
    }

    // ── Bounded refresh request ──────────────────────────────────────────

    #[test]
    fn refresh_request_uses_preflight_trigger_and_bounded_index() {
        let scope = derive_touched_scope(&query(&["src/a.rs"], &[], None, Some("feature")));
        let req = build_preflight_refresh_request(&scope, Some("feature"));
        assert_eq!(req.trigger, RefreshTrigger::Preflight);
        assert_eq!(req.allow_index, AllowIndex::Bounded);
        assert_eq!(req.scope_kind, Some(RefreshScopeKind::FileSet));
        assert_eq!(req.files, vec!["src/a.rs"]);
        assert_eq!(req.head_ref.as_deref(), Some("feature"));
    }

    #[test]
    fn refresh_request_scope_kind_follows_detection() {
        let diff_scope = derive_touched_scope(&query(&[], &[], Some("+++ b/src/x.rs\n"), None));
        assert_eq!(
            build_preflight_refresh_request(&diff_scope, None).scope_kind,
            Some(RefreshScopeKind::Diff)
        );

        let symbol_scope = derive_touched_scope(&query(&[], &["foo::bar"], None, None));
        assert_eq!(
            build_preflight_refresh_request(&symbol_scope, None).scope_kind,
            Some(RefreshScopeKind::SymbolSet)
        );

        let unknown = TouchedScope::unavailable();
        assert_eq!(
            build_preflight_refresh_request(&unknown, None).scope_kind,
            None
        );
    }

    // ── Bounded outcome mapping ──────────────────────────────────────────

    #[test]
    fn over_budget_scope_is_declined_regardless_of_readiness() {
        let files: Vec<String> = (0..(BOUNDED_SCOPE_BUDGET + 1))
            .map(|i| format!("f{i}.rs"))
            .collect();
        let scope = TouchedScope {
            detection: Some(TouchedDetectionMethod::ExplicitTargets),
            files,
            symbols: vec![],
        };
        assert_eq!(
            decide_bounded_outcome(GraphReadiness::Ready, &scope),
            RefreshOutcome::OverBudget
        );
    }

    #[test]
    fn ready_graph_with_targets_is_refreshed_within_budget() {
        let scope = derive_touched_scope(&query(&["src/a.rs"], &[], None, None));
        assert_eq!(
            decide_bounded_outcome(GraphReadiness::Ready, &scope),
            RefreshOutcome::RefreshedWithinBudget
        );
    }

    #[test]
    fn ready_graph_without_targets_is_not_requested() {
        let scope = TouchedScope::unavailable();
        assert_eq!(
            decide_bounded_outcome(GraphReadiness::Ready, &scope),
            RefreshOutcome::NotRequested
        );
    }

    #[test]
    fn indexing_graph_is_queued() {
        let scope = derive_touched_scope(&query(&["src/a.rs"], &[], None, None));
        assert_eq!(
            decide_bounded_outcome(GraphReadiness::Indexing, &scope),
            RefreshOutcome::Queued
        );
    }

    #[test]
    fn stale_graph_is_stale_outcome() {
        let scope = derive_touched_scope(&query(&["src/a.rs"], &[], None, None));
        assert_eq!(
            decide_bounded_outcome(GraphReadiness::Stale, &scope),
            RefreshOutcome::Stale
        );
    }

    // ── End-to-end freshness derivation ──────────────────────────────────

    #[test]
    fn queued_refresh_returns_unknown_stale() {
        let svc = FakeGraphService::with_readiness(GraphReadiness::Indexing);
        let f =
            compute_preflight_freshness(&query(&["src/a.rs"], &[], None, None), Some(&svc), false);
        assert_eq!(f.refresh_outcome, RefreshOutcome::Queued);
        assert_eq!(f.commit_freshness, CommitFreshness::UnknownStale);
        assert!(f
            .caveats
            .iter()
            .any(|c| c.code == FreshnessCaveatCode::RefreshQueued));
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::RetryWhenReady));
    }

    #[test]
    fn stale_graph_returns_caveats_and_next_actions() {
        let svc = FakeGraphService::with_readiness(GraphReadiness::Stale);
        let f =
            compute_preflight_freshness(&query(&["src/a.rs"], &[], None, None), Some(&svc), false);
        assert_eq!(f.commit_freshness, CommitFreshness::TouchedStale);
        assert!(!f.caveats.is_empty());
        assert!(!f.next_actions.is_empty());
        assert!(f
            .caveats
            .iter()
            .any(|c| c.code == FreshnessCaveatCode::GraphStale));
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::RequestTouchedRefresh));
    }

    #[test]
    fn unknown_touched_scope_becomes_unknown_stale() {
        // Stale graph + no diff/files/symbols → scope is unknown.
        let svc = FakeGraphService::with_readiness(GraphReadiness::Stale);
        let f = compute_preflight_freshness(&query(&[], &[], None, None), Some(&svc), false);
        assert!(f.touched_scope.is_unknown());
        assert_eq!(f.commit_freshness, CommitFreshness::UnknownStale);
        assert!(f
            .caveats
            .iter()
            .any(|c| c.code == FreshnessCaveatCode::TouchedScopeUnknown));
        assert!(f
            .next_actions
            .contains(&FreshnessNextAction::ProvideTouchedScope));
    }

    #[test]
    fn no_result_stale_graph_does_not_imply_all_clear() {
        let svc = FakeGraphService::with_readiness(GraphReadiness::Stale);
        let f =
            compute_preflight_freshness(&query(&["src/a.rs"], &[], None, None), Some(&svc), true);
        assert!(f
            .caveats
            .iter()
            .any(|c| c.code == FreshnessCaveatCode::EmptyResultStaleGraph));
    }

    #[test]
    fn diff_derived_touched_files_drive_ready_fresh() {
        let svc = FakeGraphService::with_readiness(GraphReadiness::Ready);
        let f = compute_preflight_freshness(
            &query(&[], &[], Some("+++ b/src/a.rs\n"), None),
            Some(&svc),
            false,
        );
        assert_eq!(
            f.touched_scope.detection,
            Some(TouchedDetectionMethod::DiffContext)
        );
        assert_eq!(f.touched_scope.files, vec!["src/a.rs"]);
        assert_eq!(f.commit_freshness, CommitFreshness::Fresh);
        assert!(f.caveats.is_empty());
    }

    #[test]
    fn no_workspace_reports_graph_unavailable_not_all_clear() {
        let f = compute_preflight_freshness(&query(&["src/a.rs"], &[], None, None), None, false);
        assert_eq!(f.refresh_outcome, RefreshOutcome::Unavailable);
        assert_eq!(f.commit_freshness, CommitFreshness::GraphUnavailable);
        assert_eq!(f.graph_readiness, GraphReadiness::NotConfigured);
        assert!(f
            .caveats
            .iter()
            .any(|c| c.code == FreshnessCaveatCode::RefreshUnavailable));
    }

    #[test]
    fn freshness_never_carries_authority_or_governance_fields() {
        let svc = FakeGraphService::with_readiness(GraphReadiness::Stale);
        let f =
            compute_preflight_freshness(&query(&["src/a.rs"], &[], None, None), Some(&svc), true);
        // Mirrors the phase-1 fixture conformance forbidden set. Note
        // "all-clear" is intentionally NOT forbidden: it appears only inside a
        // negation ("do not treat this empty result as a confirmed all-clear"),
        // which denies rather than asserts authority.
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
            "decision_state",
            "\"safe\"",
            "\"blocked\"",
        ] {
            assert!(
                !json.contains(forbidden),
                "freshness JSON must not contain authority/governance token `{forbidden}`: {json}"
            );
        }
    }
}
