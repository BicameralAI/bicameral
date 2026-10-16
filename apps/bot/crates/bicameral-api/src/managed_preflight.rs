//! Managed post-Planning preflight boundary and exact-spec resolution result.
//!
//! Binding types carry source identity only. Analysis types are advisory: they
//! never create candidates, append Product events, or grant canonical authority.
//! A caller-supplied ClaimTrace is untrusted input for the deterministic kernel;
//! production code must not load success fixtures from disk or env.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Coding host that emitted a managed post-Planning boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedHostKind {
    ClaudeCode,
    Codex,
}

/// Exact, content-addressed governing specification selected by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactGoverningSpecBindingV1 {
    pub repository_id: String,
    pub git_head: String,
    pub artifact_path: String,
    pub blob_id: String,
    pub artifact_content_digest: String,
}

/// Host-authored boundary emitted after one successful Planning turn.
///
/// Optional `claim_trace` is **caller-injected** untrusted ClaimSemanticTraceV1
/// JSON (tests or a future provider adapter). Absence means the reasoning
/// provider is unconfigured — typed unavailable, never fixture-selected success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPlanningBoundaryV1 {
    pub product_id: String,
    pub host_kind: ManagedHostKind,
    pub host_session_id: String,
    pub host_turn_id: String,
    pub plan_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governing_spec: Option<ExactGoverningSpecBindingV1>,
    /// Untrusted ClaimSemanticTraceV1 JSON when a provider (or test) supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_trace: Option<Value>,
    /// Advisory analysis budget in milliseconds. Default 60_000. Cancellation
    /// must not be translated into "no conflict" reassurance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_budget_ms: Option<u64>,
}

/// Outcome of resolving the managed Planning boundary before semantic work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedPreflightOutcome {
    BindingValidated,
    NoCandidate,
}

/// Status of the advisory semantic-analysis step after exact-spec binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedAnalysisStatusV1 {
    /// Binding failed; analysis was not attempted.
    NotAttempted,
    /// Binding succeeded; no ClaimTrace was supplied (provider unconfigured).
    ProviderUnavailable,
    /// ClaimTrace was supplied but failed closed under validation/kernel.
    InvalidTrace,
    /// Analysis canceled at the advisory budget.
    TimedOut,
    /// Deterministic kernel accepted the trace and produced advisory classes.
    Completed,
}

/// Distinct advisory classes preserved into RecallPacket mapping.
///
/// This path never emits Product-accepted authority. `accepted` is reserved and
/// unused so callers cannot launder analysis into approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedSemanticClassV1 {
    Proposed,
    Contradiction,
    Evidence,
    SearchedScope,
    Unknown,
    Timeout,
    NoCandidate,
}

/// Typed reason that managed semantic work did not produce a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedPreflightLimitationCode {
    MissingGoverningSpec,
    WorkspaceUnavailable,
    InvalidBoundary,
    InvalidGitHead,
    InvalidArtifactPath,
    InvalidBlobId,
    InvalidContentDigest,
    RepositoryMismatch,
    RevisionUnavailable,
    ArtifactUnavailable,
    BlobMismatch,
    ContentDigestMismatch,
    ClaimTraceProviderUnavailable,
    ClaimTraceInvalid,
    ClaimTraceIncomplete,
    AnalysisTimedOut,
    KernelUnavailable,
}

/// Public limitation returned without strengthening failure into reassurance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPreflightLimitationV1 {
    pub code: ManagedPreflightLimitationCode,
    pub detail: String,
}

/// Advisory deterministic analysis attachment (not Product authority).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedSemanticAnalysisV1 {
    pub status: ManagedAnalysisStatusV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<ManagedSemanticClassV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lowering_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<ManagedPreflightLimitationV1>,
}

/// Daemon-authored exact-spec resolution result.
///
/// `binding_validated` means only that exact retained bytes matched the
/// supplied identity. It does not mean live ClaimTrace extraction succeeded,
/// and it does not imply Product acceptance or that a promotable candidate exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPreflightResultV1 {
    pub outcome: ManagedPreflightOutcome,
    pub boundary: ManagedPlanningBoundaryV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governing_spec: Option<ExactGoverningSpecBindingV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governing_spec_binding_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<ManagedPreflightLimitationV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<ManagedSemanticAnalysisV1>,
}
