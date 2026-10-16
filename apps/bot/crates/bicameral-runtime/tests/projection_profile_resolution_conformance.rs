//! Conformance tests for projection profile resolution — #562.
//!
//! Validates that the bot correctly resolves `TargetIntegrationDescriptor`
//! and `ProjectionProfile` references by id, validates compatibility with
//! canonical artifact types and requested operations, and rejects unknown,
//! stale, unsupported, or policy-incompatible profiles.
//!
//! These tests exercise the `ProfileRegistry` from `bicameral-api` in a
//! runtime-test context to confirm cross-crate consumption works.

use bicameral_api::egress::{
    CanonicalArtifactType, ProfileRegistry, ProfileRejection, ProjectionProfile, TargetAdapter,
    TargetIntegrationDescriptor,
};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn linear_descriptor() -> TargetIntegrationDescriptor {
    TargetIntegrationDescriptor {
        target_system: "linear".into(),
        api_docs_ref: "https://api.linear.app/graphql".into(),
        auth_scopes: vec!["read".into(), "write".into()],
        target_surfaces: vec!["issue".into(), "comment".into()],
        ingress_capabilities: vec!["list".into(), "read".into()],
        egress_capabilities: vec!["create".into(), "update".into(), "comment".into()],
        connector_profiles: vec![],
        projection_profiles: vec![
            ProjectionProfile {
                profile_id: "linear.issue.summary.v1".into(),
                target_system: "linear".into(),
                target_surface: "issue".into(),
                allowed_fields: vec!["description".into()],
                forbidden_fields: vec!["assignee".into(), "state".into(), "priority".into()],
                required_authority_label: "egress:summary".into(),
                required_canonical_ref: true,
                required_receipt: true,
                rendering_constraints: {
                    let mut m = serde_json::Map::new();
                    m.insert(
                        "max_description_length".into(),
                        serde_json::Value::Number(10000.into()),
                    );
                    Some(m)
                },
                mutation_capability: "update".into(),
                approval_class_hint: Some("standard".into()),
                reconciliation_capability: Some("update_in_place".into()),
            },
            ProjectionProfile {
                profile_id: "linear.issue.work_item.v1".into(),
                target_system: "linear".into(),
                target_surface: "issue".into(),
                allowed_fields: vec!["title".into(), "description".into(), "team".into()],
                forbidden_fields: vec!["assignee".into(), "priority".into()],
                required_authority_label: "egress:work_item".into(),
                required_canonical_ref: true,
                required_receipt: true,
                rendering_constraints: None,
                mutation_capability: "create".into(),
                approval_class_hint: Some("elevated".into()),
                reconciliation_capability: Some("append_only".into()),
            },
            ProjectionProfile {
                profile_id: "linear.comment.status.v1".into(),
                target_system: "linear".into(),
                target_surface: "comment".into(),
                allowed_fields: vec!["body".into()],
                forbidden_fields: vec!["state".into(), "assignee".into(), "priority".into()],
                required_authority_label: "egress:status".into(),
                required_canonical_ref: true,
                required_receipt: true,
                rendering_constraints: None,
                mutation_capability: "create".into(),
                approval_class_hint: None,
                reconciliation_capability: Some("append_only".into()),
            },
        ],
        target_adapters: vec![TargetAdapter {
            target_system: "linear".into(),
            supported_operations: vec!["create".into(), "update".into(), "comment".into()],
            rendering_constraints: None,
            credential_scope: "issue:write".into(),
            rate_limit_behavior: None,
            receipt_mapping: None,
        }],
    }
}

// ---------------------------------------------------------------------------
// Known profile resolution
// ---------------------------------------------------------------------------

#[test]
fn known_profile_resolves_with_correct_metadata() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.issue.summary.v1",
            CanonicalArtifactType::Decision,
            "update",
        )
        .expect("known profile should resolve");

    assert_eq!(resolved.profile.profile_id, "linear.issue.summary.v1");
    assert_eq!(resolved.target_system, "linear");
    assert_eq!(resolved.profile.target_surface, "issue");
    assert_eq!(resolved.profile.mutation_capability, "update");
    assert!(resolved.profile.required_canonical_ref);
    assert!(resolved.profile.required_receipt);
}

#[test]
fn known_profile_preserves_rendering_constraints() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.issue.summary.v1",
            CanonicalArtifactType::Decision,
            "update",
        )
        .unwrap();

    let constraints = resolved
        .rendering_constraints
        .expect("summary profile has rendering constraints");
    assert!(constraints.contains_key("max_description_length"));
    assert_eq!(
        constraints["max_description_length"],
        serde_json::Value::Number(10000.into())
    );
}

#[test]
fn known_profile_includes_adapter_operations() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.issue.summary.v1",
            CanonicalArtifactType::Decision,
            "update",
        )
        .unwrap();

    assert!(resolved.adapter_operations.contains(&"create".to_string()));
    assert!(resolved.adapter_operations.contains(&"update".to_string()));
    assert!(resolved.adapter_operations.contains(&"comment".to_string()));
}

#[test]
fn known_work_item_profile_resolves_for_create() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.issue.work_item.v1",
            CanonicalArtifactType::Decision,
            "create",
        )
        .expect("work_item profile should resolve for create");

    assert_eq!(resolved.profile.profile_id, "linear.issue.work_item.v1");
    assert_eq!(resolved.profile.mutation_capability, "create");
    assert_eq!(
        resolved.profile.approval_class_hint,
        Some("elevated".into())
    );
}

#[test]
fn known_comment_profile_resolves_for_candidate() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.comment.status.v1",
            CanonicalArtifactType::Candidate,
            "create",
        )
        .expect("comment profile should resolve for candidate artifact");

    assert_eq!(resolved.profile.profile_id, "linear.comment.status.v1");
    assert_eq!(resolved.profile.target_surface, "comment");
}

// ---------------------------------------------------------------------------
// Unknown profile rejection
// ---------------------------------------------------------------------------

#[test]
fn unknown_profile_returns_typed_rejection() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let result = registry.resolve(
        "jira.issue.summary.v1",
        CanonicalArtifactType::Decision,
        "update",
    );

    match result {
        Err(ProfileRejection::UnknownProfile { profile_id }) => {
            assert_eq!(profile_id, "jira.issue.summary.v1");
        }
        other => panic!("expected UnknownProfile rejection, got {other:?}"),
    }
}

#[test]
fn empty_registry_rejects_all_profiles() {
    let registry = ProfileRegistry::new();

    let result = registry.resolve(
        "linear.issue.summary.v1",
        CanonicalArtifactType::Decision,
        "update",
    );

    assert!(matches!(
        result,
        Err(ProfileRejection::UnknownProfile { .. })
    ));
}

#[test]
fn unknown_profile_rejection_serializes_correctly() {
    let rejection = ProfileRejection::UnknownProfile {
        profile_id: "nonexistent.v1".into(),
    };
    let json = serde_json::to_value(&rejection).unwrap();
    assert_eq!(json["kind"], "unknown_profile");
    assert_eq!(json["profile_id"], "nonexistent.v1");
}

// ---------------------------------------------------------------------------
// Incompatible artifact rejection
// ---------------------------------------------------------------------------

#[test]
fn incompatible_artifact_type_rejected() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    // GovernanceResult is only compatible with "comment", but
    // linear.issue.summary.v1 targets "issue".
    let result = registry.resolve(
        "linear.issue.summary.v1",
        CanonicalArtifactType::GovernanceResult,
        "update",
    );

    match result {
        Err(ProfileRejection::IncompatibleArtifact {
            profile_id,
            artifact_type,
            target_surface,
            compatible_surfaces,
        }) => {
            assert_eq!(profile_id, "linear.issue.summary.v1");
            assert_eq!(artifact_type, "governance_result");
            assert_eq!(target_surface, "issue");
            assert!(compatible_surfaces.contains(&"comment".to_string()));
            assert!(!compatible_surfaces.contains(&"issue".to_string()));
        }
        other => panic!("expected IncompatibleArtifact rejection, got {other:?}"),
    }
}

#[test]
fn binding_artifact_incompatible_with_issue_surface() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    // Binding is only compatible with "comment".
    let result = registry.resolve(
        "linear.issue.work_item.v1",
        CanonicalArtifactType::Binding,
        "create",
    );

    match result {
        Err(ProfileRejection::IncompatibleArtifact {
            artifact_type,
            target_surface,
            ..
        }) => {
            assert_eq!(artifact_type, "binding");
            assert_eq!(target_surface, "issue");
        }
        other => panic!("expected IncompatibleArtifact, got {other:?}"),
    }
}

#[test]
fn decision_compatible_with_both_issue_and_comment() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    // Decision can target both "issue" and "comment".
    let issue_result = registry.resolve(
        "linear.issue.summary.v1",
        CanonicalArtifactType::Decision,
        "update",
    );
    assert!(issue_result.is_ok());

    let comment_result = registry.resolve(
        "linear.comment.status.v1",
        CanonicalArtifactType::Decision,
        "create",
    );
    assert!(comment_result.is_ok());
}

// ---------------------------------------------------------------------------
// Unsupported operation rejection
// ---------------------------------------------------------------------------

#[test]
fn unsupported_operation_rejected() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    // summary.v1 supports "update", not "create".
    let result = registry.resolve(
        "linear.issue.summary.v1",
        CanonicalArtifactType::Decision,
        "create",
    );

    match result {
        Err(ProfileRejection::UnsupportedOperation {
            profile_id,
            requested_operation,
            supported_operation,
        }) => {
            assert_eq!(profile_id, "linear.issue.summary.v1");
            assert_eq!(requested_operation, "create");
            assert_eq!(supported_operation, "update");
        }
        other => panic!("expected UnsupportedOperation rejection, got {other:?}"),
    }
}

#[test]
fn work_item_rejects_update_operation() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    // work_item.v1 supports "create", not "update".
    let result = registry.resolve(
        "linear.issue.work_item.v1",
        CanonicalArtifactType::Decision,
        "update",
    );

    match result {
        Err(ProfileRejection::UnsupportedOperation {
            requested_operation,
            supported_operation,
            ..
        }) => {
            assert_eq!(requested_operation, "update");
            assert_eq!(supported_operation, "create");
        }
        other => panic!("expected UnsupportedOperation, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Policy-incompatible and stale rejections
// ---------------------------------------------------------------------------

#[test]
fn policy_incompatible_rejection() {
    let rejection = ProfileRegistry::reject_policy_incompatible(
        "linear.issue.summary.v1",
        "profile capability cannot waive bot approval requirements",
    );

    match rejection {
        ProfileRejection::PolicyIncompatible { profile_id, detail } => {
            assert_eq!(profile_id, "linear.issue.summary.v1");
            assert!(detail.contains("cannot waive"));
            assert!(detail.contains("approval"));
        }
        other => panic!("expected PolicyIncompatible, got {other:?}"),
    }
}

#[test]
fn stale_profile_rejection() {
    let rejection = ProfileRegistry::reject_stale(
        "linear.issue.summary.v1",
        "descriptor captured 48h ago, max freshness is 24h",
    );

    match rejection {
        ProfileRejection::StaleProfile { profile_id, detail } => {
            assert_eq!(profile_id, "linear.issue.summary.v1");
            assert!(detail.contains("48h"));
        }
        other => panic!("expected StaleProfile, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Cross-crate round-trip and serde
// ---------------------------------------------------------------------------

#[test]
fn descriptor_round_trips_through_json() {
    let desc = linear_descriptor();
    let json = serde_json::to_string(&desc).unwrap();
    let restored: TargetIntegrationDescriptor = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.target_system, desc.target_system);
    assert_eq!(
        restored.projection_profiles.len(),
        desc.projection_profiles.len()
    );
    assert_eq!(
        restored.projection_profiles[0].profile_id,
        desc.projection_profiles[0].profile_id
    );
}

#[test]
fn profile_forbidden_fields_preserved() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.issue.summary.v1",
            CanonicalArtifactType::Decision,
            "update",
        )
        .unwrap();

    assert!(resolved
        .profile
        .forbidden_fields
        .contains(&"assignee".to_string()));
    assert!(resolved
        .profile
        .forbidden_fields
        .contains(&"state".to_string()));
    assert!(resolved
        .profile
        .forbidden_fields
        .contains(&"priority".to_string()));
}

#[test]
fn profile_allowed_fields_preserved() {
    let mut registry = ProfileRegistry::new();
    registry.register(linear_descriptor());

    let resolved = registry
        .resolve(
            "linear.issue.work_item.v1",
            CanonicalArtifactType::Decision,
            "create",
        )
        .unwrap();

    assert!(resolved
        .profile
        .allowed_fields
        .contains(&"title".to_string()));
    assert!(resolved
        .profile
        .allowed_fields
        .contains(&"description".to_string()));
    assert!(resolved
        .profile
        .allowed_fields
        .contains(&"team".to_string()));
}

#[test]
fn rejection_display_messages_are_informative() {
    let unknown = ProfileRejection::UnknownProfile {
        profile_id: "test.v1".into(),
    };
    assert!(format!("{unknown}").contains("unknown projection profile"));

    let incompatible = ProfileRejection::IncompatibleArtifact {
        profile_id: "p".into(),
        artifact_type: "binding".into(),
        target_surface: "issue".into(),
        compatible_surfaces: vec!["comment".into()],
    };
    assert!(format!("{incompatible}").contains("incompatible"));

    let unsupported = ProfileRejection::UnsupportedOperation {
        profile_id: "p".into(),
        requested_operation: "delete".into(),
        supported_operation: "update".into(),
    };
    assert!(format!("{unsupported}").contains("delete"));
    assert!(format!("{unsupported}").contains("update"));
}
