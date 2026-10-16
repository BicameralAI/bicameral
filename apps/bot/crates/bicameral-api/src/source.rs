//! Source provenance types — Source, SourceSnapshot, and typed evidence pointers.
//!
//! These types implement the provenance model from ADR-0006, PR #14, and
//! MCP v0.2 spec §5 (source evidence pipeline):
//! - `Source` is the mutable external object linkage identified by URI.
//! - `SourceSnapshot` is the immutable captured view, content-addressed.
//! - `SourceEvidenceRef` has one typed pointer into one snapshot and a mandatory excerpt.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Mutable external object linkage identified by URI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub uri: String,
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Typed classification of the source surface per CONTEXT.md.
    ///
    /// When present, downstream consumers can distinguish MCP sessions,
    /// command runs, tracker issues, and other source kinds without
    /// relying on free-form `source_type` strings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<SourceKind>,
}

/// Immutable captured view of a Source, content-addressed.
///
/// Per spec §5.2 stage 2: `content_hash` is SHA-256 of the canonical snapshot
/// representation. `content` holds the full source text when retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub snapshot_addr: String,
    pub snapshot_ref: String,
    pub captured_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_method: Option<CaptureMethod>,
}

/// How a source snapshot was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    ApiPoll,
    Webhook,
    Manual,
    AgentExtract,
}

/// Typed pointer into a snapshot with a mandatory canonical excerpt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvidenceRef {
    pub id: String,
    pub source_uri: String,
    pub snapshot_addr: String,
    pub snapshot_ref: String,
    pub pointer_type: String,
    pub pointer_value: String,
    pub excerpt: String,
    pub captured_at: DateTime<Utc>,
}

/// Classification of an observed source surface.
///
/// Source kinds share governance semantics but may have different capture,
/// refresh, indexing, and grounding implementations (CONTEXT.md).
/// `EvidenceReference` carries a `source_kind` so downstream consumers can
/// distinguish text spans, tracker excerpts, agent session turns, command
/// output, PR comments, file ranges, diff hunks, and code snapshot regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Free-form text span (meeting notes, Slack messages, documents).
    Text,
    /// Tracker issue or ticket (Jira, Linear, GitHub issue).
    Tracker,
    /// MCP session turn or tool call output.
    McpSession,
    /// CLI / daemon command execution output.
    CommandRun,
    /// Pull request comment, review, or description.
    PullRequest,
    /// File content at a known path.
    File,
    /// Diff hunk between two revisions.
    Diff,
    /// Code snapshot region backed by a `GraphSnapshot`.
    CodeSnapshot,
}

/// Source freshness state — enforced at command gating, not only display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFreshness {
    Fresh,
    Stale,
    Offline,
    Unknown,
}
