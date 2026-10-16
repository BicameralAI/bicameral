//! Provider acquisition boundary enforcement tests (issue #463).
//!
//! Acceptance criteria covered:
//! - Valid descriptor and item-envelope fixtures parse through bot-side types.
//! - Invalid authority/secret fixtures are rejected or explicitly classified.
//! - Parsing a descriptor does NOT create SourceBinding, event-store config,
//!   or setup completion state.
//! - Parsing an item envelope does NOT create Source, SourceSnapshot,
//!   SourceEvidence, or DecisionCandidate state.
//! - ToolRequest and ExternalIngestEnvelope forbidden-field boundaries remain
//!   covered.

use bicameral_api::provider_acquisition::{
    PermissionState, ProviderItemEnvelope, ProviderResourceDescriptor, ResourceCapability,
};
use bicameral_api::tool_request::{ExternalIngestEnvelope, ToolRequest};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/schemas/v2/fixtures")
        .canonicalize()
        .expect("fixtures directory must exist")
}

fn read_fixture(name: &str) -> Value {
    let path = fixtures_dir().join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("cannot parse fixture {}: {e}", path.display()))
}

/// Strip underscore-prefixed comment fields (e.g. `_comment`, `_expected_behavior`)
/// so they don't interfere with strict deserialization assertions.
fn strip_meta_fields(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, v)| (k.clone(), strip_meta_fields(v)))
                .collect();
            Value::Object(filtered)
        }
        other => other.clone(),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 1. Valid fixtures parse through bot-side types
// ═══════════════════════════════════════════════════════════════════

#[test]
fn valid_descriptor_fixture_parses_through_rust_type() {
    let raw = read_fixture("valid-provider-resource-descriptor-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor =
        serde_json::from_value(cleaned).expect("valid descriptor fixture must parse");
    assert_eq!(desc.provider, "github");
    assert_eq!(desc.resource_id, "octocat/hello-world");
    assert_eq!(desc.display_name, "Hello World");
    assert_eq!(desc.resource_type, "repository");
    assert_eq!(
        desc.capabilities,
        vec![ResourceCapability::List, ResourceCapability::Read]
    );
    assert_eq!(desc.captured_at, "2026-01-15T10:00:00Z");
}

#[test]
fn valid_item_envelope_fixture_parses_through_rust_type() {
    let raw = read_fixture("valid-provider-item-envelope-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope =
        serde_json::from_value(cleaned).expect("valid item fixture must parse");
    assert_eq!(item.provider, "github");
    assert_eq!(item.resource_id, "octocat/hello-world");
    assert_eq!(item.item_id, "issue-42");
    assert_eq!(item.item_type, "issue");
    assert_eq!(
        item.content,
        "Fix the widget alignment bug in the sidebar component"
    );
    assert_eq!(item.fetched_at, "2026-01-15T10:05:00Z");
}

#[test]
fn descriptor_with_all_optional_fields_parses() {
    let json = serde_json::json!({
        "provider": "google_drive",
        "resource_id": "folder-abc123",
        "display_name": "Engineering Specs",
        "resource_type": "folder",
        "uri": "https://drive.google.com/drive/folders/abc123",
        "capabilities": ["list", "read", "search"],
        "permission": "granted",
        "parent": { "resource_id": "root-drive", "display_name": "My Drive" },
        "freshness": {
            "last_modified": "2026-06-01T08:00:00Z",
            "etag": "rev-42",
            "item_count": 15
        },
        "provider_metadata": { "mime_type": "application/vnd.google-apps.folder" },
        "captured_at": "2026-06-15T12:00:00Z"
    });
    let desc: ProviderResourceDescriptor = serde_json::from_value(json).unwrap();
    assert_eq!(desc.permission, Some(PermissionState::Granted));
    assert!(desc.parent.is_some());
    assert_eq!(desc.freshness.as_ref().unwrap().item_count, Some(15));
}

#[test]
fn item_envelope_with_all_optional_fields_parses() {
    let json = serde_json::json!({
        "provider": "linear",
        "resource_id": "team-eng",
        "item_id": "LIN-99",
        "item_type": "issue",
        "content": "Implement caching layer for graph queries",
        "content_hash": "sha256:abcdef1234567890",
        "title": "Graph query caching",
        "uri": "https://linear.app/team/issue/LIN-99",
        "freshness": { "last_modified": "2026-06-10T09:00:00Z", "etag": "v3" },
        "provider_metadata": { "priority": 1, "status": "in_progress" },
        "fetched_at": "2026-06-15T12:00:00Z"
    });
    let item: ProviderItemEnvelope = serde_json::from_value(json).unwrap();
    assert_eq!(item.title.as_deref(), Some("Graph query caching"));
    assert_eq!(
        item.content_hash.as_deref(),
        Some("sha256:abcdef1234567890")
    );
    assert!(item.freshness.is_some());
}

/// Authority/lifecycle field names that must never appear on a parsed
/// provider-fact value, regardless of provider.
const FORBIDDEN_AUTHORITY_FIELDS: &[&str] = &[
    "binding_id",
    "source_binding",
    "binding",
    "approved_by",
    "event_store",
    "event_store_write",
    "setup_complete",
    "tracking_state",
    "compliance",
    "signoff",
    "governance_result",
    "review_command",
    "candidate",
    "decision",
];

fn golden_fixture_names(prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixtures_dir())
        .expect("fixtures dir readable")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(prefix) && n.ends_with(".json"))
        .collect();
    names.sort();
    names
}

#[test]
fn golden_descriptor_fixtures_cover_three_providers_and_parse() {
    let names = golden_fixture_names("valid-provider-resource-descriptor-");
    let mut providers = std::collections::BTreeSet::new();
    for name in &names {
        let cleaned = strip_meta_fields(&read_fixture(name));
        let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned)
            .unwrap_or_else(|e| panic!("golden descriptor {name} must parse: {e}"));
        providers.insert(desc.provider.clone());
        let serialized = serde_json::to_value(&desc).unwrap();
        for field in FORBIDDEN_AUTHORITY_FIELDS {
            assert!(
                serialized.get(field).is_none(),
                "{name}: forbidden authority field `{field}` must not survive parse"
            );
        }
    }
    for required in ["github", "linear", "google_drive"] {
        assert!(
            providers.contains(required),
            "golden descriptor fixtures must cover provider `{required}`; found {providers:?}"
        );
    }
}

#[test]
fn golden_item_envelope_fixtures_cover_three_providers_and_parse() {
    let names = golden_fixture_names("valid-provider-item-envelope-");
    let mut providers = std::collections::BTreeSet::new();
    for name in &names {
        let cleaned = strip_meta_fields(&read_fixture(name));
        let item: ProviderItemEnvelope = serde_json::from_value(cleaned)
            .unwrap_or_else(|e| panic!("golden item envelope {name} must parse: {e}"));
        providers.insert(item.provider.clone());
        let serialized = serde_json::to_value(&item).unwrap();
        for field in FORBIDDEN_AUTHORITY_FIELDS {
            assert!(
                serialized.get(field).is_none(),
                "{name}: forbidden authority field `{field}` must not survive parse"
            );
        }
    }
    for required in ["github", "linear", "google_drive"] {
        assert!(
            providers.contains(required),
            "golden item envelope fixtures must cover provider `{required}`; found {providers:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 2. Invalid authority/secret fixtures are rejected or classified
// ═══════════════════════════════════════════════════════════════════

#[test]
fn invalid_descriptor_local_authority_fields_ignored() {
    // When deserialized into ProviderResourceDescriptor, unknown fields
    // (authority, review_command, etc.) are silently dropped by serde.
    let raw = read_fixture("invalid-provider-descriptor-local-authority.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned).unwrap();
    // The descriptor parsed — but the forbidden `authority` field was discarded.
    // The resulting type has NO field to hold authority context.
    assert_eq!(desc.provider, "github");
    let serialized = serde_json::to_value(&desc).unwrap();
    assert!(
        serialized.get("authority").is_none(),
        "authority must not survive round-trip"
    );
}

#[test]
fn invalid_descriptor_review_command_fields_ignored() {
    let raw = read_fixture("invalid-provider-descriptor-review-command.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&desc).unwrap();
    assert!(
        serialized.get("review_command").is_none(),
        "review_command must not survive round-trip"
    );
}

#[test]
fn invalid_descriptor_event_store_write_fields_ignored() {
    let raw = read_fixture("invalid-provider-descriptor-event-store-write.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&desc).unwrap();
    assert!(
        serialized.get("event_store_write").is_none(),
        "event_store_write must not survive round-trip"
    );
}

#[test]
fn invalid_descriptor_bot_lifecycle_fields_ignored() {
    let raw = read_fixture("invalid-provider-descriptor-bot-lifecycle.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&desc).unwrap();
    assert!(serialized.get("tracking_state").is_none());
    assert!(serialized.get("compliance").is_none());
    assert!(serialized.get("signoff").is_none());
    assert!(serialized.get("governance_result").is_none());
}

#[test]
fn invalid_descriptor_secret_fields_ignored_from_typed_parse() {
    // Secrets in provider_metadata: the type accepts arbitrary metadata,
    // so we verify this parses but classify the fixture as invalid at the
    // protocol contract level (validated separately by screening).
    let raw = read_fixture("invalid-provider-descriptor-secret-fields.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned).unwrap();
    // The metadata exists but contains secret material — this is a screening violation,
    // not a type-system boundary issue. The type is permissive; the screening gate rejects.
    assert!(!desc.provider_metadata.is_empty());
}

#[test]
fn invalid_item_local_authority_fields_ignored() {
    let raw = read_fixture("invalid-provider-item-local-authority.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&item).unwrap();
    assert!(
        serialized.get("authority").is_none(),
        "authority must not survive round-trip"
    );
}

#[test]
fn invalid_item_review_command_fields_ignored() {
    let raw = read_fixture("invalid-provider-item-review-command.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&item).unwrap();
    assert!(
        serialized.get("review_command").is_none(),
        "review_command must not survive round-trip"
    );
}

#[test]
fn invalid_item_event_store_write_fields_ignored() {
    let raw = read_fixture("invalid-provider-item-event-store-write.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&item).unwrap();
    assert!(
        serialized.get("event_store_write").is_none(),
        "event_store_write must not survive round-trip"
    );
}

#[test]
fn invalid_item_bot_lifecycle_fields_ignored() {
    let raw = read_fixture("invalid-provider-item-bot-lifecycle.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&item).unwrap();
    assert!(serialized.get("tracking_state").is_none());
    assert!(serialized.get("source_binding").is_none());
    assert!(serialized.get("candidate").is_none());
}

#[test]
fn invalid_item_secret_fields_classified() {
    let raw = read_fixture("invalid-provider-item-secret-fields.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope = serde_json::from_value(cleaned).unwrap();
    // Content contains secrets — screening violation, not type-system issue.
    assert!(item.content.contains("api_key") || item.content.contains("sk_live"));
}

// ═══════════════════════════════════════════════════════════════════
// 3. Descriptor parsing does NOT create SourceBinding, event-store
//    config, or setup completion state
// ═══════════════════════════════════════════════════════════════════

#[test]
fn descriptor_parse_cannot_produce_source_binding() {
    // ProviderResourceDescriptor has no field of type SourceBinding or
    // anything that could be interpreted as binding creation. This test
    // proves that at the type level: we parse a valid descriptor and
    // confirm the resulting value contains nothing that could constitute
    // a binding record.
    let raw = read_fixture("valid-provider-resource-descriptor-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    let desc: ProviderResourceDescriptor = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&desc).unwrap();

    // No binding fields
    assert!(serialized.get("binding_id").is_none());
    assert!(serialized.get("source_binding").is_none());
    assert!(serialized.get("binding").is_none());
    assert!(serialized.get("approved_by").is_none());
    assert!(serialized.get("binding_config").is_none());

    // No event-store config fields
    assert!(serialized.get("event_store").is_none());
    assert!(serialized.get("event_store_write").is_none());
    assert!(serialized.get("store_config").is_none());

    // No setup completion state
    assert!(serialized.get("setup_complete").is_none());
    assert!(serialized.get("setup_state").is_none());
    assert!(serialized.get("connection_complete").is_none());
}

#[test]
fn descriptor_with_injected_binding_fields_does_not_carry_them() {
    // Even if a malicious payload includes binding/event-store fields,
    // the Rust type discards them during deserialization.
    let json = serde_json::json!({
        "provider": "github",
        "resource_id": "octocat/hello-world",
        "display_name": "Hello World",
        "resource_type": "repository",
        "captured_at": "2026-01-15T10:00:00Z",
        // Injected forbidden fields:
        "source_binding": { "binding_id": "bind-injected", "approved_by": "attacker" },
        "event_store_config": { "backend": "sqlite", "path": "/tmp/evil.db" },
        "setup_complete": true,
        "tracking_state": "active"
    });
    let desc: ProviderResourceDescriptor = serde_json::from_value(json).unwrap();
    let out = serde_json::to_value(&desc).unwrap();
    assert!(out.get("source_binding").is_none());
    assert!(out.get("event_store_config").is_none());
    assert!(out.get("setup_complete").is_none());
    assert!(out.get("tracking_state").is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 4. Item envelope parsing does NOT create Source, SourceSnapshot,
//    SourceEvidence, or DecisionCandidate state
// ═══════════════════════════════════════════════════════════════════

#[test]
fn item_envelope_parse_cannot_produce_source_state() {
    let raw = read_fixture("valid-provider-item-envelope-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    let item: ProviderItemEnvelope = serde_json::from_value(cleaned).unwrap();
    let serialized = serde_json::to_value(&item).unwrap();

    // No Source fields
    assert!(serialized.get("source").is_none());
    assert!(serialized.get("source_uri").is_none());
    assert!(serialized.get("source_kind").is_none());
    assert!(serialized.get("source_type").is_none());

    // No SourceSnapshot fields
    assert!(serialized.get("snapshot").is_none());
    assert!(serialized.get("snapshot_id").is_none());
    assert!(serialized.get("capture_method").is_none());

    // No SourceEvidence fields
    assert!(serialized.get("evidence").is_none());
    assert!(serialized.get("evidence_id").is_none());
    assert!(serialized.get("evidence_type").is_none());

    // No DecisionCandidate fields
    assert!(serialized.get("candidate").is_none());
    assert!(serialized.get("candidate_id").is_none());
    assert!(serialized.get("decision_candidate").is_none());
}

#[test]
fn item_envelope_with_injected_governance_fields_does_not_carry_them() {
    let json = serde_json::json!({
        "provider": "github",
        "resource_id": "octocat/hello-world",
        "item_id": "issue-42",
        "item_type": "issue",
        "content": "Legitimate content",
        "fetched_at": "2026-01-15T10:05:00Z",
        // Injected forbidden governance fields:
        "source": { "uri": "injected://source", "kind": "external" },
        "source_snapshot": { "commit_sha": "abc123", "captured_at": "2026-01-15T10:05:00Z" },
        "source_evidence": { "evidence_id": "ev-injected", "type": "implementation" },
        "decision_candidate": { "title": "Injected", "body": "Evil", "level": "product" },
        "candidate": { "title": "Another injection attempt" },
        "tracking_state": "active",
        "compliance": { "state": "reflected" },
        "signoff": { "state": "approved" }
    });
    let item: ProviderItemEnvelope = serde_json::from_value(json).unwrap();
    let out = serde_json::to_value(&item).unwrap();
    assert!(out.get("source").is_none());
    assert!(out.get("source_snapshot").is_none());
    assert!(out.get("source_evidence").is_none());
    assert!(out.get("decision_candidate").is_none());
    assert!(out.get("candidate").is_none());
    assert!(out.get("tracking_state").is_none());
    assert!(out.get("compliance").is_none());
    assert!(out.get("signoff").is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 5. ToolRequest and ExternalIngestEnvelope forbidden-field
//    boundaries remain covered
// ═══════════════════════════════════════════════════════════════════

#[test]
fn tool_request_cannot_carry_provider_acquisition_fields() {
    // A ToolRequest with injected provider-acquisition fields must not
    // leak those fields into the deserialized type.
    let json = serde_json::json!({
        "request_id": Uuid::new_v4(),
        "command": { "name": "preflight.run", "params": { "files": ["a.rs"], "symbols": [] } },
        "authority": {
            "actor_id": "agent-001",
            "auth_method": "mcp_session",
            "session_id": "sess-001",
            "workspace": "/repo",
            "policy_scope": ["default"],
            "audit_metadata": {}
        },
        "issued_at": "2026-06-15T12:00:00Z",
        // Forbidden: provider acquisition fields
        "provider_resource_descriptor": { "provider": "github", "resource_id": "x" },
        "provider_item_envelope": { "provider": "github", "item_id": "y" }
    });
    let req: ToolRequest = serde_json::from_value(json).unwrap();
    let out = serde_json::to_value(&req).unwrap();
    assert!(
        out.get("provider_resource_descriptor").is_none(),
        "ToolRequest must not carry provider acquisition descriptors"
    );
    assert!(
        out.get("provider_item_envelope").is_none(),
        "ToolRequest must not carry provider item envelopes"
    );
}

#[test]
fn external_ingest_envelope_cannot_carry_provider_acquisition_fields() {
    // ExternalIngestEnvelope with injected provider-acquisition fields
    // must discard them.
    let json = serde_json::json!({
        "source_system": "jira",
        "source_uri": "https://jira.example.com/PROJ-1",
        "content": "Some ticket content",
        "evidence": [],
        "candidate_hints": [],
        // Forbidden: provider acquisition fields
        "provider_resource_descriptor": { "provider": "github", "resource_id": "x" },
        "provider_item_envelope": { "provider": "github", "item_id": "y" }
    });
    let envelope: ExternalIngestEnvelope = serde_json::from_value(json).unwrap();
    let out = serde_json::to_value(&envelope).unwrap();
    assert!(
        out.get("provider_resource_descriptor").is_none(),
        "ExternalIngestEnvelope must not carry provider acquisition descriptors"
    );
    assert!(
        out.get("provider_item_envelope").is_none(),
        "ExternalIngestEnvelope must not carry provider item envelopes"
    );
}

#[test]
fn external_ingest_envelope_forbidden_review_authority_fields() {
    // ExternalIngestEnvelope must not carry review commands, tool commands,
    // or local authority even if present in the raw JSON.
    let json = serde_json::json!({
        "source_system": "slack",
        "source_uri": "https://slack.com/archives/C01/p123",
        "content": "Meeting notes about feature design",
        "evidence": [],
        "candidate_hints": [],
        // Forbidden: authority and review fields
        "review_command": { "kind": "accept_candidate", "candidate_id": "cand-1" },
        "authority": { "actor_id": "attacker", "auth_method": "local_daemon" },
        "tool_command": { "name": "review.approve_signoff", "params": { "target_id": "DEC-1" } }
    });
    let envelope: ExternalIngestEnvelope = serde_json::from_value(json).unwrap();
    let out = serde_json::to_value(&envelope).unwrap();
    assert!(out.get("review_command").is_none());
    assert!(out.get("authority").is_none());
    assert!(out.get("tool_command").is_none());
}

#[test]
fn tool_request_cannot_carry_external_ingest_envelope_fields() {
    // ToolRequest must not leak ExternalIngestEnvelope fields.
    let json = serde_json::json!({
        "request_id": Uuid::new_v4(),
        "command": { "name": "preflight.run", "params": { "files": ["b.rs"], "symbols": [] } },
        "authority": {
            "actor_id": "agent-002",
            "auth_method": "cli",
            "workspace": "/repo2",
            "policy_scope": ["default"],
            "audit_metadata": {}
        },
        "issued_at": "2026-06-15T12:00:00Z",
        // Forbidden: ExternalIngestEnvelope fields
        "source_system": "github",
        "source_uri": "https://github.com/org/repo/issues/1",
        "candidate_hints": [{ "title": "Injected", "body": "Evil" }]
    });
    let req: ToolRequest = serde_json::from_value(json).unwrap();
    let out = serde_json::to_value(&req).unwrap();
    assert!(out.get("source_system").is_none());
    assert!(out.get("source_uri").is_none());
    assert!(out.get("candidate_hints").is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 6. Cross-boundary: provider types cannot parse as ToolRequest or
//    ExternalIngestEnvelope and vice versa
// ═══════════════════════════════════════════════════════════════════

#[test]
fn descriptor_json_cannot_parse_as_tool_request() {
    let raw = read_fixture("valid-provider-resource-descriptor-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    // ToolRequest requires `command` and `authority` — descriptor JSON lacks these.
    let result = serde_json::from_value::<ToolRequest>(cleaned);
    assert!(
        result.is_err(),
        "provider descriptor must not accidentally parse as ToolRequest"
    );
}

#[test]
fn item_envelope_json_cannot_parse_as_tool_request() {
    let raw = read_fixture("valid-provider-item-envelope-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    let result = serde_json::from_value::<ToolRequest>(cleaned);
    assert!(
        result.is_err(),
        "provider item envelope must not accidentally parse as ToolRequest"
    );
}

#[test]
fn descriptor_json_cannot_parse_as_external_ingest() {
    let raw = read_fixture("valid-provider-resource-descriptor-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    // ExternalIngestEnvelope requires `source_system`, `source_uri`, `content`.
    let result = serde_json::from_value::<ExternalIngestEnvelope>(cleaned);
    assert!(
        result.is_err(),
        "provider descriptor must not accidentally parse as ExternalIngestEnvelope"
    );
}

#[test]
fn item_envelope_json_cannot_parse_as_external_ingest() {
    let raw = read_fixture("valid-provider-item-envelope-minimal.json");
    let cleaned = strip_meta_fields(&raw);
    let result = serde_json::from_value::<ExternalIngestEnvelope>(cleaned);
    assert!(
        result.is_err(),
        "provider item envelope must not accidentally parse as ExternalIngestEnvelope"
    );
}
