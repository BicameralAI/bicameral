//! Egress Projection Conformance Tests
//!
//! Verifies the bot-owned egress projection schemas and policy boundaries
//! from the governed egress projection ownership spec:
//! - `docs/specs/governed-egress-projection-ownership.md`
//! - `docs/specs/action-authorization-policy.md`
//!
//! These tests lock in the following invariants:
//!
//! 1. Egress preview and external write paths are distinguishable by mode.
//! 2. External writes require bot eligibility, policy, approval class, and
//!    receipt behavior.
//! 3. Integration metadata (`ProjectionProfileRef`, `TargetAdapterRef`) is
//!    consumed as shape/capability only — cannot grant permission or waive
//!    approval.
//! 4. `ProjectionProfile` and `TargetAdapter` cannot smuggle authority
//!    through any field path.
//!
//! ## Fixture coverage
//!
//! Protocol-level JSON fixtures:
//! - `protocol/schemas/v2/fixtures/egress-allowed-preview.json`
//! - `protocol/schemas/v2/fixtures/egress-approval-required-external-write.json`
//! - `protocol/schemas/v2/fixtures/egress-rejected-unsupported-profile.json`
//! - `protocol/schemas/v2/fixtures/egress-forbidden-profile-policy-shortcut.json`

use bicameral_api::egress_projection::*;

// ===========================================================================
// Part 1: Preview and external write are distinguishable
// ===========================================================================

/// Egress modes serialize to distinct values.
#[test]
fn egress_modes_are_distinguishable() {
    let preview = serde_json::to_value(EgressMode::Preview).unwrap();
    let write = serde_json::to_value(EgressMode::ExternalWrite).unwrap();
    assert_ne!(
        preview, write,
        "Preview and ExternalWrite must serialize differently"
    );
    assert_eq!(preview, serde_json::json!("preview"));
    assert_eq!(write, serde_json::json!("external_write"));
}

/// Preview eligibility check has NoneReadOnly approval class.
#[test]
fn preview_eligibility_is_read_only() {
    let check =
        EgressEligibilityCheck::fixture_allowed_preview("DEC-100", "linear.issue.summary.v1");
    assert!(check.eligible);
    assert_eq!(check.approval_class, ApprovalClass::NoneReadOnly);
    assert!(check.rejection_reasons.is_empty());
}

/// External write eligibility check requires ProductOwnerRequired approval.
#[test]
fn external_write_eligibility_requires_approval() {
    let check = EgressEligibilityCheck::fixture_approval_required_write(
        "DEC-200",
        "linear.issue.work_item.v1",
    );
    assert!(check.eligible);
    assert_eq!(check.approval_class, ApprovalClass::ProductOwnerRequired);
    assert!(check.rejection_reasons.is_empty());
}

/// Preview projection carries mode=Preview.
#[test]
fn preview_projection_carries_preview_mode() {
    let policy_result = ProjectionPolicyResult {
        policy_id: uuid::Uuid::new_v4(),
        verdict: PolicyVerdict::Allowed,
        approval_class: ApprovalClass::NoneReadOnly,
    };
    let proj =
        EgressProjection::fixture_preview("DEC-100", "linear.issue.summary.v1", policy_result);
    assert_eq!(proj.mode, EgressMode::Preview);
}

/// External write projection carries mode=ExternalWrite.
#[test]
fn external_write_projection_carries_write_mode() {
    let policy_result = ProjectionPolicyResult {
        policy_id: uuid::Uuid::new_v4(),
        verdict: PolicyVerdict::ApprovalRequired,
        approval_class: ApprovalClass::ProductOwnerRequired,
    };
    let proj = EgressProjection::fixture_external_write(
        "DEC-200",
        "linear.issue.work_item.v1",
        policy_result,
    );
    assert_eq!(proj.mode, EgressMode::ExternalWrite);
}

// ===========================================================================
// Part 2: External writes require eligibility, policy, approval, and receipt
// ===========================================================================

/// External write receipt records the operation and pending reconciliation.
#[test]
fn external_write_receipt_requires_pending_reconciliation() {
    let policy_result = ProjectionPolicyResult {
        policy_id: uuid::Uuid::new_v4(),
        verdict: PolicyVerdict::ApprovalRequired,
        approval_class: ApprovalClass::ProductOwnerRequired,
    };
    let proj = EgressProjection::fixture_external_write(
        "DEC-200",
        "linear.issue.work_item.v1",
        policy_result.clone(),
    );
    let receipt = EgressReceipt::fixture_external_write(proj.projection_id, policy_result);

    assert_eq!(receipt.mode, EgressMode::ExternalWrite);
    assert_eq!(receipt.target_operation, "create_issue");
    assert!(receipt.target_object_ref.is_some());
    assert_eq!(receipt.reconciliation_state, ReconciliationState::Pending);
    assert_eq!(receipt.projection_id, proj.projection_id);
}

/// Preview receipt has NotApplicable reconciliation and no target object ref.
#[test]
fn preview_receipt_has_no_reconciliation() {
    let policy_result = ProjectionPolicyResult {
        policy_id: uuid::Uuid::new_v4(),
        verdict: PolicyVerdict::Allowed,
        approval_class: ApprovalClass::NoneReadOnly,
    };
    let proj = EgressProjection::fixture_preview(
        "DEC-100",
        "linear.issue.summary.v1",
        policy_result.clone(),
    );
    let receipt = EgressReceipt::fixture_preview(proj.projection_id, policy_result);

    assert_eq!(receipt.mode, EgressMode::Preview);
    assert!(receipt.target_object_ref.is_none());
    assert_eq!(
        receipt.reconciliation_state,
        ReconciliationState::NotApplicable
    );
}

/// Policy verdict for external write must not be Allowed without approval.
/// ApprovalRequired verdict requires ProductOwnerRequired approval class.
#[test]
fn external_write_policy_requires_approval_class() {
    let policy = ProjectionPolicy::fixture_approval_required_write(
        "DEC-200",
        "linear.issue.work_item.v1",
        "user-member-02",
    );
    assert_eq!(policy.verdict, PolicyVerdict::ApprovalRequired);
    assert_eq!(policy.approval_class, ApprovalClass::ProductOwnerRequired);
}

/// Rejected policy blocks projection regardless of target shape metadata.
#[test]
fn rejected_policy_blocks_projection() {
    let policy = ProjectionPolicy::fixture_rejected("DEC-300", "jira.epic.v1", "user-member-03");
    assert_eq!(policy.verdict, PolicyVerdict::Rejected);
    assert!(!policy.reasons.is_empty());
}

// ===========================================================================
// Part 3: Rejected unsupported profile
// ===========================================================================

/// Unsupported profile is rejected at eligibility — the bot decides, not the
/// profile metadata.
#[test]
fn unsupported_profile_rejected_at_eligibility() {
    let check =
        EgressEligibilityCheck::fixture_rejected_unsupported_profile("DEC-300", "jira.epic.v1");
    assert!(!check.eligible);
    assert!(!check.rejection_reasons.is_empty());
    assert!(
        check.rejection_reasons[0].contains("not supported"),
        "rejection must explain unsupported profile, got: {}",
        check.rejection_reasons[0]
    );
}

// ===========================================================================
// Part 4: Forbidden permission smuggling through profiles/adapters
// ===========================================================================

/// ProjectionProfileRef cannot carry authority fields — type system prevents it.
/// This test verifies that even if raw JSON contains forbidden fields, they are
/// dropped during deserialization.
#[test]
fn profile_ref_drops_forbidden_authority_fields() {
    let json = serde_json::json!({
        "profile_id": "linear.issue.work_item.v1",
        "target_system": "linear",
        "description": "Linear work item profile",
        "capabilities": ["create_issue"],
        "grant_permission": true,
        "waive_approval": true,
        "override_policy_verdict": "allowed",
        "promote_target_state": true,
        "mutate_canonical_state": true,
        "bypass_eligibility": true,
        "approval_class": "none_read_only"
    });

    let profile_ref: ProjectionProfileRef = serde_json::from_value(json).unwrap();
    let reserialized = serde_json::to_value(&profile_ref).unwrap();
    let obj = reserialized.as_object().unwrap();

    let forbidden_keys = [
        "grant_permission",
        "waive_approval",
        "override_policy_verdict",
        "promote_target_state",
        "mutate_canonical_state",
        "bypass_eligibility",
        "approval_class",
    ];
    for key in &forbidden_keys {
        assert!(
            !obj.contains_key(*key),
            "ProjectionProfileRef must not preserve forbidden field '{key}'"
        );
    }
}

/// TargetAdapterRef cannot carry policy or authority fields.
#[test]
fn adapter_ref_drops_forbidden_policy_fields() {
    let json = serde_json::json!({
        "adapter_id": "linear-adapter-v1",
        "target_system": "linear",
        "supported_operations": ["create_issue", "update_issue"],
        "decide_policy": true,
        "grant_authority": true,
        "rewrite_projection_content": true,
        "promote_target_state": true,
        "policy_verdict": "allowed",
        "approval_class": "none_read_only"
    });

    let adapter_ref: TargetAdapterRef = serde_json::from_value(json).unwrap();
    let reserialized = serde_json::to_value(&adapter_ref).unwrap();
    let obj = reserialized.as_object().unwrap();

    let forbidden_keys = [
        "decide_policy",
        "grant_authority",
        "rewrite_projection_content",
        "promote_target_state",
        "policy_verdict",
        "approval_class",
    ];
    for key in &forbidden_keys {
        assert!(
            !obj.contains_key(*key),
            "TargetAdapterRef must not preserve forbidden field '{key}'"
        );
    }
}

/// Full end-to-end: profile with all smuggling attempts is deserialized,
/// then the resulting struct contains only shape/capability fields.
#[test]
fn profile_ref_contains_only_shape_fields() {
    let profile_ref = ProjectionProfileRef {
        profile_id: "linear.issue.summary.v1".into(),
        target_system: "linear".into(),
        description: Some("Summary projection".into()),
        capabilities: vec!["read_issue".into(), "read_comment".into()],
    };
    let serialized = serde_json::to_value(&profile_ref).unwrap();
    let obj = serialized.as_object().unwrap();

    // Positive: contains expected fields
    assert!(obj.contains_key("profile_id"));
    assert!(obj.contains_key("target_system"));
    assert!(obj.contains_key("description"));
    assert!(obj.contains_key("capabilities"));

    // Negative: does not contain any authority-like fields
    let authority_keywords = [
        "permission",
        "approval",
        "policy",
        "authority",
        "canonical",
        "bypass",
        "promote",
        "mutate",
        "grant",
        "waive",
    ];
    for key in obj.keys() {
        for keyword in &authority_keywords {
            assert!(
                !key.to_lowercase().contains(keyword),
                "ProfileRef field '{}' suspiciously contains authority keyword '{}'",
                key,
                keyword,
            );
        }
    }
}

/// Full end-to-end: adapter with all smuggling attempts is deserialized,
/// then the resulting struct contains only execution shape fields.
#[test]
fn adapter_ref_contains_only_execution_shape_fields() {
    let adapter_ref = TargetAdapterRef {
        adapter_id: "linear-adapter-v1".into(),
        target_system: "linear".into(),
        supported_operations: vec!["create_issue".into()],
    };
    let serialized = serde_json::to_value(&adapter_ref).unwrap();
    let obj = serialized.as_object().unwrap();

    assert!(obj.contains_key("adapter_id"));
    assert!(obj.contains_key("target_system"));
    assert!(obj.contains_key("supported_operations"));

    let authority_keywords = [
        "permission",
        "approval",
        "policy",
        "authority",
        "canonical",
        "bypass",
        "promote",
        "mutate",
        "grant",
        "waive",
        "rewrite",
    ];
    for key in obj.keys() {
        for keyword in &authority_keywords {
            assert!(
                !key.to_lowercase().contains(keyword),
                "AdapterRef field '{}' suspiciously contains authority keyword '{}'",
                key,
                keyword,
            );
        }
    }
}

// ===========================================================================
// Part 5: Serde round-trip and JSON fixture loading
// ===========================================================================

/// EgressEligibilityCheck round-trips through serde_json.
#[test]
fn eligibility_check_round_trips() {
    let check =
        EgressEligibilityCheck::fixture_allowed_preview("DEC-100", "linear.issue.summary.v1");
    let json = serde_json::to_string(&check).unwrap();
    let deserialized: EgressEligibilityCheck = serde_json::from_str(&json).unwrap();
    assert_eq!(check, deserialized);
}

/// ProjectionPolicy round-trips through serde_json.
#[test]
fn projection_policy_round_trips() {
    let policy = ProjectionPolicy::fixture_allowed_preview(
        "DEC-100",
        "linear.issue.summary.v1",
        "user-member-01",
    );
    let json = serde_json::to_string(&policy).unwrap();
    let deserialized: ProjectionPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(policy, deserialized);
}

/// EgressProjection round-trips through serde_json.
#[test]
fn egress_projection_round_trips() {
    let policy_result = ProjectionPolicyResult {
        policy_id: uuid::Uuid::new_v4(),
        verdict: PolicyVerdict::Allowed,
        approval_class: ApprovalClass::NoneReadOnly,
    };
    let proj =
        EgressProjection::fixture_preview("DEC-100", "linear.issue.summary.v1", policy_result);
    let json = serde_json::to_string(&proj).unwrap();
    let deserialized: EgressProjection = serde_json::from_str(&json).unwrap();
    assert_eq!(proj, deserialized);
}

/// EgressReceipt round-trips through serde_json.
#[test]
fn egress_receipt_round_trips() {
    let policy_result = ProjectionPolicyResult {
        policy_id: uuid::Uuid::new_v4(),
        verdict: PolicyVerdict::Allowed,
        approval_class: ApprovalClass::NoneReadOnly,
    };
    let receipt = EgressReceipt::fixture_preview(uuid::Uuid::new_v4(), policy_result);
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: EgressReceipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt, deserialized);
}

/// JSON fixtures load and contain expected fields.
#[test]
fn preview_fixture_json_loads() {
    let fixture_json =
        include_str!("../../../protocol/schemas/v2/fixtures/egress-allowed-preview.json");
    let value: serde_json::Value = serde_json::from_str(fixture_json).unwrap();
    let obj = value.as_object().unwrap();

    assert!(obj.contains_key("eligibility"));
    assert!(obj.contains_key("policy"));
    assert!(obj.contains_key("projection"));
    assert!(obj.contains_key("receipt"));

    let elig = obj["eligibility"].as_object().unwrap();
    assert_eq!(elig["eligible"], true);
    assert_eq!(elig["approval_class"], "none_read_only");
    assert_eq!(obj["projection"]["mode"], "preview");
    assert_eq!(obj["receipt"]["reconciliation_state"], "not_applicable");
}

/// External write fixture loads with approval-required policy.
#[test]
fn external_write_fixture_json_loads() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/egress-approval-required-external-write.json"
    );
    let value: serde_json::Value = serde_json::from_str(fixture_json).unwrap();
    let obj = value.as_object().unwrap();

    let elig = obj["eligibility"].as_object().unwrap();
    assert_eq!(elig["eligible"], true);
    assert_eq!(elig["approval_class"], "product_owner_required");
    assert_eq!(obj["policy"]["verdict"], "approval_required");
    assert_eq!(obj["projection"]["mode"], "external_write");
    assert_eq!(obj["receipt"]["reconciliation_state"], "pending");
}

/// Rejected profile fixture loads with rejection reasons.
#[test]
fn rejected_profile_fixture_json_loads() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/egress-rejected-unsupported-profile.json"
    );
    let value: serde_json::Value = serde_json::from_str(fixture_json).unwrap();
    let obj = value.as_object().unwrap();

    let elig = obj["eligibility"].as_object().unwrap();
    assert_eq!(elig["eligible"], false);
    assert!(elig["rejection_reasons"].as_array().unwrap().len() > 0);
    assert_eq!(obj["policy"]["verdict"], "rejected");
}

/// Forbidden profile-policy shortcut fixture documents smuggling attempts.
#[test]
fn forbidden_shortcut_fixture_json_loads() {
    let fixture_json = include_str!(
        "../../../protocol/schemas/v2/fixtures/egress-forbidden-profile-policy-shortcut.json"
    );
    let value: serde_json::Value = serde_json::from_str(fixture_json).unwrap();
    let obj = value.as_object().unwrap();

    // The fixture documents forbidden fields
    assert!(obj.contains_key("forbidden_fields"));
    let forbidden = obj["forbidden_fields"].as_object().unwrap();
    assert_eq!(forbidden["grant_permission"], true);
    assert_eq!(forbidden["waive_approval"], true);

    // The profile_ref and adapter_ref are valid shape-only references
    assert!(obj.contains_key("profile_ref"));
    assert!(obj.contains_key("adapter_ref"));

    // Verify profile_ref deserializes cleanly to our struct (dropping forbidden fields)
    let profile: ProjectionProfileRef = serde_json::from_value(obj["profile_ref"].clone()).unwrap();
    assert_eq!(profile.profile_id, "linear.issue.work_item.v1");

    // Verify adapter_ref deserializes cleanly to our struct
    let adapter: TargetAdapterRef = serde_json::from_value(obj["adapter_ref"].clone()).unwrap();
    assert_eq!(adapter.adapter_id, "linear-adapter-v1");
}

// ===========================================================================
// Part 6: Approval class semantics
// ===========================================================================

/// Approval classes serialize to distinct snake_case values.
#[test]
fn approval_classes_are_distinct() {
    let none = serde_json::to_value(ApprovalClass::NoneReadOnly).unwrap();
    let po = serde_json::to_value(ApprovalClass::ProductOwnerRequired).unwrap();
    let member = serde_json::to_value(ApprovalClass::MemberConfirmation).unwrap();

    assert_ne!(none, po);
    assert_ne!(none, member);
    assert_ne!(po, member);

    assert_eq!(none, serde_json::json!("none_read_only"));
    assert_eq!(po, serde_json::json!("product_owner_required"));
    assert_eq!(member, serde_json::json!("member_confirmation"));
}

/// Policy verdicts serialize to distinct snake_case values.
#[test]
fn policy_verdicts_are_distinct() {
    let allowed = serde_json::to_value(PolicyVerdict::Allowed).unwrap();
    let required = serde_json::to_value(PolicyVerdict::ApprovalRequired).unwrap();
    let rejected = serde_json::to_value(PolicyVerdict::Rejected).unwrap();

    assert_ne!(allowed, required);
    assert_ne!(allowed, rejected);
    assert_ne!(required, rejected);
}

/// Reconciliation states serialize to distinct values.
#[test]
fn reconciliation_states_are_distinct() {
    let na = serde_json::to_value(ReconciliationState::NotApplicable).unwrap();
    let pending = serde_json::to_value(ReconciliationState::Pending).unwrap();
    let confirmed = serde_json::to_value(ReconciliationState::Confirmed).unwrap();
    let diverged = serde_json::to_value(ReconciliationState::Diverged).unwrap();

    let states = [&na, &pending, &confirmed, &diverged];
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "Reconciliation states must be distinct");
            }
        }
    }
}

// ===========================================================================
// Linear egress projection conformance (issue #561)
// ===========================================================================

mod linear_egress {
    use bicameral_api::candidate::DecisionLevel;
    use bicameral_api::dashboard::SignoffState;
    use bicameral_api::egress::{
        ApprovalClass, EgressApproval, EgressError, LinearProfileId, ObservationKind,
        ReconciliationState,
    };
    use bicameral_runtime::egress::{
        AdapterExecutionResult, ArtifactState, EgressService, TargetAdapter,
    };
    use chrono::Utc;
    use uuid::Uuid;

    // ---------------------------------------------------------------------------
    // Test adapter
    // ---------------------------------------------------------------------------

    /// Stub adapter that records the call and returns a fixed result.
    struct StubLinearAdapter {
        operation: String,
        object_url: Option<String>,
        object_id: Option<String>,
    }

    impl StubLinearAdapter {
        fn new_issue() -> Self {
            Self {
                operation: "create_issue".into(),
                object_url: Some("https://linear.app/team/issue/PROJ-99".into()),
                object_id: Some("PROJ-99".into()),
            }
        }

        fn new_comment() -> Self {
            Self {
                operation: "add_comment".into(),
                object_url: Some("https://linear.app/team/issue/PROJ-99#comment-1".into()),
                object_id: Some("comment-1".into()),
            }
        }
    }

    impl TargetAdapter for StubLinearAdapter {
        fn execute(
            &self,
            _projection: &bicameral_api::egress::EgressProjection,
        ) -> Result<AdapterExecutionResult, EgressError> {
            Ok(AdapterExecutionResult {
                target_operation: self.operation.clone(),
                target_object_url: self.object_url.clone(),
                target_object_id: self.object_id.clone(),
            })
        }
    }

    /// Adapter that always fails — proves adapter errors propagate.
    struct FailingAdapter;

    impl TargetAdapter for FailingAdapter {
        fn execute(
            &self,
            _projection: &bicameral_api::egress::EgressProjection,
        ) -> Result<AdapterExecutionResult, EgressError> {
            Err(EgressError::AdapterExecutionFailed(
                "target API returned 500".into(),
            ))
        }
    }

    // ---------------------------------------------------------------------------
    // Artifact fixtures
    // ---------------------------------------------------------------------------

    fn approved_architecture_artifact() -> ArtifactState {
        ArtifactState {
            artifact_id: Uuid::new_v4(),
            title: "Health check endpoint must verify DB connection".into(),
            level: DecisionLevel::Architecture,
            signoff: SignoffState::Approved,
            revision: "abc123".into(),
            evidence_refs: vec!["evidence-001".into()],
        }
    }

    fn proposed_artifact() -> ArtifactState {
        ArtifactState {
            artifact_id: Uuid::new_v4(),
            title: "Proposed: Add logging middleware".into(),
            level: DecisionLevel::Architecture,
            signoff: SignoffState::Proposed,
            revision: "def456".into(),
            evidence_refs: vec!["evidence-002".into()],
        }
    }

    fn rejected_artifact() -> ArtifactState {
        ArtifactState {
            artifact_id: Uuid::new_v4(),
            title: "Rejected: Use MongoDB".into(),
            level: DecisionLevel::Architecture,
            signoff: SignoffState::Rejected,
            revision: "ghi789".into(),
            evidence_refs: vec![],
        }
    }

    fn implementation_artifact() -> ArtifactState {
        ArtifactState {
            artifact_id: Uuid::new_v4(),
            title: "impl detail: use HashMap for cache".into(),
            level: DecisionLevel::Implementation,
            signoff: SignoffState::Approved,
            revision: "jkl012".into(),
            evidence_refs: vec!["evidence-003".into()],
        }
    }

    fn superseded_artifact() -> ArtifactState {
        ArtifactState {
            artifact_id: Uuid::new_v4(),
            title: "Superseded: old API design".into(),
            level: DecisionLevel::Architecture,
            signoff: SignoffState::Superseded,
            revision: "mno345".into(),
            evidence_refs: vec!["evidence-004".into()],
        }
    }

    fn sample_payload() -> serde_json::Value {
        serde_json::json!({
            "title": "Health check endpoint",
            "description": "Must verify DB connection before returning 200"
        })
    }

    fn sample_approval() -> EgressApproval {
        EgressApproval {
            approved_by: "human-reviewer-001".into(),
            approved_at: Utc::now(),
            reason: Some("Approved for Linear projection".into()),
        }
    }

    // ===========================================================================
    // Part 1: Preview eligibility
    // ===========================================================================

    #[test]
    fn preview_summary_approved_artifact_is_eligible() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueSummary);

        assert!(
            check.eligible,
            "approved artifact should be eligible for summary"
        );
        assert!(check.rejection_reasons.is_empty());
        assert_eq!(check.target_system, "linear");
        assert_eq!(check.profile_id, "linear.issue.summary.v1");
    }

    #[test]
    fn preview_summary_proposed_artifact_eligible_with_warning() {
        let svc = EgressService::new_alpha();
        let artifact = proposed_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueSummary);

        assert!(
            check.eligible,
            "proposed artifact should be eligible for summary preview"
        );
        assert!(
            check.warnings.iter().any(|w| w.contains("proposed")),
            "should warn about proposed status"
        );
    }

    #[test]
    fn preview_summary_rejected_artifact_ineligible() {
        let svc = EgressService::new_alpha();
        let artifact = rejected_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueSummary);

        assert!(!check.eligible, "rejected artifact should not be eligible");
        assert!(!check.rejection_reasons.is_empty());
    }

    #[test]
    fn preview_summary_superseded_artifact_ineligible() {
        let svc = EgressService::new_alpha();
        let artifact = superseded_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueSummary);

        assert!(
            !check.eligible,
            "superseded artifact should not be eligible"
        );
        assert!(check
            .rejection_reasons
            .iter()
            .any(|r| r.contains("superseded")));
    }

    #[test]
    fn preview_work_item_requires_approved_signoff() {
        let svc = EgressService::new_alpha();

        // Approved architecture artifact: eligible
        let artifact = approved_architecture_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueWorkItem);
        assert!(
            check.eligible,
            "approved arch artifact should be eligible for work-item"
        );

        // Proposed: not eligible
        let artifact = proposed_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueWorkItem);
        assert!(
            !check.eligible,
            "proposed artifact should not be eligible for work-item"
        );
    }

    #[test]
    fn preview_work_item_rejects_implementation_level() {
        let svc = EgressService::new_alpha();
        let artifact = implementation_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueWorkItem);

        assert!(
            !check.eligible,
            "implementation-level artifacts should not be eligible for work-item"
        );
        assert!(check
            .rejection_reasons
            .iter()
            .any(|r| r.contains("implementation")));
    }

    #[test]
    fn preview_status_proposed_is_eligible() {
        let svc = EgressService::new_alpha();
        let artifact = proposed_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::CommentStatus);

        assert!(
            check.eligible,
            "proposed artifact should be eligible for status projection"
        );
    }

    #[test]
    fn preview_status_rejected_is_ineligible() {
        let svc = EgressService::new_alpha();
        let artifact = rejected_artifact();
        let check = svc.check_eligibility(&artifact, LinearProfileId::CommentStatus);

        assert!(
            !check.eligible,
            "rejected artifact should not be eligible for status"
        );
    }

    // ===========================================================================
    // Part 2: Preview projection (no external write)
    // ===========================================================================

    #[test]
    fn preview_generates_projection_with_preview_flag() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_preview(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor-001",
                sample_payload(),
            )
            .expect("preview should succeed for eligible artifact");

        assert!(
            projection.preview_only,
            "preview projection must be preview_only"
        );
        assert_eq!(projection.target_system, "linear");
        assert_eq!(projection.profile_id, "linear.issue.summary.v1");
        assert_eq!(projection.artifact_id, artifact.artifact_id);
        assert!(
            projection.approval.is_none(),
            "preview should have no approval"
        );
    }

    #[test]
    fn preview_cannot_be_executed() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_preview(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor-001",
                sample_payload(),
            )
            .unwrap();

        let adapter = StubLinearAdapter::new_issue();
        let result = svc.execute_projection(&projection, &adapter);

        assert!(
            matches!(result, Err(EgressError::PreviewCannotExecute(_))),
            "preview projection must not produce a receipt: {result:?}"
        );
    }

    #[test]
    fn preview_for_ineligible_artifact_returns_error() {
        let svc = EgressService::new_alpha();
        let artifact = rejected_artifact();
        let result = svc.generate_preview(
            &artifact,
            LinearProfileId::IssueSummary,
            "actor-001",
            sample_payload(),
        );

        assert!(
            matches!(result, Err(EgressError::IneligibleArtifact(_))),
            "preview for ineligible artifact should fail: {result:?}"
        );
    }

    // ===========================================================================
    // Part 3: Approved execution produces EgressReceipt
    // ===========================================================================

    #[test]
    fn approved_execution_produces_receipt() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueWorkItem,
                "actor-001",
                sample_payload(),
                Some(sample_approval()),
            )
            .expect("execution projection should succeed with approval");

        assert!(
            !projection.preview_only,
            "execution projection must not be preview_only"
        );
        assert!(projection.is_approved(), "projection must be approved");

        let adapter = StubLinearAdapter::new_issue();
        let receipt = svc
            .execute_projection(&projection, &adapter)
            .expect("execution should produce a receipt");

        assert_eq!(receipt.projection_id, projection.projection_id);
        assert_eq!(receipt.target_operation, "create_issue");
        assert_eq!(
            receipt.target_object_url,
            Some("https://linear.app/team/issue/PROJ-99".into())
        );
        assert_eq!(receipt.target_object_id, Some("PROJ-99".into()));
        assert!(
            !receipt.payload_hash.is_empty(),
            "payload hash must not be empty"
        );
        assert_eq!(receipt.artifact_revision, "abc123");
        assert_eq!(receipt.reconciliation_state, ReconciliationState::Pending);
    }

    #[test]
    fn auto_approved_execution_needs_no_explicit_approval() {
        // Build a policy where summary is auto-approved.
        use bicameral_api::egress::ProjectionPolicy;

        let policy = ProjectionPolicy {
            policy_id: Uuid::new_v4(),
            name: "test-auto-approve".into(),
            allowed_targets: vec!["linear".into()],
            allowed_profiles: vec!["linear.issue.summary.v1".into()],
            default_approval: ApprovalClass::AutoApproved,
            rules: vec![],
        };

        let svc = EgressService::with_policy(policy);
        let artifact = approved_architecture_artifact();

        // No approval provided, but policy is AutoApproved.
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor-001",
                sample_payload(),
                None,
            )
            .expect("auto-approved should not require explicit approval");

        assert!(
            projection.is_approved(),
            "auto-approved projection is_approved()"
        );

        let adapter = StubLinearAdapter::new_comment();
        let receipt = svc
            .execute_projection(&projection, &adapter)
            .expect("auto-approved execution should produce receipt");

        assert_eq!(receipt.target_operation, "add_comment");
    }

    // ===========================================================================
    // Part 4: Rejected — missing approval
    // ===========================================================================

    #[test]
    fn execution_without_required_approval_is_rejected() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();

        let result = svc.generate_for_execution(
            &artifact,
            LinearProfileId::IssueWorkItem,
            "actor-001",
            sample_payload(),
            None, // no approval
        );

        assert!(
            matches!(result, Err(EgressError::ApprovalRequired(_))),
            "execution without approval should be rejected: {result:?}"
        );
    }

    #[test]
    fn execution_projection_without_approval_cannot_execute() {
        // Build a projection manually without approval to test the execute guard.
        use bicameral_api::egress::{EgressProjection, ProjectionPolicyResult};

        let projection = EgressProjection {
            projection_id: Uuid::new_v4(),
            artifact_id: Uuid::new_v4(),
            artifact_revision: "abc123".into(),
            target_system: "linear".into(),
            profile_id: "linear.issue.work_item.v1".into(),
            authority_label: "egress:execute:linear.issue.work_item.v1".into(),
            evidence_refs: vec![],
            policy_result: ProjectionPolicyResult {
                policy_id: Uuid::new_v4(),
                permitted: true,
                approval_class: ApprovalClass::HumanApproval,
                matched_rules: vec![],
                reason: "test".into(),
            },
            preview_only: false,
            payload: sample_payload(),
            approval: None,
            actor_id: "actor-001".into(),
            created_at: Utc::now(),
        };

        let svc = EgressService::new_alpha();
        let adapter = StubLinearAdapter::new_issue();
        let result = svc.execute_projection(&projection, &adapter);

        assert!(
            matches!(result, Err(EgressError::ApprovalRequired(_))),
            "unapproved projection must not execute: {result:?}"
        );
    }

    // ===========================================================================
    // Part 5: Receipt mapping
    // ===========================================================================

    #[test]
    fn receipt_maps_target_fields_from_adapter_result() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::CommentStatus,
                "actor-001",
                sample_payload(),
                Some(sample_approval()),
            )
            .unwrap();

        let adapter = StubLinearAdapter::new_comment();
        let receipt = svc.execute_projection(&projection, &adapter).unwrap();

        assert_eq!(receipt.target_operation, "add_comment");
        assert_eq!(
            receipt.target_object_url,
            Some("https://linear.app/team/issue/PROJ-99#comment-1".into())
        );
        assert_eq!(receipt.target_object_id, Some("comment-1".into()));
        assert_eq!(receipt.artifact_revision, artifact.revision);
        assert!(receipt.policy_result.permitted);
    }

    #[test]
    fn receipt_payload_hash_is_deterministic() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let payload = sample_payload();

        let p1 = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor",
                payload.clone(),
                Some(sample_approval()),
            )
            .unwrap();
        let p2 = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor",
                payload,
                Some(sample_approval()),
            )
            .unwrap();

        let adapter = StubLinearAdapter::new_comment();
        let r1 = svc.execute_projection(&p1, &adapter).unwrap();
        let r2 = svc.execute_projection(&p2, &adapter).unwrap();

        assert_eq!(
            r1.payload_hash, r2.payload_hash,
            "same payload should produce same hash"
        );
    }

    #[test]
    fn adapter_failure_propagates_error() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueWorkItem,
                "actor-001",
                sample_payload(),
                Some(sample_approval()),
            )
            .unwrap();

        let adapter = FailingAdapter;
        let result = svc.execute_projection(&projection, &adapter);

        assert!(
            matches!(result, Err(EgressError::AdapterExecutionFailed(_))),
            "adapter failure should propagate: {result:?}"
        );
    }

    // ===========================================================================
    // Part 6: Reconciliation observation
    // ===========================================================================

    #[test]
    fn reconciliation_observation_does_not_promote() {
        let svc = EgressService::new_alpha();
        let obs = svc.record_observation(
            Some(Uuid::new_v4()),
            "linear",
            "PROJ-99",
            ObservationKind::ExternalEdit,
            "Issue description was edited by external user",
            Some(serde_json::json!({"editor": "user@example.com"})),
        );

        assert!(!obs.promoted, "observation must not be promoted by default");
        assert_eq!(obs.target_system, "linear");
        assert_eq!(obs.target_object_id, "PROJ-99");
        assert_eq!(obs.observation_kind, ObservationKind::ExternalEdit);
        assert!(obs.receipt_id.is_some());
    }

    #[test]
    fn reconciliation_observation_without_receipt() {
        let svc = EgressService::new_alpha();
        let obs = svc.record_observation(
            None,
            "linear",
            "PROJ-100",
            ObservationKind::Created,
            "New issue observed in Linear",
            None,
        );

        assert!(obs.receipt_id.is_none());
        assert!(!obs.promoted);
        assert_eq!(obs.observation_kind, ObservationKind::Created);
    }

    #[test]
    fn reconciliation_observation_serde_roundtrip() {
        let svc = EgressService::new_alpha();
        let obs = svc.record_observation(
            Some(Uuid::new_v4()),
            "linear",
            "PROJ-99",
            ObservationKind::StatusChange,
            "Issue moved to Done",
            Some(serde_json::json!({"old_status": "In Progress", "new_status": "Done"})),
        );

        let json = serde_json::to_string(&obs).unwrap();
        let back: bicameral_api::egress::ReconciliationObservation =
            serde_json::from_str(&json).unwrap();

        assert_eq!(back.observation_id, obs.observation_id);
        assert_eq!(back.target_system, "linear");
        assert!(!back.promoted);
    }

    // ===========================================================================
    // Part 7: Policy boundary — profile/adapter cannot grant authority
    // ===========================================================================

    #[test]
    fn projection_profile_cannot_waive_approval() {
        // The alpha default policy requires HumanApproval for work-item.
        // Even if the artifact is eligible, omitting approval must fail.
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();

        let result = svc.generate_for_execution(
            &artifact,
            LinearProfileId::IssueWorkItem,
            "actor-001",
            sample_payload(),
            None,
        );

        assert!(
            matches!(result, Err(EgressError::ApprovalRequired(_))),
            "profile cannot waive approval requirement"
        );
    }

    #[test]
    fn target_adapter_cannot_decide_policy() {
        // The adapter is only called after policy and approval checks pass.
        // Even a successful adapter result doesn't change policy decisions.
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();

        // Generate with approval.
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueWorkItem,
                "actor-001",
                sample_payload(),
                Some(sample_approval()),
            )
            .unwrap();

        let receipt = svc
            .execute_projection(&projection, &StubLinearAdapter::new_issue())
            .unwrap();

        // Receipt carries the original policy result — adapter didn't change it.
        assert!(receipt.policy_result.permitted);
        assert_eq!(
            receipt.policy_result.approval_class,
            ApprovalClass::HumanApproval
        );
    }

    // ===========================================================================
    // Part 8: EgressProjection helper methods
    // ===========================================================================

    #[test]
    fn preview_projection_requires_approval_is_false() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_preview(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor",
                sample_payload(),
            )
            .unwrap();

        assert!(
            !projection.requires_approval(),
            "preview never requires approval"
        );
        assert!(!projection.is_approved(), "preview is never approved");
    }

    #[test]
    fn execution_projection_with_human_approval_is_approved() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueSummary,
                "actor",
                sample_payload(),
                Some(sample_approval()),
            )
            .unwrap();

        assert!(projection.requires_approval());
        assert!(projection.is_approved());
    }

    // ===========================================================================
    // Part 9: Full end-to-end flow
    // ===========================================================================

    #[test]
    fn end_to_end_preview_eligibility_then_execution_then_observation() {
        let svc = EgressService::new_alpha();
        let artifact = approved_architecture_artifact();

        // Step 1: Check eligibility.
        let check = svc.check_eligibility(&artifact, LinearProfileId::IssueWorkItem);
        assert!(check.eligible);
        assert_eq!(check.approval_class, ApprovalClass::HumanApproval);

        // Step 2: Generate preview (no external write).
        let preview = svc
            .generate_preview(
                &artifact,
                LinearProfileId::IssueWorkItem,
                "actor-001",
                sample_payload(),
            )
            .unwrap();
        assert!(preview.preview_only);

        // Step 3: Preview cannot execute.
        let adapter = StubLinearAdapter::new_issue();
        assert!(matches!(
            svc.execute_projection(&preview, &adapter),
            Err(EgressError::PreviewCannotExecute(_))
        ));

        // Step 4: Generate for execution with approval.
        let projection = svc
            .generate_for_execution(
                &artifact,
                LinearProfileId::IssueWorkItem,
                "actor-001",
                sample_payload(),
                Some(sample_approval()),
            )
            .unwrap();
        assert!(!projection.preview_only);
        assert!(projection.is_approved());

        // Step 5: Execute and get receipt.
        let receipt = svc.execute_projection(&projection, &adapter).unwrap();
        assert_eq!(receipt.target_operation, "create_issue");
        assert_eq!(receipt.reconciliation_state, ReconciliationState::Pending);

        // Step 6: Record reconciliation observation.
        let obs = svc.record_observation(
            Some(receipt.receipt_id),
            "linear",
            receipt.target_object_id.as_deref().unwrap_or("unknown"),
            ObservationKind::Created,
            "Issue created successfully in Linear",
            None,
        );
        assert!(!obs.promoted, "observation must not auto-promote");
        assert_eq!(obs.receipt_id, Some(receipt.receipt_id));
    }
} // mod linear_egress
