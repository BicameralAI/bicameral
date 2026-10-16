//! Legacy/internal: invite bundle for local workspace pairing.
//!
//! **Deprecated.** This module is retained as legacy/internal compatibility code.
//! The product join mechanism is now provider-based project discovery
//! (see `docs/specs/project-setup-ui.md` and `docs/specs/oauth-and-integration-connections.md`).
//! Do not use `InviteBundle` as a product-facing join or setup surface.
//!
//! An `InviteBundle` is a checksummed JSON blob that carries safe,
//! non-secret workspace configuration. An EM generates one from their
//! configured workspace; a teammate imports it to bootstrap matching
//! local config without network access or hosted identity services.
//!
//! The bundle is *bootstrap configuration*, not shared authority.
//! Accepted team context still comes from the configured event-store
//! substrate and replay.

use crate::schema::{BicameralConfig, GovernancePolicyConfig};
use bicameral_api::event_store::SubstrateKind;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Bundle format version.
const BUNDLE_VERSION: u32 = 1;

/// Patterns that must never appear in a serialized invite bundle.
/// These are a defense-in-depth screen against accidental secret leakage.
const SECRET_CANARIES: &[&str] = &[
    "-----BEGIN",
    "PRIVATE KEY",
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "Bearer ",
    "token ",
    "password",
    "secret",
    "credential",
];

/// A checksummed, non-secret invite bundle for workspace pairing.
///
/// **Legacy/internal.** Retained for backward compatibility only.
/// The product join mechanism is now provider-based project discovery.
#[deprecated(note = "InviteBundle is legacy/internal. Use provider-based project discovery.")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InviteBundle {
    /// Bundle format version for forward compatibility.
    pub version: u32,

    /// Workspace display name.
    #[serde(default)]
    pub workspace_name: Option<String>,

    /// Event-store substrate kind.
    pub substrate_kind: SubstrateKind,

    /// Drive-folder path hint (for `drive_folder` substrate).
    #[serde(default)]
    pub drive_folder_path: Option<PathBuf>,

    /// Git event-store path hint (for `git` substrate).
    #[serde(default)]
    pub git_path: Option<PathBuf>,

    /// SHA-256 hash of the governance policy config (canonical JSON).
    #[serde(default)]
    pub policy_hash: Option<String>,

    /// References to integration manifests (safe non-secret refs only).
    #[serde(default)]
    pub integration_manifest_refs: Vec<String>,

    /// ISO 8601 timestamp when the bundle was generated.
    pub generated_at: String,

    /// SHA-256 checksum of the bundle content (computed over all other
    /// fields serialized as sorted-key canonical JSON).
    pub checksum: String,
}

/// Errors from invite bundle operations.
#[derive(Debug, thiserror::Error)]
pub enum InviteBundleError {
    #[error("invalid bundle JSON: {reason}")]
    InvalidJson { reason: String },

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("unsupported bundle version: {version}")]
    UnsupportedVersion { version: u32 },

    #[error("secret canary detected in bundle: {pattern}")]
    SecretDetected { pattern: String },

    #[error("bundle validation failed: {reason}")]
    ValidationFailed { reason: String },
}

/// Preview of config changes that importing a bundle would apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigPreview {
    /// Workspace name change (old, new).
    #[serde(default)]
    pub workspace_name: Option<(Option<String>, Option<String>)>,

    /// Substrate kind change (old, new).
    #[serde(default)]
    pub substrate_kind: Option<(String, String)>,

    /// Drive folder path change (old, new).
    #[serde(default)]
    pub drive_folder_path: Option<(Option<String>, Option<String>)>,

    /// Git path change (old, new).
    #[serde(default)]
    pub git_path: Option<(Option<String>, Option<String>)>,

    /// Whether the governance policy hash differs.
    #[serde(default)]
    pub policy_hash_changed: bool,

    /// Reminder that shared context requires the shared event substrate.
    pub pairing_notice: String,
}

#[allow(deprecated)]
impl InviteBundle {
    /// Generate an invite bundle from the current workspace configuration.
    pub fn generate(config: &BicameralConfig) -> Result<Self, InviteBundleError> {
        let policy_hash = compute_policy_hash(&config.governance);
        let source_names: Vec<String> = config.sources.iter().map(|s| s.name.clone()).collect();

        let generated_at = Utc::now().to_rfc3339();

        let mut bundle = Self {
            version: BUNDLE_VERSION,
            workspace_name: config.workspace_name.clone(),
            substrate_kind: config.event_store.substrate,
            drive_folder_path: config.event_store.drive_folder_path.clone(),
            git_path: config.event_store.git_path.clone(),
            policy_hash: Some(policy_hash),
            integration_manifest_refs: source_names,
            generated_at,
            checksum: String::new(),
        };

        bundle.checksum = bundle.compute_checksum()?;
        bundle.screen_secrets()?;

        Ok(bundle)
    }

    /// Parse and validate an invite bundle from JSON.
    pub fn from_json(json: &str) -> Result<Self, InviteBundleError> {
        // Screen the raw JSON for secret canaries before parsing
        screen_raw_json(json)?;

        let bundle: Self =
            serde_json::from_str(json).map_err(|e| InviteBundleError::InvalidJson {
                reason: e.to_string(),
            })?;

        bundle.validate()?;
        Ok(bundle)
    }

    /// Serialize the bundle to JSON.
    pub fn to_json(&self) -> Result<String, InviteBundleError> {
        let json =
            serde_json::to_string_pretty(self).map_err(|e| InviteBundleError::InvalidJson {
                reason: e.to_string(),
            })?;

        // Final screen before returning
        screen_raw_json(&json)?;
        Ok(json)
    }

    /// Validate bundle integrity: version, checksum, and secret screen.
    pub fn validate(&self) -> Result<(), InviteBundleError> {
        if self.version != BUNDLE_VERSION {
            return Err(InviteBundleError::UnsupportedVersion {
                version: self.version,
            });
        }

        let expected = self.compute_checksum()?;
        if self.checksum != expected {
            return Err(InviteBundleError::ChecksumMismatch {
                expected,
                actual: self.checksum.clone(),
            });
        }

        self.screen_secrets()?;
        Ok(())
    }

    /// Preview config changes that importing this bundle would apply.
    pub fn preview(&self, current: &BicameralConfig) -> ConfigPreview {
        let workspace_name = if self.workspace_name != current.workspace_name {
            Some((current.workspace_name.clone(), self.workspace_name.clone()))
        } else {
            None
        };

        let current_substrate = format!("{}", current.event_store.substrate);
        let bundle_substrate = format!("{}", self.substrate_kind);
        let substrate_kind = if current_substrate != bundle_substrate {
            Some((current_substrate, bundle_substrate))
        } else {
            None
        };

        let drive_folder_path = {
            let cur = current
                .event_store
                .drive_folder_path
                .as_ref()
                .map(|p| p.display().to_string());
            let new = self
                .drive_folder_path
                .as_ref()
                .map(|p| p.display().to_string());
            if cur != new {
                Some((cur, new))
            } else {
                None
            }
        };

        let git_path = {
            let cur = current
                .event_store
                .git_path
                .as_ref()
                .map(|p| p.display().to_string());
            let new = self.git_path.as_ref().map(|p| p.display().to_string());
            if cur != new {
                Some((cur, new))
            } else {
                None
            }
        };

        let current_policy_hash = compute_policy_hash(&current.governance);
        let policy_hash_changed = self
            .policy_hash
            .as_ref()
            .map(|h| h != &current_policy_hash)
            .unwrap_or(false);

        ConfigPreview {
            workspace_name,
            substrate_kind,
            drive_folder_path,
            git_path,
            policy_hash_changed,
            pairing_notice: "Shared context requires the shared event-store substrate. \
                This invite configures your local workspace to point at the same \
                substrate — it does not grant authority or synchronize decisions."
                .to_string(),
        }
    }

    /// Apply bundle config to produce a new `BicameralConfig`.
    ///
    /// Does not write to disk — the caller decides when to persist.
    pub fn apply(&self, current: &BicameralConfig) -> BicameralConfig {
        let mut config = current.clone();
        config.workspace_name = self.workspace_name.clone();
        config.event_store.substrate = self.substrate_kind;
        config.event_store.drive_folder_path = self.drive_folder_path.clone();
        config.event_store.git_path = self.git_path.clone();
        config
    }

    /// Compute SHA-256 checksum over all fields except `checksum` itself.
    fn compute_checksum(&self) -> Result<String, InviteBundleError> {
        // Build a canonical representation with sorted keys by using
        // serde_json::Value and sorting, to ensure deterministic hashing.
        let canonical = ChecksumPayload {
            version: self.version,
            workspace_name: &self.workspace_name,
            substrate_kind: &self.substrate_kind,
            drive_folder_path: &self.drive_folder_path,
            git_path: &self.git_path,
            policy_hash: &self.policy_hash,
            integration_manifest_refs: &self.integration_manifest_refs,
            generated_at: &self.generated_at,
        };

        let json =
            serde_json::to_string(&canonical).map_err(|e| InviteBundleError::InvalidJson {
                reason: format!("checksum serialization: {}", e),
            })?;

        // Canonicalize via Value to get sorted keys
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| InviteBundleError::InvalidJson {
                reason: format!("checksum canonicalization: {}", e),
            })?;
        let canonical_json = canonical_json_string(&value);

        let mut hasher = Sha256::new();
        hasher.update(canonical_json.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Screen the bundle for accidental secret content.
    fn screen_secrets(&self) -> Result<(), InviteBundleError> {
        let json = serde_json::to_string(self).map_err(|e| InviteBundleError::InvalidJson {
            reason: e.to_string(),
        })?;
        screen_raw_json(&json)
    }
}

/// Helper struct for checksum computation (all fields except `checksum`).
#[derive(Serialize)]
struct ChecksumPayload<'a> {
    version: u32,
    workspace_name: &'a Option<String>,
    substrate_kind: &'a SubstrateKind,
    drive_folder_path: &'a Option<PathBuf>,
    git_path: &'a Option<PathBuf>,
    policy_hash: &'a Option<String>,
    integration_manifest_refs: &'a Vec<String>,
    generated_at: &'a str,
}

/// Produce a canonical JSON string with sorted keys for deterministic hashing.
fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        canonical_json_string(&map[*k])
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        serde_json::Value::Array(arr) => {
            let entries: Vec<String> = arr.iter().map(canonical_json_string).collect();
            format!("[{}]", entries.join(","))
        }
        _ => serde_json::to_string(value).unwrap(),
    }
}

/// Compute SHA-256 hash of the governance policy config.
fn compute_policy_hash(policy: &GovernancePolicyConfig) -> String {
    let json = serde_json::to_string(policy).unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    let canonical = canonical_json_string(&value);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Screen raw JSON text for secret canary patterns (case-insensitive).
fn screen_raw_json(json: &str) -> Result<(), InviteBundleError> {
    let lower = json.to_lowercase();
    for pattern in SECRET_CANARIES {
        if lower.contains(&pattern.to_lowercase()) {
            return Err(InviteBundleError::SecretDetected {
                pattern: pattern.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::schema::{BicameralConfig, EventStoreConfig, SourceConfig};

    fn sample_config() -> BicameralConfig {
        BicameralConfig {
            version: 1,
            workspace_name: Some("acme-engineering".to_string()),
            event_store: EventStoreConfig {
                substrate: SubstrateKind::DriveFolder,
                git_path: None,
                drive_folder_path: Some(PathBuf::from("/Users/shared/bicameral-events")),
                ci_merge_gate: false,
            },
            gateway: Default::default(),
            governance: GovernancePolicyConfig {
                require_review: true,
                min_extraction_confidence: None,
                approvers: vec!["alice".to_string(), "bob".to_string()],
            },
            sources: vec![SourceConfig {
                name: "github-main".to_string(),
                source_type: "github".to_string(),
                config: serde_json::json!({}),
            }],
            product_join: None,
        }
    }

    #[test]
    fn generate_produces_valid_bundle() {
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();

        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.workspace_name, Some("acme-engineering".to_string()));
        assert_eq!(bundle.substrate_kind, SubstrateKind::DriveFolder);
        assert_eq!(
            bundle.drive_folder_path,
            Some(PathBuf::from("/Users/shared/bicameral-events"))
        );
        assert!(bundle.policy_hash.is_some());
        assert_eq!(bundle.integration_manifest_refs, vec!["github-main"]);
        assert!(!bundle.checksum.is_empty());
        assert!(!bundle.generated_at.is_empty());

        // Validate passes
        bundle.validate().unwrap();
    }

    #[test]
    fn roundtrip_json_preserves_bundle() {
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();
        let json = bundle.to_json().unwrap();
        let parsed = InviteBundle::from_json(&json).unwrap();
        assert_eq!(bundle, parsed);
    }

    #[test]
    fn tampered_checksum_is_rejected() {
        let config = sample_config();
        let mut bundle = InviteBundle::generate(&config).unwrap();
        bundle.checksum =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();

        let err = bundle.validate().unwrap_err();
        assert!(
            matches!(err, InviteBundleError::ChecksumMismatch { .. }),
            "Expected ChecksumMismatch, got: {err}"
        );
    }

    #[test]
    fn tampered_content_is_rejected() {
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();
        let mut json: serde_json::Value = serde_json::from_str(&bundle.to_json().unwrap()).unwrap();
        // Tamper with workspace name
        json["workspace_name"] = serde_json::json!("evil-corp");
        let tampered = serde_json::to_string_pretty(&json).unwrap();

        let err = InviteBundle::from_json(&tampered).unwrap_err();
        assert!(
            matches!(err, InviteBundleError::ChecksumMismatch { .. }),
            "Expected ChecksumMismatch, got: {err}"
        );
    }

    #[test]
    fn invalid_json_is_rejected() {
        let err = InviteBundle::from_json("not json at all").unwrap_err();
        assert!(
            matches!(err, InviteBundleError::InvalidJson { .. }),
            "Expected InvalidJson, got: {err}"
        );
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let config = sample_config();
        let mut bundle = InviteBundle::generate(&config).unwrap();
        bundle.version = 99;
        // Recompute checksum for the modified version so checksum passes
        bundle.checksum = bundle.compute_checksum().unwrap();

        let err = bundle.validate().unwrap_err();
        assert!(
            matches!(err, InviteBundleError::UnsupportedVersion { version: 99 }),
            "Expected UnsupportedVersion(99), got: {err}"
        );
    }

    #[test]
    fn no_secrets_in_serialized_bundle() {
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();
        let json = bundle.to_json().unwrap();

        // Verify none of the canary patterns appear
        let lower = json.to_lowercase();
        for pattern in SECRET_CANARIES {
            assert!(
                !lower.contains(&pattern.to_lowercase()),
                "Secret canary '{pattern}' found in bundle JSON"
            );
        }
    }

    #[test]
    fn bundle_with_injected_secret_is_rejected() {
        // Manually craft a bundle-like JSON with a secret pattern
        let json = r#"{
            "version": 1,
            "workspace_name": "ghp_1234567890abcdef",
            "substrate_kind": "git",
            "generated_at": "2025-01-01T00:00:00Z",
            "checksum": "abc"
        }"#;

        let err = InviteBundle::from_json(json).unwrap_err();
        assert!(
            matches!(err, InviteBundleError::SecretDetected { .. }),
            "Expected SecretDetected, got: {err}"
        );
    }

    #[test]
    fn preview_shows_changes() {
        let current = BicameralConfig::default();
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();

        let preview = bundle.preview(&current);
        assert!(preview.workspace_name.is_some());
        assert!(preview.substrate_kind.is_some());
        assert!(preview.drive_folder_path.is_some());
        assert!(!preview.pairing_notice.is_empty());
    }

    #[test]
    fn preview_no_changes_when_identical() {
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();
        let preview = bundle.preview(&config);

        assert!(preview.workspace_name.is_none());
        assert!(preview.substrate_kind.is_none());
        assert!(preview.drive_folder_path.is_none());
    }

    #[test]
    fn apply_produces_correct_config() {
        let current = BicameralConfig::default();
        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();

        let applied = bundle.apply(&current);
        assert_eq!(applied.workspace_name, Some("acme-engineering".to_string()));
        assert_eq!(applied.event_store.substrate, SubstrateKind::DriveFolder);
        assert_eq!(
            applied.event_store.drive_folder_path,
            Some(PathBuf::from("/Users/shared/bicameral-events"))
        );
    }

    #[test]
    fn apply_preserves_unrelated_config() {
        let mut current = BicameralConfig::default();
        current.gateway.port = 9999;
        current.governance.approvers = vec!["charlie".to_string()];

        let config = sample_config();
        let bundle = InviteBundle::generate(&config).unwrap();
        let applied = bundle.apply(&current);

        // Gateway and governance are not overwritten by invite
        assert_eq!(applied.gateway.port, 9999);
        assert_eq!(applied.governance.approvers, vec!["charlie".to_string()]);
    }

    #[test]
    fn generate_is_deterministic_for_same_timestamp() {
        let config = sample_config();
        let bundle1 = InviteBundle::generate(&config).unwrap();

        // Create a second bundle with the same generated_at
        let mut bundle2 = InviteBundle::generate(&config).unwrap();
        bundle2.generated_at = bundle1.generated_at.clone();
        bundle2.checksum = bundle2.compute_checksum().unwrap();

        assert_eq!(bundle1.checksum, bundle2.checksum);
    }

    #[test]
    fn git_substrate_bundle_roundtrip() {
        let config = BicameralConfig {
            version: 1,
            workspace_name: Some("git-team".to_string()),
            event_store: EventStoreConfig {
                substrate: SubstrateKind::Git,
                git_path: Some(PathBuf::from(".bicameral/events")),
                drive_folder_path: None,
                ci_merge_gate: true,
            },
            gateway: Default::default(),
            governance: Default::default(),
            sources: vec![],
            product_join: None,
        };

        let bundle = InviteBundle::generate(&config).unwrap();
        assert_eq!(bundle.substrate_kind, SubstrateKind::Git);
        assert_eq!(bundle.git_path, Some(PathBuf::from(".bicameral/events")));

        let json = bundle.to_json().unwrap();
        let parsed = InviteBundle::from_json(&json).unwrap();
        assert_eq!(parsed.substrate_kind, SubstrateKind::Git);
    }
}
