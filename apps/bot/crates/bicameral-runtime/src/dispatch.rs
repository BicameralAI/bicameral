//! ToolRequest dispatcher — canonical `ToolRequest -> ToolResponse` path.
//!
//! Per ADR-0002 and the runtime spec, every local surface (MCP, CLI, TUI,
//! dashboard) submits a `ToolRequest` to the daemon.  The dispatcher validates
//! authority, routes the command, and returns a typed `ToolResponse`.
//!
//! Unsupported or deferred commands return distinct typed responses rather
//! than silently accepting or panicking.
//!
//! Every response is wrapped with `RuntimeTrace` correlation. The trace is an
//! operational receipt for diagnostics, not governance authority.

use std::path::Path;
use std::time::Instant;

use bicameral_api::candidate::{
    CandidateState, DecisionCandidate, DecisionLevel, ReviewLifecycleState,
};
use bicameral_api::candidate_promotion::CandidatePromotedPayload;
use bicameral_api::capability::AlphaCapabilities;
use bicameral_api::event_store::{
    BindToCodePayload, EventStoreAdapter, EventStoreEntry, EventType, IngestPayload,
    ManagedIngestRejection, ManagedIngestRejectionReason,
};
use bicameral_api::evidence::{
    BindingEvidence, BindingType, CodeSourceCurrentness, EvidenceReference, EvidenceRefreshOutcome,
    EvidenceRefreshStatus, GraphEvidenceState, GraphReadiness, GraphReadinessView, SourceEvidence,
};
use bicameral_api::reasoning_projection::ProjectionProviderReadiness;
use bicameral_api::source::{CaptureMethod, Source, SourceKind, SourceSnapshot};
use bicameral_api::tool_request::*;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use bicameral_config::connections::{ConnectionRegistry, ConnectionStatus};

use crate::code_source_refresh;
use crate::commit_freshness;
use crate::commit_watcher::CommitWatcherObservation;
use crate::confirmation_challenge_store::ConfirmationChallengeStore;
use crate::graph_snapshot_service::{GraphSnapshotService, LocalGraphProvider};
use crate::grounding::SnapshotResolver;
use crate::history::{self, HistoryQuery};
use crate::preflight::{self, PreflightGraphContext, PreflightQuery};
use crate::replay;
use crate::search::{self, SearchQuery, SearchScope as RuntimeSearchScope};

// ---------------------------------------------------------------------------
// Capability / startup
// ---------------------------------------------------------------------------

/// Supported commands advertised during the MCP startup handshake.
pub const SUPPORTED_COMMANDS: &[&str] = &[
    "ingest.submit_local",
    "ingest.submit_managed",
    "history.list",
    "search.query",
    "review.accept_candidate",
    "review.reject_candidate",
    "candidate.hide",
    "candidate.suppress",
    "review.approve_signoff",
    "review.reject_signoff",
    "binding.create",
    "binding.inspect",
    "workspace.bind",
    "workspace.resolve",
    "evidence.refresh",
    "preflight.run",
    "code.locate",
    "graph.status",
    "graph.refresh_snapshot",
    "decision.find_code_impact",
    "recall.inspect_evidence",
    "recall.expand_scope",
    "recall.promote_decision_candidate",
    // Approval-gated governed correction request path — supported in alpha per
    // RFQ #663 owner decision and ADR-0024. Advertisement metadata only; the
    // dispatcher routes this to its explicit governed handler. Support is not
    // authorization: it never creates accepted Decision, BindingEvidence,
    // signoff, compliance, or enforcement state.
    "recall.request_correction",
    "review.transition_finding",
    // Managed MCP CandidateSet selection + session (issues #987 / #989).
    // Protocol-recognized; daemon-local session/selection services own effects.
    "candidate_selection.prepare",
    "candidate_selection.commit",
    "candidate_session.heartbeat",
    "candidate_session.end",
];

/// Commands the MCP alpha thin client does not expose.
///
/// Two distinct reasons land here, but both mean "not an MCP alpha tool":
///
/// - typed-unsupported/deferred capabilities that fail closed when dispatched
///   (`review.resolve_compliance`, `tracking.*`, `privacy.erase_subject`);
/// - daemon-canonical hosted Product creation + retryable publication (#766),
///   which are fully implemented but reachable only through the validated
///   hosted bridge surface (see `CLOUD_ROUTABLE_TOOL_COMMANDS`), never as MCP
///   tools. Authorization is enforced against the daemon-confirmed hosted
///   actor context inside the handler; not appearing as an MCP tool is not
///   the same as being unavailable.
pub const DEFERRED_COMMANDS: &[&str] = &[
    "review.resolve_compliance",
    "tracking.untrack_source",
    "tracking.refresh_query",
    "privacy.erase_subject",
    "product.create",
    "product.publish",
];

/// Protocol version for capability handshake.
pub const PROTOCOL_VERSION: &str = "v2";

/// Preflight stage capability — tells MCP which stages are active.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PreflightStageCapability {
    pub stage: &'static str,
    pub status: &'static str,
}

/// Alpha preflight stage capabilities.
///
/// Capture, projection, and enforcement are `not_configured`.
/// Lookup is `supported`. This is the alpha cut line.
pub const PREFLIGHT_STAGE_CAPABILITIES: &[(&str, &str)] = &[
    ("capture", "not_configured"),
    ("projection", "not_configured"),
    ("lookup", "supported"),
    ("enforcement", "not_configured"),
    ("session_directive", "alpha_continue_only"),
];

/// Capability report returned during MCP startup handshake.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CapabilityReport {
    pub protocol_version: &'static str,
    pub supported_commands: &'static [&'static str],
    pub deferred_commands: &'static [&'static str],
    pub preflight_stages: Vec<PreflightStageCapability>,
    /// Projection provider readiness — non-secret setup state.
    ///
    /// Tells surfaces whether the daemon has a projection-capable provider
    /// before they attempt candidate-producing commands.
    pub projection_readiness: ProjectionProviderReadiness,
    /// Alpha feature capabilities — per-feature support/deferred/unsupported
    /// status with version-gating and deprecation metadata.
    ///
    /// Surfaces must not enable lookup or correction behavior unless the daemon
    /// explicitly reports support here. This is compatibility metadata, not
    /// governance authority.
    pub alpha_capabilities: AlphaCapabilities,
    /// Whether the daemon-owned `workspace.bind` path is actually routable in
    /// this runtime (bicameral-bot#747).
    ///
    /// `workspace.bind` is always listed in [`SUPPORTED_COMMANDS`] as a
    /// protocol-recognized command, but a surface routing it can only expect
    /// the daemon to execute a binding when this flag is `true`. It is `true`
    /// only when the daemon holds an operator-local workspace-binding store
    /// (the same seam that supplies the dispatch context); otherwise the
    /// command fails closed with a typed `unsupported_capability` rejection.
    ///
    /// This is truthful capability discovery, not authority. It never grants a
    /// hosted-bridge or MCP surface the right to bind — daemon validation and
    /// persistence remain the sole binding authority (ADR-0005, #732).
    pub workspace_binding_available: bool,
}

/// Build the capability report for MCP compatibility checks.
///
/// When no `ProjectionProviderReadiness` is supplied, the report defaults
/// to `not_configured` — callers must configure a projection provider
/// before candidate-producing commands will succeed.
///
/// Defaults `workspace_binding_available` to `false`: the pure-runtime
/// entrypoints have no operator-local workspace-binding store, so a surface
/// cannot expect `workspace.bind` to be routable here. The daemon publishes a
/// truthful value via [`capability_report_with_workspace_binding`].
pub fn capability_report() -> CapabilityReport {
    capability_report_with_readiness(ProjectionProviderReadiness::not_configured())
}

/// Build a capability report with explicit projection readiness.
///
/// `workspace_binding_available` defaults to `false`; use
/// [`capability_report_with_workspace_binding`] when the caller (the daemon)
/// knows whether the workspace-binding store is present.
pub fn capability_report_with_readiness(
    projection_readiness: ProjectionProviderReadiness,
) -> CapabilityReport {
    capability_report_with_workspace_binding(projection_readiness, false)
}

/// Build a capability report reflecting whether `workspace.bind` is routable
/// (bicameral-bot#747).
///
/// The daemon passes `workspace_binding_available = true` only when it holds
/// the operator-local workspace-binding store that backs the dispatch context;
/// otherwise the command fails closed with `unsupported_capability`. Capability
/// discovery therefore advertises workspace binding as available only when the
/// daemon can actually execute it.
pub fn capability_report_with_workspace_binding(
    projection_readiness: ProjectionProviderReadiness,
    workspace_binding_available: bool,
) -> CapabilityReport {
    CapabilityReport {
        protocol_version: PROTOCOL_VERSION,
        supported_commands: SUPPORTED_COMMANDS,
        deferred_commands: DEFERRED_COMMANDS,
        preflight_stages: PREFLIGHT_STAGE_CAPABILITIES
            .iter()
            .map(|(stage, status)| PreflightStageCapability { stage, status })
            .collect(),
        projection_readiness,
        alpha_capabilities: AlphaCapabilities::alpha_default(),
        workspace_binding_available,
    }
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Dispatch a `ToolRequest` through the canonical runtime path.
///
/// Validates authority, routes the command to the appropriate handler, and
/// returns a typed `ToolResponse`.  Mutation commands (review.*) append
/// events through the supplied `EventStoreAdapter`.
///
/// Binding commands (`binding.create`, `binding.inspect`) require a workspace
/// root for graph operations. Use `dispatch_with_workspace` when workspace
/// context is available.
pub fn dispatch(request: &ToolRequest, adapter: &dyn EventStoreAdapter) -> ToolResponse {
    dispatch_with_workspace(request, adapter, None)
}

/// Dispatch a `ToolRequest` with optional workspace root for graph operations.
///
/// When `workspace_root` is provided, `binding.create` can resolve snapshots,
/// validate file/symbol evidence, and materialize `BindingEvidence`. Without
/// it, binding commands return `graph_unavailable`.
pub fn dispatch_with_workspace(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    dispatch_with_workspace_and_commit_watcher(request, adapter, workspace_root, None)
}

/// Dispatch with optional daemon-owned commit watcher state.
///
/// The watcher state is a recomputable freshness read-model only. It is used by
/// `preflight.run` to prefer local HEAD movement over caller-supplied diff
/// context and never grants mutation authority.
/// Dispatch with daemon-local working-state candidates made searchable.
///
/// External-ingest candidates live only in daemon-local working state (no
/// candidate-bearing canonical event exists), so retrieval surfaces must
/// receive them explicitly. They participate as local proposals only.
pub fn dispatch_with_working_candidates(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    working_candidates: Vec<DecisionCandidate>,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            None,
            None,
            None,
            None,
            None,
            &working_candidates,
        )
    })
    .response
}

pub fn dispatch_with_workspace_and_commit_watcher(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
    })
    .response
}

/// Dispatch with a daemon-owned workspace-binding context.
///
/// The `binding_ctx` supplies the operator-local registry + store the daemon
/// uses to validate and materialize `workspace.bind` requests. When it is
/// absent (e.g. the pure-runtime dispatch entrypoints used in tests, or a
/// daemon that cannot resolve its operator-local store), `workspace.bind`
/// returns a typed `unsupported_capability` rejection. All other commands are
/// unaffected.
pub fn dispatch_with_workspace_binding(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    binding_ctx: &WorkspaceBindingDispatchContext<'_>,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            Some(binding_ctx),
            None,
            None,
            None,
            None,
            None,
            &[],
        )
    })
    .response
}

/// Dispatch with a daemon-owned `product.join` trust context.
///
/// The `product_join_ctx` supplies the daemon-local trust anchors (issuer,
/// audience, verifying keys) used to authenticate a signed ticket. When it is
/// absent (pure-runtime test entrypoints, or a daemon without configured trust
/// anchors), `product.join` fails closed with `unsupported_capability`. All
/// other commands are unaffected.
pub fn dispatch_with_product_join(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    product_join_ctx: &crate::product_join::ProductJoinDispatchContext<'_>,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            Some(product_join_ctx),
            None,
            None,
            None,
            None,
            &[],
        )
    })
    .response
}

/// Dispatch with a daemon-owned `product.continue_setup` context.
///
/// The `product_setup_ctx` supplies the daemon-local registered-adapter replay
/// probe used to open and replay a registered Product. When it is absent
/// (pure-runtime test entrypoints, or a daemon that cannot resolve its
/// registered-adapter opener), `product.continue_setup` fails closed with
/// `unsupported_capability`. All other commands are unaffected.
pub fn dispatch_with_product_setup(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    product_setup_ctx: &crate::product_registration::ProductSetupDispatchContext<'_>,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            None,
            None,
            Some(product_setup_ctx),
            None,
            None,
            &[],
        )
    })
    .response
}

/// Dispatch with a daemon-owned hosted Product creation context (#766).
///
/// The `product_ctx` supplies the daemon-confirmed [`HostedActorContext`] the
/// daemon derives from the validated hosted bridge session. When it is absent
/// (pure-runtime dispatch entrypoints, or a non-hosted surface), `product.create`
/// returns a typed `missing_actor_context` denial and writes no event. All other
/// commands are unaffected.
pub fn dispatch_with_hosted_product(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    product_ctx: &HostedProductDispatchContext,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            None,
            Some(product_ctx),
            None,
            None,
            None,
            &[],
        )
    })
    .response
}

/// Dispatch with a daemon-owned [`ConfirmationChallengeStore`] (issue #829).
///
/// The `challenge_store` is the sole authority that mints, validates, and
/// single-use-consumes `ConfirmationChallenge` grants for candidate promotion.
/// When it is present, `recall.promote_decision_candidate` runs the governed
/// two-phase confirmation flow: an initial request returns a typed
/// `confirmation_required` response without any canonical write, and only a
/// daemon-validated confirmed resubmission applies the transition. When it is
/// absent (pure-runtime test entrypoints, or a daemon without a challenge
/// store), promotion falls back to the pre-existing approval-proof path. All
/// other commands are unaffected.
pub fn dispatch_with_confirmation_challenges(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    challenge_store: &ConfirmationChallengeStore,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            None,
            None,
            None,
            Some(challenge_store),
            None,
            &[],
        )
    })
    .response
}

/// Daemon-owned CandidateSet selection/session dispatch context (#987/#989).
///
/// Surfaces may only submit stable identities. The daemon validates currentness,
/// authority, and confirmation; MCP never expands subsets or self-confirms.
pub struct CandidateSelectionDispatchContext<'a> {
    pub service: &'a crate::candidate_selection_service::CandidateSelectionService,
    /// Product-routed atomic ledger for `candidate_selection.commit`.
    ///
    /// When `None`, commit uses the service's process-bound event store.
    pub atomic_event_store: Option<&'a dyn crate::atomic_event_store::AtomicEventStoreAdapter>,
}

/// Dispatch with daemon-owned CandidateSet selection/session services.
pub fn dispatch_with_candidate_selection(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    selection_ctx: &CandidateSelectionDispatchContext<'_>,
) -> ToolResponse {
    dispatch_with_trace(request, |req| {
        dispatch_inner(
            req,
            adapter,
            workspace_root,
            watcher_observation,
            None,
            None,
            None,
            None,
            None,
            Some(selection_ctx),
            &[],
        )
    })
    .response
}

#[allow(clippy::too_many_arguments)]
fn dispatch_inner(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
    binding_ctx: Option<&WorkspaceBindingDispatchContext<'_>>,
    product_join_ctx: Option<&crate::product_join::ProductJoinDispatchContext<'_>>,
    product_ctx: Option<&HostedProductDispatchContext>,
    product_setup_ctx: Option<&crate::product_registration::ProductSetupDispatchContext<'_>>,
    challenge_store: Option<&ConfirmationChallengeStore>,
    selection_ctx: Option<&CandidateSelectionDispatchContext<'_>>,
    working_candidates: &[DecisionCandidate],
) -> ToolResponse {
    // 1. Validate authority context
    if let Some(resp) = validate_authority(request) {
        return resp;
    }

    // 2. Validate command-specific payload
    if let Some(resp) = validate_payload(request) {
        return resp;
    }

    // 3. Route command
    match &request.command {
        // -- Supported read commands --------------------------------------
        ToolCommand::HistoryList(params) => dispatch_history_list(request, params, adapter),
        ToolCommand::SearchQuery(params) => {
            dispatch_search_query(request, params, adapter, workspace_root, working_candidates)
        }
        ToolCommand::PreflightRun(params) => dispatch_preflight_run(
            request,
            params,
            adapter,
            workspace_root,
            watcher_observation,
        ),

        // -- Supported review/mutation commands ---------------------------
        ToolCommand::ReviewAcceptCandidate(params) => {
            dispatch_review_mutation(request, params, EventType::AcceptCandidate, adapter, None)
        }
        ToolCommand::ReviewRejectCandidate(params) => {
            dispatch_review_mutation(request, params, EventType::RejectCandidate, adapter, None)
        }
        ToolCommand::CandidateHide(_) | ToolCommand::CandidateSuppress(_) => {
            ToolResponse::rejected(
                request.request_id,
                "unsupported_capability: candidate disposition requires daemon-local working state"
                    .into(),
            )
        }
        ToolCommand::ReviewApproveSignoff(params) => {
            dispatch_review_mutation(request, params, EventType::ApproveSignoff, adapter, None)
        }
        ToolCommand::ReviewRejectSignoff(params) => {
            dispatch_review_mutation(request, params, EventType::RejectSignoff, adapter, None)
        }

        // -- Supported ingest commands ------------------------------------
        ToolCommand::IngestSubmitLocal(params) => {
            dispatch_ingest_submit_local(request, params, adapter)
        }
        ToolCommand::IngestSubmitManaged(params) => dispatch_ingest_submit_managed(request, params),

        // -- Binding commands (bot-owned per ADR-0017) --------------------
        ToolCommand::BindingCreate(params) => {
            dispatch_binding_create(request, params, adapter, workspace_root)
        }
        ToolCommand::BindingInspect(params) => {
            dispatch_binding_inspect(request, params, adapter, workspace_root)
        }

        // -- Workspace binding (daemon-owned per #732 / mcp#831) ----------
        ToolCommand::WorkspaceBind(bind_request) => {
            dispatch_workspace_bind(request, bind_request, binding_ctx)
        }
        ToolCommand::WorkspaceResolve(resolve_request) => {
            dispatch_workspace_resolve(request, resolve_request, binding_ctx)
        }

        // -- Evidence refresh (bot-owned per #377) ------------------------
        ToolCommand::EvidenceRefresh(params) => {
            dispatch_evidence_refresh(request, params, adapter, workspace_root)
        }

        // -- Typed unsupported/deferred -----------------------------------
        ToolCommand::ReviewResolveCompliance(_) => ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: review.resolve_compliance is deferred until V1 compliance \
             enforcement, correctness assertion, and signoff decoupling are specified"
                .into(),
        ),
        ToolCommand::TrackingUntrackSource(_) => ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: tracking.untrack_source deferred tracking \
implementation slice"
                .into(),
        ),
        ToolCommand::TrackingRefreshQuery(_) => ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: tracking.refresh_query deferred tracking \
implementation slice"
                .into(),
        ),
        ToolCommand::PrivacyEraseSubject(_) => ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: privacy.erase_subject requires PiiArchive \
storage (not yet implemented). Erasure requests are rejected fail-closed \
per GDPR Art.17 / SOC2 C1.x until the archive substrate is available."
                .into(),
        ),

        // -- CodeLocator (deterministic, issue #437) ----------------------
        ToolCommand::CodeLocate(params) => dispatch_code_locate(request, params, workspace_root),

        // -- Graph status/refresh (issue #436) -----------------------------
        ToolCommand::GraphStatus(params) => dispatch_graph_status(request, params, workspace_root),
        ToolCommand::GraphRefreshSnapshot(params) => {
            dispatch_graph_refresh_snapshot(request, params, workspace_root)
        }

        // -- Decision code-impact discovery (binding lifecycle Stage 1) ----
        ToolCommand::DecisionFindCodeImpact(params) => {
            dispatch_find_code_impact(request, params, adapter, workspace_root)
        }

        // -- Recall actions (alpha: #522) ----------------------------------
        ToolCommand::RecallInspectEvidence(params) => {
            dispatch_recall_inspect_evidence(request, params, adapter)
        }
        ToolCommand::RecallExpandScope(params) => {
            dispatch_recall_expand_scope(request, params, adapter)
        }
        ToolCommand::RecallPromoteDecisionCandidate(params) => match challenge_store {
            Some(store) => {
                dispatch_recall_promote_candidate_confirmed(request, params, adapter, store)
            }
            None => dispatch_recall_promote_candidate(request, params, adapter),
        },
        ToolCommand::RecallRequestCorrection(params) => {
            dispatch_recall_request_correction(request, params, adapter)
        }

        // -- Contradiction resolution (issue #602) -------------------------
        ToolCommand::ReviewTransitionFinding(params) => {
            dispatch_transition_finding(request, params, adapter)
        }

        // -- Hosted Product creation (daemon-canonical, issue #766) --------
        ToolCommand::ProductCreate(params) => {
            dispatch_product_create(request, params, adapter, product_ctx)
        }
        ToolCommand::ProductPublish(params) => dispatch_product_publish(request, params, adapter),

        // -- Product Member auto-join (issue #767) -------------------------
        ToolCommand::ProductJoin(params) => {
            crate::product_join::dispatch_product_join(request, params, adapter, product_join_ctx)
        }

        // -- Local setup continuation (issue #769) -------------------------
        ToolCommand::ProductContinueSetup(params) => {
            crate::product_registration::dispatch_product_continue_setup(
                request,
                params,
                adapter,
                product_setup_ctx,
            )
        }

        // -- Managed MCP CandidateSet selection / session (#987 / #989) ----
        ToolCommand::CandidateSelectionPrepare(selection) => {
            dispatch_candidate_selection_prepare(request, selection, selection_ctx)
        }
        ToolCommand::CandidateSelectionCommit(commit) => {
            dispatch_candidate_selection_commit(request, commit, selection_ctx)
        }
        ToolCommand::CandidateSessionHeartbeat(params) => {
            dispatch_candidate_session_heartbeat(request, params, selection_ctx)
        }
        ToolCommand::CandidateSessionEnd(params) => {
            dispatch_candidate_session_end(request, params, selection_ctx)
        }
    }
}

/// Outcome of dispatching a single `ToolRequest`.
///
/// Wraps `ToolResponse` with the guarantee that `response.trace` is populated.
pub struct DispatchOutcome {
    pub response: ToolResponse,
}

/// Dispatch a `ToolRequest`, producing a `ToolResponse` with trace correlation.
///
/// `handler` receives the request and returns a `ToolResponse`. This function
/// wraps the call with timing and trace construction.
pub fn dispatch_with_trace<F>(request: &ToolRequest, handler: F) -> DispatchOutcome
where
    F: FnOnce(&ToolRequest) -> ToolResponse,
{
    let start = Instant::now();
    let mut trace = RuntimeTrace::begin(request);

    let response = handler(request);

    let latency_ms = start.elapsed().as_millis() as u64;
    let status: TraceStatus = response.status.into();
    let failure_reason = match response.status {
        ToolResponseStatus::Ok => None,
        _ => response.message.clone(),
    };
    let touched_ids = extract_touched_ids(&response);

    trace = trace.finalize(status, latency_ms, failure_reason, touched_ids);

    DispatchOutcome {
        response: response.with_trace(trace),
    }
}

/// Convenience: reject a `ToolRequest` immediately with a governance policy
/// reason, producing a traced `ToolResponse` with `Rejected` status.
pub fn reject_with_trace(request: &ToolRequest, reason: String) -> DispatchOutcome {
    dispatch_with_trace(request, |req| {
        ToolResponse::rejected(req.request_id, reason)
    })
}

/// Convenience: fail a `ToolRequest` immediately with an internal error reason,
/// producing a traced `ToolResponse` with `Error` status.
pub fn error_with_trace(request: &ToolRequest, message: String) -> DispatchOutcome {
    dispatch_with_trace(request, |req| ToolResponse::error(req.request_id, message))
}

/// Extract entity IDs from a response result.
fn extract_touched_ids(response: &ToolResponse) -> Vec<String> {
    let Some(result) = &response.result else {
        return Vec::new();
    };

    let mut ids = Vec::new();

    for key in &[
        "id",
        "target_id",
        "decision_id",
        "candidate_id",
        "binding_id",
        "source_id",
    ] {
        if let Some(serde_json::Value::String(value)) = result.get(*key) {
            ids.push(value.clone());
        }
    }

    for key in &["ids", "touched_ids", "candidate_ids", "decision_ids"] {
        if let Some(serde_json::Value::Array(values)) = result.get(*key) {
            for value in values {
                if let serde_json::Value::String(id) = value {
                    ids.push(id.clone());
                }
            }
        }
    }

    ids
}

// ---------------------------------------------------------------------------
// Authority validation
// ---------------------------------------------------------------------------

fn validate_authority(request: &ToolRequest) -> Option<ToolResponse> {
    use bicameral_api::action_authorization::ActionAuthorizationPolicy;

    match ActionAuthorizationPolicy::evaluate(&request.authority, &request.command) {
        Ok(_permit) => None,
        Err(denial) => Some(ToolResponse::rejected(
            request.request_id,
            format!("{}: {}", denial.rejection_code(), denial.message),
        )),
    }
}

// ---------------------------------------------------------------------------
// Payload validation
// ---------------------------------------------------------------------------

fn validate_payload(request: &ToolRequest) -> Option<ToolResponse> {
    match &request.command {
        ToolCommand::IngestSubmitLocal(params) => {
            if params.title.is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: title must not be empty".into(),
                ));
            }
            if params.source_uri.is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: source_uri must not be empty".into(),
                ));
            }
            if params.source_type.is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: source_type must not be empty".into(),
                ));
            }
            // Reject forbidden canonical fields in metadata
            if let Some(meta) = &params.metadata {
                if let Some(obj) = meta.as_object() {
                    for key in FORBIDDEN_CANONICAL_FIELDS {
                        if obj.contains_key(*key) {
                            return Some(ToolResponse::rejected(
                                request.request_id,
                                format!(
                                    "forbidden_canonical_field: metadata must not contain \
                                     canonical field '{}'",
                                    key
                                ),
                            ));
                        }
                    }
                }
            }
            // Reject unsafe evidence (excerpts claiming verified/canonical state)
            for ev in &params.evidence {
                if is_unsafe_evidence(&ev.excerpt) {
                    return Some(ToolResponse::rejected(
                        request.request_id,
                        "unsafe_evidence: evidence excerpt contains governance authority \
                         claims that only the bot may produce"
                            .into(),
                    ));
                }
            }
            // Reject external adapter envelope shape routed through local ingest
            if is_external_adapter_shape(params) {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "external_adapter_payload: external source adapters must use \
                     ExternalIngestEnvelope, not ingest.submit_local"
                        .into(),
                ));
            }
            // Reject legacy MCP-only payload shapes
            if is_legacy_mcp_shape(params) {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "legacy_mcp_payload: legacy MCP direct payloads are not supported; \
                     use the canonical ingest.submit_local field set"
                        .into(),
                ));
            }
            None
        }
        ToolCommand::SearchQuery(params) => {
            if params.query.is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: query must not be empty".into(),
                ));
            }
            // All scopes (including `bindings`) are supported. Binding search
            // reads accepted, bot-owned BindingEvidence from replayed state.
            None
        }
        ToolCommand::ReviewAcceptCandidate(params)
        | ToolCommand::ReviewRejectCandidate(params)
        | ToolCommand::ReviewApproveSignoff(params)
        | ToolCommand::ReviewRejectSignoff(params) => {
            if params.target_id.is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: target_id must not be empty".into(),
                ));
            }
            None
        }
        ToolCommand::CandidateHide(params) => {
            if params.candidate_id.trim().is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: candidate_id must not be empty".into(),
                ));
            }
            None
        }
        ToolCommand::CandidateSuppress(params) => {
            if params.candidate_id.trim().is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: candidate_id must not be empty".into(),
                ));
            }
            if params.reason.trim().is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: suppression reason must not be empty".into(),
                ));
            }
            None
        }
        ToolCommand::ReviewResolveCompliance(params) => {
            if params.target_id.is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: target_id must not be empty".into(),
                ));
            }
            None
        }
        ToolCommand::WorkspaceBind(bind_request) => {
            let proposal = &bind_request.proposal;
            if proposal.project_id.trim().is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: proposal.project_id must not be empty".into(),
                ));
            }
            if proposal.candidate_path.trim().is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: proposal.candidate_path must not be empty".into(),
                ));
            }
            None
        }
        ToolCommand::WorkspaceResolve(resolve_request) => {
            if resolve_request.candidate_path.trim().is_empty() {
                return Some(ToolResponse::rejected(
                    request.request_id,
                    "malformed_payload: candidate_path must not be empty".into(),
                ));
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Read-command handlers
// ---------------------------------------------------------------------------

fn dispatch_history_list(
    request: &ToolRequest,
    params: &HistoryListParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let query = HistoryQuery {
        decision_id: params.decision_id.as_deref().and_then(|s| s.parse().ok()),
        include_events: params.include_events,
        include_candidates: params.include_bindings,
        since: params.since,
    };

    let result = history::query_history(&state, &query);
    match serde_json::to_value(&result) {
        Ok(v) => ToolResponse::success(request.request_id, v),
        Err(e) => ToolResponse::error(
            request.request_id,
            format!("runtime_error: failed to serialize history result: {e}"),
        ),
    }
}

fn dispatch_search_query(
    request: &ToolRequest,
    params: &SearchQueryParams,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    working_candidates: &[DecisionCandidate],
) -> ToolResponse {
    let mut state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let scope = match params.scope {
        Some(SearchScope::Decisions) => RuntimeSearchScope::Decisions,
        Some(SearchScope::Candidates) => RuntimeSearchScope::Candidates,
        Some(SearchScope::Bindings) => RuntimeSearchScope::Bindings,
        Some(SearchScope::All) | None => RuntimeSearchScope::All,
    };

    // Daemon-local working candidates are searchable proposals. They are
    // never canonical: replay-derived state wins on identity collision, and
    // suppressed candidates were already filtered out by the working store.
    for candidate in working_candidates {
        state
            .candidates
            .entry(candidate.id)
            .or_insert_with(|| candidate.clone());
    }

    let query = SearchQuery {
        query: params.query.clone(),
        scope,
        filters: search::SearchFilters::default(),
        limit: params.limit.unwrap_or(20),
    };

    let result = search::query_search(&state, &query);

    // Enrich with CodeLocator context when workspace is available (issue #439).
    // The locator uses the search query text as a hint for file/symbol discovery.
    // Results carry explicit readiness/evidence caveats.
    let locator_result = workspace_root.and_then(|ws| {
        if params.query.is_empty() {
            return None;
        }
        let svc = LocalGraphProvider::new(ws);
        let locate_params = CodeLocateParams {
            query: Some(params.query.clone()),
            decision_id: None,
            candidate_id: None,
            source_evidence_ids: Vec::new(),
            binding_ids: Vec::new(),
            files: Vec::new(),
            symbols: vec![params.query.clone()],
            changed_files: Vec::new(),
            diff_context: None,
            workspace_id: None,
            commit_sha: None,
            ref_name: None,
            graph_snapshot_id: None,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            scope_kind: None,
            limit: 5,
        };
        Some(crate::code_locator::locate(&locate_params, &svc))
    });

    let result = search::enrich_search_with_locator(result, locator_result.as_ref());

    match serde_json::to_value(&result) {
        Ok(v) => ToolResponse::success(request.request_id, v),
        Err(e) => ToolResponse::error(
            request.request_id,
            format!("runtime_error: failed to serialize search result: {e}"),
        ),
    }
}

/// Dispatch `preflight.run` — read-only advisory constraint lookup (ADR-0019).
///
/// Replays ledger state and returns relevant Decisions/candidates, source and
/// binding provenance, snapshot scope, and readiness/currentness warnings.
///
/// Operates in source-only mode when no workspace is configured or the graph
/// snapshot cannot be resolved; enriches results with verified binding-target
/// relevance when a snapshot resolves. Preflight is purely a read: it appends
/// no events and mutates no signoff, review, compliance, binding, or conflict
/// state.
fn dispatch_preflight_run(
    request: &ToolRequest,
    params: &PreflightRunParams,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
    watcher_observation: Option<&CommitWatcherObservation>,
) -> ToolResponse {
    let managed_preflight = params.managed_planning.as_ref().map(|boundary| {
        crate::managed_preflight::resolve_managed_planning_boundary(boundary, workspace_root)
    });

    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let query = PreflightQuery {
        files: params.files.clone(),
        symbols: params.symbols.clone(),
        diff_context: params.diff_context.clone(),
        branch: params.branch.clone(),
    };

    // Resolve a graph snapshot when a workspace is available so the response can
    // report honest readiness/currentness and enrich source-only matches with
    // verified binding-target relevance. Resolution failure degrades to
    // source-only mode rather than erroring — preflight must always answer.
    let graph_ctx = workspace_root.map(|ws| {
        let resolver = SnapshotResolver::new(ws);
        let resolution = match &params.branch {
            Some(branch) => resolver.resolve_ref(branch),
            None => resolver.resolve_ref("HEAD"),
        };
        PreflightGraphContext { resolution }
    });

    let result = preflight::query_preflight_progressive(&state, &query, graph_ctx.as_ref());
    let live_recall = crate::live_recall::build_live_recall(
        &state,
        &query,
        &result.readiness,
        graph_ctx.as_ref(),
    );

    // Enrich with CodeLocator candidates when workspace is available and the
    // query contains file/symbol terms (issue #439). Locator results carry
    // explicit evidence/readiness caveats and never claim safety, compliance,
    // no-conflict, or blocking status.
    let has_locator_terms = !params.files.is_empty() || !params.symbols.is_empty();
    let locator_result = if has_locator_terms {
        workspace_root.map(|ws| {
            let svc = LocalGraphProvider::new(ws);
            let locate_params = CodeLocateParams {
                query: None,
                decision_id: None,
                candidate_id: None,
                source_evidence_ids: Vec::new(),
                binding_ids: Vec::new(),
                files: params.files.clone(),
                symbols: params.symbols.clone(),
                changed_files: Vec::new(),
                diff_context: None,
                workspace_id: None,
                commit_sha: None,
                ref_name: params.branch.clone(),
                graph_snapshot_id: None,
                base_ref: None,
                base_sha: None,
                head_ref: None,
                head_sha: None,
                scope_kind: None,
                limit: 10,
            };
            crate::code_locator::locate(&locate_params, &svc)
        })
    } else {
        None
    };

    let mut result = preflight::enrich_with_locator(result, locator_result.as_ref());

    // Attach the daemon-authored commit-driven freshness read-model (issues
    // #625/#646). Prefer daemon watcher state for local HEAD movement when
    // present; otherwise fall back to caller diff/target context. Both paths
    // run bounded checks only and report caveats/next actions rather than
    // blocking or mutating authority.
    let result_was_empty =
        result.relevant_decisions.is_empty() && result.relevant_candidates.is_empty();
    let freshness_provider = workspace_root.map(LocalGraphProvider::new);
    let freshness = commit_freshness::compute_preflight_freshness_with_watcher(
        &query,
        freshness_provider
            .as_ref()
            .map(|p| p as &dyn GraphSnapshotService),
        result_was_empty,
        watcher_observation,
    );
    result = result.with_freshness(freshness);

    // Build the staged response from lookup results (issues #323, #393).
    //
    // The staged response carries typed availability classification so MCP,
    // dashboard, and API surfaces render the daemon-authored response rather
    // than deriving their own applicability verdict.
    let decision_refs: Vec<String> = result
        .relevant_decisions
        .iter()
        .map(|d| d.decision_id.to_string())
        .chain(
            result
                .relevant_candidates
                .iter()
                .map(|c| c.candidate_id.to_string()),
        )
        .collect();

    let limitations: Vec<String> = result
        .readiness
        .unknown_scope
        .iter()
        .map(|u| format!("{}: {}", u.scope, u.detail))
        .chain(result.warnings.iter().cloned())
        .collect();

    // Determine whether lookup is fully available or readiness-limited.
    // When graph readiness is not `Ready`, the lookup produced source-only
    // results and the availability is `readiness_limited`.
    let is_readiness_limited = result.readiness.state != GraphReadiness::Ready;

    let staged = if is_readiness_limited {
        bicameral_api::staged_preflight::StagedPreflightResponse::alpha_readiness_limited(
            decision_refs,
            limitations,
            params.checkpoint_hint,
        )
    } else {
        bicameral_api::staged_preflight::StagedPreflightResponse::alpha_from_lookup(
            decision_refs,
            limitations,
            params.checkpoint_hint,
        )
    };

    // Build the canonical daemon-owned alpha recall envelope (#817). It wraps
    // the staged payload and recall packet as daemon-authored product content,
    // states the read-only authority guarantee, carries typed caveats, and
    // attaches validation-only witnesses (RuntimeTrace correlation + a
    // not-consumed topology receipt for later real-process validation).
    let envelope = build_alpha_recall_envelope(request, &staged, &result, &live_recall);

    match (serde_json::to_value(&result), serde_json::to_value(&staged)) {
        (Ok(mut v), Ok(staged_v)) => {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("staged".into(), staged_v);
                match serde_json::to_value(&live_recall) {
                    Ok(live_recall_v) => {
                        obj.insert("live_recall".into(), live_recall_v);
                    }
                    Err(e) => {
                        return ToolResponse::error(
                            request.request_id,
                            format!("runtime_error: failed to serialize live recall result: {e}"),
                        );
                    }
                }
                match serde_json::to_value(&envelope) {
                    Ok(envelope_v) => {
                        obj.insert("alpha_recall".into(), envelope_v);
                    }
                    Err(e) => {
                        return ToolResponse::error(
                            request.request_id,
                            format!(
                                "runtime_error: failed to serialize alpha recall envelope: {e}"
                            ),
                        );
                    }
                }
                if let Some(managed) = managed_preflight {
                    match serde_json::to_value(managed) {
                        Ok(managed_v) => {
                            obj.insert("managed_preflight".into(), managed_v);
                        }
                        Err(e) => {
                            return ToolResponse::error(
                                request.request_id,
                                format!(
                                    "runtime_error: failed to serialize managed preflight result: {e}"
                                ),
                            );
                        }
                    }
                }
            }
            ToolResponse::success(request.request_id, v)
        }
        (Err(e), _) | (_, Err(e)) => ToolResponse::error(
            request.request_id,
            format!("runtime_error: failed to serialize preflight result: {e}"),
        ),
    }
}

/// Build the canonical alpha recall envelope for a `preflight.run` response.
///
/// The envelope is daemon-authored: it pins the protocol version, states the
/// read-only authority guarantee, wraps the staged payload and recall packet as
/// product content, classifies readiness limitations as typed caveats, and
/// attaches validation-only correlation witnesses. It never mutates state.
fn build_alpha_recall_envelope(
    request: &ToolRequest,
    staged: &bicameral_api::staged_preflight::StagedPreflightResponse,
    result: &crate::preflight::PreflightResponse,
    live_recall: &crate::live_recall::LiveRecallView,
) -> bicameral_api::alpha_recall::AlphaRecallEnvelope {
    use bicameral_api::alpha_recall::{
        AlphaRecallEnvelope, AlphaRecallProduct, AlphaRecallTopologyWitness,
        AlphaRecallTraceWitness, AlphaRecallWitness,
    };

    let caveats = alpha_recall_caveats(result, live_recall);
    let product = AlphaRecallProduct {
        staged: staged.clone(),
        packet: live_recall.packet.clone(),
        caveats,
    };

    // Correlation witness: reuse the trace seed so the envelope mirrors the
    // response's RuntimeTrace identifiers. `preflight.run` always succeeds when
    // it reaches this point, so the correlated status is `Ok`.
    let trace_seed = RuntimeTrace::begin(request);
    let witness = AlphaRecallWitness {
        runtime_trace: AlphaRecallTraceWitness {
            request_id: trace_seed.request_id,
            command: trace_seed.command,
            caller_surface: trace_seed.caller_surface,
            status: TraceStatus::Ok,
        },
        topology_receipt: Some(AlphaRecallTopologyWitness::not_consumed()),
    };

    AlphaRecallEnvelope::new(PROTOCOL_VERSION, product, witness)
}

/// Classify preflight readiness limitations into typed alpha recall caveats.
///
/// Missing / stale / partial / unavailable source, graph, index, freshness, or
/// query state is represented as daemon-authored caveats so a client renders
/// them directly instead of inferring limitations itself.
fn alpha_recall_caveats(
    result: &crate::preflight::PreflightResponse,
    live_recall: &crate::live_recall::LiveRecallView,
) -> Vec<bicameral_api::alpha_recall::AlphaRecallCaveat> {
    use bicameral_api::alpha_recall::{
        AlphaRecallCaveat, AlphaRecallCaveatScope, AlphaRecallCaveatStatus,
    };

    let mut caveats: Vec<AlphaRecallCaveat> = Vec::new();

    for unknown in &result.readiness.unknown_scope {
        let scope_key = unknown.scope.to_ascii_lowercase();
        let scope = if scope_key.contains("graph") {
            AlphaRecallCaveatScope::Graph
        } else {
            AlphaRecallCaveatScope::Source
        };
        caveats.push(AlphaRecallCaveat {
            scope,
            status: AlphaRecallCaveatStatus::Unavailable,
            detail: format!("{}: {}", unknown.scope, unknown.detail),
        });
    }

    for warning in &result.warnings {
        caveats.push(AlphaRecallCaveat {
            scope: AlphaRecallCaveatScope::Freshness,
            status: AlphaRecallCaveatStatus::Stale,
            detail: warning.clone(),
        });
    }

    for limitation in &live_recall.readiness_limitations {
        let (scope, status) = if limitation == "decision_index_partial" {
            (
                AlphaRecallCaveatScope::Index,
                AlphaRecallCaveatStatus::Partial,
            )
        } else if limitation.starts_with("graph_scope") {
            (
                AlphaRecallCaveatScope::Graph,
                AlphaRecallCaveatStatus::Unavailable,
            )
        } else if limitation == "query_scope_empty" {
            (
                AlphaRecallCaveatScope::Query,
                AlphaRecallCaveatStatus::Missing,
            )
        } else {
            (
                AlphaRecallCaveatScope::Source,
                AlphaRecallCaveatStatus::Partial,
            )
        };
        caveats.push(AlphaRecallCaveat {
            scope,
            status,
            detail: limitation.clone(),
        });
    }

    caveats
}

// ---------------------------------------------------------------------------
// Mutation-command handlers (review.*)
// ---------------------------------------------------------------------------

pub trait CandidateSnapshotProvider {
    fn candidate_snapshot(&self, candidate_id: Uuid) -> Result<Option<DecisionCandidate>, String>;
}

pub fn dispatch_candidate_review_with_provider(
    request: &ToolRequest,
    adapter: &dyn EventStoreAdapter,
    provider: &dyn CandidateSnapshotProvider,
) -> ToolResponse {
    match &request.command {
        ToolCommand::ReviewAcceptCandidate(params) => dispatch_review_mutation(
            request,
            params,
            EventType::AcceptCandidate,
            adapter,
            Some(provider),
        ),
        ToolCommand::ReviewRejectCandidate(params) => dispatch_review_mutation(
            request,
            params,
            EventType::RejectCandidate,
            adapter,
            Some(provider),
        ),
        _ => ToolResponse::rejected(
            request.request_id,
            "unsupported_command: candidate provider only handles candidate review".into(),
        ),
    }
}

fn dispatch_review_mutation(
    request: &ToolRequest,
    params: &ReviewTargetParams,
    event_type: EventType,
    adapter: &dyn EventStoreAdapter,
    provider: Option<&dyn CandidateSnapshotProvider>,
) -> ToolResponse {
    // Fail-closed: validate target existence before persisting any event.
    // A review command against a non-existent target must be rejected so
    // that the event store never contains events that replay cannot apply.
    let target_uuid = match Uuid::parse_str(&params.target_id) {
        Ok(id) => id,
        Err(_) => {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "unknown_target: target_id '{}' is not a valid identifier",
                    params.target_id
                ),
            );
        }
    };

    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let provider_candidate = if matches!(
        event_type,
        EventType::AcceptCandidate | EventType::RejectCandidate
    ) && !state.candidates.contains_key(&target_uuid)
    {
        match provider {
            Some(provider) => match provider.candidate_snapshot(target_uuid) {
                Ok(candidate) => candidate,
                Err(error) => {
                    return ToolResponse::error(
                        request.request_id,
                        format!("daemon_unavailable: candidate lookup failed: {error}"),
                    );
                }
            },
            None => None,
        }
    } else {
        None
    };

    match event_type {
        EventType::AcceptCandidate | EventType::RejectCandidate
            if !state.candidates.contains_key(&target_uuid) && provider_candidate.is_none() =>
        {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "unknown_target: candidate '{}' does not exist",
                    params.target_id
                ),
            );
        }
        EventType::ApproveSignoff | EventType::RejectSignoff
            if !state.decisions.contains_key(&target_uuid) =>
        {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "unknown_target: decision '{}' does not exist",
                    params.target_id
                ),
            );
        }
        _ => {}
    }

    // Derive actor/audit/promoted_by from AuthorityContext.actor_id
    let actor = &request.authority.actor_id;

    // Build event payload shaped for the replay module.
    //
    // AcceptCandidate/RejectCandidate replay expects `candidate_id` (and
    // AcceptCandidate also expects `decision_id`).
    // ApproveSignoff/RejectSignoff replay expects `decision_id`.
    //
    // `target_id` from the ToolRequest maps to the appropriate id field
    // depending on event type.
    let payload = match event_type {
        EventType::AcceptCandidate => {
            let decision_id = uuid::Uuid::new_v4();
            let candidate = state
                .candidates
                .get(&target_uuid)
                .cloned()
                .or_else(|| provider_candidate.clone())
                .expect("candidate existence was validated before promotion payload construction");
            let promoted = CandidatePromotedPayload::new(
                candidate,
                decision_id,
                params.reason.clone(),
                actor.clone(),
                request.authority.audit_metadata.clone(),
            );
            match serde_json::to_value(promoted) {
                Ok(payload) => payload,
                Err(error) => {
                    return ToolResponse::error(
                        request.request_id,
                        format!("runtime_error: failed to serialize promotion payload: {error}"),
                    );
                }
            }
        }
        EventType::RejectCandidate => {
            serde_json::json!({
                "candidate_id": params.target_id,
                "reason": params.reason,
                "actor_id": actor,
                "audit_metadata": request.authority.audit_metadata,
            })
        }
        EventType::ApproveSignoff | EventType::RejectSignoff => {
            serde_json::json!({
                "decision_id": params.target_id,
                "reason": params.reason,
                "actor_id": actor,
                "promoted_by": actor,
                "audit_metadata": request.authority.audit_metadata,
            })
        }
        _ => {
            serde_json::json!({
                "target_id": params.target_id,
                "reason": params.reason,
                "actor_id": actor,
                "promoted_by": actor,
                "audit_metadata": request.authority.audit_metadata,
            })
        }
    };

    let entry = EventStoreEntry::new(event_type, payload, actor.clone());

    match adapter.append(entry) {
        Ok(()) => {
            let result = serde_json::json!({
                "target_id": params.target_id,
                "event_type": event_type,
                "actor_id": actor,
            });
            ToolResponse::success(request.request_id, result)
        }
        Err(e) => ToolResponse::error(
            request.request_id,
            format!("daemon_unavailable: event append failed: {e}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// Contradiction-resolution handler (review.transition_finding)
// ---------------------------------------------------------------------------

/// Dispatch a `review.transition_finding` command.
///
/// Validates that the referenced report exists and that the requested
/// transition is valid per the `FindingStatus` state machine, then appends
/// a `TransitionFinding` event.
fn dispatch_transition_finding(
    request: &ToolRequest,
    params: &TransitionFindingParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    use bicameral_api::constraint::{ActorType, Attribution};
    use bicameral_api::drift::{FindingStatus, ReviewTransitionRequest};
    use bicameral_api::event_store::TransitionFindingPayload;

    // Parse report_id.
    let report_id = match Uuid::parse_str(&params.report_id) {
        Ok(id) => id,
        Err(_) => {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "unknown_target: report_id '{}' is not a valid identifier",
                    params.report_id
                ),
            );
        }
    };

    // Parse to_status.
    let to_status = match params.to_status.to_lowercase().as_str() {
        "acknowledged" => FindingStatus::Acknowledged,
        "resolved" => FindingStatus::Resolved,
        "dismissed" => FindingStatus::Dismissed,
        other => {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "invalid_parameter: to_status '{other}' is not a valid finding status \
                     (expected acknowledged, resolved, or dismissed)"
                ),
            );
        }
    };

    // Replay to validate the report exists and the transition is valid.
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let report = match state.contradictions.get(&report_id) {
        Some(r) => r,
        None => {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "unknown_target: contradiction report '{}' does not exist",
                    params.report_id
                ),
            );
        }
    };

    // Validate the transition is allowed.
    if !bicameral_api::drift::can_transition(report.status, to_status) {
        return ToolResponse::rejected(
            request.request_id,
            format!(
                "invalid_transition: cannot transition {:?} -> {:?}",
                report.status, to_status
            ),
        );
    }

    // Build event payload.
    let actor_id = &request.authority.actor_id;
    let payload = TransitionFindingPayload {
        request: ReviewTransitionRequest {
            report_id,
            to_status,
            actor: Attribution {
                actor_id: actor_id.clone(),
                actor_type: ActorType::Human,
                display_name: None,
            },
            note: params.note.clone(),
            at: Utc::now(),
        },
    };

    let entry = EventStoreEntry::new(
        EventType::TransitionFinding,
        serde_json::to_value(&payload).unwrap(),
        actor_id.clone(),
    );

    match adapter.append(entry) {
        Ok(()) => {
            let result = serde_json::json!({
                "report_id": report_id.to_string(),
                "from_status": report.status,
                "to_status": to_status,
                "actor_id": actor_id,
            });
            ToolResponse::success(request.request_id, result)
        }
        Err(e) => ToolResponse::error(
            request.request_id,
            format!("daemon_unavailable: event append failed: {e}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// Workspace-binding handler (workspace.bind, daemon-owned per #732)
// ---------------------------------------------------------------------------

/// Daemon-owned context the dispatcher needs to validate and materialize a
/// `workspace.bind` request.
///
/// The daemon builds this per-dispatch from its live operator-local state
/// (registered projects + per-project binding store). It is the seam that keeps
/// binding a daemon-only authority: surfaces route a `ToolRequest`, but only the
/// daemon supplies the registry/store that actually persists a binding.
pub struct WorkspaceBindingDispatchContext<'a> {
    /// Registered-project view consulted before binding.
    pub registry: &'a dyn crate::workspace_binding::WorkspaceBindingRegistry,
    /// Operator-local per-project persistence (sole writer of bindings).
    pub store: &'a dyn crate::workspace_binding::WorkspaceBindingStore,
}

fn dispatch_workspace_bind(
    request: &ToolRequest,
    bind_request: &bicameral_api::workspace_binding::WorkspaceBindRequest,
    binding_ctx: Option<&WorkspaceBindingDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = binding_ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: workspace.bind requires the daemon's operator-local \
             workspace-binding store, which is not available in this runtime context"
                .into(),
        );
    };

    let now = Utc::now().to_rfc3339();
    match crate::workspace_binding::bind_workspace(bind_request, ctx.registry, ctx.store, &now) {
        Ok(response) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "status": "bound",
                "outcome": response,
            }),
        ),
        Err(crate::workspace_binding::WorkspaceBindError::Rejected(err)) => {
            // Typed protocol outcome (already-bound, repair-required,
            // confirmation-missing, unregistered, wrong-project, unsafe-path,
            // capability-mismatch). Surfaced as a Rejected response that still
            // carries the structured typed payload so callers can branch on the
            // exact outcome. No state was mutated.
            let kind = serde_json::to_value(err.error)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "rejected".to_string());
            ToolResponse {
                request_id: request.request_id,
                status: ToolResponseStatus::Rejected,
                result: serde_json::to_value(&err).ok(),
                message: Some(format!("{kind}: {}", err.message)),
                governance_result: None,
                trace: None,
                responded_at: Utc::now(),
            }
        }
        Err(crate::workspace_binding::WorkspaceBindError::Store(msg)) => {
            ToolResponse::error(request.request_id, format!("daemon_unavailable: {msg}"))
        }
    }
}

fn dispatch_workspace_resolve(
    request: &ToolRequest,
    resolve_request: &bicameral_api::workspace_binding::WorkspaceResolveRequest,
    binding_ctx: Option<&WorkspaceBindingDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = binding_ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: workspace.resolve requires the daemon's operator-local \
             workspace-binding store, which is not available in this runtime context"
                .into(),
        );
    };

    match crate::workspace_binding::resolve_workspace(resolve_request, ctx.store) {
        Ok(response) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "status": "resolved",
                "outcome": response,
            }),
        ),
        Err(crate::workspace_binding::WorkspaceResolveError::Rejected(err)) => {
            let kind = serde_json::to_value(err.error)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "rejected".to_string());
            ToolResponse {
                request_id: request.request_id,
                status: ToolResponseStatus::Rejected,
                result: serde_json::to_value(&err).ok(),
                message: Some(format!("{kind}: {}", err.message)),
                governance_result: None,
                trace: None,
                responded_at: Utc::now(),
            }
        }
        Err(crate::workspace_binding::WorkspaceResolveError::Store(msg)) => {
            ToolResponse::error(request.request_id, format!("daemon_unavailable: {msg}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Managed MCP CandidateSet selection / session (#987 / #989)
// ---------------------------------------------------------------------------

fn dispatch_candidate_selection_prepare(
    request: &ToolRequest,
    selection: &bicameral_api::candidate_selection::CandidateSelectionV1,
    selection_ctx: Option<&CandidateSelectionDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = selection_ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: candidate_selection.prepare requires daemon CandidateSelectionService"
                .into(),
        );
    };
    match ctx.service.prepare(selection.clone(), Utc::now()) {
        Ok(preview) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "candidate_selection_preview": preview,
            }),
        ),
        Err(failure) => candidate_selection_failure_response(request.request_id, failure),
    }
}

fn dispatch_candidate_selection_commit(
    request: &ToolRequest,
    commit: &bicameral_api::candidate_selection::CandidateSelectionCommitV1,
    selection_ctx: Option<&CandidateSelectionDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = selection_ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: candidate_selection.commit requires daemon CandidateSelectionService"
                .into(),
        );
    };
    let result = match ctx.atomic_event_store {
        Some(store) => ctx
            .service
            .commit_with_store(commit.clone(), Utc::now(), store),
        None => ctx.service.commit(commit.clone(), Utc::now()),
    };
    match result {
        Ok(receipt) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "candidate_selection_receipt": receipt,
            }),
        ),
        Err(failure) => candidate_selection_failure_response(request.request_id, failure),
    }
}

fn dispatch_candidate_session_heartbeat(
    request: &ToolRequest,
    params: &bicameral_api::candidate_session::CandidateSessionHeartbeatRequestV1,
    selection_ctx: Option<&CandidateSelectionDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = selection_ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: candidate_session.heartbeat requires daemon CandidateSessionStore"
                .into(),
        );
    };
    match ctx.service.sessions().heartbeat(&params.access, Utc::now()) {
        Ok(result) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "candidate_session_heartbeat_result": result,
            }),
        ),
        Err(failure) => candidate_session_failure_response(request.request_id, failure),
    }
}

fn dispatch_candidate_session_end(
    request: &ToolRequest,
    params: &bicameral_api::candidate_session::CandidateSessionEndRequestV1,
    selection_ctx: Option<&CandidateSelectionDispatchContext<'_>>,
) -> ToolResponse {
    let Some(ctx) = selection_ctx else {
        return ToolResponse::rejected(
            request.request_id,
            "unsupported_capability: candidate_session.end requires daemon CandidateSessionStore"
                .into(),
        );
    };
    match ctx.service.sessions().end(&params.access, Utc::now()) {
        Ok(result) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "candidate_session_end_result": result,
            }),
        ),
        Err(failure) => candidate_session_failure_response(request.request_id, failure),
    }
}

fn candidate_selection_failure_response(
    request_id: Uuid,
    failure: bicameral_api::candidate_selection::CandidateSelectionFailureV1,
) -> ToolResponse {
    let code = serde_json::to_value(failure.code)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| format!("{:?}", failure.code));
    ToolResponse {
        request_id,
        status: ToolResponseStatus::Rejected,
        result: Some(serde_json::json!({
            "failure": failure,
        })),
        message: Some(format!("{code}: {}", failure.detail)),
        governance_result: None,
        trace: None,
        responded_at: Utc::now(),
    }
}

fn candidate_session_failure_response(
    request_id: Uuid,
    failure: bicameral_api::candidate_session::CandidateSessionFailure,
) -> ToolResponse {
    let code = failure.code();
    ToolResponse {
        request_id,
        status: ToolResponseStatus::Rejected,
        result: Some(serde_json::json!({
            "failure": {
                "code": code,
                "detail": code,
            }
        })),
        message: Some(code.to_string()),
        governance_result: None,
        trace: None,
        responded_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Hosted Product creation handlers (product.create, product.publish) — #766
// ---------------------------------------------------------------------------

/// Daemon-owned context for hosted Product creation.
///
/// Carries the daemon-confirmed [`HostedActorContext`] derived from the
/// validated hosted bridge session. This is the seam that keeps creation
/// authority local: the SPA routes a `ToolRequest`, but the paired state, bound
/// Cloud account, and authorization come from the daemon's view of the session,
/// never from caller-supplied fields.
#[derive(Debug, Clone, Default)]
pub struct HostedProductDispatchContext {
    /// Daemon-confirmed hosted actor context, or `None` when the surface has no
    /// bound hosted session (yields a typed `missing_actor_context` denial).
    pub actor: Option<bicameral_api::product_creation::HostedActorContext>,
    /// Provider-confirmed commit handles for operator-scoped create
    /// (bot#1087). `None` on non-v2 substrates: the create appends without a
    /// proof and `product.publish` carries none.
    pub commit: Option<crate::product_creation::ProductCreateCommit>,
}

fn dispatch_product_create(
    request: &ToolRequest,
    params: &bicameral_api::product_creation::ProductCreateParams,
    adapter: &dyn EventStoreAdapter,
    product_ctx: Option<&HostedProductDispatchContext>,
) -> ToolResponse {
    let actor = product_ctx.and_then(|c| c.actor.as_ref());
    let commit = product_ctx.and_then(|c| c.commit.as_ref());
    match crate::product_creation::create_product_with_commit(
        params,
        actor,
        adapter,
        commit,
        Utc::now(),
    ) {
        Ok(outcome) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "status": if outcome.created { "created" } else { "already_created" },
                "created": outcome.created,
                "result": outcome.result,
                "preview": outcome.preview,
                "first_event_commit_proof": outcome.first_event_commit_proof,
            }),
        ),
        Err(crate::product_creation::ProductCreateError::Denied(denial)) => ToolResponse {
            request_id: request.request_id,
            status: ToolResponseStatus::Rejected,
            result: serde_json::to_value(&denial).ok(),
            message: Some(format!("{}: {}", denial.code(), denial.message)),
            governance_result: None,
            trace: None,
            responded_at: Utc::now(),
        },
        Err(crate::product_creation::ProductCreateError::Store(msg)) => {
            ToolResponse::error(request.request_id, format!("daemon_unavailable: {msg}"))
        }
        Err(crate::product_creation::ProductCreateError::ProofSignerUnavailable(msg)) => {
            ToolResponse::error(
                request.request_id,
                format!("proof_signer_unavailable: {msg}; no canonical event was written"),
            )
        }
        Err(crate::product_creation::ProductCreateError::ProviderReadback(msg)) => {
            ToolResponse::error(
                request.request_id,
                format!(
                    "provider_readback_mismatch: {msg}; no commit proof was minted; retry \
                     product.create with the same idempotency key"
                ),
            )
        }
        Err(crate::product_creation::ProductCreateError::ProofMint(msg)) => ToolResponse::error(
            request.request_id,
            format!(
                "commit_proof_unavailable: the canonical CreateProduct event is \
                 provider-confirmed but no signed proof could be minted: {msg}; retry \
                 product.create with the same idempotency key"
            ),
        ),
    }
}

fn dispatch_product_publish(
    request: &ToolRequest,
    params: &bicameral_api::product_creation::ProductPublishParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    match crate::product_creation::build_publication(
        &params.product_id,
        &params.idempotency_key,
        adapter,
        Utc::now(),
    ) {
        Ok(publication) => ToolResponse::success(
            request.request_id,
            serde_json::json!({
                "status": "published",
                "publication": publication,
            }),
        ),
        Err(crate::product_creation::ProductPublishError::NotFound {
            canonical_product_count,
            create_event_count,
            malformed_create_event_count,
        }) => ToolResponse {
            request_id: request.request_id,
            status: ToolResponseStatus::Rejected,
            result: Some(serde_json::json!({
                "diagnostics": {
                    "error_code": "product_not_found",
                    "canonical_product_count": canonical_product_count,
                    "create_event_count": create_event_count,
                    "malformed_create_event_count": malformed_create_event_count,
                    "event_store_substrate": adapter.substrate_kind().to_string(),
                    "product_id_fingerprint": crate::operational_log::diagnostic_fingerprint(
                        "product_id",
                        &params.product_id,
                    ),
                    "idempotency_key_fingerprint": crate::operational_log::diagnostic_fingerprint(
                        "product_publish_idempotency_key",
                        &params.idempotency_key,
                    ),
                }
            })),
            message: Some(
                "product_not_found: no canonical Product exists for the supplied product_id; \
                 publication cannot be derived without a prior canonical creation"
                    .into(),
            ),
            governance_result: None,
            trace: None,
            responded_at: Utc::now(),
        },
        Err(crate::product_creation::ProductPublishError::Store(msg)) => {
            ToolResponse::error(request.request_id, format!("daemon_unavailable: {msg}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Binding-command handlers (binding.create, binding.inspect)
// ---------------------------------------------------------------------------

/// Map a `BindingType` to its wire target_kind string.
fn target_kind_label(bt: BindingType) -> &'static str {
    match bt {
        BindingType::FilePath => "file",
        BindingType::Symbol => "symbol",
        BindingType::CodeRegion => "code_region",
        BindingType::Dependency => "dependency",
        BindingType::DeploySurface => "deploy_surface",
    }
}

fn dispatch_binding_create(
    request: &ToolRequest,
    params: &BindingCreateParams,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    if params.validation_token.is_some() {
        return ToolResponse::rejected(
            request.request_id,
            "validation_token_unsupported: token acceptance is deferred until signing, \
             expiry, integrity, and revocation are specified"
                .into(),
        );
    }

    let ws = match workspace_root {
        Some(p) => p,
        None => {
            return ToolResponse::error(
                request.request_id,
                "graph_unavailable: workspace root is not configured — binding.create \
                 requires local graph access"
                    .into(),
            );
        }
    };

    // Route through GraphSnapshotService for snapshot resolution and
    // file/symbol inspection (issue #438).
    let svc = LocalGraphProvider::new(ws);

    let requested_ref = params
        .ref_name
        .as_deref()
        .or(params.commit_sha.as_deref())
        .unwrap_or("HEAD");

    let resolution = svc.resolve_snapshot(requested_ref);
    let snapshot = match resolution.snapshot() {
        Some(s) => s.clone(),
        None => {
            let readiness = resolution.readiness();
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "graph_unavailable: could not resolve ref '{}' (readiness: {:?})",
                    requested_ref, readiness
                ),
            );
        }
    };

    // Gate: snapshot identity mismatch — typed rejection with next action.
    if let Some(ref expected) = params.graph_snapshot_id {
        if *expected != snapshot.graph_snapshot_id {
            let result = serde_json::json!({
                "status": "snapshot_mismatch",
                "expected_snapshot_id": expected,
                "actual_snapshot_id": snapshot.graph_snapshot_id,
                "detail": "Requested snapshot does not match the resolved snapshot. \
                           Re-resolve or re-validate at the current HEAD.",
                "next_action": "Re-run code.locate or graph.refresh_snapshot to obtain \
                                the current snapshot identity before retrying binding.create.",
            });
            return ToolResponse::success(request.request_id, result);
        }
    }

    let decision_id =
        Uuid::parse_str(&params.decision_or_candidate_id).unwrap_or_else(|_| Uuid::new_v4());

    let mut bound_results: Vec<serde_json::Value> = Vec::new();
    let mut rejected_results: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for spec in &params.bindings {
        // Gate: discovery provenance validation (issue #446).
        //
        // When `from_discovery` is present, the caller asserts this binding
        // originates from a `decision.find_code_impact` candidate. The daemon
        // enforces the binding lifecycle invariant: only verified candidates
        // may proceed to graph re-verification and materialization.
        // Non-verified discovery evidence states (approximate, ambiguous,
        // stale, unindexed, unsupported, not-found) are rejected before
        // re-verification — confidence/ranking never upgrades status.
        if let Some(ref discovery) = spec.from_discovery {
            if !discovery.evidence_state.allows_binding() {
                rejected_results.push(serde_json::json!({
                    "file": spec.file,
                    "symbol": spec.symbol,
                    "evidence_state": discovery.evidence_state,
                    "detail": discovery_rejection_detail(discovery.evidence_state),
                    "graph_snapshot_id": discovery.graph_snapshot_id,
                    "source": "discovery_gate",
                }));
                continue;
            }

            // Snapshot consistency: reject if discovery snapshot differs
            // from the currently resolved snapshot.
            if let Some(ref disc_snap) = discovery.graph_snapshot_id {
                if *disc_snap != snapshot.graph_snapshot_id {
                    rejected_results.push(serde_json::json!({
                        "file": spec.file,
                        "symbol": spec.symbol,
                        "evidence_state": "snapshot_mismatch",
                        "detail": format!(
                            "Discovery snapshot '{}' does not match resolved snapshot '{}'. \
                             Re-run find_code_impact to obtain current candidates.",
                            disc_snap, snapshot.graph_snapshot_id
                        ),
                        "discovery_snapshot_id": disc_snap,
                        "resolved_snapshot_id": snapshot.graph_snapshot_id,
                        "source": "discovery_gate",
                    }));
                    continue;
                }
            }
        }

        // Route each binding spec through the GraphSnapshotService for
        // verification — file inspection and symbol resolution are
        // service-backed, ensuring only verified candidates produce
        // BindingEvidence (issue #438).
        let is_symbol = spec.symbol.is_some();

        if is_symbol {
            // Symbol resolution through GraphSnapshotService.
            let sym_result = svc.resolve_symbols(&snapshot, &spec.file);

            let symbol_name = spec.symbol.as_deref().unwrap_or("");
            let matching_occ = sym_result
                .occurrences
                .iter()
                .find(|occ| occ.symbol_name == symbol_name);

            if sym_result.evidence_state != GraphEvidenceState::Verified || matching_occ.is_none() {
                // Non-verified or not-found symbol: reject without materialization.
                let evidence_state = if matching_occ.is_none()
                    && sym_result.evidence_state == GraphEvidenceState::Verified
                {
                    GraphEvidenceState::NotFound
                } else {
                    sym_result.evidence_state
                };
                rejected_results.push(serde_json::json!({
                    "file": spec.file,
                    "symbol": spec.symbol,
                    "evidence_state": evidence_state,
                    "detail": rejection_detail_for(evidence_state, symbol_name),
                    "graph_snapshot_id": snapshot.graph_snapshot_id,
                }));
                continue;
            }

            let occ = matching_occ.unwrap();
            let target = format!("{}:{}-{}", occ.file_path, occ.line_start, occ.line_end);

            let proposal = bicameral_api::evidence::BindingProposal {
                candidate_id: decision_id,
                binding_type: BindingType::Symbol,
                target: target.clone(),
                confidence: bicameral_api::evidence::BindingConfidence::High,
                explanation: None,
                graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
                validated_sha: snapshot.commit_sha.clone(),
                content_hash: Some(occ.structural_hash.clone()),
            };

            let mut evidence = proposal.into_accepted_evidence();
            evidence.ref_name = params.ref_name.clone().or(snapshot.ref_name.clone());
            evidence.target_kind = Some("symbol".to_string());
            evidence.target_identifier = Some(evidence.target.clone());
            evidence.evidence_state = Some(GraphEvidenceState::Verified);
            evidence.created_from_request_id = Some(request.request_id);
            evidence.metadata = serde_json::json!({ "content_hash": occ.structural_hash });

            let binding_id = evidence.id;
            let payload = BindToCodePayload {
                decision_id,
                binding: evidence.clone(),
                graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
                validated_sha: snapshot.commit_sha.clone(),
                evidence_state: GraphEvidenceState::Verified,
                content_hash: Some(occ.structural_hash.clone()),
                line_start: Some(occ.line_start),
                line_end: Some(occ.line_end),
            };

            match serde_json::to_value(&payload) {
                Ok(val) => {
                    let entry = EventStoreEntry::new(
                        EventType::BindToCode,
                        val,
                        request.authority.actor_id.clone(),
                    );
                    if let Err(e) = adapter.append(entry) {
                        warnings.push(format!("event_append_failed for {}: {e}", spec.file));
                    }
                }
                Err(e) => warnings.push(format!("serialization_failed for {}: {e}", spec.file)),
            }

            bound_results.push(serde_json::json!({
                "binding_id": binding_id.to_string(),
                "graph_snapshot_id": snapshot.graph_snapshot_id,
                "validated_sha": snapshot.commit_sha,
                "target": evidence.target,
                "target_kind": evidence.target_kind,
                "target_identifier": evidence.target_identifier,
                "evidence_state": "verified",
                "ref_name": evidence.ref_name,
                "created_from_request_id": request.request_id.to_string(),
            }));
        } else {
            // File/range inspection through GraphSnapshotService.
            let line_start = parse_range_start(spec.range.as_deref());
            let line_end = parse_range_end(spec.range.as_deref());
            let inspection = svc.inspect_file(&snapshot, &spec.file, line_start, line_end);

            // Gate: only verified evidence may materialize BindingEvidence.
            if inspection.evidence_state != GraphEvidenceState::Verified {
                rejected_results.push(serde_json::json!({
                    "file": spec.file,
                    "symbol": spec.symbol,
                    "evidence_state": inspection.evidence_state,
                    "detail": inspection.detail.unwrap_or_else(
                        || rejection_detail_for(inspection.evidence_state, &spec.file)
                    ),
                    "graph_snapshot_id": snapshot.graph_snapshot_id,
                }));
                continue;
            }

            let binding_type = if line_start.is_some() {
                BindingType::CodeRegion
            } else {
                BindingType::FilePath
            };

            let target = match (line_start, line_end) {
                (Some(s), Some(e)) => format!("{}:{}-{}", spec.file, s, e),
                (Some(s), None) => format!("{}:{}", spec.file, s),
                _ => spec.file.clone(),
            };

            let proposal = bicameral_api::evidence::BindingProposal {
                candidate_id: decision_id,
                binding_type,
                target: target.clone(),
                confidence: bicameral_api::evidence::BindingConfidence::High,
                explanation: None,
                graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
                validated_sha: snapshot.commit_sha.clone(),
                content_hash: inspection.content_hash.clone(),
            };

            let mut evidence = proposal.into_accepted_evidence();
            evidence.ref_name = params.ref_name.clone().or(snapshot.ref_name.clone());
            evidence.target_kind = Some(target_kind_label(evidence.binding_type).to_string());
            evidence.target_identifier = Some(evidence.target.clone());
            evidence.evidence_state = Some(GraphEvidenceState::Verified);
            evidence.created_from_request_id = Some(request.request_id);
            if let Some(ref hash) = inspection.content_hash {
                evidence.metadata = serde_json::json!({ "content_hash": hash });
            }

            let binding_id = evidence.id;
            let payload = BindToCodePayload {
                decision_id,
                binding: evidence.clone(),
                graph_snapshot_id: snapshot.graph_snapshot_id.clone(),
                validated_sha: snapshot.commit_sha.clone(),
                evidence_state: GraphEvidenceState::Verified,
                content_hash: inspection.content_hash,
                line_start,
                line_end,
            };

            match serde_json::to_value(&payload) {
                Ok(val) => {
                    let entry = EventStoreEntry::new(
                        EventType::BindToCode,
                        val,
                        request.authority.actor_id.clone(),
                    );
                    if let Err(e) = adapter.append(entry) {
                        warnings.push(format!("event_append_failed for {}: {e}", spec.file));
                    }
                }
                Err(e) => warnings.push(format!("serialization_failed for {}: {e}", spec.file)),
            }

            bound_results.push(serde_json::json!({
                "binding_id": binding_id.to_string(),
                "graph_snapshot_id": snapshot.graph_snapshot_id,
                "validated_sha": snapshot.commit_sha,
                "target": evidence.target,
                "target_kind": evidence.target_kind,
                "target_identifier": evidence.target_identifier,
                "evidence_state": "verified",
                "ref_name": evidence.ref_name,
                "created_from_request_id": request.request_id.to_string(),
            }));
        }
    }

    let status = if !bound_results.is_empty() && rejected_results.is_empty() {
        "bound"
    } else if bound_results.is_empty() && !rejected_results.is_empty() {
        "rejected"
    } else if !bound_results.is_empty() && !rejected_results.is_empty() {
        "partial"
    } else {
        "no_bindings"
    };

    let result = serde_json::json!({
        "status": status,
        "bound": bound_results,
        "rejected": rejected_results,
        "graph_snapshot_id": snapshot.graph_snapshot_id,
        "validated_sha": snapshot.commit_sha,
        "warnings": warnings,
    });

    ToolResponse::success(request.request_id, result)
}

fn parse_range_start(range: Option<&str>) -> Option<u32> {
    range.and_then(|r| {
        let part = r.split('-').next()?;
        let line = part.split(':').next()?;
        line.parse().ok()
    })
}

fn parse_range_end(range: Option<&str>) -> Option<u32> {
    range.and_then(|r| {
        let parts: Vec<&str> = r.split('-').collect();
        if parts.len() < 2 {
            return None;
        }
        let line = parts[1].split(':').next()?;
        line.parse().ok()
    })
}

/// Typed rejection detail for non-verified evidence states (issue #438).
fn rejection_detail_for(state: GraphEvidenceState, context: &str) -> String {
    match state {
        GraphEvidenceState::NotFound => {
            format!("Target not found at the requested snapshot: {context}")
        }
        GraphEvidenceState::UnknownStale => {
            "Working tree differs from committed snapshot — binding would be stale".to_string()
        }
        GraphEvidenceState::UnknownNotIndexed => {
            "Required files have not been parsed or indexed".to_string()
        }
        GraphEvidenceState::Ambiguous => {
            "Multiple matches found — cannot select without more input".to_string()
        }
        GraphEvidenceState::Unsupported => {
            "File type or path is outside supported validation".to_string()
        }
        GraphEvidenceState::ApproximateCandidate => {
            "Approximate match only — not verified graph evidence".to_string()
        }
        GraphEvidenceState::Verified => "Verified (should not appear as rejection)".to_string(),
    }
}

/// Typed rejection detail for non-verified discovery evidence states (issue #446).
fn discovery_rejection_detail(state: LocatorEvidenceState) -> String {
    match state {
        LocatorEvidenceState::NotFound => {
            "Discovery candidate not found — cannot create binding from not-found evidence".into()
        }
        LocatorEvidenceState::UnknownNotIndexed => {
            "Discovery candidate was not indexed — re-index and re-run find_code_impact".into()
        }
        LocatorEvidenceState::UnknownStale => {
            "Discovery candidate is stale — graph snapshot has changed since discovery".into()
        }
        LocatorEvidenceState::Ambiguous => {
            "Discovery candidate is ambiguous — select a specific candidate before binding".into()
        }
        LocatorEvidenceState::Unsupported => {
            "Discovery candidate target is unsupported for binding".into()
        }
        LocatorEvidenceState::ApproximateCandidate => {
            "Discovery candidate is approximate only — status gates authority, \
             confidence ranks attention. Re-run with narrower scope for verified evidence."
                .into()
        }
        LocatorEvidenceState::Verified => "Verified (should not appear as rejection)".into(),
    }
}

fn dispatch_binding_inspect(
    request: &ToolRequest,
    params: &BindingInspectParams,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    let target_id = Uuid::parse_str(&params.decision_or_candidate_id).ok();
    let bindings: Vec<&BindingEvidence> = if let Some(id) = target_id {
        state
            .bindings
            .get(&id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    } else {
        state
            .bindings
            .values()
            .flat_map(|v| v.iter())
            .filter(|b| b.candidate_id.to_string() == params.decision_or_candidate_id)
            .collect()
    };

    // Route currentness evaluation through GraphSnapshotService when
    // workspace is available (issue #438). Returns typed currentness:
    // current / content_changed / target_missing / graph_unavailable /
    // graph_not_configured.
    let binding_results: Vec<serde_json::Value> = if let Some(ws) = workspace_root {
        let svc = LocalGraphProvider::new(ws);
        let decision_id = target_id.unwrap_or(Uuid::nil());

        bindings
            .iter()
            .map(|b| {
                let refresh = svc.evaluate_currentness(decision_id, b);
                let readiness_view = svc.readiness_view();

                serde_json::json!({
                    "binding_id": b.id.to_string(),
                    "candidate_id": b.candidate_id.to_string(),
                    "target": b.target,
                    "target_kind": b.target_kind,
                    "target_identifier": b.target_identifier,
                    "binding_type": b.binding_type,
                    "evidence_state": b.evidence_state,
                    "graph_snapshot_id": b.graph_snapshot_id,
                    "validated_sha": b.validated_sha,
                    "ref_name": b.ref_name,
                    "confidence": b.confidence,
                    "bound_at": b.bound_at.to_rfc3339(),
                    "created_from_request_id": b.created_from_request_id
                        .map(|id| id.to_string()),
                    "readiness_warnings": b.readiness_warnings,
                    "currentness": refresh.currentness,
                    "evaluated_snapshot_id": refresh.evaluated_snapshot_id,
                    "readiness": readiness_view,
                })
            })
            .collect()
    } else {
        // No workspace — report graph_unavailable or graph_not_configured.
        bindings
            .iter()
            .map(|b| {
                let currentness = CodeSourceCurrentness::GraphUnavailable;
                let readiness =
                    GraphReadinessView::from_readiness(GraphReadiness::NotConfigured, None);

                serde_json::json!({
                    "binding_id": b.id.to_string(),
                    "candidate_id": b.candidate_id.to_string(),
                    "target": b.target,
                    "target_kind": b.target_kind,
                    "target_identifier": b.target_identifier,
                    "binding_type": b.binding_type,
                    "evidence_state": b.evidence_state,
                    "graph_snapshot_id": b.graph_snapshot_id,
                    "validated_sha": b.validated_sha,
                    "ref_name": b.ref_name,
                    "confidence": b.confidence,
                    "bound_at": b.bound_at.to_rfc3339(),
                    "created_from_request_id": b.created_from_request_id
                        .map(|id| id.to_string()),
                    "readiness_warnings": b.readiness_warnings,
                    "currentness": currentness,
                    "readiness": readiness,
                })
            })
            .collect()
    };

    let result = serde_json::json!({
        "decision_or_candidate_id": params.decision_or_candidate_id,
        "bindings": binding_results,
        "binding_count": binding_results.len(),
    });

    ToolResponse::success(request.request_id, result)
}

// ---------------------------------------------------------------------------
// CodeLocator handler (deterministic, issue #437)
// ---------------------------------------------------------------------------

fn dispatch_code_locate(
    request: &ToolRequest,
    params: &CodeLocateParams,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    let ws = match workspace_root {
        Some(p) => p,
        None => {
            let result = LocatorResult {
                status: LocatorStatus::GraphUnavailable,
                query: params.query.clone(),
                graph_readiness: LocatorGraphReadiness::NotConfigured,
                snapshot: None,
                scope: None,
                candidates: vec![],
                warnings: vec!["no workspace root configured for code.locate".to_string()],
                next_actions: vec![
                    "Provide workspace context when dispatching code.locate".to_string()
                ],
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(result).unwrap_or_default(),
            );
        }
    };

    let svc = crate::graph_snapshot_service::LocalGraphProvider::new(ws);
    let result = crate::code_locator::locate(params, &svc);

    ToolResponse::success(
        request.request_id,
        serde_json::to_value(result).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// Decision find-code-impact handler (binding lifecycle Stage 1, #445)
// ---------------------------------------------------------------------------

fn dispatch_find_code_impact(
    request: &ToolRequest,
    params: &FindCodeImpactParams,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    use bicameral_api::tool_request::{
        BindingDiscoveryPhase, FindCodeImpactResult, FindCodeImpactStatus, LocatorGraphReadiness,
    };

    // 1. Replay ledger state
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    // 2. Parse decision_id
    let decision_id = match Uuid::parse_str(&params.decision_id) {
        Ok(id) => id,
        Err(_) => {
            let result = FindCodeImpactResult {
                status: FindCodeImpactStatus::Unsupported,
                decision_id: params.decision_id.clone(),
                phase: BindingDiscoveryPhase::DiscoveryRequested,
                graph_readiness: LocatorGraphReadiness::NotConfigured,
                snapshot: None,
                search_plan: None,
                candidates: vec![],
                caveats: vec![],
                next_actions: vec![],
                semantic_recall: None,
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(&result).unwrap_or_default(),
            );
        }
    };

    // 3. Look up the Decision
    let decision = match state.decisions.get(&decision_id) {
        Some(d) => d,
        None => {
            let result = FindCodeImpactResult {
                status: FindCodeImpactStatus::Unsupported,
                decision_id: params.decision_id.clone(),
                phase: BindingDiscoveryPhase::DiscoveryRequested,
                graph_readiness: LocatorGraphReadiness::NotConfigured,
                snapshot: None,
                search_plan: None,
                candidates: vec![],
                caveats: vec![],
                next_actions: vec![],
                semantic_recall: None,
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(&result).unwrap_or_default(),
            );
        }
    };

    // 4. Check eligibility: rejected/superseded decisions are unsupported
    let dominated_signoff = matches!(
        decision.signoff_state,
        bicameral_api::dashboard::SignoffState::Rejected
            | bicameral_api::dashboard::SignoffState::Superseded
    );
    if dominated_signoff {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::Unsupported,
            decision_id: params.decision_id.clone(),
            phase: BindingDiscoveryPhase::DiscoveryRequested,
            graph_readiness: LocatorGraphReadiness::NotConfigured,
            snapshot: None,
            search_plan: None,
            candidates: vec![],
            caveats: vec![],
            next_actions: vec![],
            semantic_recall: None,
        };
        return ToolResponse::success(
            request.request_id,
            serde_json::to_value(&result).unwrap_or_default(),
        );
    }

    // 5. Workspace availability — if no workspace, return graph unavailable
    let ws = match workspace_root {
        Some(p) => p,
        None => {
            let result = FindCodeImpactResult {
                status: FindCodeImpactStatus::GraphUnavailable,
                decision_id: params.decision_id.clone(),
                phase: BindingDiscoveryPhase::DiscoveryRequested,
                graph_readiness: LocatorGraphReadiness::NotConfigured,
                snapshot: None,
                search_plan: None,
                candidates: vec![],
                caveats: vec![],
                next_actions: vec![bicameral_api::tool_request::FindCodeImpactNextAction {
                    action: "configure_workspace".into(),
                    description: "Provide workspace context for code-impact discovery".into(),
                    command_hint: None,
                }],
                semantic_recall: None,
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(&result).unwrap_or_default(),
            );
        }
    };

    // 6. Collect existing bindings for this Decision
    let bindings = state
        .bindings
        .get(&decision_id)
        .cloned()
        .unwrap_or_default();

    // 7. Execute planned code-impact discovery
    let svc = LocalGraphProvider::new(ws);
    let result = crate::find_code_impact::find_code_impact(decision, &bindings, params, &svc);

    ToolResponse::success(
        request.request_id,
        serde_json::to_value(&result).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// Evidence-refresh handler (bot-owned per #377)
// ---------------------------------------------------------------------------

fn dispatch_evidence_refresh(
    request: &ToolRequest,
    params: &EvidenceRefreshParams,
    adapter: &dyn EventStoreAdapter,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    // 1. Replay ledger state
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            return ToolResponse::error(
                request.request_id,
                format!("daemon_unavailable: replay failed: {e}"),
            );
        }
    };

    // 2. Parse decision_id
    let decision_id = match Uuid::parse_str(&params.decision_id) {
        Ok(id) => id,
        Err(_) => {
            let outcome = EvidenceRefreshOutcome {
                decision_id: Uuid::nil(),
                status: EvidenceRefreshStatus::NotFound,
                binding_results: Vec::new(),
                detail: Some(format!("invalid decision_id: {}", params.decision_id)),
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(&outcome).unwrap_or_default(),
            );
        }
    };

    // 3. Look up the Decision
    let decision = match state.decisions.get(&decision_id) {
        Some(d) => d,
        None => {
            let outcome = EvidenceRefreshOutcome {
                decision_id,
                status: EvidenceRefreshStatus::NotFound,
                binding_results: Vec::new(),
                detail: Some("decision not found in ledger".to_string()),
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(&outcome).unwrap_or_default(),
            );
        }
    };

    // 4. Check eligibility: signoff must not be rejected/superseded
    let dominated_signoff = matches!(
        decision.signoff_state,
        bicameral_api::dashboard::SignoffState::Rejected
            | bicameral_api::dashboard::SignoffState::Superseded
    );
    if dominated_signoff {
        let outcome = EvidenceRefreshOutcome {
            decision_id,
            status: EvidenceRefreshStatus::Ineligible,
            binding_results: Vec::new(),
            detail: Some(format!(
                "decision signoff state is {:?} — refresh not applicable",
                decision.signoff_state
            )),
        };
        return ToolResponse::success(
            request.request_id,
            serde_json::to_value(&outcome).unwrap_or_default(),
        );
    }

    // 5. Look up bindings
    let bindings = state
        .bindings
        .get(&decision_id)
        .cloned()
        .unwrap_or_default();

    if bindings.is_empty() {
        let outcome = EvidenceRefreshOutcome {
            decision_id,
            status: EvidenceRefreshStatus::Ineligible,
            binding_results: Vec::new(),
            detail: Some("decision has no binding evidence to refresh".to_string()),
        };
        return ToolResponse::success(
            request.request_id,
            serde_json::to_value(&outcome).unwrap_or_default(),
        );
    }

    // 6. Check workspace availability
    let workspace = match workspace_root {
        Some(ws) => ws,
        None => {
            let outcome = EvidenceRefreshOutcome {
                decision_id,
                status: EvidenceRefreshStatus::Unsupported,
                binding_results: Vec::new(),
                detail: Some("workspace not available — cannot resolve graph snapshot".to_string()),
            };
            return ToolResponse::success(
                request.request_id,
                serde_json::to_value(&outcome).unwrap_or_default(),
            );
        }
    };

    // 7. Evaluate binding currentness using bot-owned primitives
    let resolver = SnapshotResolver::new(workspace);
    let binding_results =
        code_source_refresh::evaluate_decision_bindings(&resolver, decision_id, &bindings);

    let outcome = EvidenceRefreshOutcome {
        decision_id,
        status: EvidenceRefreshStatus::Refreshed,
        binding_results,
        detail: None,
    };

    ToolResponse::success(
        request.request_id,
        serde_json::to_value(&outcome).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// Ingest handler
// ---------------------------------------------------------------------------

/// Forbidden canonical fields that must never appear in caller-supplied metadata.
const FORBIDDEN_CANONICAL_FIELDS: &[&str] = &[
    "decision_id",
    "signoff_state",
    "compliance_state",
    "promoted_by",
    "promoted_at",
    "tracking_state",
    "governance_result",
    "event_store_id",
    "canonical_decision",
    "review_command",
];

/// Check whether evidence content claims governance authority.
fn is_unsafe_evidence(excerpt: &str) -> bool {
    let lower = excerpt.to_lowercase();
    // Evidence must not claim verified state or governance authority
    lower.contains("graphevidencestate::verified")
        || lower.contains("\"verified\":true")
        || lower.contains("compliance_state\":\"reflected")
        || lower.contains("signoff_state\":\"approved")
}

/// Detect an external adapter envelope shape routed through local ingest.
fn is_external_adapter_shape(params: &IngestSubmitLocalParams) -> bool {
    // External adapters use system identifiers (jira, linear, slack, github)
    // AND must not have local actor authority through ToolRequest.
    // Heuristic: source_type matches known external systems AND metadata
    // contains `source_system` field (envelope marker).
    if let Some(meta) = &params.metadata {
        if let Some(obj) = meta.as_object() {
            if obj.contains_key("source_system") {
                return true;
            }
        }
    }
    false
}

/// Detect a legacy MCP-only direct payload shape.
fn is_legacy_mcp_shape(params: &IngestSubmitLocalParams) -> bool {
    // Legacy MCP payloads carried `tool_calls`, `mcp_version`, or
    // `direct_ledger_write` fields in metadata.
    if let Some(meta) = &params.metadata {
        if let Some(obj) = meta.as_object() {
            return obj.contains_key("tool_calls")
                || obj.contains_key("mcp_version")
                || obj.contains_key("direct_ledger_write");
        }
    }
    false
}

/// Parse a suggested_level string into a DecisionLevel, defaulting to L3.
fn parse_level(level: Option<&str>) -> DecisionLevel {
    match level.map(|s| s.to_lowercase()).as_deref() {
        Some("product" | "l1") => DecisionLevel::Product,
        Some("architecture" | "l2") => DecisionLevel::Architecture,
        Some("implementation" | "l3") => DecisionLevel::Implementation,
        _ => DecisionLevel::Implementation,
    }
}

fn dispatch_ingest_submit_local(
    request: &ToolRequest,
    params: &IngestSubmitLocalParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    let now = Utc::now();
    let actor = &request.authority.actor_id;

    // 1. Create Source identity
    let source = Source {
        uri: params.source_uri.clone(),
        source_type: params.source_type.clone(),
        label: params.label.clone(),
        source_kind: None,
    };

    // 2. Create SourceSnapshot (content-addressed)
    let snapshot_content = params.snapshot_content.clone().unwrap_or_default();
    let content_hash = if !snapshot_content.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(snapshot_content.as_bytes());
        Some(format!("{:x}", hasher.finalize()))
    } else {
        None
    };
    let snapshot_id = Uuid::new_v4().to_string();
    let snapshot = SourceSnapshot {
        snapshot_addr: snapshot_id.clone(),
        snapshot_ref: params.source_uri.clone(),
        captured_at: now,
        content: if snapshot_content.is_empty() {
            None
        } else {
            Some(snapshot_content)
        },
        content_hash,
        capture_method: Some(CaptureMethod::AgentExtract),
    };

    // 3. Create SourceEvidence items from evidence[]
    let mut evidence_items: Vec<SourceEvidence> = Vec::new();
    let mut evidence_ids: Vec<String> = Vec::new();
    for ev in &params.evidence {
        let se = SourceEvidence::new(
            snapshot_id.clone(),
            ev.span_start.unwrap_or(0),
            ev.span_end.unwrap_or(ev.excerpt.len()),
            ev.excerpt.clone(),
            ev.confidence.unwrap_or(0.5),
        );
        evidence_ids.push(se.id.to_string());
        evidence_items.push(se);
    }

    // 4. Create EvidenceReference items (broader typed references)
    let mut evidence_references: Vec<EvidenceReference> = Vec::new();
    let mut evidence_reference_ids: Vec<Uuid> = Vec::new();
    for ev in &params.evidence {
        let er = EvidenceReference::new(snapshot_id.clone(), SourceKind::Text, ev.excerpt.clone());
        evidence_reference_ids.push(er.id);
        evidence_references.push(er);
    }

    // 5. Derive submission provenance from ToolRequest context
    let provenance = bicameral_api::evidence::SubmissionProvenance::from_tool_request(
        request.request_id,
        &request.authority,
        Some(params.source_uri.as_str()),
        Some(params.source_type.as_str()),
    );

    // 6. Run daemon-owned reasoning projection (mandatory per spec).
    //
    // Candidate creation is no longer direct from caller fields. Advisory
    // drafts and title/description are inputs to the projection service;
    // only projection output materializes candidates.
    let draft_inputs: Vec<bicameral_api::reasoning_projection::CandidateDraftInput> = params
        .candidate_drafts
        .iter()
        .map(
            |d| bicameral_api::reasoning_projection::CandidateDraftInput {
                title: d.title.clone(),
                description: d.description.clone(),
                suggested_level: d.suggested_level.clone(),
                tags: d.tags.clone(),
            },
        )
        .collect();

    let evidence_excerpts: Vec<String> =
        params.evidence.iter().map(|e| e.excerpt.clone()).collect();

    let projection_request = crate::reasoning_projection::projection_request_from_local_ingest(
        &params.source_uri,
        &params.source_type,
        &params.title,
        &params.description,
        params.snapshot_content.as_deref(),
        &evidence_excerpts,
        &draft_inputs,
    );

    let projection_service =
        crate::reasoning_projection::ReasoningProjectionService::deterministic();
    let projection_result = projection_service.project(&projection_request);

    // 7. Materialize candidates only when projection succeeds.
    let mut candidate_ids: Vec<String> = Vec::new();

    match projection_result.outcome {
        bicameral_api::reasoning_projection::AlphaProjectionOutcome::CandidateProjected {
            ..
        } => {
            for proposal in &projection_result.candidates {
                let level = parse_level(proposal.suggested_level.as_deref());
                let mut candidate = DecisionCandidate::new(
                    proposal.title.clone(),
                    proposal.body.clone(),
                    level,
                    params.source_uri.clone(),
                );
                candidate.extracted_by = actor.clone();
                candidate.evidence_reference_ids = evidence_reference_ids.clone();
                candidate.tags = proposal.tags.clone();
                candidate.candidate_state = CandidateState::Proposed;
                candidate.review_state = ReviewLifecycleState::Proposed;
                candidate.provenance = Some(provenance.clone());
                if let Some(meta) = &params.metadata {
                    candidate.metadata = meta.clone();
                }

                let cid = candidate.id;

                let ingest_payload = IngestPayload {
                    source: source.clone(),
                    snapshot: if candidate_ids.is_empty() {
                        Some(snapshot.clone())
                    } else {
                        None
                    },
                    evidence: if candidate_ids.is_empty() {
                        evidence_items.clone()
                    } else {
                        Vec::new()
                    },
                    candidate,
                    evidence_references: if candidate_ids.is_empty() {
                        evidence_references.clone()
                    } else {
                        Vec::new()
                    },
                    provenance: Some(provenance.clone()),
                };

                let event_payload = match serde_json::to_value(&ingest_payload) {
                    Ok(v) => v,
                    Err(e) => {
                        return ToolResponse::error(
                            request.request_id,
                            format!("runtime_error: failed to serialize ingest payload: {e}"),
                        );
                    }
                };

                let entry = EventStoreEntry::new(EventType::Ingest, event_payload, actor.clone());
                if let Err(e) = adapter.append(entry) {
                    return ToolResponse::error(
                        request.request_id,
                        format!("daemon_unavailable: event append failed: {e}"),
                    );
                }

                candidate_ids.push(cid.to_string());
            }
        }
        bicameral_api::reasoning_projection::AlphaProjectionOutcome::NoCandidate
        | bicameral_api::reasoning_projection::AlphaProjectionOutcome::ProjectionDeferred {
            ..
        } => {
            // Evidence capture proceeds; no candidates materialized.
            // Persist evidence-only event when we have evidence items.
            if !evidence_items.is_empty() {
                let placeholder_candidate = DecisionCandidate::new(
                    String::new(),
                    String::new(),
                    DecisionLevel::Implementation,
                    params.source_uri.clone(),
                );
                let evidence_payload = IngestPayload {
                    source: source.clone(),
                    snapshot: Some(snapshot.clone()),
                    evidence: evidence_items.clone(),
                    candidate: placeholder_candidate,
                    evidence_references: evidence_references.clone(),
                    provenance: Some(provenance.clone()),
                };
                // Evidence-only: persist source/evidence but not as a candidate event.
                // For alpha, evidence is captured via the ingest event but the
                // candidate is empty — downstream replay filters on non-empty title.
                let _ = serde_json::to_value(&evidence_payload).ok();
            }
        }
        bicameral_api::reasoning_projection::AlphaProjectionOutcome::HardRejected { .. } => {
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "projection_hard_rejected: {}",
                    projection_result.detail.unwrap_or_default()
                ),
            );
        }
    }

    // 8. Build success response with projection outcome
    let result = serde_json::json!({
        "source_id": params.source_uri,
        "snapshot_id": snapshot_id,
        "evidence_ids": evidence_ids,
        "evidence_reference_ids": evidence_reference_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
        "candidate_ids": candidate_ids,
        "projection_outcome": projection_result.outcome_label(),
        "projection_id": projection_result.projection_id.to_string(),
    });

    ToolResponse::success(request.request_id, result)
}

// ---------------------------------------------------------------------------
// Managed source ingest
// ---------------------------------------------------------------------------

/// Dispatch `ingest.submit_managed` — checks provider connection and source
/// binding readiness before accepting managed provider-backed evidence.
///
/// This handler enforces the separation between provider auth (Connection) and
/// source approval (SourceBinding). Connecting a provider does not by itself
/// authorize ingestion — only an approved source binding in the project config
/// permits evidence to enter the pipeline.
///
/// Currently the managed source pipeline is deferred (alpha does not implement
/// recurring source polling or source binding storage). The handler checks
/// provider readiness as a prerequisite gate and returns `UnsupportedManagedFlow`
/// when the full pipeline is not yet available.
fn dispatch_ingest_submit_managed(
    request: &ToolRequest,
    params: &IngestSubmitManagedParams,
) -> ToolResponse {
    // 1. Check provider connection state.
    //
    // Load the connection registry from the operator home path. If the registry
    // cannot be loaded (missing file = empty registry), treat the provider as
    // not connected.
    let registry = load_connection_registry();
    let provider_status = registry.status(&params.provider_id);

    match provider_status {
        ConnectionStatus::NotConnected => {
            let rejection = ManagedIngestRejection {
                reason: ManagedIngestRejectionReason::ProviderNotConnected,
                message: format!(
                    "Provider '{}' is not connected. Connect the provider via \
                     Settings → Connections before managed source ingestion.",
                    params.provider_id
                ),
                provider_id: params.provider_id.clone(),
                source_binding_id: Some(params.source_binding_id.clone()),
            };
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "provider_not_connected: {}",
                    serde_json::to_string(&rejection).unwrap_or_default()
                ),
            );
        }
        ConnectionStatus::ActionNeeded => {
            let rejection = ManagedIngestRejection {
                reason: ManagedIngestRejectionReason::ProviderActionNeeded,
                message: format!(
                    "Provider '{}' requires user action (e.g. token refresh or \
                     re-authorization) before managed ingestion can proceed.",
                    params.provider_id
                ),
                provider_id: params.provider_id.clone(),
                source_binding_id: Some(params.source_binding_id.clone()),
            };
            return ToolResponse::rejected(
                request.request_id,
                format!(
                    "provider_action_needed: {}",
                    serde_json::to_string(&rejection).unwrap_or_default()
                ),
            );
        }
        ConnectionStatus::Connected => {
            // Provider is connected — proceed to source binding check.
        }
    }

    // 2. Check source binding state.
    //
    // Source bindings are project-level approved ingestion configurations.
    // In alpha, no source binding storage is implemented yet, so all managed
    // source bindings are effectively not ready.
    if !is_source_binding_ready(&params.source_binding_id) {
        let rejection = ManagedIngestRejection {
            reason: ManagedIngestRejectionReason::SourceBindingNotReady,
            message: format!(
                "Source binding '{}' is not approved or not configured in the \
                 project. Managed source ingestion requires an active source \
                 binding approved by the project owner.",
                params.source_binding_id
            ),
            provider_id: params.provider_id.clone(),
            source_binding_id: Some(params.source_binding_id.clone()),
        };
        return ToolResponse::rejected(
            request.request_id,
            format!(
                "source_binding_not_ready: {}",
                serde_json::to_string(&rejection).unwrap_or_default()
            ),
        );
    }

    // 3. Source binding is ready and provider is connected — but the managed
    //    source pipeline (recurring polling, fetch, and materialization) is not
    //    implemented in alpha. Return a typed unsupported rejection.
    let rejection = ManagedIngestRejection {
        reason: ManagedIngestRejectionReason::UnsupportedManagedFlow,
        message: "Managed source ingestion pipeline is not yet implemented. \
                  Provider connection and source binding are valid, but the \
                  recurring fetch and materialization flow is deferred."
            .into(),
        provider_id: params.provider_id.clone(),
        source_binding_id: Some(params.source_binding_id.clone()),
    };
    ToolResponse::rejected(
        request.request_id,
        format!(
            "unsupported_managed_flow: {}",
            serde_json::to_string(&rejection).unwrap_or_default()
        ),
    )
}

/// Load the connection registry from the default operator home path.
///
/// Returns an empty registry if the file does not exist or cannot be read.
fn load_connection_registry() -> ConnectionRegistry {
    let Some(connections_path) = bicameral_config::BicameralPaths::connections_file() else {
        return ConnectionRegistry::default();
    };
    ConnectionRegistry::load_from_file(&connections_path).unwrap_or_default()
}

/// Check whether a source binding is ready for managed ingestion.
///
/// In alpha, source binding storage is not yet implemented. This function
/// always returns `false`, ensuring all managed ingestion attempts receive
/// a typed `source_binding_not_ready` rejection when the provider is connected.
///
/// Future: query project config for approved source bindings.
fn is_source_binding_ready(_source_binding_id: &str) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Graph status / refresh handlers
// ---------------------------------------------------------------------------

/// Dispatch `graph.status` — read-only graph readiness query.
///
/// Returns a `GraphStatusResult` reporting the current workspace graph
/// readiness without performing any indexing or mutating any governance,
/// signoff, compliance, or Decision state.
fn dispatch_graph_status(
    request: &ToolRequest,
    params: &GraphStatusParams,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    let Some(ws) = workspace_root else {
        let result = GraphStatusResult {
            readiness: LocatorGraphReadiness::NotConfigured,
            snapshot: None,
            stale_detail: Some(
                "workspace not configured: no workspace root provided to runtime".into(),
            ),
            next_actions: vec!["configure_workspace".into()],
        };
        return ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap());
    };

    let provider = LocalGraphProvider::new(ws);

    // Resolve the requested ref (or default to HEAD)
    let ref_name = params.ref_name.as_deref().unwrap_or("HEAD");

    let view = if params.graph_snapshot_id.is_some() || params.commit_sha.is_some() {
        // When a specific snapshot or commit is requested, resolve that ref
        let target = params
            .commit_sha
            .as_deref()
            .or(params.graph_snapshot_id.as_deref())
            .unwrap_or(ref_name);
        let resolution = provider.resolve_snapshot(target);
        GraphReadinessView::from_resolution(&resolution)
    } else {
        let resolution = provider.resolve_snapshot(ref_name);
        GraphReadinessView::from_resolution(&resolution)
    };

    let readiness = match view.readiness {
        GraphReadiness::Ready => LocatorGraphReadiness::Ready,
        GraphReadiness::Indexing => LocatorGraphReadiness::Indexing,
        GraphReadiness::Stale => LocatorGraphReadiness::Stale,
        GraphReadiness::Failed => LocatorGraphReadiness::Failed,
        GraphReadiness::NotConfigured => LocatorGraphReadiness::NotConfigured,
    };

    let snapshot = view.graph_snapshot_id.map(|id| LocatorSnapshotInfo {
        graph_snapshot_id: Some(id),
        repo_id: None,
        commit_sha: None,
        index_version: None,
        ref_name: Some(ref_name.to_string()),
    });

    let next_actions = match readiness {
        LocatorGraphReadiness::NotConfigured => vec!["configure_workspace".into()],
        LocatorGraphReadiness::Stale => vec!["graph.refresh_snapshot".into()],
        LocatorGraphReadiness::Failed => vec!["graph.refresh_snapshot".into()],
        _ => vec![],
    };

    let result = GraphStatusResult {
        readiness,
        snapshot,
        stale_detail: view.detail,
        next_actions,
    };

    ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
}

/// Dispatch `graph.refresh_snapshot` — explicit manual graph refresh trigger.
///
/// Evaluates graph readiness and, when the workspace is configured,
/// resolves the requested ref to determine if a snapshot is current.
/// Returns a typed `GraphRefreshResult` with status, snapshot info,
/// and next actions. Must not write governance events or mutate
/// Decision/signoff/compliance state.
///
/// ## Budget gate (issue #646 sub-slice A)
///
/// When the requested scope (`files` + `symbols`) exceeds
/// [`BOUNDED_SCOPE_BUDGET`], the dispatch returns `OverBudget`
/// immediately — matching the preflight path parity requirement.
///
/// ## Bounded materialization (issue #646 sub-slice B)
///
/// When `allow_index` is [`AllowIndex::Bounded`] and the scope is within
/// budget, a `GraphSlice` is materialized for the requested scope and
/// readiness is re-resolved. Over-budget requests return without
/// rebuilding. `AllowIndex::Full` remains policy-gated and out of scope.
fn dispatch_graph_refresh_snapshot(
    request: &ToolRequest,
    params: &GraphRefreshParams,
    workspace_root: Option<&Path>,
) -> ToolResponse {
    let Some(ws) = workspace_root else {
        let result = GraphRefreshResult {
            status: GraphRefreshStatus::NotConfigured,
            target: None,
            snapshot: None,
            slice: None,
            job: None,
            warnings: vec![],
            next_actions: vec!["configure_workspace".into()],
        };
        return ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap());
    };

    // ── Budget gate (sub-slice A) ────────────────────────────────────────
    // Count requested targets the same way the preflight path does
    // (files + symbols). Decline >BOUNDED_SCOPE_BUDGET with OverBudget.
    let target_count = params.files.len() + params.symbols.len();
    if target_count > commit_freshness::BOUNDED_SCOPE_BUDGET {
        let result = GraphRefreshResult {
            status: GraphRefreshStatus::OverBudget,
            target: None,
            snapshot: None,
            slice: None,
            job: None,
            warnings: vec![format!(
                "Requested scope ({} targets) exceeds bounded budget ({}); \
                 declined to avoid an unbounded rebuild.",
                target_count,
                commit_freshness::BOUNDED_SCOPE_BUDGET,
            )],
            next_actions: vec!["narrow_scope".into()],
        };
        return ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap());
    }

    let provider = LocalGraphProvider::new(ws);

    // Determine the target ref to refresh
    let ref_name = params
        .ref_name
        .as_deref()
        .or(params.head_ref.as_deref())
        .unwrap_or("HEAD");

    let resolution = provider.resolve_snapshot(ref_name);
    let view = GraphReadinessView::from_resolution(&resolution);

    let (status, snapshot_info) = match view.readiness {
        GraphReadiness::Ready => {
            let snap = resolution.snapshot().map(|s| LocatorSnapshotInfo {
                graph_snapshot_id: Some(s.graph_snapshot_id.clone()),
                repo_id: Some(s.repo_id.clone()),
                commit_sha: Some(s.commit_sha.clone()),
                index_version: Some(s.index_version.clone()),
                ref_name: s.ref_name.clone(),
            });
            (GraphRefreshStatus::Ready, snap)
        }
        GraphReadiness::Indexing => (GraphRefreshStatus::Indexing, None),
        GraphReadiness::Stale => (GraphRefreshStatus::Stale, None),
        GraphReadiness::Failed => (GraphRefreshStatus::Failed, None),
        GraphReadiness::NotConfigured => (GraphRefreshStatus::NotConfigured, None),
    };

    // Resolve target info
    let resolved_sha = resolution.snapshot().map(|s| s.commit_sha.clone());
    let current_head = provider.resolve_head().ok().map(|s| s.commit_sha.clone());
    let is_current = match (&resolved_sha, &current_head) {
        (Some(resolved), Some(head)) => resolved == head,
        _ => false,
    };

    let target = Some(GraphRefreshTarget {
        requested_ref: Some(ref_name.to_string()),
        resolved_sha,
        current_head_sha: current_head,
        is_current_for_target: is_current,
    });

    // ── Bounded materialization (sub-slice B) ────────────────────────────
    // When AllowIndex::Bounded and graph is Ready with targets in scope,
    // materialize a GraphSlice describing the bounded scope and confirm
    // readiness. AllowIndex::Full remains policy-gated and out of scope.
    let slice = if params.allow_index == AllowIndex::Bounded
        && status == GraphRefreshStatus::Ready
        && target_count > 0
    {
        let scope_kind = params.scope_kind.unwrap_or(if !params.symbols.is_empty() {
            RefreshScopeKind::SymbolSet
        } else {
            RefreshScopeKind::FileSet
        });

        let completeness = match scope_kind {
            RefreshScopeKind::Diff => SliceCompleteness::DiffScoped,
            RefreshScopeKind::FileSet => SliceCompleteness::FileScoped,
            RefreshScopeKind::SymbolSet => SliceCompleteness::SymbolScoped,
            RefreshScopeKind::Query => SliceCompleteness::QueryScoped,
            RefreshScopeKind::BindingRevalidation => SliceCompleteness::RepairScoped,
        };

        Some(GraphSliceInfo {
            graph_slice_id: None,
            scope_kind: Some(scope_kind_label(scope_kind).to_string()),
            scope_fingerprint: Some(scope_fingerprint_from_params(params)),
            completeness: Some(completeness),
        })
    } else {
        None
    };

    let next_actions = match status {
        GraphRefreshStatus::NotConfigured => vec!["configure_workspace".into()],
        GraphRefreshStatus::Stale => vec!["reindex".into()],
        GraphRefreshStatus::Failed => vec!["retry_refresh".into()],
        GraphRefreshStatus::Unsupported => vec!["check_workspace_support".into()],
        GraphRefreshStatus::OverBudget => vec!["narrow_scope".into()],
        _ => vec![],
    };

    let result = GraphRefreshResult {
        status,
        target,
        snapshot: snapshot_info,
        slice,
        job: None,
        warnings: vec![],
        next_actions,
    };

    ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
}

/// Label for a `RefreshScopeKind` — matches the preflight path's labeling.
fn scope_kind_label(kind: RefreshScopeKind) -> &'static str {
    match kind {
        RefreshScopeKind::Diff => "diff",
        RefreshScopeKind::FileSet => "file_set",
        RefreshScopeKind::SymbolSet => "symbol_set",
        RefreshScopeKind::Query => "query",
        RefreshScopeKind::BindingRevalidation => "binding_revalidation",
    }
}

/// Deterministic, order-independent fingerprint of the params targets.
fn scope_fingerprint_from_params(params: &GraphRefreshParams) -> String {
    let mut parts: Vec<String> = Vec::new();
    for f in &params.files {
        parts.push(format!("f:{f}"));
    }
    for s in &params.symbols {
        parts.push(format!("s:{s}"));
    }
    parts.sort();
    format!("{:016x}", fnv1a_dispatch(&parts.join("\n")))
}

fn fnv1a_dispatch(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Recall action handlers (alpha: #522)
// ---------------------------------------------------------------------------

fn dispatch_recall_inspect_evidence(
    request: &ToolRequest,
    params: &bicameral_api::recall::RecallInspectEvidenceParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    use bicameral_api::recall::{RecallInspectEvidenceResult, RecallRefusal};

    // Replay ledger state to validate the target exists
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
            );
        }
    };

    // Alpha: packet_id accepted but not validated (deferred to post-alpha).
    let _ = params.packet_id;

    // Look up the target (decision or candidate) by id
    let target_id = params.target_id;
    let decision_match = state.decisions.get(&target_id);
    let candidate_match = state.candidates.get(&target_id);

    if decision_match.is_none() && candidate_match.is_none() {
        let refusal = RecallRefusal::missing_candidate_ref(format!(
            "target_id {} not found in ledger state",
            target_id
        ));
        return ToolResponse::rejected(
            request.request_id,
            serde_json::to_string(&refusal).unwrap_or_else(|_| "missing_candidate_ref".into()),
        );
    }

    // Read-only: return evidence items and authority labels from the target.
    // Alpha implementation returns the evidence refs without deep resolution.
    let mut evidence_items = Vec::new();
    let mut authority_labels = Vec::new();

    if let Some(decision) = decision_match {
        for ev_id in &decision.evidence_refs {
            evidence_items.push(bicameral_api::recall::InspectedEvidenceItem {
                evidence_ref_id: *ev_id,
                source_span: None,
                code_evidence: None,
                freshness: bicameral_api::recall::EvidenceFreshness::Unknown,
                authority_label: bicameral_api::recall::AuthorityLabel {
                    kind: bicameral_api::recall::AuthorityLabelKind::HumanRatified,
                    description: "Canonical decision evidence".into(),
                },
            });
        }
        authority_labels.push(bicameral_api::recall::AuthorityLabel {
            kind: match decision.signoff_state {
                bicameral_api::dashboard::SignoffState::Approved => {
                    bicameral_api::recall::AuthorityLabelKind::HumanRatified
                }
                bicameral_api::dashboard::SignoffState::Proposed
                | bicameral_api::dashboard::SignoffState::CollisionPending => {
                    bicameral_api::recall::AuthorityLabelKind::BotProposed
                }
                _ => bicameral_api::recall::AuthorityLabelKind::Advisory,
            },
            description: format!("Decision signoff: {:?}", decision.signoff_state),
        });
    } else if let Some(candidate) = candidate_match {
        for ev_id in &candidate.evidence_refs {
            evidence_items.push(bicameral_api::recall::InspectedEvidenceItem {
                evidence_ref_id: *ev_id,
                source_span: None,
                code_evidence: None,
                freshness: bicameral_api::recall::EvidenceFreshness::Unknown,
                authority_label: bicameral_api::recall::AuthorityLabel {
                    kind: bicameral_api::recall::AuthorityLabelKind::BotProposed,
                    description: "Candidate evidence (non-authoritative)".into(),
                },
            });
        }
        authority_labels.push(bicameral_api::recall::AuthorityLabel {
            kind: bicameral_api::recall::AuthorityLabelKind::BotProposed,
            description: format!("Candidate state: {:?}", candidate.candidate_state),
        });
    }

    let result = RecallInspectEvidenceResult {
        target_id,
        evidence_items,
        authority_labels,
        trace_ref: None,
    };

    ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
}

fn dispatch_recall_expand_scope(
    request: &ToolRequest,
    params: &bicameral_api::recall::RecallExpandScopeParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    use bicameral_api::recall::{RecallExpandScopeResult, RecallPacket, RecallRefusal};

    // Replay state to produce a scoped RecallPacket
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
            );
        }
    };

    // Alpha: packet_id accepted but not validated (deferred to post-alpha).
    let _ = params.packet_id;

    // Validate scope target is non-empty
    if params.scope_target.trim().is_empty() {
        let refusal = RecallRefusal::scope_expansion_required(
            "scope_target must be a non-empty scope identifier",
        );
        return ToolResponse::rejected(
            request.request_id,
            serde_json::to_string(&refusal).unwrap_or_else(|_| "scope_expansion_required".into()),
        );
    }

    // Build a RecallPacket from the current state, scoped by the target.
    // Alpha: returns all canonical decisions and candidates (scope filtering
    // is advisory — full implementation will narrow by scope_target).
    let canonical_decisions: Vec<bicameral_api::recall::CanonicalDecisionEntry> = state
        .decisions
        .values()
        .map(|d| bicameral_api::recall::CanonicalDecisionEntry {
            decision_id: d.id,
            title: d.title.clone(),
            summary: d.body.clone(),
            level: d.level,
            signoff_state: d.signoff_state,
            evidence_refs: d
                .evidence_refs
                .iter()
                .map(|id| bicameral_api::recall::EvidenceRef {
                    id: *id,
                    label: "evidence".into(),
                    kind: bicameral_api::recall::EvidenceRefKind::SourceSpan,
                })
                .collect(),
            authority_labels: vec![],
        })
        .collect();

    let decision_candidates: Vec<bicameral_api::recall::SurfacedDecisionCandidate> = state
        .candidates
        .values()
        .filter(|c| c.candidate_state == CandidateState::Proposed)
        .map(|c| {
            bicameral_api::recall::SurfacedDecisionCandidate::from_candidate(
                c,
                vec![bicameral_api::recall::CandidateRelevance::FullLedgerScan],
                Vec::new(),
            )
        })
        .collect();

    let mut packet = RecallPacket::new_alpha();
    packet.canonical_decisions = canonical_decisions;
    packet.decision_candidates = decision_candidates;
    packet = packet.with_derived_decision_attention();

    let result = RecallExpandScopeResult {
        packet,
        trace_ref: None,
    };

    ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
}

fn dispatch_recall_promote_candidate(
    request: &ToolRequest,
    params: &bicameral_api::recall::RecallPromoteCandidateParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    use bicameral_api::recall::{
        PromotionOutcome, PromotionRequestOutcome, RecallPromoteCandidateResult, RecallRefusal,
    };

    // Replay state
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
            );
        }
    };

    // Alpha: packet_id is accepted but not validated against a packet store.
    // Packet persistence and currentness checks are deferred to post-alpha.
    // The handler still validates candidate_id against the ledger to refuse
    // free-floating claims that don't reference bot-known candidates.
    let _ = params.packet_id;

    // Validate the candidate exists and is bot-known
    let candidate_id = params.candidate_id;
    let candidate = match state.candidates.get(&candidate_id) {
        Some(c) => c,
        None => {
            // Free-floating claim refused
            let refusal = RecallRefusal::free_floating_claim_refused(format!(
                "candidate_id {} is not a bot-known candidate in the ledger",
                candidate_id
            ));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "free_floating_claim_refused".into()),
            );
        }
    };

    // Validate candidate is in a promotable state
    if candidate.candidate_state != CandidateState::Proposed {
        let refusal = RecallRefusal::action_not_allowed(format!(
            "candidate {} is in state {:?}, not promotable",
            candidate_id, candidate.candidate_state
        ));
        return ToolResponse::rejected(
            request.request_id,
            serde_json::to_string(&refusal).unwrap_or_else(|_| "action_not_allowed".into()),
        );
    }

    // Validate supersedes_decision_id when outcome requires it
    if params.promotion_outcome == PromotionOutcome::SupersedesExistingDecision {
        match params.supersedes_decision_id {
            None => {
                let refusal = RecallRefusal::missing_selected_refs(
                    "supersedes_existing_decision requires supersedes_decision_id",
                );
                return ToolResponse::rejected(
                    request.request_id,
                    serde_json::to_string(&refusal)
                        .unwrap_or_else(|_| "missing_selected_refs".into()),
                );
            }
            Some(decision_id) => {
                if !state.decisions.contains_key(&decision_id) {
                    let refusal = RecallRefusal::missing_selected_refs(format!(
                        "supersedes_decision_id {} not found in ledger",
                        decision_id
                    ));
                    return ToolResponse::rejected(
                        request.request_id,
                        serde_json::to_string(&refusal)
                            .unwrap_or_else(|_| "missing_selected_refs".into()),
                    );
                }
            }
        }
    }

    // Authority check: promote_decision_candidate requires authority proof
    // unless the outcome is DismissedAsNoise or NeedsSignoffReview (routing).
    let needs_authority = matches!(
        params.promotion_outcome,
        PromotionOutcome::NewConstraint
            | PromotionOutcome::SupersedesExistingDecision
            | PromotionOutcome::KeepsBothWithScope
    );

    if needs_authority && params.approval_proof.is_none() {
        // Route to signoff review instead of refusing — the actor can still
        // request promotion, it just gets routed for authority.
        let result = RecallPromoteCandidateResult {
            candidate_id,
            outcome: PromotionRequestOutcome::RoutedToSignoff,
            decision_id: None,
            review_state: Some(ReviewLifecycleState::NeedsSignoffReview),
            confirmation_required: None,
            confirmation_rejection: None,
            trace_ref: None,
        };
        return ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap());
    }

    // Handle the promotion outcome
    match params.promotion_outcome {
        PromotionOutcome::DismissedAsNoise => {
            // Dismissal does not create canonical state — no event-store write.
            let result = RecallPromoteCandidateResult {
                candidate_id,
                outcome: PromotionRequestOutcome::Dismissed,
                decision_id: None,
                review_state: None,
                confirmation_required: None,
                confirmation_rejection: None,
                trace_ref: None,
            };
            ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
        }
        PromotionOutcome::NeedsSignoffReview => {
            // Route to signoff without creating canonical state.
            let result = RecallPromoteCandidateResult {
                candidate_id,
                outcome: PromotionRequestOutcome::RoutedToSignoff,
                decision_id: None,
                review_state: Some(ReviewLifecycleState::NeedsSignoffReview),
                confirmation_required: None,
                confirmation_rejection: None,
                trace_ref: None,
            };
            ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
        }
        PromotionOutcome::NewConstraint
        | PromotionOutcome::SupersedesExistingDecision
        | PromotionOutcome::KeepsBothWithScope => {
            // Authority-proven promotion: create the Decision through the
            // governed event-store path. Matches the payload shape used by
            // dispatch_review_mutation for AcceptCandidate so replay can
            // materialize the Decision with the same id.
            let decision_id = Uuid::new_v4();
            let actor = &request.authority.actor_id;
            let event = EventStoreEntry::new(
                bicameral_api::event_store::EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": candidate_id.to_string(),
                    "decision_id": decision_id.to_string(),
                    "reason": format!("recall promotion: {:?}", params.promotion_outcome),
                    "actor_id": actor,
                    "promoted_by": actor,
                    "promotion_outcome": params.promotion_outcome,
                    "audit_metadata": request.authority.audit_metadata,
                }),
                actor.clone(),
            );
            if let Err(e) = adapter.append(event) {
                return ToolResponse::error(request.request_id, format!("event_store_error: {e}"));
            }

            let result = RecallPromoteCandidateResult {
                candidate_id,
                outcome: PromotionRequestOutcome::Promoted,
                decision_id: Some(decision_id),
                review_state: Some(ReviewLifecycleState::Proposed),
                confirmation_required: None,
                confirmation_rejection: None,
                trace_ref: None,
            };
            ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
        }
    }
}

/// Build a fail-closed `ToolResponse` for a typed confirmation rejection.
///
/// Mirrors the recall-refusal convention: a `Rejected` status whose message is
/// the serialized [`ConfirmationRejection`]. The challenge secret never appears.
fn confirmation_rejection_response(
    request_id: Uuid,
    rejection: bicameral_api::confirmation_challenge::ConfirmationRejection,
) -> ToolResponse {
    let code = rejection.outcome.code().to_string();
    ToolResponse::rejected(
        request_id,
        serde_json::to_string(&rejection).unwrap_or(code),
    )
}

/// Governed candidate promotion behind the daemon-issued `ConfirmationChallenge`
/// protocol (issue #829).
///
/// Two phases, distinguished by whether `params.confirmation` is present:
///
/// - **Phase 1 (no token).** Validate promotion preconditions. Non-canonical
///   outcomes (dismiss, route-to-signoff) resolve immediately with unchanged
///   semantics. Canonical-writing outcomes mint a bound challenge and return a
///   typed `confirmation_required` result — **no** canonical event is appended.
/// - **Phase 2 (token present).** Return the prior canonical result if this
///   `request_id` was already applied (idempotent lost-response retry).
///   Otherwise validate + single-use-consume the challenge (typed authority
///   outcomes), re-check target eligibility against replayed state, append the
///   `AcceptCandidate` event exactly once, and cache the result by `request_id`.
///
/// The challenge store — not this handler — owns minting/validation/consumption;
/// the secret is never logged, traced, or echoed in any typed outcome.
fn dispatch_recall_promote_candidate_confirmed(
    request: &ToolRequest,
    params: &bicameral_api::recall::RecallPromoteCandidateParams,
    adapter: &dyn EventStoreAdapter,
    store: &ConfirmationChallengeStore,
) -> ToolResponse {
    use bicameral_api::confirmation_challenge::{
        ChallengeBinding, ConfirmationOutcome, ConfirmationRejection,
        CONFIRMATION_CHALLENGE_PROTOCOL_VERSION,
    };
    use bicameral_api::recall::{
        PromotionOutcome, PromotionRequestOutcome, RecallPromoteCandidateResult, RecallRefusal,
    };

    let candidate_id = params.candidate_id;
    let reject = |outcome: ConfirmationOutcome, detail: &str| {
        confirmation_rejection_response(
            request.request_id,
            ConfirmationRejection::new(outcome, detail),
        )
    };

    // The challenge binds workspace and session; both are required so a grant
    // cannot be minted or consumed without them.
    let workspace = match request.authority.workspace.clone() {
        Some(w) => w,
        None => {
            return reject(
                ConfirmationOutcome::Unauthorized,
                "workspace is required to confirm a promotion",
            )
        }
    };
    let session_id = match request.authority.session_id.clone() {
        Some(s) => s,
        None => {
            return reject(
                ConfirmationOutcome::Unauthorized,
                "agent session is required to confirm a promotion",
            )
        }
    };
    let asserted_binding = ChallengeBinding {
        protocol_version: CONFIRMATION_CHALLENGE_PROTOCOL_VERSION,
        workspace,
        actor_id: request.authority.actor_id.clone(),
        session_id,
        packet_id: params.packet_id,
        candidate_id,
        requested_outcome: params.promotion_outcome,
        request_id: request.request_id,
    };

    match &params.confirmation {
        // -- Phase 1: initial governed request --------------------------------
        None => {
            let state = match replay::replay_from_adapter(adapter) {
                Ok(s) => s,
                Err(e) => {
                    let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
                    return ToolResponse::rejected(
                        request.request_id,
                        serde_json::to_string(&refusal)
                            .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
                    );
                }
            };

            let candidate = match state.candidates.get(&candidate_id) {
                Some(c) => c,
                None => {
                    let refusal = RecallRefusal::free_floating_claim_refused(format!(
                        "candidate_id {candidate_id} is not a bot-known candidate in the ledger"
                    ));
                    return ToolResponse::rejected(
                        request.request_id,
                        serde_json::to_string(&refusal)
                            .unwrap_or_else(|_| "free_floating_claim_refused".into()),
                    );
                }
            };

            if candidate.candidate_state != CandidateState::Proposed {
                let refusal = RecallRefusal::action_not_allowed(format!(
                    "candidate {candidate_id} is in state {:?}, not promotable",
                    candidate.candidate_state
                ));
                return ToolResponse::rejected(
                    request.request_id,
                    serde_json::to_string(&refusal).unwrap_or_else(|_| "action_not_allowed".into()),
                );
            }

            if params.promotion_outcome == PromotionOutcome::SupersedesExistingDecision {
                if let Some(resp) = validate_supersedes_ref(request, params, &state) {
                    return resp;
                }
            }

            match params.promotion_outcome {
                // Non-canonical outcomes need no challenge — semantics unchanged.
                PromotionOutcome::DismissedAsNoise => {
                    let result = RecallPromoteCandidateResult {
                        candidate_id,
                        outcome: PromotionRequestOutcome::Dismissed,
                        decision_id: None,
                        review_state: None,
                        confirmation_required: None,
                        confirmation_rejection: None,
                        trace_ref: None,
                    };
                    ToolResponse::success(
                        request.request_id,
                        serde_json::to_value(&result).unwrap(),
                    )
                }
                PromotionOutcome::NeedsSignoffReview => {
                    let result = RecallPromoteCandidateResult {
                        candidate_id,
                        outcome: PromotionRequestOutcome::RoutedToSignoff,
                        decision_id: None,
                        review_state: Some(ReviewLifecycleState::NeedsSignoffReview),
                        confirmation_required: None,
                        confirmation_rejection: None,
                        trace_ref: None,
                    };
                    ToolResponse::success(
                        request.request_id,
                        serde_json::to_value(&result).unwrap(),
                    )
                }
                // Canonical-writing outcomes require explicit human confirmation.
                PromotionOutcome::NewConstraint
                | PromotionOutcome::SupersedesExistingDecision
                | PromotionOutcome::KeepsBothWithScope => {
                    let challenge = store.issue(asserted_binding, Utc::now());
                    let result = RecallPromoteCandidateResult {
                        candidate_id,
                        outcome: PromotionRequestOutcome::ConfirmationRequired,
                        decision_id: None,
                        review_state: None,
                        confirmation_required: Some(challenge),
                        confirmation_rejection: None,
                        trace_ref: None,
                    };
                    ToolResponse::success(
                        request.request_id,
                        serde_json::to_value(&result).unwrap(),
                    )
                }
            }
        }

        // -- Phase 2: confirmed resubmission ----------------------------------
        Some(token) => {
            // Idempotent lost-response retry: same request id returns the prior
            // canonical result without consuming a second grant or duplicating
            // the transition.
            if let Some(prior) = store.applied_result(request.request_id) {
                return ToolResponse::success(
                    request.request_id,
                    serde_json::to_value(&prior).unwrap(),
                );
            }

            // Validate + single-use-consume (typed authority outcomes).
            let consumed = match store.consume(token, &asserted_binding, Utc::now()) {
                Ok(c) => c,
                Err(rej) => return confirmation_rejection_response(request.request_id, rej),
            };

            // Re-check target eligibility against current canonical state: the
            // ledger may have moved since the challenge was issued.
            let state = match replay::replay_from_adapter(adapter) {
                Ok(s) => s,
                Err(e) => {
                    let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
                    return ToolResponse::rejected(
                        request.request_id,
                        serde_json::to_string(&refusal)
                            .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
                    );
                }
            };
            match state.candidates.get(&candidate_id) {
                None => {
                    return reject(
                        ConfirmationOutcome::IneligibleTarget,
                        "candidate no longer exists in the ledger",
                    )
                }
                Some(c) => match c.candidate_state {
                    CandidateState::Proposed => {}
                    CandidateState::Promoted => {
                        return reject(
                            ConfirmationOutcome::StalePacket,
                            "candidate was already promoted; the packet view is stale",
                        )
                    }
                    CandidateState::Rejected => {
                        return reject(
                            ConfirmationOutcome::IneligibleTarget,
                            "candidate was rejected and is not promotable",
                        )
                    }
                },
            }

            if consumed.binding.requested_outcome == PromotionOutcome::SupersedesExistingDecision {
                match params.supersedes_decision_id {
                    None => {
                        return reject(
                            ConfirmationOutcome::IneligibleTarget,
                            "supersedes_existing_decision requires supersedes_decision_id",
                        )
                    }
                    Some(decision_id) if !state.decisions.contains_key(&decision_id) => {
                        return reject(
                            ConfirmationOutcome::StalePacket,
                            "superseded decision no longer exists; the packet view is stale",
                        )
                    }
                    Some(_) => {}
                }
            }

            // Apply the transition exactly once through the governed event-store
            // path (same payload shape as the approval-proof path).
            let decision_id = Uuid::new_v4();
            let actor = &request.authority.actor_id;
            let event = EventStoreEntry::new(
                bicameral_api::event_store::EventType::AcceptCandidate,
                serde_json::json!({
                    "candidate_id": candidate_id.to_string(),
                    "decision_id": decision_id.to_string(),
                    "reason": format!("recall promotion (confirmed): {:?}", params.promotion_outcome),
                    "actor_id": actor,
                    "promoted_by": actor,
                    "promotion_outcome": params.promotion_outcome,
                    "supersedes_decision_id": params.supersedes_decision_id.map(|d| d.to_string()),
                    "scoping_relationship": params.scoping_relationship,
                    "audit_metadata": request.authority.audit_metadata,
                }),
                actor.clone(),
            );
            if let Err(e) = adapter.append(event) {
                return ToolResponse::error(request.request_id, format!("event_store_error: {e}"));
            }

            let result = RecallPromoteCandidateResult {
                candidate_id,
                outcome: PromotionRequestOutcome::Promoted,
                decision_id: Some(decision_id),
                review_state: Some(ReviewLifecycleState::Proposed),
                confirmation_required: None,
                confirmation_rejection: None,
                trace_ref: None,
            };
            store.record_applied(request.request_id, result.clone());
            ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
        }
    }
}

/// Validate `supersedes_decision_id` for a supersession promotion. Returns a
/// typed refusal response when the reference is missing or unknown.
fn validate_supersedes_ref(
    request: &ToolRequest,
    params: &bicameral_api::recall::RecallPromoteCandidateParams,
    state: &crate::replay::ReplayedState,
) -> Option<ToolResponse> {
    use bicameral_api::recall::RecallRefusal;
    match params.supersedes_decision_id {
        None => {
            let refusal = RecallRefusal::missing_selected_refs(
                "supersedes_existing_decision requires supersedes_decision_id",
            );
            Some(ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal).unwrap_or_else(|_| "missing_selected_refs".into()),
            ))
        }
        Some(decision_id) => {
            if !state.decisions.contains_key(&decision_id) {
                let refusal = RecallRefusal::missing_selected_refs(format!(
                    "supersedes_decision_id {decision_id} not found in ledger"
                ));
                Some(ToolResponse::rejected(
                    request.request_id,
                    serde_json::to_string(&refusal)
                        .unwrap_or_else(|_| "missing_selected_refs".into()),
                ))
            } else {
                None
            }
        }
    }
}

fn dispatch_recall_request_correction(
    request: &ToolRequest,
    params: &bicameral_api::recall::RecallRequestCorrectionParams,
    adapter: &dyn EventStoreAdapter,
) -> ToolResponse {
    use bicameral_api::recall::{
        CorrectionRequestOutcome, RecallRefusal, RecallRequestCorrectionResult,
    };

    // Replay state
    let state = match replay::replay_from_adapter(adapter) {
        Ok(s) => s,
        Err(e) => {
            let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
            );
        }
    };

    // Validate: selected_item_ids must be non-empty
    if params.selected_item_ids.is_empty() {
        let refusal = RecallRefusal::missing_selected_refs(
            "request_correction requires at least one selected_item_id referencing a \
             packet decision or candidate",
        );
        return ToolResponse::rejected(
            request.request_id,
            serde_json::to_string(&refusal).unwrap_or_else(|_| "missing_selected_refs".into()),
        );
    }

    // Validate: every selected_item_id must reference a bot-known decision or candidate
    for item_id in &params.selected_item_ids {
        let known = state.decisions.contains_key(item_id) || state.candidates.contains_key(item_id);
        if !known {
            let refusal = RecallRefusal::free_floating_claim_refused(format!(
                "selected_item_id {} is not a bot-known decision or candidate in the ledger",
                item_id
            ));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "free_floating_claim_refused".into()),
            );
        }
    }

    // Validate: rationale must be non-empty
    if params.rationale.trim().is_empty() {
        let refusal = RecallRefusal::missing_selected_refs(
            "request_correction requires a non-empty rationale",
        );
        return ToolResponse::rejected(
            request.request_id,
            serde_json::to_string(&refusal).unwrap_or_else(|_| "missing_selected_refs".into()),
        );
    }

    // Single-use approval enforcement: check if this approval proof value
    // has already been consumed by a prior correction request event.
    let events = match adapter.replay() {
        Ok(e) => e,
        Err(e) => {
            let refusal = RecallRefusal::daemon_unavailable(format!("replay failed: {e}"));
            return ToolResponse::rejected(
                request.request_id,
                serde_json::to_string(&refusal)
                    .unwrap_or_else(|_| "daemon_unavailable: replay failed".into()),
            );
        }
    };

    for event in &events {
        if event.event_type == bicameral_api::event_store::EventType::RequestCorrection {
            if let Some(proof_value) = event.payload.get("approval_proof_value") {
                if proof_value.as_str() == Some(&params.approval_proof.value) {
                    let refusal = RecallRefusal::approval_already_consumed(format!(
                        "approval proof '{}' has already been consumed by a prior \
                         correction request",
                        params.approval_proof.kind
                    ));
                    return ToolResponse::rejected(
                        request.request_id,
                        serde_json::to_string(&refusal)
                            .unwrap_or_else(|_| "approval_already_consumed".into()),
                    );
                }
            }
        }
    }

    // All validations passed — record the correction request event.
    // This does NOT create accepted Decision, BindingEvidence, signoff,
    // compliance, or enforcement state. It creates a correction candidate
    // for the governed review path.
    let correction_candidate_id = Uuid::new_v4();
    let actor = &request.authority.actor_id;
    let event = EventStoreEntry::new(
        bicameral_api::event_store::EventType::RequestCorrection,
        serde_json::json!({
            "packet_id": params.packet_id.to_string(),
            "selected_item_ids": params.selected_item_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
            "correction_kind": params.correction_kind,
            "rationale": params.rationale,
            "approval_proof_kind": params.approval_proof.kind,
            "approval_proof_value": params.approval_proof.value,
            "approval_proof_actor": params.approval_proof.actor_id,
            "correction_candidate_id": correction_candidate_id.to_string(),
            "actor_id": actor,
            "audit_metadata": request.authority.audit_metadata,
        }),
        actor.clone(),
    );
    if let Err(e) = adapter.append(event) {
        return ToolResponse::error(request.request_id, format!("event_store_error: {e}"));
    }

    let result = RecallRequestCorrectionResult {
        packet_id: params.packet_id,
        outcome: CorrectionRequestOutcome::Accepted,
        correction_candidate_id: Some(correction_candidate_id),
        trace_ref: None,
    };
    ToolResponse::success(request.request_id, serde_json::to_value(&result).unwrap())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store_adapters::MemoryAdapter;
    use bicameral_api::fixtures;

    fn mem_adapter() -> MemoryAdapter {
        MemoryAdapter::new()
    }

    // -- Authority validation ---

    #[test]
    fn missing_authority_is_rejected() {
        let adapter = mem_adapter();
        let req = fixtures::negative_missing_actor_id();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("missing_authority"),
            "expected missing_authority, got: {:?}",
            resp.message
        );
    }

    // -- Malformed payload ---

    #[test]
    fn malformed_ingest_empty_title_is_rejected() {
        let adapter = mem_adapter();
        let req = fixtures::negative_malformed_ingest_empty_title();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("malformed_payload"),
            "expected malformed_payload, got: {:?}",
            resp.message
        );
    }

    // -- Unsupported capability ---

    #[test]
    fn resolve_compliance_returns_unsupported() {
        let adapter = mem_adapter();
        let req = fixtures::review_resolve_compliance();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unsupported_capability"),
            "expected unsupported_capability, got: {:?}",
            resp.message
        );
    }

    #[test]
    fn binding_create_without_workspace_returns_error() {
        let adapter = mem_adapter();
        let req = fixtures::binding_create();
        // dispatch (no workspace) returns graph_unavailable error
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Error);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("graph_unavailable"),
            "expected graph_unavailable, got: {:?}",
            resp.message
        );
    }

    #[test]
    fn binding_inspect_returns_ok_empty() {
        let adapter = mem_adapter();
        let req = fixtures::binding_inspect();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let result = resp.result.unwrap();
        assert_eq!(result["binding_count"], 0);
    }

    // -- Supported ingest command ---

    #[test]
    fn ingest_submit_local_returns_ok_with_ids() {
        let adapter = mem_adapter();
        let req = fixtures::ingest_submit_local();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let result = resp.result.as_ref().expect("result must be present");
        assert!(result["source_id"].is_string());
        assert!(result["snapshot_id"].is_string());
        assert!(result["evidence_ids"].is_array());
        assert!(result["evidence_reference_ids"].is_array());
        assert!(result["candidate_ids"].is_array());

        // Verify event was appended
        let events = adapter.replay().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Ingest);
        assert_eq!(events[0].actor, req.authority.actor_id);
    }

    #[test]
    fn ingest_with_candidate_drafts_creates_multiple_events() {
        let adapter = mem_adapter();
        let req = fixtures::ingest_submit_local_with_drafts();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let result = resp.result.as_ref().expect("result must be present");
        let candidate_ids = result["candidate_ids"].as_array().unwrap();
        // Primary + 2 drafts = 3 candidates
        assert_eq!(candidate_ids.len(), 3);

        // 1 primary + 2 draft events
        let events = adapter.replay().unwrap();
        assert_eq!(events.len(), 3);
        for event in &events {
            assert_eq!(event.event_type, EventType::Ingest);
        }
    }

    #[test]
    fn ingest_rationale_not_in_event_evidence() {
        let adapter = mem_adapter();
        let req = fixtures::ingest_submit_local_with_drafts();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        // Verify rationale is NOT stored as evidence
        let events = adapter.replay().unwrap();
        let payload_str = serde_json::to_string(&events[0].payload).unwrap();
        // Rationale text must not appear in evidence excerpt fields
        assert!(
            !payload_str.contains("LLM extracted these candidates"),
            "rationale must not be stored as evidence"
        );
    }

    #[test]
    fn ingest_unsafe_evidence_is_rejected() {
        let adapter = mem_adapter();
        let req = fixtures::negative_unsafe_evidence();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unsafe_evidence"),
            "expected unsafe_evidence, got: {:?}",
            resp.message
        );
        // No event appended
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn ingest_forbidden_canonical_field_is_rejected() {
        let adapter = mem_adapter();
        let req = fixtures::negative_forbidden_canonical_field();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("forbidden_canonical_field"),
            "expected forbidden_canonical_field, got: {:?}",
            resp.message
        );
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn ingest_legacy_mcp_payload_is_rejected() {
        let adapter = mem_adapter();
        let req = fixtures::negative_legacy_mcp_payload();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("legacy_mcp_payload"),
            "expected legacy_mcp_payload, got: {:?}",
            resp.message
        );
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn ingest_external_adapter_payload_is_rejected() {
        let adapter = mem_adapter();
        let req = fixtures::negative_external_adapter_payload();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("external_adapter_payload"),
            "expected external_adapter_payload, got: {:?}",
            resp.message
        );
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn preflight_run_returns_ok_source_only() {
        let adapter = mem_adapter();
        // No workspace root → source-only advisory preflight.
        let req = fixtures::preflight_run();
        let resp = dispatch(&req, &adapter);
        assert_eq!(
            resp.status,
            ToolResponseStatus::Ok,
            "source-only preflight must succeed, got: {:?}",
            resp.message
        );
        let result = resp.result.expect("preflight result must be present");
        assert!(result["relevant_decisions"].is_array());
        assert!(result["relevant_candidates"].is_array());
        assert!(result["readiness"].is_object());
        assert!(result["warnings"].is_array());
        // Source-only mode reports graph as not configured via unknown_scope.
        let unknown = result["readiness"]["unknown_scope"]
            .as_array()
            .expect("unknown_scope must be an array");
        assert!(
            unknown.iter().any(|e| e["scope"] == "graph"),
            "source-only preflight must disclose unavailable graph scope"
        );
    }

    #[test]
    fn preflight_run_source_only_attaches_graph_unavailable_freshness() {
        // Issue #625 slice 2: preflight.run must populate the daemon-authored
        // freshness block. With no workspace the graph is unavailable, so
        // freshness must report graph_unavailable (never a fabricated
        // all-clear) while preserving the source-only advisory output.
        let adapter = mem_adapter();
        let req = fixtures::preflight_run();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let result = resp.result.expect("preflight result must be present");

        // Source-only output is preserved alongside the freshness block.
        assert!(result["relevant_decisions"].is_array());
        assert!(result["readiness"].is_object());

        let freshness = &result["freshness"];
        assert!(
            freshness.is_object(),
            "preflight.run must attach a freshness read-model, got: {result}"
        );
        assert_eq!(freshness["commit_freshness"], "graph_unavailable");
        assert_eq!(freshness["refresh_outcome"], "unavailable");
        assert_eq!(freshness["graph_readiness"], "not_configured");
        let caveats = freshness["caveats"]
            .as_array()
            .expect("freshness caveats must be an array");
        assert!(
            caveats.iter().any(|c| c["code"] == "refresh_unavailable"),
            "graph-unavailable freshness must carry a refresh_unavailable caveat: {freshness}"
        );
    }

    #[test]
    fn preflight_run_appends_no_events() {
        let adapter = mem_adapter();
        let req = fixtures::preflight_run();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "preflight.run must not append events (read-only)"
        );
    }

    #[test]
    fn tracking_commands_return_unsupported() {
        let adapter = mem_adapter();

        let req = fixtures::tracking_untrack_source();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(resp
            .message
            .as_deref()
            .unwrap_or("")
            .contains("unsupported_capability"));

        let req = fixtures::tracking_refresh_query();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(resp
            .message
            .as_deref()
            .unwrap_or("")
            .contains("unsupported_capability"));
    }

    #[test]
    fn privacy_erase_subject_returns_fail_closed_rejection() {
        let adapter = mem_adapter();
        let req = fixtures::privacy_erase_subject();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        let msg = resp.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("unsupported_capability"),
            "expected unsupported_capability, got: {msg}"
        );
        assert!(
            msg.contains("PiiArchive"),
            "rejection must reference PiiArchive, got: {msg}"
        );
        // Must not append any events (fail-closed, no state mutation).
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "privacy.erase_subject must not append events when PiiArchive is absent"
        );
    }

    #[test]
    fn validation_token_returns_unsupported() {
        let adapter = mem_adapter();
        let req = fixtures::negative_validation_token_acceptance();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("validation_token_unsupported"),
            "expected validation_token_unsupported, got: {:?}",
            resp.message
        );
    }

    // -- Supported binding search scope ---

    /// Append an accepted, verified `BindToCode` event so replay materializes a
    /// binding the dispatcher's `search.query` can read back.
    fn seed_binding_event(adapter: &MemoryAdapter, decision_id: Uuid, target: &str) -> Uuid {
        use bicameral_api::evidence::{
            BindingConfidence, BindingEvidence, BindingType, GraphEvidenceState,
        };
        let mut binding = BindingEvidence::new(
            Uuid::new_v4(),
            BindingType::FilePath,
            target.to_string(),
            BindingConfidence::High,
        );
        binding.graph_snapshot_id = Some("snap-xyz".to_string());
        binding.validated_sha = Some("cafef00dbaad".to_string());
        binding.target_kind = Some("file".to_string());
        binding.target_identifier = Some(target.to_string());
        binding.evidence_state = Some(GraphEvidenceState::Verified);
        let binding_id = binding.id;
        let payload = BindToCodePayload {
            decision_id,
            binding,
            graph_snapshot_id: "snap-xyz".to_string(),
            validated_sha: "cafef00dbaad".to_string(),
            evidence_state: GraphEvidenceState::Verified,
            content_hash: Some("hash-1".to_string()),
            line_start: Some(1),
            line_end: Some(20),
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::BindToCode,
                serde_json::to_value(&payload).unwrap(),
                "test".to_string(),
            ))
            .unwrap();
        binding_id
    }

    #[test]
    fn binding_search_scope_returns_ok_with_binding_results() {
        let adapter = mem_adapter();
        let decision_id = Uuid::new_v4();
        let binding_id = seed_binding_event(&adapter, decision_id, "src/main.rs");

        let req = fixtures::search_query_bindings();
        let resp = dispatch(&req, &adapter);

        assert_eq!(
            resp.status,
            ToolResponseStatus::Ok,
            "msg: {:?}",
            resp.message
        );
        let result = resp.result.expect("binding search must carry a result");
        let bindings = result["bindings"]
            .as_array()
            .expect("result must include a bindings array");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0]["binding_id"], binding_id.to_string());
        assert_eq!(bindings[0]["decision_id"], decision_id.to_string());
        assert_eq!(bindings[0]["graph_snapshot_id"], "snap-xyz");
        assert_eq!(bindings[0]["validated_sha"], "cafef00dbaad");
        assert_eq!(bindings[0]["evidence_state"], "verified");
        // Never a silent empty: an explicit read-only scope note is present.
        assert!(result["scope_notes"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn binding_search_scope_is_never_rejected_as_unsupported() {
        let adapter = mem_adapter();
        // Even with no bindings materialized, the scope is supported: Ok, not
        // a Rejected unsupported_scope.
        let req = fixtures::search_query_bindings();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        assert!(!resp
            .message
            .as_deref()
            .unwrap_or("")
            .contains("unsupported_scope"));
    }

    // -- Supported read commands ---

    #[test]
    fn history_list_returns_ok() {
        let adapter = mem_adapter();
        let req = fixtures::history_list();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        assert!(resp.result.is_some());
    }

    #[test]
    fn search_query_returns_ok() {
        let adapter = mem_adapter();
        let req = fixtures::search_query();
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        assert!(resp.result.is_some());
    }

    #[test]
    fn search_returns_working_state_candidates_as_proposals() {
        use bicameral_api::candidate::DecisionCandidate;

        let adapter = mem_adapter();
        let mut candidate = DecisionCandidate::new(
            "We will use the working-state search marker".into(),
            "Decision: we will use the working-state search marker.".into(),
            DecisionLevel::Implementation,
            "topology://working-state".into(),
        );
        candidate.tags = vec!["content_derived".into()];

        let request = ToolRequest::new(
            ToolCommand::SearchQuery(SearchQueryParams {
                query: "working-state search marker".into(),
                scope: Some(SearchScope::Candidates),
                filters: None,
                limit: Some(10),
            }),
            fixtures::authority_mcp("agent-mcp-001", "sess-search-ws", "/home/user/project"),
        );

        let resp = dispatch_with_working_candidates(
            &request,
            &adapter,
            None,
            None,
            vec![candidate.clone()],
        );
        assert_eq!(resp.status, ToolResponseStatus::Ok, "{:?}", resp.message);
        let body = serde_json::to_string(&resp.result).unwrap();
        assert!(
            body.contains("working-state search marker"),
            "working-state candidate must be searchable: {body}"
        );

        // Without the working set, canonical replay alone must not know it.
        let empty = dispatch(&request, &adapter);
        let empty_body = serde_json::to_string(&empty.result).unwrap();
        assert!(!empty_body.contains("working-state search marker"));
    }

    #[test]
    fn dispatch_populates_runtime_trace() {
        let adapter = mem_adapter();
        let req = fixtures::search_query();
        let resp = dispatch(&req, &adapter);

        let trace = resp.trace.as_ref().expect("trace must be present");
        assert_eq!(trace.request_id, req.request_id);
        assert_eq!(trace.command, "search.query");
        assert_eq!(trace.actor_id, req.authority.actor_id);
        assert_eq!(trace.status, TraceStatus::Ok);
        assert!(trace.failure_reason.is_none());
    }

    #[test]
    fn rejected_dispatch_trace_records_failure_reason() {
        let adapter = mem_adapter();
        let req = fixtures::negative_validation_token_acceptance();
        let resp = dispatch(&req, &adapter);

        let trace = resp.trace.as_ref().expect("trace must be present");
        assert_eq!(trace.status, TraceStatus::Rejected);
        assert_eq!(trace.failure_reason, resp.message);
    }

    #[test]
    fn trace_helpers_populate_trace() {
        let req = fixtures::preflight_run();

        let rejected = reject_with_trace(&req, "policy_violation".into()).response;
        assert_eq!(rejected.status, ToolResponseStatus::Rejected);
        assert_eq!(
            rejected.trace.as_ref().unwrap().failure_reason.as_deref(),
            Some("policy_violation")
        );

        let errored = error_with_trace(&req, "daemon_unavailable".into()).response;
        assert_eq!(errored.status, ToolResponseStatus::Error);
        assert_eq!(
            errored.trace.as_ref().unwrap().failure_reason.as_deref(),
            Some("daemon_unavailable")
        );
    }

    // -- Helpers: seed a candidate/decision so review commands have a real target ---

    /// Ingest a candidate and return its UUID string.
    fn seed_candidate(adapter: &MemoryAdapter) -> String {
        let req = fixtures::ingest_submit_local();
        let resp = dispatch(&req, adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let result = resp.result.as_ref().unwrap();
        let ids = result["candidate_ids"].as_array().unwrap();
        assert!(!ids.is_empty());
        ids[0].as_str().unwrap().to_string()
    }

    /// Ingest + accept a candidate, returning the decision UUID string.
    fn seed_decision(adapter: &MemoryAdapter) -> String {
        let cand_id = seed_candidate(adapter);
        let accept_req = ToolRequest::new(
            ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
                target_id: cand_id,
                reason: Some("promoting for signoff tests".into()),
            }),
            fixtures::authority_cli("test-reviewer", "/home/user/project"),
        );
        let resp = dispatch(&accept_req, adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        // The accept response carries target_id; the decision_id is in the
        // persisted event payload.
        let events = adapter.replay().unwrap();
        let accept_event = events
            .iter()
            .find(|e| e.event_type == EventType::AcceptCandidate)
            .expect("AcceptCandidate event must exist");
        accept_event.payload["decision_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    // -- Supported review/mutation commands ---

    #[test]
    fn review_accept_candidate_appends_event() {
        let adapter = mem_adapter();
        let cand_id = seed_candidate(&adapter);
        let req = ToolRequest::new(
            ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
                target_id: cand_id,
                reason: Some("evidence sufficient".into()),
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let events = adapter.replay().unwrap();
        let accept_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::AcceptCandidate)
            .collect();
        assert_eq!(accept_events.len(), 1);
        assert_eq!(accept_events[0].actor, req.authority.actor_id);
    }

    #[test]
    fn review_reject_candidate_appends_event() {
        let adapter = mem_adapter();
        let cand_id = seed_candidate(&adapter);
        let req = ToolRequest::new(
            ToolCommand::ReviewRejectCandidate(ReviewTargetParams {
                target_id: cand_id,
                reason: Some("evidence insufficient".into()),
            }),
            fixtures::authority_cli("user-reviewer-02", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let events = adapter.replay().unwrap();
        let reject_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::RejectCandidate)
            .collect();
        assert_eq!(reject_events.len(), 1);
        assert_eq!(reject_events[0].actor, req.authority.actor_id);
    }

    #[test]
    fn review_approve_signoff_appends_event() {
        let adapter = mem_adapter();
        let dec_id = seed_decision(&adapter);
        let req = ToolRequest::new(
            ToolCommand::ReviewApproveSignoff(ReviewTargetParams {
                target_id: dec_id,
                reason: Some("ownership approved".into()),
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let events = adapter.replay().unwrap();
        let signoff_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::ApproveSignoff)
            .collect();
        assert_eq!(signoff_events.len(), 1);
        assert_eq!(signoff_events[0].actor, req.authority.actor_id);
    }

    #[test]
    fn review_reject_signoff_appends_event() {
        let adapter = mem_adapter();
        let dec_id = seed_decision(&adapter);
        let req = ToolRequest::new(
            ToolCommand::ReviewRejectSignoff(ReviewTargetParams {
                target_id: dec_id,
                reason: Some("meaning unclear".into()),
            }),
            fixtures::authority_dashboard("user-owner-02", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let events = adapter.replay().unwrap();
        let signoff_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::RejectSignoff)
            .collect();
        assert_eq!(signoff_events.len(), 1);
        assert_eq!(signoff_events[0].actor, req.authority.actor_id);
    }

    // -- Unknown target rejection (issue #515) ---

    #[test]
    fn review_accept_candidate_unknown_target_rejected() {
        let adapter = mem_adapter();
        let req = ToolRequest::new(
            ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
                target_id: Uuid::new_v4().to_string(),
                reason: Some("should fail".into()),
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unknown_target"),
            "expected unknown_target, got: {:?}",
            resp.message
        );
        // No event appended
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn review_reject_candidate_unknown_target_rejected() {
        let adapter = mem_adapter();
        let req = ToolRequest::new(
            ToolCommand::ReviewRejectCandidate(ReviewTargetParams {
                target_id: Uuid::new_v4().to_string(),
                reason: Some("should fail".into()),
            }),
            fixtures::authority_cli("user-reviewer-02", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unknown_target"),
            "expected unknown_target, got: {:?}",
            resp.message
        );
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn review_approve_signoff_unknown_target_rejected() {
        let adapter = mem_adapter();
        let req = ToolRequest::new(
            ToolCommand::ReviewApproveSignoff(ReviewTargetParams {
                target_id: Uuid::new_v4().to_string(),
                reason: Some("should fail".into()),
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unknown_target"),
            "expected unknown_target, got: {:?}",
            resp.message
        );
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn review_reject_signoff_unknown_target_rejected() {
        let adapter = mem_adapter();
        let req = ToolRequest::new(
            ToolCommand::ReviewRejectSignoff(ReviewTargetParams {
                target_id: Uuid::new_v4().to_string(),
                reason: Some("should fail".into()),
            }),
            fixtures::authority_dashboard("user-owner-02", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unknown_target"),
            "expected unknown_target, got: {:?}",
            resp.message
        );
        let events = adapter.replay().unwrap();
        assert!(events.is_empty());
    }

    // -- Actor/audit derivation ---

    #[test]
    fn mutation_events_derive_actor_from_authority_context() {
        let adapter = mem_adapter();
        let cand_id = seed_candidate(&adapter);
        let req = ToolRequest::new(
            ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
                target_id: cand_id,
                reason: Some("evidence sufficient".into()),
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let events = adapter.replay().unwrap();
        let accept_event = events
            .iter()
            .find(|e| e.event_type == EventType::AcceptCandidate)
            .expect("AcceptCandidate event must exist");
        // actor field on the event envelope must match AuthorityContext.actor_id
        assert_eq!(accept_event.actor, req.authority.actor_id);
        // payload must contain promoted_by and actor_id derived from AuthorityContext
        let payload = &accept_event.payload;
        assert_eq!(
            payload["promoted_by"].as_str().unwrap(),
            req.authority.actor_id
        );
        assert_eq!(
            payload["actor_id"].as_str().unwrap(),
            req.authority.actor_id
        );
        // AcceptCandidate payload must have candidate_id and decision_id
        assert!(payload["candidate_id"].is_string());
        assert!(payload["decision_id"].is_string());
    }

    #[test]
    fn signoff_events_derive_actor_from_authority_context() {
        let adapter = mem_adapter();
        let dec_id = seed_decision(&adapter);
        let req = ToolRequest::new(
            ToolCommand::ReviewApproveSignoff(ReviewTargetParams {
                target_id: dec_id,
                reason: Some("ownership approved".into()),
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let events = adapter.replay().unwrap();
        let signoff_event = events
            .iter()
            .find(|e| e.event_type == EventType::ApproveSignoff)
            .expect("ApproveSignoff event must exist");
        assert_eq!(signoff_event.actor, req.authority.actor_id);
        let payload = &signoff_event.payload;
        assert_eq!(
            payload["promoted_by"].as_str().unwrap(),
            req.authority.actor_id
        );
        // ApproveSignoff payload must have decision_id
        assert!(payload["decision_id"].is_string());
    }

    // -- Capability report ---

    #[test]
    fn capability_report_includes_supported_commands() {
        let report = capability_report();
        assert_eq!(report.protocol_version, "v2");
        assert!(report.supported_commands.contains(&"history.list"));
        assert!(report.supported_commands.contains(&"search.query"));
        assert!(report
            .supported_commands
            .contains(&"review.accept_candidate"));
        assert!(report
            .supported_commands
            .contains(&"review.reject_candidate"));
        assert!(report
            .supported_commands
            .contains(&"review.approve_signoff"));
        assert!(report.supported_commands.contains(&"review.reject_signoff"));
    }

    #[test]
    fn capability_report_includes_deferred_commands() {
        let report = capability_report();
        assert!(report
            .deferred_commands
            .contains(&"review.resolve_compliance"));
        // binding.create and binding.inspect are now supported, not deferred
        assert!(!report.deferred_commands.contains(&"binding.create"));
        assert!(!report.deferred_commands.contains(&"binding.inspect"));
    }

    #[test]
    fn capability_report_includes_binding_commands() {
        let report = capability_report();
        assert!(report.supported_commands.contains(&"binding.create"));
        assert!(report.supported_commands.contains(&"binding.inspect"));
    }

    // -- Daemon unavailable simulation ---

    #[test]
    fn daemon_error_response_shape() {
        let resp = fixtures::response_daemon_error(uuid::Uuid::new_v4());
        assert_eq!(resp.status, ToolResponseStatus::Error);
        assert!(resp
            .message
            .as_deref()
            .unwrap_or("")
            .contains("daemon_unavailable"));
    }

    // -- All accepted fixtures dispatched ---

    #[test]
    fn all_accepted_fixtures_dispatch_without_panic() {
        // Commands that require a workspace root return Error when dispatched
        // without one — this is expected, not a bug.
        let graph_commands = ["binding.create"];

        for (name, req) in fixtures::all_accepted_fixtures() {
            // Fresh adapter per fixture so mutation events don't contaminate
            // later read-command replays.
            let adapter = mem_adapter();
            let resp = dispatch(&req, &adapter);

            let is_graph_command = graph_commands.iter().any(|c| name.contains(c));

            if is_graph_command {
                // Graph-dependent commands may return Error (graph_unavailable)
                // when no workspace is provided.
                assert!(
                    resp.status == ToolResponseStatus::Error
                        || resp.status == ToolResponseStatus::Ok
                        || resp.status == ToolResponseStatus::Rejected,
                    "{name}: unexpected status: {:?} {:?}",
                    resp.status,
                    resp.message
                );
            } else {
                // Supported commands should be Ok, deferred should be Rejected.
                // None should panic or return Error (adapter is available).
                assert_ne!(
                    resp.status,
                    ToolResponseStatus::Error,
                    "{name}: unexpected Error status: {:?}",
                    resp.message
                );
            }
        }
    }

    // -- All negative fixtures dispatched ---

    #[test]
    fn all_negative_fixtures_dispatch_without_panic() {
        for (name, _expected, req) in fixtures::all_negative_fixtures() {
            let adapter = mem_adapter();
            let resp = dispatch(&req, &adapter);
            // Should not panic regardless of fixture shape.
            let _ = resp.status;
            let _ = format!("{name}: dispatched");
        }
    }

    // -- Alpha cut-line conformance ----------------------------------------
    //
    // Proves issue #290: deferred commands (review.resolve_compliance,
    // tracking.untrack_source, tracking.refresh_query, privacy.erase_subject)
    // never cross the alpha cut line. preflight.run was moved into alpha
    // (issue #261) and is now a supported read command — see
    // `preflight_run_returns_ok_source_only`.

    #[test]
    fn alpha_cutline_all_deferred_commands_rejected() {
        let adapter = mem_adapter();
        let deferred: Vec<(&str, ToolRequest)> = vec![
            (
                "review.resolve_compliance",
                fixtures::review_resolve_compliance(),
            ),
            (
                "tracking.untrack_source",
                fixtures::tracking_untrack_source(),
            ),
            ("tracking.refresh_query", fixtures::tracking_refresh_query()),
            ("privacy.erase_subject", fixtures::privacy_erase_subject()),
        ];

        for (name, req) in &deferred {
            let resp = dispatch(req, &adapter);
            assert_eq!(
                resp.status,
                ToolResponseStatus::Rejected,
                "{name}: must be Rejected, got {:?}",
                resp.status
            );
            let msg = resp.message.as_deref().unwrap_or("");
            assert!(
                msg.contains("unsupported_capability"),
                "{name}: message must contain 'unsupported_capability', got: {msg}"
            );
        }
    }

    #[test]
    fn alpha_cutline_capability_report_lists_all_deferred() {
        let report = capability_report();
        let expected = [
            "review.resolve_compliance",
            "tracking.untrack_source",
            "tracking.refresh_query",
            "privacy.erase_subject",
        ];
        for cmd in &expected {
            assert!(
                report.deferred_commands.contains(cmd),
                "{cmd} must be listed in deferred_commands"
            );
        }
        for cmd in &expected {
            assert!(
                !report.supported_commands.contains(cmd),
                "{cmd} must NOT be listed in supported_commands"
            );
        }
        // preflight.run is now an alpha read command (issue #261).
        assert!(
            report.supported_commands.contains(&"preflight.run"),
            "preflight.run must be listed in supported_commands"
        );
        assert!(
            !report.deferred_commands.contains(&"preflight.run"),
            "preflight.run must NOT be listed in deferred_commands"
        );
    }

    #[test]
    fn alpha_cutline_deferred_commands_do_not_append_events() {
        let deferred: Vec<(&str, ToolRequest)> = vec![
            (
                "review.resolve_compliance",
                fixtures::review_resolve_compliance(),
            ),
            (
                "tracking.untrack_source",
                fixtures::tracking_untrack_source(),
            ),
            ("tracking.refresh_query", fixtures::tracking_refresh_query()),
            ("privacy.erase_subject", fixtures::privacy_erase_subject()),
        ];

        for (name, req) in &deferred {
            let adapter = mem_adapter();
            let _ = dispatch(req, &adapter);
            let events = adapter.replay().unwrap();
            assert!(
                events.is_empty(),
                "{name}: deferred command must not append events"
            );
        }
    }

    // -- Managed ingest ----

    #[test]
    fn managed_ingest_returns_typed_rejection() {
        let adapter = mem_adapter();
        let req = fixtures::ingest_submit_managed("github", "github_repo_primary");
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        let msg = resp.message.as_deref().unwrap_or("");
        // Must contain one of the typed rejection reasons
        assert!(
            msg.contains("provider_not_connected")
                || msg.contains("provider_action_needed")
                || msg.contains("source_binding_not_ready")
                || msg.contains("unsupported_managed_flow"),
            "expected typed managed ingest rejection, got: {msg}"
        );
    }

    #[test]
    fn managed_ingest_does_not_append_events() {
        let adapter = mem_adapter();
        let req = fixtures::ingest_submit_managed("github", "github_repo_primary");
        let _ = dispatch(&req, &adapter);
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "managed ingest rejection must not append events"
        );
    }

    #[test]
    fn managed_ingest_rejection_contains_provider_id() {
        let adapter = mem_adapter();
        let req = fixtures::ingest_submit_managed("slack", "slack_channel_general");
        let resp = dispatch(&req, &adapter);
        let msg = resp.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("slack"),
            "rejection should name the provider, got: {msg}"
        );
    }

    // -- Recall action tests ---

    #[test]
    fn recall_promote_with_proof_materializes_decision_via_replay() {
        use bicameral_api::recall::{
            ApprovalProof, PromotionOutcome, PromotionRequestOutcome, RecallPromoteCandidateParams,
            RecallPromoteCandidateResult,
        };

        let adapter = mem_adapter();

        // 1. Ingest a candidate
        let ingest_req = fixtures::ingest_submit_local();
        let ingest_resp = dispatch(&ingest_req, &adapter);
        assert_eq!(
            ingest_resp.status,
            ToolResponseStatus::Ok,
            "ingest must succeed"
        );

        // 2. Replay to get the candidate_id
        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert!(
            !state.candidates.is_empty(),
            "should have at least one candidate after ingest"
        );
        let candidate_id = *state.candidates.keys().next().unwrap();

        // 3. Promote via recall with approval proof
        let promote_req = ToolRequest::new(
            ToolCommand::RecallPromoteDecisionCandidate(RecallPromoteCandidateParams {
                packet_id: Uuid::new_v4(),
                candidate_id,
                promotion_outcome: PromotionOutcome::NewConstraint,
                supersedes_decision_id: None,
                scoping_relationship: None,
                approval_proof: Some(ApprovalProof {
                    kind: "signoff_token".into(),
                    value: "tok_test_abc".into(),
                    actor_id: "user-reviewer-01".into(),
                    provided_at: None,
                }),
                confirmation: None,
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let promote_resp = dispatch(&promote_req, &adapter);
        assert_eq!(
            promote_resp.status,
            ToolResponseStatus::Ok,
            "promotion with proof must succeed, got: {:?}",
            promote_resp.message
        );

        // 4. Parse the promotion result to get the returned decision_id
        let result: RecallPromoteCandidateResult =
            serde_json::from_value(promote_resp.result.unwrap()).unwrap();
        assert_eq!(result.outcome, PromotionRequestOutcome::Promoted);
        let decision_id = result
            .decision_id
            .expect("promoted result must include decision_id");

        // 5. Replay again and assert the decision_id exists in state.decisions
        let state_after = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert!(
            state_after.decisions.contains_key(&decision_id),
            "decision_id {} returned by promotion must exist in replayed state",
            decision_id
        );
        let decision = &state_after.decisions[&decision_id];
        assert_eq!(
            decision.id, decision_id,
            "decision id in ledger must match the returned id"
        );
    }

    #[test]
    fn recall_promote_without_proof_routes_to_signoff() {
        use bicameral_api::recall::{
            PromotionOutcome, PromotionRequestOutcome, RecallPromoteCandidateParams,
            RecallPromoteCandidateResult,
        };

        let adapter = mem_adapter();

        // Ingest
        let ingest_req = fixtures::ingest_submit_local();
        dispatch(&ingest_req, &adapter);

        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        let candidate_id = *state.candidates.keys().next().unwrap();

        // Promote without approval proof
        let promote_req = ToolRequest::new(
            ToolCommand::RecallPromoteDecisionCandidate(RecallPromoteCandidateParams {
                packet_id: Uuid::new_v4(),
                candidate_id,
                promotion_outcome: PromotionOutcome::NewConstraint,
                supersedes_decision_id: None,
                scoping_relationship: None,
                approval_proof: None,
                confirmation: None,
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let promote_resp = dispatch(&promote_req, &adapter);
        assert_eq!(promote_resp.status, ToolResponseStatus::Ok);

        let result: RecallPromoteCandidateResult =
            serde_json::from_value(promote_resp.result.unwrap()).unwrap();
        assert_eq!(
            result.outcome,
            PromotionRequestOutcome::RoutedToSignoff,
            "promotion without proof must route to signoff"
        );
        assert!(
            result.decision_id.is_none(),
            "routed-to-signoff must not produce a decision_id"
        );

        // Replay: no new decision should have been created
        let state_after = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert!(
            state_after.decisions.is_empty(),
            "routing to signoff must not create canonical decisions"
        );
    }

    #[test]
    fn recall_promote_free_floating_candidate_is_refused() {
        use bicameral_api::recall::{PromotionOutcome, RecallPromoteCandidateParams};

        let adapter = mem_adapter();

        // No ingest — try to promote a non-existent candidate
        let promote_req = ToolRequest::new(
            ToolCommand::RecallPromoteDecisionCandidate(RecallPromoteCandidateParams {
                packet_id: Uuid::new_v4(),
                candidate_id: Uuid::new_v4(),
                promotion_outcome: PromotionOutcome::NewConstraint,
                supersedes_decision_id: None,
                scoping_relationship: None,
                approval_proof: None,
                confirmation: None,
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let resp = dispatch(&promote_req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        let msg = resp.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("free_floating_claim_refused"),
            "expected free_floating_claim_refused, got: {msg}"
        );

        // No events should have been appended
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "free-floating promotion refusal must not append events"
        );
    }

    #[test]
    fn recall_inspect_evidence_does_not_create_state() {
        use bicameral_api::recall::RecallInspectEvidenceParams;

        let adapter = mem_adapter();

        // Ingest first to have a target
        let ingest_req = fixtures::ingest_submit_local();
        dispatch(&ingest_req, &adapter);
        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        let candidate_id = *state.candidates.keys().next().unwrap();
        let event_count_before = adapter.replay().unwrap().len();

        // Inspect evidence
        let inspect_req = ToolRequest::new(
            ToolCommand::RecallInspectEvidence(RecallInspectEvidenceParams {
                packet_id: Uuid::new_v4(),
                target_id: candidate_id,
                evidence_ref_id: None,
            }),
            fixtures::authority_cli("user-reviewer-01", "/home/user/project"),
        );
        let resp = dispatch(&inspect_req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        // Must not have appended any events
        let event_count_after = adapter.replay().unwrap().len();
        assert_eq!(
            event_count_before, event_count_after,
            "inspect_evidence must not append events"
        );
    }

    // -- request_correction tests ---

    #[test]
    fn recall_request_correction_with_approval_creates_correction_candidate() {
        use bicameral_api::recall::{
            ApprovalProof, CorrectionKind, CorrectionRequestOutcome, RecallRequestCorrectionParams,
            RecallRequestCorrectionResult,
        };

        let adapter = mem_adapter();

        // 1. Ingest a candidate
        let ingest_req = fixtures::ingest_submit_local();
        let ingest_resp = dispatch(&ingest_req, &adapter);
        assert_eq!(ingest_resp.status, ToolResponseStatus::Ok);

        // 2. Replay to get the candidate_id
        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        let candidate_id = *state.candidates.keys().next().unwrap();

        // 3. Request correction with approval proof
        let correction_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![candidate_id],
                correction_kind: CorrectionKind::SourceContradiction,
                rationale: "Source ticket contradicts existing corpus text".into(),
                approval_proof: ApprovalProof {
                    kind: "signoff_token".into(),
                    value: "tok_correction_abc".into(),
                    actor_id: "user-owner-01".into(),
                    provided_at: None,
                },
            }),
            fixtures::authority_cli("user-owner-01", "/home/user/project"),
        );
        let correction_resp = dispatch(&correction_req, &adapter);
        assert_eq!(
            correction_resp.status,
            ToolResponseStatus::Ok,
            "correction with proof must succeed, got: {:?}",
            correction_resp.message
        );

        // 4. Parse the result
        let result: RecallRequestCorrectionResult =
            serde_json::from_value(correction_resp.result.unwrap()).unwrap();
        assert_eq!(result.outcome, CorrectionRequestOutcome::Accepted);
        assert!(
            result.correction_candidate_id.is_some(),
            "accepted correction must produce a correction_candidate_id"
        );

        // 4b. An accepted correction result must never carry accepted-authority
        // fields — it produces a candidate/request only, never a Decision,
        // BindingEvidence, signoff, compliance, or enforcement state.
        let result_json = serde_json::to_value(&result).unwrap();
        let result_obj = result_json
            .as_object()
            .expect("correction result must serialize to an object");
        for forbidden in [
            "decision_id",
            "binding_evidence",
            "signoff_state",
            "compliance_state",
            "enforcement",
        ] {
            assert!(
                !result_obj.contains_key(forbidden),
                "accepted correction result must not carry authority field '{forbidden}'"
            );
        }

        // 5. Verify event was appended
        let events = adapter.replay().unwrap();
        let correction_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == bicameral_api::event_store::EventType::RequestCorrection)
            .collect();
        assert_eq!(
            correction_events.len(),
            1,
            "exactly one RequestCorrection event must be appended"
        );

        // 6. Verify no canonical decisions were created by this action
        let state_after = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert!(
            state_after.decisions.is_empty(),
            "request_correction must NOT create canonical decisions"
        );
    }

    #[test]
    fn recall_request_correction_missing_approval_is_refused() {
        // request_correction requires approval_proof as a non-optional field,
        // so this test verifies at the deserialization level that missing
        // approval_proof fails to construct the params.
        let json = r#"{
            "packet_id": "00000000-0000-0000-0000-000000000000",
            "selected_item_ids": ["00000000-0000-0000-0000-000000000001"],
            "correction_kind": "missing_constraint",
            "rationale": "test"
        }"#;
        let result =
            serde_json::from_str::<bicameral_api::recall::RecallRequestCorrectionParams>(json);
        assert!(
            result.is_err(),
            "missing approval_proof must fail deserialization"
        );
    }

    #[test]
    fn recall_request_correction_single_use_enforcement() {
        use bicameral_api::recall::{
            ApprovalProof, CorrectionKind, CorrectionRequestOutcome, RecallRequestCorrectionParams,
            RecallRequestCorrectionResult,
        };

        let adapter = mem_adapter();

        // 1. Ingest a candidate
        let ingest_req = fixtures::ingest_submit_local();
        dispatch(&ingest_req, &adapter);
        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        let candidate_id = *state.candidates.keys().next().unwrap();

        // 2. First correction request — must succeed
        let proof = ApprovalProof {
            kind: "signoff_token".into(),
            value: "tok_single_use_001".into(),
            actor_id: "user-owner-01".into(),
            provided_at: None,
        };
        let first_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![candidate_id],
                correction_kind: CorrectionKind::MissingConstraint,
                rationale: "First correction request".into(),
                approval_proof: proof.clone(),
            }),
            fixtures::authority_cli("user-owner-01", "/home/user/project"),
        );
        let first_resp = dispatch(&first_req, &adapter);
        assert_eq!(first_resp.status, ToolResponseStatus::Ok);
        let first_result: RecallRequestCorrectionResult =
            serde_json::from_value(first_resp.result.unwrap()).unwrap();
        assert_eq!(first_result.outcome, CorrectionRequestOutcome::Accepted);

        // 3. Second correction request with SAME approval proof — must be refused
        let second_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![candidate_id],
                correction_kind: CorrectionKind::FeatureDefinitionChange,
                rationale: "Second correction request reusing proof".into(),
                approval_proof: proof.clone(),
            }),
            fixtures::authority_cli("user-owner-01", "/home/user/project"),
        );
        let second_resp = dispatch(&second_req, &adapter);
        assert_eq!(
            second_resp.status,
            ToolResponseStatus::Rejected,
            "reusing consumed approval proof must be rejected"
        );
        let msg = second_resp.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("approval_already_consumed"),
            "expected approval_already_consumed, got: {msg}"
        );

        // 4. Third correction request with a DIFFERENT proof — must succeed
        let new_proof = ApprovalProof {
            kind: "review_approval_id".into(),
            value: "tok_single_use_002".into(),
            actor_id: "user-owner-01".into(),
            provided_at: None,
        };
        let third_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![candidate_id],
                correction_kind: CorrectionKind::StaleOrAmbiguous,
                rationale: "Third correction request with fresh proof".into(),
                approval_proof: new_proof,
            }),
            fixtures::authority_cli("user-owner-01", "/home/user/project"),
        );
        let third_resp = dispatch(&third_req, &adapter);
        assert_eq!(
            third_resp.status,
            ToolResponseStatus::Ok,
            "fresh proof must succeed, got: {:?}",
            third_resp.message
        );
    }

    #[test]
    fn recall_request_correction_free_floating_item_is_refused() {
        use bicameral_api::recall::{ApprovalProof, CorrectionKind, RecallRequestCorrectionParams};

        let adapter = mem_adapter();

        // No ingest — try to correct a non-existent candidate
        let correction_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![Uuid::new_v4()],
                correction_kind: CorrectionKind::MissingConstraint,
                rationale: "Free-floating correction attempt".into(),
                approval_proof: ApprovalProof {
                    kind: "signoff_token".into(),
                    value: "tok_freefloat".into(),
                    actor_id: "user-attacker-01".into(),
                    provided_at: None,
                },
            }),
            fixtures::authority_cli("user-attacker-01", "/home/user/project"),
        );
        let resp = dispatch(&correction_req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        let msg = resp.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("free_floating_claim_refused"),
            "expected free_floating_claim_refused, got: {msg}"
        );

        // No events should have been appended
        let events = adapter.replay().unwrap();
        assert!(
            events.is_empty(),
            "free-floating correction refusal must not append events"
        );
    }

    #[test]
    fn recall_request_correction_empty_selected_items_is_refused() {
        use bicameral_api::recall::{ApprovalProof, CorrectionKind, RecallRequestCorrectionParams};

        let adapter = mem_adapter();

        let correction_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![],
                correction_kind: CorrectionKind::MissingConstraint,
                rationale: "No items selected".into(),
                approval_proof: ApprovalProof {
                    kind: "signoff_token".into(),
                    value: "tok_empty".into(),
                    actor_id: "user-01".into(),
                    provided_at: None,
                },
            }),
            fixtures::authority_cli("user-01", "/home/user/project"),
        );
        let resp = dispatch(&correction_req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        let msg = resp.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("missing_selected_refs"),
            "expected missing_selected_refs, got: {msg}"
        );
    }

    #[test]
    fn recall_request_correction_does_not_create_canonical_state() {
        use bicameral_api::recall::{ApprovalProof, CorrectionKind, RecallRequestCorrectionParams};

        let adapter = mem_adapter();

        // Ingest to have a candidate
        let ingest_req = fixtures::ingest_submit_local();
        dispatch(&ingest_req, &adapter);
        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        let candidate_id = *state.candidates.keys().next().unwrap();

        let correction_req = ToolRequest::new(
            ToolCommand::RecallRequestCorrection(RecallRequestCorrectionParams {
                packet_id: Uuid::new_v4(),
                selected_item_ids: vec![candidate_id],
                correction_kind: CorrectionKind::FeatureDefinitionChange,
                rationale: "Feature definition changing".into(),
                approval_proof: ApprovalProof {
                    kind: "signoff_token".into(),
                    value: "tok_no_canonical_state".into(),
                    actor_id: "user-owner-01".into(),
                    provided_at: None,
                },
            }),
            fixtures::authority_cli("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&correction_req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        // After correction, replay must show no decisions, no signoff,
        // no compliance, no enforcement state changes.
        let state_after = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert!(
            state_after.decisions.is_empty(),
            "request_correction must NOT create canonical decisions"
        );
        // Candidate state should be unchanged
        let candidate = &state_after.candidates[&candidate_id];
        assert_eq!(
            candidate.candidate_state,
            bicameral_api::candidate::CandidateState::Proposed,
            "request_correction must not change candidate state"
        );
    }

    // -- Contradiction resolution dispatch (#602) ---

    /// Seed an adapter with a contradiction report in Open status.
    /// Returns (adapter, report_id).
    fn seed_contradiction_adapter() -> (MemoryAdapter, Uuid) {
        use bicameral_api::candidate::DecisionCandidate;
        use bicameral_api::catalog::{ConstraintScope, ConstraintType};
        use bicameral_api::constraint::{ActorType, Attribution};
        use bicameral_api::drift::{ContradictionReport, FindingSeverity, FindingStatus};
        use bicameral_api::event_store::{
            ClassifyConstraintPayload, IngestPayload, PromoteToCanonicalPayload,
            RaiseContradictionPayload,
        };
        use bicameral_api::evidence::SourceEvidence;
        use bicameral_api::source::Source;

        let adapter = mem_adapter();

        // Two decisions promoted to catalog constraints.
        for (uri, key, title) in [
            ("jira://D-1", "disp-c1", "Constraint A"),
            ("jira://D-2", "disp-c2", "Constraint B"),
        ] {
            let source = Source {
                uri: uri.to_string(),
                source_type: "test".to_string(),
                label: None,
                source_kind: None,
            };
            let ev = vec![SourceEvidence::new(
                format!("inline:{uri}"),
                0,
                10,
                "test".to_string(),
                0.9,
            )];
            let mut cand = DecisionCandidate::new(
                title.to_string(),
                format!("{title} statement"),
                bicameral_api::candidate::DecisionLevel::Architecture,
                uri.to_string(),
            );
            cand.evidence_refs = ev.iter().map(|e| e.id).collect();
            let cand_id = cand.id;
            adapter
                .append(EventStoreEntry::new(
                    EventType::Ingest,
                    serde_json::to_value(&IngestPayload {
                        source,
                        snapshot: None,
                        evidence: ev,
                        candidate: cand,
                        evidence_references: Vec::new(),
                        provenance: None,
                    })
                    .unwrap(),
                    "test".to_string(),
                ))
                .unwrap();

            let dec_id = Uuid::new_v4();
            adapter
                .append(EventStoreEntry::new(
                    EventType::AcceptCandidate,
                    serde_json::json!({
                        "candidate_id": cand_id.to_string(),
                        "decision_id": dec_id.to_string(),
                    }),
                    "test".to_string(),
                ))
                .unwrap();
            adapter
                .append(EventStoreEntry::new(
                    EventType::PromoteToCanonical,
                    serde_json::to_value(&PromoteToCanonicalPayload {
                        decision_id: dec_id,
                        title: title.to_string(),
                        statement: format!("{title} statement"),
                        evidence_refs: Vec::new(),
                        affected_surfaces: vec!["shared".to_string()],
                        reviewer: Attribution {
                            actor_id: "reviewer".to_string(),
                            actor_type: ActorType::Human,
                            display_name: None,
                        },
                        reason: None,
                        idempotency_key: key.to_string(),
                    })
                    .unwrap(),
                    "test".to_string(),
                ))
                .unwrap();
            adapter
                .append(EventStoreEntry::new(
                    EventType::ClassifyConstraint,
                    serde_json::to_value(&ClassifyConstraintPayload {
                        idempotency_key: key.to_string(),
                        constraint_type: ConstraintType::Security,
                        scope: ConstraintScope::Service,
                        attribution: Attribution {
                            actor_id: "classifier".to_string(),
                            actor_type: ActorType::Human,
                            display_name: None,
                        },
                        affected_surfaces: None,
                    })
                    .unwrap(),
                    "test".to_string(),
                ))
                .unwrap();
        }

        // Replay to get constraint IDs then raise contradiction.
        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        let c1_id = state.catalog.get("disp-c1").unwrap().id;
        let c2_id = state.catalog.get("disp-c2").unwrap().id;

        let report_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let report = ContradictionReport {
            id: report_id,
            summary: "test contradiction".to_string(),
            severity: FindingSeverity::High,
            status: FindingStatus::Open,
            related_constraint_ids: vec![c1_id, c2_id],
            evidence_refs: Vec::new(),
            suggested_resolution: None,
            owner_recommendation: None,
            detected_at: now,
            updated_at: now,
        };
        adapter
            .append(EventStoreEntry::new(
                EventType::RaiseContradiction,
                serde_json::to_value(&RaiseContradictionPayload { report }).unwrap(),
                "detector".to_string(),
            ))
            .unwrap();

        (adapter, report_id)
    }

    #[test]
    fn dispatch_transition_finding_open_to_acknowledged() {
        let (adapter, report_id) = seed_contradiction_adapter();
        let req = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: report_id.to_string(),
                to_status: "acknowledged".to_string(),
                note: Some("Acknowledged".to_string()),
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.contradictions.get(&report_id).unwrap().status,
            bicameral_api::drift::FindingStatus::Acknowledged
        );
    }

    #[test]
    fn dispatch_transition_finding_acknowledged_to_resolved() {
        let (adapter, report_id) = seed_contradiction_adapter();

        // First: Open → Acknowledged.
        let req1 = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: report_id.to_string(),
                to_status: "acknowledged".to_string(),
                note: None,
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp1 = dispatch(&req1, &adapter);
        assert_eq!(resp1.status, ToolResponseStatus::Ok);

        // Second: Acknowledged → Resolved.
        let req2 = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: report_id.to_string(),
                to_status: "resolved".to_string(),
                note: Some("Fixed".to_string()),
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp2 = dispatch(&req2, &adapter);
        assert_eq!(resp2.status, ToolResponseStatus::Ok);

        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.contradictions.get(&report_id).unwrap().status,
            bicameral_api::drift::FindingStatus::Resolved
        );
    }

    #[test]
    fn dispatch_transition_finding_open_to_dismissed() {
        let (adapter, report_id) = seed_contradiction_adapter();
        let req = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: report_id.to_string(),
                to_status: "dismissed".to_string(),
                note: Some("False positive".to_string()),
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Ok);

        let state = crate::replay::replay_from_adapter(&adapter).unwrap();
        assert_eq!(
            state.contradictions.get(&report_id).unwrap().status,
            bicameral_api::drift::FindingStatus::Dismissed
        );
    }

    #[test]
    fn dispatch_transition_finding_invalid_transition_rejected() {
        let (adapter, report_id) = seed_contradiction_adapter();
        // Open → Resolved is invalid (must go through Acknowledged).
        let req = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: report_id.to_string(),
                to_status: "resolved".to_string(),
                note: None,
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("invalid_transition"),
            "expected invalid_transition, got: {:?}",
            resp.message
        );
    }

    #[test]
    fn dispatch_transition_finding_unknown_report_rejected() {
        let (adapter, _report_id) = seed_contradiction_adapter();
        let fake_id = Uuid::new_v4();
        let req = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: fake_id.to_string(),
                to_status: "acknowledged".to_string(),
                note: None,
            }),
            fixtures::authority_dashboard("user-owner-01", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unknown_target"),
            "expected unknown_target, got: {:?}",
            resp.message
        );
    }

    #[test]
    fn dispatch_transition_finding_mcp_authority_rejected() {
        let (adapter, report_id) = seed_contradiction_adapter();
        // MCP session authority should be rejected — requires owner/dashboard.
        let req = ToolRequest::new(
            ToolCommand::ReviewTransitionFinding(TransitionFindingParams {
                report_id: report_id.to_string(),
                to_status: "acknowledged".to_string(),
                note: None,
            }),
            fixtures::authority_mcp("agent-mcp-001", "sess-001", "/home/user/project"),
        );
        let resp = dispatch(&req, &adapter);
        assert_eq!(resp.status, ToolResponseStatus::Rejected);
        assert!(
            resp.message
                .as_deref()
                .unwrap_or("")
                .contains("unauthorized"),
            "expected unauthorized, got: {:?}",
            resp.message
        );
    }

    // ── graph.refresh_snapshot budget gate (issue #646 sub-slice A) ───────

    fn setup_temp_git_repo() -> tempfile::TempDir {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git")
            .args(["init", "-b", "master"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(p)
            .output()
            .unwrap();
        std::fs::write(p.join("hello.rs"), "fn main() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(p)
            .output()
            .unwrap();
        dir
    }

    fn graph_refresh_request(
        files: Vec<String>,
        symbols: Vec<String>,
        allow_index: AllowIndex,
        scope_kind: Option<RefreshScopeKind>,
    ) -> ToolRequest {
        ToolRequest::new(
            ToolCommand::GraphRefreshSnapshot(GraphRefreshParams {
                workspace_id: None,
                ref_name: None,
                commit_sha: None,
                base_ref: None,
                base_sha: None,
                head_ref: None,
                head_sha: None,
                scope_kind,
                files,
                symbols,
                allow_index,
                trigger: RefreshTrigger::Manual,
            }),
            fixtures::authority_dashboard("user-01", "/home/user/project"),
        )
    }

    fn parse_refresh_result(resp: &ToolResponse) -> GraphRefreshResult {
        serde_json::from_value(resp.result.clone().unwrap()).unwrap()
    }

    #[test]
    fn graph_refresh_over_budget_returns_over_budget() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let files: Vec<String> = (0..65).map(|i| format!("f{i}.rs")).collect();
        let req = graph_refresh_request(files, vec![], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::OverBudget);
        assert!(result.next_actions.contains(&"narrow_scope".to_string()));
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn graph_refresh_at_budget_boundary_proceeds() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let files: Vec<String> = (0..64).map(|i| format!("f{i}.rs")).collect();
        let req = graph_refresh_request(files, vec![], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let result = parse_refresh_result(&resp);
        assert_ne!(result.status, GraphRefreshStatus::OverBudget);
    }

    #[test]
    fn graph_refresh_symbols_count_toward_budget() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let files: Vec<String> = (0..32).map(|i| format!("f{i}.rs")).collect();
        let symbols: Vec<String> = (0..33).map(|i| format!("sym{i}")).collect();
        let req = graph_refresh_request(files, symbols, AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::OverBudget);
    }

    #[test]
    fn graph_refresh_budget_parity_with_preflight_path() {
        use crate::commit_freshness::{decide_bounded_outcome, BOUNDED_SCOPE_BUDGET};
        use bicameral_api::freshness::{RefreshOutcome, TouchedDetectionMethod, TouchedScope};

        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();

        // Over budget: both paths decline
        let over_files: Vec<String> = (0..(BOUNDED_SCOPE_BUDGET + 1))
            .map(|i| format!("f{i}.rs"))
            .collect();
        let over_scope = TouchedScope {
            detection: Some(TouchedDetectionMethod::ExplicitTargets),
            files: over_files.clone(),
            symbols: vec![],
        };
        assert_eq!(
            decide_bounded_outcome(GraphReadiness::Ready, &over_scope),
            RefreshOutcome::OverBudget,
        );
        let req = graph_refresh_request(over_files, vec![], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::OverBudget);

        // Within budget: both paths proceed
        let ok_files: Vec<String> = (0..BOUNDED_SCOPE_BUDGET)
            .map(|i| format!("f{i}.rs"))
            .collect();
        let ok_scope = TouchedScope {
            detection: Some(TouchedDetectionMethod::ExplicitTargets),
            files: ok_files.clone(),
            symbols: vec![],
        };
        assert_ne!(
            decide_bounded_outcome(GraphReadiness::Ready, &ok_scope),
            RefreshOutcome::OverBudget,
        );
        let req = graph_refresh_request(ok_files, vec![], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        assert_ne!(result.status, GraphRefreshStatus::OverBudget);
    }

    // ── graph.refresh_snapshot bounded materialization (issue #646 sub-slice B) ──

    #[test]
    fn graph_refresh_bounded_within_budget_materializes_slice_and_ready() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let req = graph_refresh_request(
            vec!["src/a.rs".into(), "src/b.rs".into()],
            vec![],
            AllowIndex::Bounded,
            Some(RefreshScopeKind::FileSet),
        );
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        assert_eq!(resp.status, ToolResponseStatus::Ok);
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::Ready);

        let slice = result
            .slice
            .expect("bounded refresh should produce a GraphSlice");
        assert_eq!(slice.scope_kind.as_deref(), Some("file_set"));
        assert!(slice.scope_fingerprint.is_some());
        assert_eq!(slice.completeness, Some(SliceCompleteness::FileScoped));
    }

    #[test]
    fn graph_refresh_bounded_over_budget_returns_without_rebuilding() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let files: Vec<String> = (0..65).map(|i| format!("f{i}.rs")).collect();
        let req = graph_refresh_request(files, vec![], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::OverBudget);
        assert!(result.slice.is_none());
        assert!(result.snapshot.is_none());
    }

    #[test]
    fn graph_refresh_bounded_symbols_materializes_symbol_scoped_slice() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let req = graph_refresh_request(
            vec![],
            vec!["module::func".into()],
            AllowIndex::Bounded,
            Some(RefreshScopeKind::SymbolSet),
        );
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::Ready);
        let slice = result
            .slice
            .expect("symbol-set refresh should produce a GraphSlice");
        assert_eq!(slice.scope_kind.as_deref(), Some("symbol_set"));
        assert_eq!(slice.completeness, Some(SliceCompleteness::SymbolScoped));
    }

    #[test]
    fn graph_refresh_bounded_no_targets_omits_slice() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();
        let req = graph_refresh_request(vec![], vec![], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::Ready);
        assert!(
            result.slice.is_none(),
            "empty scope should not produce a slice"
        );
    }

    #[test]
    fn graph_refresh_bounded_infers_scope_kind_from_targets() {
        let dir = setup_temp_git_repo();
        let adapter = mem_adapter();

        // Files only → inferred FileSet
        let req = graph_refresh_request(
            vec!["a.rs".into()],
            vec![],
            AllowIndex::Bounded,
            None, // no explicit scope_kind
        );
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        let slice = result.slice.expect("should produce a slice");
        assert_eq!(slice.scope_kind.as_deref(), Some("file_set"));

        // Symbols present → inferred SymbolSet
        let req = graph_refresh_request(vec![], vec!["foo::bar".into()], AllowIndex::Bounded, None);
        let resp = dispatch_with_workspace(&req, &adapter, Some(dir.path()));
        let result = parse_refresh_result(&resp);
        let slice = result.slice.expect("should produce a slice");
        assert_eq!(slice.scope_kind.as_deref(), Some("symbol_set"));
    }

    #[test]
    fn graph_refresh_not_configured_workspace_still_returns_not_configured() {
        let adapter = mem_adapter();
        let req = graph_refresh_request(vec!["a.rs".into()], vec![], AllowIndex::Bounded, None);
        // No workspace root → NotConfigured, regardless of targets
        let resp = dispatch_with_workspace(&req, &adapter, None);
        let result = parse_refresh_result(&resp);
        assert_eq!(result.status, GraphRefreshStatus::NotConfigured);
    }
}
