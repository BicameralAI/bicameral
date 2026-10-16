//! Alpha capability advertising conformance — issue #552.
//!
//! Proves that the daemon advertises per-feature alpha capabilities for lookup
//! and request_correction, that MCP can distinguish supported, deferred, and
//! unsupported states, and that version-gated advertising works correctly.
//!
//! Acceptance criteria covered:
//!   1. MCP can distinguish supported lookup from unsupported daemon versions.
//!   2. MCP can distinguish supported correction, deferred correction, and
//!      unsupported daemon versions.
//!   3. Capability output does not grant approval or mutate state.
//!   4. Tests cover supported, deferred, and unsupported reports.
//!   5. Version-gating metadata is present and consistent.
//!
//! Scope boundary: this file is verification-only. It adds no lifecycle behavior.

use bicameral_api::capability::{
    AlphaCapabilities, AlphaFeatureCapability, FeatureSupportStatus, MigrationStatus,
};
use bicameral_runtime::dispatch::{
    capability_report, DEFERRED_COMMANDS, PROTOCOL_VERSION, SUPPORTED_COMMANDS,
};

// ─── 1. Capability report includes alpha_capabilities ────────────────────────

#[test]
fn capability_report_includes_alpha_capabilities() {
    let report = capability_report();
    assert!(
        !report.alpha_capabilities.features.is_empty(),
        "capability report must include alpha_capabilities"
    );
}

#[test]
fn capability_report_advertises_lookup_supported() {
    let report = capability_report();
    let lookup = report
        .alpha_capabilities
        .get_feature("lookup")
        .expect("lookup must be advertised");
    assert_eq!(lookup.status, FeatureSupportStatus::Supported);
    assert_eq!(lookup.since_version, PROTOCOL_VERSION);
    assert_eq!(lookup.migration, MigrationStatus::Current);
}

#[test]
fn capability_report_advertises_request_correction_supported() {
    let report = capability_report();
    let correction = report
        .alpha_capabilities
        .get_feature("request_correction")
        .expect("request_correction must be advertised");
    assert_eq!(correction.status, FeatureSupportStatus::Supported);
    assert_eq!(correction.since_version, PROTOCOL_VERSION);
    assert_eq!(correction.migration, MigrationStatus::Current);
}

#[test]
fn alpha_default_request_correction_posture_matches_runtime_advertising() {
    let caps = AlphaCapabilities::alpha_default();
    assert!(
        caps.is_supported("request_correction"),
        "alpha_default must advertise request_correction supported (RFQ #663)"
    );
    assert!(
        SUPPORTED_COMMANDS.contains(&"recall.request_correction"),
        "recall.request_correction must be advertised supported"
    );
    assert!(
        !DEFERRED_COMMANDS.contains(&"recall.request_correction"),
        "recall.request_correction must not be advertised deferred"
    );
}

#[test]
fn alpha_default_lookup_posture_matches_runtime_advertising() {
    let caps = AlphaCapabilities::alpha_default();
    assert!(
        caps.is_supported("lookup"),
        "alpha_default must keep lookup supported"
    );
    assert!(
        SUPPORTED_COMMANDS.contains(&"recall.inspect_evidence"),
        "recall.inspect_evidence must be advertised supported"
    );
    assert!(
        SUPPORTED_COMMANDS.contains(&"recall.expand_scope"),
        "recall.expand_scope must be advertised supported"
    );
}

// ─── 2. MCP surface gating simulation ───────────────────────────────────────
//
// The decision logic lives in the MCP client. We model it here and verify
// the daemon report enables correct gating. This is test-local modeling,
// not product behavior.

/// Simulates an MCP surface deciding whether to enable lookup behavior.
fn mcp_should_enable_lookup(caps: &AlphaCapabilities) -> bool {
    caps.is_supported("lookup")
}

/// Simulates an MCP surface deciding whether to enable correction behavior.
fn mcp_should_enable_correction(caps: &AlphaCapabilities) -> bool {
    caps.is_supported("request_correction")
}

#[test]
fn mcp_enables_lookup_for_alpha_daemon() {
    let caps = AlphaCapabilities::alpha_default();
    assert!(
        mcp_should_enable_lookup(&caps),
        "MCP must enable lookup when daemon reports supported"
    );
}

#[test]
fn mcp_enables_correction_for_alpha_daemon() {
    let caps = AlphaCapabilities::alpha_default();
    assert!(
        mcp_should_enable_correction(&caps),
        "MCP must enable correction when daemon reports supported (RFQ #663)"
    );
}

#[test]
fn mcp_does_not_enable_lookup_for_unsupported_daemon() {
    let caps = AlphaCapabilities::unsupported();
    assert!(
        !mcp_should_enable_lookup(&caps),
        "MCP must not enable lookup when daemon reports unsupported"
    );
}

#[test]
fn mcp_does_not_enable_correction_for_unsupported_daemon() {
    let caps = AlphaCapabilities::unsupported();
    assert!(
        !mcp_should_enable_correction(&caps),
        "MCP must not enable correction when daemon reports unsupported"
    );
}

// ─── 3. Version-gated advertising ───────────────────────────────────────────

#[test]
fn all_alpha_features_have_version_since_v2() {
    let report = capability_report();
    for f in &report.alpha_capabilities.features {
        assert_eq!(
            f.since_version, "v2",
            "feature '{}' must report since_version = v2",
            f.feature
        );
    }
}

#[test]
fn version_gating_rejects_older_daemon_without_alpha_capabilities() {
    // Simulate an older daemon that doesn't advertise alpha capabilities
    // by using the unsupported constructor.
    let caps = AlphaCapabilities::unsupported();
    assert!(!mcp_should_enable_lookup(&caps));
    assert!(!mcp_should_enable_correction(&caps));
}

// ─── 4. Unsupported state handling ──────────────────────────────────────────

#[test]
fn unsupported_features_have_notes() {
    let caps = AlphaCapabilities::unsupported();
    for f in &caps.features {
        assert!(
            f.note.is_some(),
            "unsupported feature '{}' should have an explanatory note",
            f.feature
        );
    }
}

#[test]
fn supported_request_correction_has_no_stale_note() {
    let caps = AlphaCapabilities::alpha_default();
    let correction = caps
        .get_feature("request_correction")
        .expect("request_correction must exist");
    assert_eq!(correction.status, FeatureSupportStatus::Supported);
    assert!(
        correction.note.is_none(),
        "supported request_correction must not carry a stale deferred note"
    );
}

#[test]
fn supported_feature_has_no_note() {
    let caps = AlphaCapabilities::alpha_default();
    let lookup = caps.get_feature("lookup").expect("lookup must exist");
    assert!(
        lookup.note.is_none(),
        "supported feature should not need a note"
    );
}

// ─── 5. Capability output is not authority ──────────────────────────────────

#[test]
fn capability_report_alpha_capabilities_never_claim_authority() {
    let report = capability_report();
    let json = serde_json::to_string(&report.alpha_capabilities).unwrap();
    let forbidden = ["approve", "reject", "mutate", "signoff", "compliance"];
    for term in forbidden {
        assert!(
            !json.contains(term),
            "alpha_capabilities must not contain authority term '{term}'"
        );
    }
}

#[test]
fn alpha_capabilities_serialization_shape() {
    let report = capability_report();
    let v: serde_json::Value = serde_json::to_value(&report.alpha_capabilities).unwrap();

    // Must be an object with a "features" array.
    let features = v["features"].as_array().expect("features must be array");
    assert!(
        features.len() >= 2,
        "must have at least lookup and request_correction"
    );

    // Check lookup entry shape.
    let lookup = features
        .iter()
        .find(|f| f["feature"] == "lookup")
        .expect("lookup entry must exist");
    assert_eq!(lookup["status"], "supported");
    assert_eq!(lookup["since_version"], "v2");
    assert_eq!(lookup["migration"], "current");

    // Check request_correction entry shape.
    let correction = features
        .iter()
        .find(|f| f["feature"] == "request_correction")
        .expect("request_correction entry must exist");
    assert_eq!(correction["status"], "supported");
    assert_eq!(correction["since_version"], "v2");
    assert_eq!(correction["migration"], "current");
}

// ─── 6. Migration metadata ──────────────────────────────────────────────────

#[test]
fn current_features_have_no_migration_target() {
    let report = capability_report();
    for f in &report.alpha_capabilities.features {
        if f.migration == MigrationStatus::Current {
            assert!(
                f.migration_target.is_none(),
                "feature '{}' with current migration must have no migration_target",
                f.feature
            );
        }
    }
}

#[test]
fn deprecated_feature_reports_migration_target() {
    // Construct a hypothetical deprecated feature to verify the contract.
    let deprecated = AlphaFeatureCapability {
        feature: "legacy_lookup".into(),
        status: FeatureSupportStatus::Supported,
        since_version: "v2".into(),
        migration: MigrationStatus::Deprecated,
        migration_target: Some("lookup".into()),
        note: Some("Use 'lookup' instead.".into()),
    };
    assert_eq!(deprecated.migration, MigrationStatus::Deprecated);
    assert_eq!(deprecated.migration_target.as_deref(), Some("lookup"));

    // Roundtrip.
    let json = serde_json::to_string(&deprecated).unwrap();
    let parsed: AlphaFeatureCapability = serde_json::from_str(&json).unwrap();
    assert_eq!(deprecated, parsed);
}

#[test]
fn migrating_feature_reports_migration_target() {
    let migrating = AlphaFeatureCapability {
        feature: "old_correction".into(),
        status: FeatureSupportStatus::Deferred,
        since_version: "v2".into(),
        migration: MigrationStatus::Migrating,
        migration_target: Some("request_correction".into()),
        note: Some("Being replaced by request_correction.".into()),
    };
    assert_eq!(migrating.migration, MigrationStatus::Migrating);
    assert_eq!(
        migrating.migration_target.as_deref(),
        Some("request_correction")
    );

    let json = serde_json::to_string(&migrating).unwrap();
    let parsed: AlphaFeatureCapability = serde_json::from_str(&json).unwrap();
    assert_eq!(migrating, parsed);
}
