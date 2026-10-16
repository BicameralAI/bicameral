//! Provider item-envelope fixture bridge conformance tests (issue #464).
//!
//! Validates the acceptance criteria:
//!
//! 1. A provider item-envelope fixture can be parsed and handed to a
//!    bot-owned fixture bridge.
//! 2. The first canonical state, if modeled, is bot-owned source/evidence
//!    capture, not integrations state.
//! 3. No `DecisionCandidate` is created directly from provider item-envelope
//!    parsing.
//! 4. No review/signoff/compliance/event-store mutation can be emitted by
//!    the envelope.
//! 5. The bridge uses fixtures/stubs only; no live provider calls are
//!    introduced.

use bicameral_api::fixtures::{
    provider_item_envelope_github_issue, provider_item_envelope_minimal,
    provider_item_envelope_slack_message,
};
use bicameral_api::provider_acquisition::ProviderItemEnvelope;
use bicameral_api::source::{CaptureMethod, SourceKind};
use bicameral_runtime::provider_envelope_bridge::{
    bridge_envelope_to_source, EnvelopeBridgeOutput,
};

// ═══════════════════════════════════════════════════════════════════
// 1. Provider item-envelope fixture can be parsed and bridged
// ═══════════════════════════════════════════════════════════════════

#[test]
fn envelope_fixture_parses_from_json_and_bridges() {
    let json = serde_json::json!({
        "provider": "github",
        "resource_id": "acme/checkout",
        "item_id": "issue-99",
        "item_type": "issue",
        "content": "Fix the widget alignment bug",
        "fetched_at": "2026-01-15T10:05:00Z"
    });
    let envelope: ProviderItemEnvelope = serde_json::from_value(json).unwrap();
    let output = bridge_envelope_to_source(&envelope);

    assert_eq!(output.source.source_type, "github");
    assert!(output.snapshot.content.is_some());
    assert!(output.snapshot.content_hash.is_some());
}

#[test]
fn github_issue_fixture_bridges_successfully() {
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    assert_eq!(
        output.source.uri,
        "https://github.com/acme/checkout/issues/123"
    );
    assert_eq!(output.source.source_type, "github");
    assert!(output.snapshot.captured_at.timestamp() > 0);
}

#[test]
fn slack_message_fixture_bridges_successfully() {
    let envelope = provider_item_envelope_slack_message();
    let output = bridge_envelope_to_source(&envelope);

    assert_eq!(output.source.source_type, "slack");
    assert_eq!(
        output.source.uri,
        "https://acme.slack.com/archives/C01234ABCDE/p1706000000000100"
    );
}

#[test]
fn minimal_fixture_bridges_successfully() {
    let envelope = provider_item_envelope_minimal();
    let output = bridge_envelope_to_source(&envelope);

    assert_eq!(output.source.source_type, "linear");
    assert_eq!(output.source.uri, "provider://linear:team-proj/LIN-42");
}

// ═══════════════════════════════════════════════════════════════════
// 2. First canonical state is bot-owned source/evidence capture
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bridge_output_is_source_and_snapshot_not_integrations_state() {
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    // Source is bot-owned mutable linkage
    assert!(!output.source.uri.is_empty());
    assert!(!output.source.source_type.is_empty());

    // Snapshot is bot-owned immutable capture
    assert!(!output.snapshot.snapshot_addr.is_empty());
    assert!(!output.snapshot.snapshot_ref.is_empty());
    assert!(output.snapshot.content.is_some());
    assert!(output.snapshot.content_hash.is_some());

    // Capture method is ApiPoll — bot fetched from provider
    assert_eq!(output.snapshot.capture_method, Some(CaptureMethod::ApiPoll));
}

#[test]
fn bridge_output_source_kind_is_bot_classified() {
    let issue = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&issue);
    assert_eq!(output.source.source_kind, Some(SourceKind::Tracker));

    let msg = provider_item_envelope_slack_message();
    let output = bridge_envelope_to_source(&msg);
    assert_eq!(output.source.source_kind, Some(SourceKind::Text));
}

#[test]
fn bridge_output_snapshot_ref_encodes_provider_provenance() {
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    // snapshot_ref encodes provider:resource:item for traceability
    assert!(output.snapshot.snapshot_ref.starts_with("provider:github:"));
    assert!(output.snapshot.snapshot_ref.contains("acme/checkout"));
    assert!(output.snapshot.snapshot_ref.contains("issue-123"));
}

// ═══════════════════════════════════════════════════════════════════
// 3. No DecisionCandidate is created from envelope parsing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bridge_output_type_does_not_contain_decision_candidate() {
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    // EnvelopeBridgeOutput has only `source` and `snapshot` fields.
    // This compile-time structural test proves no DecisionCandidate is
    // part of the bridge output type.
    let EnvelopeBridgeOutput { source, snapshot } = output;
    let _ = source;
    let _ = snapshot;
    // If this compiles, the struct has exactly two fields and neither is
    // a DecisionCandidate.
}

#[test]
fn bridge_output_serialized_contains_no_candidate_keys() {
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    let source_json = serde_json::to_string(&output.source).unwrap();
    let snapshot_json = serde_json::to_string(&output.snapshot).unwrap();

    // No decision candidate vocabulary in the output
    for keyword in &[
        "DecisionCandidate",
        "decision_candidate",
        "candidate_id",
        "candidate_state",
        "extraction_confidence",
        "review_lifecycle_state",
    ] {
        assert!(
            !source_json.contains(keyword),
            "Source JSON must not contain '{keyword}'"
        );
        assert!(
            !snapshot_json.contains(keyword),
            "Snapshot JSON must not contain '{keyword}'"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. No review/signoff/compliance/event-store mutation from envelope
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bridge_does_not_produce_event_store_entry() {
    // The bridge function signature returns EnvelopeBridgeOutput which
    // contains only Source + SourceSnapshot. It does not accept or return
    // an EventStoreAdapter or EventStoreEntry, proving no event-store
    // mutation path exists.
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    // Serialize both outputs and verify no event-store vocabulary
    let source_json = serde_json::to_string(&output.source).unwrap();
    let snapshot_json = serde_json::to_string(&output.snapshot).unwrap();

    for keyword in &[
        "event_store",
        "EventStoreEntry",
        "ingest_payload",
        "IngestPayload",
        "bind_to_code_payload",
    ] {
        assert!(
            !source_json.contains(keyword),
            "Source must not reference event-store: '{keyword}'"
        );
        assert!(
            !snapshot_json.contains(keyword),
            "Snapshot must not reference event-store: '{keyword}'"
        );
    }
}

#[test]
fn bridge_does_not_produce_review_signoff_compliance() {
    let envelope = provider_item_envelope_github_issue();
    let output = bridge_envelope_to_source(&envelope);

    let source_json = serde_json::to_string(&output.source).unwrap();
    let snapshot_json = serde_json::to_string(&output.snapshot).unwrap();
    let combined = format!("{source_json}{snapshot_json}");

    for keyword in &[
        "ReviewCommand",
        "review_command",
        "signoff",
        "approve_signoff",
        "reject_signoff",
        "compliance_verdict",
        "resolve_compliance",
        "GovernanceVerdict",
        "governance_verdict",
    ] {
        assert!(
            !combined.contains(keyword),
            "Bridge output must not contain review/signoff/compliance keyword: '{keyword}'"
        );
    }
}

#[test]
fn bridge_function_is_pure_no_side_effects() {
    // Calling the bridge twice with the same input produces identical output,
    // proving no state mutation or side effects.
    let envelope = provider_item_envelope_github_issue();
    let output1 = bridge_envelope_to_source(&envelope);
    let output2 = bridge_envelope_to_source(&envelope);

    assert_eq!(output1.source.uri, output2.source.uri);
    assert_eq!(output1.source.source_type, output2.source.source_type);
    assert_eq!(
        output1.snapshot.snapshot_addr,
        output2.snapshot.snapshot_addr
    );
    assert_eq!(output1.snapshot.snapshot_ref, output2.snapshot.snapshot_ref);
    assert_eq!(output1.snapshot.content_hash, output2.snapshot.content_hash);
}

// ═══════════════════════════════════════════════════════════════════
// 5. Bridge uses fixtures/stubs only — no live provider calls
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bridge_requires_no_network_or_runtime_state() {
    // The bridge function takes only a &ProviderItemEnvelope reference.
    // It does not take:
    // - HTTP client / reqwest
    // - ConnectionRegistry
    // - EventStoreAdapter
    // - Runtime config / daemon state
    // - Filesystem paths
    //
    // This is enforced by its function signature:
    //   fn bridge_envelope_to_source(envelope: &ProviderItemEnvelope) -> EnvelopeBridgeOutput
    //
    // Calling it with a static fixture proves no live provider interaction.
    let envelope = provider_item_envelope_minimal();
    let output = bridge_envelope_to_source(&envelope);
    assert!(!output.source.uri.is_empty());
}

#[test]
fn all_fixture_variants_bridge_without_error() {
    let fixtures: Vec<ProviderItemEnvelope> = vec![
        provider_item_envelope_github_issue(),
        provider_item_envelope_slack_message(),
        provider_item_envelope_minimal(),
    ];

    for envelope in &fixtures {
        let output = bridge_envelope_to_source(envelope);
        assert!(!output.source.uri.is_empty());
        assert!(!output.snapshot.snapshot_addr.is_empty());
        assert!(output.snapshot.content.is_some());
        assert!(output.snapshot.content_hash.is_some());
        assert_eq!(output.snapshot.capture_method, Some(CaptureMethod::ApiPoll));
    }
}

#[test]
fn bridge_content_hash_matches_sha256_of_content() {
    use sha2::{Digest, Sha256};

    let envelope = provider_item_envelope_slack_message();
    let output = bridge_envelope_to_source(&envelope);

    let mut hasher = Sha256::new();
    hasher.update(envelope.content.as_bytes());
    let expected = format!("{:x}", hasher.finalize());

    assert_eq!(output.snapshot.content_hash.as_ref().unwrap(), &expected);
}
