//! Provider acquisition protocol types — provisional alpha.
//!
//! These types define the cross-repo handoff contract for provider resource
//! discovery and item retrieval. They are **provisional alpha provider-fact
//! handoff objects**, not governance authority.
//!
//! # Authority boundary
//!
//! `ProviderResourceDescriptor` and `ProviderItemEnvelope` are purpose-neutral
//! provider facts. They are **not**:
//!
//! - `SourceBinding` — project-level approved ingestion configuration
//! - `Source` / `SourceSnapshot` / `SourceEvidence` — bot-owned evidence state
//! - `DecisionCandidate` / `Decision` — governance lifecycle objects
//! - `ToolRequest` / `ExternalIngestEnvelope` — local or external authority
//! - Review, signoff, compliance, or event-store authority
//!
//! Provider acquisition objects carry facts about what a provider offers. They
//! enter downstream governance only after explicit approval through the
//! existing `SourceBinding` promotion path.
//!
//! # Routing
//!
//! These objects must **not** be routed through `ToolRequest` or
//! `ExternalIngestEnvelope`. They occupy a separate protocol surface for
//! cross-repo provider-fact handoff.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ProviderResourceDescriptor
// ---------------------------------------------------------------------------

/// Provisional alpha descriptor for a provider-hosted resource.
///
/// Describes a single discoverable resource (repository, folder, channel,
/// document, board, etc.) that a provider exposes. Carries provider-neutral
/// metadata about the resource's identity, capabilities, and freshness —
/// nothing about governance, review, signoff, or binding approval.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderResourceDescriptor {
    /// Provider system identifier (e.g. `github`, `google_drive`, `linear`, `slack`).
    pub provider: String,

    /// Provider-scoped unique resource identifier.
    pub resource_id: String,

    /// Human-readable display name for the resource.
    pub display_name: String,

    /// Provider-assigned resource type (e.g. `repository`, `folder`, `channel`,
    /// `document`, `board`, `team`).
    pub resource_type: String,

    /// Canonical URI for the resource on the provider (e.g. HTTPS URL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Provider-neutral capabilities this resource supports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<ResourceCapability>,

    /// Current permission state for the authenticated connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionState>,

    /// Hint for why an action is needed, when `permission` is
    /// [`PermissionState::ActionNeeded`]. Advisory provider fact only; it does
    /// not grant or withhold any Bicameral authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_needed: Option<ActionNeededReason>,

    /// Parent resource reference, if this resource is nested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ResourceRef>,

    /// Freshness metadata from the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<FreshnessMetadata>,

    /// Provider-supplied metadata that does not fit the typed fields above.
    /// Must not contain secrets, tokens, credentials, or PII.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub provider_metadata: serde_json::Map<String, serde_json::Value>,

    /// When this descriptor was captured from the provider (ISO-8601).
    pub captured_at: String,
}

/// Provider-neutral capability that a resource supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceCapability {
    /// The resource can be listed/enumerated.
    List,
    /// Individual items within the resource can be read.
    Read,
    /// Items within the resource can be searched.
    Search,
    /// The provider can deliver change notifications for this resource.
    Watch,
    /// Items within the resource can be fetched incrementally.
    IncrementalFetch,
}

/// Permission state for the authenticated connection to a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    /// The connection has sufficient permission.
    Granted,
    /// Permission has not been evaluated yet.
    Unknown,
    /// Permission is insufficient; action is needed.
    ActionNeeded,
    /// Permission was explicitly denied by the provider.
    Denied,
}

/// Reason or hint for why an action is needed on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionNeededReason {
    /// The OAuth scope is missing a required permission.
    MissingScope,
    /// The provider requires re-authentication.
    ReauthRequired,
    /// An administrator needs to approve access.
    AdminApprovalRequired,
    /// The resource requires a paid plan or higher tier.
    PlanUpgradeRequired,
    /// The connection credentials have expired.
    CredentialExpired,
}

/// Lightweight reference to a parent or related resource.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceRef {
    /// Provider-scoped identifier of the referenced resource.
    pub resource_id: String,
    /// Human-readable label for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Provider-reported freshness metadata for a resource or item.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FreshnessMetadata {
    /// Provider-reported last-modified timestamp (ISO-8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Provider-reported entity tag or revision identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Provider-reported item or version count, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<u64>,
}

// ---------------------------------------------------------------------------
// ProviderItemEnvelope
// ---------------------------------------------------------------------------

/// Provisional alpha envelope for a fetched provider item.
///
/// Wraps one item retrieved from a provider resource (an issue, file, message,
/// document, commit, etc.) with provider-neutral metadata. The payload is
/// screened content — it must not contain secrets, tokens, credentials, PII,
/// or raw authentication material.
///
/// # Authority boundary
///
/// This envelope is a **provider fact**, not Bicameral evidence or a decision
/// candidate. It does not carry:
///
/// - Local actor/session authority (`AuthorityContext`)
/// - Review commands or signoff approval
/// - Event-store write intent
/// - Bot lifecycle authority (tracking state, compliance, governance verdicts)
/// - `SourceBinding` approval or `Source` materialization
///
/// The item becomes Bicameral evidence only after explicit promotion through
/// the existing ingestion and governance paths.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProviderItemEnvelope {
    /// Provider system identifier (must match the descriptor's `provider`).
    pub provider: String,

    /// The resource this item belongs to (matches the descriptor's `resource_id`).
    pub resource_id: String,

    /// Provider-scoped unique item identifier.
    pub item_id: String,

    /// Provider-assigned item type (e.g. `issue`, `file`, `message`, `document`,
    /// `commit`, `comment`).
    pub item_type: String,

    /// Screened textual content of the item. Must not contain secrets, tokens,
    /// credentials, or PII.
    pub content: String,

    /// Content hash (SHA-256) of the screened content. Downstream consumers
    /// verify if provided, compute if omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,

    /// Human-readable title or subject line, if the item has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Canonical URI for the item on the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Freshness metadata from the provider for this specific item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<FreshnessMetadata>,

    /// Provider-supplied metadata that does not fit the typed fields above.
    /// Must not contain secrets, tokens, credentials, or PII.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub provider_metadata: serde_json::Map<String, serde_json::Value>,

    /// When this item was fetched from the provider (ISO-8601).
    pub fetched_at: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_round_trip() {
        let json = serde_json::json!({
            "provider": "github",
            "resource_id": "octocat/hello-world",
            "display_name": "Hello World",
            "resource_type": "repository",
            "capabilities": ["list", "read"],
            "captured_at": "2026-01-15T10:00:00Z"
        });
        let desc: ProviderResourceDescriptor = serde_json::from_value(json).unwrap();
        assert_eq!(desc.provider, "github");
        assert_eq!(desc.resource_id, "octocat/hello-world");
        assert_eq!(desc.capabilities.len(), 2);
        let serialized = serde_json::to_value(&desc).unwrap();
        assert!(serialized.get("permission").is_none());
        assert!(serialized.get("parent").is_none());
    }

    #[test]
    fn descriptor_carries_action_needed_reason() {
        let json = serde_json::json!({
            "provider": "google_drive",
            "resource_id": "folder-xyz",
            "display_name": "Restricted Folder",
            "resource_type": "folder",
            "permission": "action_needed",
            "action_needed": "missing_scope",
            "captured_at": "2026-01-15T10:00:00Z"
        });
        let desc: ProviderResourceDescriptor = serde_json::from_value(json).unwrap();
        assert_eq!(desc.permission, Some(PermissionState::ActionNeeded));
        assert_eq!(desc.action_needed, Some(ActionNeededReason::MissingScope));
        // Absent when not provided.
        let minimal: ProviderResourceDescriptor = serde_json::from_value(serde_json::json!({
            "provider": "github",
            "resource_id": "octocat/hello-world",
            "display_name": "Hello World",
            "resource_type": "repository",
            "captured_at": "2026-01-15T10:00:00Z"
        }))
        .unwrap();
        assert_eq!(minimal.action_needed, None);
        let serialized = serde_json::to_value(&minimal).unwrap();
        assert!(serialized.get("action_needed").is_none());
    }

    #[test]
    fn item_envelope_round_trip() {
        let json = serde_json::json!({
            "provider": "github",
            "resource_id": "octocat/hello-world",
            "item_id": "issue-42",
            "item_type": "issue",
            "content": "Fix the widget alignment bug",
            "fetched_at": "2026-01-15T10:05:00Z"
        });
        let item: ProviderItemEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(item.provider, "github");
        assert_eq!(item.item_id, "issue-42");
        let serialized = serde_json::to_value(&item).unwrap();
        assert!(serialized.get("content_hash").is_none());
        assert!(serialized.get("title").is_none());
    }

    #[test]
    fn descriptor_schema_generated() {
        let schema = schemars::schema_for!(ProviderResourceDescriptor);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("ProviderResourceDescriptor"));
        assert!(json.contains("provider"));
        assert!(json.contains("resource_id"));
    }

    #[test]
    fn item_envelope_schema_generated() {
        let schema = schemars::schema_for!(ProviderItemEnvelope);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("ProviderItemEnvelope"));
        assert!(json.contains("item_id"));
        assert!(json.contains("content"));
    }

    #[test]
    fn descriptor_rejects_missing_required_fields() {
        let incomplete = serde_json::json!({
            "provider": "github"
        });
        assert!(serde_json::from_value::<ProviderResourceDescriptor>(incomplete).is_err());
    }

    #[test]
    fn item_envelope_rejects_missing_required_fields() {
        let incomplete = serde_json::json!({
            "provider": "github",
            "item_id": "issue-1"
        });
        assert!(serde_json::from_value::<ProviderItemEnvelope>(incomplete).is_err());
    }
}
