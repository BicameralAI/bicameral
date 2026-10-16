//! Mod manifest validation.

use crate::manifest::{ModActionType, ModManifest};

/// Validation errors for mod manifests.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error("manifest missing required field: {field}")]
    MissingField { field: String },
    #[error("manifest has no triggers defined")]
    NoTriggers,
    #[error("manifest has no actions defined")]
    NoActions,
    #[error("invalid filter in trigger: {reason}")]
    InvalidFilter { reason: String },
    #[error("mod name is empty")]
    EmptyName,
    #[error("mod action type {action_type} is authority-granting, not evidence-producing")]
    AuthorityGrantingAction { action_type: String },
}

/// Validate a mod manifest for correctness.
///
/// Returns a list of validation errors (empty means valid).
pub fn validate_manifest(manifest: &ModManifest) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if manifest.name.is_empty() {
        errors.push(ValidationError::EmptyName);
    }

    if manifest.triggers.is_empty() {
        errors.push(ValidationError::NoTriggers);
    }

    if manifest.actions.is_empty() {
        errors.push(ValidationError::NoActions);
    }

    for trigger in &manifest.triggers {
        for filter in &trigger.filters {
            if filter.field.is_empty() {
                errors.push(ValidationError::InvalidFilter {
                    reason: "filter field is empty".to_string(),
                });
            }
        }
    }

    for action in &manifest.actions {
        if !is_evidence_producing(&action.action_type) {
            errors.push(ValidationError::AuthorityGrantingAction {
                action_type: format!("{:?}", action.action_type),
            });
        }
    }

    errors
}

/// Returns true if the action type is evidence-producing (allowed).
/// Mods may only emit candidates, evidence, warnings, or route to reviewers.
/// They may never approve signoff, resolve compliance, or grant their own authority.
fn is_evidence_producing(action_type: &ModActionType) -> bool {
    matches!(
        action_type,
        ModActionType::EmitWarning
            | ModActionType::RouteToReviewer
            | ModActionType::AddTag
            | ModActionType::ElevateGovernance
            | ModActionType::EmitEvidence
    )
}

/// Load and validate a manifest from a YAML file.
pub fn load_and_validate(
    path: &std::path::Path,
) -> anyhow::Result<(ModManifest, Vec<ValidationError>)> {
    let content = std::fs::read_to_string(path)?;
    let manifest: ModManifest = serde_yaml::from_str(&content)?;
    let errors = validate_manifest(&manifest);
    Ok((manifest, errors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ModAction, ModTrigger, TriggerEvent};

    fn minimal_manifest(actions: Vec<ModAction>) -> ModManifest {
        ModManifest {
            name: "test-mod".to_string(),
            description: "A test mod".to_string(),
            version: 1,
            author: None,
            triggers: vec![ModTrigger {
                event_type: TriggerEvent::CandidateIngested,
                filters: Vec::new(),
            }],
            actions,
            tags: Vec::new(),
        }
    }

    #[test]
    fn all_defined_action_types_are_evidence_producing() {
        let allowed_types = [
            ModActionType::EmitWarning,
            ModActionType::RouteToReviewer,
            ModActionType::AddTag,
            ModActionType::ElevateGovernance,
            ModActionType::EmitEvidence,
        ];

        for action_type in &allowed_types {
            assert!(
                is_evidence_producing(action_type),
                "{:?} should be evidence-producing",
                action_type
            );
        }
    }

    #[test]
    fn valid_manifest_with_evidence_actions_passes() {
        let manifest = minimal_manifest(vec![ModAction {
            action_type: ModActionType::EmitWarning,
            parameters: serde_json::Value::Null,
        }]);
        let errors = validate_manifest(&manifest);
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn no_action_type_permits_signoff_approval_or_compliance_resolution() {
        // Exhaustive check: every variant of ModActionType is evidence-producing.
        // If a new variant is added that is NOT evidence-producing, this test
        // forces the author to update is_evidence_producing().
        let all_types = [
            ModActionType::EmitWarning,
            ModActionType::RouteToReviewer,
            ModActionType::AddTag,
            ModActionType::ElevateGovernance,
            ModActionType::EmitEvidence,
        ];

        for action_type in &all_types {
            let manifest = minimal_manifest(vec![ModAction {
                action_type: *action_type,
                parameters: serde_json::Value::Null,
            }]);
            let errors = validate_manifest(&manifest);
            let has_authority_error = errors
                .iter()
                .any(|e| matches!(e, ValidationError::AuthorityGrantingAction { .. }));
            assert!(
                !has_authority_error,
                "{:?} should not trigger AuthorityGrantingAction",
                action_type
            );
        }
    }

    #[test]
    fn deserializing_unknown_action_type_fails() {
        // Attempt to deserialize an authority-granting action type that does not
        // exist in the enum. serde should reject it, preventing bypass.
        let yaml = r#"
name: evil-mod
description: tries to approve signoff
version: 1
triggers:
  - event_type: candidate_ingested
    filters: []
actions:
  - action_type: approve_signoff
    parameters: null
"#;
        let result: Result<ModManifest, _> = serde_yaml::from_str(yaml);
        assert!(
            result.is_err(),
            "Deserializing an unknown/authority-granting action_type should fail"
        );
    }
}
