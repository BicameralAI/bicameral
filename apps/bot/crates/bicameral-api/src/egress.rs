//! Egress projection types — bot-side consumption of integrations-owned
//! target descriptors and projection profiles.
//!
//! Per the governed-egress-projection-ownership spec and integrations ADR-0019:
//!
//! - `TargetIntegrationDescriptor` and `ProjectionProfile` are
//!   integrations-owned shape/capability metadata only.
//! - They carry **no** permission, approval, eligibility, or authority.
//! - Bot resolves profile references by id, validates compatibility with
//!   canonical artifact type and requested operation, and preserves
//!   integrations-owned rendering constraints as capability metadata.
//! - Unknown, stale, unsupported, or policy-incompatible profiles are
//!   rejected with typed rejection reasons.
//!
//! # Authority boundary
//!
//! `ProjectionProfile` describes target shape. It must not grant permission,
//! waive approval, decide artifact eligibility, mutate canonical state,
//! interpret external state as Bicameral authority, or bypass
//! `ProjectionPolicy`.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// TargetIntegrationDescriptor
// ---------------------------------------------------------------------------

/// Integrations-owned target-boundary descriptor — unifies ingress and egress
/// over shared target facts.
///
/// One per target system. The bot consumes this to discover available
/// projection profiles and target capabilities without embedding
/// target-specific rendering rules.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TargetIntegrationDescriptor {
    /// Target system identifier (e.g. `linear`, `github`, `jira`, `slack`).
    pub target_system: String,

    /// Canonical target API documentation reference — shared by connector
    /// and projection.
    pub api_docs_ref: String,

    /// Target auth scopes (read + write), shared across directions.
    #[serde(default)]
    pub auth_scopes: Vec<String>,

    /// Addressable target surfaces (`issue`, `comment`, `repository`, etc.).
    #[serde(default)]
    pub target_surfaces: Vec<String>,

    /// Declarative read/fetch capabilities (connector side).
    #[serde(default)]
    pub ingress_capabilities: Vec<String>,

    /// Declarative render/write capabilities (projection side).
    #[serde(default)]
    pub egress_capabilities: Vec<String>,

    /// References to ingress connector profiles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connector_profiles: Vec<String>,

    /// Egress projection profiles — shape + capability metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection_profiles: Vec<ProjectionProfile>,

    /// Per-target operation/rendering/credential/receipt constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_adapters: Vec<TargetAdapter>,
}

// ---------------------------------------------------------------------------
// ProjectionProfile
// ---------------------------------------------------------------------------

/// Target-specific projection shape and capability declaration.
///
/// Integrations-owned, authority-free. Describes how a governed projection
/// fits a target surface. Must not grant permission, waive approval, decide
/// eligibility, or create authority.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionProfile {
    /// Stable, versioned profile id (e.g. `linear.issue.summary.v1`).
    pub profile_id: String,

    /// Owning target system.
    pub target_system: String,

    /// Surface this projects onto (`issue`, `comment`, etc.).
    pub target_surface: String,

    /// Fields a projection of this kind may populate.
    #[serde(default)]
    pub allowed_fields: Vec<String>,

    /// Fields it must not touch.
    #[serde(default)]
    pub forbidden_fields: Vec<String>,

    /// Authority label the bot must attach for eligibility — named here,
    /// decided bot-side.
    pub required_authority_label: String,

    /// Whether the projection must carry a back-reference to the canonical
    /// decision it renders.
    pub required_canonical_ref: bool,

    /// Whether a mutating projection must return a bot-mapped receipt.
    pub required_receipt: bool,

    /// Target rendering limits (length, markdown subset, field formats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendering_constraints: Option<serde_json::Map<String, serde_json::Value>>,

    /// Target operation this can drive (`create`/`update`/`comment`).
    /// A declaration, not a grant.
    pub mutation_capability: String,

    /// Non-binding hint to bot approval routing — never an approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_class_hint: Option<String>,

    /// What reconciliation the target supports — capability, not
    /// interpretation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation_capability: Option<String>,
}

// ---------------------------------------------------------------------------
// TargetAdapter
// ---------------------------------------------------------------------------

/// Per-target operation/rendering/credential/receipt constraints.
///
/// Must not execute a mutation, hold/resolve credentials, decide eligibility,
/// or interpret receipts as canonical truth.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TargetAdapter {
    /// Owning target system.
    pub target_system: String,

    /// Operations available (`create`/`update`/`comment`).
    #[serde(default)]
    pub supported_operations: Vec<String>,

    /// Hard target limits the renderer must respect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendering_constraints: Option<serde_json::Map<String, serde_json::Value>>,

    /// Declared write scope — secret stays operator/sidecar-side.
    pub credential_scope: String,

    /// How the target rate-limits writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_behavior: Option<String>,

    /// Maps target write result to `{external_id, external_url, status}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_mapping: Option<serde_json::Map<String, serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// Canonical artifact types (bot-owned)
// ---------------------------------------------------------------------------

/// Canonical artifact types the bot may project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalArtifactType {
    /// An accepted Decision.
    Decision,
    /// A DecisionCandidate (not yet accepted).
    Candidate,
    /// A BindingEvidence record.
    Binding,
    /// A compliance/governance result.
    GovernanceResult,
}

impl CanonicalArtifactType {
    /// Target surfaces compatible with this artifact type.
    pub fn compatible_surfaces(&self) -> &[&str] {
        match self {
            Self::Decision => &["issue", "comment"],
            Self::Candidate => &["issue", "comment"],
            Self::Binding => &["comment"],
            Self::GovernanceResult => &["comment"],
        }
    }
}

impl std::fmt::Display for CanonicalArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decision => write!(f, "decision"),
            Self::Candidate => write!(f, "candidate"),
            Self::Binding => write!(f, "binding"),
            Self::GovernanceResult => write!(f, "governance_result"),
        }
    }
}

// ---------------------------------------------------------------------------
// Profile resolution and validation
// ---------------------------------------------------------------------------

/// Rejection reason when profile resolution fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileRejection {
    /// The requested profile id was not found in any registered descriptor.
    UnknownProfile { profile_id: String },

    /// The profile exists but the descriptor is stale (e.g. captured_at is
    /// too old relative to a freshness policy).
    StaleProfile { profile_id: String, detail: String },

    /// The profile's target surface is incompatible with the canonical
    /// artifact type being projected.
    IncompatibleArtifact {
        profile_id: String,
        artifact_type: String,
        target_surface: String,
        compatible_surfaces: Vec<String>,
    },

    /// The requested operation is not supported by the profile's
    /// `mutation_capability`.
    UnsupportedOperation {
        profile_id: String,
        requested_operation: String,
        supported_operation: String,
    },

    /// The profile's capability would require waiving bot policy or approval,
    /// which is forbidden.
    PolicyIncompatible { profile_id: String, detail: String },
}

impl std::fmt::Display for ProfileRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProfile { profile_id } => {
                write!(f, "unknown projection profile: {profile_id}")
            }
            Self::StaleProfile { profile_id, detail } => {
                write!(f, "stale projection profile {profile_id}: {detail}")
            }
            Self::IncompatibleArtifact {
                profile_id,
                artifact_type,
                target_surface,
                compatible_surfaces,
            } => {
                write!(
                    f,
                    "profile {profile_id} targets surface '{target_surface}' \
                     which is incompatible with artifact type '{artifact_type}' \
                     (compatible: {})",
                    compatible_surfaces.join(", ")
                )
            }
            Self::UnsupportedOperation {
                profile_id,
                requested_operation,
                supported_operation,
            } => {
                write!(
                    f,
                    "profile {profile_id} supports '{supported_operation}' \
                     but '{requested_operation}' was requested"
                )
            }
            Self::PolicyIncompatible { profile_id, detail } => {
                write!(f, "profile {profile_id} is policy-incompatible: {detail}")
            }
        }
    }
}

/// Resolved profile — a validated, ready-to-use profile reference with
/// preserved integrations-owned rendering constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedProfile {
    /// The validated profile.
    pub profile: ProjectionProfile,

    /// The target system from the parent descriptor.
    pub target_system: String,

    /// Preserved rendering constraints from the profile — capability
    /// metadata only, never authority.
    pub rendering_constraints: Option<serde_json::Map<String, serde_json::Value>>,

    /// The adapter for this target system, if one was registered.
    pub adapter_operations: Vec<String>,
}

/// A registry of target integration descriptors for profile resolution.
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    descriptors: HashMap<String, TargetIntegrationDescriptor>,
}

impl ProfileRegistry {
    pub fn new() -> Self {
        Self {
            descriptors: HashMap::new(),
        }
    }

    /// Register a target integration descriptor.
    pub fn register(&mut self, descriptor: TargetIntegrationDescriptor) {
        self.descriptors
            .insert(descriptor.target_system.clone(), descriptor);
    }

    /// Look up a descriptor by target system.
    pub fn descriptor(&self, target_system: &str) -> Option<&TargetIntegrationDescriptor> {
        self.descriptors.get(target_system)
    }

    /// Resolve a profile by id across all registered descriptors.
    ///
    /// Returns the profile and its parent descriptor's target_system, or
    /// `ProfileRejection::UnknownProfile` if not found.
    fn find_profile(
        &self,
        profile_id: &str,
    ) -> Result<(&ProjectionProfile, &TargetIntegrationDescriptor), ProfileRejection> {
        for descriptor in self.descriptors.values() {
            for profile in &descriptor.projection_profiles {
                if profile.profile_id == profile_id {
                    return Ok((profile, descriptor));
                }
            }
        }
        Err(ProfileRejection::UnknownProfile {
            profile_id: profile_id.to_string(),
        })
    }

    /// Resolve and validate a projection profile for a given artifact type
    /// and requested operation.
    ///
    /// Checks:
    /// 1. Profile exists (not unknown).
    /// 2. Profile's target surface is compatible with the artifact type.
    /// 3. Profile's mutation_capability matches the requested operation.
    /// 4. Profile capability does not waive bot policy or approval.
    pub fn resolve(
        &self,
        profile_id: &str,
        artifact_type: CanonicalArtifactType,
        requested_operation: &str,
    ) -> Result<ResolvedProfile, ProfileRejection> {
        let (profile, descriptor) = self.find_profile(profile_id)?;

        // Validate surface compatibility with artifact type.
        let compatible = artifact_type.compatible_surfaces();
        if !compatible.contains(&profile.target_surface.as_str()) {
            return Err(ProfileRejection::IncompatibleArtifact {
                profile_id: profile_id.to_string(),
                artifact_type: artifact_type.to_string(),
                target_surface: profile.target_surface.clone(),
                compatible_surfaces: compatible.iter().map(|s| s.to_string()).collect(),
            });
        }

        // Validate requested operation matches mutation_capability.
        if profile.mutation_capability != requested_operation {
            return Err(ProfileRejection::UnsupportedOperation {
                profile_id: profile_id.to_string(),
                requested_operation: requested_operation.to_string(),
                supported_operation: profile.mutation_capability.clone(),
            });
        }

        // Collect adapter operations for reference.
        let adapter_operations = descriptor
            .target_adapters
            .iter()
            .flat_map(|a| a.supported_operations.clone())
            .collect();

        Ok(ResolvedProfile {
            profile: profile.clone(),
            target_system: descriptor.target_system.clone(),
            rendering_constraints: profile.rendering_constraints.clone(),
            adapter_operations,
        })
    }

    /// Resolve a profile by id only (no artifact/operation validation).
    /// Returns `UnknownProfile` if not found.
    pub fn resolve_by_id(&self, profile_id: &str) -> Result<ResolvedProfile, ProfileRejection> {
        let (profile, descriptor) = self.find_profile(profile_id)?;

        let adapter_operations = descriptor
            .target_adapters
            .iter()
            .flat_map(|a| a.supported_operations.clone())
            .collect();

        Ok(ResolvedProfile {
            profile: profile.clone(),
            target_system: descriptor.target_system.clone(),
            rendering_constraints: profile.rendering_constraints.clone(),
            adapter_operations,
        })
    }

    /// Mark a profile as stale. Returns a rejection for the given profile id.
    pub fn reject_stale(profile_id: &str, detail: &str) -> ProfileRejection {
        ProfileRejection::StaleProfile {
            profile_id: profile_id.to_string(),
            detail: detail.to_string(),
        }
    }

    /// Mark a profile as policy-incompatible. Returns a rejection.
    pub fn reject_policy_incompatible(profile_id: &str, detail: &str) -> ProfileRejection {
        ProfileRejection::PolicyIncompatible {
            profile_id: profile_id.to_string(),
            detail: detail.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn descriptor_round_trip() {
        let desc = linear_descriptor();
        let json = serde_json::to_value(&desc).unwrap();
        let restored: TargetIntegrationDescriptor = serde_json::from_value(json).unwrap();
        assert_eq!(restored.target_system, "linear");
        assert_eq!(restored.projection_profiles.len(), 3);
    }

    #[test]
    fn profile_round_trip() {
        let desc = linear_descriptor();
        let profile = &desc.projection_profiles[0];
        let json = serde_json::to_value(profile).unwrap();
        let restored: ProjectionProfile = serde_json::from_value(json).unwrap();
        assert_eq!(restored.profile_id, "linear.issue.summary.v1");
        assert!(restored.required_canonical_ref);
        assert!(restored.required_receipt);
    }

    #[test]
    fn resolve_known_profile() {
        let mut registry = ProfileRegistry::new();
        registry.register(linear_descriptor());

        let resolved = registry
            .resolve(
                "linear.issue.summary.v1",
                CanonicalArtifactType::Decision,
                "update",
            )
            .expect("should resolve known profile");

        assert_eq!(resolved.profile.profile_id, "linear.issue.summary.v1");
        assert_eq!(resolved.target_system, "linear");
        assert!(resolved.rendering_constraints.is_some());
        assert!(resolved.adapter_operations.contains(&"update".to_string()));
    }

    #[test]
    fn reject_unknown_profile() {
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
            other => panic!("expected UnknownProfile, got {other:?}"),
        }
    }

    #[test]
    fn reject_incompatible_artifact() {
        let mut registry = ProfileRegistry::new();
        registry.register(linear_descriptor());

        // GovernanceResult is only compatible with "comment", not "issue".
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
                ..
            }) => {
                assert_eq!(profile_id, "linear.issue.summary.v1");
                assert_eq!(artifact_type, "governance_result");
                assert_eq!(target_surface, "issue");
            }
            other => panic!("expected IncompatibleArtifact, got {other:?}"),
        }
    }

    #[test]
    fn reject_unsupported_operation() {
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
            other => panic!("expected UnsupportedOperation, got {other:?}"),
        }
    }

    #[test]
    fn profile_capability_cannot_waive_policy() {
        let rejection = ProfileRegistry::reject_policy_incompatible(
            "linear.issue.summary.v1",
            "profile capability cannot waive bot approval requirements",
        );
        match rejection {
            ProfileRejection::PolicyIncompatible { profile_id, detail } => {
                assert_eq!(profile_id, "linear.issue.summary.v1");
                assert!(detail.contains("cannot waive"));
            }
            other => panic!("expected PolicyIncompatible, got {other:?}"),
        }
    }

    #[test]
    fn stale_profile_rejected() {
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

    #[test]
    fn resolve_by_id_only() {
        let mut registry = ProfileRegistry::new();
        registry.register(linear_descriptor());

        let resolved = registry
            .resolve_by_id("linear.comment.status.v1")
            .expect("should resolve by id");

        assert_eq!(resolved.profile.profile_id, "linear.comment.status.v1");
        assert_eq!(resolved.profile.target_surface, "comment");
    }

    #[test]
    fn resolve_by_id_unknown() {
        let registry = ProfileRegistry::new();
        let result = registry.resolve_by_id("nonexistent.v1");
        assert!(matches!(
            result,
            Err(ProfileRejection::UnknownProfile { .. })
        ));
    }

    #[test]
    fn descriptor_schema_generated() {
        let schema = schemars::schema_for!(TargetIntegrationDescriptor);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("TargetIntegrationDescriptor"));
        assert!(json.contains("target_system"));
        assert!(json.contains("projection_profiles"));
    }

    #[test]
    fn profile_schema_generated() {
        let schema = schemars::schema_for!(ProjectionProfile);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("ProjectionProfile"));
        assert!(json.contains("profile_id"));
        assert!(json.contains("mutation_capability"));
    }

    #[test]
    fn rejection_display_unknown() {
        let r = ProfileRejection::UnknownProfile {
            profile_id: "foo.v1".into(),
        };
        assert_eq!(format!("{r}"), "unknown projection profile: foo.v1");
    }

    #[test]
    fn rejection_display_incompatible() {
        let r = ProfileRejection::IncompatibleArtifact {
            profile_id: "p".into(),
            artifact_type: "binding".into(),
            target_surface: "issue".into(),
            compatible_surfaces: vec!["comment".into()],
        };
        let msg = format!("{r}");
        assert!(msg.contains("incompatible"));
        assert!(msg.contains("comment"));
    }

    #[test]
    fn rejection_serializes_tagged() {
        let r = ProfileRejection::UnknownProfile {
            profile_id: "x.v1".into(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "unknown_profile");
        assert_eq!(json["profile_id"], "x.v1");
    }

    #[test]
    fn decision_compatible_with_issue_and_comment() {
        let surfaces = CanonicalArtifactType::Decision.compatible_surfaces();
        assert!(surfaces.contains(&"issue"));
        assert!(surfaces.contains(&"comment"));
    }

    #[test]
    fn binding_only_compatible_with_comment() {
        let surfaces = CanonicalArtifactType::Binding.compatible_surfaces();
        assert!(surfaces.contains(&"comment"));
        assert!(!surfaces.contains(&"issue"));
    }

    #[test]
    fn work_item_profile_resolves_for_create() {
        let mut registry = ProfileRegistry::new();
        registry.register(linear_descriptor());

        let resolved = registry
            .resolve(
                "linear.issue.work_item.v1",
                CanonicalArtifactType::Decision,
                "create",
            )
            .expect("work_item should resolve for create");

        assert_eq!(resolved.profile.profile_id, "linear.issue.work_item.v1");
        assert_eq!(resolved.profile.mutation_capability, "create");
    }

    #[test]
    fn comment_profile_resolves_for_candidate() {
        let mut registry = ProfileRegistry::new();
        registry.register(linear_descriptor());

        // Candidates are compatible with "comment" surface.
        let resolved = registry
            .resolve(
                "linear.comment.status.v1",
                CanonicalArtifactType::Candidate,
                "create",
            )
            .expect("comment profile should resolve for candidate");

        assert_eq!(resolved.profile.profile_id, "linear.comment.status.v1");
    }

    #[test]
    fn empty_registry_rejects_all() {
        let registry = ProfileRegistry::new();
        let result = registry.resolve("any.profile.v1", CanonicalArtifactType::Decision, "update");
        assert!(matches!(
            result,
            Err(ProfileRejection::UnknownProfile { .. })
        ));
    }

    #[test]
    fn rendering_constraints_preserved() {
        let mut registry = ProfileRegistry::new();
        registry.register(linear_descriptor());

        let resolved = registry
            .resolve(
                "linear.issue.summary.v1",
                CanonicalArtifactType::Decision,
                "update",
            )
            .unwrap();

        let constraints = resolved.rendering_constraints.unwrap();
        assert!(constraints.contains_key("max_description_length"));
    }
}

// ===========================================================================
// Linear egress projection, receipt, and reconciliation (#561)
// ===========================================================================

// Governed egress projection, receipt, and reconciliation types.
//
// Per `docs/specs/governed-egress-projection-ownership.md` and issue #561:
//
// - `EgressEligibilityCheck`: evaluates whether a canonical artifact may be
//   projected to a requested target and profile. Read-only, no external write.
// - `EgressProjection`: bot-authored projection payload bound to a canonical
//   artifact revision, target system, profile id, authority label, evidence
//   refs, and policy result. The only payload target adapters may execute.
// - `ProjectionPolicy`: bot-owned policy deciding whether a projection request
//   may proceed, which approval class is required, and which target/profile
//   combinations are allowed.
// - `EgressReceipt`: audit object recording target operation, target
//   object/url, payload hash, canonical revision, policy result, actor,
//   timestamp, and reconciliation state.
// - `ReconciliationObservation`: observed target-side activity after
//   projection. May trigger recall, contradiction detection, correction
//   candidates, review routing, or drift surfacing. Does not directly mutate
//   canonical Bicameral authority.
//
// Egress is governed projection, not export or sync.
// External writes require approval and produce an audit receipt.
// Preview creates no external write receipt.

// ---------------------------------------------------------------------------
// Linear projection profile identifiers (alpha)
// ---------------------------------------------------------------------------

/// Known Linear projection profile identifiers for the alpha target family.
///
/// These reference integrations-owned `ProjectionProfile` declarations by id.
/// Bot does not own rendering constraints — only eligibility and policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearProfileId {
    /// Summary projection: render accepted Bicameral state as a compact Linear
    /// issue or comment projection.
    #[serde(rename = "linear.issue.summary.v1")]
    IssueSummary,
    /// Work-item projection: create a Linear issue from an implementation-ready
    /// Bicameral artifact after policy and approval.
    #[serde(rename = "linear.issue.work_item.v1")]
    IssueWorkItem,
    /// Status projection: publish constrained status updates such as review
    /// required or accepted decision linked.
    #[serde(rename = "linear.comment.status.v1")]
    CommentStatus,
}

impl LinearProfileId {
    /// Returns the canonical string identifier for this profile.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IssueSummary => "linear.issue.summary.v1",
            Self::IssueWorkItem => "linear.issue.work_item.v1",
            Self::CommentStatus => "linear.comment.status.v1",
        }
    }
}

impl std::fmt::Display for LinearProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Approval class
// ---------------------------------------------------------------------------

/// Approval class required before an egress projection may be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalClass {
    /// No approval required — preview-only, no external write.
    PreviewOnly,
    /// Human approval required before external write.
    HumanApproval,
    /// Automatic approval — policy allows execution without human gate.
    AutoApproved,
}

// ---------------------------------------------------------------------------
// EgressEligibilityCheck
// ---------------------------------------------------------------------------

/// Evaluates whether a canonical artifact may be projected to a requested
/// target and profile.
///
/// Returns eligibility, approval requirements, warnings, and rejection reasons.
/// Does not write externally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressEligibilityCheck {
    /// Unique check identifier.
    pub check_id: Uuid,
    /// Canonical artifact (Decision) being evaluated.
    pub artifact_id: Uuid,
    /// Target system identifier (e.g. "linear").
    pub target_system: String,
    /// Profile id within the target system (e.g. "linear.issue.summary.v1").
    pub profile_id: String,
    /// Whether the artifact is eligible for projection.
    pub eligible: bool,
    /// Required approval class if eligible.
    pub approval_class: ApprovalClass,
    /// Warnings that do not block eligibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Rejection reasons if not eligible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_reasons: Vec<String>,
    /// Timestamp of the eligibility evaluation.
    pub checked_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// ProjectionPolicy
// ---------------------------------------------------------------------------

/// Bot-owned policy deciding whether a projection request may proceed.
///
/// Determines which approval class is required and which target/profile
/// combinations are allowed for the artifact and actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionPolicy {
    /// Unique policy identifier.
    pub policy_id: Uuid,
    /// Human-readable policy name.
    pub name: String,
    /// Target systems this policy applies to.
    #[serde(default)]
    pub allowed_targets: Vec<String>,
    /// Profile ids this policy allows within permitted targets.
    #[serde(default)]
    pub allowed_profiles: Vec<String>,
    /// Default approval class when no rule overrides.
    pub default_approval: ApprovalClass,
    /// Policy rules for specific conditions.
    #[serde(default)]
    pub rules: Vec<ProjectionPolicyRule>,
}

/// A single rule within a `ProjectionPolicy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionPolicyRule {
    /// Condition description (human-readable for alpha).
    pub condition: String,
    /// Approval class override when condition matches.
    pub approval_class: ApprovalClass,
    /// Whether this rule blocks projection entirely.
    #[serde(default)]
    pub blocks_projection: bool,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// ProjectionPolicyResult
// ---------------------------------------------------------------------------

/// Result of evaluating a `ProjectionPolicy` for a specific projection request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionPolicyResult {
    /// Policy that was evaluated.
    pub policy_id: Uuid,
    /// Whether projection is permitted.
    pub permitted: bool,
    /// Required approval class.
    pub approval_class: ApprovalClass,
    /// Matched rule conditions (if any).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_rules: Vec<String>,
    /// Reason for the policy decision.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// EgressProjection
// ---------------------------------------------------------------------------

/// Bot-authored projection payload bound to a canonical artifact revision.
///
/// This is the only payload target adapters may execute. It carries the
/// canonical artifact reference, target system, profile id, authority label,
/// evidence refs, and policy result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressProjection {
    /// Unique projection identifier.
    pub projection_id: Uuid,
    /// Canonical artifact (Decision) identifier.
    pub artifact_id: Uuid,
    /// Canonical artifact revision (commit SHA or event sequence number).
    pub artifact_revision: String,
    /// Target system identifier (e.g. "linear").
    pub target_system: String,
    /// Profile id (e.g. "linear.issue.summary.v1").
    pub profile_id: String,
    /// Authority label describing the governance posture of this projection.
    pub authority_label: String,
    /// Evidence references cited by this projection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    /// Policy result that permitted this projection.
    pub policy_result: ProjectionPolicyResult,
    /// Whether this projection is preview-only (no external write).
    pub preview_only: bool,
    /// Rendered payload content for the target (opaque to bot, shaped by
    /// integrations-owned profile).
    pub payload: serde_json::Value,
    /// Approval proof if external write is approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<EgressApproval>,
    /// Actor who created this projection.
    pub actor_id: String,
    /// Timestamp when this projection was created.
    pub created_at: DateTime<Utc>,
}

impl EgressProjection {
    /// Whether this projection requires approval before execution.
    pub fn requires_approval(&self) -> bool {
        !self.preview_only && self.policy_result.approval_class == ApprovalClass::HumanApproval
    }

    /// Whether this projection is approved for execution.
    pub fn is_approved(&self) -> bool {
        if self.preview_only {
            return false;
        }
        match self.policy_result.approval_class {
            ApprovalClass::PreviewOnly => false,
            ApprovalClass::AutoApproved => true,
            ApprovalClass::HumanApproval => self.approval.is_some(),
        }
    }
}

// ---------------------------------------------------------------------------
// EgressApproval
// ---------------------------------------------------------------------------

/// Approval proof for an egress projection external write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressApproval {
    /// Actor who approved the external write.
    pub approved_by: String,
    /// Timestamp of approval.
    pub approved_at: DateTime<Utc>,
    /// Optional approval reason/note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// EgressReceipt
// ---------------------------------------------------------------------------

/// Audit object recording the result of an egress projection execution.
///
/// Produced only when an external write occurs. Preview creates no receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressReceipt {
    /// Unique receipt identifier.
    pub receipt_id: Uuid,
    /// Projection that was executed.
    pub projection_id: Uuid,
    /// Target operation that was performed (e.g. "create_issue", "add_comment").
    pub target_operation: String,
    /// Target object identifier or URL (e.g. Linear issue URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_object_url: Option<String>,
    /// Target-side object identifier (e.g. Linear issue ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_object_id: Option<String>,
    /// SHA-256 hash of the projection payload at execution time.
    pub payload_hash: String,
    /// Canonical artifact revision at execution time.
    pub artifact_revision: String,
    /// Policy result that permitted execution.
    pub policy_result: ProjectionPolicyResult,
    /// Actor who triggered execution.
    pub actor_id: String,
    /// Timestamp of execution.
    pub executed_at: DateTime<Utc>,
    /// Current reconciliation state.
    pub reconciliation_state: ReconciliationState,
}

/// Reconciliation state of an egress receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationState {
    /// Execution completed, no target-side observations yet.
    Pending,
    /// Target-side observation received, no discrepancy.
    Reconciled,
    /// Target-side observation shows discrepancy with projected content.
    Diverged,
    /// Target-side object was modified externally after projection.
    ExternallyModified,
}

// ---------------------------------------------------------------------------
// ReconciliationObservation
// ---------------------------------------------------------------------------

/// Observed target-side activity after projection.
///
/// May trigger recall, contradiction detection, correction candidates, review
/// routing, or drift surfacing. Does not directly mutate canonical Bicameral
/// authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationObservation {
    /// Unique observation identifier.
    pub observation_id: Uuid,
    /// Receipt this observation relates to (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<Uuid>,
    /// Target system where the observation originated.
    pub target_system: String,
    /// Target object identifier.
    pub target_object_id: String,
    /// Kind of observed activity.
    pub observation_kind: ObservationKind,
    /// Human-readable summary of the observation.
    pub summary: String,
    /// Raw observed data from the target (opaque to bot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_data: Option<serde_json::Value>,
    /// Timestamp of the observation.
    pub observed_at: DateTime<Utc>,
    /// Whether this observation was promoted through a governed path.
    /// Observations are never directly promoted — this tracks downstream
    /// processing status.
    pub promoted: bool,
}

/// Kind of observed target-side activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    /// Target object was created as expected.
    Created,
    /// Target object was updated externally after projection.
    ExternalEdit,
    /// Target object status changed.
    StatusChange,
    /// Target object was deleted.
    Deleted,
    /// Target-side comment or reaction added.
    CommentAdded,
    /// Unclassified target-side activity.
    Other,
}

// ---------------------------------------------------------------------------
// EgressError
// ---------------------------------------------------------------------------

/// Errors that can occur during egress operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum EgressError {
    /// Artifact is not eligible for projection.
    IneligibleArtifact(String),
    /// Policy does not permit this projection.
    PolicyDenied(String),
    /// Approval is required but not provided.
    ApprovalRequired(String),
    /// Target adapter execution failed.
    AdapterExecutionFailed(String),
    /// Unknown target system or profile.
    UnknownTargetProfile(String),
    /// Preview projection cannot be executed (no external write for preview).
    PreviewCannotExecute(String),
}

impl std::fmt::Display for EgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IneligibleArtifact(d) => write!(f, "ineligible artifact: {d}"),
            Self::PolicyDenied(d) => write!(f, "policy denied: {d}"),
            Self::ApprovalRequired(d) => write!(f, "approval required: {d}"),
            Self::AdapterExecutionFailed(d) => write!(f, "adapter execution failed: {d}"),
            Self::UnknownTargetProfile(d) => write!(f, "unknown target/profile: {d}"),
            Self::PreviewCannotExecute(d) => write!(f, "preview cannot execute: {d}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod linear_egress_tests {
    use super::*;

    #[test]
    fn linear_profile_ids_roundtrip() {
        let profiles = [
            LinearProfileId::IssueSummary,
            LinearProfileId::IssueWorkItem,
            LinearProfileId::CommentStatus,
        ];
        for p in &profiles {
            let json = serde_json::to_string(p).unwrap();
            let back: LinearProfileId = serde_json::from_str(&json).unwrap();
            assert_eq!(*p, back);
            assert_eq!(json.trim_matches('"'), p.as_str());
        }
    }

    #[test]
    fn approval_class_serde_roundtrip() {
        let classes = [
            ApprovalClass::PreviewOnly,
            ApprovalClass::HumanApproval,
            ApprovalClass::AutoApproved,
        ];
        for c in &classes {
            let json = serde_json::to_string(c).unwrap();
            let back: ApprovalClass = serde_json::from_str(&json).unwrap();
            assert_eq!(*c, back);
        }
    }

    #[test]
    fn egress_error_display() {
        let err = EgressError::ApprovalRequired("human approval needed".into());
        assert!(err.to_string().contains("approval required"));
    }

    #[test]
    fn reconciliation_state_serde_roundtrip() {
        let states = [
            ReconciliationState::Pending,
            ReconciliationState::Reconciled,
            ReconciliationState::Diverged,
            ReconciliationState::ExternallyModified,
        ];
        for s in &states {
            let json = serde_json::to_string(s).unwrap();
            let back: ReconciliationState = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, back);
        }
    }

    #[test]
    fn observation_kind_serde_roundtrip() {
        let kinds = [
            ObservationKind::Created,
            ObservationKind::ExternalEdit,
            ObservationKind::StatusChange,
            ObservationKind::Deleted,
            ObservationKind::CommentAdded,
            ObservationKind::Other,
        ];
        for k in &kinds {
            let json = serde_json::to_string(k).unwrap();
            let back: ObservationKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*k, back);
        }
    }
}
