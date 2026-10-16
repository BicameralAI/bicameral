//! Provider item-envelope fixture bridge — converts provider facts to
//! bot-owned source/evidence capture state.
//!
//! # Authority boundary
//!
//! This bridge produces **Source** and **SourceSnapshot** objects (bot-owned
//! evidence capture) from a `ProviderItemEnvelope` (provider fact).
//!
//! It does **not**:
//!
//! - Create `DecisionCandidate` or `Decision` objects
//! - Emit review, signoff, or compliance commands
//! - Write to the event store
//! - Create `SourceBinding`, `ToolRequest`, or `ExternalIngestEnvelope`
//! - Make live provider calls
//!
//! The bridge uses fixtures/stubs only. It models the first canonical state
//! in the provider-to-bot lifecycle: **source/evidence capture**.

use bicameral_api::provider_acquisition::ProviderItemEnvelope;
use bicameral_api::source::{CaptureMethod, Source, SourceKind, SourceSnapshot};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// Output of the provider item-envelope bridge.
///
/// Contains the bot-owned source/evidence capture state produced from a
/// provider item envelope. This is the first canonical state in the
/// provider-to-bot lifecycle.
///
/// No `DecisionCandidate`, review command, signoff, compliance verdict,
/// or event-store mutation is produced.
#[derive(Debug, Clone)]
pub struct EnvelopeBridgeOutput {
    /// Bot-owned mutable source linkage.
    pub source: Source,

    /// Bot-owned immutable snapshot of the captured content.
    pub snapshot: SourceSnapshot,
}

/// Bridge a `ProviderItemEnvelope` to bot-owned source/evidence capture state.
///
/// This function:
/// - Derives a `Source` URI from the envelope's provider, resource, and item identity
/// - Captures a `SourceSnapshot` from the envelope's screened content
/// - Computes a content hash if one was not provided
/// - Sets `CaptureMethod::ApiPoll` (the envelope came from a provider fetch)
/// - Classifies `SourceKind` based on the envelope's `item_type`
///
/// It does NOT:
/// - Create any `DecisionCandidate`
/// - Emit review, signoff, or compliance actions
/// - Write to the event store
/// - Make live provider calls
pub fn bridge_envelope_to_source(envelope: &ProviderItemEnvelope) -> EnvelopeBridgeOutput {
    let source_uri = derive_source_uri(envelope);
    let source_kind = classify_source_kind(&envelope.item_type);

    let source = Source {
        uri: source_uri.clone(),
        source_type: envelope.provider.clone(),
        label: envelope.title.clone(),
        source_kind: Some(source_kind),
    };

    let content_hash = envelope
        .content_hash
        .clone()
        .unwrap_or_else(|| compute_content_hash(&envelope.content));

    let captured_at = parse_fetched_at(&envelope.fetched_at);

    let snapshot_addr = format!(
        "sha256:{}",
        &content_hash[..std::cmp::min(16, content_hash.len())]
    );

    let snapshot = SourceSnapshot {
        snapshot_addr,
        snapshot_ref: format!(
            "provider:{}:{}:{}",
            envelope.provider, envelope.resource_id, envelope.item_id
        ),
        captured_at,
        content: Some(envelope.content.clone()),
        content_hash: Some(content_hash),
        capture_method: Some(CaptureMethod::ApiPoll),
    };

    EnvelopeBridgeOutput { source, snapshot }
}

/// Derive a canonical source URI from envelope identity fields.
fn derive_source_uri(envelope: &ProviderItemEnvelope) -> String {
    envelope.uri.clone().unwrap_or_else(|| {
        format!(
            "provider://{}:{}/{}",
            envelope.provider, envelope.resource_id, envelope.item_id
        )
    })
}

/// Classify the `SourceKind` from the provider's item_type string.
fn classify_source_kind(item_type: &str) -> SourceKind {
    match item_type {
        "issue" | "ticket" => SourceKind::Tracker,
        "message" | "comment" => SourceKind::Text,
        "file" => SourceKind::File,
        "pull_request" | "pr" | "merge_request" => SourceKind::PullRequest,
        "diff" | "patch" => SourceKind::Diff,
        "commit" => SourceKind::CodeSnapshot,
        _ => SourceKind::Text,
    }
}

/// Compute SHA-256 content hash for screened content.
fn compute_content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Parse ISO-8601 fetched_at timestamp, falling back to now.
fn parse_fetched_at(fetched_at: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(fetched_at)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::fixtures::{
        provider_item_envelope_github_issue, provider_item_envelope_minimal,
        provider_item_envelope_slack_message,
    };

    #[test]
    fn bridge_github_issue_produces_source_and_snapshot() {
        let envelope = provider_item_envelope_github_issue();
        let output = bridge_envelope_to_source(&envelope);

        assert_eq!(
            output.source.uri,
            "https://github.com/acme/checkout/issues/123"
        );
        assert_eq!(output.source.source_type, "github");
        assert_eq!(output.source.source_kind, Some(SourceKind::Tracker));
        assert_eq!(
            output.source.label,
            Some("Rate limiting for checkout".into())
        );

        assert!(output.snapshot.content.is_some());
        assert!(output.snapshot.content_hash.is_some());
        assert_eq!(output.snapshot.capture_method, Some(CaptureMethod::ApiPoll));
    }

    #[test]
    fn bridge_slack_message_classifies_as_text() {
        let envelope = provider_item_envelope_slack_message();
        let output = bridge_envelope_to_source(&envelope);

        assert_eq!(output.source.source_kind, Some(SourceKind::Text));
        assert_eq!(output.source.source_type, "slack");
    }

    #[test]
    fn bridge_minimal_derives_synthetic_uri() {
        let envelope = provider_item_envelope_minimal();
        let output = bridge_envelope_to_source(&envelope);

        assert_eq!(output.source.uri, "provider://linear:team-proj/LIN-42");
    }

    #[test]
    fn bridge_computes_content_hash_when_absent() {
        let envelope = provider_item_envelope_slack_message();
        assert!(envelope.content_hash.is_none());

        let output = bridge_envelope_to_source(&envelope);
        assert!(output.snapshot.content_hash.is_some());
        assert!(!output.snapshot.content_hash.as_ref().unwrap().is_empty());
    }

    #[test]
    fn bridge_preserves_provided_content_hash() {
        let envelope = provider_item_envelope_github_issue();
        let provided_hash = envelope.content_hash.clone().unwrap();

        let output = bridge_envelope_to_source(&envelope);
        assert_eq!(
            output.snapshot.content_hash.as_ref().unwrap(),
            &provided_hash
        );
    }
}
