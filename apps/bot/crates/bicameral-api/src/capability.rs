//! Alpha feature capability advertising — issue #552.
//!
//! Structured capability metadata that tells MCP (and other surfaces) which
//! alpha features the running daemon supports, which are deferred, and which
//! are unsupported. Surfaces use this to gate behavior — they must not enable
//! lookup or correction unless the daemon explicitly reports support.
//!
//! This is compatibility/readiness metadata, not governance authority.
//! Capability output does not grant approval or mutate state.

use serde::{Deserialize, Serialize};

// ── Feature support status ─────────────────────────────────────────────────

/// Support status for an alpha feature.
///
/// Surfaces render this directly to decide whether to enable a feature.
/// The daemon owns the classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureSupportStatus {
    /// Feature is active and available in this daemon version.
    Supported,
    /// Feature is recognized but intentionally deferred to a future release.
    Deferred,
    /// Feature is not recognized by this daemon version.
    Unsupported,
}

// ── Migration status ───────────────────────────────────────────────────────

/// Deprecation/migration status for a feature capability.
///
/// Tells surfaces whether the feature is stable, deprecated, or in migration.
/// Surfaces should render warnings for deprecated features and prevent new
/// usage of features in migration unless the target path is also supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStatus {
    /// Feature is current and stable for this release.
    Current,
    /// Feature is deprecated — it works but a newer path exists or will exist.
    Deprecated,
    /// Feature is in active migration — callers should check `migration_target`.
    Migrating,
}

// ── Alpha feature capability entry ─────────────────────────────────────────

/// A single alpha feature capability entry advertised by the daemon.
///
/// Each entry describes a feature's support status, the minimum daemon version
/// that introduced it, and its deprecation/migration posture. This is
/// read-only metadata for surface gating — not governance authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlphaFeatureCapability {
    /// Feature identifier (e.g. "lookup", "request_correction").
    pub feature: String,

    /// Whether the daemon supports, defers, or does not support this feature.
    pub status: FeatureSupportStatus,

    /// Minimum daemon protocol version that introduced this feature.
    /// Surfaces can use this to version-gate behavior.
    pub since_version: String,

    /// Deprecation/migration status.
    pub migration: MigrationStatus,

    /// Optional migration target feature name.
    /// When `migration` is `Migrating` or `Deprecated`, this names the
    /// replacement feature or path. `None` when not applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_target: Option<String>,

    /// Optional human-readable note for surfaces rendering capability state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ── Alpha capabilities collection ──────────────────────────────────────────

/// Collection of alpha feature capabilities advertised by the daemon.
///
/// Included in `CapabilityReport` so MCP and other surfaces can determine
/// which features to enable without guessing from command lists alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlphaCapabilities {
    /// Per-feature capability entries.
    pub features: Vec<AlphaFeatureCapability>,
}

impl AlphaCapabilities {
    /// Build the alpha-default capability set for the current daemon.
    ///
    /// - `lookup`: supported (alpha read-only constraint lookup is active).
    /// - `request_correction`: supported (approval-gated governed correction
    ///   request path per RFQ #663 / ADR-0024). It is not corpus CRUD and not
    ///   product-truth mutation.
    pub fn alpha_default() -> Self {
        Self {
            features: vec![
                AlphaFeatureCapability {
                    feature: "lookup".into(),
                    status: FeatureSupportStatus::Supported,
                    since_version: "v2".into(),
                    migration: MigrationStatus::Current,
                    migration_target: None,
                    note: None,
                },
                AlphaFeatureCapability {
                    feature: "request_correction".into(),
                    status: FeatureSupportStatus::Supported,
                    since_version: "v2".into(),
                    migration: MigrationStatus::Current,
                    migration_target: None,
                    note: None,
                },
            ],
        }
    }

    /// Build a capabilities set where both lookup and correction are unsupported.
    ///
    /// Used to simulate an older daemon version that does not advertise these
    /// features.
    pub fn unsupported() -> Self {
        Self {
            features: vec![
                AlphaFeatureCapability {
                    feature: "lookup".into(),
                    status: FeatureSupportStatus::Unsupported,
                    since_version: "v2".into(),
                    migration: MigrationStatus::Current,
                    migration_target: None,
                    note: Some("Daemon version does not support lookup.".into()),
                },
                AlphaFeatureCapability {
                    feature: "request_correction".into(),
                    status: FeatureSupportStatus::Unsupported,
                    since_version: "v2".into(),
                    migration: MigrationStatus::Current,
                    migration_target: None,
                    note: Some("Daemon version does not support request_correction.".into()),
                },
            ],
        }
    }

    /// Look up a feature's capability entry by name.
    pub fn get_feature(&self, name: &str) -> Option<&AlphaFeatureCapability> {
        self.features.iter().find(|f| f.feature == name)
    }

    /// Check whether a feature is supported.
    pub fn is_supported(&self, name: &str) -> bool {
        self.get_feature(name)
            .map(|f| f.status == FeatureSupportStatus::Supported)
            .unwrap_or(false)
    }

    /// Check whether a feature is deferred.
    pub fn is_deferred(&self, name: &str) -> bool {
        self.get_feature(name)
            .map(|f| f.status == FeatureSupportStatus::Deferred)
            .unwrap_or(false)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_default_lookup_is_supported() {
        let caps = AlphaCapabilities::alpha_default();
        assert!(caps.is_supported("lookup"));
        assert!(!caps.is_deferred("lookup"));
    }

    #[test]
    fn alpha_default_request_correction_is_supported() {
        let caps = AlphaCapabilities::alpha_default();
        assert!(caps.is_supported("request_correction"));
        assert!(!caps.is_deferred("request_correction"));
    }

    #[test]
    fn alpha_default_supported_correction_carries_no_stale_note() {
        let caps = AlphaCapabilities::alpha_default();
        let correction = caps
            .get_feature("request_correction")
            .expect("request_correction must be advertised");
        assert_eq!(correction.status, FeatureSupportStatus::Supported);
        assert!(
            correction.note.is_none(),
            "supported request_correction must not carry a stale deferred note"
        );
    }

    #[test]
    fn unsupported_reports_both_unsupported() {
        let caps = AlphaCapabilities::unsupported();
        assert!(!caps.is_supported("lookup"));
        assert!(!caps.is_supported("request_correction"));
        assert!(!caps.is_deferred("lookup"));
        assert!(!caps.is_deferred("request_correction"));
    }

    #[test]
    fn unknown_feature_is_not_supported() {
        let caps = AlphaCapabilities::alpha_default();
        assert!(!caps.is_supported("nonexistent_feature"));
        assert!(!caps.is_deferred("nonexistent_feature"));
        assert!(caps.get_feature("nonexistent_feature").is_none());
    }

    #[test]
    fn alpha_default_roundtrip() {
        let caps = AlphaCapabilities::alpha_default();
        let json = serde_json::to_string_pretty(&caps).unwrap();
        let parsed: AlphaCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, parsed);
    }

    #[test]
    fn feature_support_status_serialization() {
        for (status, expected) in [
            (FeatureSupportStatus::Supported, "\"supported\""),
            (FeatureSupportStatus::Deferred, "\"deferred\""),
            (FeatureSupportStatus::Unsupported, "\"unsupported\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            let parsed: FeatureSupportStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn migration_status_serialization() {
        for (status, expected) in [
            (MigrationStatus::Current, "\"current\""),
            (MigrationStatus::Deprecated, "\"deprecated\""),
            (MigrationStatus::Migrating, "\"migrating\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            let parsed: MigrationStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn capability_never_claims_authority() {
        let caps = AlphaCapabilities::alpha_default();
        let json = serde_json::to_string(&caps).unwrap();
        let forbidden = ["approve", "reject", "mutate", "authority", "signoff"];
        for term in forbidden {
            assert!(
                !json.contains(term),
                "capability metadata must not contain '{term}'"
            );
        }
    }

    #[test]
    fn since_version_is_protocol_version() {
        let caps = AlphaCapabilities::alpha_default();
        for f in &caps.features {
            assert_eq!(f.since_version, "v2", "all alpha features since v2");
        }
    }

    #[test]
    fn migration_target_absent_when_current() {
        let caps = AlphaCapabilities::alpha_default();
        for f in &caps.features {
            assert_eq!(f.migration, MigrationStatus::Current);
            assert!(f.migration_target.is_none());
        }
    }
}
