//! Projection provider readiness and typed deferred outcome conformance — issue #432.
//!
//! Terminal acceptance tests proving the alpha projection provider readiness
//! contract defined by RFQ #424:
//!
//!   1. Runtime responses distinguish successful projection from
//!      provider-unavailable / projection-deferred outcomes.
//!   2. The daemon exposes projection readiness through a typed capability
//!      path without leaking secrets.
//!   3. Missing or invalid projection configuration does not create a
//!      candidate and does not silently fall back to legacy behavior.
//!   4. Tests cover supported, missing-config, invalid-config, and
//!      unavailable-provider paths.
//!
//! All tests are deterministic — no network, no hosted providers, no secrets.

use bicameral_api::reasoning_projection::{
    AlphaProjectionOutcome, ProjectionDeferralReason, ProjectionProviderMode,
    ProjectionProviderReadiness,
};
use bicameral_runtime::dispatch::{
    capability_report, capability_report_with_readiness, CapabilityReport, DEFERRED_COMMANDS,
    PREFLIGHT_STAGE_CAPABILITIES, PROTOCOL_VERSION, SUPPORTED_COMMANDS,
};

// ─── 1. Capability report surfaces projection readiness ─────────────────────

#[test]
fn default_capability_report_shows_not_configured() {
    let report = capability_report();

    assert!(
        !report.projection_readiness.ready,
        "default report must show projection not ready"
    );
    assert!(
        report.projection_readiness.configured_mode.is_none(),
        "default report must have no configured mode"
    );
    assert!(
        report
            .projection_readiness
            .status_summary
            .to_lowercase()
            .contains("no projection provider configured"),
        "status summary must guide user to configure a provider"
    );
}

#[test]
fn capability_report_with_configured_provider_shows_ready() {
    let readiness = ProjectionProviderReadiness::available(ProjectionProviderMode::LocalCli);
    let report = capability_report_with_readiness(readiness);

    assert!(
        report.projection_readiness.ready,
        "report with configured provider must show ready"
    );
    assert_eq!(
        report.projection_readiness.configured_mode,
        Some(ProjectionProviderMode::LocalCli),
        "report must reflect the configured provider mode"
    );
}

#[test]
fn capability_report_with_unavailable_provider() {
    let readiness = ProjectionProviderReadiness::unavailable(
        ProjectionProviderMode::LocalOpenaiCompatible,
        "endpoint returned HTTP 503",
    );
    let report = capability_report_with_readiness(readiness);

    assert!(
        !report.projection_readiness.ready,
        "unavailable provider must show not ready"
    );
    assert_eq!(
        report.projection_readiness.configured_mode,
        Some(ProjectionProviderMode::LocalOpenaiCompatible),
    );
    assert!(
        report
            .projection_readiness
            .status_summary
            .contains("unavailable"),
        "status summary must mention unavailability"
    );
}

// ─── 2. No secret material in readiness ─────────────────────────────────────

#[test]
fn readiness_never_contains_secret_material() {
    let readiness_variants = vec![
        ProjectionProviderReadiness::not_configured(),
        ProjectionProviderReadiness::available(ProjectionProviderMode::LocalCli),
        ProjectionProviderReadiness::available(ProjectionProviderMode::HostedBicameral),
        ProjectionProviderReadiness::unavailable(
            ProjectionProviderMode::LocalOpenaiCompatible,
            "connection refused",
        ),
    ];

    for readiness in readiness_variants {
        let report = capability_report_with_readiness(readiness);
        let serialized = serde_json::to_string(&report).unwrap().to_lowercase();

        assert!(!serialized.contains("api_key"), "must not contain api_key");
        assert!(!serialized.contains("bearer"), "must not contain bearer");
        assert!(
            !serialized.contains("\"secret\""),
            "must not contain secret as a field"
        );
        assert!(!serialized.contains("token"), "must not contain token");
        assert!(
            !serialized.contains("password"),
            "must not contain password"
        );
    }
}

// ─── 3. Alpha projection outcome types ──────────────────────────────────────

#[test]
fn candidate_projected_outcome_indicates_candidates() {
    let outcome = AlphaProjectionOutcome::CandidateProjected { candidate_count: 3 };
    assert!(outcome.has_candidates());
    assert!(!outcome.is_deferred());

    let json = serde_json::to_value(&outcome).unwrap();
    assert_eq!(json["outcome"], "candidate_projected");
    assert_eq!(json["candidate_count"], 3);
}

#[test]
fn no_candidate_outcome_creates_nothing() {
    let outcome = AlphaProjectionOutcome::NoCandidate;
    assert!(!outcome.has_candidates());
    assert!(!outcome.is_deferred());

    let json = serde_json::to_value(&outcome).unwrap();
    assert_eq!(json["outcome"], "no_candidate");
}

#[test]
fn projection_deferred_missing_config_creates_no_candidates() {
    let outcome = AlphaProjectionOutcome::ProjectionDeferred {
        reason: ProjectionDeferralReason::ProviderConfigMissing,
    };
    assert!(!outcome.has_candidates());
    assert!(outcome.is_deferred());

    let json = serde_json::to_value(&outcome).unwrap();
    assert_eq!(json["outcome"], "projection_deferred");
    assert_eq!(json["reason"], "provider_config_missing");
}

#[test]
fn projection_deferred_invalid_config_creates_no_candidates() {
    let outcome = AlphaProjectionOutcome::ProjectionDeferred {
        reason: ProjectionDeferralReason::ProviderConfigInvalid {
            detail: "endpoint URL missing scheme".into(),
        },
    };
    assert!(!outcome.has_candidates());
    assert!(outcome.is_deferred());
}

#[test]
fn projection_deferred_unavailable_creates_no_candidates() {
    let outcome = AlphaProjectionOutcome::ProjectionDeferred {
        reason: ProjectionDeferralReason::ProviderUnavailable {
            detail: "HTTP 503 from provider endpoint".into(),
        },
    };
    assert!(!outcome.has_candidates());
    assert!(outcome.is_deferred());
}

#[test]
fn projection_deferred_timeout_creates_no_candidates() {
    let outcome = AlphaProjectionOutcome::ProjectionDeferred {
        reason: ProjectionDeferralReason::ProviderTimeout { timeout_ms: 30000 },
    };
    assert!(!outcome.has_candidates());
    assert!(outcome.is_deferred());
}

#[test]
fn projection_deferred_invalid_output_creates_no_candidates() {
    let outcome = AlphaProjectionOutcome::ProjectionDeferred {
        reason: ProjectionDeferralReason::ProviderInvalidOutput {
            detail: "malformed JSON: expected object".into(),
        },
    };
    assert!(!outcome.has_candidates());
    assert!(outcome.is_deferred());
}

#[test]
fn hard_rejected_creates_no_candidates() {
    let outcome = AlphaProjectionOutcome::HardRejected {
        reason: "sensitive data detected in evidence".into(),
    };
    assert!(!outcome.has_candidates());
    assert!(!outcome.is_deferred());
}

// ─── 4. Outcome round-trip serialization ────────────────────────────────────

#[test]
fn all_outcomes_roundtrip_through_json() {
    let outcomes = vec![
        AlphaProjectionOutcome::CandidateProjected { candidate_count: 1 },
        AlphaProjectionOutcome::NoCandidate,
        AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderConfigMissing,
        },
        AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderConfigInvalid {
                detail: "bad endpoint".into(),
            },
        },
        AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderUnavailable {
                detail: "connection refused".into(),
            },
        },
        AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderTimeout { timeout_ms: 5000 },
        },
        AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderInvalidOutput {
                detail: "missing status field".into(),
            },
        },
        AlphaProjectionOutcome::HardRejected {
            reason: "forbidden authority fields".into(),
        },
    ];

    for outcome in outcomes {
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: AlphaProjectionOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, outcome, "round-trip must preserve outcome");
    }
}

// ─── 5. Provider mode labels ────────────────────────────────────────────────

#[test]
fn provider_modes_have_human_readable_labels() {
    assert_eq!(ProjectionProviderMode::LocalCli.label(), "local CLI");
    assert_eq!(
        ProjectionProviderMode::LocalOpenaiCompatible.label(),
        "local OpenAI-compatible"
    );
    assert_eq!(
        ProjectionProviderMode::HostedBicameral.label(),
        "hosted Bicameral"
    );
}

// ─── 6. Readiness status summaries ──────────────────────────────────────────

#[test]
fn not_configured_readiness_guides_user() {
    let r = ProjectionProviderReadiness::not_configured();
    assert!(r.status_summary.contains("Configure"));
}

#[test]
fn available_readiness_confirms_mode() {
    let r = ProjectionProviderReadiness::available(ProjectionProviderMode::LocalCli);
    assert!(r.status_summary.contains("ready"));
    assert!(r.status_summary.contains("local CLI"));
}

#[test]
fn unavailable_readiness_explains_failure() {
    let r = ProjectionProviderReadiness::unavailable(
        ProjectionProviderMode::HostedBicameral,
        "auth expired",
    );
    assert!(r.status_summary.contains("unavailable"));
    assert!(r.status_summary.contains("auth expired"));
}

// ─── 7. Capability report projection readiness wire shape ───────────────────

#[test]
fn capability_report_serializes_with_projection_readiness() {
    let report = capability_report();
    let json = serde_json::to_value(&report).unwrap();

    assert!(
        json.get("projection_readiness").is_some(),
        "capability report must include projection_readiness field"
    );

    let readiness = &json["projection_readiness"];
    assert_eq!(readiness["ready"], false);
    assert!(readiness["configured_mode"].is_null());
    assert!(readiness["status_summary"].is_string());
}

#[test]
fn capability_report_with_provider_serializes_readiness() {
    let readiness = ProjectionProviderReadiness::available(ProjectionProviderMode::LocalCli);
    let report = capability_report_with_readiness(readiness);
    let json = serde_json::to_value(&report).unwrap();

    let r = &json["projection_readiness"];
    assert_eq!(r["ready"], true);
    assert_eq!(r["configured_mode"], "local_cli");
}

// ─── 8. Deferred outcome does not imply legacy fallback ─────────────────────

#[test]
fn deferred_outcome_never_implies_legacy_candidate_creation() {
    let deferred_reasons = vec![
        ProjectionDeferralReason::ProviderConfigMissing,
        ProjectionDeferralReason::ProviderConfigInvalid {
            detail: "test".into(),
        },
        ProjectionDeferralReason::ProviderUnavailable {
            detail: "test".into(),
        },
        ProjectionDeferralReason::ProviderTimeout { timeout_ms: 1000 },
        ProjectionDeferralReason::ProviderInvalidOutput {
            detail: "test".into(),
        },
    ];

    for reason in deferred_reasons {
        let outcome = AlphaProjectionOutcome::ProjectionDeferred {
            reason: reason.clone(),
        };
        assert!(
            !outcome.has_candidates(),
            "deferred outcome for {:?} must not indicate candidates",
            reason
        );
    }
}

// ─── 9. Display implementation for deferral reasons ─────────────────────────

#[test]
fn deferral_reasons_have_useful_display_messages() {
    let reasons = vec![
        (
            ProjectionDeferralReason::ProviderConfigMissing,
            "no projection provider configured",
        ),
        (
            ProjectionDeferralReason::ProviderConfigInvalid {
                detail: "bad URL".into(),
            },
            "provider configuration invalid: bad URL",
        ),
        (
            ProjectionDeferralReason::ProviderUnavailable {
                detail: "HTTP 503".into(),
            },
            "provider unavailable: HTTP 503",
        ),
        (
            ProjectionDeferralReason::ProviderTimeout { timeout_ms: 5000 },
            "provider timed out after 5000ms",
        ),
        (
            ProjectionDeferralReason::ProviderInvalidOutput {
                detail: "bad JSON".into(),
            },
            "provider returned invalid output: bad JSON",
        ),
    ];

    for (reason, expected) in reasons {
        assert_eq!(reason.to_string(), expected);
    }
}
