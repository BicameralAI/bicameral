//! Dashboard API contract types — typed payloads for the two-page dashboard.
//!
//! These types match the ADR-0006 data contracts and the Svelte dashboard's
//! TypeScript definitions. The gateway serves these as JSON from
//! `/api/v1/dashboard/*` endpoints.

use crate::candidate::{ReviewLifecycleState, TrackingState};
use crate::evidence::{GraphEvidenceState, GraphReadinessView};
use crate::source::{Source, SourceEvidenceRef, SourceFreshness, SourceSnapshot};
use serde::{Deserialize, Serialize};

// ── Ingestion Gate ───────────────────────────────────────────────

/// A single row in the Ingestion Gate page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionGateItem {
    pub source: Source,
    pub snapshot: SourceSnapshot,
    pub source_title: String,
    pub source_freshness: SourceFreshness,
    pub evidence: Vec<SourceEvidenceRef>,
    pub candidates: Vec<CandidatePreview>,
    pub tracked: bool,
    /// What kind of context this row represents in the ingestion gate.
    ///
    /// Raw source/candidate context is advisory. Only promoted Decisions
    /// carry governance freshness obligations. Stale untracked context
    /// does not appear as a governance failure (ADR-0022).
    #[serde(default)]
    pub context_disposition: ContextDisposition,
    /// When `true`, this row's freshness warning is informational only —
    /// the source or candidate is stale but untracked, so staleness is
    /// not a governance failure.
    #[serde(default)]
    pub stale_is_advisory: bool,
}

/// Classification of an Ingestion Gate row for UI rendering.
///
/// Per ADR-0022 and the codegraph-ledger-ui-protocol-boundary spec:
/// - `RawSource`: raw source context that has not been extracted into candidates.
/// - `CandidateContext`: extracted candidate claims awaiting review.
/// - `TrackedDecision`: promoted and tracked Decision whose evidence is
///   within governance freshness scope.
///
/// The UI uses this to distinguish advisory context from governed context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextDisposition {
    #[default]
    RawSource,
    CandidateContext,
    TrackedDecision,
}

/// Lightweight candidate preview shown in the Ingestion Gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_hint: Option<String>,
    pub evidence_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_state: Option<ReviewLifecycleState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_state: Option<crate::candidate::CandidateState>,
}

// ── Ledger View ──────────────────────────────────────────────────

/// An item in the Ledger View — either a promoted Decision or a queued candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LedgerReviewItem {
    Decision(Box<LedgerDecision>),
    Candidate(LedgerCandidate),
}

/// A promoted Decision in the Ledger View.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerDecision {
    pub id: String,
    pub summary: String,
    pub feature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub signoff: SignoffState,
    pub review_state: ReviewLifecycleState,
    pub sources: Vec<SourceEvidenceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<CodeRegion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicts_with: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovered: Option<bool>,
    pub allowed_commands: Vec<DecisionCommandKind>,
    /// Graph readiness for this Decision's workspace scope.
    ///
    /// This is view state — it surfaces graph availability honestly
    /// (stale, not_configured, indexing, failed) without being part of the
    /// Decision lifecycle. Per ADR-0010/0011/0022: non-verified graph
    /// states never directly mutate signoff or review state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_readiness: Option<GraphReadinessView>,
    /// Tracking lifecycle state per ADR-0022.
    ///
    /// Tracked Decisions are within governance freshness scope; untracked
    /// Decisions may go stale without governance failure.
    pub tracking_state: TrackingState,
    /// Source freshness for this Decision's cited evidence.
    ///
    /// This surfaces whether the source evidence behind the Decision is
    /// current, stale, or unknown. Stale tracked Decisions surface
    /// warnings and refresh actions; stale untracked Decisions do not.
    pub source_freshness: SourceFreshness,
    /// Code evidence section for the selected Decision view.
    ///
    /// Contains accepted bindings, advisory proposals, graph snapshot
    /// provenance, and graph evidence state. Per the codegraph-ledger-ui
    /// spec, code evidence appears under the selected Decision, not as
    /// a top-level ledger entity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_evidence: Option<CodeEvidenceSection>,
    /// Freshness warning for stale tracked Decisions.
    ///
    /// Only set when `tracking_state == Tracked` and source evidence is
    /// stale. Untracked Decisions with stale evidence do not receive
    /// this warning because staleness is not a governance failure for
    /// untracked context (ADR-0022).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness_warning: Option<FreshnessWarning>,
    /// Signoff display — ownership-only framing with posture-correct copy.
    ///
    /// Derived from `signoff` state. Makes it explicit that signoff is
    /// about ownership approval of Decision meaning, not code correctness,
    /// compliance, or merge safety (ADR-0022, issue #392).
    pub signoff_display: SignoffDisplay,
    /// Compliance display — separated from signoff, with category and copy
    /// that distinguish unknown/deferred compliance from successful resolution.
    ///
    /// Derived from the Decision's compliance state. Deferred/unsupported
    /// compliance is visibly NOT a successful compliance resolution.
    /// This field is read-only view state — no mutation (issue #392).
    pub compliance_display: ComplianceDisplay,
}

/// Code evidence section under a selected Decision in the Ledger View.
///
/// Per the codegraph-ledger-ui-protocol-boundary spec:
/// - Accepted `BindingEvidence` from the event store.
/// - Advisory `BindingProposal` or code hints, labeled as not accepted.
/// - Graph snapshot provenance.
/// - Graph evidence state and scope sufficiency.
/// - Reviewer actions allowed by policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEvidenceSection {
    /// Graph evidence state for the binding scope.
    pub evidence_state: GraphEvidenceState,
    /// User-facing label for the evidence state.
    pub evidence_label: String,
    /// Explanatory copy for the evidence state — posture-correct per spec.
    pub evidence_copy: String,
    /// Graph snapshot identity when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_snapshot_id: Option<String>,
    /// Commit SHA the evidence was validated against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_sha: Option<String>,
    /// Scope sufficiency status (e.g. "sufficient", "insufficient").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_sufficiency: Option<String>,
    /// Actions available for this code evidence state.
    pub allowed_actions: Vec<CodeEvidenceAction>,
}

/// Actions available in the code evidence section per the spec's
/// Failure State Div.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeEvidenceAction {
    ViewProvenance,
    AcceptBinding,
    RefineTarget,
    Revalidate,
    MarkReviewNeeded,
    KeepSourceOnly,
    BuildIndex,
    ExpandIndex,
    RebuildGraph,
    ChooseTarget,
    RefineHint,
    RequestHostedCheck,
    ValidateExactly,
    DismissSuggestion,
    KeepAsAdvisory,
}

/// Freshness warning for a tracked Decision whose source evidence is stale.
///
/// Only appears on tracked Decisions. Per ADR-0022, stale untracked context
/// does not create a governance failure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreshnessWarning {
    /// User-facing warning message.
    pub message: String,
    /// Available refresh actions.
    pub actions: Vec<FreshnessAction>,
}

/// Actions available for stale tracked Decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessAction {
    RefreshSource,
    RevalidateBinding,
    MarkReviewed,
    Untrack,
}

/// A queued candidate in the Ledger View.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerCandidate {
    pub id: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_hint: Option<String>,
    pub sources: Vec<SourceEvidenceRef>,
    pub review_state: ReviewLifecycleState,
    pub allowed_commands: Vec<CandidateCommandKind>,
}

/// Signoff lifecycle on a Decision (ownership authority axis).
///
/// Signoff is exclusively ownership approval of Decision *meaning* in V1.
/// It does not imply, prove, or create enforcement, compliance state,
/// implementation correctness, code readiness, or blocking behavior.
/// Future enforcement workflows may consult signoff as one input, but
/// signoff alone is never sufficient proof of compliance or readiness
/// (see ADR-0022, ADR-0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignoffState {
    Proposed,
    Approved,
    Rejected,
    CollisionPending,
    Superseded,
}

// ── Display types for separated compliance/signoff presentation ───

/// Category for compliance display — distinguishes unknown, deferred,
/// and resolved compliance states for UI rendering.
///
/// Per issue #392 and ADR-0022: the UI must visually separate compliance
/// state from signoff state, and deferred/unsupported compliance must not
/// appear as a successful compliance resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceDisplayCategory {
    /// No compliance information is available (e.g. no bindings exist).
    Unknown,
    /// Compliance evaluation is recognized but deferred/unsupported in
    /// the current release. This is NOT a successful resolution.
    Deferred,
    /// Compliance has been evaluated and a verdict exists. Reserved for
    /// future use when compliance resolution is implemented.
    Resolved,
}

/// Presentation-layer compliance display for the dashboard.
///
/// This is a read-only view-state projection — it does NOT mutate
/// compliance state, signoff state, or any canonical Decision state.
/// It exists solely to provide posture-correct labels and copy that
/// distinguish unknown/deferred compliance from signoff ownership.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComplianceDisplay {
    /// The raw compliance state value being rendered.
    pub state: ComplianceState,
    /// Display category: unknown, deferred, or resolved.
    pub category: ComplianceDisplayCategory,
    /// Short user-facing label (e.g. "Not evaluated", "Deferred").
    pub label: String,
    /// Explanatory copy clarifying that this is NOT a compliance verdict,
    /// and does NOT imply signoff, code correctness, or merge safety.
    pub copy: String,
}

/// Presentation-layer signoff display for the dashboard.
///
/// This is a read-only view-state projection that clarifies signoff is
/// exclusively about ownership approval of Decision *meaning*. It does
/// NOT imply code correctness, compliance, implementation quality, or
/// merge safety (ADR-0022).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignoffDisplay {
    /// The raw signoff state value being rendered.
    pub state: SignoffState,
    /// Short user-facing label (e.g. "Ownership: pending review").
    pub label: String,
    /// Explanatory copy clarifying that signoff is about ownership
    /// approval only, not compliance or code correctness.
    pub copy: String,
}

impl ComplianceDisplay {
    /// Derive display from a raw `ComplianceState`.
    ///
    /// This is a pure function — no state mutation. The mapping:
    /// - `Ungrounded` → Unknown: no evidence exists to evaluate.
    /// - `Pending` → Deferred: compliance evaluation is not active.
    /// - `Reflected`, `Partial`, `Drifted` → Deferred in alpha
    ///   (compliance resolution is not implemented).
    pub fn from_state(state: ComplianceState) -> Self {
        match state {
            ComplianceState::Ungrounded => Self {
                state,
                category: ComplianceDisplayCategory::Unknown,
                label: "Not evaluated".into(),
                copy: "No code evidence exists for compliance evaluation. \
                       This does not affect signoff or ownership state."
                    .into(),
            },
            ComplianceState::Pending => Self {
                state,
                category: ComplianceDisplayCategory::Deferred,
                label: "Deferred".into(),
                copy: "Compliance evaluation is recognized but not active \
                       in this release. This is not a compliance verdict."
                    .into(),
            },
            ComplianceState::Reflected | ComplianceState::Partial | ComplianceState::Drifted => {
                Self {
                    state,
                    category: ComplianceDisplayCategory::Deferred,
                    label: "Deferred (unsupported)".into(),
                    copy: "Compliance resolution is not implemented in the \
                       current release. This state is informational only \
                       and does not represent a successful compliance \
                       resolution."
                        .into(),
                }
            }
        }
    }
}

impl SignoffDisplay {
    /// Derive display from a raw `SignoffState`.
    ///
    /// This is a pure function — no state mutation. All labels explicitly
    /// frame signoff as ownership approval, never as compliance or code
    /// correctness (ADR-0022).
    pub fn from_state(state: SignoffState) -> Self {
        match state {
            SignoffState::Proposed => Self {
                state,
                label: "Ownership: pending review".into(),
                copy: "This Decision is awaiting ownership review. \
                       Signoff is about approving the meaning of this \
                       Decision, not its implementation correctness or \
                       compliance."
                    .into(),
            },
            SignoffState::Approved => Self {
                state,
                label: "Ownership: approved".into(),
                copy: "Ownership of this Decision's meaning has been \
                       approved. This does not imply code correctness, \
                       compliance, or that the Decision is safe to merge."
                    .into(),
            },
            SignoffState::Rejected => Self {
                state,
                label: "Ownership: rejected".into(),
                copy: "Ownership of this Decision's meaning has been \
                       rejected. This is an ownership verdict only."
                    .into(),
            },
            SignoffState::CollisionPending => Self {
                state,
                label: "Ownership: collision pending".into(),
                copy: "A potential conflict with another Decision must be \
                       resolved before ownership review can proceed."
                    .into(),
            },
            SignoffState::Superseded => Self {
                state,
                label: "Ownership: superseded".into(),
                copy: "This Decision has been superseded by a newer \
                       Decision. Its ownership state is no longer active."
                    .into(),
            },
        }
    }
}

/// Compliance/grounding state (technical reality axis).
///
/// Compliance is independent from signoff. A Decision may be `Approved`
/// on the signoff axis while simultaneously `Drifted` on the compliance
/// axis. Signoff alone must never be used as proof of compliance,
/// readiness, or implementation correctness (see ADR-0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceState {
    Reflected,
    Partial,
    Drifted,
    Pending,
    Ungrounded,
}

/// Code region binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRegion {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// Commands available for Decisions in the Ledger View.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionCommandKind {
    ApproveSignoff,
    RejectSignoff,
    RequestContext,
    AssignReviewer,
    BindToCode,
    SupersedeDecision,
    MarkDifferentScopes,
    RefreshSource,
    UntrackDecision,
}

/// Commands available for Candidates in the Ledger View.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateCommandKind {
    AcceptCandidate,
    RejectCandidate,
    EditCandidate,
    RequestContext,
    AssignReviewer,
}

// ── Decision detail (read-model projection) ─────────────────────

/// Full detail view for a single accepted Decision.
///
/// This is a **read-model projection type** — it separates the orthogonal
/// state axes of a Decision for rendering in detail surfaces (dashboard,
/// ledger detail, projection). It must never be used to mutate canonical
/// state.
///
/// Per issue #387 acceptance criteria: the same `decision_id` must be
/// retrievable with stable identity across `history.list`, `search.query`,
/// and `dashboard/projection` detail endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionDetail {
    pub decision_id: String,
    pub candidate_id: String,
    pub title: String,
    pub body: String,
    pub level: crate::candidate::DecisionLevel,
    pub promoted_at: chrono::DateTime<chrono::Utc>,
    pub promoted_by: String,
    pub tracking_state: crate::candidate::TrackingState,
    pub candidate_review: CandidateReviewSection,
    pub signoff: SignoffSection,
    pub code_evidence: CodeEvidenceDetailSection,
    pub compliance: ComplianceDeferredSection,
    /// Informational submission provenance preserved from the Decision — which
    /// surface/actor/source submitted the originating evidence (#389). Mirrors
    /// `HistoryDecision.provenance`/`SearchResult.provenance` so provenance is
    /// visible on the decision-detail surface too. Never creates review,
    /// signoff, or compliance authority; absent (`None`) when unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<crate::evidence::SubmissionProvenance>,
    /// Daemon-authored semantic recall contribution for this decision detail
    /// (issue #640). Advisory read-model: renders graph-verified semantic
    /// targets and approximate hints (kept distinct) plus caveats/next
    /// actions, visually separate from `code_evidence` so approximate hints
    /// never collapse into verified graph evidence. Never mutates signoff,
    /// compliance, no-conflict, or merge-safety, and never asserts
    /// safe-to-proceed. Absent (`None`) when no semantic provider is
    /// configured, so the no-op stays quiet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_recall: Option<crate::semantic_recall::SemanticRecallView>,
}

/// Candidate/review lifecycle state section.
///
/// Surfaces where the candidate came from and its review lifecycle state.
/// Separated from signoff, code-evidence, and compliance axes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateReviewSection {
    pub review_state: crate::candidate::ReviewLifecycleState,
    pub source_id: Option<String>,
    pub source_freshness: crate::source::SourceFreshness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness_warning: Option<FreshnessWarning>,
}

/// Signoff (ownership authority) section.
///
/// Signoff is exclusively ownership approval of Decision *meaning*.
/// It is independent from compliance and code evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignoffSection {
    pub state: SignoffState,
    pub allowed_commands: Vec<DecisionCommandKind>,
}

/// Code evidence detail section.
///
/// Surfaces accepted bindings and graph evidence state without implying
/// compliance verdicts or blocking behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeEvidenceDetailSection {
    pub evidence_state: crate::evidence::GraphEvidenceState,
    pub has_bindings: bool,
    pub bindings: Vec<CodeEvidenceBinding>,
}

/// A single code evidence binding within the detail section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeEvidenceBinding {
    pub id: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub binding_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

/// Compliance/deferred state section.
///
/// Rendered as a separate axis from signoff and code evidence.
/// Per ADR-0022: compliance state is never derived solely from signoff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComplianceDeferredSection {
    pub state: ComplianceState,
    /// Whether compliance resolution is deferred in the current release.
    pub deferred: bool,
}

// ── Review command payload (emitted by dashboard) ────────────────

/// A review command submitted by the dashboard UI.
///
/// The dashboard emits substrate-neutral commands — it does not write
/// event-store internals directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardReviewCommand {
    pub target_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ── Deferred command response ────────────────────────────────────

/// Typed response for dashboard commands that are recognized but deferred
/// (not yet implemented in the current release).
///
/// Returned with HTTP 202 to signal the command was understood but not
/// executed. Consumers can distinguish deferred from successful commands
/// by inspecting the `status` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeferredCommandResponse {
    /// Always `"deferred"` for commands recognized but not yet active.
    pub status: String,
    /// Machine-readable reason code, e.g. `"not_supported_in_alpha"`.
    pub reason: String,
    /// The command name that was deferred.
    pub command: String,
}

impl DeferredCommandResponse {
    pub fn alpha_unsupported(command: &str) -> Self {
        Self {
            status: "deferred".into(),
            reason: "not_supported_in_alpha".into(),
            command: command.into(),
        }
    }
}

// ── Connection registry ──────────────────────────────────────────

/// Response for `GET /connections` — non-secret metadata for all known providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRegistryResponse {
    pub connections: std::collections::HashMap<String, ConnectionRegistryEntry>,
}

/// Non-secret metadata for a single provider connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRegistryEntry {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_method: Option<String>,
}

// ── Connection start outcome ─────────────────────────────────────

/// Typed outcome for `POST /connections/:provider/start`.
///
/// Each variant maps to a distinct dashboard UX path. The response
/// never contains secret material — tokens stay in `secrets_store`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ConnectionStartOutcome {
    /// Provider is already connected; no further action needed.
    Connected {
        provider: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account_label: Option<String>,
    },
    /// OAuth or device-flow URL for the user to open.
    ///
    /// `auth_session_id` identifies this auth session for subsequent
    /// polling (`GET /status/:session_id`) or callback correlation.
    StartUrl {
        provider: String,
        url: String,
        /// Opaque session identifier for polling or callback correlation.
        auth_session_id: AuthSessionId,
        /// Device-flow user code (e.g. GitHub device flow). `None` for
        /// local-loopback OAuth.
        #[serde(skip_serializing_if = "Option::is_none")]
        user_code: Option<String>,
        /// Hint for the dashboard: `"device_flow"` or `"local_loopback"`.
        #[serde(skip_serializing_if = "Option::is_none")]
        flow_type: Option<String>,
    },
    /// Import instructions for token/PAT import path.
    ImportInstructions { provider: String, message: String },
    /// Provider requires user action before connection can proceed.
    ActionNeeded { provider: String, message: String },
    /// Provider is not supported or not configured.
    Unsupported { provider: String, message: String },
}

// ── Auth-session completion contract ─────────────────────────────

/// Opaque session identifier returned with `StartUrl` and used to poll
/// or correlate a callback for an in-progress provider auth flow.
///
/// The value is server-generated and opaque to the dashboard. It is
/// passed back to `GET /connections/:provider/status/:session_id` for
/// device-flow polling or correlated automatically on the
/// `POST /connections/:provider/callback` path.
pub type AuthSessionId = String;

/// Terminal outcome of a provider auth-session completion.
///
/// After `ConnectionStartOutcome::StartUrl` sends the user into a
/// provider auth flow, the dashboard resolves the session by polling
/// `/status/:session_id` (device flow) or receiving the callback
/// (local-loopback OAuth). Both paths return this enum as the terminal
/// result.
///
/// **Contract invariants:**
/// - No variant contains secret material (tokens, keys, codes).
/// - `Connected` means `ConnectionRegistry` was updated and secrets were
///   persisted to `secrets_store`. The response carries only non-secret
///   metadata.
/// - Successful auth does **not** create a `SourceBinding`, approve
///   ingestion, or materialize Decision Ledger state.
/// - Credential-import is excluded; only `ConnectionStartOutcome` offers
///   `ImportInstructions`.
///
/// **Provider compatibility:**
/// - **Device flow** (GitHub): dashboard polls
///   `GET /connections/:provider/status/:session_id` until a terminal
///   variant is returned or the session expires.
/// - **Local-loopback callback** (Google): the provider redirects to
///   `POST /connections/:provider/callback`; the server resolves the
///   session and returns this outcome synchronously.
/// - A provider need not implement both paths. The `start_url` response
///   indicates which mechanism applies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ConnectionCompletionOutcome {
    /// Auth completed successfully. `ConnectionRegistry` has been updated
    /// with non-secret metadata; tokens are in `secrets_store`.
    Connected {
        provider: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account_label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_method: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capabilities: Vec<String>,
    },
    /// The provider requires additional user action (e.g. scope approval,
    /// email verification) before the connection can be finalized.
    ActionNeeded { provider: String, message: String },
    /// The provider denied consent or the user rejected the auth prompt.
    Denied { provider: String, message: String },
    /// The auth session expired before the user completed the flow.
    Expired { provider: String, message: String },
    /// The provider is not supported or cannot be connected.
    Unsupported { provider: String, message: String },
    /// The provider is currently unreachable or returned an error.
    ProviderUnavailable { provider: String, message: String },
    /// The auth session is still in progress (device-flow polling only).
    /// The dashboard should continue polling.
    Pending {
        provider: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

/// Request body for `POST /connections/:provider/callback`.
///
/// Carries the authorization code or state parameter from the provider's
/// OAuth redirect. Secret material (the code) is consumed server-side
/// and never persisted in logs, responses, or registry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionCallbackRequest {
    /// The `auth_session_id` that was returned in `StartUrl`.
    pub auth_session_id: AuthSessionId,
    /// Authorization code from the OAuth redirect (consumed server-side).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// OAuth state parameter for CSRF validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Provider-specific error code from the redirect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signoff_state_serializes_to_snake_case() {
        let cases = vec![
            (SignoffState::Proposed, "\"proposed\""),
            (SignoffState::Approved, "\"approved\""),
            (SignoffState::Rejected, "\"rejected\""),
            (SignoffState::CollisionPending, "\"collision_pending\""),
            (SignoffState::Superseded, "\"superseded\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn signoff_state_deserializes_from_snake_case() {
        let cases = vec![
            ("\"proposed\"", SignoffState::Proposed),
            ("\"approved\"", SignoffState::Approved),
            ("\"rejected\"", SignoffState::Rejected),
            ("\"collision_pending\"", SignoffState::CollisionPending),
            ("\"superseded\"", SignoffState::Superseded),
        ];
        for (json, expected) in cases {
            assert_eq!(
                serde_json::from_str::<SignoffState>(json).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn signoff_state_rejects_stale_ratified_variant() {
        assert!(serde_json::from_str::<SignoffState>("\"ratified\"").is_err());
    }

    #[test]
    fn compliance_state_serializes_to_snake_case() {
        let cases = vec![
            (ComplianceState::Reflected, "\"reflected\""),
            (ComplianceState::Partial, "\"partial\""),
            (ComplianceState::Drifted, "\"drifted\""),
            (ComplianceState::Pending, "\"pending\""),
            (ComplianceState::Ungrounded, "\"ungrounded\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn compliance_state_deserializes_from_snake_case() {
        let cases = vec![
            ("\"reflected\"", ComplianceState::Reflected),
            ("\"partial\"", ComplianceState::Partial),
            ("\"drifted\"", ComplianceState::Drifted),
            ("\"pending\"", ComplianceState::Pending),
            ("\"ungrounded\"", ComplianceState::Ungrounded),
        ];
        for (json, expected) in cases {
            assert_eq!(
                serde_json::from_str::<ComplianceState>(json).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn context_disposition_serializes_to_snake_case() {
        let cases = vec![
            (ContextDisposition::RawSource, "\"raw_source\""),
            (
                ContextDisposition::CandidateContext,
                "\"candidate_context\"",
            ),
            (ContextDisposition::TrackedDecision, "\"tracked_decision\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn context_disposition_default_is_raw_source() {
        assert_eq!(ContextDisposition::default(), ContextDisposition::RawSource);
    }

    #[test]
    fn freshness_action_serializes_to_snake_case() {
        let cases = vec![
            (FreshnessAction::RefreshSource, "\"refresh_source\""),
            (FreshnessAction::RevalidateBinding, "\"revalidate_binding\""),
            (FreshnessAction::MarkReviewed, "\"mark_reviewed\""),
            (FreshnessAction::Untrack, "\"untrack\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn code_evidence_action_serializes_to_snake_case() {
        let cases = vec![
            (CodeEvidenceAction::ViewProvenance, "\"view_provenance\""),
            (CodeEvidenceAction::Revalidate, "\"revalidate\""),
            (CodeEvidenceAction::KeepSourceOnly, "\"keep_source_only\""),
            (CodeEvidenceAction::RebuildGraph, "\"rebuild_graph\""),
            (CodeEvidenceAction::ChooseTarget, "\"choose_target\""),
            (CodeEvidenceAction::ValidateExactly, "\"validate_exactly\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn decision_command_kind_includes_refresh_and_untrack() {
        let refresh = serde_json::to_string(&DecisionCommandKind::RefreshSource).unwrap();
        assert_eq!(refresh, "\"refresh_source\"");
        let untrack = serde_json::to_string(&DecisionCommandKind::UntrackDecision).unwrap();
        assert_eq!(untrack, "\"untrack_decision\"");
    }

    #[test]
    fn deferred_command_response_serializes() {
        let resp = DeferredCommandResponse::alpha_unsupported("resolve_compliance");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "deferred");
        assert_eq!(json["reason"], "not_supported_in_alpha");
        assert_eq!(json["command"], "resolve_compliance");
    }

    #[test]
    fn deferred_command_response_roundtrips() {
        let resp = DeferredCommandResponse::alpha_unsupported("refresh_source");
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DeferredCommandResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    // ── Compliance/Signoff display separation tests (#392) ───────────

    #[test]
    fn compliance_display_ungrounded_maps_to_unknown_category() {
        let display = ComplianceDisplay::from_state(ComplianceState::Ungrounded);
        assert_eq!(display.state, ComplianceState::Ungrounded);
        assert_eq!(display.category, ComplianceDisplayCategory::Unknown);
        assert_eq!(display.label, "Not evaluated");
        assert!(
            display.copy.contains("No code evidence"),
            "copy should explain no evidence: {}",
            display.copy
        );
        assert!(
            !display.copy.contains("successful"),
            "unknown must not imply success"
        );
    }

    #[test]
    fn compliance_display_pending_maps_to_deferred_category() {
        let display = ComplianceDisplay::from_state(ComplianceState::Pending);
        assert_eq!(display.state, ComplianceState::Pending);
        assert_eq!(display.category, ComplianceDisplayCategory::Deferred);
        assert_eq!(display.label, "Deferred");
        assert!(
            display.copy.contains("not active"),
            "copy should say not active: {}",
            display.copy
        );
        assert!(
            !display.copy.contains("compliant"),
            "deferred must not imply compliance"
        );
    }

    #[test]
    fn compliance_display_reflected_maps_to_deferred_in_alpha() {
        let display = ComplianceDisplay::from_state(ComplianceState::Reflected);
        assert_eq!(display.category, ComplianceDisplayCategory::Deferred);
        assert_eq!(display.label, "Deferred (unsupported)");
        assert!(
            display.copy.contains("not implemented"),
            "copy should say not implemented: {}",
            display.copy
        );
        assert!(
            display.copy.contains("not represent a successful"),
            "must explicitly say NOT a successful resolution: {}",
            display.copy
        );
    }

    #[test]
    fn compliance_display_partial_maps_to_deferred_in_alpha() {
        let display = ComplianceDisplay::from_state(ComplianceState::Partial);
        assert_eq!(display.category, ComplianceDisplayCategory::Deferred);
        assert_eq!(display.label, "Deferred (unsupported)");
    }

    #[test]
    fn compliance_display_drifted_maps_to_deferred_in_alpha() {
        let display = ComplianceDisplay::from_state(ComplianceState::Drifted);
        assert_eq!(display.category, ComplianceDisplayCategory::Deferred);
        assert_eq!(display.label, "Deferred (unsupported)");
    }

    #[test]
    fn compliance_display_category_serializes_to_snake_case() {
        let cases = vec![
            (ComplianceDisplayCategory::Unknown, "\"unknown\""),
            (ComplianceDisplayCategory::Deferred, "\"deferred\""),
            (ComplianceDisplayCategory::Resolved, "\"resolved\""),
        ];
        for (variant, expected) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn signoff_display_proposed_frames_as_ownership() {
        let display = SignoffDisplay::from_state(SignoffState::Proposed);
        assert_eq!(display.state, SignoffState::Proposed);
        assert!(
            display.label.contains("Ownership"),
            "label must say ownership: {}",
            display.label
        );
        assert!(
            display.copy.contains("not its implementation correctness"),
            "copy must disclaim code correctness: {}",
            display.copy
        );
        assert!(
            !display.copy.contains("compliant"),
            "signoff copy must never say compliant"
        );
    }

    #[test]
    fn signoff_display_approved_does_not_imply_compliance() {
        let display = SignoffDisplay::from_state(SignoffState::Approved);
        assert_eq!(display.state, SignoffState::Approved);
        assert!(display.label.contains("Ownership"));
        assert!(display.label.contains("approved"));
        assert!(
            display.copy.contains("does not imply code correctness"),
            "approved copy must disclaim correctness: {}",
            display.copy
        );
        assert!(
            display.copy.contains("compliance"),
            "approved copy must mention compliance is separate: {}",
            display.copy
        );
        assert!(
            !display.copy.contains("safe to merge")
                || display.copy.contains("not") && display.copy.contains("safe to merge"),
            "approved must not imply merge safety"
        );
    }

    #[test]
    fn signoff_display_rejected_is_ownership_only() {
        let display = SignoffDisplay::from_state(SignoffState::Rejected);
        assert!(display.label.contains("Ownership"));
        assert!(display.label.contains("rejected"));
        assert!(display.copy.contains("ownership verdict only"));
    }

    #[test]
    fn signoff_display_collision_pending_is_ownership() {
        let display = SignoffDisplay::from_state(SignoffState::CollisionPending);
        assert!(display.label.contains("Ownership"));
        assert!(display.label.contains("collision"));
    }

    #[test]
    fn signoff_display_superseded_is_ownership() {
        let display = SignoffDisplay::from_state(SignoffState::Superseded);
        assert!(display.label.contains("Ownership"));
        assert!(display.label.contains("superseded"));
    }

    #[test]
    fn compliance_display_roundtrips_via_serde() {
        let display = ComplianceDisplay::from_state(ComplianceState::Pending);
        let json = serde_json::to_string(&display).unwrap();
        let parsed: ComplianceDisplay = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, display);
    }

    #[test]
    fn signoff_display_roundtrips_via_serde() {
        let display = SignoffDisplay::from_state(SignoffState::Approved);
        let json = serde_json::to_string(&display).unwrap();
        let parsed: SignoffDisplay = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, display);
    }

    #[test]
    fn compliance_display_deferred_is_visibly_not_success() {
        // Acceptance criterion: "Deferred compliance state is visibly not
        // a successful compliance resolution."
        for state in [
            ComplianceState::Pending,
            ComplianceState::Reflected,
            ComplianceState::Partial,
            ComplianceState::Drifted,
        ] {
            let display = ComplianceDisplay::from_state(state);
            assert_ne!(
                display.category,
                ComplianceDisplayCategory::Resolved,
                "state {:?} must not map to Resolved in alpha",
                state
            );
            assert!(
                display.label.contains("Deferred"),
                "state {:?} label must contain 'Deferred': {}",
                state,
                display.label
            );
        }
    }

    #[test]
    fn signoff_display_never_implies_compliance_or_correctness() {
        // Acceptance criterion: "Signoff UI/copy does not imply code
        // correctness, compliance, or merge safety."
        for state in [
            SignoffState::Proposed,
            SignoffState::Approved,
            SignoffState::Rejected,
            SignoffState::CollisionPending,
            SignoffState::Superseded,
        ] {
            let display = SignoffDisplay::from_state(state);
            let copy_lower = display.copy.to_lowercase();
            assert!(
                !copy_lower.contains("compliant")
                    || copy_lower.contains("not") && copy_lower.contains("compliance"),
                "signoff {:?} copy must not imply compliance: {}",
                state,
                display.copy
            );
            assert!(
                !copy_lower.contains("code is correct"),
                "signoff {:?} copy must not imply code correctness",
                state
            );
        }
    }

    #[test]
    fn compliance_display_distinguishes_unknown_from_deferred() {
        // Acceptance criterion: "compliance/status display distinguishes
        // unknown, deferred/unsupported, and signed-off ownership."
        let unknown = ComplianceDisplay::from_state(ComplianceState::Ungrounded);
        let deferred = ComplianceDisplay::from_state(ComplianceState::Pending);
        assert_ne!(unknown.category, deferred.category);
        assert_ne!(unknown.label, deferred.label);
    }

    #[test]
    fn connection_start_outcome_connected_serializes() {
        let outcome = ConnectionStartOutcome::Connected {
            provider: "github".into(),
            account_label: Some("jinhongkuan".into()),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "connected");
        assert_eq!(json["provider"], "github");
        assert_eq!(json["account_label"], "jinhongkuan");
    }

    #[test]
    fn connection_start_outcome_unsupported_serializes() {
        let outcome = ConnectionStartOutcome::Unsupported {
            provider: "notion".into(),
            message: "Notion is not yet supported.".into(),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "unsupported");
        assert_eq!(json["provider"], "notion");
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("not yet supported"));
    }

    #[test]
    fn connection_start_outcome_no_secret_material() {
        let outcomes = vec![
            ConnectionStartOutcome::Connected {
                provider: "github".into(),
                account_label: Some("user".into()),
            },
            ConnectionStartOutcome::ImportInstructions {
                provider: "github".into(),
                message: "Import a PAT via Settings.".into(),
            },
            ConnectionStartOutcome::Unsupported {
                provider: "slack".into(),
                message: "Not supported.".into(),
            },
        ];
        for outcome in outcomes {
            let json = serde_json::to_string(&outcome).unwrap();
            for secret_key in &["oauth_token", "refresh_token", "api_key", "client_secret"] {
                assert!(
                    !json.contains(secret_key),
                    "ConnectionStartOutcome must not contain: {secret_key}"
                );
            }
        }
    }

    #[test]
    fn connection_start_outcome_roundtrips() {
        let cases = vec![
            ConnectionStartOutcome::Connected {
                provider: "github".into(),
                account_label: None,
            },
            ConnectionStartOutcome::StartUrl {
                provider: "google".into(),
                url: "https://accounts.google.com/o/oauth2/auth".into(),
                auth_session_id: "sess-test-001".into(),
                user_code: None,
                flow_type: Some("local_loopback".into()),
            },
            ConnectionStartOutcome::ActionNeeded {
                provider: "google".into(),
                message: "Refresh token expired.".into(),
            },
            ConnectionStartOutcome::Unsupported {
                provider: "linear".into(),
                message: "Linear is not yet supported.".into(),
            },
        ];
        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let parsed: ConnectionStartOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, case);
        }
    }

    // ── ConnectionCompletionOutcome tests ────────────────────────

    #[test]
    fn completion_connected_serializes() {
        let outcome = ConnectionCompletionOutcome::Connected {
            provider: "github".into(),
            account_label: Some("jinhongkuan".into()),
            auth_method: Some("device_flow".into()),
            capabilities: vec!["github.repo.read".into()],
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "connected");
        assert_eq!(json["provider"], "github");
        assert_eq!(json["account_label"], "jinhongkuan");
        assert_eq!(json["auth_method"], "device_flow");
    }

    #[test]
    fn completion_denied_serializes() {
        let outcome = ConnectionCompletionOutcome::Denied {
            provider: "github".into(),
            message: "User denied consent.".into(),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "denied");
        assert_eq!(json["provider"], "github");
    }

    #[test]
    fn completion_expired_serializes() {
        let outcome = ConnectionCompletionOutcome::Expired {
            provider: "github".into(),
            message: "Device code expired after 900 seconds.".into(),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "expired");
    }

    #[test]
    fn completion_pending_serializes() {
        let outcome = ConnectionCompletionOutcome::Pending {
            provider: "github".into(),
            message: Some("Waiting for user to enter device code.".into()),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "pending");
    }

    #[test]
    fn completion_provider_unavailable_serializes() {
        let outcome = ConnectionCompletionOutcome::ProviderUnavailable {
            provider: "google".into(),
            message: "Google OAuth endpoint returned 503.".into(),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "provider_unavailable");
    }

    #[test]
    fn completion_no_secret_material() {
        let outcomes = vec![
            ConnectionCompletionOutcome::Connected {
                provider: "github".into(),
                account_label: Some("user".into()),
                auth_method: Some("device_flow".into()),
                capabilities: vec!["github.repo.read".into()],
            },
            ConnectionCompletionOutcome::ActionNeeded {
                provider: "google".into(),
                message: "Scope missing.".into(),
            },
            ConnectionCompletionOutcome::Denied {
                provider: "github".into(),
                message: "Denied.".into(),
            },
            ConnectionCompletionOutcome::Expired {
                provider: "github".into(),
                message: "Expired.".into(),
            },
            ConnectionCompletionOutcome::Unsupported {
                provider: "slack".into(),
                message: "Not supported.".into(),
            },
            ConnectionCompletionOutcome::ProviderUnavailable {
                provider: "google".into(),
                message: "Unreachable.".into(),
            },
            ConnectionCompletionOutcome::Pending {
                provider: "github".into(),
                message: None,
            },
        ];
        for outcome in outcomes {
            let json = serde_json::to_string(&outcome).unwrap();
            for secret_key in &["oauth_token", "refresh_token", "api_key", "client_secret"] {
                assert!(
                    !json.contains(secret_key),
                    "ConnectionCompletionOutcome must not contain: {secret_key}"
                );
            }
        }
    }

    #[test]
    fn completion_roundtrips() {
        let cases = vec![
            ConnectionCompletionOutcome::Connected {
                provider: "github".into(),
                account_label: Some("user".into()),
                auth_method: Some("device_flow".into()),
                capabilities: vec!["github.repo.read".into()],
            },
            ConnectionCompletionOutcome::ActionNeeded {
                provider: "google".into(),
                message: "Re-consent needed.".into(),
            },
            ConnectionCompletionOutcome::Denied {
                provider: "github".into(),
                message: "Consent denied.".into(),
            },
            ConnectionCompletionOutcome::Expired {
                provider: "github".into(),
                message: "Session expired.".into(),
            },
            ConnectionCompletionOutcome::Unsupported {
                provider: "linear".into(),
                message: "Not supported.".into(),
            },
            ConnectionCompletionOutcome::ProviderUnavailable {
                provider: "google".into(),
                message: "503 error.".into(),
            },
            ConnectionCompletionOutcome::Pending {
                provider: "github".into(),
                message: None,
            },
        ];
        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let parsed: ConnectionCompletionOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, case);
        }
    }

    #[test]
    fn start_url_includes_auth_session_id() {
        let outcome = ConnectionStartOutcome::StartUrl {
            provider: "github".into(),
            url: "https://github.com/login/device".into(),
            auth_session_id: "sess-abc-123".into(),
            user_code: Some("ABCD-1234".into()),
            flow_type: Some("device_flow".into()),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "start_url");
        assert_eq!(json["auth_session_id"], "sess-abc-123");
        assert_eq!(json["user_code"], "ABCD-1234");
        assert_eq!(json["flow_type"], "device_flow");
    }

    #[test]
    fn callback_request_deserializes() {
        let json = r#"{
            "auth_session_id": "sess-001",
            "code": "auth-code-xyz",
            "state": "csrf-token"
        }"#;
        let req: ConnectionCallbackRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.auth_session_id, "sess-001");
        assert_eq!(req.code.as_deref(), Some("auth-code-xyz"));
        assert_eq!(req.state.as_deref(), Some("csrf-token"));
        assert!(req.error.is_none());
    }

    #[test]
    fn callback_request_error_path_deserializes() {
        let json = r#"{
            "auth_session_id": "sess-002",
            "error": "access_denied"
        }"#;
        let req: ConnectionCallbackRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.auth_session_id, "sess-002");
        assert!(req.code.is_none());
        assert_eq!(req.error.as_deref(), Some("access_denied"));
    }
}
