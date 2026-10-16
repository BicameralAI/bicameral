//! Deterministic conformance for the canonical alpha recall envelope fixture
//! (issue #818, parent #789; blocker #817).
//!
//! The daemon returns the canonical alpha recall envelope nested inside
//! `ToolResponse.result` under the `alpha_recall` key (see #817). This suite
//! pins an MCP-consumable golden fixture of that envelope so the `bicameral-mcp`
//! thin client can render staged recall, caveats, the topology witness, and the
//! `RuntimeTrace` correlation **without deriving product state locally** and
//! without guessing at the nested response shape.
//!
//! The golden JSON fixture under `protocol/schemas/v2/fixtures/` must equal the
//! [`bicameral_api::fixtures::alpha_recall_envelope`] constructor, so every
//! surface renders a byte-for-byte reproducible, daemon-authored contract.
//!
//! Set `BIC_REGEN_FIXTURES=1` to (re)write the golden JSON file from the Rust
//! fixture instead of asserting against it.

use std::path::PathBuf;

use bicameral_api::alpha_recall::{
    AlphaRecallAuthorityMode, AlphaRecallEnvelope, AlphaRecallMismatchKind,
    ALPHA_RECALL_ENVELOPE_VERSION, ALPHA_RECALL_RESULT_KEY,
};
use bicameral_api::fixtures::alpha_recall_envelope;

const FIXTURE_NAME: &str = "alpha-recall-envelope";
const RESULT_ENVELOPE_PATH: &str = "ToolResponse.result.alpha_recall";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/schemas/v2/fixtures")
        .canonicalize()
        .expect("fixtures dir exists")
}

fn read_fixture_doc() -> serde_json::Value {
    let path = fixtures_dir().join(format!("{FIXTURE_NAME}.json"));
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{FIXTURE_NAME}: parse failed: {e}"))
}

/// Parse the canonical envelope from the fixture's `alpha_recall` block.
fn read_fixture_envelope() -> AlphaRecallEnvelope {
    let doc = read_fixture_doc();
    let block = doc
        .get(ALPHA_RECALL_RESULT_KEY)
        .unwrap_or_else(|| panic!("{FIXTURE_NAME}: missing `{ALPHA_RECALL_RESULT_KEY}` key"));
    serde_json::from_value(block.clone())
        .unwrap_or_else(|e| panic!("{FIXTURE_NAME}: envelope block does not deserialize: {e}"))
}

fn write_fixture(envelope: &AlphaRecallEnvelope) {
    let path = fixtures_dir().join(format!("{FIXTURE_NAME}.json"));
    let envelope_v = serde_json::to_value(envelope).expect("serialize envelope");
    let doc = serde_json::json!({
        "_comment": format!(
            "Canonical daemon-owned alpha recall envelope (issues #817/#818, parent #789). \
             The daemon nests this at `{RESULT_ENVELOPE_PATH}` on `preflight.run`. The \
             `bicameral-mcp` thin client renders `product.staged`, `product.caveats`, \
             `witness.topology_receipt`, and `witness.runtime_trace` verbatim; it must not \
             compute recall, own freshness, derive caveats, or widen authority. The daemon \
             preserves the sibling result keys `staged` and `live_recall` for back-compat, \
             but `{ALPHA_RECALL_RESULT_KEY}` is the canonical envelope."
        ),
        "_result_envelope_path": RESULT_ENVELOPE_PATH,
        "_sibling_result_keys": ["staged", "live_recall", ALPHA_RECALL_RESULT_KEY],
        "_expected": {
            "envelope_version": envelope.envelope_version,
            "protocol_version": envelope.protocol_version,
            "authority_mode": envelope.authority.mode,
            "appended_canonical_events": envelope.authority.appended_canonical_events,
            "mutated_lifecycle_state": envelope.authority.mutated_lifecycle_state,
            "caveat_count": envelope.product.caveats.len(),
            "topology_receipt_consumed": envelope
                .witness
                .topology_receipt
                .as_ref()
                .map(|t| t.consumed),
            "runtime_trace_command": envelope.witness.runtime_trace.command,
        },
        ALPHA_RECALL_RESULT_KEY: envelope_v,
    });
    let pretty = serde_json::to_string_pretty(&doc).expect("serialize");
    std::fs::write(&path, format!("{pretty}\n")).expect("write fixture");
}

#[test]
fn protocol_fixture_matches_rust_envelope() {
    let envelope = alpha_recall_envelope();
    if std::env::var("BIC_REGEN_FIXTURES").is_ok() {
        write_fixture(&envelope);
        return;
    }

    let from_disk = read_fixture_envelope();
    // `AlphaRecallEnvelope` composes non-`Eq` product types, so compare on the
    // canonical JSON value form.
    assert_eq!(
        serde_json::to_value(&from_disk).unwrap(),
        serde_json::to_value(&envelope).unwrap(),
        "protocol fixture diverged from the Rust envelope — regenerate with BIC_REGEN_FIXTURES=1"
    );

    // The fixture round-trips through JSON without loss.
    let json = serde_json::to_string(&envelope).unwrap();
    let reparsed: AlphaRecallEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(&reparsed).unwrap(),
        serde_json::to_value(&envelope).unwrap(),
        "envelope is not round-trip stable"
    );
}

/// The fixture documents the canonical nesting path so the MCP repo does not
/// guess where the envelope lives in `ToolResponse.result`.
#[test]
fn fixture_documents_canonical_nesting() {
    let doc = read_fixture_doc();
    assert_eq!(
        doc.get("_result_envelope_path").and_then(|v| v.as_str()),
        Some(RESULT_ENVELOPE_PATH)
    );
    let siblings = doc
        .get("_sibling_result_keys")
        .and_then(|v| v.as_array())
        .expect("_sibling_result_keys present");
    for key in ["staged", "live_recall", ALPHA_RECALL_RESULT_KEY] {
        assert!(
            siblings.iter().any(|v| v.as_str() == Some(key)),
            "fixture must document sibling result key '{key}'"
        );
    }
    // The documented key equals the exported canonical constant.
    assert_eq!(ALPHA_RECALL_RESULT_KEY, "alpha_recall");
}

/// The envelope states its read-only authority and pins its contract, so a
/// surface renders the posture rather than inferring it.
#[test]
fn fixture_envelope_is_read_only_and_pinned() {
    let envelope = read_fixture_envelope();
    assert_eq!(envelope.envelope_version, ALPHA_RECALL_ENVELOPE_VERSION);
    assert_eq!(envelope.protocol_version, "v2");
    assert_eq!(envelope.authority.mode, AlphaRecallAuthorityMode::ReadOnly);
    assert!(!envelope.authority.appended_canonical_events);
    assert!(!envelope.authority.mutated_lifecycle_state);
}

/// The fixture carries everything the MCP repair needs to render, in the
/// daemon-owned `product` / `witness` split — nothing to derive locally.
#[test]
fn fixture_carries_renderable_product_and_witnesses() {
    let envelope = read_fixture_envelope();

    // Staged recall payload + recall packet + caveats are daemon-authored.
    assert!(
        !envelope.product.caveats.is_empty(),
        "fixture must carry daemon-authored caveats to render"
    );

    // Validation-only witnesses: RuntimeTrace correlation + not-consumed
    // topology receipt (never product state).
    assert_eq!(envelope.witness.runtime_trace.command, "preflight.run");
    let topology = envelope
        .witness
        .topology_receipt
        .as_ref()
        .expect("topology receipt witness present");
    assert!(
        !topology.consumed,
        "in-process contract slice must carry a not-consumed topology receipt"
    );

    // The wire form keeps product and witness in distinct sections.
    let v = serde_json::to_value(&envelope).unwrap();
    assert!(v["product"]["staged"].is_object());
    assert!(v["product"]["packet"].is_object());
    assert!(v["witness"]["runtime_trace"].is_object());
    assert!(v["product"].get("runtime_trace").is_none());
}

/// A thin client extracts the fixture envelope from a canonical nested
/// `ToolResponse.result` shape, and fails closed when the daemon content is
/// missing — never a legacy MCP-local fallback.
#[test]
fn fixture_envelope_extracts_from_nested_result_and_fails_closed() {
    let envelope = read_fixture_envelope();
    let result = serde_json::json!({
        "staged": { "legacy": "sibling" },
        "live_recall": { "legacy": "sibling" },
        ALPHA_RECALL_RESULT_KEY: serde_json::to_value(&envelope).unwrap(),
    });
    let extracted = AlphaRecallEnvelope::from_tool_result(Some(&result))
        .expect("canonical nested result must yield the envelope");
    assert_eq!(
        serde_json::to_value(&extracted).unwrap(),
        serde_json::to_value(&envelope).unwrap()
    );

    // A result missing the canonical envelope is a visible failure.
    let missing = serde_json::json!({ "staged": {}, "live_recall": {} });
    let err = AlphaRecallEnvelope::from_tool_result(Some(&missing))
        .expect_err("missing envelope must fail closed");
    assert_eq!(err.kind, AlphaRecallMismatchKind::MissingEnvelope);
}

/// The envelope never serializes governance/authority vocabulary — alpha recall
/// is advisory, read-only content.
#[test]
fn fixture_carries_no_authority_or_governance_vocabulary() {
    let envelope = read_fixture_envelope();
    let json = serde_json::to_string(&envelope).unwrap().to_lowercase();
    for forbidden in [
        "compliant",
        "\"block\"",
        "\"pause\"",
        "\"warn\"",
        "merge safe",
        "merge-safe",
        "safe to proceed",
    ] {
        assert!(
            !json.contains(forbidden),
            "alpha recall fixture must not contain '{forbidden}'"
        );
    }
}
