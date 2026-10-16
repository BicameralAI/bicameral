//! Configuration schema for `.bicameral/config.yaml`.

use bicameral_api::event_store::SubstrateKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level Bicameral workspace configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BicameralConfig {
    /// Schema version for forward compatibility.
    #[serde(default = "default_version")]
    pub version: u32,

    /// Workspace display name.
    #[serde(default)]
    pub workspace_name: Option<String>,

    /// Event-store substrate configuration.
    #[serde(default)]
    pub event_store: EventStoreConfig,

    /// Gateway configuration.
    #[serde(default)]
    pub gateway: GatewayConfig,

    /// Governance policy configuration.
    #[serde(default)]
    pub governance: GovernancePolicyConfig,

    /// Sources connected for ingest.
    #[serde(default)]
    pub sources: Vec<SourceConfig>,

    /// Trust anchors for same-domain Product Member auto-join (`product.join`).
    ///
    /// When absent, the daemon cannot verify signed join tickets and
    /// `product.join` fails closed with `unsupported_capability`. The daemon
    /// only ever verifies tickets against these anchors; it never mints them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_join: Option<ProductJoinTrustConfig>,
}

/// Daemon-local trust anchors for verifying `product.join` tickets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductJoinTrustConfig {
    /// The single trusted hosted issuer identity.
    pub issuer: String,
    /// Ticket audience policy.
    ///
    /// - `"product_scoped"` (default) or empty: expect Cloud's
    ///   `daemon:{product_id}` audience so multi-Product daemons work.
    /// - any other non-empty string: fixed audience equality check (tests /
    ///   single-Product lab configs).
    #[serde(default = "default_product_join_audience")]
    pub audience: String,
    /// Trusted Ed25519 verifying keys.
    #[serde(default)]
    pub verifying_keys: Vec<ProductJoinVerifyingKeyConfig>,
}

fn default_product_join_audience() -> String {
    "product_scoped".to_string()
}

/// A single trusted Ed25519 verifying key for `product.join`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductJoinVerifyingKeyConfig {
    /// Identifier the ticket references in its `key_id`.
    pub key_id: String,
    /// Base64 (standard, padded) raw 32-byte Ed25519 public key.
    pub public_key_b64: String,
}

impl Default for BicameralConfig {
    fn default() -> Self {
        Self {
            version: 1,
            workspace_name: None,
            event_store: EventStoreConfig::default(),
            gateway: GatewayConfig::default(),
            governance: GovernancePolicyConfig::default(),
            sources: Vec::new(),
            product_join: None,
        }
    }
}

impl BicameralConfig {
    /// Load configuration from a YAML file.
    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Write configuration to a YAML file.
    pub fn save_to_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate safety invariants before persisting.
    ///
    /// Returns a list of validation errors. Empty means valid.
    /// Per ADR-0020: non-loopback unauthenticated gateway exposure is
    /// unsafe. v0.1 has no auth, so only loopback hosts are allowed.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !is_safe_gateway_host(&self.gateway.host) {
            errors.push(format!(
                "Gateway host '{}' is not a loopback address. \
                 v0.1 has no authentication — only 127.0.0.1, ::1, \
                 and localhost are safe.",
                self.gateway.host
            ));
        }
        if self.gateway.port == 0 {
            errors.push("Gateway port must be non-zero.".to_string());
        }
        errors
    }
}

/// Returns true if the host is a loopback address safe for unauthenticated use.
pub fn is_safe_gateway_host(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host == "localhost"
}

/// Event-store substrate configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStoreConfig {
    /// Which substrate kind to use.
    #[serde(default = "default_substrate")]
    pub substrate: SubstrateKind,

    /// Path for git-backed event store (relative to workspace root).
    #[serde(default)]
    pub git_path: Option<PathBuf>,

    /// Path for drive-folder-backed event store.
    /// Compatible with Google Drive sync; explicit freshness/offline states.
    #[serde(default)]
    pub drive_folder_path: Option<PathBuf>,

    /// Whether CI/merge-gate enforcement is configured for the Git substrate.
    ///
    /// Only meaningful when `substrate == Git`. When true, the Git adapter
    /// advertises `CiMergeGateEnforcement` as an available capability. When
    /// false (default), the adapter advertises baseline Git capabilities
    /// (branch proposals, PR review, merge conflict resolution) without
    /// claiming CI blocking.
    #[serde(default)]
    pub ci_merge_gate: bool,
}

impl Default for EventStoreConfig {
    fn default() -> Self {
        Self {
            substrate: SubstrateKind::Git,
            git_path: None,
            drive_folder_path: None,
            ci_merge_gate: false,
        }
    }
}

/// Gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Host to bind the gateway to.
    #[serde(default = "default_host")]
    pub host: String,

    /// Port for the local gateway.
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

/// Governance policy settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernancePolicyConfig {
    /// Default policy: require review for all candidates.
    #[serde(default = "default_true")]
    pub require_review: bool,

    /// Automatically reject candidates below this extraction confidence.
    #[serde(default)]
    pub min_extraction_confidence: Option<String>,

    /// Roles/people who can approve candidates.
    #[serde(default)]
    pub approvers: Vec<String>,
}

impl Default for GovernancePolicyConfig {
    fn default() -> Self {
        Self {
            require_review: true,
            min_extraction_confidence: None,
            approvers: Vec::new(),
        }
    }
}

/// A connected source for ingest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub name: String,
    pub source_type: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

fn default_version() -> u32 {
    1
}
fn default_substrate() -> SubstrateKind {
    SubstrateKind::Git
}
/// Canonical local daemon port — the single source of truth for the
/// browser/MCP/dashboard <-> daemon contract.
///
/// Documented in `protocol/README.md`; consumers (MCP thin client, dashboard,
/// bicameral-cloud hosted bridge) MUST match this. The cross-artifact contract
/// test in `tests/port_contract.rs` fails if this constant and the protocol
/// doc ever diverge (the drift that caused cloud pairing to break on 19520).
pub const DEFAULT_GATEWAY_PORT: u16 = 37373;

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    DEFAULT_GATEWAY_PORT
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        let config = BicameralConfig::default();
        assert!(
            config.validate().is_empty(),
            "Default config should be valid"
        );
    }

    #[test]
    fn non_loopback_host_is_rejected() {
        let mut config = BicameralConfig::default();
        config.gateway.host = "0.0.0.0".to_string();
        let errors = config.validate();
        assert!(!errors.is_empty(), "0.0.0.0 should be rejected");
        assert!(errors[0].contains("loopback"));
    }

    #[test]
    fn loopback_hosts_are_accepted() {
        for host in ["127.0.0.1", "::1", "localhost"] {
            let mut config = BicameralConfig::default();
            config.gateway.host = host.to_string();
            assert!(
                config.validate().is_empty(),
                "{host} should be accepted as safe"
            );
        }
    }

    #[test]
    fn external_hosts_are_rejected() {
        for host in ["0.0.0.0", "192.168.1.1", "10.0.0.1", "example.com"] {
            assert!(
                !is_safe_gateway_host(host),
                "{host} should not be considered safe"
            );
        }
    }

    #[test]
    fn zero_port_is_rejected() {
        let mut config = BicameralConfig::default();
        config.gateway.port = 0;
        let errors = config.validate();
        assert!(!errors.is_empty(), "Port 0 should be rejected");
    }

    #[test]
    fn config_round_trip_via_yaml() {
        let mut config = BicameralConfig::default();
        config.workspace_name = Some("test-ws".to_string());
        config.event_store.substrate = SubstrateKind::DriveFolder;
        config.event_store.drive_folder_path = Some("/tmp/drive".into());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        config.save_to_file(&path).unwrap();

        let loaded = BicameralConfig::load_from_file(&path).unwrap();
        assert_eq!(loaded.workspace_name, Some("test-ws".to_string()));
        assert_eq!(loaded.event_store.substrate, SubstrateKind::DriveFolder);
    }
}
