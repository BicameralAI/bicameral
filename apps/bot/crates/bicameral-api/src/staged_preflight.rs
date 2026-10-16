//! Staged preflight response contract — issues #323, #393.
//!
//! The staged response wraps the existing `preflight.run` lookup result with
//! explicit stage statuses for capture, projection, lookup, enforcement, and
//! a daemon-owned session directive.
//!
//! In alpha:
//!
//! - `capture`, `projection`, and `enforcement` report `not_configured`.
//! - `lookup` reflects the actual read-only preflight lookup outcome.
//! - `session_directive` is always `continue`.
//! - Each stage carries a typed `availability` classification.
//!
//! Unsupported stages are explicit, never omitted or inferred by MCP. The
//! response never claims compliance, implementation correctness, global
//! safety, blocking authority, or global no-conflict.
//!
//! MCP, dashboard, and API surfaces must render the daemon-authored response
//! as-is rather than deriving their own applicability verdict from stage
//! statuses. The typed availability classification removes interpretation
//! ambiguity so surfaces never need to guess whether `not_configured` means
//! "unsupported", "deferred", or "readiness-limited".

use serde::{Deserialize, Serialize};

use crate::tool_request::CheckpointHint;

// ── Availability classification ───────────────────────────────────────────

/// Typed availability classification for a preflight capability.
///
/// Surfaces render this directly — they must not derive their own
/// applicability verdict. The daemon owns the classification.
///
/// - `Unsupported`: not part of alpha; will not produce results.
/// - `Deferred`: recognized capability, intentionally delayed to a future release.
/// - `ReadinessLimited`: capability exists but data/graph readiness prevents it.
/// - `Available`: capability is active and producing results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightAvailability {
    /// Capability is active and producing results for this request.
    Available,
    /// Capability is not part of the alpha release.
    Unsupported,
    /// Capability is recognized but intentionally deferred to a future release.
    Deferred,
    /// Capability exists but graph/data readiness prevents it from functioning.
    ReadinessLimited,
}

// ── Stage status enums ────────────────────────────────────────────────────

/// Status of the evidence-capture stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    /// Stage is not configured in this daemon instance.
    NotConfigured,
    /// Evidence was captured for this preflight request.
    Captured,
    /// Evidence capture was attempted but rejected.
    Rejected,
}

/// Status of the candidate-projection stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionStatus {
    /// Stage is not configured in this daemon instance.
    NotConfigured,
    /// Projection is recognized but deferred (e.g. no provider configured).
    Deferred,
    /// Projection completed and produced candidate(s).
    Completed,
    /// Projection ran but produced no candidates from the evidence.
    NoCandidate,
}

/// Status of the read-only constraint-lookup stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupStatus {
    /// Stage is not configured in this daemon instance.
    NotConfigured,
    /// Lookup completed and returned its results.
    Completed,
}

/// Status of the enforcement stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementStatus {
    /// Stage is not configured in this daemon instance.
    NotConfigured,
    /// Enforcement evaluated and allows the operation.
    Allow,
    /// Enforcement evaluated and advises caution.
    Warn,
    /// Enforcement evaluated and requests a pause for review.
    Pause,
    /// Enforcement evaluated and blocks the operation.
    Block,
}

/// Daemon-owned session directive mode.
///
/// Tells the caller how to proceed after preflight. In alpha, this is
/// always `Continue`. The daemon owns the directive — MCP must not
/// compute enforcement or override the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDirectiveMode {
    /// Proceed normally.
    Continue,
    /// Proceed with advisory warnings visible.
    Warn,
    /// Pause and surface review prompt before proceeding.
    Pause,
    /// Block the operation until review completes.
    Block,
}

// ── Stage result structs ──────────────────────────────────────────────────

/// Evidence-capture stage result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureStage {
    pub status: CaptureStatus,
    /// Typed availability — surfaces render this directly.
    pub availability: PreflightAvailability,
}

/// Candidate-projection stage result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionStage {
    pub status: ProjectionStatus,
    /// Typed availability — surfaces render this directly.
    pub availability: PreflightAvailability,
}

/// Read-only constraint-lookup stage result.
///
/// `decision_refs` lists decision/candidate IDs returned by the lookup.
/// `limitations` lists scoping or readiness warnings.
/// `checkpoint_hint` echoes the caller's inert checkpoint metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LookupStage {
    pub status: LookupStatus,
    /// Typed availability — surfaces render this directly.
    pub availability: PreflightAvailability,
    #[serde(default)]
    pub decision_refs: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
    /// Echoed checkpoint hint from the request — inert trace metadata.
    ///
    /// The daemon echoes the caller-provided hint for diagnostics and
    /// telemetry. It does not change lookup behavior, persistence,
    /// authority, guard semantics, or any other protocol aspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_hint: Option<CheckpointHint>,
}

/// Enforcement stage result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnforcementStage {
    pub status: EnforcementStatus,
    /// Typed availability — surfaces render this directly.
    pub availability: PreflightAvailability,
}

/// Session directive produced by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionDirective {
    pub mode: SessionDirectiveMode,
    /// Typed availability — surfaces render this directly.
    pub availability: PreflightAvailability,
}

// ── Top-level staged response ─────────────────────────────────────────────

/// Staged preflight response returned by `preflight.run`.
///
/// Each stage is explicit. Unsupported stages report `not_configured`
/// rather than being omitted, so MCP can distinguish "not yet implemented"
/// from "ran and produced no results."
///
/// The response never claims compliance, implementation correctness,
/// global safety, blocking authority, or global no-conflict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StagedPreflightResponse {
    pub capture: CaptureStage,
    pub projection: ProjectionStage,
    pub lookup: LookupStage,
    pub enforcement: EnforcementStage,
    pub session_directive: SessionDirective,
}

impl StagedPreflightResponse {
    /// Build the alpha-default staged response from lookup results.
    ///
    /// Capture, projection, and enforcement are `not_configured` / `unsupported`.
    /// Lookup is `completed` / `available`. Session directive is `continue`.
    /// The optional `checkpoint_hint` is echoed as inert trace metadata.
    pub fn alpha_from_lookup(
        decision_refs: Vec<String>,
        limitations: Vec<String>,
        checkpoint_hint: Option<CheckpointHint>,
    ) -> Self {
        Self {
            capture: CaptureStage {
                status: CaptureStatus::NotConfigured,
                availability: PreflightAvailability::Unsupported,
            },
            projection: ProjectionStage {
                status: ProjectionStatus::NotConfigured,
                availability: PreflightAvailability::Deferred,
            },
            lookup: LookupStage {
                status: LookupStatus::Completed,
                availability: PreflightAvailability::Available,
                decision_refs,
                limitations,
                checkpoint_hint,
            },
            enforcement: EnforcementStage {
                status: EnforcementStatus::NotConfigured,
                availability: PreflightAvailability::Unsupported,
            },
            session_directive: SessionDirective {
                mode: SessionDirectiveMode::Continue,
                availability: PreflightAvailability::Available,
            },
        }
    }

    /// Build a readiness-limited staged response.
    ///
    /// Used when lookup capability exists but graph/data readiness prevents
    /// full results. The lookup stage is `readiness_limited` rather than
    /// `available`. Stages that are unsupported in alpha remain unchanged.
    /// The optional `checkpoint_hint` is echoed as inert trace metadata.
    pub fn alpha_readiness_limited(
        decision_refs: Vec<String>,
        limitations: Vec<String>,
        checkpoint_hint: Option<CheckpointHint>,
    ) -> Self {
        Self {
            capture: CaptureStage {
                status: CaptureStatus::NotConfigured,
                availability: PreflightAvailability::Unsupported,
            },
            projection: ProjectionStage {
                status: ProjectionStatus::NotConfigured,
                availability: PreflightAvailability::Deferred,
            },
            lookup: LookupStage {
                status: LookupStatus::Completed,
                availability: PreflightAvailability::ReadinessLimited,
                decision_refs,
                limitations,
                checkpoint_hint,
            },
            enforcement: EnforcementStage {
                status: EnforcementStatus::NotConfigured,
                availability: PreflightAvailability::Unsupported,
            },
            session_directive: SessionDirective {
                mode: SessionDirectiveMode::Continue,
                availability: PreflightAvailability::Available,
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_default_roundtrip() {
        let staged = StagedPreflightResponse::alpha_from_lookup(
            vec!["dec-001".into(), "dec-002".into()],
            vec!["Source-only preflight; graph not configured.".into()],
            None,
        );

        let json = serde_json::to_string_pretty(&staged).unwrap();
        let parsed: StagedPreflightResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(staged, parsed);
    }

    #[test]
    fn alpha_stages_are_explicit() {
        let staged = StagedPreflightResponse::alpha_from_lookup(vec![], vec![], None);
        assert_eq!(staged.capture.status, CaptureStatus::NotConfigured);
        assert_eq!(staged.projection.status, ProjectionStatus::NotConfigured);
        assert_eq!(staged.lookup.status, LookupStatus::Completed);
        assert_eq!(staged.enforcement.status, EnforcementStatus::NotConfigured);
        assert_eq!(
            staged.session_directive.mode,
            SessionDirectiveMode::Continue
        );
    }

    #[test]
    fn staged_response_serializes_expected_shape() {
        let staged = StagedPreflightResponse::alpha_from_lookup(
            vec!["dec-abc".into()],
            vec!["graph not available".into()],
            None,
        );
        let v: serde_json::Value = serde_json::to_value(&staged).unwrap();

        assert_eq!(v["capture"]["status"], "not_configured");
        assert_eq!(v["capture"]["availability"], "unsupported");
        assert_eq!(v["projection"]["status"], "not_configured");
        assert_eq!(v["projection"]["availability"], "deferred");
        assert_eq!(v["lookup"]["status"], "completed");
        assert_eq!(v["lookup"]["availability"], "available");
        assert_eq!(v["lookup"]["decision_refs"][0], "dec-abc");
        assert_eq!(v["lookup"]["limitations"][0], "graph not available");
        assert_eq!(v["enforcement"]["status"], "not_configured");
        assert_eq!(v["enforcement"]["availability"], "unsupported");
        assert_eq!(v["session_directive"]["mode"], "continue");
        assert_eq!(v["session_directive"]["availability"], "available");
    }

    #[test]
    fn all_capture_statuses_roundtrip() {
        for status in [
            CaptureStatus::NotConfigured,
            CaptureStatus::Captured,
            CaptureStatus::Rejected,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: CaptureStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn all_projection_statuses_roundtrip() {
        for status in [
            ProjectionStatus::NotConfigured,
            ProjectionStatus::Deferred,
            ProjectionStatus::Completed,
            ProjectionStatus::NoCandidate,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: ProjectionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn all_enforcement_statuses_roundtrip() {
        for status in [
            EnforcementStatus::NotConfigured,
            EnforcementStatus::Allow,
            EnforcementStatus::Warn,
            EnforcementStatus::Pause,
            EnforcementStatus::Block,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: EnforcementStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn all_session_directive_modes_roundtrip() {
        for mode in [
            SessionDirectiveMode::Continue,
            SessionDirectiveMode::Warn,
            SessionDirectiveMode::Pause,
            SessionDirectiveMode::Block,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: SessionDirectiveMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, parsed);
        }
    }

    #[test]
    fn never_claims_compliance_or_blocking() {
        let staged = StagedPreflightResponse::alpha_from_lookup(vec![], vec![], None);
        let json = serde_json::to_string(&staged).unwrap();

        // The alpha response must never contain compliance, correctness,
        // safety, or blocking claims.
        let forbidden = [
            "compliant",
            "correct",
            "safe",
            "\"block\"",
            "\"pause\"",
            "\"warn\"",
        ];
        for term in forbidden {
            assert!(
                !json.contains(term),
                "alpha staged response must not contain '{term}'"
            );
        }
    }

    #[test]
    fn all_availability_variants_roundtrip() {
        for avail in [
            PreflightAvailability::Available,
            PreflightAvailability::Unsupported,
            PreflightAvailability::Deferred,
            PreflightAvailability::ReadinessLimited,
        ] {
            let json = serde_json::to_string(&avail).unwrap();
            let parsed: PreflightAvailability = serde_json::from_str(&json).unwrap();
            assert_eq!(avail, parsed);
        }
    }

    #[test]
    fn alpha_availability_classification() {
        let staged = StagedPreflightResponse::alpha_from_lookup(vec![], vec![], None);
        assert_eq!(
            staged.capture.availability,
            PreflightAvailability::Unsupported
        );
        assert_eq!(
            staged.projection.availability,
            PreflightAvailability::Deferred
        );
        assert_eq!(staged.lookup.availability, PreflightAvailability::Available);
        assert_eq!(
            staged.enforcement.availability,
            PreflightAvailability::Unsupported
        );
        assert_eq!(
            staged.session_directive.availability,
            PreflightAvailability::Available
        );
    }

    #[test]
    fn readiness_limited_response_roundtrip() {
        let staged = StagedPreflightResponse::alpha_readiness_limited(
            vec!["dec-rl-001".into()],
            vec!["graph: stale index for requested scope".into()],
            None,
        );
        let json = serde_json::to_string_pretty(&staged).unwrap();
        let parsed: StagedPreflightResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(staged, parsed);
    }

    #[test]
    fn readiness_limited_lookup_availability() {
        let staged = StagedPreflightResponse::alpha_readiness_limited(vec![], vec![], None);
        assert_eq!(
            staged.lookup.availability,
            PreflightAvailability::ReadinessLimited
        );
        assert_eq!(staged.lookup.status, LookupStatus::Completed);
        // Still never claims blocking or compliance.
        assert_eq!(
            staged.enforcement.availability,
            PreflightAvailability::Unsupported
        );
        assert_eq!(
            staged.session_directive.mode,
            SessionDirectiveMode::Continue
        );
    }

    #[test]
    fn readiness_limited_never_claims_compliance_or_blocking() {
        let staged = StagedPreflightResponse::alpha_readiness_limited(vec![], vec![], None);
        let json = serde_json::to_string(&staged).unwrap();

        let forbidden = [
            "compliant",
            "correct",
            "safe",
            "\"block\"",
            "\"pause\"",
            "\"warn\"",
        ];
        for term in forbidden {
            assert!(
                !json.contains(term),
                "readiness-limited response must not contain '{term}'"
            );
        }
    }
}
