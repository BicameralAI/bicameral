//! Conformance harness for deterministic authority resolution (bicameral#55).
//!
//! Governing record: ARCH-0005 (accepted) and the ARCH-0003/0004 model it
//! builds on: one `SourceSnapshot` has one `GovernedDomain`, and `Signoff` is
//! the approved state of that domain.
//!
//! This test changes no product behavior. It does two things:
//!
//! 1. It checks that the synthetic fixtures under
//!    `protocol/authority-resolution/fixtures/` are well formed against the
//!    forward-defining payload schemas in `protocol/spec-registry/`, that every
//!    `source_digest` recomputes, and that every fixture states a typed expected
//!    outcome. These tests run and pass today.
//! 2. It marks the resolver rows `#[ignore]` with a BLOCKED reason. Each one
//!    loads its fixture and then stops with `unimplemented!`. They carry the
//!    expected outcome and nothing else: this crate has no resolver, and this
//!    file does not invent a resolver API. A skipped test is not a PASS.
//!
//! The reason labels in `expected` are test-local. ARCH-0005 asks for no new
//! broad status enum, so they are not a product type.
//!
//! Digest recipe (ARCH-0003): SHA-256 hex of one compact UTF-8 JSON object whose
//! only keys are `atoms`, `claim_assertions`, `claims` and `relations`, object
//! keys sorted, array order as stored, provenance fields outside the hash. ARCH-0003
//! gives the empty-set vector that `empty_claim_set_has_the_recorded_digest`
//! asserts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Replay states of a snapshot (ARCH-0004). Fixture-level labels, not a type.
const REPLAYED_STATES: [&str; 4] = ["tracked", "approved", "rejected", "superseded"];

/// Test-local exclusion reasons. See the module docs.
const EXCLUSION_REASONS: [&str; 6] = [
    "not_approved",
    "superseded",
    "rejected",
    "extract_failed",
    "out_of_scope",
    "conflict_unresolved",
];

/// Test-local typed-unknown labels. See the module docs.
const TYPED_UNKNOWNS: [&str; 5] = [
    "stale_source",
    "partial_corpus",
    "inaccessible_source",
    "unsupported_relation",
    "missing_check_record",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/authority-resolution/fixtures")
        .canonicalize()
        .expect("authority-resolution fixtures dir exists")
}

/// All fixtures as `(file stem, document)`, sorted by file name.
fn load_fixtures() -> Vec<(String, Value)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .expect("read fixtures dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("utf-8 file name")
                .to_string();
            let raw = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{stem}: read {}: {e}", path.display()));
            let doc: Value =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{stem}: invalid JSON: {e}"));
            (stem, doc)
        })
        .collect()
}

fn fixture(stem: &str) -> Value {
    load_fixtures()
        .into_iter()
        .find(|(name, _)| name == stem)
        .unwrap_or_else(|| panic!("fixture {stem} is missing"))
        .1
}

/// Compact JSON with object keys sorted recursively. Written out here so the
/// result does not depend on `serde_json` map ordering features.
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let members: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).expect("key serializes"),
                        canonical_json(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let members: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", members.join(","))
        }
        other => serde_json::to_string(other).expect("scalar serializes"),
    }
}

fn domain_digest(domain: &Value) -> String {
    let mut object = serde_json::Map::new();
    for key in ["claims", "atoms", "claim_assertions", "relations"] {
        object.insert(
            key.to_string(),
            domain
                .get(key)
                .unwrap_or_else(|| panic!("domain is missing `{key}`"))
                .clone(),
        );
    }
    let canonical = canonical_json(&Value::Object(object));
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn assert_keys_within(value: &Value, allowed: &[&str], context: &str) {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let present = keys(value);
    let extra: Vec<&&str> = present.difference(&allowed).collect();
    assert!(
        extra.is_empty(),
        "{context}: keys not in the registry schema: {extra:?}"
    );
}

fn assert_keys_required(value: &Value, required: &[&str], context: &str) {
    let present = keys(value);
    for key in required {
        assert!(present.contains(key), "{context}: missing required `{key}`");
    }
}

fn str_field<'a>(value: &'a Value, key: &str, context: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| panic!("{context}: `{key}` must be a non-empty string"))
}

fn snapshots(doc: &Value) -> &Vec<Value> {
    doc["snapshots"].as_array().expect("snapshots array")
}

fn snapshot_addr(snap: &Value) -> &str {
    snap["source_snapshot"]["snapshot_addr"]
        .as_str()
        .expect("snapshot_addr")
}

// ── Harness: these tests run and pass today ─────────────────────────────

#[test]
fn canonical_json_is_key_order_independent() {
    let a: Value = serde_json::from_str(r#"{"b":[{"y":1,"x":2}],"a":"s"}"#).unwrap();
    let b: Value = serde_json::from_str(r#"{"a":"s","b":[{"x":2,"y":1}]}"#).unwrap();
    assert_eq!(canonical_json(&a), canonical_json(&b));
    assert_eq!(canonical_json(&a), r#"{"a":"s","b":[{"x":2,"y":1}]}"#);
}

#[test]
fn empty_claim_set_has_the_recorded_digest() {
    // ARCH-0003: "four empty arrays hash the bytes
    // `{"atoms":[],"claim_assertions":[],"claims":[],"relations":[]}`".
    let empty: Value = serde_json::json!({
        "claims": [], "atoms": [], "claim_assertions": [], "relations": []
    });
    let bytes = canonical_json(&serde_json::json!({
        "atoms": [], "claim_assertions": [], "claims": [], "relations": []
    }));
    assert_eq!(
        bytes,
        r#"{"atoms":[],"claim_assertions":[],"claims":[],"relations":[]}"#
    );
    assert_eq!(
        domain_digest(&empty),
        "db4cd0ee0b2a1dbdbe2e0aea7ae0d7564d8ca2fb47d5b5ab458eb22c1114cbd9"
    );
}

#[test]
fn fixtures_exist_and_are_synthetic() {
    let fixtures = load_fixtures();
    assert!(
        fixtures.len() >= 2,
        "expected at least the two first fixtures"
    );
    for (stem, doc) in &fixtures {
        assert_eq!(doc["synthetic"], true, "{stem}: must be marked synthetic");
        assert_eq!(doc["fixture_version"], 1, "{stem}: fixture_version");
        assert_eq!(str_field(doc, "case_id", stem), stem, "{stem}: case_id");
        let text = canonical_json(doc);
        for forbidden in ["http://", "https://", "@", "/Users/"] {
            assert!(
                !text.contains(forbidden),
                "{stem}: synthetic fixtures must not contain `{forbidden}`"
            );
        }
        for snap in snapshots(doc) {
            let uri = str_field(&snap["source_snapshot"], "source_uri", stem);
            assert!(
                uri.starts_with("synthetic://"),
                "{stem}: source_uri must be synthetic, got {uri}"
            );
        }
    }
}

#[test]
fn governed_domains_match_registry_shape_and_digest() {
    for (stem, doc) in load_fixtures() {
        let product_id = str_field(&doc, "product_id", &stem).to_string();
        for snap in snapshots(&doc) {
            let addr = snapshot_addr(snap);
            let ctx = format!("{stem}/{addr}");
            let domain = &snap["governed_domain"];
            assert_keys_within(
                domain,
                &[
                    "product_id",
                    "snapshot_addr",
                    "source_digest",
                    "route",
                    "model_id",
                    "prompt_id",
                    "prompt_digest",
                    "extract_error",
                    "claims",
                    "atoms",
                    "claim_assertions",
                    "relations",
                ],
                &ctx,
            );
            assert_keys_required(
                domain,
                &[
                    "product_id",
                    "snapshot_addr",
                    "claims",
                    "atoms",
                    "claim_assertions",
                    "relations",
                ],
                &ctx,
            );
            assert_eq!(str_field(domain, "product_id", &ctx), product_id, "{ctx}");
            assert_eq!(str_field(domain, "snapshot_addr", &ctx), addr, "{ctx}");

            // Completion fields come together, and exclude `extract_error`.
            let completion = [
                "source_digest",
                "route",
                "model_id",
                "prompt_id",
                "prompt_digest",
            ];
            let present: Vec<bool> = completion
                .iter()
                .map(|k| domain.get(*k).is_some())
                .collect();
            if domain.get("extract_error").is_some() {
                assert!(
                    present.iter().all(|p| !p),
                    "{ctx}: extract_error excludes the completion fields"
                );
            } else if present.iter().any(|p| *p) {
                assert!(
                    present.iter().all(|p| *p),
                    "{ctx}: completion fields are written together"
                );
                assert!(
                    ["system_one", "openai_compatible"].contains(&str_field(domain, "route", &ctx)),
                    "{ctx}: route"
                );
                assert_eq!(
                    str_field(domain, "source_digest", &ctx),
                    domain_digest(domain),
                    "{ctx}: source_digest must recompute from claims, atoms, claim_assertions, relations"
                );
            } else {
                assert!(
                    domain["claims"].as_array().is_some_and(|c| c.is_empty()),
                    "{ctx}: an uncompleted capture has an empty claim set"
                );
            }

            // Item shapes and referential integrity.
            let mut claim_ids = BTreeSet::new();
            for claim in domain["claims"].as_array().expect("claims") {
                assert_keys_within(claim, &["claim_id", "revision", "context_statement"], &ctx);
                assert_keys_required(claim, &["claim_id", "revision", "context_statement"], &ctx);
                assert!(claim["revision"].as_u64().is_some(), "{ctx}: revision");
                assert!(
                    claim_ids.insert(str_field(claim, "claim_id", &ctx)),
                    "{ctx}: duplicate claim_id"
                );
            }
            let mut atom_ids = BTreeSet::new();
            for atom in domain["atoms"].as_array().expect("atoms") {
                assert_keys_within(atom, &["atom_id", "atom_kind", "semantic_key"], &ctx);
                assert_keys_required(atom, &["atom_id", "atom_kind", "semantic_key"], &ctx);
                str_field(atom, "atom_kind", &ctx);
                str_field(atom, "semantic_key", &ctx);
                assert!(
                    atom_ids.insert(str_field(atom, "atom_id", &ctx)),
                    "{ctx}: duplicate atom_id"
                );
            }
            let mut assertion_ids = BTreeSet::new();
            for assertion in domain["claim_assertions"].as_array().expect("assertions") {
                let fields = ["assertion_id", "claim_id", "atom_id", "polarity"];
                assert_keys_within(assertion, &fields, &ctx);
                assert_keys_required(assertion, &fields, &ctx);
                assert!(
                    assertion_ids.insert(str_field(assertion, "assertion_id", &ctx)),
                    "{ctx}: duplicate assertion_id"
                );
                assert!(
                    claim_ids.contains(str_field(assertion, "claim_id", &ctx)),
                    "{ctx}: assertion cites an unknown claim"
                );
                assert!(
                    atom_ids.contains(str_field(assertion, "atom_id", &ctx)),
                    "{ctx}: assertion cites an unknown atom"
                );
                assert!(
                    ["positive", "negative", "unknown"]
                        .contains(&str_field(assertion, "polarity", &ctx)),
                    "{ctx}: polarity"
                );
            }
            for relation in domain["relations"].as_array().expect("relations") {
                let fields = [
                    "relation_id",
                    "relation_kind",
                    "from_atom_id",
                    "to_atom_id",
                    "supporting_claim_ids",
                ];
                assert_keys_within(relation, &fields, &ctx);
                assert_keys_required(relation, &fields, &ctx);
                str_field(relation, "relation_id", &ctx);
                str_field(relation, "relation_kind", &ctx);
                assert!(
                    atom_ids.contains(str_field(relation, "from_atom_id", &ctx))
                        && atom_ids.contains(str_field(relation, "to_atom_id", &ctx)),
                    "{ctx}: relation cites an unknown atom"
                );
                for claim_id in relation["supporting_claim_ids"].as_array().expect("array") {
                    assert!(
                        claim_ids.contains(claim_id.as_str().expect("string")),
                        "{ctx}: relation cites an unknown supporting claim"
                    );
                }
            }
        }
    }
}

#[test]
fn source_snapshots_signoffs_and_supersession_are_consistent() {
    for (stem, doc) in load_fixtures() {
        let by_addr: BTreeMap<&str, &Value> = snapshots(&doc)
            .iter()
            .map(|s| (snapshot_addr(s), s))
            .collect();
        assert_eq!(
            by_addr.len(),
            snapshots(&doc).len(),
            "{stem}: duplicate snapshot_addr"
        );
        for snap in snapshots(&doc) {
            let addr = snapshot_addr(snap);
            let ctx = format!("{stem}/{addr}");
            let state = str_field(snap, "replayed_state", &ctx);
            assert!(REPLAYED_STATES.contains(&state), "{ctx}: replayed_state");

            let source = &snap["source_snapshot"];
            assert_keys_within(
                source,
                &[
                    "snapshot_addr",
                    "snapshot_ref",
                    "captured_at",
                    "content",
                    "content_hash",
                    "capture_method",
                    "source_uri",
                ],
                &ctx,
            );
            assert_keys_required(
                source,
                &["snapshot_addr", "snapshot_ref", "captured_at", "source_uri"],
                &ctx,
            );

            let signoff = &snap["signoff"];
            let supersede = &snap["supersede_snapshot"];
            match state {
                "approved" | "superseded" => {
                    assert!(signoff.is_object(), "{ctx}: {state} needs a signoff");
                    assert_keys_within(
                        signoff,
                        &[
                            "snapshot_addr",
                            "domain_digest",
                            "daemon_id",
                            "actor",
                            "role",
                            "person_id",
                            "check_records",
                        ],
                        &ctx,
                    );
                    assert_keys_required(
                        signoff,
                        &[
                            "snapshot_addr",
                            "domain_digest",
                            "daemon_id",
                            "actor",
                            "check_records",
                        ],
                        &ctx,
                    );
                    assert_eq!(str_field(signoff, "snapshot_addr", &ctx), addr, "{ctx}");
                    assert_eq!(
                        str_field(signoff, "domain_digest", &ctx),
                        str_field(&snap["governed_domain"], "source_digest", &ctx),
                        "{ctx}: a signoff is bound to its own domain digest"
                    );
                    str_field(signoff, "daemon_id", &ctx);
                    match str_field(signoff, "actor", &ctx) {
                        "manual" => {
                            assert!(["owner", "member"].contains(&str_field(signoff, "role", &ctx)));
                            str_field(signoff, "person_id", &ctx);
                        }
                        "policy" => assert!(
                            signoff.get("role").is_none() && signoff.get("person_id").is_none(),
                            "{ctx}: a policy signoff has no role or person"
                        ),
                        other => panic!("{ctx}: actor `{other}`"),
                    }
                }
                _ => assert!(signoff.is_null(), "{ctx}: {state} must have no signoff"),
            }
            if state == "superseded" {
                assert_keys_within(
                    supersede,
                    &[
                        "source_uri",
                        "snapshot_addr",
                        "successor_snapshot_addr",
                        "person_id",
                        "daemon_id",
                    ],
                    &ctx,
                );
                assert_keys_required(
                    supersede,
                    &[
                        "source_uri",
                        "snapshot_addr",
                        "successor_snapshot_addr",
                        "person_id",
                        "daemon_id",
                    ],
                    &ctx,
                );
                assert_eq!(str_field(supersede, "snapshot_addr", &ctx), addr, "{ctx}");
                let successor = str_field(supersede, "successor_snapshot_addr", &ctx);
                let successor_snap = by_addr.get(successor).unwrap_or_else(|| {
                    panic!("{ctx}: successor {successor} is not in the fixture")
                });
                assert_eq!(
                    str_field(supersede, "source_uri", &ctx),
                    str_field(&successor_snap["source_snapshot"], "source_uri", &ctx),
                    "{ctx}: a successor belongs to the same source"
                );
            } else {
                assert!(
                    supersede.is_null(),
                    "{ctx}: only superseded has a supersede event"
                );
            }
        }
    }
}

#[test]
fn check_records_are_ordered_pairs_over_known_digests() {
    for (stem, doc) in load_fixtures() {
        let digests: BTreeSet<&str> = snapshots(&doc)
            .iter()
            .filter_map(|s| s["governed_domain"]["source_digest"].as_str())
            .collect();
        let mut seen = BTreeSet::new();
        for record in doc["check_records"].as_array().expect("check_records") {
            assert_keys_within(
                record,
                &[
                    "domain_digest",
                    "other_domain_digest",
                    "conflict",
                    "daemon_id",
                ],
                &stem,
            );
            assert_keys_required(
                record,
                &[
                    "domain_digest",
                    "other_domain_digest",
                    "conflict",
                    "daemon_id",
                ],
                &stem,
            );
            let lesser = str_field(record, "domain_digest", &stem);
            let greater = str_field(record, "other_domain_digest", &stem);
            assert!(
                lesser < greater,
                "{stem}: pair stored lexicographically, distinct"
            );
            assert!(
                digests.contains(lesser) && digests.contains(greater),
                "{stem}: unknown digest"
            );
            assert!(record["conflict"].is_boolean(), "{stem}: conflict");
            str_field(record, "daemon_id", &stem);
            assert!(seen.insert((lesser, greater)), "{stem}: duplicate pair");
        }
    }
}

#[test]
fn every_fixture_states_a_typed_expected_outcome() {
    for (stem, doc) in load_fixtures() {
        assert!(
            str_field(&doc, "status", &stem).starts_with("BLOCKED"),
            "{stem}: a fixture whose rows need the resolver is BLOCKED, never PASS"
        );
        let expected = &doc["expected"];
        assert_keys_within(
            expected,
            &["governing", "excluded", "unresolved", "typed_unknown"],
            &stem,
        );
        assert_keys_required(
            expected,
            &["governing", "excluded", "unresolved", "typed_unknown"],
            &stem,
        );
        let states: BTreeMap<&str, &str> = snapshots(&doc)
            .iter()
            .map(|s| {
                (
                    snapshot_addr(s),
                    s["replayed_state"].as_str().expect("state"),
                )
            })
            .collect();

        let governing: Vec<&str> = expected["governing"]
            .as_array()
            .expect("governing array")
            .iter()
            .map(|v| v.as_str().expect("addr"))
            .collect();
        let mut accounted: BTreeSet<&str> = BTreeSet::new();
        for addr in &governing {
            assert_eq!(
                states.get(addr).copied(),
                Some("approved"),
                "{stem}: only an approved snapshot may govern ({addr})"
            );
            assert!(accounted.insert(addr), "{stem}: {addr} listed twice");
        }
        for entry in expected["excluded"].as_array().expect("excluded array") {
            assert_keys_within(entry, &["snapshot_addr", "reason"], &stem);
            let addr = str_field(entry, "snapshot_addr", &stem);
            assert!(
                states.contains_key(addr),
                "{stem}: excluded {addr} is unknown"
            );
            assert!(
                EXCLUSION_REASONS.contains(&str_field(entry, "reason", &stem)),
                "{stem}: reason must be a test-local label"
            );
            assert!(accounted.insert(addr), "{stem}: {addr} listed twice");
        }
        for unknown in expected["typed_unknown"].as_array().expect("typed_unknown") {
            assert!(
                TYPED_UNKNOWNS.contains(&unknown.as_str().expect("label")),
                "{stem}: typed_unknown must be a test-local label"
            );
        }
        let unresolved = expected["unresolved"].as_bool().expect("unresolved bool");
        if unresolved {
            assert!(
                governing.is_empty(),
                "{stem}: unresolved means nothing governs"
            );
        } else {
            assert_eq!(
                accounted.len(),
                states.len(),
                "{stem}: every snapshot is governing or excluded with a reason"
            );
        }
    }
}

// ── Resolver rows: BLOCKED until the resolver lands (ARCH-0005) ─────────
//
// Each test loads its fixture so a deleted or renamed fixture still fails
// loudly, then stops. When a resolver exists, call it here with the fixture's
// `snapshots`, `check_records` and `query`, and compare with `expected`.

#[test]
#[ignore = "BLOCKED (ARCH-0005): resolver not implemented"]
fn tracked_snapshot_never_governs() {
    let doc = fixture("tracked-is-not-governing");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005): resolver not implemented"]
fn supersession_resolves_to_the_approved_successor_and_a_later_draft_never_wins() {
    let doc = fixture("supersession-with-later-draft");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}
