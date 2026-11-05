//! Conformance harness for deterministic authority resolution (bicameral#55).
//!
//! Governing record: ARCH-0005 (accepted) and the ARCH-0003/0004 model it
//! builds on: one `SourceSnapshot` has one `GovernedDomain`, and `Signoff` is
//! the approved state of that domain.
//!
//! This test changes no product behavior. It does two things:
//!
//! 1. It checks that the synthetic fixtures under
//!    `protocol/authority-resolution/fixtures/` are sound. Payload shape comes
//!    from the existing registry validator
//!    (`scripts/validate_authority_resolution_fixtures.py`, which reuses
//!    `validate_spec_registry_schemas.errors_for`), so this file carries no
//!    second schema implementation. The rules that span several objects are
//!    checked here: `source_digest` recomputation, signoff binding, supersede
//!    event identity, `CheckRecord` pairs and the typed expected outcome. The
//!    negative regression tests prove each rule rejects a broken fixture.
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
//! keys sorted, array order as stored, provenance fields outside the hash.
//! ARCH-0003 gives the empty-set vector that
//! `empty_claim_set_has_the_recorded_digest` asserts.
//!
//! Running the registry validator needs `python3` (3.10 or newer), as the
//! repository's other contract checks do.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

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

type Check = Result<(), String>;

fn bot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("apps/bot root exists")
}

fn fixtures_dir() -> PathBuf {
    bot_root()
        .join("protocol/authority-resolution/fixtures")
        .canonicalize()
        .expect("authority-resolution fixtures dir exists")
}

/// All fixtures in `dir` as `(file stem, document)`, sorted by file name.
fn load_dir(dir: &Path) -> Vec<(String, Value)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
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

fn load_fixtures() -> Vec<(String, Value)> {
    load_dir(&fixtures_dir())
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

fn text<'a>(value: &'a Value, key: &str, ctx: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{ctx}: `{key}` must be a non-empty string"))
}

fn list<'a>(value: &'a Value, key: &str, ctx: &str) -> Result<&'a Vec<Value>, String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{ctx}: `{key}` must be an array"))
}

fn snapshots(doc: &Value) -> Result<&Vec<Value>, String> {
    list(doc, "snapshots", "fixture")
}

fn addr_of(snap: &Value) -> Result<&str, String> {
    text(&snap["source_snapshot"], "snapshot_addr", "source_snapshot")
}

// ── Cross-object rules (registry shape is checked by the Python validator) ──

fn check_synthetic(stem: &str, doc: &Value) -> Check {
    if doc["synthetic"] != true {
        return Err(format!("{stem}: must be marked synthetic"));
    }
    if doc["fixture_version"] != 1 {
        return Err(format!("{stem}: fixture_version must be 1"));
    }
    if text(doc, "case_id", stem)? != stem {
        return Err(format!("{stem}: case_id must equal the file name"));
    }
    let body = canonical_json(doc);
    for forbidden in ["http://", "https://", "@", "/Users/"] {
        if body.contains(forbidden) {
            return Err(format!(
                "{stem}: synthetic fixtures must not contain `{forbidden}`"
            ));
        }
    }
    for snap in snapshots(doc)? {
        let uri = text(&snap["source_snapshot"], "source_uri", stem)?;
        if !uri.starts_with("synthetic://") {
            return Err(format!("{stem}: source_uri must be synthetic, got {uri}"));
        }
    }
    Ok(())
}

/// Identifier uniqueness, references inside a domain, and `source_digest`.
fn check_domains(stem: &str, doc: &Value) -> Check {
    let product_id = text(doc, "product_id", stem)?;
    for snap in snapshots(doc)? {
        let addr = addr_of(snap)?;
        let ctx = format!("{stem}/{addr}");
        let domain = &snap["governed_domain"];
        if text(domain, "product_id", &ctx)? != product_id {
            return Err(format!("{ctx}: domain product_id differs from the fixture"));
        }
        if text(domain, "snapshot_addr", &ctx)? != addr {
            return Err(format!(
                "{ctx}: domain snapshot_addr differs from its snapshot"
            ));
        }

        let mut claims = BTreeSet::new();
        for claim in list(domain, "claims", &ctx)? {
            if !claims.insert(text(claim, "claim_id", &ctx)?) {
                return Err(format!("{ctx}: duplicate claim_id"));
            }
        }
        let mut atoms = BTreeSet::new();
        for atom in list(domain, "atoms", &ctx)? {
            if !atoms.insert(text(atom, "atom_id", &ctx)?) {
                return Err(format!("{ctx}: duplicate atom_id"));
            }
        }
        let mut assertions = BTreeSet::new();
        for assertion in list(domain, "claim_assertions", &ctx)? {
            if !assertions.insert(text(assertion, "assertion_id", &ctx)?) {
                return Err(format!("{ctx}: duplicate assertion_id"));
            }
            if !claims.contains(text(assertion, "claim_id", &ctx)?) {
                return Err(format!("{ctx}: assertion cites an unknown claim"));
            }
            if !atoms.contains(text(assertion, "atom_id", &ctx)?) {
                return Err(format!("{ctx}: assertion cites an unknown atom"));
            }
        }
        for relation in list(domain, "relations", &ctx)? {
            if !atoms.contains(text(relation, "from_atom_id", &ctx)?)
                || !atoms.contains(text(relation, "to_atom_id", &ctx)?)
            {
                return Err(format!("{ctx}: relation cites an unknown atom"));
            }
            for claim_id in list(relation, "supporting_claim_ids", &ctx)? {
                if !claims.contains(claim_id.as_str().unwrap_or_default()) {
                    return Err(format!("{ctx}: relation cites an unknown supporting claim"));
                }
            }
        }

        if domain.get("source_digest").is_some()
            && text(domain, "source_digest", &ctx)? != domain_digest(domain)
        {
            return Err(format!(
                "{ctx}: source_digest must recompute from claims, atoms, claim_assertions, relations"
            ));
        }
    }
    Ok(())
}

/// Replayed state against signoff and supersede payloads, and event identity.
fn check_replay(stem: &str, doc: &Value) -> Check {
    let by_addr: BTreeMap<&str, &Value> = snapshots(doc)?
        .iter()
        .map(|s| addr_of(s).map(|a| (a, s)))
        .collect::<Result<_, _>>()?;
    if by_addr.len() != snapshots(doc)?.len() {
        return Err(format!("{stem}: duplicate snapshot_addr"));
    }
    for snap in snapshots(doc)? {
        let addr = addr_of(snap)?;
        let ctx = format!("{stem}/{addr}");
        let state = text(snap, "replayed_state", &ctx)?;
        if !REPLAYED_STATES.contains(&state) {
            return Err(format!("{ctx}: unknown replayed_state `{state}`"));
        }
        let signoff = &snap["signoff"];
        let supersede = &snap["supersede_snapshot"];

        if state == "approved" || state == "superseded" {
            if !signoff.is_object() {
                return Err(format!("{ctx}: {state} needs a signoff"));
            }
            if text(signoff, "snapshot_addr", &ctx)? != addr {
                return Err(format!("{ctx}: signoff names a different snapshot"));
            }
            if text(signoff, "domain_digest", &ctx)?
                != text(&snap["governed_domain"], "source_digest", &ctx)?
            {
                return Err(format!(
                    "{ctx}: a signoff is bound to its own domain digest"
                ));
            }
        } else if !signoff.is_null() {
            return Err(format!("{ctx}: {state} must have no signoff"));
        }

        if state == "superseded" {
            if !supersede.is_object() {
                return Err(format!("{ctx}: superseded needs a supersede event"));
            }
            if text(supersede, "snapshot_addr", &ctx)? != addr {
                return Err(format!("{ctx}: supersede event names a different snapshot"));
            }
            // The event's source is the source of the snapshot it replaces.
            // The successor may belong to another source (ARCH-0004 conflict
            // resolution), so no same-source rule is applied to it.
            if text(supersede, "source_uri", &ctx)?
                != text(&snap["source_snapshot"], "source_uri", &ctx)?
            {
                return Err(format!(
                    "{ctx}: supersede event source_uri differs from the source of the snapshot it replaces"
                ));
            }
            let successor = text(supersede, "successor_snapshot_addr", &ctx)?;
            if successor == addr || !by_addr.contains_key(successor) {
                return Err(format!(
                    "{ctx}: successor {successor} must be another snapshot in the fixture"
                ));
            }
        } else if !supersede.is_null() {
            return Err(format!(
                "{ctx}: only a superseded snapshot has a supersede event"
            ));
        }
    }
    Ok(())
}

fn check_check_records(stem: &str, doc: &Value) -> Check {
    let digests: BTreeSet<&str> = snapshots(doc)?
        .iter()
        .filter_map(|s| s["governed_domain"]["source_digest"].as_str())
        .collect();
    let mut seen = BTreeSet::new();
    for record in list(doc, "check_records", stem)? {
        let lesser = text(record, "domain_digest", stem)?;
        let greater = text(record, "other_domain_digest", stem)?;
        if lesser >= greater {
            return Err(format!(
                "{stem}: a pair is stored lexicographically and its digests differ"
            ));
        }
        if !digests.contains(lesser) || !digests.contains(greater) {
            return Err(format!("{stem}: check record cites an unknown digest"));
        }
        if !seen.insert((lesser, greater)) {
            return Err(format!("{stem}: duplicate check record pair"));
        }
    }
    Ok(())
}

fn check_expected(stem: &str, doc: &Value) -> Check {
    if !text(doc, "status", stem)?.starts_with("BLOCKED") {
        return Err(format!(
            "{stem}: a fixture whose rows need the resolver is BLOCKED, never PASS"
        ));
    }
    let expected = &doc["expected"];
    let states: BTreeMap<&str, &str> = snapshots(doc)?
        .iter()
        .map(|s| Ok((addr_of(s)?, text(s, "replayed_state", stem)?)))
        .collect::<Result<_, String>>()?;

    let mut accounted: BTreeSet<&str> = BTreeSet::new();
    let governing = list(expected, "governing", stem)?;
    for addr in governing {
        let addr = addr.as_str().unwrap_or_default();
        if states.get(addr) != Some(&"approved") {
            return Err(format!(
                "{stem}: only an approved snapshot may govern ({addr})"
            ));
        }
        if !accounted.insert(addr) {
            return Err(format!("{stem}: {addr} listed twice"));
        }
    }
    for entry in list(expected, "excluded", stem)? {
        let addr = text(entry, "snapshot_addr", stem)?;
        if !states.contains_key(addr) {
            return Err(format!("{stem}: excluded {addr} is unknown"));
        }
        if !EXCLUSION_REASONS.contains(&text(entry, "reason", stem)?) {
            return Err(format!("{stem}: reason must be a test-local label"));
        }
        if !accounted.insert(addr) {
            return Err(format!("{stem}: {addr} listed twice"));
        }
    }
    for unknown in list(expected, "typed_unknown", stem)? {
        if !TYPED_UNKNOWNS.contains(&unknown.as_str().unwrap_or_default()) {
            return Err(format!("{stem}: typed_unknown must be a test-local label"));
        }
    }
    let unresolved = expected["unresolved"]
        .as_bool()
        .ok_or_else(|| format!("{stem}: `unresolved` must be a boolean"))?;
    if unresolved {
        if !governing.is_empty() {
            return Err(format!("{stem}: unresolved means nothing governs"));
        }
    } else if accounted.len() != states.len() {
        return Err(format!(
            "{stem}: every snapshot is governing or excluded with a reason"
        ));
    }
    Ok(())
}

// ── Registry validator (reuses the repository's schema machinery) ──────────

fn run_registry_validator(dir: &Path) -> Result<String, String> {
    let output = Command::new("python3")
        .args(["-I", "scripts/validate_authority_resolution_fixtures.py"])
        .arg(dir)
        .current_dir(bot_root())
        .output()
        .map_err(|e| {
            format!("python3 (3.10 or newer) is required to run the registry validator: {e}")
        })?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(combined)
    } else {
        Err(combined)
    }
}

/// Writes `docs` as `<name>.json` into a fresh temp dir and validates it.
fn validate_in_temp_dir(tag: &str, docs: &[(&str, &Value)]) -> Result<String, String> {
    let dir =
        std::env::temp_dir().join(format!("authority-resolution-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    for (name, doc) in docs {
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_vec_pretty(doc).expect("serialize"),
        )
        .expect("write fixture copy");
    }
    let result = run_registry_validator(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
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
        check_synthetic(stem, doc).unwrap();
    }
}

#[test]
fn fixtures_validate_against_the_spec_registry_schemas() {
    let output = run_registry_validator(&fixtures_dir()).unwrap_or_else(|e| panic!("{e}"));
    assert!(output.contains("passed"), "{output}");
}

#[test]
fn governed_domains_are_consistent_and_digests_recompute() {
    for (stem, doc) in load_fixtures() {
        check_domains(&stem, &doc).unwrap();
    }
}

#[test]
fn replayed_states_signoffs_and_supersession_are_consistent() {
    for (stem, doc) in load_fixtures() {
        check_replay(&stem, &doc).unwrap();
    }
}

#[test]
fn check_records_are_ordered_pairs_over_known_digests() {
    for (stem, doc) in load_fixtures() {
        check_check_records(&stem, &doc).unwrap();
    }
}

#[test]
fn every_fixture_states_a_typed_expected_outcome() {
    for (stem, doc) in load_fixtures() {
        check_expected(&stem, &doc).unwrap();
    }
}

// ── Negative regressions: a broken fixture must be rejected ─────────────

fn failed_extract_variant(populated: bool) -> Value {
    let mut doc = fixture("tracked-is-not-governing");
    let domain = doc["snapshots"][0]["governed_domain"]
        .as_object_mut()
        .expect("domain object");
    for key in [
        "source_digest",
        "route",
        "model_id",
        "prompt_id",
        "prompt_digest",
    ] {
        domain.remove(key);
    }
    domain.insert(
        "extract_error".into(),
        serde_json::json!({"reason": "synthetic_timeout", "failed_at": "2026-01-10T00:01:00Z"}),
    );
    if !populated {
        for key in ["claims", "atoms", "claim_assertions", "relations"] {
            domain.insert(key.into(), serde_json::json!([]));
        }
    }
    doc
}

#[test]
fn registry_validator_accepts_a_failed_extract_with_an_empty_claim_set() {
    let doc = failed_extract_variant(false);
    validate_in_temp_dir("failed-empty", &[("failed-extract-empty", &doc)])
        .unwrap_or_else(|e| panic!("a failed extract with empty arrays is valid: {e}"));
}

#[test]
fn registry_validator_rejects_populated_claims_on_a_failed_extract() {
    let doc = failed_extract_variant(true);
    let err = validate_in_temp_dir("failed-populated", &[("failed-extract-populated", &doc)])
        .expect_err("claims, atoms and assertions must be empty when extraction failed");
    assert!(err.contains("maxItems"), "unexpected rejection: {err}");
}

#[test]
fn supersede_event_must_name_the_source_of_the_snapshot_it_replaces() {
    let mut doc = fixture("supersession-with-later-draft");
    check_replay("control", &doc).expect("the unmodified fixture is consistent");
    doc["snapshots"][0]["source_snapshot"]["source_uri"] =
        Value::String("synthetic://policies/unrelated".into());
    let err = check_replay("mutated", &doc).expect_err("event and replaced snapshot disagree");
    assert!(
        err.contains("supersede event source_uri"),
        "unexpected rejection: {err}"
    );
}

#[test]
fn supersede_successor_may_belong_to_another_source() {
    // Cross-source conflict resolution names a successor from another source
    // (ARCH-0004); the identity check above must not forbid it.
    let mut doc = fixture("supersession-with-later-draft");
    doc["snapshots"][1]["source_snapshot"]["source_uri"] =
        Value::String("synthetic://policies/other-source".into());
    check_replay("cross-source", &doc).expect("a successor from another source is allowed");
}

#[test]
fn a_tampered_digest_is_rejected() {
    let mut doc = fixture("supersession-with-later-draft");
    let digest = doc["snapshots"][1]["governed_domain"]["source_digest"]
        .as_str()
        .expect("digest")
        .to_string();
    let flipped = if digest.starts_with('0') { "1" } else { "0" };
    doc["snapshots"][1]["governed_domain"]["source_digest"] =
        Value::String(format!("{flipped}{}", &digest[1..]));
    assert!(check_domains("tampered", &doc).is_err());
}

#[test]
fn a_tracked_snapshot_listed_as_governing_is_rejected() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["governing"] = serde_json::json!(["snap_t1"]);
    doc["expected"]["excluded"] = serde_json::json!([]);
    assert!(check_expected("tracked-governs", &doc).is_err());
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
