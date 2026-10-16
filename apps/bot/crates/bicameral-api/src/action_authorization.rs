//! Action authorization policy — runtime evaluator for governed action checks.
//!
//! Determines whether an actor + surface + command combination is permitted
//! before the command reaches the handler. This is the single policy gate
//! for all governed action paths:
//!
//! - **Candidate review** (accept/reject): permitted for local actors without
//!   implying signoff authority.
//! - **Decision signoff** (approve/reject signoff, supersede, reopen,
//!   correction): requires stronger owner authority — only human actors on
//!   local surfaces.
//! - **External ingest**: cannot carry review commands, signoff, supersession,
//!   compliance approval, egress execution, or policy grants.
//!
//! Profiles, adapters, provider credentials, and MCP server identity do not
//! grant lifecycle authority by themselves.
//!
//! This module does **not** implement remote callbacks, governed egress
//! execution, or product-owner authority model changes.

use crate::tool_request::{AuthMethod, AuthorityContext, ToolCommand};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Governed action classification
// ---------------------------------------------------------------------------

/// Classification of a command into its governed action class.
///
/// Each class carries different authority requirements. The evaluator maps
/// `ToolCommand` variants into these classes before checking policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedActionClass {
    /// Read-only queries (history, search, preflight, graph status).
    /// Permitted for any authenticated actor.
    ReadOnly,

    /// Candidate review actions (accept/reject candidate).
    /// Permitted for local authenticated actors without implying signoff.
    CandidateReview,

    /// Decision signoff and lifecycle actions (approve/reject signoff).
    /// Requires owner-level authority: human actor on a local surface.
    DecisionSignoff,

    /// Compliance resolution. Deferred in alpha.
    ComplianceResolution,

    /// Binding creation and inspection. Permitted for local actors.
    BindingMutation,

    /// Daemon-owned local workspace binding (`workspace.bind`) and reverse
    /// lookup (`workspace.resolve`). Surfaces may propose/read from any local
    /// surface, but the hosted bridge must never carry `candidate_path`:
    /// materialization is daemon-only authority and resolve is a local-only
    /// routing read. Permitted for local actors; hosted bridge is rejected.
    WorkspaceBinding,

    /// Evidence refresh. Permitted for local actors.
    EvidenceRefresh,

    /// Tracking mutations (untrack, refresh query). Deferred in alpha.
    TrackingMutation,

    /// Local ingest (submit_local, submit_managed). Permitted for local actors.
    LocalIngest,

    /// Graph operations (status, refresh). Permitted for local actors.
    GraphOperation,

    /// Contradiction resolution — transitioning a contradiction finding
    /// (acknowledge, resolve, dismiss). Requires owner-level authority.
    ContradictionResolution,

    /// External ingest via `ExternalIngestEnvelope`. Strictly evidence-only;
    /// cannot carry authority-bearing payloads.
    ExternalIngest,

    /// PII erasure (GDPR Art.17). Deferred until PiiArchive is implemented.
    PrivacyErasure,

    /// Daemon-canonical hosted Product creation (`product.create`,
    /// `product.publish`). The surface may reach the handler, but real authority
    /// is enforced by the daemon-confirmed hosted actor context (paired state,
    /// bound Cloud account, authorization) inside the command handler, which
    /// returns typed denials without writing any event. Permitted for the hosted
    /// bridge and local actors; the handler binds the actual authority.
    ProductCreation,

    /// Same-domain Product Member auto-join (`product.join`). Originates from
    /// the paired hosted bridge; the daemon verifies a signed ticket and binds
    /// locally confirmed actor context. Grants Product Member only — never
    /// Policy Owner or Product Owner — so it is not a lifecycle/owner authority
    /// action.
    ProductMembership,

    /// Local setup continuation after a canonical join (`product.continue_setup`,
    /// issue #769). Originates from the paired hosted bridge; the daemon resolves
    /// provider access, validates the event-store descriptor, and writes at most
    /// one durable local `ProjectRegistration`. It never grants, changes, or
    /// revokes canonical membership, so it is not an owner-authority action.
    ProjectRegistration,
}

impl GovernedActionClass {
    /// Classify a `ToolCommand` into its governed action class.
    pub fn from_command(command: &ToolCommand) -> Self {
        match command {
            ToolCommand::HistoryList(_)
            | ToolCommand::SearchQuery(_)
            | ToolCommand::PreflightRun(_)
            // RecallPacket lookup/inspection actions are read-only projections:
            // they drill into evidence and expand scope without mutating canonical state.
            | ToolCommand::RecallInspectEvidence(_)
            | ToolCommand::RecallExpandScope(_)
            // Session lease extension only — no candidate enumeration, no Product write.
            | ToolCommand::CandidateSessionHeartbeat(_) => Self::ReadOnly,

            // Governed candidate promotion via a RecallPacket action is a
            // candidate-review action: MCP may route exactly one ToolRequest, but
            // candidate review does not imply signoff authority (#522/#525). The
            // promotion handler still routes to signoff when the actor lacks
            // authority proof.
            //
            // Managed MCP CandidateSet selection (prepare/commit) and session end
            // are the same class: local authenticated actors may request them, but
            // daemon-owned authority/currentness still gate the actual effect.
            ToolCommand::ReviewAcceptCandidate(_)
            | ToolCommand::ReviewRejectCandidate(_)
            | ToolCommand::CandidateHide(_)
            | ToolCommand::CandidateSuppress(_)
            | ToolCommand::RecallPromoteDecisionCandidate(_)
            | ToolCommand::RecallRequestCorrection(_)
            | ToolCommand::CandidateSelectionPrepare(_)
            | ToolCommand::CandidateSelectionCommit(_)
            | ToolCommand::CandidateSessionEnd(_) => Self::CandidateReview,

            ToolCommand::ReviewApproveSignoff(_) | ToolCommand::ReviewRejectSignoff(_) => {
                Self::DecisionSignoff
            }

            ToolCommand::ReviewResolveCompliance(_) => Self::ComplianceResolution,

            ToolCommand::BindingCreate(_) | ToolCommand::BindingInspect(_) => Self::BindingMutation,

            ToolCommand::WorkspaceBind(_) | ToolCommand::WorkspaceResolve(_) => {
                Self::WorkspaceBinding
            }

            ToolCommand::EvidenceRefresh(_) => Self::EvidenceRefresh,

            ToolCommand::TrackingUntrackSource(_) | ToolCommand::TrackingRefreshQuery(_) => {
                Self::TrackingMutation
            }

            ToolCommand::IngestSubmitLocal(_) | ToolCommand::IngestSubmitManaged(_) => {
                Self::LocalIngest
            }

            ToolCommand::CodeLocate(_)
            | ToolCommand::GraphStatus(_)
            | ToolCommand::GraphRefreshSnapshot(_)
            | ToolCommand::DecisionFindCodeImpact(_) => Self::GraphOperation,

            ToolCommand::PrivacyEraseSubject(_) => Self::PrivacyErasure,
            ToolCommand::ReviewTransitionFinding(_) => Self::ContradictionResolution,

            ToolCommand::ProductCreate(_) | ToolCommand::ProductPublish(_) => Self::ProductCreation,

            ToolCommand::ProductJoin(_) => Self::ProductMembership,

            ToolCommand::ProductContinueSetup(_) => Self::ProjectRegistration,
        }
    }
}

// ---------------------------------------------------------------------------
// Authorization outcomes
// ---------------------------------------------------------------------------

/// Why an action was denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionDenialReason {
    /// The actor is not authorized for this action class.
    Unauthorized,
    /// The action is recognized but not supported in this release.
    Unsupported,
    /// The target entity is not in a state eligible for this action.
    TargetIneligible,
    /// The action is recognized and the actor may be eligible, but
    /// evaluation is deferred (e.g. pending remote callback support).
    Deferred,
}

/// A denied action result with structured reason and message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDenial {
    pub reason: ActionDenialReason,
    pub action_class: GovernedActionClass,
    pub message: String,
}

impl fmt::Display for ActionDenial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.reason, self.message)
    }
}

impl ActionDenial {
    pub fn unauthorized(action_class: GovernedActionClass, message: impl Into<String>) -> Self {
        Self {
            reason: ActionDenialReason::Unauthorized,
            action_class,
            message: message.into(),
        }
    }

    pub fn unsupported(action_class: GovernedActionClass, message: impl Into<String>) -> Self {
        Self {
            reason: ActionDenialReason::Unsupported,
            action_class,
            message: message.into(),
        }
    }

    pub fn target_ineligible(
        action_class: GovernedActionClass,
        message: impl Into<String>,
    ) -> Self {
        Self {
            reason: ActionDenialReason::TargetIneligible,
            action_class,
            message: message.into(),
        }
    }

    pub fn deferred(action_class: GovernedActionClass, message: impl Into<String>) -> Self {
        Self {
            reason: ActionDenialReason::Deferred,
            action_class,
            message: message.into(),
        }
    }

    /// Wire-format code string for use in `ToolResponse` rejection messages.
    pub fn rejection_code(&self) -> &'static str {
        match self.reason {
            ActionDenialReason::Unauthorized => "unauthorized",
            ActionDenialReason::Unsupported => "unsupported",
            ActionDenialReason::TargetIneligible => "target_ineligible",
            ActionDenialReason::Deferred => "deferred",
        }
    }
}

/// A permitted action result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPermit {
    pub action_class: GovernedActionClass,
}

/// Result of an authorization evaluation.
pub type AuthorizationResult = Result<ActionPermit, ActionDenial>;

// ---------------------------------------------------------------------------
// Surfaces that cannot carry lifecycle authority
// ---------------------------------------------------------------------------

/// Surfaces/identities that must never be granted lifecycle authority
/// (signoff, supersession, compliance resolution, policy grants).
///
/// Profiles, adapters, provider credentials, and MCP server identity
/// cannot grant lifecycle authority by themselves.
const NON_AUTHORITY_SURFACES: &[&str] = &[
    "adapter",
    "connector",
    "provider",
    "mcp_server",
    "profile",
    "credential",
];

/// Commands that external ingest must never carry.
///
/// These are authority-bearing payloads that the `ExternalIngestEnvelope`
/// type-level contract forbids. The runtime evaluator enforces this as a
/// defense-in-depth check beyond the type system.
const EXTERNAL_INGEST_FORBIDDEN_COMMANDS: &[&str] = &[
    "review.accept_candidate",
    "review.reject_candidate",
    "candidate.hide",
    "candidate.suppress",
    "review.approve_signoff",
    "review.reject_signoff",
    "review.resolve_compliance",
    "privacy.erase_subject",
];

// ---------------------------------------------------------------------------
// Policy evaluator
// ---------------------------------------------------------------------------

/// Central action authorization policy evaluator.
///
/// Evaluates whether an actor + authority context + command is permitted.
/// Returns `ActionPermit` on success or `ActionDenial` with a typed reason.
///
/// This evaluator does not implement remote callbacks or governed egress.
pub struct ActionAuthorizationPolicy;

impl ActionAuthorizationPolicy {
    /// Evaluate whether a `ToolCommand` is permitted for the given authority.
    pub fn evaluate(authority: &AuthorityContext, command: &ToolCommand) -> AuthorizationResult {
        let action_class = GovernedActionClass::from_command(command);
        Self::evaluate_action_class(authority, action_class)
    }

    /// Evaluate whether an action class is permitted for the given authority.
    pub fn evaluate_action_class(
        authority: &AuthorityContext,
        action_class: GovernedActionClass,
    ) -> AuthorizationResult {
        // Gate: actor_id must be present
        if authority.actor_id.is_empty() {
            return Err(ActionDenial::unauthorized(
                action_class,
                "missing_authority: actor_id is required",
            ));
        }

        // Gate: non-authority surfaces cannot hold lifecycle authority
        if is_non_authority_surface(&authority.actor_id) {
            match action_class {
                GovernedActionClass::CandidateReview
                | GovernedActionClass::DecisionSignoff
                | GovernedActionClass::ComplianceResolution
                | GovernedActionClass::PrivacyErasure
                | GovernedActionClass::ContradictionResolution => {
                    return Err(ActionDenial::unauthorized(
                        action_class,
                        "Profiles, adapters, provider credentials, and MCP server \
                         identity cannot grant lifecycle authority",
                    ));
                }
                _ => {}
            }
        }

        match action_class {
            // Read-only: any authenticated actor
            GovernedActionClass::ReadOnly => Ok(ActionPermit { action_class }),

            // Candidate review: any local authenticated actor.
            // Review does not imply signoff authority.
            GovernedActionClass::CandidateReview => {
                Self::require_local_actor(authority, action_class)
            }

            // Decision signoff: requires owner-level authority.
            // Only human actors on local daemon or dashboard surfaces.
            GovernedActionClass::DecisionSignoff => {
                Self::require_owner_authority(authority, action_class)
            }

            // Compliance resolution and tracking: deferred in alpha, but
            // authority-permitted — the dispatch layer handles the
            // unsupported_capability rejection with the right message.
            GovernedActionClass::ComplianceResolution | GovernedActionClass::TrackingMutation => {
                Self::require_local_actor(authority, action_class)
            }

            // Contradiction resolution: requires owner-level authority.
            GovernedActionClass::ContradictionResolution => {
                Self::require_owner_authority(authority, action_class)
            }

            // Binding, evidence, local ingest, graph: local actors
            GovernedActionClass::BindingMutation
            | GovernedActionClass::EvidenceRefresh
            | GovernedActionClass::LocalIngest
            | GovernedActionClass::GraphOperation => {
                Self::require_local_actor(authority, action_class)
            }

            // Privacy erasure: deferred until PiiArchive, but authority-permitted.
            // The dispatch layer handles the unsupported_capability rejection.
            GovernedActionClass::PrivacyErasure => {
                Self::require_local_actor(authority, action_class)
            }

            // Workspace binding: local surfaces (daemon, CLI, dashboard, MCP)
            // may propose, but the hosted bridge can never bind because the
            // local candidate_path must never cross the bridge. The daemon
            // remains the sole authority that validates and materializes the
            // binding; surface authorization here is not binding authority.
            GovernedActionClass::WorkspaceBinding => {
                Self::require_local_actor_without_bridge(authority, action_class)
            }

            // Product Member auto-join: originates from the paired hosted
            // bridge, so the hosted-bridge surface is explicitly permitted here.
            // This is not owner authority — the daemon still verifies the
            // signed ticket, binds locally confirmed actor context, and grants
            // Product Member only.
            GovernedActionClass::ProductMembership => {
                Self::require_local_actor(authority, action_class)
            }

            // Local setup continuation: reachable from the paired hosted bridge,
            // same as auto-join. Not owner authority — the daemon resolves
            // provider/descriptor facts and writes only local registration
            // state; membership is never touched here.
            GovernedActionClass::ProjectRegistration => {
                Self::require_local_actor(authority, action_class)
            }

            // External ingest: evaluated separately via `evaluate_external_ingest`
            GovernedActionClass::ExternalIngest => Ok(ActionPermit { action_class }),

            // Product creation: reachable from the hosted bridge and local
            // surfaces. The surface gate only requires an authenticated actor;
            // the daemon-confirmed hosted actor context enforces the real
            // authority (paired, bound Cloud account, authorization) in the
            // command handler and returns typed denials without side effects.
            GovernedActionClass::ProductCreation => Ok(ActionPermit { action_class }),
        }
    }

    /// Evaluate whether an external ingest command name is forbidden.
    ///
    /// External ingest cannot carry review commands, signoff, supersession,
    /// compliance approval, egress execution, or policy grants.
    pub fn evaluate_external_ingest_command(command_name: &str) -> Result<(), ActionDenial> {
        if EXTERNAL_INGEST_FORBIDDEN_COMMANDS.contains(&command_name) {
            return Err(ActionDenial::unauthorized(
                GovernedActionClass::ExternalIngest,
                format!("External ingest cannot carry authority-bearing command: {command_name}"),
            ));
        }
        Ok(())
    }

    /// Check that the actor is local (any authenticated local method).
    fn require_local_actor(
        authority: &AuthorityContext,
        action_class: GovernedActionClass,
    ) -> AuthorizationResult {
        match authority.auth_method {
            AuthMethod::LocalDaemon
            | AuthMethod::McpSession
            | AuthMethod::Cli
            | AuthMethod::Dashboard
            | AuthMethod::HostedBridge => Ok(ActionPermit { action_class }),
        }
    }

    /// Check that the actor is local and NOT the hosted bridge.
    ///
    /// Workspace bind and resolve are reachable from any local surface
    /// (daemon, CLI, dashboard, MCP session) but never from the hosted
    /// bridge: the local `candidate_path` must never cross the bridge. This
    /// is a boundary guard, not binding authority — the daemon still
    /// validates every bind and owns every reverse-lookup.
    fn require_local_actor_without_bridge(
        authority: &AuthorityContext,
        action_class: GovernedActionClass,
    ) -> AuthorizationResult {
        if authority.auth_method == AuthMethod::HostedBridge {
            return Err(ActionDenial::unauthorized(
                action_class,
                "Workspace binding cannot be authored through the hosted bridge; \
                 the local candidate_path never crosses the bridge and binding \
                 is daemon-only authority",
            ));
        }
        match authority.auth_method {
            AuthMethod::LocalDaemon
            | AuthMethod::McpSession
            | AuthMethod::Cli
            | AuthMethod::Dashboard => Ok(ActionPermit { action_class }),
            AuthMethod::HostedBridge => unreachable!(),
        }
    }

    /// Check that the actor has owner-level authority for decision signoff.
    ///
    /// Owner authority requires:
    /// 1. Local surface (daemon, CLI, or dashboard — not MCP agent sessions)
    /// 2. The actor must not be a non-authority surface identity
    ///
    /// MCP agent sessions can review candidates but cannot sign off on
    /// decisions — signoff is a human ownership action.
    fn require_owner_authority(
        authority: &AuthorityContext,
        action_class: GovernedActionClass,
    ) -> AuthorizationResult {
        // MCP sessions and hosted bridge cannot perform signoff — signoff is
        // human ownership through a local surface only.
        if authority.auth_method == AuthMethod::McpSession
            || authority.auth_method == AuthMethod::HostedBridge
        {
            return Err(ActionDenial::unauthorized(
                action_class,
                "Decision signoff requires owner authority; MCP agent sessions \
                 and hosted bridge cannot approve or reject signoff",
            ));
        }

        match authority.auth_method {
            AuthMethod::LocalDaemon | AuthMethod::Cli | AuthMethod::Dashboard => {
                Ok(ActionPermit { action_class })
            }
            AuthMethod::McpSession | AuthMethod::HostedBridge => unreachable!(),
        }
    }
}

/// Check if an actor_id indicates a non-authority surface.
fn is_non_authority_surface(actor_id: &str) -> bool {
    let lower = actor_id.to_lowercase();
    NON_AUTHORITY_SURFACES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn local_human() -> AuthorityContext {
        AuthorityContext {
            actor_id: "user-alice".into(),
            auth_method: AuthMethod::Dashboard,
            session_id: None,
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn mcp_agent() -> AuthorityContext {
        AuthorityContext {
            actor_id: "agent-claude".into(),
            auth_method: AuthMethod::McpSession,
            session_id: Some("sess-1".into()),
            workspace: Some("/repo".into()),
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn cli_user() -> AuthorityContext {
        AuthorityContext {
            actor_id: "user-bob".into(),
            auth_method: AuthMethod::Cli,
            session_id: None,
            workspace: Some("/repo".into()),
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn adapter_identity() -> AuthorityContext {
        AuthorityContext {
            actor_id: "adapter-jira-prod".into(),
            auth_method: AuthMethod::LocalDaemon,
            session_id: None,
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn provider_identity() -> AuthorityContext {
        AuthorityContext {
            actor_id: "provider-github-oauth".into(),
            auth_method: AuthMethod::LocalDaemon,
            session_id: None,
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn empty_actor() -> AuthorityContext {
        AuthorityContext {
            actor_id: "".into(),
            auth_method: AuthMethod::LocalDaemon,
            session_id: None,
            workspace: None,
            policy_scope: vec![],
            audit_metadata: serde_json::Map::new(),
        }
    }

    fn review_accept() -> ToolCommand {
        ToolCommand::ReviewAcceptCandidate(crate::tool_request::ReviewTargetParams {
            target_id: "CAND-1".into(),
            reason: None,
        })
    }

    fn review_reject() -> ToolCommand {
        ToolCommand::ReviewRejectCandidate(crate::tool_request::ReviewTargetParams {
            target_id: "CAND-1".into(),
            reason: None,
        })
    }

    fn signoff_approve() -> ToolCommand {
        ToolCommand::ReviewApproveSignoff(crate::tool_request::ReviewTargetParams {
            target_id: "DEC-1".into(),
            reason: None,
        })
    }

    fn signoff_reject() -> ToolCommand {
        ToolCommand::ReviewRejectSignoff(crate::tool_request::ReviewTargetParams {
            target_id: "DEC-1".into(),
            reason: None,
        })
    }

    fn history_list() -> ToolCommand {
        ToolCommand::HistoryList(crate::tool_request::HistoryListParams {
            decision_id: None,
            include_events: false,
            include_bindings: false,
            since: None,
        })
    }

    fn compliance_resolve() -> ToolCommand {
        ToolCommand::ReviewResolveCompliance(crate::tool_request::ResolveComplianceParams {
            target_id: "DEC-1".into(),
            compliance_verdict: "reflected".into(),
            reason: None,
        })
    }

    fn recall_inspect_evidence() -> ToolCommand {
        ToolCommand::RecallInspectEvidence(crate::recall::RecallInspectEvidenceParams {
            packet_id: uuid::Uuid::nil(),
            target_id: uuid::Uuid::nil(),
            evidence_ref_id: None,
        })
    }

    fn recall_expand_scope() -> ToolCommand {
        ToolCommand::RecallExpandScope(crate::recall::RecallExpandScopeParams {
            packet_id: uuid::Uuid::nil(),
            scope_target: "docs/specs".into(),
            hints: vec![],
        })
    }

    fn recall_promote_candidate() -> ToolCommand {
        ToolCommand::RecallPromoteDecisionCandidate(crate::recall::RecallPromoteCandidateParams {
            packet_id: uuid::Uuid::nil(),
            candidate_id: uuid::Uuid::nil(),
            promotion_outcome: crate::recall::PromotionOutcome::NewConstraint,
            supersedes_decision_id: None,
            scoping_relationship: None,
            approval_proof: None,
            confirmation: None,
        })
    }

    fn recall_request_correction() -> ToolCommand {
        ToolCommand::RecallRequestCorrection(crate::recall::RecallRequestCorrectionParams {
            packet_id: uuid::Uuid::nil(),
            selected_item_ids: vec![uuid::Uuid::nil()],
            correction_kind: crate::recall::CorrectionKind::SourceContradiction,
            rationale: "corpus text contradicts ticket".into(),
            approval_proof: crate::recall::ApprovalProof {
                kind: "signoff_token".into(),
                value: "tok_correction_1".into(),
                actor_id: "user-owner-01".into(),
                provided_at: None,
            },
        })
    }

    // -- Read-only: any actor permitted ------------------------------------

    #[test]
    fn read_only_permitted_for_any_actor() {
        let result = ActionAuthorizationPolicy::evaluate(&local_human(), &history_list());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().action_class, GovernedActionClass::ReadOnly);
    }

    #[test]
    fn read_only_permitted_for_mcp_agent() {
        let result = ActionAuthorizationPolicy::evaluate(&mcp_agent(), &history_list());
        assert!(result.is_ok());
    }

    // -- Candidate review: local actors permitted --------------------------

    #[test]
    fn candidate_review_permitted_for_dashboard_user() {
        let result = ActionAuthorizationPolicy::evaluate(&local_human(), &review_accept());
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().action_class,
            GovernedActionClass::CandidateReview
        );
    }

    #[test]
    fn candidate_review_permitted_for_mcp_agent() {
        let result = ActionAuthorizationPolicy::evaluate(&mcp_agent(), &review_accept());
        assert!(result.is_ok());
    }

    #[test]
    fn candidate_review_permitted_for_cli_user() {
        let result = ActionAuthorizationPolicy::evaluate(&cli_user(), &review_reject());
        assert!(result.is_ok());
    }

    #[test]
    fn candidate_review_denied_for_adapter_identity() {
        let result = ActionAuthorizationPolicy::evaluate(&adapter_identity(), &review_accept());
        assert!(result.is_err());
        let denial = result.unwrap_err();
        assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
        assert_eq!(denial.action_class, GovernedActionClass::CandidateReview);
    }

    #[test]
    fn candidate_review_denied_for_provider_identity() {
        let result = ActionAuthorizationPolicy::evaluate(&provider_identity(), &review_accept());
        assert!(result.is_err());
        let denial = result.unwrap_err();
        assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
    }

    // -- Decision signoff: owner authority required ------------------------

    #[test]
    fn signoff_permitted_for_dashboard_human() {
        let result = ActionAuthorizationPolicy::evaluate(&local_human(), &signoff_approve());
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().action_class,
            GovernedActionClass::DecisionSignoff
        );
    }

    #[test]
    fn signoff_permitted_for_cli_user() {
        let result = ActionAuthorizationPolicy::evaluate(&cli_user(), &signoff_reject());
        assert!(result.is_ok());
    }

    #[test]
    fn signoff_denied_for_mcp_agent() {
        let result = ActionAuthorizationPolicy::evaluate(&mcp_agent(), &signoff_approve());
        assert!(result.is_err());
        let denial = result.unwrap_err();
        assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
        assert_eq!(denial.action_class, GovernedActionClass::DecisionSignoff);
        assert!(denial.message.contains("owner authority"));
    }

    #[test]
    fn signoff_denied_for_adapter_identity() {
        let result = ActionAuthorizationPolicy::evaluate(&adapter_identity(), &signoff_approve());
        assert!(result.is_err());
        let denial = result.unwrap_err();
        assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
    }

    #[test]
    fn signoff_denied_for_provider_identity() {
        let result = ActionAuthorizationPolicy::evaluate(&provider_identity(), &signoff_reject());
        assert!(result.is_err());
        let denial = result.unwrap_err();
        assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
    }

    // -- Empty actor denied for everything ---------------------------------

    #[test]
    fn empty_actor_denied_for_read() {
        let result = ActionAuthorizationPolicy::evaluate(&empty_actor(), &history_list());
        assert!(result.is_err());
        let denial = result.unwrap_err();
        assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
    }

    #[test]
    fn empty_actor_denied_for_review() {
        let result = ActionAuthorizationPolicy::evaluate(&empty_actor(), &review_accept());
        assert!(result.is_err());
    }

    #[test]
    fn empty_actor_denied_for_signoff() {
        let result = ActionAuthorizationPolicy::evaluate(&empty_actor(), &signoff_approve());
        assert!(result.is_err());
    }

    // -- Compliance resolution: passes authority (dispatch defers) ----------

    #[test]
    fn compliance_resolution_passes_authority_for_local_actor() {
        let result = ActionAuthorizationPolicy::evaluate(&local_human(), &compliance_resolve());
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().action_class,
            GovernedActionClass::ComplianceResolution
        );
    }

    #[test]
    fn compliance_resolution_denied_for_adapter_identity() {
        let result =
            ActionAuthorizationPolicy::evaluate(&adapter_identity(), &compliance_resolve());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().reason, ActionDenialReason::Unauthorized);
    }

    // -- Tracking: passes authority (dispatch defers) ----------------------

    #[test]
    fn tracking_passes_authority_for_local_actor() {
        let cmd = ToolCommand::TrackingUntrackSource(crate::tool_request::UntrackSourceParams {
            decision_id: "DEC-1".into(),
            reason: None,
            detail: None,
        });
        let result = ActionAuthorizationPolicy::evaluate(&local_human(), &cmd);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().action_class,
            GovernedActionClass::TrackingMutation
        );
    }

    // -- RecallPacket actions ----------------------------------------------

    #[test]
    fn recall_inspect_evidence_classified_read_only() {
        assert_eq!(
            GovernedActionClass::from_command(&recall_inspect_evidence()),
            GovernedActionClass::ReadOnly
        );
    }

    #[test]
    fn recall_expand_scope_classified_read_only() {
        assert_eq!(
            GovernedActionClass::from_command(&recall_expand_scope()),
            GovernedActionClass::ReadOnly
        );
    }

    #[test]
    fn recall_promote_candidate_classified_candidate_review() {
        assert_eq!(
            GovernedActionClass::from_command(&recall_promote_candidate()),
            GovernedActionClass::CandidateReview
        );
    }

    #[test]
    fn recall_inspect_permitted_for_any_actor() {
        // Read-only recall lookups are permitted for every actor surface,
        // including MCP agents and non-local identities.
        for actor in [mcp_agent(), local_human(), adapter_identity()] {
            let result = ActionAuthorizationPolicy::evaluate(&actor, &recall_inspect_evidence());
            assert!(result.is_ok(), "inspect should be permitted for {actor:?}");
        }
        for actor in [mcp_agent(), local_human(), provider_identity()] {
            let result = ActionAuthorizationPolicy::evaluate(&actor, &recall_expand_scope());
            assert!(
                result.is_ok(),
                "expand_scope should be permitted for {actor:?}"
            );
        }
    }

    #[test]
    fn recall_promote_permitted_for_mcp_agent() {
        // MCP may route exactly one candidate-review ToolRequest on behalf of a
        // local human; candidate review does not by itself confer signoff.
        let result = ActionAuthorizationPolicy::evaluate(&mcp_agent(), &recall_promote_candidate());
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().action_class,
            GovernedActionClass::CandidateReview
        );
    }

    #[test]
    fn recall_promote_denied_for_provider_identity() {
        // Provider/adapter identities are non-authority surfaces and cannot
        // promote candidates even via the RecallPacket path.
        let result =
            ActionAuthorizationPolicy::evaluate(&provider_identity(), &recall_promote_candidate());
        assert!(result.is_err());
    }

    #[test]
    fn recall_request_correction_classified_candidate_review() {
        // request_correction is a governed candidate-review action: it may
        // create/route a correction candidate but never confers signoff,
        // compliance, or Decision-acceptance authority (RFQ #663 / ADR-0024).
        assert_eq!(
            GovernedActionClass::from_command(&recall_request_correction()),
            GovernedActionClass::CandidateReview
        );
    }

    #[test]
    fn recall_request_correction_not_classified_decision_signoff() {
        // Guard against silently promoting correction to owner-signoff class.
        assert_ne!(
            GovernedActionClass::from_command(&recall_request_correction()),
            GovernedActionClass::DecisionSignoff
        );
    }

    #[test]
    fn recall_request_correction_permitted_for_mcp_agent_as_candidate_review() {
        // MCP may route exactly one candidate-review ToolRequest on behalf of a
        // local human; correction is candidate-review, not signoff.
        let result =
            ActionAuthorizationPolicy::evaluate(&mcp_agent(), &recall_request_correction());
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().action_class,
            GovernedActionClass::CandidateReview
        );
    }

    #[test]
    fn recall_request_correction_denied_for_provider_identity() {
        // Non-authority surfaces cannot start a correction even via recall.
        let result =
            ActionAuthorizationPolicy::evaluate(&provider_identity(), &recall_request_correction());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().reason, ActionDenialReason::Unauthorized);
    }

    // -- External ingest forbidden commands --------------------------------

    #[test]
    fn external_ingest_rejects_review_commands() {
        for cmd in EXTERNAL_INGEST_FORBIDDEN_COMMANDS {
            let result = ActionAuthorizationPolicy::evaluate_external_ingest_command(cmd);
            assert!(result.is_err(), "should reject command: {cmd}");
            let denial = result.unwrap_err();
            assert_eq!(denial.reason, ActionDenialReason::Unauthorized);
            assert_eq!(denial.action_class, GovernedActionClass::ExternalIngest);
        }
    }

    #[test]
    fn external_ingest_allows_non_authority_commands() {
        let allowed = ["observe", "submit_evidence", "hint"];
        for cmd in allowed {
            let result = ActionAuthorizationPolicy::evaluate_external_ingest_command(cmd);
            assert!(result.is_ok(), "should allow command: {cmd}");
        }
    }

    // -- Denial reason serialization round-trips ---------------------------

    #[test]
    fn denial_reason_round_trips() {
        for (variant, json) in [
            (ActionDenialReason::Unauthorized, "\"unauthorized\""),
            (ActionDenialReason::Unsupported, "\"unsupported\""),
            (
                ActionDenialReason::TargetIneligible,
                "\"target_ineligible\"",
            ),
            (ActionDenialReason::Deferred, "\"deferred\""),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<ActionDenialReason>(json).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn denial_reason_inverse_coverage() {
        assert!(serde_json::from_str::<ActionDenialReason>("\"forbidden\"").is_err());
        assert!(serde_json::from_str::<ActionDenialReason>("\"Unauthorized\"").is_err());
    }

    #[test]
    fn governed_action_class_round_trips() {
        for (variant, json) in [
            (GovernedActionClass::ReadOnly, "\"read_only\""),
            (GovernedActionClass::CandidateReview, "\"candidate_review\""),
            (GovernedActionClass::DecisionSignoff, "\"decision_signoff\""),
            (
                GovernedActionClass::ComplianceResolution,
                "\"compliance_resolution\"",
            ),
            (GovernedActionClass::BindingMutation, "\"binding_mutation\""),
            (GovernedActionClass::EvidenceRefresh, "\"evidence_refresh\""),
            (
                GovernedActionClass::TrackingMutation,
                "\"tracking_mutation\"",
            ),
            (GovernedActionClass::LocalIngest, "\"local_ingest\""),
            (GovernedActionClass::GraphOperation, "\"graph_operation\""),
            (GovernedActionClass::ExternalIngest, "\"external_ingest\""),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<GovernedActionClass>(json).unwrap(),
                variant
            );
        }
    }

    // -- Rejection code format ---------------------------------------------

    #[test]
    fn rejection_code_matches_wire_format() {
        let denial = ActionDenial::unauthorized(GovernedActionClass::DecisionSignoff, "test");
        assert_eq!(denial.rejection_code(), "unauthorized");

        let denial = ActionDenial::deferred(GovernedActionClass::ComplianceResolution, "test");
        assert_eq!(denial.rejection_code(), "deferred");

        let denial = ActionDenial::target_ineligible(GovernedActionClass::CandidateReview, "test");
        assert_eq!(denial.rejection_code(), "target_ineligible");
    }

    // -- Non-authority surface detection ------------------------------------

    #[test]
    fn non_authority_surfaces_detected() {
        assert!(is_non_authority_surface("adapter-jira"));
        assert!(is_non_authority_surface("Adapter-Slack"));
        assert!(is_non_authority_surface("connector-github"));
        assert!(is_non_authority_surface("provider-google"));
        assert!(is_non_authority_surface("mcp_server-claude"));
        assert!(is_non_authority_surface("profile-default"));
        assert!(is_non_authority_surface("credential-token"));
    }

    #[test]
    fn authority_surfaces_not_blocked() {
        assert!(!is_non_authority_surface("user-alice"));
        assert!(!is_non_authority_surface("agent-claude"));
        assert!(!is_non_authority_surface("admin-bob"));
    }
}
