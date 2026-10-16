//! Operator-local MCP capability readiness.
//!
//! Readiness is independent of dashboard pairing and Product identity. It is
//! a daemon-owned setup projection, never canonical Product or Decision state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperHostId {
    Claude,
    Codex,
}

impl DeveloperHostId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReadinessState {
    NotInstalled,
    PrerequisiteMissing,
    InstalledPendingHostAction,
    ActiveVerified,
    RepairRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpPrerequisiteCode {
    ReleaseBindingMissing,
    UvMissing,
    PythonUnsupported,
    DeveloperHostMissing,
    DeveloperHostAuthenticationRequired,
    DaemonUnavailable,
}

impl McpPrerequisiteCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReleaseBindingMissing => "release_binding_missing",
            Self::UvMissing => "uv_missing",
            Self::PythonUnsupported => "python_unsupported",
            Self::DeveloperHostMissing => "developer_host_missing",
            Self::DeveloperHostAuthenticationRequired => "developer_host_authentication_required",
            Self::DaemonUnavailable => "daemon_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpDriftCode {
    PackageDrift,
    HostRegistrationDrift,
    ManagedHookDrift,
    DuplicateManagedEntry,
    ActivationReceiptExpired,
    DaemonHandshakeFailed,
}

impl McpDriftCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::PackageDrift => "package_drift",
            Self::HostRegistrationDrift => "host_registration_drift",
            Self::ManagedHookDrift => "managed_hook_drift",
            Self::DuplicateManagedEntry => "duplicate_managed_entry",
            Self::ActivationReceiptExpired => "activation_receipt_expired",
            Self::DaemonHandshakeFailed => "daemon_handshake_failed",
        }
    }
}

/// Observed state of the Bicameral-owned host permission grant (bot#1239).
///
/// This is a setup attribute, never an activation proof. A configured grant
/// only stops per-call host prompts for Bicameral MCP tools; authentic
/// `active_verified` still requires a fresh-session activation receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostPermissionState {
    /// The Bicameral-owned, server-scoped grant is present exactly as recorded.
    Configured,
    /// No Bicameral grant is present (consent not given, never applied, or
    /// removed by the operator).
    Missing,
    /// A Bicameral-recorded grant no longer matches the observed host
    /// configuration. Repair re-applies only with explicit consent.
    Drifted,
    /// The installed host generation exposes no server-scoped grant and no
    /// declared registry to derive a fallback from; tools stay host-prompted.
    Unsupported,
}

impl HostPermissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Missing => "missing",
            Self::Drifted => "drifted",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Which host-native mechanism carries the Bicameral grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostPermissionMechanism {
    /// One host-native grant scoped to the whole `bicameral` MCP server.
    ServerScoped,
    /// A deterministic per-tool allowlist derived from the declared registry.
    RegistryFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostPermissionObservation {
    pub state: HostPermissionState,
    pub mechanism: Option<HostPermissionMechanism>,
    /// Exact Bicameral-owned rules (Claude permission rules or Codex keys)
    /// recorded at grant time. Never includes shell, sandbox, network, or
    /// unrelated-server rules.
    #[serde(default)]
    pub owned_rules: Vec<String>,
}

/// Read-only projection of the host permission attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostPermissionProjection {
    pub state: HostPermissionState,
    pub mechanism: Option<HostPermissionMechanism>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifiedActivationEvidence {
    pub correlation_id: String,
    pub receipt_digest: String,
    pub artifact_generation: String,
    pub verified_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpReadinessObservation {
    pub subject_id: DeveloperHostId,
    pub artifact_generation: String,
    pub package_installed: bool,
    pub registration_verified: bool,
    pub managed_hook_verified: bool,
    #[serde(default)]
    pub missing_prerequisites: Vec<McpPrerequisiteCode>,
    #[serde(default)]
    pub drift_reason_codes: Vec<McpDriftCode>,
    pub activation: Option<VerifiedActivationEvidence>,
    /// Bicameral-owned host permission state (bot#1239). Absent when the
    /// setup controller predates the attribute; treated as unobserved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_permission: Option<HostPermissionObservation>,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonCapabilityReadinessV1 {
    pub schema_version: u8,
    pub capability_kind: CapabilityKind,
    pub subject_id: DeveloperHostId,
    pub artifact_generation: String,
    pub state: CapabilityReadinessState,
    pub reason_codes: Vec<String>,
    pub observed_at: String,
    pub last_verified_at: Option<String>,
    pub next_action_code: Option<String>,
    pub safe_display_argv: Option<Vec<String>>,
    /// Host permission attribute (bot#1239). It never changes `state`: a
    /// configured grant cannot manufacture `active_verified`, and a missing or
    /// drifted grant does not by itself demand repair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_permission: Option<HostPermissionProjection>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReadinessReductionError {
    #[error("artifact_generation must be a sha256 digest")]
    InvalidArtifactGeneration,
    #[error("activation evidence does not bind the observed artifact generation")]
    ActivationArtifactMismatch,
    #[error("activation evidence timestamp is invalid or expired")]
    ActivationExpired,
    #[error("observation timestamp is invalid")]
    InvalidObservationTimestamp,
    #[error("activation correlation id is not an opaque safe identifier")]
    InvalidCorrelationId,
}

impl McpReadinessObservation {
    pub fn reduce(&self) -> Result<DaemonCapabilityReadinessV1, ReadinessReductionError> {
        require_digest(&self.artifact_generation)?;
        let observed_at = chrono::DateTime::parse_from_rfc3339(&self.observed_at)
            .map_err(|_| ReadinessReductionError::InvalidObservationTimestamp)?;

        let mut reason_codes = Vec::new();
        let (state, last_verified_at) = if !self.drift_reason_codes.is_empty() {
            reason_codes.extend(
                self.drift_reason_codes
                    .iter()
                    .map(|code| code.as_str().to_string()),
            );
            (CapabilityReadinessState::RepairRequired, None)
        } else if !self.missing_prerequisites.is_empty() {
            reason_codes.extend(
                self.missing_prerequisites
                    .iter()
                    .map(|code| code.as_str().to_string()),
            );
            (CapabilityReadinessState::PrerequisiteMissing, None)
        } else if !self.package_installed {
            reason_codes.push("package_not_installed".into());
            (CapabilityReadinessState::NotInstalled, None)
        } else if self.registration_verified && self.managed_hook_verified {
            match &self.activation {
                Some(activation) => {
                    if !safe_correlation_id(&activation.correlation_id) {
                        return Err(ReadinessReductionError::InvalidCorrelationId);
                    }
                    require_digest(&activation.receipt_digest)?;
                    if activation.artifact_generation != self.artifact_generation {
                        return Err(ReadinessReductionError::ActivationArtifactMismatch);
                    }
                    let expires_at = chrono::DateTime::parse_from_rfc3339(&activation.expires_at)
                        .map_err(|_| ReadinessReductionError::ActivationExpired)?;
                    let verified_at = chrono::DateTime::parse_from_rfc3339(&activation.verified_at)
                        .map_err(|_| ReadinessReductionError::ActivationExpired)?;
                    if verified_at > observed_at || expires_at <= observed_at {
                        return Err(ReadinessReductionError::ActivationExpired);
                    }
                    (
                        CapabilityReadinessState::ActiveVerified,
                        Some(activation.verified_at.clone()),
                    )
                }
                None => {
                    reason_codes.push("activation_receipt_required".into());
                    (CapabilityReadinessState::InstalledPendingHostAction, None)
                }
            }
        } else {
            if !self.registration_verified {
                reason_codes.push("host_registration_required".into());
            }
            if !self.managed_hook_verified {
                reason_codes.push("managed_hook_required".into());
            }
            (CapabilityReadinessState::InstalledPendingHostAction, None)
        };

        // Host permission is an orthogonal attribute: it is reported with a
        // typed reason code but never moves the lifecycle state in either
        // direction (bot#1239 locked policy, items 5 and 19).
        let host_permission = self.host_permission.as_ref().map(|permission| {
            match permission.state {
                HostPermissionState::Configured => {}
                HostPermissionState::Missing => reason_codes.push("host_permission_missing".into()),
                HostPermissionState::Drifted => reason_codes.push("host_permission_drifted".into()),
                HostPermissionState::Unsupported => {
                    reason_codes.push("host_permission_unsupported".into())
                }
            }
            HostPermissionProjection {
                state: permission.state,
                mechanism: permission.mechanism,
            }
        });

        let needs_action = state != CapabilityReadinessState::ActiveVerified;
        Ok(DaemonCapabilityReadinessV1 {
            schema_version: 1,
            capability_kind: CapabilityKind::Mcp,
            subject_id: self.subject_id,
            artifact_generation: self.artifact_generation.clone(),
            state,
            reason_codes,
            observed_at: self.observed_at.clone(),
            last_verified_at,
            next_action_code: needs_action.then(|| "run_mcp_setup".into()),
            safe_display_argv: needs_action
                .then(|| vec!["bicameral".into(), "setup".into(), "mcp".into()]),
            host_permission,
        })
    }
}

fn safe_correlation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn require_digest(value: &str) -> Result<(), ReadinessReductionError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ReadinessReductionError::InvalidArtifactGeneration);
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ReadinessReductionError::InvalidArtifactGeneration);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_configuration_without_activation_is_pending_host_action() {
        let observation = McpReadinessObservation {
            subject_id: DeveloperHostId::Codex,
            artifact_generation:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            package_installed: true,
            registration_verified: true,
            managed_hook_verified: true,
            missing_prerequisites: Vec::new(),
            drift_reason_codes: Vec::new(),
            activation: None,
            host_permission: None,
            observed_at: "2026-07-30T00:00:00Z".into(),
        };

        let readiness = observation.reduce().unwrap();

        assert_eq!(
            readiness.state,
            CapabilityReadinessState::InstalledPendingHostAction
        );
        assert_eq!(
            readiness.safe_display_argv,
            Some(vec!["bicameral".into(), "setup".into(), "mcp".into()])
        );
        assert_eq!(readiness.last_verified_at, None);
    }

    #[test]
    fn exact_unexpired_activation_produces_active_verified() {
        let artifact = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let readiness =
            McpReadinessObservation {
                subject_id: DeveloperHostId::Claude,
                artifact_generation: artifact.into(),
                package_installed: true,
                registration_verified: true,
                managed_hook_verified: true,
                missing_prerequisites: Vec::new(),
                drift_reason_codes: Vec::new(),
                activation: Some(VerifiedActivationEvidence {
                    correlation_id: "session-start-1".into(),
                    receipt_digest:
                        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            .into(),
                    artifact_generation: artifact.into(),
                    verified_at: "2026-07-30T00:00:00Z".into(),
                    expires_at: "2026-07-30T01:00:00Z".into(),
                }),
                host_permission: None,
                observed_at: "2026-07-30T00:05:00Z".into(),
            }
            .reduce()
            .unwrap();

        assert_eq!(readiness.state, CapabilityReadinessState::ActiveVerified);
        assert_eq!(
            readiness.last_verified_at.as_deref(),
            Some("2026-07-30T00:00:00Z")
        );
        assert_eq!(readiness.safe_display_argv, None);
        assert_eq!(readiness.next_action_code, None);
    }

    fn artifact() -> String {
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()
    }

    fn configured_permission() -> HostPermissionObservation {
        HostPermissionObservation {
            state: HostPermissionState::Configured,
            mechanism: Some(HostPermissionMechanism::ServerScoped),
            owned_rules: vec!["mcp__bicameral".into()],
        }
    }

    #[test]
    fn configured_host_permission_cannot_manufacture_active_verified() {
        // bot#1239 item 19: permission configuration alone is never activation.
        let readiness = McpReadinessObservation {
            subject_id: DeveloperHostId::Claude,
            artifact_generation: artifact(),
            package_installed: true,
            registration_verified: true,
            managed_hook_verified: true,
            missing_prerequisites: Vec::new(),
            drift_reason_codes: Vec::new(),
            activation: None,
            host_permission: Some(configured_permission()),
            observed_at: "2026-09-10T00:00:00Z".into(),
        }
        .reduce()
        .unwrap();

        assert_eq!(
            readiness.state,
            CapabilityReadinessState::InstalledPendingHostAction
        );
        assert_eq!(readiness.reason_codes, vec!["activation_receipt_required"]);
        assert_eq!(
            readiness.host_permission,
            Some(HostPermissionProjection {
                state: HostPermissionState::Configured,
                mechanism: Some(HostPermissionMechanism::ServerScoped),
            })
        );
        assert_eq!(readiness.last_verified_at, None);
    }

    #[test]
    fn missing_or_drifted_host_permission_is_a_typed_attribute_not_a_state() {
        for (state, code) in [
            (HostPermissionState::Missing, "host_permission_missing"),
            (HostPermissionState::Drifted, "host_permission_drifted"),
            (
                HostPermissionState::Unsupported,
                "host_permission_unsupported",
            ),
        ] {
            let readiness = McpReadinessObservation {
                subject_id: DeveloperHostId::Codex,
                artifact_generation: artifact(),
                package_installed: true,
                registration_verified: true,
                managed_hook_verified: true,
                missing_prerequisites: Vec::new(),
                drift_reason_codes: Vec::new(),
                activation: Some(VerifiedActivationEvidence {
                    correlation_id: "codex-session-1".into(),
                    receipt_digest:
                        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            .into(),
                    artifact_generation: artifact(),
                    verified_at: "2026-09-10T00:00:00Z".into(),
                    expires_at: "2026-09-10T01:00:00Z".into(),
                }),
                host_permission: Some(HostPermissionObservation {
                    state,
                    mechanism: None,
                    owned_rules: Vec::new(),
                }),
                observed_at: "2026-09-10T00:05:00Z".into(),
            }
            .reduce()
            .unwrap();

            // Authentic activation still wins; the permission attribute is
            // reported alongside so a human can tell configuration from
            // activation.
            assert_eq!(readiness.state, CapabilityReadinessState::ActiveVerified);
            assert_eq!(readiness.reason_codes, vec![code.to_string()]);
            assert_eq!(readiness.host_permission.map(|p| p.state), Some(state));
        }
    }

    #[test]
    fn observation_without_host_permission_stays_backward_compatible() {
        let json = serde_json::json!({
            "subject_id": "claude",
            "artifact_generation": artifact(),
            "package_installed": false,
            "registration_verified": false,
            "managed_hook_verified": false,
            "activation": null,
            "observed_at": "2026-09-10T00:00:00Z"
        });
        let observation: McpReadinessObservation = serde_json::from_value(json).unwrap();
        assert_eq!(observation.host_permission, None);
        let readiness = observation.reduce().unwrap();
        assert_eq!(readiness.host_permission, None);
        let rendered = serde_json::to_value(&readiness).unwrap();
        assert!(rendered.get("host_permission").is_none());
    }
}
