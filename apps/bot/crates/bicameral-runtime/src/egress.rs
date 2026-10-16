//! Governed egress projection service — Linear alpha.
//!
//! Per `docs/specs/governed-egress-projection-ownership.md` and issue #561:
//!
//! - Evaluates Linear summary, work-item, and status projection preview
//!   eligibility against canonical artifact state.
//! - Executes only approved bot-authored `EgressProjection` payloads through an
//!   approved adapter boundary.
//! - Persists and returns `EgressReceipt` for executed target effects.
//! - Captures Linear-side changes as `ReconciliationObservation` that do not
//!   directly mutate canonical authority.
//!
//! ## Boundary invariants
//!
//! - Preview creates no external write receipt.
//! - External write requires approval and produces an audit receipt.
//! - `ProjectionProfile` and `TargetAdapter` cannot grant permission, waive
//!   approval, or decide artifact eligibility.
//! - Reconciliation observations are evidence, not governance verdicts.

use bicameral_api::candidate::DecisionLevel;
use bicameral_api::dashboard::SignoffState;
use bicameral_api::egress::{
    ApprovalClass, EgressApproval, EgressEligibilityCheck, EgressError, EgressProjection,
    EgressReceipt, LinearProfileId, ObservationKind, ProjectionPolicy, ProjectionPolicyResult,
    ProjectionPolicyRule, ReconciliationObservation, ReconciliationState,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Adapter boundary trait
// ---------------------------------------------------------------------------

/// Target adapter boundary for executing approved egress projections.
///
/// Integrations own the adapter implementation. Bot owns the call boundary:
/// only approved `EgressProjection` payloads may be passed to `execute`.
pub trait TargetAdapter {
    /// Execute an approved projection and return the target-side result.
    ///
    /// The adapter must not rewrite projection content, decide policy, grant
    /// authority, or promote target-side state.
    fn execute(&self, projection: &EgressProjection)
        -> Result<AdapterExecutionResult, EgressError>;
}

/// Result from a target adapter execution.
#[derive(Debug, Clone)]
pub struct AdapterExecutionResult {
    /// Target operation performed (e.g. "create_issue", "add_comment").
    pub target_operation: String,
    /// Target-side object URL (e.g. Linear issue URL).
    pub target_object_url: Option<String>,
    /// Target-side object identifier (e.g. Linear issue ID).
    pub target_object_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Artifact state for eligibility evaluation
// ---------------------------------------------------------------------------

/// Minimal canonical artifact state needed for egress eligibility evaluation.
///
/// Decouples the egress service from the full event-store replay path.
#[derive(Debug, Clone)]
pub struct ArtifactState {
    pub artifact_id: Uuid,
    pub title: String,
    pub level: DecisionLevel,
    pub signoff: SignoffState,
    pub revision: String,
    pub evidence_refs: Vec<String>,
}

// ---------------------------------------------------------------------------
// EgressService
// ---------------------------------------------------------------------------

/// Governed egress projection service for the Linear alpha target family.
pub struct EgressService {
    policy: ProjectionPolicy,
}

impl EgressService {
    /// Create a new egress service with the alpha default policy.
    pub fn new_alpha() -> Self {
        Self {
            policy: alpha_default_policy(),
        }
    }

    /// Create a new egress service with a custom policy.
    pub fn with_policy(policy: ProjectionPolicy) -> Self {
        Self { policy }
    }

    /// Check whether a canonical artifact is eligible for projection to a
    /// Linear profile.
    pub fn check_eligibility(
        &self,
        artifact: &ArtifactState,
        profile: LinearProfileId,
    ) -> EgressEligibilityCheck {
        let mut warnings = Vec::new();
        let mut rejection_reasons = Vec::new();

        // Evaluate artifact state against profile requirements.
        let eligible = match profile {
            LinearProfileId::IssueSummary => {
                check_summary_eligibility(artifact, &mut warnings, &mut rejection_reasons)
            }
            LinearProfileId::IssueWorkItem => {
                check_work_item_eligibility(artifact, &mut warnings, &mut rejection_reasons)
            }
            LinearProfileId::CommentStatus => {
                check_status_eligibility(artifact, &mut warnings, &mut rejection_reasons)
            }
        };

        let approval_class = if !eligible {
            ApprovalClass::PreviewOnly
        } else {
            self.evaluate_policy_approval(profile)
        };

        EgressEligibilityCheck {
            check_id: Uuid::new_v4(),
            artifact_id: artifact.artifact_id,
            target_system: "linear".into(),
            profile_id: profile.as_str().into(),
            eligible,
            approval_class,
            warnings,
            rejection_reasons,
            checked_at: Utc::now(),
        }
    }

    /// Generate a preview-only `EgressProjection` (no external write).
    pub fn generate_preview(
        &self,
        artifact: &ArtifactState,
        profile: LinearProfileId,
        actor_id: &str,
        payload: serde_json::Value,
    ) -> Result<EgressProjection, EgressError> {
        let eligibility = self.check_eligibility(artifact, profile);
        if !eligibility.eligible {
            return Err(EgressError::IneligibleArtifact(
                eligibility
                    .rejection_reasons
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "artifact not eligible".into()),
            ));
        }

        let policy_result = self.evaluate_policy(profile);
        if !policy_result.permitted {
            return Err(EgressError::PolicyDenied(policy_result.reason.clone()));
        }

        Ok(EgressProjection {
            projection_id: Uuid::new_v4(),
            artifact_id: artifact.artifact_id,
            artifact_revision: artifact.revision.clone(),
            target_system: "linear".into(),
            profile_id: profile.as_str().into(),
            authority_label: format!("egress:preview:{}", profile.as_str()),
            evidence_refs: artifact.evidence_refs.clone(),
            policy_result,
            preview_only: true,
            payload,
            approval: None,
            actor_id: actor_id.into(),
            created_at: Utc::now(),
        })
    }

    /// Generate an `EgressProjection` for approved execution (external write).
    pub fn generate_for_execution(
        &self,
        artifact: &ArtifactState,
        profile: LinearProfileId,
        actor_id: &str,
        payload: serde_json::Value,
        approval: Option<EgressApproval>,
    ) -> Result<EgressProjection, EgressError> {
        let eligibility = self.check_eligibility(artifact, profile);
        if !eligibility.eligible {
            return Err(EgressError::IneligibleArtifact(
                eligibility
                    .rejection_reasons
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "artifact not eligible".into()),
            ));
        }

        let policy_result = self.evaluate_policy(profile);
        if !policy_result.permitted {
            return Err(EgressError::PolicyDenied(policy_result.reason.clone()));
        }

        // Check approval requirement.
        if policy_result.approval_class == ApprovalClass::HumanApproval && approval.is_none() {
            return Err(EgressError::ApprovalRequired(
                "human approval required for this projection".into(),
            ));
        }

        Ok(EgressProjection {
            projection_id: Uuid::new_v4(),
            artifact_id: artifact.artifact_id,
            artifact_revision: artifact.revision.clone(),
            target_system: "linear".into(),
            profile_id: profile.as_str().into(),
            authority_label: format!("egress:execute:{}", profile.as_str()),
            evidence_refs: artifact.evidence_refs.clone(),
            policy_result,
            preview_only: false,
            payload,
            approval,
            actor_id: actor_id.into(),
            created_at: Utc::now(),
        })
    }

    /// Execute an approved projection through a target adapter and produce an
    /// `EgressReceipt`.
    ///
    /// Preview projections cannot be executed — they produce no receipt.
    pub fn execute_projection(
        &self,
        projection: &EgressProjection,
        adapter: &dyn TargetAdapter,
    ) -> Result<EgressReceipt, EgressError> {
        // Guard: preview projections cannot be executed.
        if projection.preview_only {
            return Err(EgressError::PreviewCannotExecute(
                "preview projections do not produce external writes".into(),
            ));
        }

        // Guard: approval required but not present.
        if projection.requires_approval() && !projection.is_approved() {
            return Err(EgressError::ApprovalRequired(
                "projection requires human approval before execution".into(),
            ));
        }

        // Execute through the adapter boundary.
        let result = adapter.execute(projection)?;

        // Compute payload hash for receipt.
        let payload_bytes = serde_json::to_vec(&projection.payload).unwrap_or_default();
        let payload_hash = format!("{:x}", Sha256::digest(&payload_bytes));

        Ok(EgressReceipt {
            receipt_id: Uuid::new_v4(),
            projection_id: projection.projection_id,
            target_operation: result.target_operation,
            target_object_url: result.target_object_url,
            target_object_id: result.target_object_id,
            payload_hash,
            artifact_revision: projection.artifact_revision.clone(),
            policy_result: projection.policy_result.clone(),
            actor_id: projection.actor_id.clone(),
            executed_at: Utc::now(),
            reconciliation_state: ReconciliationState::Pending,
        })
    }

    /// Record a reconciliation observation from target-side activity.
    ///
    /// Observations are evidence only — they do not directly mutate canonical
    /// Bicameral authority.
    pub fn record_observation(
        &self,
        receipt_id: Option<Uuid>,
        target_system: &str,
        target_object_id: &str,
        kind: ObservationKind,
        summary: &str,
        observed_data: Option<serde_json::Value>,
    ) -> ReconciliationObservation {
        ReconciliationObservation {
            observation_id: Uuid::new_v4(),
            receipt_id,
            target_system: target_system.into(),
            target_object_id: target_object_id.into(),
            observation_kind: kind,
            summary: summary.into(),
            observed_data,
            observed_at: Utc::now(),
            promoted: false,
        }
    }

    // -- internal helpers ---------------------------------------------------

    fn evaluate_policy(&self, profile: LinearProfileId) -> ProjectionPolicyResult {
        let profile_str = profile.as_str();

        // Check if target and profile are allowed.
        let target_allowed = self.policy.allowed_targets.is_empty()
            || self.policy.allowed_targets.iter().any(|t| t == "linear");
        let profile_allowed = self.policy.allowed_profiles.is_empty()
            || self
                .policy
                .allowed_profiles
                .iter()
                .any(|p| p == profile_str);

        if !target_allowed || !profile_allowed {
            return ProjectionPolicyResult {
                policy_id: self.policy.policy_id,
                permitted: false,
                approval_class: self.policy.default_approval,
                matched_rules: vec![],
                reason: format!("target/profile not permitted: linear/{}", profile_str),
            };
        }

        // Evaluate rules.
        let mut matched = Vec::new();
        let mut approval = self.policy.default_approval;

        for rule in &self.policy.rules {
            if rule_matches_profile(&rule.condition, profile_str) {
                matched.push(rule.condition.clone());
                if rule.blocks_projection {
                    return ProjectionPolicyResult {
                        policy_id: self.policy.policy_id,
                        permitted: false,
                        approval_class: rule.approval_class,
                        matched_rules: matched,
                        reason: rule
                            .description
                            .clone()
                            .unwrap_or_else(|| "blocked by policy rule".into()),
                    };
                }
                approval = rule.approval_class;
            }
        }

        ProjectionPolicyResult {
            policy_id: self.policy.policy_id,
            permitted: true,
            approval_class: approval,
            matched_rules: matched,
            reason: "policy permits projection".into(),
        }
    }

    fn evaluate_policy_approval(&self, profile: LinearProfileId) -> ApprovalClass {
        self.evaluate_policy(profile).approval_class
    }
}

// ---------------------------------------------------------------------------
// Eligibility helpers
// ---------------------------------------------------------------------------

/// Summary projection: requires accepted (Approved signoff) artifact.
fn check_summary_eligibility(
    artifact: &ArtifactState,
    warnings: &mut Vec<String>,
    rejection_reasons: &mut Vec<String>,
) -> bool {
    let mut eligible = true;

    match artifact.signoff {
        SignoffState::Approved => {}
        SignoffState::Proposed => {
            warnings.push("artifact is only proposed, not yet approved".into());
        }
        SignoffState::Rejected => {
            rejection_reasons.push("artifact signoff is rejected".into());
            eligible = false;
        }
        SignoffState::Superseded => {
            rejection_reasons.push("artifact has been superseded".into());
            eligible = false;
        }
        SignoffState::CollisionPending => {
            rejection_reasons.push("artifact has a pending collision".into());
            eligible = false;
        }
    }

    if artifact.evidence_refs.is_empty() {
        warnings.push("no evidence references cited".into());
    }

    eligible
}

/// Work-item projection: requires approved signoff and architecture/product
/// level — implementation-ready artifacts only.
fn check_work_item_eligibility(
    artifact: &ArtifactState,
    _warnings: &mut Vec<String>,
    rejection_reasons: &mut Vec<String>,
) -> bool {
    let mut eligible = true;

    match artifact.signoff {
        SignoffState::Approved => {}
        other => {
            rejection_reasons.push(format!(
                "work-item projection requires approved signoff, got {other:?}"
            ));
            eligible = false;
        }
    }

    if artifact.level == DecisionLevel::Implementation {
        rejection_reasons.push(
            "implementation-level artifacts are not eligible for work-item projection".into(),
        );
        eligible = false;
    }

    eligible
}

/// Status projection: requires at least proposed signoff.
fn check_status_eligibility(
    artifact: &ArtifactState,
    _warnings: &mut Vec<String>,
    rejection_reasons: &mut Vec<String>,
) -> bool {
    match artifact.signoff {
        SignoffState::Approved | SignoffState::Proposed => true,
        SignoffState::Rejected => {
            rejection_reasons.push("rejected artifacts cannot have status projected".into());
            false
        }
        SignoffState::Superseded => {
            rejection_reasons.push("superseded artifacts cannot have status projected".into());
            false
        }
        SignoffState::CollisionPending => {
            rejection_reasons
                .push("collision-pending artifacts cannot have status projected".into());
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Policy helpers
// ---------------------------------------------------------------------------

/// Alpha default projection policy for Linear.
pub fn alpha_default_policy() -> ProjectionPolicy {
    ProjectionPolicy {
        policy_id: Uuid::new_v4(),
        name: "alpha-linear-default".into(),
        allowed_targets: vec!["linear".into()],
        allowed_profiles: vec![
            "linear.issue.summary.v1".into(),
            "linear.issue.work_item.v1".into(),
            "linear.comment.status.v1".into(),
        ],
        default_approval: ApprovalClass::HumanApproval,
        rules: vec![
            ProjectionPolicyRule {
                condition: "profile:linear.comment.status.v1".into(),
                approval_class: ApprovalClass::HumanApproval,
                blocks_projection: false,
                description: Some("status projections require human approval in alpha".into()),
            },
            ProjectionPolicyRule {
                condition: "profile:linear.issue.work_item.v1".into(),
                approval_class: ApprovalClass::HumanApproval,
                blocks_projection: false,
                description: Some("work-item creation requires human approval in alpha".into()),
            },
        ],
    }
}

/// Simple rule matching: checks if condition references the given profile.
fn rule_matches_profile(condition: &str, profile: &str) -> bool {
    if let Some(stripped) = condition.strip_prefix("profile:") {
        stripped == profile
    } else {
        false
    }
}
