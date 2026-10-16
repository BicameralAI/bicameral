//! ToolRequest / ToolResponse — the canonical local tool API contract.
//!
//! Per ADR-0002 and issue #103: `ToolRequest` is the transport-neutral local
//! action/query request object.  MCP, CLI, TUI, and dashboard actions map to
//! `ToolRequest` before daemon validation and policy evaluation.
//!
//! This module does **not** preserve the existing MCP v0.2 direct input/output
//! contract.  MCP clients should migrate to tool calls that map into canonical
//! `ToolRequest` commands.
//!
//! `ExternalIngestEnvelope` (external/source-system evidence submission) is a
//! separate contract and must never be routed through `ToolRequest`.  External
//! integrations cannot emit review commands, canonical decisions, event-store
//! writes, signoff approval, compliance resolution, policy grants, local actor
//! / session authority, or blocking governance results.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Authority context
// ---------------------------------------------------------------------------

/// Local actor identity or daemon-authenticated equivalent.
///
/// Every `ToolRequest` carries an `AuthorityContext` so the daemon can evaluate
/// governance policy and produce an auditable event trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityContext {
    /// Who is issuing this request (user id, agent id, or daemon-authenticated
    /// equivalent).
    pub actor_id: String,

    /// How the actor was authenticated.
    pub auth_method: AuthMethod,

    /// Session or workspace context for scoping policy evaluation.
    pub session_id: Option<String>,

    /// Workspace root (repo path or equivalent) for policy scope resolution.
    pub workspace: Option<String>,

    /// Policy scope tags — the daemon uses these (along with `command`) to
    /// select which `GovernancePolicy` rules apply.
    #[serde(default)]
    pub policy_scope: Vec<String>,

    /// Audit metadata: free-form key-value pairs attached to every event
    /// produced by this request (e.g. `surface=mcp`, `mcp_tool=bicameral.review`).
    #[serde(default)]
    pub audit_metadata: serde_json::Map<String, serde_json::Value>,
}

/// Authentication method for the actor issuing a `ToolRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// Local daemon socket — the actor is on the same machine.
    LocalDaemon,
    /// MCP stdio session — authenticated through the MCP transport.
    McpSession,
    /// CLI invocation — the actor ran a `bicameral` CLI command.
    Cli,
    /// Dashboard/TUI — the actor used the local review UI.
    Dashboard,
    /// Hosted bridge — paired browser-origin session routed through the
    /// bridge gateway guard. AuthorityContext is daemon-derived from the
    /// validated bridge session, never caller-supplied.
    HostedBridge,
}

// ---------------------------------------------------------------------------
// ToolRequest
// ---------------------------------------------------------------------------

/// The canonical local tool API request object.
///
/// All local surfaces (MCP, CLI, TUI, dashboard) construct a `ToolRequest`
/// and submit it to the daemon.  The daemon validates the request, evaluates
/// governance policy, and returns a `ToolResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    /// Unique request identifier (for idempotency and audit correlation).
    pub request_id: Uuid,

    /// Canonical Product selected for this request.
    ///
    /// Operator-scoped daemons use this identifier to resolve the Product's
    /// event-store adapter and local workspace binding from the operator
    /// registry. It is never inferred from process cwd or `authority.workspace`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,

    /// The command to execute.
    pub command: ToolCommand,

    /// Authority context for policy evaluation and audit.
    pub authority: AuthorityContext,

    /// Timestamp at which the surface created this request.
    pub issued_at: DateTime<Utc>,
}

impl ToolRequest {
    pub fn new(command: ToolCommand, authority: AuthorityContext) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            product_id: None,
            command,
            authority,
            issued_at: Utc::now(),
        }
    }

    /// Select the canonical Product that the operator daemon should route.
    pub fn for_product(mut self, product_id: impl Into<String>) -> Self {
        self.product_id = Some(product_id.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Command registry
// ---------------------------------------------------------------------------

/// Local command registry — the exhaustive set of actions the daemon accepts
/// through `ToolRequest`.
///
/// Each variant carries the typed payload for that command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name", content = "params", rename_all = "snake_case")]
pub enum ToolCommand {
    // -- ingest -----------------------------------------------------------
    /// Submit source evidence and/or decision candidates to the local daemon.
    #[serde(rename = "ingest.submit_local")]
    IngestSubmitLocal(IngestSubmitLocalParams),

    /// Submit ingestion for a managed provider-backed source.
    ///
    /// Unlike `ingest.submit_local`, this path checks provider connection
    /// state and source binding readiness before accepting any evidence.
    /// Returns typed not-ready/rejection when prerequisites are unmet.
    #[serde(rename = "ingest.submit_managed")]
    IngestSubmitManaged(IngestSubmitManagedParams),

    // -- preflight --------------------------------------------------------
    /// Surface relevant decisions before implementation.
    #[serde(rename = "preflight.run")]
    PreflightRun(PreflightRunParams),

    // -- binding ----------------------------------------------------------
    /// Propose or inspect binding evidence for a decision/candidate.
    #[serde(rename = "binding.create")]
    BindingCreate(BindingCreateParams),

    /// Inspect existing bindings for a decision/candidate.
    #[serde(rename = "binding.inspect")]
    BindingInspect(BindingInspectParams),

    // -- workspace binding ------------------------------------------------
    /// Materialize a daemon-owned local workspace binding from a surface
    /// [`WorkspaceBindingProposal`](crate::workspace_binding::WorkspaceBindingProposal).
    ///
    /// The daemon is the sole authority: MCP/CLI/dashboard surfaces may only
    /// propose, and the daemon validates registration, session scope, candidate
    /// path safety, and explicit operator confirmation before persisting
    /// operator-local per-project [`LocalWorkspaceBinding`] state. Binding is
    /// project-identity grounding only — never `BindingEvidence`, source
    /// authority, or cloud/hosted state.
    #[serde(rename = "workspace.bind")]
    WorkspaceBind(crate::workspace_binding::WorkspaceBindRequest),

    /// Reverse-lookup a unique daemon-owned local workspace binding for a
    /// host-reported folder (mcp#831 / ADR-0023 SessionStart routing).
    ///
    /// Local-only read: the daemon returns at most one registered `product_id`
    /// for an exact canonical path match. This is a **routing fact**, not
    /// Product identity, BindingEvidence, or a bind write. The hosted bridge
    /// must never receive `candidate_path`.
    #[serde(rename = "workspace.resolve")]
    WorkspaceResolve(crate::workspace_binding::WorkspaceResolveRequest),

    // -- review -----------------------------------------------------------
    /// Accept a decision candidate through governance.
    #[serde(rename = "review.accept_candidate")]
    ReviewAcceptCandidate(ReviewTargetParams),

    /// Reject a decision candidate.
    #[serde(rename = "review.reject_candidate")]
    ReviewRejectCandidate(ReviewTargetParams),

    /// Hide a candidate from the current daemon session/view only.
    ///
    /// This is transient presentation state. It is never canonical and is lost
    /// on daemon restart.
    #[serde(rename = "candidate.hide")]
    CandidateHide(CandidateHideParams),

    /// Suppress candidate exposure in daemon-local working state.
    ///
    /// Suppression remains local and non-canonical. It expires explicitly or is
    /// invalidated when the evidence identity, source identity, content digest,
    /// or semantic fingerprint changes.
    #[serde(rename = "candidate.suppress")]
    CandidateSuppress(CandidateSuppressParams),

    /// Approve signoff on a promoted decision (ownership approval only).
    ///
    /// Signoff approval records that a human accepts the Decision's meaning.
    /// It does not imply compliance, enforcement, code readiness, or
    /// implementation correctness (ADR-0022).
    #[serde(rename = "review.approve_signoff")]
    ReviewApproveSignoff(ReviewTargetParams),

    /// Reject signoff on a promoted decision (ownership rejection only).
    ///
    /// Signoff rejection records that a human rejects the Decision's meaning.
    /// It does not alter compliance state or graph readiness.
    #[serde(rename = "review.reject_signoff")]
    ReviewRejectSignoff(ReviewTargetParams),

    /// Resolve compliance state for a decision.
    #[serde(rename = "review.resolve_compliance")]
    ReviewResolveCompliance(ResolveComplianceParams),

    // -- evidence ---------------------------------------------------------
    /// Refresh evidence currentness for a tracked Decision.
    ///
    /// Resolves the local repo snapshot, inspects existing binding evidence
    /// with bot-owned snapshot/file-inspection primitives, and materializes
    /// an evidence currentness outcome. Does not mutate signoff, compliance,
    /// or accepted `BindingEvidence`.
    ///
    /// Returns typed rejection for ineligible Decisions (untracked, rejected,
    /// superseded, no bindings) and typed unsupported/unavailable outcomes
    /// when the workspace or graph is not available.
    #[serde(rename = "evidence.refresh")]
    EvidenceRefresh(EvidenceRefreshParams),

    // -- tracking ---------------------------------------------------------
    /// Stop automatic refresh tracking for a Decision.
    ///
    /// Emits an `UntrackSource` event and sets `tracking_state = Untracked`.
    /// The Decision remains in the ledger but automatic refresh ceases.
    #[serde(rename = "tracking.untrack_source")]
    TrackingUntrackSource(UntrackSourceParams),

    /// Query which Decisions are eligible for automatic source refresh.
    #[serde(rename = "tracking.refresh_query")]
    TrackingRefreshQuery(RefreshQueryParams),

    // -- graph / code locate -----------------------------------------------
    /// Deterministic code target retrieval and verification.
    ///
    /// Returns `LocatorResult` with ranked candidates, graph readiness,
    /// evidence state, and next actions. Does not materialize
    /// `BindingEvidence`. Read-only in V1.
    #[serde(rename = "code.locate")]
    CodeLocate(CodeLocateParams),

    /// Read graph readiness for a workspace/ref/snapshot without indexing.
    #[serde(rename = "graph.status")]
    GraphStatus(GraphStatusParams),

    /// Explicit graph refresh/index command — V1 manual mutating trigger.
    ///
    /// Builds or refreshes a bounded graph snapshot/slice for a requested
    /// target. Must not write governance events or mutate working-tree
    /// files or git refs.
    #[serde(rename = "graph.refresh_snapshot")]
    GraphRefreshSnapshot(GraphRefreshParams),

    // -- decision discovery ------------------------------------------------
    /// One-click code-impact discovery for an accepted Decision.
    ///
    /// Returns candidate code regions, evidence state/status, confidence,
    /// reasons, currentness/status caveats, and next actions. Does not
    /// materialize `BindingEvidence`, resolve compliance, produce signoff,
    /// assert safety/no-conflict, or emit blocking governance results.
    #[serde(rename = "decision.find_code_impact")]
    DecisionFindCodeImpact(FindCodeImpactParams),

    // -- recall ------------------------------------------------------------
    /// Inspect evidence for a decision or candidate in a RecallPacket.
    ///
    /// Read-only drill-in to cited evidence refs, source spans, code evidence,
    /// freshness/readiness, and authority labels. Does not mutate any state.
    #[serde(rename = "recall.inspect_evidence")]
    RecallInspectEvidence(crate::recall::RecallInspectEvidenceParams),

    /// Expand scope within a RecallPacket context.
    ///
    /// Explicit user-selected read-only search over a named unknown/additional
    /// source scope. Returns a new or refreshed RecallPacket.
    #[serde(rename = "recall.expand_scope")]
    RecallExpandScope(crate::recall::RecallExpandScopeParams),

    /// Promote a DecisionCandidate from a RecallPacket.
    ///
    /// Explicit governed request to promote, supersede, or route a
    /// DecisionCandidate for signoff. References a bot-known candidate;
    /// free-floating LLM text is refused.
    #[serde(rename = "recall.promote_decision_candidate")]
    RecallPromoteDecisionCandidate(crate::recall::RecallPromoteCandidateParams),

    /// Request a governed correction/corpus-update path.
    ///
    /// Explicit, approval-gated follow-up to a prior RecallPacket.
    /// References packet items. Requires single-use approval proof and
    /// bot authorization. Cannot create accepted Decision, accepted
    /// BindingEvidence, signoff, compliance, or enforcement state.
    #[serde(rename = "recall.request_correction")]
    RecallRequestCorrection(crate::recall::RecallRequestCorrectionParams),

    // -- privacy ----------------------------------------------------------
    /// Right-to-erasure request for a PII subject (GDPR Art.17).
    ///
    /// MCP surfaces gate this through a single-use `ErasureGate` approval
    /// before dispatching to the daemon. The daemon validates the request
    /// and delegates to the PII archive for actual disposal. When the PII
    /// archive is not yet implemented, the handler fails closed — the
    /// request is rejected rather than silently ignored.
    #[serde(rename = "privacy.erase_subject")]
    PrivacyEraseSubject(PrivacyEraseSubjectParams),

    // -- contradiction resolution -----------------------------------------
    /// Transition a contradiction finding's status (acknowledge, resolve,
    /// dismiss). Requires owner-level authority.
    #[serde(rename = "review.transition_finding")]
    ReviewTransitionFinding(TransitionFindingParams),

    // -- history / search -------------------------------------------------
    /// Read replayed/materialized decision state.
    #[serde(rename = "history.list")]
    HistoryList(HistoryListParams),

    /// Search decisions, candidates, and bindings.
    #[serde(rename = "search.query")]
    SearchQuery(SearchQueryParams),

    // -- product creation (daemon-canonical, hosted) ----------------------
    /// Create a canonical Product through the daemon on behalf of a paired
    /// hosted user (issue #766).
    ///
    /// The daemon mints the Product identity, materializes the creator's roles
    /// atomically (exactly Policy Owner + Product Owner in a single membership
    /// event), and persists an initial Governance Policy Version. Authority is
    /// bound from the validated hosted session — never caller-supplied. Invalid
    /// authority (unpaired, wrong Cloud account, missing actor context,
    /// unauthorized actor) returns a typed denial and writes no event. Retrying
    /// the same idempotency key never creates a second Product.
    #[serde(rename = "product.create")]
    ProductCreate(crate::product_creation::ProductCreateParams),

    /// Retryable, non-canonical hosted publication of an already-created
    /// Product, keyed by the canonical Product id (issue #766).
    ///
    /// Derives a safe publication descriptor from the canonical creation
    /// receipt. Retrying never re-runs canonical Product creation.
    #[serde(rename = "product.publish")]
    ProductPublish(crate::product_creation::ProductPublishParams),

    // -- onboarding -------------------------------------------------------
    /// Same-domain Product Member auto-join through a signed Cloud ticket.
    ///
    /// The daemon verifies the narrow signed [`ProductJoinTicket`](crate::product_join::ProductJoinTicket),
    /// binds the ticket account to the locally confirmed actor context,
    /// re-evaluates the Product's active auto-join policy in canonical state,
    /// and idempotently materializes a Product Member-only membership event.
    /// It never accepts a hosted session token, an SPA-asserted actor, or
    /// Cloud membership state as authority.
    #[serde(rename = "product.join")]
    ProductJoin(crate::product_join::ProductJoinCommandParams),

    /// Continue an already-joined Product through local setup (issue #769).
    ///
    /// After canonical membership exists (bot#767), the daemon continues the
    /// *local* lifecycle: it resolves provider access and validates the
    /// event-store descriptor (both daemon-derived, never SPA-asserted), writes
    /// at most one durable local `ProjectRegistration`, opens the adapter, and
    /// replays canonical Product state. It returns a typed
    /// [`SetupReadiness`](crate::product_registration::SetupReadiness) —
    /// `Open` / `Continue setup` / `Repair`. It never creates, changes, or
    /// revokes canonical membership.
    #[serde(rename = "product.continue_setup")]
    ProductContinueSetup(crate::product_registration::ProductContinueSetupParams),

    // -- Managed MCP CandidateSet selection / session (issues #987 / #989) --
    /// Validate a stable-id promote/reject subset and return an exact no-write
    /// preview with a daemon-minted confirmation challenge.
    ///
    /// MCP may transport stable candidate ids only. The daemon validates
    /// currentness, dependency closure, authority, and policy. No canonical
    /// Product write occurs on prepare.
    #[serde(rename = "candidate_selection.prepare")]
    CandidateSelectionPrepare(crate::candidate_selection::CandidateSelectionV1),

    /// Consume explicit human confirmation and atomically apply one exact
    /// previewed selection (canonical promotions + transient rejections).
    ///
    /// Requires `human_confirmed = true` and a daemon-issued single-use token.
    /// MCP cannot self-confirm, alter effects, or append events.
    #[serde(rename = "candidate_selection.commit")]
    CandidateSelectionCommit(crate::candidate_selection::CandidateSelectionCommitV1),

    /// Extend the exact daemon-owned CandidateSet session lease.
    ///
    /// Does not enumerate candidates or create Product state.
    #[serde(rename = "candidate_session.heartbeat")]
    CandidateSessionHeartbeat(crate::candidate_session::CandidateSessionHeartbeatRequestV1),

    /// End the exact CandidateSet session and erase transient candidate and
    /// disposition state. Erasure is not rejection and creates no canonical event.
    #[serde(rename = "candidate_session.end")]
    CandidateSessionEnd(crate::candidate_session::CandidateSessionEndRequestV1),
}

impl ToolCommand {
    /// Returns the canonical command name string (matches serde tag).
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::IngestSubmitLocal(_) => "ingest.submit_local",
            Self::IngestSubmitManaged(_) => "ingest.submit_managed",
            Self::PreflightRun(_) => "preflight.run",
            Self::BindingCreate(_) => "binding.create",
            Self::BindingInspect(_) => "binding.inspect",
            Self::WorkspaceBind(_) => "workspace.bind",
            Self::WorkspaceResolve(_) => "workspace.resolve",
            Self::ReviewAcceptCandidate(_) => "review.accept_candidate",
            Self::ReviewRejectCandidate(_) => "review.reject_candidate",
            Self::CandidateHide(_) => "candidate.hide",
            Self::CandidateSuppress(_) => "candidate.suppress",
            Self::ReviewApproveSignoff(_) => "review.approve_signoff",
            Self::ReviewRejectSignoff(_) => "review.reject_signoff",
            Self::ReviewResolveCompliance(_) => "review.resolve_compliance",
            Self::EvidenceRefresh(_) => "evidence.refresh",
            Self::TrackingUntrackSource(_) => "tracking.untrack_source",
            Self::TrackingRefreshQuery(_) => "tracking.refresh_query",
            Self::CodeLocate(_) => "code.locate",
            Self::GraphStatus(_) => "graph.status",
            Self::GraphRefreshSnapshot(_) => "graph.refresh_snapshot",
            Self::DecisionFindCodeImpact(_) => "decision.find_code_impact",
            Self::RecallInspectEvidence(_) => "recall.inspect_evidence",
            Self::RecallExpandScope(_) => "recall.expand_scope",
            Self::RecallPromoteDecisionCandidate(_) => "recall.promote_decision_candidate",
            Self::RecallRequestCorrection(_) => "recall.request_correction",
            Self::PrivacyEraseSubject(_) => "privacy.erase_subject",
            Self::ReviewTransitionFinding(_) => "review.transition_finding",
            Self::HistoryList(_) => "history.list",
            Self::SearchQuery(_) => "search.query",
            Self::ProductCreate(_) => "product.create",
            Self::ProductPublish(_) => "product.publish",
            Self::ProductJoin(_) => "product.join",
            Self::ProductContinueSetup(_) => "product.continue_setup",
            Self::CandidateSelectionPrepare(_) => "candidate_selection.prepare",
            Self::CandidateSelectionCommit(_) => "candidate_selection.commit",
            Self::CandidateSessionHeartbeat(_) => "candidate_session.heartbeat",
            Self::CandidateSessionEnd(_) => "candidate_session.end",
        }
    }

    /// Compatibility/readiness metadata only; not governance authority.
    ///
    /// Must stay aligned with the runtime capability advertisement lists.
    pub fn capability_class(&self) -> CommandCapabilityClass {
        match self {
            // Deferred = not an MCP alpha thin-client tool. The first four fail
            // closed when dispatched; `product.*` are fully implemented but
            // reachable only through the validated hosted bridge surface (#766),
            // never as MCP tools.
            Self::ReviewResolveCompliance(_)
            | Self::TrackingUntrackSource(_)
            | Self::TrackingRefreshQuery(_)
            | Self::PrivacyEraseSubject(_)
            | Self::ProductCreate(_)
            | Self::ProductPublish(_) => CommandCapabilityClass::Deferred,
            Self::RecallRequestCorrection(_)
            | Self::IngestSubmitLocal(_)
            | Self::IngestSubmitManaged(_)
            | Self::PreflightRun(_)
            | Self::BindingCreate(_)
            | Self::BindingInspect(_)
            | Self::WorkspaceBind(_)
            | Self::WorkspaceResolve(_)
            | Self::ReviewAcceptCandidate(_)
            | Self::ReviewRejectCandidate(_)
            | Self::CandidateHide(_)
            | Self::CandidateSuppress(_)
            | Self::ReviewApproveSignoff(_)
            | Self::ReviewRejectSignoff(_)
            | Self::EvidenceRefresh(_)
            | Self::CodeLocate(_)
            | Self::GraphStatus(_)
            | Self::GraphRefreshSnapshot(_)
            | Self::DecisionFindCodeImpact(_)
            | Self::RecallInspectEvidence(_)
            | Self::RecallExpandScope(_)
            | Self::RecallPromoteDecisionCandidate(_)
            | Self::ReviewTransitionFinding(_)
            | Self::HistoryList(_)
            | Self::SearchQuery(_)
            | Self::ProductJoin(_)
            | Self::ProductContinueSetup(_)
            | Self::CandidateSelectionPrepare(_)
            | Self::CandidateSelectionCommit(_)
            | Self::CandidateSessionHeartbeat(_)
            | Self::CandidateSessionEnd(_) => CommandCapabilityClass::Supported,
        }
    }
}

/// Compatibility/readiness class for a tool command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCapabilityClass {
    Supported,
    Deferred,
}

// ---------------------------------------------------------------------------
// Command payloads
// ---------------------------------------------------------------------------

/// Payload for `ingest.submit_local`.
///
/// Per the runtime spec, accepted fields: `source_uri`, `source_type`, `title`,
/// `description`, `evidence[]`, `candidate_drafts[]`, `binding_hints[]`,
/// `rationale`, `suggested_level`, and `metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestSubmitLocalParams {
    pub source_uri: String,
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub title: String,
    pub description: String,
    /// Advisory governance level hint — governance-owned, never authoritative
    /// from caller. Renamed from `level` in the first-pass field set.
    #[serde(default, alias = "level", skip_serializing_if = "Option::is_none")]
    pub suggested_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_content: Option<String>,
    #[serde(default)]
    pub evidence: Vec<IngestEvidenceItem>,
    /// Optional candidate drafts — become reviewable `DecisionCandidate` material
    /// without becoming tracked Decisions or signoffs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_drafts: Vec<CandidateDraft>,
    /// Advisory binding hints — code regions the caller believes are relevant.
    /// Never verified evidence; advisory only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binding_hints: Vec<BindingHint>,
    /// Caller rationale or LLM reasoning — advisory only, never verified
    /// source evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// Inert caller metadata — preserved for telemetry/diagnostics but never
    /// treated as governance authority or verified evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Payload for `ingest.submit_managed`.
///
/// Managed source ingestion requires an active provider connection and
/// an approved source binding before evidence enters the pipeline. This
/// separates provider auth readiness from source approval — connecting a
/// provider does not by itself authorize ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestSubmitManagedParams {
    /// Provider identifier (e.g. `"github"`, `"google"`).
    pub provider_id: String,

    /// Source binding identifier within the project (e.g. `"github_repo_primary"`).
    pub source_binding_id: String,

    /// Source URI for provenance.
    pub source_uri: String,

    /// Title for the primary candidate (when ingestion succeeds).
    pub title: String,

    /// Description for the primary candidate.
    pub description: String,

    /// Snapshot content from the managed source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_content: Option<String>,

    /// Evidence items extracted from the managed source.
    #[serde(default)]
    pub evidence: Vec<IngestEvidenceItem>,

    /// Inert caller metadata — preserved for telemetry/diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A single evidence item within an ingest submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestEvidenceItem {
    pub excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_end: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// A candidate draft within an ingest submission — becomes reviewable
/// `DecisionCandidate` material without becoming a tracked Decision or signoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateDraft {
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_level: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// An advisory binding hint — a code region the caller believes is relevant.
/// Never verified evidence; advisory until bot-owned validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingHint {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Payload for `preflight.run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightRunParams {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Inert checkpoint hint — trace/relevance metadata only.
    ///
    /// Tells the daemon where in the session the lookup was requested.
    /// The daemon may record this in traces or relevance diagnostics but
    /// must not treat the hint as permission to capture, block, enforce,
    /// or mutate state. Protocol semantics, persistence, guard behavior,
    /// and authority are identical regardless of the hint value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_hint: Option<CheckpointHint>,
    /// Exact post-Planning host boundary for the Managed MCP journey.
    ///
    /// Absent for manual and legacy preflight calls. When present, the daemon
    /// resolves the governing specification from retained Git bytes before any
    /// semantic provider or deterministic analysis may run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_planning: Option<crate::managed_preflight::ManagedPlanningBoundaryV1>,
}

/// Inert checkpoint hint for `preflight.run` lookup requests.
///
/// Indicates where in the session the lookup was requested. This is
/// trace/relevance metadata only — it does not change protocol semantics,
/// persistence, guard behavior, capture, authority, or any other
/// daemon behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointHint {
    /// Before starting implementation work.
    PreWork,
    /// During an active implementation session.
    MidSession,
    /// Before writing/committing code changes.
    PreWrite,
    /// Explicit user-initiated lookup (not session-automated).
    ManualLookup,
}

/// Payload for `binding.create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingCreateParams {
    pub decision_or_candidate_id: String,
    pub bindings: Vec<BindingSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// Expected graph snapshot identity. When provided, the daemon rejects
    /// the request with `snapshot_mismatch` if its resolved snapshot differs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    /// Optional validation token for future handoff (ADR-0017).
    ///
    /// In the first implementation slice, tokens are represented in the
    /// protocol but always return `validation_token_unsupported`. No
    /// implementation may accept an unsigned or unprotected token as proof
    /// of verified evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_token: Option<String>,
}

/// A single binding specification within a create/inspect request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingSpec {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// When present, indicates this binding was selected from a
    /// `decision.find_code_impact` discovery result. The daemon validates
    /// that the discovery evidence state allows binding before
    /// re-verifying through the graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_discovery: Option<DiscoverySource>,
}

/// Provenance metadata from a `decision.find_code_impact` result that
/// the caller attaches when creating a binding from a discovery candidate.
///
/// The daemon uses this to enforce the binding lifecycle invariant:
/// only verified, sufficient-scope candidates may produce accepted
/// `BindingEvidence`. Non-verified evidence states (approximate, ambiguous,
/// stale, unindexed, unsupported, graph-unavailable, not-found) are
/// rejected before re-verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySource {
    /// Evidence state of the candidate as reported by discovery.
    pub evidence_state: LocatorEvidenceState,
    /// Graph snapshot identity at which the candidate was discovered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    /// Commit SHA at which the candidate was validated during discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_sha: Option<String>,
    /// Content hash of the target region from discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Total number of candidates returned by the discovery result.
    /// When > 1 with non-verified states, the daemon requires explicit
    /// selection (the caller must pick one candidate, not batch-bind all).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<usize>,
}

/// Payload for `binding.inspect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingInspectParams {
    pub decision_or_candidate_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Optional ref for currentness check during inspection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// Expected graph snapshot identity for currentness check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
}

/// Payload for review commands that target a single entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTargetParams {
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for `candidate.hide`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateHideParams {
    pub candidate_id: String,
}

/// Payload for `candidate.suppress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSuppressParams {
    pub candidate_id: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Payload for `review.resolve_compliance`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveComplianceParams {
    pub target_id: String,
    pub compliance_verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for `review.transition_finding`.
///
/// Transitions a contradiction finding's status. The `report_id` must
/// reference an existing `ContradictionReport`. Valid transitions follow
/// the `FindingStatus` state machine (open → acknowledged → resolved/dismissed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionFindingParams {
    /// The `ContradictionReport.id` to transition.
    pub report_id: String,
    /// Target status (acknowledged, resolved, dismissed).
    pub to_status: String,
    /// Optional reason/note for the transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Payload for `evidence.refresh`.
///
/// Requests evidence currentness refresh for a single tracked Decision.
/// The bot runtime resolves the local repo snapshot, inspects existing
/// binding evidence, and materializes a derived currentness outcome
/// without mutating signoff, compliance, or accepted `BindingEvidence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRefreshParams {
    /// Decision id to refresh evidence for.
    pub decision_id: String,
}

/// Payload for `tracking.untrack_source`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UntrackSourceParams {
    /// Decision id to stop tracking.
    pub decision_id: String,
    /// Why tracking is being stopped (maps to `UntrackReason`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optional free-form detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Payload for `tracking.refresh_query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshQueryParams {
    /// `"auto"` (default) or `"manual"`.
    #[serde(default = "RefreshQueryParams::default_mode")]
    pub mode: String,
}

impl RefreshQueryParams {
    fn default_mode() -> String {
        "auto".to_string()
    }
}

/// Payload for `privacy.erase_subject`.
///
/// Daemon-side right-to-erasure request parameters. MCP's `ErasureGate`
/// validates single-use approval before forwarding. The daemon either
/// delegates to a PII archive or fails closed when no archive is present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyEraseSubjectParams {
    /// Opaque subject identifier whose PII should be erased.
    pub subject_id: String,
    /// Optional predicate narrowing the erasure scope (e.g. `"speaker"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    /// Free-form reason for the erasure request (audit trail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for `history.list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(default)]
    pub include_events: bool,
    #[serde(default)]
    pub include_bindings: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
}

/// Payload for `search.query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueryParams {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<SearchScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<SearchFilters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Scope for search queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Decisions,
    Candidates,
    Bindings,
    All,
}

/// Optional filters for search queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signoff_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

// ---------------------------------------------------------------------------
// decision.find_code_impact payloads
// ---------------------------------------------------------------------------

/// Payload for `decision.find_code_impact`.
///
/// Requests one-click code-impact discovery for an accepted Decision.
/// The caller does not need to write a manual code query — the runtime
/// uses the Decision's source context and optional caller hints to plan
/// and execute code location.
///
/// This is a discovery/status command. It does not materialize
/// `BindingEvidence`, resolve compliance, produce signoff, assert
/// safety/no-conflict, or emit blocking governance results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindCodeImpactParams {
    /// Accepted Decision identifier.
    pub decision_id: String,

    /// Optional caller-provided search hints (files, symbols, keywords)
    /// to narrow or seed the search plan. Advisory only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<FindCodeImpactHint>,

    /// Optional workspace identifier for graph resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,

    /// Optional ref name for snapshot resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,

    /// Optional commit SHA for snapshot pinning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,

    /// Maximum candidate regions to return.
    #[serde(default = "FindCodeImpactParams::default_limit")]
    pub limit: usize,
}

impl FindCodeImpactParams {
    fn default_limit() -> usize {
        20
    }
}

/// A caller-provided hint to seed or narrow the code-impact search.
/// Advisory only — never treated as verified evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindCodeImpactHint {
    /// Hint kind (e.g. `"file"`, `"symbol"`, `"keyword"`).
    pub kind: FindCodeImpactHintKind,
    /// Hint value (file path, symbol name, or keyword).
    pub value: String,
}

/// Kind of caller hint for code-impact discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindCodeImpactHintKind {
    File,
    Symbol,
    Keyword,
}

/// Result of a `decision.find_code_impact` command.
///
/// Represents the binding lifecycle state after discovery:
/// `BindingDiscoveryRequested → BindingSearchPlanned → BindingCandidatesReady`.
///
/// Candidates are **not** accepted `BindingEvidence`. They require explicit
/// `binding.create` to become durable bindings. This result cannot encode
/// compliance, safety/no-conflict, blocking, signoff, or automatic
/// accepted-binding authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindCodeImpactResult {
    /// Overall discovery outcome status.
    pub status: FindCodeImpactStatus,

    /// Decision identifier echoed from request.
    pub decision_id: String,

    /// The lifecycle phase this result represents.
    pub phase: BindingDiscoveryPhase,

    /// Graph readiness at the time of discovery.
    pub graph_readiness: LocatorGraphReadiness,

    /// Snapshot identity used for discovery (when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<LocatorSnapshotInfo>,

    /// Search plan summary (when the planner executed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_plan: Option<SearchPlanSummary>,

    /// Candidate code regions found by the search.
    ///
    /// These are discovery candidates, **not** accepted `BindingEvidence`.
    /// Each candidate must go through `binding.create` to become a durable
    /// binding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CodeImpactCandidate>,

    /// Currentness and status caveats (e.g. graph staleness, partial index).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<CodeImpactCaveat>,

    /// Suggested next actions for the caller.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<FindCodeImpactNextAction>,

    /// Daemon-authored semantic recall contribution for the bounded
    /// `code.locate` queries this discovery seeded (issue #640). Evidence-only
    /// read-model: renders graph-verified semantic targets and approximate
    /// hints (kept distinct) plus caveats/next actions. Never materializes
    /// `BindingEvidence` and never asserts compliance, signoff, safety, or
    /// blocking. Absent (`None`) when no semantic provider is configured, so
    /// the no-op stays quiet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_recall: Option<crate::semantic_recall::SemanticRecallView>,
}

/// Top-level status of a find-code-impact result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindCodeImpactStatus {
    /// Candidates found.
    Found,
    /// Approximate candidates found but human selection needed.
    NeedsHumanSelection,
    /// No candidates found.
    NotFound,
    /// Graph is unavailable (workspace not configured or graph service down).
    GraphUnavailable,
    /// Graph is stale — results may not reflect current code.
    GraphStale,
    /// Decision or request is unsupported (e.g. wrong lifecycle state).
    Unsupported,
}

/// Binding discovery lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingDiscoveryPhase {
    /// Discovery was requested but no plan was produced yet.
    DiscoveryRequested,
    /// A search plan was produced.
    SearchPlanned,
    /// Candidates are ready for human review / binding creation.
    CandidatesReady,
}

/// Summary of the search plan produced by the planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPlanSummary {
    /// Strategy used by the planner (e.g. `"source_context"`, `"hint_seeded"`).
    pub strategy: String,
    /// Number of locator queries executed.
    pub queries_executed: usize,
    /// Whether the plan was constrained by caller hints.
    pub hint_seeded: bool,
}

/// A candidate code region from find-code-impact discovery.
///
/// This is a discovery candidate — **not** accepted `BindingEvidence`.
/// Only verified candidates may proceed to `binding.create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeImpactCandidate {
    /// Target identifier (file path, symbol, region descriptor).
    pub target: String,
    /// Target type.
    pub target_type: LocatorTargetType,
    /// File path containing the candidate region.
    pub file: String,
    /// Line range within the file (e.g. `"10:1-25:1"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// Symbol name (when target is a symbol).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Evidence state from deterministic graph/runtime validation.
    pub evidence_state: LocatorEvidenceState,
    /// Confidence ranking.
    pub confidence: LocatorConfidence,
    /// Reasons explaining why this candidate was ranked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<LocatorReason>,
    /// Snapshot identity at which this candidate was resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    /// Commit SHA at which the candidate was validated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_sha: Option<String>,
    /// Content hash of the target region (when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl CodeImpactCandidate {
    /// Only `Verified` candidates may proceed to `binding.create`.
    pub fn allows_binding(&self) -> bool {
        self.evidence_state.allows_binding()
    }
}

/// Currentness or status caveat attached to a find-code-impact result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeImpactCaveat {
    /// Caveat kind.
    pub kind: CodeImpactCaveatKind,
    /// Human-readable detail.
    pub detail: String,
}

/// Kind of caveat in a find-code-impact result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeImpactCaveatKind {
    /// Graph snapshot is stale relative to the current HEAD.
    GraphStale,
    /// Graph index is partial (not all files/symbols indexed).
    PartialIndex,
    /// Decision source context is limited.
    LimitedSourceContext,
    /// Some candidates are approximate (not verified).
    ApproximateCandidates,
    /// Locator result was truncated by limit.
    Truncated,
}

/// A structured next action suggested by the find-code-impact result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindCodeImpactNextAction {
    /// Action identifier (e.g. `"binding_create"`, `"refresh_graph"`).
    pub action: String,
    /// Human-readable description.
    pub description: String,
    /// The command that would execute this action (when applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_hint: Option<String>,
}

// ---------------------------------------------------------------------------
// Graph / CodeLocator payloads
// ---------------------------------------------------------------------------

/// Payload for `code.locate`.
///
/// At least one of `query`, `decision_id`, `candidate_id`, `files`,
/// `symbols`, `changed_files`, `binding_ids`, or `source_evidence_ids`
/// is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLocateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_evidence_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binding_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<LocatorScopeKind>,
    #[serde(default = "CodeLocateParams::default_limit")]
    pub limit: usize,
}

impl CodeLocateParams {
    fn default_limit() -> usize {
        20
    }
}

/// Scope kind for locator and graph refresh queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorScopeKind {
    Binding,
    Diff,
    FileSet,
    SymbolSet,
    Query,
    BindingRevalidation,
}

/// Result of a `code.locate` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocatorResult {
    pub status: LocatorStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub graph_readiness: LocatorGraphReadiness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<LocatorSnapshotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<LocatorScopeInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<LocatorCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
}

/// Top-level status of a locator result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorStatus {
    Found,
    Ambiguous,
    NotFound,
    GraphUnavailable,
    GraphNotConfigured,
    Unsupported,
    Failed,
}

/// Graph readiness as reported inline in a locator result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorGraphReadiness {
    Ready,
    Indexing,
    Stale,
    Failed,
    NotConfigured,
}

/// Snapshot identity attached to a locator result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatorSnapshotInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
}

/// Scope proof info for a locator result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocatorScopeInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_state: Option<ScopeProofState>,
}

/// Sufficiency proof state for a graph scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeProofState {
    Sufficient,
    Insufficient,
    Stale,
    Failed,
    HostedRequired,
    NotAvailable,
}

/// A single code target candidate within a locator result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocatorCandidate {
    pub target: String,
    pub target_type: LocatorTargetType,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub evidence_state: LocatorEvidenceState,
    pub confidence: LocatorConfidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<LocatorReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
}

/// Target type for a locator candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorTargetType {
    FilePath,
    CodeRegion,
    Symbol,
    Dependency,
    DeploySurface,
}

/// Evidence state for a locator candidate.
///
/// This mirrors the core `GraphEvidenceState` but is scoped to the locator
/// result wire format. Only `Verified` candidates may become accepted
/// `BindingEvidence` through the governed materialization path.
/// Non-verified states are informational — they must not produce binding
/// authority, compliance conclusions, or blocking governance results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorEvidenceState {
    Verified,
    NotFound,
    UnknownNotIndexed,
    UnknownStale,
    Ambiguous,
    Unsupported,
    ApproximateCandidate,
}

impl LocatorEvidenceState {
    /// Only `Verified` evidence may produce binding-authoritative outcomes.
    pub fn allows_binding(&self) -> bool {
        matches!(self, LocatorEvidenceState::Verified)
    }
}

/// Confidence level for a locator candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

/// A reason code explaining why a locator candidate was ranked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocatorReason {
    pub code: LocatorReasonCode,
    pub detail: String,
}

/// Reason codes for locator ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorReasonCode {
    ExactPath,
    ExactSymbol,
    LexicalMatch,
    ChangedFile,
    PriorBinding,
    ImportEdge,
    CallEdge,
    ReferenceEdge,
    SourceEvidenceOverlap,
    /// Semantic provider candidate verified through deterministic graph proof.
    SemanticTarget,
    /// Approximate semantic/provider hint without deterministic graph proof.
    SemanticHint,
}

/// Payload for `graph.status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStatusParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
}

/// Result of a `graph.status` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStatusResult {
    pub readiness: LocatorGraphReadiness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<LocatorSnapshotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
}

/// Payload for `graph.refresh_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRefreshParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<RefreshScopeKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    #[serde(default = "GraphRefreshParams::default_allow_index")]
    pub allow_index: AllowIndex,
    #[serde(default = "GraphRefreshParams::default_trigger")]
    pub trigger: RefreshTrigger,
}

impl GraphRefreshParams {
    fn default_allow_index() -> AllowIndex {
        AllowIndex::Bounded
    }
    fn default_trigger() -> RefreshTrigger {
        RefreshTrigger::Manual
    }
}

/// Scope kind for graph refresh requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshScopeKind {
    Diff,
    FileSet,
    SymbolSet,
    Query,
    BindingRevalidation,
}

/// Index permission level for graph refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowIndex {
    /// Do not build or extend the index.
    False,
    /// Build a bounded index for the requested scope.
    Bounded,
    /// Full index (may require admin/policy gate in the future).
    Full,
}

/// Trigger source for a graph refresh request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshTrigger {
    Manual,
    LocatorRequest,
    EvidenceRefresh,
    Preflight,
    PrOpened,
    PrUpdated,
    BranchHeadChanged,
    GitHook,
    DaemonWatcher,
}

/// Result of a `graph.refresh_snapshot` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRefreshResult {
    pub status: GraphRefreshStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<GraphRefreshTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<LocatorSnapshotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice: Option<GraphSliceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<GraphJobInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
}

/// Top-level status of a graph refresh result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphRefreshStatus {
    Ready,
    Indexing,
    Stale,
    Failed,
    NotConfigured,
    Unsupported,
    /// Requested scope exceeds the bounded budget; refresh was declined to
    /// avoid an unbounded rebuild.
    OverBudget,
}

/// Target resolution info for a graph refresh result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRefreshTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_head_sha: Option<String>,
    #[serde(default)]
    pub is_current_for_target: bool,
}

/// Graph slice info within a refresh result.
// PartialEq/Eq added so read-models embedding this slice info (e.g.
// PreflightFreshness, issue #625) can derive structural equality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSliceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_slice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completeness: Option<SliceCompleteness>,
}

/// Completeness of a graph slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceCompleteness {
    DiffScoped,
    FileScoped,
    SymbolScoped,
    QueryScoped,
    RepairScoped,
    NotAvailable,
}

/// Graph job info within a refresh result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphJobInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<GraphJobState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by_sha: Option<String>,
}

/// State of a graph refresh job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphJobState {
    Queued,
    Indexing,
    Ready,
    Stale,
    Failed,
    Superseded,
    Cancelled,
}

// ---------------------------------------------------------------------------
// RuntimeTrace — operational receipt for correlation and diagnostics
// ---------------------------------------------------------------------------

/// Runtime trace correlation object for Bot-MCP lifecycle commands.
///
/// Emitted alongside `ToolResponse` to correlate MCP and bot work across
/// success and failure paths. This is an operational receipt — it explains
/// runtime behavior but **cannot** create Decisions, approve signoff, resolve
/// compliance, or emit blocking governance results.
///
/// Privacy: avoids logging prompt/source content by default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTrace {
    /// Correlates with `ToolRequest.request_id`.
    pub request_id: Uuid,

    /// The command name that was dispatched (e.g. `"ingest.submit_local"`).
    pub command: String,

    /// The surface that submitted the request (e.g. `"mcp"`, `"cli"`, `"dashboard"`).
    pub caller_surface: String,

    /// Actor identity from `AuthorityContext`.
    pub actor_id: String,

    /// Canonical Product route selected for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,

    /// Workspace/repo scope from `AuthorityContext`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,

    /// Session context from `AuthorityContext`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Reasoning provider identifier (e.g. `"openai"`, `"anthropic"`).
    /// Populated when an LLM call was involved in processing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Model identifier (e.g. `"gpt-4"`, `"claude-3"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Classification of the reasoning task (e.g. `"extraction"`, `"grounding"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_kind: Option<String>,

    /// Token usage for LLM calls associated with this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,

    /// End-to-end latency in milliseconds for command processing.
    pub latency_ms: u64,

    /// Outcome status mirroring `ToolResponseStatus`.
    pub status: TraceStatus,

    /// Failure or rejection reason when status is not `ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,

    /// Entity IDs touched during processing (decisions, candidates, bindings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched_ids: Vec<String>,
}

/// Token usage metadata for reasoning provider spans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input/prompt tokens consumed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,

    /// Output/completion tokens produced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,

    /// Total tokens (input + output) if not separately tracked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

/// Trace-level status for runtime correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    /// Command completed successfully.
    Ok,
    /// Command rejected by governance policy.
    Rejected,
    /// Command failed due to internal error.
    Error,
}

impl From<ToolResponseStatus> for TraceStatus {
    fn from(s: ToolResponseStatus) -> Self {
        match s {
            ToolResponseStatus::Ok => TraceStatus::Ok,
            ToolResponseStatus::Rejected => TraceStatus::Rejected,
            ToolResponseStatus::Error => TraceStatus::Error,
        }
    }
}

impl RuntimeTrace {
    /// Begin constructing a trace from a `ToolRequest`.
    ///
    /// Populates correlation fields from the request's authority context.
    /// Caller must finalize `latency_ms`, `status`, `failure_reason`, and
    /// `touched_ids` after command execution completes.
    pub fn begin(request: &ToolRequest) -> Self {
        Self {
            request_id: request.request_id,
            command: request.command.command_name().to_string(),
            caller_surface: caller_surface_from_auth(&request.authority),
            actor_id: request.authority.actor_id.clone(),
            product_id: request.product_id.clone(),
            workspace_id: request.authority.workspace.clone(),
            session_id: request.authority.session_id.clone(),
            provider: None,
            model: None,
            task_kind: None,
            token_usage: None,
            latency_ms: 0,
            status: TraceStatus::Ok,
            failure_reason: None,
            touched_ids: Vec::new(),
        }
    }

    /// Finalize the trace with execution outcome.
    pub fn finalize(
        mut self,
        status: TraceStatus,
        latency_ms: u64,
        failure_reason: Option<String>,
        touched_ids: Vec<String>,
    ) -> Self {
        self.status = status;
        self.latency_ms = latency_ms;
        self.failure_reason = failure_reason;
        self.touched_ids = touched_ids;
        self
    }

    /// Attach reasoning provider span metadata.
    pub fn with_provider_span(
        mut self,
        provider: String,
        model: String,
        task_kind: String,
        token_usage: Option<TokenUsage>,
    ) -> Self {
        self.provider = Some(provider);
        self.model = Some(model);
        self.task_kind = Some(task_kind);
        self.token_usage = token_usage;
        self
    }
}

/// Derive the caller surface label from `AuthMethod` and audit metadata.
fn caller_surface_from_auth(authority: &AuthorityContext) -> String {
    // Prefer explicit `surface` key in audit_metadata if present.
    if let Some(serde_json::Value::String(s)) = authority.audit_metadata.get("surface") {
        return s.clone();
    }
    // Fall back to auth_method.
    match authority.auth_method {
        AuthMethod::LocalDaemon => "local_daemon".to_string(),
        AuthMethod::McpSession => "mcp".to_string(),
        AuthMethod::Cli => "cli".to_string(),
        AuthMethod::Dashboard => "dashboard".to_string(),
        AuthMethod::HostedBridge => "hosted_bridge".to_string(),
    }
}

// ---------------------------------------------------------------------------
// ToolResponse
// ---------------------------------------------------------------------------

/// The canonical local tool API response object.
///
/// Returned by the daemon after validating and executing a `ToolRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    /// Correlates with `ToolRequest.request_id`.
    pub request_id: Uuid,

    /// Whether the command succeeded, was rejected by policy, or failed.
    pub status: ToolResponseStatus,

    /// Command-specific result payload (JSON).  `null` when status is `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// Human-readable message (error detail, governance rejection reason, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Governance result produced by policy evaluation, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance_result: Option<serde_json::Value>,

    /// Runtime trace correlation for diagnostics and observability.
    /// Operational receipt only — never governance authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<RuntimeTrace>,

    /// Timestamp at which the daemon produced this response.
    pub responded_at: DateTime<Utc>,
}

impl ToolResponse {
    pub fn success(request_id: Uuid, result: serde_json::Value) -> Self {
        Self {
            request_id,
            status: ToolResponseStatus::Ok,
            result: Some(result),
            message: None,
            governance_result: None,
            trace: None,
            responded_at: Utc::now(),
        }
    }

    pub fn rejected(request_id: Uuid, reason: String) -> Self {
        Self {
            request_id,
            status: ToolResponseStatus::Rejected,
            result: None,
            message: Some(reason),
            governance_result: None,
            trace: None,
            responded_at: Utc::now(),
        }
    }

    pub fn error(request_id: Uuid, message: String) -> Self {
        Self {
            request_id,
            status: ToolResponseStatus::Error,
            result: None,
            message: Some(message),
            governance_result: None,
            trace: None,
            responded_at: Utc::now(),
        }
    }

    /// Attach a `RuntimeTrace` to this response.
    pub fn with_trace(mut self, trace: RuntimeTrace) -> Self {
        self.trace = Some(trace);
        self
    }
}

/// Status of a `ToolResponse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResponseStatus {
    /// Command executed successfully.
    Ok,
    /// Command rejected by governance policy (not an error — policy worked as designed).
    Rejected,
    /// Internal error (validation failure, substrate error, etc.).
    Error,
}

// ---------------------------------------------------------------------------
// ExternalIngestEnvelope
// ---------------------------------------------------------------------------

/// External/source-system evidence submission contract.
///
/// External integrations (Jira, Linear, Slack, GitHub webhooks, etc.) use this
/// envelope to submit evidence, candidates, hints, and signals.  The envelope
/// is **not** a `ToolRequest` — it enters the daemon through a separate
/// validation path (the external ingest gate) and cannot carry or trigger:
///
/// - Review commands (`accept_candidate`, `reject_candidate`, `approve_signoff`, etc.)
/// - Canonical decisions or event-store writes
/// - Signoff approval or compliance resolution
/// - Policy grants or local actor/session authority
/// - Blocking governance results
///
/// The daemon validates the envelope, projects candidates/evidence through
/// policy, and returns only advisory governance results.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalIngestEnvelope {
    /// Source system identifier (e.g. `jira`, `linear`, `slack`, `github`).
    pub source_system: String,

    /// Source URI for provenance.
    pub source_uri: String,

    /// Captured content snapshot.
    pub content: String,

    /// Content hash (SHA-256). Daemon verifies if provided, computes if omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,

    /// Evidence excerpts within the content.
    #[serde(default)]
    pub evidence: Vec<ExternalEvidenceItem>,

    /// Candidate suggestions — advisory only, not canonical authority.
    #[serde(default)]
    pub candidate_hints: Vec<ExternalCandidateHint>,
}

/// An evidence excerpt from an external source.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalEvidenceItem {
    pub excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_end: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// A candidate hint from an external source — advisory, not authority.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalCandidateHint {
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_request_roundtrip() {
        let authority = AuthorityContext {
            actor_id: "agent-001".into(),
            auth_method: AuthMethod::McpSession,
            session_id: Some("sess-abc".into()),
            workspace: Some("/repo".into()),
            policy_scope: vec!["default".into()],
            audit_metadata: serde_json::Map::new(),
        };
        let cmd = ToolCommand::PreflightRun(PreflightRunParams {
            files: vec!["src/main.rs".into()],
            symbols: vec![],
            diff_context: None,
            branch: None,
            checkpoint_hint: None,
            managed_planning: None,
        });
        let req = ToolRequest::new(cmd, authority);
        let json = serde_json::to_string_pretty(&req).unwrap();
        let parsed: ToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
    }

    #[test]
    fn tool_response_success_roundtrip() {
        let id = Uuid::new_v4();
        let resp = ToolResponse::success(id, serde_json::json!({"decisions": []}));
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ToolResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, id);
        assert_eq!(parsed.status, ToolResponseStatus::Ok);
    }

    #[test]
    fn tool_response_rejected_roundtrip() {
        let id = Uuid::new_v4();
        let resp = ToolResponse::rejected(id, "policy_violation".into());
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ToolResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, ToolResponseStatus::Rejected);
    }

    #[test]
    fn command_registry_serde_tags() {
        let cmd = ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: "CAND-001".into(),
            reason: Some("evidence sufficient".into()),
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("review.accept_candidate"));

        let cmd2 = ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: "https://jira.example.com/PROJ-1".into(),
            source_type: "jira".into(),
            label: None,
            title: "Add healthz".into(),
            description: "Needs /healthz endpoint".into(),
            suggested_level: Some("architecture".into()),
            snapshot_content: None,
            evidence: vec![IngestEvidenceItem {
                excerpt: "The service needs a /healthz endpoint".into(),
                span_start: Some(0),
                span_end: Some(37),
                confidence: Some(0.9),
            }],
            candidate_drafts: vec![],
            binding_hints: vec![],
            rationale: None,
            metadata: None,
        });
        let json2 = serde_json::to_string(&cmd2).unwrap();
        assert!(json2.contains("ingest.submit_local"));
    }

    #[test]
    fn capability_class_marks_recall_request_correction_supported() {
        let cmd =
            ToolCommand::RecallRequestCorrection(crate::recall::RecallRequestCorrectionParams {
                packet_id: Uuid::nil(),
                selected_item_ids: vec![Uuid::nil()],
                correction_kind: crate::recall::CorrectionKind::SourceContradiction,
                rationale: "test".into(),
                approval_proof: crate::recall::ApprovalProof {
                    kind: "signoff_token".into(),
                    value: "tok".into(),
                    actor_id: "user-owner-01".into(),
                    provided_at: None,
                },
            });
        assert_eq!(cmd.capability_class(), CommandCapabilityClass::Supported);
    }

    #[test]
    fn external_ingest_envelope_cannot_carry_review_commands() {
        // ExternalIngestEnvelope has no field for review commands, tool
        // commands, authority context, or governance overrides — this test
        // verifies the type-level separation by confirming that injecting
        // forbidden fields is silently ignored during deserialization.
        let json = r#"{
            "source_system": "jira",
            "source_uri": "https://jira.example.com/PROJ-1",
            "content": "Some ticket content",
            "evidence": [],
            "candidate_hints": [],
            "review_command": "approve_signoff",
            "authority": { "actor_id": "attacker", "auth_method": "local_daemon" },
            "tool_command": { "name": "review.approve_signoff", "params": { "target_id": "DEC-1" } }
        }"#;
        let envelope: ExternalIngestEnvelope = serde_json::from_str(json).unwrap();
        // Forbidden fields are ignored — only the defined fields survive.
        assert_eq!(envelope.source_system, "jira");
        assert!(envelope.evidence.is_empty());
        assert!(envelope.candidate_hints.is_empty());
    }

    #[test]
    fn all_command_variants_deserialize() {
        let cases = vec![
            r#"{"name":"ingest.submit_local","params":{"source_uri":"u","source_type":"manual","title":"t","description":"d","evidence":[]}}"#,
            r#"{"name":"preflight.run","params":{"files":["a.rs"],"symbols":[]}}"#,
            r#"{"name":"binding.create","params":{"decision_or_candidate_id":"DEC-1","bindings":[{"file":"a.rs"}]}}"#,
            r#"{"name":"binding.inspect","params":{"decision_or_candidate_id":"DEC-1"}}"#,
            r#"{"name":"review.accept_candidate","params":{"target_id":"CAND-1"}}"#,
            r#"{"name":"review.reject_candidate","params":{"target_id":"CAND-1"}}"#,
            r#"{"name":"review.approve_signoff","params":{"target_id":"DEC-1"}}"#,
            r#"{"name":"review.reject_signoff","params":{"target_id":"DEC-1"}}"#,
            r#"{"name":"review.resolve_compliance","params":{"target_id":"DEC-1","compliance_verdict":"reflected"}}"#,
            r#"{"name":"tracking.untrack_source","params":{"decision_id":"DEC-1"}}"#,
            r#"{"name":"tracking.refresh_query","params":{}}"#,
            r#"{"name":"history.list","params":{}}"#,
            r#"{"name":"search.query","params":{"query":"healthz"}}"#,
            r#"{"name":"code.locate","params":{"query":"device flow auth","limit":20}}"#,
            r#"{"name":"graph.status","params":{}}"#,
            r#"{"name":"graph.refresh_snapshot","params":{}}"#,
            r#"{"name":"decision.find_code_impact","params":{"decision_id":"DEC-1","limit":20}}"#,
            r#"{"name":"candidate_selection.prepare","params":{"contract_version":1,"selection_operation_id":"00000000-0000-0000-0000-000000000001","candidate_set_id":"00000000-0000-0000-0000-000000000002","candidate_set_generation":1,"candidate_set_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","product_id":"prod","host_session_id":"host","session_lease_id":"lease","promote_candidate_ids":[],"reject_candidate_ids":[],"actor_id":"owner"}}"#,
            r#"{"name":"candidate_selection.commit","params":{"contract_version":1,"selection_operation_id":"00000000-0000-0000-0000-000000000001","selection_digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","actor_id":"owner","human_confirmed":true,"confirmation_token":{"challenge_id":"00000000-0000-0000-0000-000000000003","restart_epoch":"00000000-0000-0000-0000-000000000004","secret":"opaque"}}}"#,
            r#"{"name":"candidate_session.heartbeat","params":{"access":{"contract_version":1,"product_id":"prod","host_session_id":"host","session_lease_id":"lease","candidate_set_generation":1}}}"#,
            r#"{"name":"candidate_session.end","params":{"access":{"contract_version":1,"product_id":"prod","host_session_id":"host","session_lease_id":"lease","candidate_set_generation":1}}}"#,
        ];
        for (i, json) in cases.iter().enumerate() {
            let result: Result<ToolCommand, _> = serde_json::from_str(json);
            assert!(result.is_ok(), "case {i} failed: {}", result.unwrap_err());
        }
    }

    #[test]
    fn code_locate_command_tag() {
        let cmd = ToolCommand::CodeLocate(CodeLocateParams {
            query: Some("device flow".into()),
            decision_id: None,
            candidate_id: None,
            source_evidence_ids: vec![],
            binding_ids: vec![],
            files: vec![],
            symbols: vec![],
            changed_files: vec![],
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
            limit: 20,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("code.locate"));
        assert_eq!(cmd.command_name(), "code.locate");
    }

    #[test]
    fn graph_status_command_tag() {
        let cmd = ToolCommand::GraphStatus(GraphStatusParams {
            workspace_id: None,
            ref_name: Some("HEAD".into()),
            commit_sha: None,
            graph_snapshot_id: None,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("graph.status"));
        assert_eq!(cmd.command_name(), "graph.status");
    }

    #[test]
    fn graph_refresh_snapshot_command_tag() {
        let cmd = ToolCommand::GraphRefreshSnapshot(GraphRefreshParams {
            workspace_id: None,
            ref_name: Some("main".into()),
            commit_sha: None,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            scope_kind: None,
            files: vec![],
            symbols: vec![],
            allow_index: AllowIndex::Bounded,
            trigger: RefreshTrigger::Manual,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("graph.refresh_snapshot"));
        assert_eq!(cmd.command_name(), "graph.refresh_snapshot");
    }

    #[test]
    fn locator_evidence_state_serialization() {
        let cases = vec![
            (LocatorEvidenceState::Verified, "\"verified\""),
            (LocatorEvidenceState::NotFound, "\"not_found\""),
            (
                LocatorEvidenceState::UnknownNotIndexed,
                "\"unknown_not_indexed\"",
            ),
            (LocatorEvidenceState::UnknownStale, "\"unknown_stale\""),
            (LocatorEvidenceState::Ambiguous, "\"ambiguous\""),
            (LocatorEvidenceState::Unsupported, "\"unsupported\""),
            (
                LocatorEvidenceState::ApproximateCandidate,
                "\"approximate_candidate\"",
            ),
        ];
        for (state, expected) in &cases {
            let json = serde_json::to_string(state).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", state);
            let parsed: LocatorEvidenceState = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, state, "roundtrip mismatch for {:?}", state);
        }
    }

    #[test]
    fn locator_evidence_state_binding_authority() {
        assert!(LocatorEvidenceState::Verified.allows_binding());
        let non_binding = [
            LocatorEvidenceState::NotFound,
            LocatorEvidenceState::UnknownNotIndexed,
            LocatorEvidenceState::UnknownStale,
            LocatorEvidenceState::Ambiguous,
            LocatorEvidenceState::Unsupported,
            LocatorEvidenceState::ApproximateCandidate,
        ];
        for state in &non_binding {
            assert!(
                !state.allows_binding(),
                "{:?} must not allow binding authority",
                state
            );
        }
    }

    #[test]
    fn locator_graph_readiness_serialization() {
        let cases = vec![
            (LocatorGraphReadiness::Ready, "\"ready\""),
            (LocatorGraphReadiness::Indexing, "\"indexing\""),
            (LocatorGraphReadiness::Stale, "\"stale\""),
            (LocatorGraphReadiness::Failed, "\"failed\""),
            (LocatorGraphReadiness::NotConfigured, "\"not_configured\""),
        ];
        for (state, expected) in &cases {
            let json = serde_json::to_string(state).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", state);
        }
    }

    #[test]
    fn locator_result_roundtrip() {
        let result = LocatorResult {
            status: LocatorStatus::Found,
            query: Some("device flow auth".into()),
            graph_readiness: LocatorGraphReadiness::Ready,
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-001".into()),
                repo_id: Some("repo-1".into()),
                commit_sha: Some("abc123".into()),
                index_version: Some("1".into()),
                ref_name: Some("HEAD".into()),
            }),
            scope: Some(LocatorScopeInfo {
                scope_kind: Some("query".into()),
                scope_fingerprint: Some("fp-001".into()),
                proof_state: Some(ScopeProofState::Sufficient),
            }),
            candidates: vec![LocatorCandidate {
                target: "src/auth.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/auth.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::Verified,
                confidence: LocatorConfidence::High,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::ExactPath,
                    detail: "File exists at snapshot".into(),
                }],
                graph_snapshot_id: Some("gs-001".into()),
                validated_sha: Some("abc123".into()),
                content_hash: None,
                caveats: vec![],
            }],
            warnings: vec![],
            next_actions: vec!["choose_candidate".into()],
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: LocatorResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, result.status);
        assert_eq!(parsed.candidates.len(), 1);
        assert_eq!(
            parsed.candidates[0].evidence_state,
            LocatorEvidenceState::Verified
        );
    }

    #[test]
    fn graph_refresh_result_roundtrip() {
        let result = GraphRefreshResult {
            status: GraphRefreshStatus::Stale,
            target: Some(GraphRefreshTarget {
                requested_ref: Some("main".into()),
                resolved_sha: Some("c10".into()),
                current_head_sha: Some("c12".into()),
                is_current_for_target: false,
            }),
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-002".into()),
                repo_id: None,
                commit_sha: Some("c10".into()),
                index_version: Some("1".into()),
                ref_name: None,
            }),
            slice: Some(GraphSliceInfo {
                graph_slice_id: Some("sl-001".into()),
                scope_kind: Some("diff".into()),
                scope_fingerprint: None,
                completeness: Some(SliceCompleteness::DiffScoped),
            }),
            job: Some(GraphJobInfo {
                job_id: Some("job-001".into()),
                state: Some(GraphJobState::Stale),
                superseded_by_sha: Some("c12".into()),
            }),
            warnings: vec!["Target moved during indexing".into()],
            next_actions: vec!["refresh_latest".into()],
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: GraphRefreshResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, GraphRefreshStatus::Stale);
    }

    #[test]
    fn graph_status_result_roundtrip() {
        let result = GraphStatusResult {
            readiness: LocatorGraphReadiness::NotConfigured,
            snapshot: None,
            stale_detail: Some("workspace not configured for graph indexing".into()),
            next_actions: vec!["configure_workspace".into()],
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: GraphStatusResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.readiness, LocatorGraphReadiness::NotConfigured);
    }

    #[test]
    fn refresh_trigger_serialization() {
        let cases = vec![
            (RefreshTrigger::Manual, "\"manual\""),
            (RefreshTrigger::LocatorRequest, "\"locator_request\""),
            (RefreshTrigger::EvidenceRefresh, "\"evidence_refresh\""),
            (RefreshTrigger::Preflight, "\"preflight\""),
            (RefreshTrigger::PrOpened, "\"pr_opened\""),
            (RefreshTrigger::PrUpdated, "\"pr_updated\""),
            (RefreshTrigger::BranchHeadChanged, "\"branch_head_changed\""),
            (RefreshTrigger::GitHook, "\"git_hook\""),
            (RefreshTrigger::DaemonWatcher, "\"daemon_watcher\""),
        ];
        for (trigger, expected) in &cases {
            let json = serde_json::to_string(trigger).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", trigger);
        }
    }

    // -- decision.find_code_impact contract tests -------------------------

    #[test]
    fn find_code_impact_command_tag() {
        let cmd = ToolCommand::DecisionFindCodeImpact(FindCodeImpactParams {
            decision_id: "DEC-001".into(),
            hints: vec![],
            workspace_id: None,
            ref_name: None,
            commit_sha: None,
            limit: 20,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("decision.find_code_impact"));
        assert_eq!(cmd.command_name(), "decision.find_code_impact");
    }

    #[test]
    fn find_code_impact_params_roundtrip() {
        let params = FindCodeImpactParams {
            decision_id: "DEC-500".into(),
            hints: vec![
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::File,
                    value: "src/auth.rs".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Symbol,
                    value: "AuthProvider".into(),
                },
                FindCodeImpactHint {
                    kind: FindCodeImpactHintKind::Keyword,
                    value: "device flow".into(),
                },
            ],
            workspace_id: Some("/home/user/project".into()),
            ref_name: Some("HEAD".into()),
            commit_sha: None,
            limit: 10,
        };
        let json = serde_json::to_string_pretty(&params).unwrap();
        let parsed: FindCodeImpactParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.decision_id, "DEC-500");
        assert_eq!(parsed.hints.len(), 3);
        assert_eq!(parsed.limit, 10);
    }

    #[test]
    fn find_code_impact_params_default_limit() {
        let json = r#"{"decision_id":"DEC-1"}"#;
        let parsed: FindCodeImpactParams = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.limit, 20);
        assert!(parsed.hints.is_empty());
    }

    #[test]
    fn find_code_impact_result_verified_roundtrip() {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::Found,
            decision_id: "DEC-500".into(),
            phase: BindingDiscoveryPhase::CandidatesReady,
            graph_readiness: LocatorGraphReadiness::Ready,
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-impact-001".into()),
                repo_id: Some("repo-1".into()),
                commit_sha: Some("abc123".into()),
                index_version: Some("1".into()),
                ref_name: Some("HEAD".into()),
            }),
            search_plan: Some(SearchPlanSummary {
                strategy: "source_context".into(),
                queries_executed: 3,
                hint_seeded: false,
            }),
            candidates: vec![CodeImpactCandidate {
                target: "src/auth/device_flow.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/auth/device_flow.rs".into(),
                range: Some("10:1-45:1".into()),
                symbol: Some("DeviceFlowAuth".into()),
                evidence_state: LocatorEvidenceState::Verified,
                confidence: LocatorConfidence::High,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::ExactSymbol,
                    detail: "Symbol matches Decision source context".into(),
                }],
                graph_snapshot_id: Some("gs-impact-001".into()),
                validated_sha: Some("abc123".into()),
                content_hash: None,
            }],
            caveats: vec![],
            next_actions: vec![FindCodeImpactNextAction {
                action: "binding_create".into(),
                description: "Create binding evidence for verified candidate".into(),
                command_hint: Some("binding.create".into()),
            }],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: FindCodeImpactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, FindCodeImpactStatus::Found);
        assert_eq!(parsed.phase, BindingDiscoveryPhase::CandidatesReady);
        assert_eq!(parsed.candidates.len(), 1);
        assert!(parsed.candidates[0].allows_binding());
    }

    #[test]
    fn find_code_impact_result_approximate_roundtrip() {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::NeedsHumanSelection,
            decision_id: "DEC-501".into(),
            phase: BindingDiscoveryPhase::CandidatesReady,
            graph_readiness: LocatorGraphReadiness::Ready,
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-approx-001".into()),
                repo_id: None,
                commit_sha: Some("def456".into()),
                index_version: Some("1".into()),
                ref_name: None,
            }),
            search_plan: Some(SearchPlanSummary {
                strategy: "hint_seeded".into(),
                queries_executed: 2,
                hint_seeded: true,
            }),
            candidates: vec![CodeImpactCandidate {
                target: "src/middleware/rate_limit.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/middleware/rate_limit.rs".into(),
                range: None,
                symbol: Some("RateLimiter".into()),
                evidence_state: LocatorEvidenceState::ApproximateCandidate,
                confidence: LocatorConfidence::Low,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Lexical match; scope insufficient for exact proof".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            }],
            caveats: vec![CodeImpactCaveat {
                kind: CodeImpactCaveatKind::ApproximateCandidates,
                detail: "Some candidates are approximate; human selection required".into(),
            }],
            next_actions: vec![FindCodeImpactNextAction {
                action: "narrow_search".into(),
                description: "Provide additional hints to improve search accuracy".into(),
                command_hint: None,
            }],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: FindCodeImpactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, FindCodeImpactStatus::NeedsHumanSelection);
        assert!(!parsed.candidates[0].allows_binding());
    }

    #[test]
    fn find_code_impact_result_graph_unavailable() {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::GraphUnavailable,
            decision_id: "DEC-502".into(),
            phase: BindingDiscoveryPhase::DiscoveryRequested,
            graph_readiness: LocatorGraphReadiness::NotConfigured,
            snapshot: None,
            search_plan: None,
            candidates: vec![],
            caveats: vec![],
            next_actions: vec![FindCodeImpactNextAction {
                action: "configure_workspace".into(),
                description: "Configure workspace for graph indexing".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            }],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: FindCodeImpactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, FindCodeImpactStatus::GraphUnavailable);
        assert_eq!(parsed.phase, BindingDiscoveryPhase::DiscoveryRequested);
        assert!(parsed.candidates.is_empty());
    }

    #[test]
    fn find_code_impact_result_graph_stale() {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::GraphStale,
            decision_id: "DEC-503".into(),
            phase: BindingDiscoveryPhase::CandidatesReady,
            graph_readiness: LocatorGraphReadiness::Stale,
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-stale-001".into()),
                repo_id: None,
                commit_sha: Some("old-sha".into()),
                index_version: Some("1".into()),
                ref_name: None,
            }),
            search_plan: Some(SearchPlanSummary {
                strategy: "source_context".into(),
                queries_executed: 1,
                hint_seeded: false,
            }),
            candidates: vec![CodeImpactCandidate {
                target: "src/db/pool.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/db/pool.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::UnknownStale,
                confidence: LocatorConfidence::Unknown,
                reasons: vec![LocatorReason {
                    code: LocatorReasonCode::LexicalMatch,
                    detail: "Match against stale index".into(),
                }],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            }],
            caveats: vec![CodeImpactCaveat {
                kind: CodeImpactCaveatKind::GraphStale,
                detail: "Graph snapshot is behind HEAD; results may not reflect current code"
                    .into(),
            }],
            next_actions: vec![FindCodeImpactNextAction {
                action: "refresh_graph".into(),
                description: "Refresh graph snapshot to current HEAD".into(),
                command_hint: Some("graph.refresh_snapshot".into()),
            }],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: FindCodeImpactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, FindCodeImpactStatus::GraphStale);
        assert!(!parsed.candidates[0].allows_binding());
    }

    #[test]
    fn find_code_impact_result_not_found() {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::NotFound,
            decision_id: "DEC-504".into(),
            phase: BindingDiscoveryPhase::SearchPlanned,
            graph_readiness: LocatorGraphReadiness::Ready,
            snapshot: Some(LocatorSnapshotInfo {
                graph_snapshot_id: Some("gs-nf-001".into()),
                repo_id: None,
                commit_sha: Some("abc123".into()),
                index_version: Some("1".into()),
                ref_name: None,
            }),
            search_plan: Some(SearchPlanSummary {
                strategy: "source_context".into(),
                queries_executed: 5,
                hint_seeded: false,
            }),
            candidates: vec![],
            caveats: vec![],
            next_actions: vec![FindCodeImpactNextAction {
                action: "provide_hints".into(),
                description: "Provide file/symbol hints to seed a new search".into(),
                command_hint: None,
            }],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: FindCodeImpactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, FindCodeImpactStatus::NotFound);
        assert!(parsed.candidates.is_empty());
    }

    #[test]
    fn find_code_impact_result_unsupported() {
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::Unsupported,
            decision_id: "DEC-505".into(),
            phase: BindingDiscoveryPhase::DiscoveryRequested,
            graph_readiness: LocatorGraphReadiness::Ready,
            snapshot: None,
            search_plan: None,
            candidates: vec![],
            caveats: vec![],
            next_actions: vec![],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: FindCodeImpactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, FindCodeImpactStatus::Unsupported);
        assert!(parsed.candidates.is_empty());
    }

    #[test]
    fn find_code_impact_status_serialization() {
        let cases = vec![
            (FindCodeImpactStatus::Found, "\"found\""),
            (
                FindCodeImpactStatus::NeedsHumanSelection,
                "\"needs_human_selection\"",
            ),
            (FindCodeImpactStatus::NotFound, "\"not_found\""),
            (
                FindCodeImpactStatus::GraphUnavailable,
                "\"graph_unavailable\"",
            ),
            (FindCodeImpactStatus::GraphStale, "\"graph_stale\""),
            (FindCodeImpactStatus::Unsupported, "\"unsupported\""),
        ];
        for (status, expected) in &cases {
            let json = serde_json::to_string(status).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", status);
            let parsed: FindCodeImpactStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, status, "roundtrip mismatch for {:?}", status);
        }
    }

    #[test]
    fn find_code_impact_phase_serialization() {
        let cases = vec![
            (
                BindingDiscoveryPhase::DiscoveryRequested,
                "\"discovery_requested\"",
            ),
            (BindingDiscoveryPhase::SearchPlanned, "\"search_planned\""),
            (
                BindingDiscoveryPhase::CandidatesReady,
                "\"candidates_ready\"",
            ),
        ];
        for (phase, expected) in &cases {
            let json = serde_json::to_string(phase).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", phase);
            let parsed: BindingDiscoveryPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, phase, "roundtrip mismatch for {:?}", phase);
        }
    }

    #[test]
    fn find_code_impact_caveat_kind_serialization() {
        let cases = vec![
            (CodeImpactCaveatKind::GraphStale, "\"graph_stale\""),
            (CodeImpactCaveatKind::PartialIndex, "\"partial_index\""),
            (
                CodeImpactCaveatKind::LimitedSourceContext,
                "\"limited_source_context\"",
            ),
            (
                CodeImpactCaveatKind::ApproximateCandidates,
                "\"approximate_candidates\"",
            ),
            (CodeImpactCaveatKind::Truncated, "\"truncated\""),
        ];
        for (kind, expected) in &cases {
            let json = serde_json::to_string(kind).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", kind);
            let parsed: CodeImpactCaveatKind = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, kind, "roundtrip mismatch for {:?}", kind);
        }
    }

    #[test]
    fn find_code_impact_hint_kind_serialization() {
        let cases = vec![
            (FindCodeImpactHintKind::File, "\"file\""),
            (FindCodeImpactHintKind::Symbol, "\"symbol\""),
            (FindCodeImpactHintKind::Keyword, "\"keyword\""),
        ];
        for (kind, expected) in &cases {
            let json = serde_json::to_string(kind).unwrap();
            assert_eq!(&json, *expected, "serialize mismatch for {:?}", kind);
            let parsed: FindCodeImpactHintKind = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, kind, "roundtrip mismatch for {:?}", kind);
        }
    }

    #[test]
    fn find_code_impact_result_cannot_encode_compliance() {
        // Structural test: FindCodeImpactResult and CodeImpactCandidate have
        // no fields for compliance_state, signoff_state, safety, blocking,
        // no_conflict, or accepted_binding. This test verifies by serializing
        // a full result and confirming forbidden terms are absent.
        let result = FindCodeImpactResult {
            status: FindCodeImpactStatus::Found,
            decision_id: "DEC-600".into(),
            phase: BindingDiscoveryPhase::CandidatesReady,
            graph_readiness: LocatorGraphReadiness::Ready,
            snapshot: None,
            search_plan: None,
            candidates: vec![CodeImpactCandidate {
                target: "src/main.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/main.rs".into(),
                range: None,
                symbol: None,
                evidence_state: LocatorEvidenceState::Verified,
                confidence: LocatorConfidence::High,
                reasons: vec![],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            }],
            caveats: vec![],
            next_actions: vec![],
            semantic_recall: None,
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        assert!(!json.contains("compliance"));
        assert!(!json.contains("signoff"));
        assert!(!json.contains("safe"));
        assert!(!json.contains("blocking"));
        assert!(!json.contains("no_conflict"));
        assert!(!json.contains("accepted_binding"));
    }

    #[test]
    fn find_code_impact_candidates_not_binding_evidence() {
        // Candidates in a find-code-impact result are discovery candidates,
        // not accepted BindingEvidence. Verify the type is CodeImpactCandidate
        // (not BindingEvidence) and non-verified ones cannot bind.
        let candidate_verified = CodeImpactCandidate {
            target: "src/auth.rs".into(),
            target_type: LocatorTargetType::FilePath,
            file: "src/auth.rs".into(),
            range: None,
            symbol: None,
            evidence_state: LocatorEvidenceState::Verified,
            confidence: LocatorConfidence::High,
            reasons: vec![],
            graph_snapshot_id: None,
            validated_sha: None,
            content_hash: None,
        };
        assert!(candidate_verified.allows_binding());

        let non_binding_states = [
            LocatorEvidenceState::NotFound,
            LocatorEvidenceState::UnknownNotIndexed,
            LocatorEvidenceState::UnknownStale,
            LocatorEvidenceState::Ambiguous,
            LocatorEvidenceState::Unsupported,
            LocatorEvidenceState::ApproximateCandidate,
        ];
        for state in &non_binding_states {
            let c = CodeImpactCandidate {
                target: "src/main.rs".into(),
                target_type: LocatorTargetType::FilePath,
                file: "src/main.rs".into(),
                range: None,
                symbol: None,
                evidence_state: *state,
                confidence: LocatorConfidence::Unknown,
                reasons: vec![],
                graph_snapshot_id: None,
                validated_sha: None,
                content_hash: None,
            };
            assert!(
                !c.allows_binding(),
                "{:?} candidate must not allow binding",
                state
            );
        }
    }

    #[test]
    fn privacy_erase_subject_serde_roundtrip() {
        let cmd = ToolCommand::PrivacyEraseSubject(PrivacyEraseSubjectParams {
            subject_id: "subj-001".into(),
            predicate: Some("speaker".into()),
            reason: Some("GDPR Art.17 request".into()),
        });
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("privacy.erase_subject"));
        assert!(json.contains("subj-001"));

        let parsed: ToolCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command_name(), "privacy.erase_subject");
        match &parsed {
            ToolCommand::PrivacyEraseSubject(p) => {
                assert_eq!(p.subject_id, "subj-001");
                assert_eq!(p.predicate.as_deref(), Some("speaker"));
                assert_eq!(p.reason.as_deref(), Some("GDPR Art.17 request"));
            }
            other => panic!("expected PrivacyEraseSubject, got {}", other.command_name()),
        }
    }

    #[test]
    fn privacy_erase_subject_minimal_fields() {
        let json = r#"{"name":"privacy.erase_subject","params":{"subject_id":"subj-002"}}"#;
        let parsed: ToolCommand = serde_json::from_str(json).unwrap();
        match &parsed {
            ToolCommand::PrivacyEraseSubject(p) => {
                assert_eq!(p.subject_id, "subj-002");
                assert!(p.predicate.is_none());
                assert!(p.reason.is_none());
            }
            other => panic!("expected PrivacyEraseSubject, got {}", other.command_name()),
        }
    }

    #[test]
    fn request_carries_explicit_product_routing_context() {
        let request = ToolRequest::new(
            ToolCommand::HistoryList(HistoryListParams {
                decision_id: None,
                include_events: true,
                include_bindings: false,
                since: None,
            }),
            AuthorityContext {
                actor_id: "agent-908".into(),
                auth_method: AuthMethod::McpSession,
                session_id: Some("session-908".into()),
                workspace: None,
                policy_scope: vec!["default".into()],
                audit_metadata: serde_json::Map::new(),
            },
        )
        .for_product("prd_ada");

        assert_eq!(request.product_id.as_deref(), Some("prd_ada"));
        assert_eq!(
            serde_json::to_value(request).unwrap()["product_id"],
            "prd_ada"
        );
    }
}
