//! Bot-owned egress projection schemas.
//!
//! These types define the governed egress path from Bicameral canonical
//! authority into external target systems. The bot owns eligibility,
//! projection, policy, and receipt — integrations own target shape metadata
//! only.
//!
//! Spec: `docs/specs/governed-egress-projection-ownership.md`
//! Policy: `docs/specs/action-authorization-policy.md`
//!
//! Key invariants:
//! - `ProjectionProfile` (integrations-owned) cannot grant permission or
//!   waive approval.
//! - `TargetAdapter` (integrations-owned) cannot rewrite projection content,
//!   decide policy, grant authority, or promote target-side state.
//! - Egress preview and external write are distinguishable paths.
//! - External writes require eligibility, policy, approval class, and receipt.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Approval class
// ---------------------------------------------------------------------------

/// Approval class for an egress operation.
///
/// Determines what level of approval is required before an external effect
/// may be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalClass {
    /// Read-only preview — no external effect, no approval needed.
    NoneReadOnly,
    /// Product Owner approval required before execution.
    ProductOwnerRequired,
    /// Workspace member confirmation sufficient.
    MemberConfirmation,
}

// ---------------------------------------------------------------------------
// Eligibility
// ---------------------------------------------------------------------------

/// Result of evaluating whether a canonical artifact may be projected to a
/// requested target and profile.
///
/// This is a bot-owned evaluation — integrations-owned `ProjectionProfile`
/// metadata is consumed as shape/capability only, never as permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EgressEligibilityCheck {
    /// Identifier for this eligibility evaluation.
    pub check_id: Uuid,

    /// The canonical artifact under evaluation.
    pub artifact_id: String,

    /// The target system (e.g. `"linear"`, `"github"`, `"slack"`).
    pub target_system: String,

    /// The profile id from integrations (shape/capability only).
    pub profile_id: String,

    /// Whether the artifact is eligible for projection.
    pub eligible: bool,

    /// Approval class determined by bot policy.
    pub approval_class: ApprovalClass,

    /// Warnings surfaced during evaluation (non-blocking).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,

    /// Rejection reasons if not eligible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_reasons: Vec<String>,

    /// Timestamp of the evaluation.
    pub evaluated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Egress mode
// ---------------------------------------------------------------------------

/// Distinguishes preview (read-only) from external write paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressMode {
    /// Read-only preview — no external effect.
    Preview,
    /// External write — creates or mutates external target state.
    ExternalWrite,
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Bot-authored projection payload bound to a canonical artifact revision.
///
/// This is the only payload that target adapters may execute. The bot owns
/// its generation — `ProjectionProfile` provides shape metadata but cannot
/// alter the projection content or policy result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EgressProjection {
    /// Unique projection identifier.
    pub projection_id: Uuid,

    /// The canonical artifact revision being projected.
    pub artifact_id: String,

    /// Content-addressed revision of the artifact at projection time.
    pub artifact_revision: String,

    /// Target system identifier.
    pub target_system: String,

    /// Profile id from integrations (consumed as shape only).
    pub profile_id: String,

    /// The egress mode (preview vs. external write).
    pub mode: EgressMode,

    /// Bot-owned authority label for the projection.
    pub authority_label: String,

    /// Evidence references supporting this projection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,

    /// Policy result that authorized this projection.
    pub policy_result: ProjectionPolicyResult,

    /// The projection payload — target-format content the adapter will use.
    pub payload: serde_json::Value,

    /// Timestamp of projection generation.
    pub projected_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Bot-owned projection policy verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVerdict {
    /// Projection is allowed to proceed.
    Allowed,
    /// Projection requires approval before proceeding.
    ApprovalRequired,
    /// Projection is rejected by policy.
    Rejected,
}

/// Bot-owned policy evaluation for a projection request.
///
/// Decides whether a projection request may proceed, which approval class is
/// required, and which target/profile combinations are allowed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionPolicy {
    /// Identifier for this policy evaluation.
    pub policy_id: Uuid,

    /// The artifact being projected.
    pub artifact_id: String,

    /// Target system.
    pub target_system: String,

    /// Profile id.
    pub profile_id: String,

    /// Actor requesting the projection.
    pub actor_id: String,

    /// Policy verdict.
    pub verdict: PolicyVerdict,

    /// Approval class when verdict is `ApprovalRequired`.
    pub approval_class: ApprovalClass,

    /// Reasons for the policy decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,

    /// Timestamp of policy evaluation.
    pub evaluated_at: DateTime<Utc>,
}

/// The policy result embedded in an `EgressProjection`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionPolicyResult {
    /// The policy evaluation id.
    pub policy_id: Uuid,

    /// The verdict.
    pub verdict: PolicyVerdict,

    /// Approval class.
    pub approval_class: ApprovalClass,
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

/// Reconciliation state for an egress receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationState {
    /// No reconciliation needed (preview only).
    NotApplicable,
    /// Awaiting reconciliation observation.
    Pending,
    /// Reconciliation observed and confirmed.
    Confirmed,
    /// Reconciliation observed a discrepancy.
    Diverged,
}

/// Audit record of a completed egress operation.
///
/// Records the target operation, target object/url, payload hash, canonical
/// revision, policy result, actor, timestamp, and reconciliation state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EgressReceipt {
    /// Unique receipt identifier.
    pub receipt_id: Uuid,

    /// The projection that was executed.
    pub projection_id: Uuid,

    /// The egress mode that was executed.
    pub mode: EgressMode,

    /// Target operation performed (e.g. `"create_issue"`, `"update_comment"`).
    pub target_operation: String,

    /// Target system.
    pub target_system: String,

    /// Target-side object identifier or URL returned by the adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_object_ref: Option<String>,

    /// SHA-256 hash of the projection payload at execution time.
    pub payload_hash: String,

    /// Canonical artifact revision at execution time.
    pub artifact_revision: String,

    /// Policy result that authorized execution.
    pub policy_result: ProjectionPolicyResult,

    /// Actor who initiated or approved the operation.
    pub actor_id: String,

    /// Timestamp of execution.
    pub executed_at: DateTime<Utc>,

    /// Reconciliation state.
    pub reconciliation_state: ReconciliationState,
}

// ---------------------------------------------------------------------------
// Integrations-consumed reference types (shape/capability only)
// ---------------------------------------------------------------------------

/// Reference to an integrations-owned `ProjectionProfile`.
///
/// The bot consumes this as shape/capability metadata only. A profile
/// reference cannot grant permission, waive approval, decide artifact
/// eligibility, mutate canonical state, interpret external state as
/// Bicameral authority, or bypass `ProjectionPolicy`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionProfileRef {
    /// Profile identifier (e.g. `"linear.issue.summary.v1"`).
    pub profile_id: String,

    /// Target system the profile describes.
    pub target_system: String,

    /// Human-readable description of target shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Declared capabilities (read-only metadata).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// Reference to an integrations-owned `TargetAdapter`.
///
/// The adapter executes an approved operation and returns target result data.
/// It cannot rewrite projection content, decide policy, grant authority, or
/// promote target-side state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetAdapterRef {
    /// Adapter identifier.
    pub adapter_id: String,

    /// Target system.
    pub target_system: String,

    /// Supported operations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_operations: Vec<String>,
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

impl EgressEligibilityCheck {
    /// Build an eligible preview check for fixture use.
    pub fn fixture_allowed_preview(artifact_id: &str, profile_id: &str) -> Self {
        Self {
            check_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            eligible: true,
            approval_class: ApprovalClass::NoneReadOnly,
            warnings: Vec::new(),
            rejection_reasons: Vec::new(),
            evaluated_at: Utc::now(),
        }
    }

    /// Build an eligible external-write check requiring approval.
    pub fn fixture_approval_required_write(artifact_id: &str, profile_id: &str) -> Self {
        Self {
            check_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            eligible: true,
            approval_class: ApprovalClass::ProductOwnerRequired,
            warnings: Vec::new(),
            rejection_reasons: Vec::new(),
            evaluated_at: Utc::now(),
        }
    }

    /// Build a rejected check for an unsupported profile.
    pub fn fixture_rejected_unsupported_profile(artifact_id: &str, profile_id: &str) -> Self {
        Self {
            check_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            eligible: false,
            approval_class: ApprovalClass::NoneReadOnly,
            warnings: Vec::new(),
            rejection_reasons: vec![format!(
                "Profile '{}' is not supported for target 'linear'",
                profile_id
            )],
            evaluated_at: Utc::now(),
        }
    }
}

impl ProjectionPolicy {
    /// Build an allowed policy for preview.
    pub fn fixture_allowed_preview(artifact_id: &str, profile_id: &str, actor: &str) -> Self {
        Self {
            policy_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            actor_id: actor.into(),
            verdict: PolicyVerdict::Allowed,
            approval_class: ApprovalClass::NoneReadOnly,
            reasons: vec!["Preview is read-only, no approval required".into()],
            evaluated_at: Utc::now(),
        }
    }

    /// Build a policy requiring Product Owner approval for external write.
    pub fn fixture_approval_required_write(
        artifact_id: &str,
        profile_id: &str,
        actor: &str,
    ) -> Self {
        Self {
            policy_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            actor_id: actor.into(),
            verdict: PolicyVerdict::ApprovalRequired,
            approval_class: ApprovalClass::ProductOwnerRequired,
            reasons: vec!["External write to linear requires Product Owner approval".into()],
            evaluated_at: Utc::now(),
        }
    }

    /// Build a rejected policy.
    pub fn fixture_rejected(artifact_id: &str, profile_id: &str, actor: &str) -> Self {
        Self {
            policy_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            actor_id: actor.into(),
            verdict: PolicyVerdict::Rejected,
            approval_class: ApprovalClass::NoneReadOnly,
            reasons: vec![format!(
                "Profile '{}' is not allowed for actor '{}'",
                profile_id, actor
            )],
            evaluated_at: Utc::now(),
        }
    }
}

impl EgressProjection {
    /// Build a preview projection for fixture use.
    pub fn fixture_preview(
        artifact_id: &str,
        profile_id: &str,
        policy_result: ProjectionPolicyResult,
    ) -> Self {
        Self {
            projection_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            artifact_revision: "rev-fixture-001".into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            mode: EgressMode::Preview,
            authority_label: "bicameral.daemon.alpha".into(),
            evidence_refs: vec!["evidence-ref-001".into()],
            policy_result,
            payload: serde_json::json!({
                "title": "Fixture preview projection",
                "summary": "Preview of canonical artifact state"
            }),
            projected_at: Utc::now(),
        }
    }

    /// Build an external-write projection for fixture use.
    pub fn fixture_external_write(
        artifact_id: &str,
        profile_id: &str,
        policy_result: ProjectionPolicyResult,
    ) -> Self {
        Self {
            projection_id: Uuid::new_v4(),
            artifact_id: artifact_id.into(),
            artifact_revision: "rev-fixture-002".into(),
            target_system: "linear".into(),
            profile_id: profile_id.into(),
            mode: EgressMode::ExternalWrite,
            authority_label: "bicameral.daemon.alpha".into(),
            evidence_refs: vec!["evidence-ref-002".into()],
            policy_result,
            payload: serde_json::json!({
                "title": "Fixture work item",
                "description": "Implementation-ready work item from accepted artifact"
            }),
            projected_at: Utc::now(),
        }
    }
}

impl EgressReceipt {
    /// Build a preview receipt for fixture use.
    pub fn fixture_preview(projection_id: Uuid, policy_result: ProjectionPolicyResult) -> Self {
        Self {
            receipt_id: Uuid::new_v4(),
            projection_id,
            mode: EgressMode::Preview,
            target_operation: "preview".into(),
            target_system: "linear".into(),
            target_object_ref: None,
            payload_hash: "sha256:fixture-preview-hash".into(),
            artifact_revision: "rev-fixture-001".into(),
            policy_result,
            actor_id: "user-member-01".into(),
            executed_at: Utc::now(),
            reconciliation_state: ReconciliationState::NotApplicable,
        }
    }

    /// Build an external-write receipt for fixture use.
    pub fn fixture_external_write(
        projection_id: Uuid,
        policy_result: ProjectionPolicyResult,
    ) -> Self {
        Self {
            receipt_id: Uuid::new_v4(),
            projection_id,
            mode: EgressMode::ExternalWrite,
            target_operation: "create_issue".into(),
            target_system: "linear".into(),
            target_object_ref: Some("https://linear.app/team/issue/PROJ-99".into()),
            payload_hash: "sha256:fixture-write-hash".into(),
            artifact_revision: "rev-fixture-002".into(),
            policy_result,
            actor_id: "user-owner-01".into(),
            executed_at: Utc::now(),
            reconciliation_state: ReconciliationState::Pending,
        }
    }
}
