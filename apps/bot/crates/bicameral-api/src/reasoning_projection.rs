//! Alpha reasoning projection types — provider readiness, typed outcomes,
//! and the daemon-owned projection service.
//!
//! These types implement the alpha projection lifecycle defined in RFQ #424:
//!
//! - `candidate_projected`: projection succeeded, candidates materialized.
//! - `no_candidate`: projection ran but found no decision-like claim.
//! - `projection_deferred`: provider missing, unavailable, or timed out.
//! - `hard_rejected`: validation/policy gate rejected the request.
//!
//! Provider readiness is non-secret setup state. It may be rendered in
//! capability reports, settings, and command responses, but it must not
//! expose API keys, tokens, or secret material.
//!
//! Per the mandatory-reasoning-candidate-projection spec:
//! - `DecisionCandidate` creation requires daemon-owned projection.
//! - `candidate_hints` and `candidate_drafts` are advisory inputs only.
//! - No automatic retry, same-claim convergence, or conflict handling.

use crate::decision_likeness::{
    evaluate_candidate, evaluate_source, first_decision_like_segment, DecisionLikenessEvaluation,
    DecisionLikenessInputKind, DecisionLikenessRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Alpha projection outcome
// ---------------------------------------------------------------------------

/// Alpha projection outcome — the four states from RFQ #424.
///
/// These describe what happened when the daemon attempted candidate
/// projection over observed evidence. They are not governance authority;
/// projected candidates still require review before becoming Decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AlphaProjectionOutcome {
    /// Projection succeeded and produced one or more candidate projections.
    CandidateProjected {
        /// Number of candidates produced.
        candidate_count: usize,
    },

    /// Projection ran successfully but found no decision-like claim in the
    /// evidence. This is a valid handled response — not an error.
    NoCandidate,

    /// Projection could not run because the provider is missing, unavailable,
    /// misconfigured, or timed out. No candidate is created. Evidence may
    /// persist for explicit capture commands.
    ProjectionDeferred { reason: ProjectionDeferralReason },

    /// The request was rejected by validation or policy gates before
    /// projection could run — malformed input, forbidden fields, sensitive
    /// data detected, or policy violation.
    HardRejected { reason: String },
}

impl AlphaProjectionOutcome {
    /// Whether this outcome produced any candidates.
    pub fn has_candidates(&self) -> bool {
        matches!(
            self,
            AlphaProjectionOutcome::CandidateProjected {
                candidate_count
            } if *candidate_count > 0
        )
    }

    /// Whether the projection was deferred (provider-side issue).
    pub fn is_deferred(&self) -> bool {
        matches!(self, AlphaProjectionOutcome::ProjectionDeferred { .. })
    }

    /// Whether this outcome permits candidate materialization.
    pub fn candidates_materialized(&self) -> bool {
        matches!(self, Self::CandidateProjected { .. })
    }

    /// Returns the snake_case label for use in API responses.
    pub fn label(&self) -> &'static str {
        match self {
            Self::CandidateProjected { .. } => "candidate_projected",
            Self::NoCandidate => "no_candidate",
            Self::ProjectionDeferred { .. } => "projection_deferred",
            Self::HardRejected { .. } => "hard_rejected",
        }
    }
}

// ---------------------------------------------------------------------------
// Deferral reason
// ---------------------------------------------------------------------------

/// Why projection was deferred — typed provider-side failure reasons.
///
/// Each variant identifies a specific category of provider failure so that
/// callers and UIs can guide the user toward the correct repair action
/// (configure provider, fix credentials, retry later, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDeferralReason {
    /// No projection provider is configured for this workspace.
    ProviderConfigMissing,

    /// Provider configuration exists but is invalid or incomplete
    /// (e.g. missing required fields, bad endpoint URL format).
    ProviderConfigInvalid { detail: String },

    /// Provider is configured but currently unreachable or returned an
    /// error (network failure, HTTP 5xx, process crash, etc.).
    ProviderUnavailable { detail: String },

    /// Provider execution timed out before producing a result.
    ProviderTimeout { timeout_ms: u64 },

    /// Provider returned output that could not be parsed or validated
    /// (malformed JSON, missing required fields, forbidden authority fields).
    ProviderInvalidOutput { detail: String },
}

impl std::fmt::Display for ProjectionDeferralReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderConfigMissing => {
                write!(f, "no projection provider configured")
            }
            Self::ProviderConfigInvalid { detail } => {
                write!(f, "provider configuration invalid: {detail}")
            }
            Self::ProviderUnavailable { detail } => {
                write!(f, "provider unavailable: {detail}")
            }
            Self::ProviderTimeout { timeout_ms } => {
                write!(f, "provider timed out after {timeout_ms}ms")
            }
            Self::ProviderInvalidOutput { detail } => {
                write!(f, "provider returned invalid output: {detail}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Provider readiness
// ---------------------------------------------------------------------------

/// Projection provider readiness — non-secret setup state.
///
/// Surfaced through `CapabilityReport` and runtime responses so callers
/// can determine whether candidate projection is available before
/// attempting it. Must not contain API keys, tokens, or secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionProviderReadiness {
    /// Whether the daemon has a projection-capable provider configured.
    pub ready: bool,

    /// The configured provider mode, if any.
    pub configured_mode: Option<ProjectionProviderMode>,

    /// Human-readable status summary for UI rendering.
    pub status_summary: String,
}

impl ProjectionProviderReadiness {
    /// Build readiness for a missing/unconfigured provider.
    pub fn not_configured() -> Self {
        Self {
            ready: false,
            configured_mode: None,
            status_summary: "No projection provider configured. \
                             Configure a provider in Settings to enable candidate projection."
                .into(),
        }
    }

    /// Build readiness for a configured and available provider.
    pub fn available(mode: ProjectionProviderMode) -> Self {
        let mode_label = mode.label();
        Self {
            ready: true,
            configured_mode: Some(mode),
            status_summary: format!("Projection provider ready ({mode_label})."),
        }
    }

    /// Build readiness for a configured but unavailable provider.
    pub fn unavailable(mode: ProjectionProviderMode, reason: &str) -> Self {
        let mode_label = mode.label();
        Self {
            ready: false,
            configured_mode: Some(mode),
            status_summary: format!(
                "Projection provider configured ({mode_label}) but unavailable: {reason}."
            ),
        }
    }
}

/// Alpha projection provider modes.
///
/// `local_cli` is the alpha default. Other modes are follow-up options
/// unless explicitly configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionProviderMode {
    /// Local CLI adapter — alpha default daemon reasoning mode.
    LocalCli,
    /// Local OpenAI-compatible endpoint.
    LocalOpenaiCompatible,
    /// Hosted Bicameral reasoning service.
    HostedBicameral,
}

impl ProjectionProviderMode {
    /// Human-readable label for UI display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::LocalCli => "local CLI",
            Self::LocalOpenaiCompatible => "local OpenAI-compatible",
            Self::HostedBicameral => "hosted Bicameral",
        }
    }
}

// ---------------------------------------------------------------------------
// Reasoning mode (service-level)
// ---------------------------------------------------------------------------

/// Reasoning provider mode for daemon-owned projection service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningMode {
    /// Local CLI adapter (alpha default).
    LocalCli,
    /// Local OpenAI-compatible endpoint.
    LocalOpenaiCompatible,
    /// Hosted Bicameral reasoning service.
    HostedBicameral,
    /// Deterministic/fake provider for testing.
    Deterministic,
}

// ---------------------------------------------------------------------------
// Projection types
// ---------------------------------------------------------------------------

/// A single projected candidate from daemon reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedCandidateProposal {
    pub title: String,
    pub body: String,
    pub suggested_level: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Input to the reasoning projection service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionRequest {
    pub source_uri: String,
    pub source_type: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub snapshot_content: Option<String>,
    #[serde(default)]
    pub evidence_excerpts: Vec<String>,
    #[serde(default)]
    pub candidate_hints: Vec<CandidateHintInput>,
    #[serde(default)]
    pub candidate_drafts: Vec<CandidateDraftInput>,
}

/// Advisory candidate hint input (from external integrations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateHintInput {
    pub title: String,
    pub body: String,
    pub level: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Advisory candidate draft input (from local ingest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateDraftInput {
    pub title: String,
    pub description: String,
    pub suggested_level: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Result from the reasoning projection service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionResult {
    pub outcome: AlphaProjectionOutcome,
    pub candidates: Vec<ProjectedCandidateProposal>,
    pub reasoning_mode: ReasoningMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub decision_likeness: Vec<DecisionLikenessRecord>,
    pub projection_id: Uuid,
}

impl ProjectionResult {
    pub fn projected(candidates: Vec<ProjectedCandidateProposal>, mode: ReasoningMode) -> Self {
        let candidate_count = candidates.len();
        Self {
            outcome: AlphaProjectionOutcome::CandidateProjected { candidate_count },
            candidates,
            reasoning_mode: mode,
            detail: None,
            decision_likeness: Vec::new(),
            projection_id: Uuid::new_v4(),
        }
    }

    pub fn no_candidate(mode: ReasoningMode) -> Self {
        Self {
            outcome: AlphaProjectionOutcome::NoCandidate,
            candidates: Vec::new(),
            reasoning_mode: mode,
            detail: None,
            decision_likeness: Vec::new(),
            projection_id: Uuid::new_v4(),
        }
    }

    pub fn deferred(reason: ProjectionDeferralReason, mode: ReasoningMode) -> Self {
        let detail = reason.to_string();
        Self {
            outcome: AlphaProjectionOutcome::ProjectionDeferred { reason },
            candidates: Vec::new(),
            reasoning_mode: mode,
            detail: Some(detail),
            decision_likeness: Vec::new(),
            projection_id: Uuid::new_v4(),
        }
    }

    pub fn hard_rejected(reason: String) -> Self {
        Self {
            outcome: AlphaProjectionOutcome::HardRejected {
                reason: reason.clone(),
            },
            candidates: Vec::new(),
            reasoning_mode: ReasoningMode::Deterministic,
            detail: Some(reason),
            decision_likeness: Vec::new(),
            projection_id: Uuid::new_v4(),
        }
    }

    /// Returns the outcome label for API response serialization.
    pub fn outcome_label(&self) -> &'static str {
        self.outcome.label()
    }
}

// ---------------------------------------------------------------------------
// Projection provider trait
// ---------------------------------------------------------------------------

/// Trait for reasoning projection providers.
pub trait ProjectionProvider: Send + Sync {
    fn project(&self, request: &ProjectionRequest) -> ProjectionResult;
    fn mode(&self) -> ReasoningMode;
}

// ---------------------------------------------------------------------------
// Deterministic provider
// ---------------------------------------------------------------------------

/// Deterministic projection provider for alpha tracer bullet and tests.
///
/// Applies simple heuristic rules:
/// - If advisory inputs (hints/drafts/title+description) contain enough
///   decision-like content, projects candidates from them.
/// - If no content suggests a decision, returns `NoCandidate`.
/// - Never manufactures generic fallback candidates.
pub struct DeterministicProjectionProvider;

impl DeterministicProjectionProvider {
    fn source_texts(request: &ProjectionRequest) -> Vec<&str> {
        request
            .title
            .iter()
            .chain(request.description.iter())
            .chain(request.snapshot_content.iter())
            .map(String::as_str)
            .collect()
    }

    fn record(
        input_kind: DecisionLikenessInputKind,
        input_index: Option<usize>,
        evaluation: DecisionLikenessEvaluation,
    ) -> DecisionLikenessRecord {
        DecisionLikenessRecord {
            input_kind,
            input_index,
            evaluation,
        }
    }

    fn project_title_description(
        request: &ProjectionRequest,
        source_texts: &[&str],
    ) -> Option<(ProjectedCandidateProposal, DecisionLikenessRecord)> {
        let (Some(title), Some(description)) = (&request.title, &request.description) else {
            return None;
        };
        let evaluation = evaluate_candidate(
            title,
            description,
            source_texts,
            &request.evidence_excerpts,
            &[],
        );
        let proposal = evaluation.qualified.then(|| ProjectedCandidateProposal {
            title: title.clone(),
            body: description.clone(),
            suggested_level: None,
            tags: Vec::new(),
        });
        Some((
            proposal.unwrap_or_else(|| ProjectedCandidateProposal {
                title: String::new(),
                body: String::new(),
                suggested_level: None,
                tags: Vec::new(),
            }),
            Self::record(
                DecisionLikenessInputKind::TitleDescription,
                None,
                evaluation,
            ),
        ))
    }

    fn project_from_hints(
        request: &ProjectionRequest,
        source_texts: &[&str],
    ) -> Vec<(Option<ProjectedCandidateProposal>, DecisionLikenessRecord)> {
        request
            .candidate_hints
            .iter()
            .enumerate()
            .map(|(index, hint)| {
                let evaluation = evaluate_candidate(
                    &hint.title,
                    &hint.body,
                    source_texts,
                    &request.evidence_excerpts,
                    &hint.labels,
                );
                let proposal = evaluation.qualified.then(|| ProjectedCandidateProposal {
                    title: hint.title.clone(),
                    body: hint.body.clone(),
                    suggested_level: hint.level.clone(),
                    tags: hint.labels.clone(),
                });
                (
                    proposal,
                    Self::record(
                        DecisionLikenessInputKind::CandidateHint,
                        Some(index),
                        evaluation,
                    ),
                )
            })
            .collect()
    }

    fn project_from_drafts(
        request: &ProjectionRequest,
        source_texts: &[&str],
    ) -> Vec<(Option<ProjectedCandidateProposal>, DecisionLikenessRecord)> {
        request
            .candidate_drafts
            .iter()
            .enumerate()
            .map(|(index, draft)| {
                let evaluation = evaluate_candidate(
                    &draft.title,
                    &draft.description,
                    source_texts,
                    &request.evidence_excerpts,
                    &draft.tags,
                );
                let proposal = evaluation.qualified.then(|| ProjectedCandidateProposal {
                    title: draft.title.clone(),
                    body: draft.description.clone(),
                    suggested_level: draft.suggested_level.clone(),
                    tags: draft.tags.clone(),
                });
                (
                    proposal,
                    Self::record(
                        DecisionLikenessInputKind::CandidateDraft,
                        Some(index),
                        evaluation,
                    ),
                )
            })
            .collect()
    }

    fn project_from_content(
        request: &ProjectionRequest,
        source_texts: &[&str],
    ) -> Option<ProjectedCandidateProposal> {
        let segment = first_decision_like_segment(source_texts, &request.evidence_excerpts)?;
        Some(ProjectedCandidateProposal {
            title: concise_title(&segment),
            body: segment,
            suggested_level: None,
            tags: vec!["content_derived".to_string()],
        })
    }
}

fn concise_title(segment: &str) -> String {
    const MAX_CHARS: usize = 80;
    let normalized = segment.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_CHARS {
        return normalized;
    }
    let mut title = normalized.chars().take(MAX_CHARS - 1).collect::<String>();
    title.push('…');
    title
}

impl ProjectionProvider for DeterministicProjectionProvider {
    fn project(&self, request: &ProjectionRequest) -> ProjectionResult {
        let source_texts = Self::source_texts(request);
        let mut records = vec![Self::record(
            DecisionLikenessInputKind::SourceContent,
            None,
            evaluate_source(&source_texts, &request.evidence_excerpts),
        )];
        let mut candidates = Vec::new();

        if let Some((proposal, record)) = Self::project_title_description(request, &source_texts) {
            if record.evaluation.qualified {
                candidates.push(proposal);
            }
            records.push(record);
        }
        for (proposal, record) in Self::project_from_hints(request, &source_texts) {
            if let Some(proposal) = proposal {
                candidates.push(proposal);
            }
            records.push(record);
        }
        for (proposal, record) in Self::project_from_drafts(request, &source_texts) {
            if let Some(proposal) = proposal {
                candidates.push(proposal);
            }
            records.push(record);
        }
        if candidates.is_empty() && records[0].evaluation.qualified {
            candidates.extend(Self::project_from_content(request, &source_texts));
        }

        let qualified_count = candidates.len();
        let rejected_count = records
            .iter()
            .skip(1)
            .filter(|record| !record.evaluation.qualified)
            .count();
        let mut result = if candidates.is_empty() {
            ProjectionResult::no_candidate(self.mode())
        } else {
            ProjectionResult::projected(candidates, self.mode())
        };
        result.detail = Some(format!(
            "decision_likeness:{};qualified={qualified_count};rejected={rejected_count}",
            result.outcome_label()
        ));
        result.decision_likeness = records;
        result
    }

    fn mode(&self) -> ReasoningMode {
        ReasoningMode::Deterministic
    }
}

/// Provider that always returns `ProjectionDeferred`.
pub struct UnavailableProjectionProvider;

impl ProjectionProvider for UnavailableProjectionProvider {
    fn project(&self, _request: &ProjectionRequest) -> ProjectionResult {
        ProjectionResult::deferred(ProjectionDeferralReason::ProviderConfigMissing, self.mode())
    }

    fn mode(&self) -> ReasoningMode {
        ReasoningMode::LocalCli
    }
}

// ---------------------------------------------------------------------------
// Reasoning projection service
// ---------------------------------------------------------------------------

/// The reasoning projection service.
///
/// Routes projection requests through the configured provider and returns
/// typed outcomes. Never creates generic fallback candidates.
pub struct ReasoningProjectionService {
    provider: Box<dyn ProjectionProvider>,
}

impl ReasoningProjectionService {
    pub fn new(provider: Box<dyn ProjectionProvider>) -> Self {
        Self { provider }
    }

    pub fn deterministic() -> Self {
        Self::new(Box::new(DeterministicProjectionProvider))
    }

    pub fn unavailable() -> Self {
        Self::new(Box::new(UnavailableProjectionProvider))
    }

    pub fn project(&self, request: &ProjectionRequest) -> ProjectionResult {
        self.provider.project(request)
    }

    pub fn mode(&self) -> ReasoningMode {
        self.provider.mode()
    }
}

// ---------------------------------------------------------------------------
// Request builders
// ---------------------------------------------------------------------------

/// Build a `ProjectionRequest` from local ingest params.
pub fn projection_request_from_local_ingest(
    source_uri: &str,
    source_type: &str,
    title: &str,
    description: &str,
    snapshot_content: Option<&str>,
    evidence_excerpts: &[String],
    candidate_drafts: &[CandidateDraftInput],
) -> ProjectionRequest {
    ProjectionRequest {
        source_uri: source_uri.to_string(),
        source_type: source_type.to_string(),
        title: Some(title.to_string()),
        description: Some(description.to_string()),
        snapshot_content: snapshot_content.map(|s| s.to_string()),
        evidence_excerpts: evidence_excerpts.to_vec(),
        candidate_hints: Vec::new(),
        candidate_drafts: candidate_drafts.to_vec(),
    }
}

/// Build a `ProjectionRequest` from an external ingest envelope.
pub fn projection_request_from_external_ingest(
    source_uri: &str,
    source_system: &str,
    content: &str,
    evidence_excerpts: &[String],
    candidate_hints: &[CandidateHintInput],
) -> ProjectionRequest {
    ProjectionRequest {
        source_uri: source_uri.to_string(),
        source_type: source_system.to_string(),
        title: None,
        description: None,
        snapshot_content: Some(content.to_string()),
        evidence_excerpts: evidence_excerpts.to_vec(),
        candidate_hints: candidate_hints.to_vec(),
        candidate_drafts: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_projected_serializes() {
        let outcome = AlphaProjectionOutcome::CandidateProjected { candidate_count: 2 };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "candidate_projected");
        assert_eq!(json["candidate_count"], 2);
        assert!(outcome.has_candidates());
        assert!(!outcome.is_deferred());
    }

    #[test]
    fn no_candidate_serializes() {
        let outcome = AlphaProjectionOutcome::NoCandidate;
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "no_candidate");
        assert!(!outcome.has_candidates());
        assert!(!outcome.is_deferred());
    }

    #[test]
    fn projection_deferred_serializes() {
        let outcome = AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderConfigMissing,
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "projection_deferred");
        assert_eq!(json["reason"], "provider_config_missing");
        assert!(!outcome.has_candidates());
        assert!(outcome.is_deferred());
    }

    #[test]
    fn projection_deferred_unavailable_serializes() {
        let outcome = AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderUnavailable {
                detail: "connection refused".into(),
            },
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "projection_deferred");
        assert_eq!(
            json["reason"]["provider_unavailable"]["detail"],
            "connection refused"
        );
    }

    #[test]
    fn projection_deferred_timeout_serializes() {
        let outcome = AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderTimeout { timeout_ms: 30000 },
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "projection_deferred");
        assert_eq!(json["reason"]["provider_timeout"]["timeout_ms"], 30000);
    }

    #[test]
    fn projection_deferred_invalid_output_serializes() {
        let outcome = AlphaProjectionOutcome::ProjectionDeferred {
            reason: ProjectionDeferralReason::ProviderInvalidOutput {
                detail: "missing schema_version field".into(),
            },
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "projection_deferred");
    }

    #[test]
    fn hard_rejected_serializes() {
        let outcome = AlphaProjectionOutcome::HardRejected {
            reason: "sensitive data detected".into(),
        };
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["outcome"], "hard_rejected");
        assert_eq!(json["reason"], "sensitive data detected");
        assert!(!outcome.has_candidates());
        assert!(!outcome.is_deferred());
    }

    #[test]
    fn provider_readiness_not_configured() {
        let r = ProjectionProviderReadiness::not_configured();
        assert!(!r.ready);
        assert!(r.configured_mode.is_none());
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["ready"], false);
    }

    #[test]
    fn provider_readiness_available() {
        let r = ProjectionProviderReadiness::available(ProjectionProviderMode::LocalCli);
        assert!(r.ready);
        assert_eq!(r.configured_mode, Some(ProjectionProviderMode::LocalCli));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["ready"], true);
        assert_eq!(json["configured_mode"], "local_cli");
    }

    #[test]
    fn provider_readiness_unavailable() {
        let r = ProjectionProviderReadiness::unavailable(
            ProjectionProviderMode::LocalOpenaiCompatible,
            "endpoint unreachable",
        );
        assert!(!r.ready);
        assert_eq!(
            r.configured_mode,
            Some(ProjectionProviderMode::LocalOpenaiCompatible)
        );
    }

    #[test]
    fn deferral_reason_display() {
        assert_eq!(
            ProjectionDeferralReason::ProviderConfigMissing.to_string(),
            "no projection provider configured"
        );
        assert_eq!(
            ProjectionDeferralReason::ProviderTimeout { timeout_ms: 5000 }.to_string(),
            "provider timed out after 5000ms"
        );
    }

    #[test]
    fn provider_readiness_no_secret_material() {
        let readiness_values = vec![
            ProjectionProviderReadiness::not_configured(),
            ProjectionProviderReadiness::available(ProjectionProviderMode::LocalCli),
            ProjectionProviderReadiness::available(ProjectionProviderMode::HostedBicameral),
            ProjectionProviderReadiness::unavailable(
                ProjectionProviderMode::LocalOpenaiCompatible,
                "test failure",
            ),
        ];
        for r in readiness_values {
            let serialized = serde_json::to_string(&r).unwrap();
            let lower = serialized.to_lowercase();
            assert!(
                !lower.contains("api_key"),
                "readiness must not contain api_key"
            );
            assert!(
                !lower.contains("bearer"),
                "readiness must not contain bearer token"
            );
            assert!(
                !lower.contains("secret"),
                "readiness must not contain secret"
            );
        }
    }

    #[test]
    fn outcome_roundtrip() {
        let outcomes = vec![
            AlphaProjectionOutcome::CandidateProjected { candidate_count: 1 },
            AlphaProjectionOutcome::NoCandidate,
            AlphaProjectionOutcome::ProjectionDeferred {
                reason: ProjectionDeferralReason::ProviderConfigMissing,
            },
            AlphaProjectionOutcome::HardRejected {
                reason: "test".into(),
            },
        ];
        for outcome in outcomes {
            let json = serde_json::to_string(&outcome).unwrap();
            let parsed: AlphaProjectionOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, outcome);
        }
    }

    #[test]
    fn outcome_label_returns_snake_case() {
        assert_eq!(
            AlphaProjectionOutcome::CandidateProjected { candidate_count: 1 }.label(),
            "candidate_projected"
        );
        assert_eq!(AlphaProjectionOutcome::NoCandidate.label(), "no_candidate");
        assert_eq!(
            AlphaProjectionOutcome::ProjectionDeferred {
                reason: ProjectionDeferralReason::ProviderConfigMissing
            }
            .label(),
            "projection_deferred"
        );
        assert_eq!(
            AlphaProjectionOutcome::HardRejected { reason: "x".into() }.label(),
            "hard_rejected"
        );
    }

    #[test]
    fn deterministic_provider_projects_from_title_description() {
        let svc = ReasoningProjectionService::deterministic();
        let req = ProjectionRequest {
            source_uri: "test://uri".into(),
            source_type: "test".into(),
            title: Some("Use async runtime".into()),
            description: Some("We should adopt tokio".into()),
            snapshot_content: None,
            evidence_excerpts: vec![],
            candidate_hints: vec![],
            candidate_drafts: vec![],
        };
        let result = svc.project(&req);
        assert!(result.outcome.candidates_materialized());
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].title, "Use async runtime");
    }

    #[test]
    fn deterministic_provider_no_candidate_on_content_only() {
        let svc = ReasoningProjectionService::deterministic();
        let req = ProjectionRequest {
            source_uri: "test://uri".into(),
            source_type: "test".into(),
            title: None,
            description: None,
            snapshot_content: Some("just some content".into()),
            evidence_excerpts: vec![],
            candidate_hints: vec![],
            candidate_drafts: vec![],
        };
        let result = svc.project(&req);
        assert_eq!(result.outcome, AlphaProjectionOutcome::NoCandidate);
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn advisory_hint_without_source_support_does_not_materialize() {
        let svc = ReasoningProjectionService::deterministic();
        let req = projection_request_from_external_ingest(
            "github://repo/issues/1",
            "github",
            "Dependabot updated serde from 1.0.1 to 1.0.2. All checks passed.",
            &[],
            &[CandidateHintInput {
                title: "Adopt PostgreSQL".into(),
                body: "We decided to adopt PostgreSQL for canonical events".into(),
                level: Some("architecture".into()),
                labels: vec!["dependency_automation".into(), "status_only".into()],
            }],
        );
        let result = svc.project(&req);
        assert_eq!(result.outcome, AlphaProjectionOutcome::NoCandidate);
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn bot_authored_content_with_real_decision_materializes_without_hint() {
        let svc = ReasoningProjectionService::deterministic();
        let req = projection_request_from_external_ingest(
            "github://repo/issues/2",
            "github",
            "Automation summary. Architecture decision: the daemon must persist evidence before projection.",
            &[],
            &[],
        );
        let result = svc.project(&req);
        assert!(result.outcome.candidates_materialized());
        assert_eq!(result.candidates.len(), 1);
        assert!(result.candidates[0]
            .body
            .contains("persist evidence before projection"));
        assert!(result.candidates[0]
            .tags
            .contains(&"content_derived".to_string()));
    }

    #[test]
    fn unavailable_provider_returns_deferred() {
        let svc = ReasoningProjectionService::unavailable();
        let req = ProjectionRequest {
            source_uri: "test://uri".into(),
            source_type: "test".into(),
            title: Some("test".into()),
            description: Some("test".into()),
            snapshot_content: None,
            evidence_excerpts: vec![],
            candidate_hints: vec![],
            candidate_drafts: vec![],
        };
        let result = svc.project(&req);
        assert!(result.outcome.is_deferred());
        assert!(result.candidates.is_empty());
    }
}
