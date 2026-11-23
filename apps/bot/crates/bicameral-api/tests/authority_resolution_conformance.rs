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
//!    tests prove each rule added for #55: negative tests show it rejects a
//!    broken fixture, and one positive test shows the allowed missing-pair
//!    refusal passes. The rules carried over from #67 keep their own tests.
//! 2. It marks the resolver rows `#[ignore]` with a BLOCKED reason. Each one
//!    loads its fixture and then stops with `unimplemented!`. They carry the
//!    expected outcome and nothing else: this crate has no resolver, and this
//!    file does not invent a resolver API. A skipped test is not a PASS.
//!
//! `expected` has four parts: `refusal` (the whole read is refused, or
//! `null`), `governing`, `excluded` (each with a reason) and `typed_unknown`
//! (scoped unknowns, each naming the snapshots it covers). The labels are
//! test-local, not a product type. ARCH-0005 asks for no new broad status enum,
//! and none of these meanings matches an existing type exactly
//! (`GraphScopeCoverage` describes code-graph indexing, `FreshnessCaveatCode`
//! graph refresh). `SourceFreshness` is the candidate to reuse once stale or
//! inaccessible inputs get fixtures.
//!
//! `access_context` and `replay_fault` are test-only stand-ins: the first for
//! the existing access/read context, the second a fault switch. Neither is a
//! proposed contract shape for #40/#42.
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

/// Test-local exclusion reasons, each with the replayed state it requires.
/// See the module docs.
const EXCLUSION_STATES: [(&str, &str); 3] = [
    ("not_approved", "tracked"),
    ("superseded", "superseded"),
    ("rejected", "rejected"),
];

/// Test-local reasons for refusing the whole read. See the module docs.
const REFUSALS: [&str; 3] = ["invalid_replay", "as_of_not_supported", "partial_replay"];

/// Test-only replay fault switch values. See the module docs.
const REPLAY_FAULTS: [&str; 1] = ["partial"];

/// Every fixture this harness expects, so a deleted or renamed one fails.
const FIXTURES: [&str; 8] = [
    "as-of-is-not-supported",
    "conflicting-approvals-are-invalid-replay",
    "missing-check-record-is-scoped-unknown",
    "partial-replay-is-refused",
    "same-source-approved-twice-is-invalid-replay",
    "supersession-with-later-draft",
    "tracked-is-not-governing",
    "two-approved-non-conflicting",
];

/// Test-local typed-unknown labels. See the module docs. Labels for stale or
/// inaccessible inputs are added with the fixtures that need them, reusing
/// `SourceFreshness` where its meaning fits.
const TYPED_UNKNOWNS: [&str; 1] = ["missing_check_record"];

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
        if !record["conflict"].is_boolean() {
            return Err(format!("{stem}: check record `conflict` must be a boolean"));
        }
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

/// The query and the test-only access context name the fixture's Product, and
/// `replay_fault` is a known switch value. Any non-null `as_of` is accepted
/// here: it must expect a refusal, and its format is not decided (ARCH-0005).
fn check_context(stem: &str, doc: &Value) -> Check {
    let product_id = text(doc, "product_id", stem)?;
    let query = &doc["query"];
    let keys: BTreeSet<&str> = query
        .as_object()
        .ok_or_else(|| format!("{stem}: `query` must be an object"))?
        .keys()
        .map(String::as_str)
        .collect();
    if keys != BTreeSet::from(["as_of", "product_id"]) {
        return Err(format!(
            "{stem}: `query` holds exactly `product_id` and `as_of`"
        ));
    }
    if text(query, "product_id", stem)? != product_id {
        return Err(format!(
            "{stem}: query product_id must match the fixture product_id"
        ));
    }
    if text(&doc["access_context"], "product_id", stem)? != product_id {
        return Err(format!(
            "{stem}: access_context product_id must match the fixture product_id"
        ));
    }
    if let Some(fault) = doc.get("replay_fault") {
        if !fault.as_str().is_some_and(|f| REPLAY_FAULTS.contains(&f)) {
            return Err(format!("{stem}: unknown replay_fault"));
        }
    }
    Ok(())
}

/// A signoff's embedded check records are copies of stored records, compared
/// as whole records, and each one cites the signed-off domain's own digest.
fn check_signoff_records(stem: &str, doc: &Value) -> Check {
    let stored = list(doc, "check_records", stem)?;
    for snap in snapshots(doc)? {
        let signoff = &snap["signoff"];
        if signoff.is_null() {
            continue;
        }
        let addr = addr_of(snap)?;
        let own = text(signoff, "domain_digest", stem)?;
        for record in list(signoff, "check_records", stem)? {
            if !stored.contains(record) {
                return Err(format!(
                    "{stem}/{addr}: signoff check record is not among the stored check records"
                ));
            }
            if record["domain_digest"] != own && record["other_domain_digest"] != own {
                return Err(format!(
                    "{stem}/{addr}: a signoff check record must cite the signed-off digest"
                ));
            }
        }
    }
    Ok(())
}

/// Approved snapshots of different sources with the same digest cannot carry a
/// check record between them (ARCH-0003 needs two different digests). How such
/// a pair is approved and read is undecided (#40/#39), so no fixture may use it
/// yet and the harness gives it no outcome.
fn check_equal_digests(stem: &str, doc: &Value) -> Check {
    let mut seen: BTreeMap<&str, (&str, &str)> = BTreeMap::new(); // digest -> (addr, source)
    for snap in snapshots(doc)? {
        if text(snap, "replayed_state", stem)? != "approved" {
            continue;
        }
        let addr = addr_of(snap)?;
        let source = text(&snap["source_snapshot"], "source_uri", stem)?;
        let digest = text(&snap["governed_domain"], "source_digest", stem)?;
        if let Some((other, other_source)) = seen.insert(digest, (addr, source)) {
            if other_source != source {
                return Err(format!(
                    "{stem}: approved {other} and {addr} of different sources share a digest; that case is undecided (#40/#39) and has no fixture outcome yet"
                ));
            }
        }
    }
    Ok(())
}

/// The whole-read refusals a fixture's inputs allow, as `(reason, condition)`.
/// `required` holds conditions that must refuse the read; `optional` holds
/// conditions where a refusal and a scoped unknown are both acceptable (an
/// approved pair across sources with no check record: "scoped unknown/refusal",
/// #55 2026-11-20). A fixture may expect any listed reason; the harness sets no
/// order between them.
type RefusalOptions = (
    Vec<(&'static str, &'static str)>,
    Vec<(&'static str, &'static str)>,
);

fn refusal_options(stem: &str, doc: &Value) -> Result<RefusalOptions, String> {
    let mut required = Vec::new();
    let mut optional = Vec::new();
    if !doc["query"]["as_of"].is_null() {
        required.push(("as_of_not_supported", "a fixture with `as_of`"));
    }
    if doc.get("replay_fault").is_some() {
        required.push(("partial_replay", "a fixture with a partial replay"));
    }
    let mut approved: Vec<(&str, &str)> = Vec::new(); // (source_uri, digest)
    for snap in snapshots(doc)? {
        if text(snap, "replayed_state", stem)? == "approved" {
            approved.push((
                text(&snap["source_snapshot"], "source_uri", stem)?,
                text(&snap["governed_domain"], "source_digest", stem)?,
            ));
        }
    }
    let approved_digests: BTreeSet<&str> = approved.iter().map(|(_, d)| *d).collect();
    let mut cleared = BTreeSet::new();
    for record in list(doc, "check_records", stem)? {
        let pair = (
            text(record, "domain_digest", stem)?,
            text(record, "other_domain_digest", stem)?,
        );
        if record["conflict"] == false {
            cleared.insert(pair);
        } else if approved_digests.contains(pair.0) && approved_digests.contains(pair.1) {
            required.push((
                "invalid_replay",
                "conflicting approvals (a conflict:true record between two approved snapshots)",
            ));
        }
    }
    let mut sources = BTreeSet::new();
    if approved.iter().any(|(source, _)| !sources.insert(*source)) {
        required.push(("invalid_replay", "one source approved twice"));
    }
    let missing = approved.iter().enumerate().any(|(i, (source, digest))| {
        approved[i + 1..].iter().any(|(other_source, other)| {
            other_source != source
                && !cleared.contains(&((*digest).min(*other), (*digest).max(*other)))
        })
    });
    if missing {
        optional.push(("invalid_replay", "an approved pair with no check record"));
    }
    Ok((required, optional))
}

fn check_expected(stem: &str, doc: &Value) -> Check {
    if !text(doc, "status", stem)?.starts_with("BLOCKED") {
        return Err(format!(
            "{stem}: a fixture whose rows need the resolver is BLOCKED, never PASS"
        ));
    }
    let expected = &doc["expected"];
    let keys: BTreeSet<&str> = expected
        .as_object()
        .ok_or_else(|| format!("{stem}: `expected` must be an object"))?
        .keys()
        .map(String::as_str)
        .collect();
    if keys != BTreeSet::from(["excluded", "governing", "refusal", "typed_unknown"]) {
        return Err(format!(
            "{stem}: `expected` holds exactly `refusal`, `governing`, `excluded` and `typed_unknown`"
        ));
    }
    let all = snapshots(doc)?;
    let states: BTreeMap<&str, &Value> = all
        .iter()
        .map(|s| Ok((addr_of(s)?, s)))
        .collect::<Result<_, String>>()?;
    let state_of = |addr: &str| -> Result<&str, String> {
        let snap = states
            .get(addr)
            .ok_or_else(|| format!("{stem}: expected names an unknown snapshot {addr}"))?;
        text(snap, "replayed_state", stem)
    };
    let addr_text = |value: &'_ Value| -> Result<String, String> {
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{stem}: a snapshot address in `expected` must be a string"))
    };
    let governing = list(expected, "governing", stem)?;
    let excluded = list(expected, "excluded", stem)?;
    let unknowns = list(expected, "typed_unknown", stem)?;

    // Refusal: the whole read is refused exactly when the inputs call for it.
    let refusal = &expected["refusal"];
    let actual = if refusal.is_null() {
        None
    } else {
        if refusal.as_object().map(|o| o.len()) != Some(1) {
            return Err(format!("{stem}: a refusal holds exactly `reason`"));
        }
        let reason = text(refusal, "reason", stem)?;
        if !REFUSALS.contains(&reason) {
            return Err(format!("{stem}: refusal reason must be a test-local label"));
        }
        Some(reason)
    };
    // When a refusal is required, only the required reasons satisfy it; the
    // optional one (a missing pair) applies only when nothing is required.
    let (required, optional) = refusal_options(stem, doc)?;
    let conditions = if required.is_empty() {
        &optional
    } else {
        &required
    };
    let acceptable: Vec<&str> = conditions.iter().map(|(r, _)| *r).collect();
    match (required.first(), actual) {
        (Some((reason, why)), got) if !got.is_some_and(|r| acceptable.contains(&r)) => {
            if required.len() == 1 {
                return Err(format!("{stem}: {why} must expect refusal {reason}"));
            }
            let listed: Vec<String> = required.iter().map(|(r, w)| format!("{r} ({w})")).collect();
            return Err(format!(
                "{stem}: the read must be refused with one of: {}",
                listed.join("; ")
            ));
        }
        (None, Some(reason)) if !acceptable.contains(&reason) => {
            return Err(format!(
                "{stem}: refusal {reason} has no matching fixture condition"
            ));
        }
        _ => {}
    }
    if actual.is_some() {
        if !(governing.is_empty() && excluded.is_empty() && unknowns.is_empty()) {
            return Err(format!(
                "{stem}: a refusal lists nothing as governing, excluded or unknown"
            ));
        }
        return Ok(());
    }

    // Stored conflict:false pairs, and the approved snapshots of other sources
    // that a given snapshot must be checked against (ARCH-0004).
    let digest_of = |addr: &str| text(&states[addr]["governed_domain"], "source_digest", stem);
    let source_of = |addr: &str| text(&states[addr]["source_snapshot"], "source_uri", stem);
    let mut cleared: BTreeSet<(&str, &str)> = BTreeSet::new();
    for record in list(doc, "check_records", stem)? {
        if record["conflict"] == false {
            cleared.insert((
                text(record, "domain_digest", stem)?,
                text(record, "other_domain_digest", stem)?,
            ));
        }
    }
    let is_cleared = |x: &str, y: &str| cleared.contains(&(x.min(y), x.max(y)));
    let uncleared_other = |addr: &str| -> Result<Option<String>, String> {
        let (digest, source) = (digest_of(addr)?, source_of(addr)?);
        for (other, snap) in &states {
            if *other == addr || text(snap, "replayed_state", stem)? != "approved" {
                continue;
            }
            if source_of(other)? != source && !is_cleared(digest, digest_of(other)?) {
                return Ok(Some(other.to_string()));
            }
        }
        Ok(None)
    };

    // Accounting: every snapshot appears exactly once across the three lists,
    // under a label its state allows.
    let mut accounted: BTreeSet<String> = BTreeSet::new();
    let mut account = |addr: String| -> Check {
        if accounted.insert(addr.clone()) {
            Ok(())
        } else {
            Err(format!("{stem}: {addr} is listed more than once"))
        }
    };
    for value in governing {
        let addr = addr_text(value)?;
        if state_of(&addr)? != "approved" {
            return Err(format!(
                "{stem}: only an approved snapshot may govern ({addr})"
            ));
        }
        if let Some(other) = uncleared_other(&addr)? {
            return Err(format!(
                "{stem}: governing {addr} has no conflict:false check record with approved {other} of another source"
            ));
        }
        account(addr)?;
    }
    for entry in excluded {
        let reason = text(entry, "reason", stem)?;
        let required_state = EXCLUSION_STATES
            .iter()
            .find(|(r, _)| *r == reason)
            .map(|(_, s)| *s)
            .ok_or_else(|| format!("{stem}: exclusion reason must be a test-local label"))?;
        let addr = addr_text(&entry["snapshot_addr"])?;
        if state_of(&addr)? != required_state {
            return Err(format!(
                "{stem}: excluded {addr} as {reason} needs replayed_state {required_state}"
            ));
        }
        account(addr)?;
    }
    for unknown in unknowns {
        if !TYPED_UNKNOWNS.contains(&text(unknown, "label", stem)?) {
            return Err(format!("{stem}: typed_unknown must be a test-local label"));
        }
        let addrs = list(unknown, "snapshot_addrs", stem)?;
        if addrs.is_empty() {
            return Err(format!(
                "{stem}: a typed unknown names at least one snapshot"
            ));
        }
        for value in addrs {
            let addr = addr_text(value)?;
            // `missing_check_record` is the only unknown label today. Both
            // ends of a missing pair are named: fixtures do not record which
            // approval came later, so neither end may govern (fail closed).
            if state_of(&addr)? != "approved" || uncleared_other(&addr)?.is_none() {
                return Err(format!(
                    "{stem}: missing_check_record names {addr}, which is not an approved snapshot missing a check record"
                ));
            }
            account(addr)?;
        }
    }
    if accounted.len() != all.len() {
        return Err(format!(
            "{stem}: every snapshot is governing, excluded or in a typed unknown"
        ));
    }
    Ok(())
}

/// Every rule, in the order the negative tests rely on.
fn check_all(stem: &str, doc: &Value) -> Check {
    check_synthetic(stem, doc)?;
    check_domains(stem, doc)?;
    check_replay(stem, doc)?;
    check_check_records(stem, doc)?;
    check_context(stem, doc)?;
    check_signoff_records(stem, doc)?;
    check_equal_digests(stem, doc)?;
    check_expected(stem, doc)
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
    let names: Vec<&str> = fixtures.iter().map(|(stem, _)| stem.as_str()).collect();
    assert_eq!(
        names, FIXTURES,
        "fixture set changed; update FIXTURES and the README"
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

#[test]
fn every_fixture_passes_all_rules_in_order() {
    for (stem, doc) in load_fixtures() {
        check_all(&stem, &doc).unwrap();
    }
}

#[test]
fn as_of_fixture_reuses_the_non_conflicting_inputs_unchanged() {
    let base = fixture("two-approved-non-conflicting");
    let as_of = fixture("as-of-is-not-supported");
    assert_eq!(as_of["snapshots"], base["snapshots"]);
    assert_eq!(as_of["check_records"], base["check_records"]);
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

// ── Negative regressions: one per rule ──────────────────────────────────
//
// Each test changes one fixture just enough to break one rule, runs every
// rule in order (`check_all`), and asserts that rule's own message, so an
// earlier rule cannot make the test pass for the wrong reason.

fn rejected_with(doc: &Value, message: &str) {
    let stem = doc["case_id"].as_str().expect("case_id");
    let err = check_all(stem, doc).expect_err("the mutated fixture must be rejected");
    assert!(err.contains(message), "unexpected rejection: {err}");
}

fn drop_records_citing(records: &mut Value, digest: &Value) {
    records
        .as_array_mut()
        .expect("records")
        .retain(|r| r["domain_digest"] != *digest && r["other_domain_digest"] != *digest);
}

// Synthetic data, digests, replay and check-record payloads.

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
    rejected_with(&doc, "source_digest must recompute");
}

#[test]
fn a_check_record_conflict_must_be_a_boolean() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["check_records"][0]["conflict"] = Value::from("false");
    rejected_with(&doc, "check record `conflict` must be a boolean");
}

// Query, access context and signoff records.

#[test]
fn query_product_must_match_the_fixture_product() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["query"]["product_id"] = Value::from("prod_synthetic_other");
    rejected_with(&doc, "query product_id must match the fixture product_id");
}

#[test]
fn access_context_product_must_match_the_fixture_product() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["access_context"]["product_id"] = Value::from("prod_synthetic_other");
    rejected_with(
        &doc,
        "access_context product_id must match the fixture product_id",
    );
}

#[test]
fn query_holds_only_product_and_as_of() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["query"]["scope"] = Value::from("synthetic-scope-a");
    rejected_with(&doc, "`query` holds exactly `product_id` and `as_of`");
}

#[test]
fn replay_fault_must_be_a_known_value() {
    let mut doc = fixture("partial-replay-is-refused");
    doc["replay_fault"] = Value::from("total");
    rejected_with(&doc, "unknown replay_fault");
}

#[test]
fn signoff_check_record_must_be_a_stored_record() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["check_records"] = serde_json::json!([]);
    rejected_with(
        &doc,
        "signoff check record is not among the stored check records",
    );
}

#[test]
fn signoff_check_record_must_cite_the_signed_off_digest() {
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    // A's signoff carries the B-C record, which does not name A.
    let bc = doc["snapshots"][2]["signoff"]["check_records"][1].clone();
    doc["snapshots"][0]["signoff"]["check_records"] = serde_json::json!([bc]);
    rejected_with(
        &doc,
        "a signoff check record must cite the signed-off digest",
    );
}

// Refusals.

#[test]
fn expected_holds_exactly_the_four_parts() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["unresolved"] = Value::from(true);
    rejected_with(&doc, "`expected` holds exactly");
}

#[test]
fn refusal_reason_must_be_a_test_local_label() {
    let mut doc = fixture("as-of-is-not-supported");
    doc["expected"]["refusal"] = serde_json::json!({"reason": "historical_lookup"});
    rejected_with(&doc, "refusal reason must be a test-local label");
}

#[test]
fn a_refusal_holds_only_a_reason() {
    let mut doc = fixture("as-of-is-not-supported");
    doc["expected"]["refusal"]["note"] = Value::from("extra");
    rejected_with(&doc, "a refusal holds exactly `reason`");
}

#[test]
fn a_missing_pair_does_not_satisfy_a_required_refusal() {
    // An `as_of` read must be refused as `as_of_not_supported`; a missing
    // check record in current state is not a reason to answer it.
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    doc["query"]["as_of"] = Value::from("2026-01-11T00:00:00Z");
    doc["expected"] = serde_json::json!({
        "refusal": {"reason": "invalid_replay"},
        "governing": [], "excluded": [], "typed_unknown": []
    });
    rejected_with(
        &doc,
        "a fixture with `as_of` must expect refusal as_of_not_supported",
    );
}

#[test]
fn fixture_with_as_of_must_expect_refusal() {
    let mut doc = fixture("as-of-is-not-supported");
    doc["expected"]["refusal"] = Value::Null;
    doc["expected"]["governing"] = serde_json::json!(["snap_a", "snap_b"]);
    rejected_with(
        &doc,
        "a fixture with `as_of` must expect refusal as_of_not_supported",
    );
}

#[test]
fn fixture_with_partial_replay_must_expect_refusal_not_an_empty_read() {
    let mut doc = fixture("partial-replay-is-refused");
    doc["expected"]["refusal"] = Value::Null;
    doc["expected"]["excluded"] =
        serde_json::json!([{"snapshot_addr": "snap_t1", "reason": "not_approved"}]);
    rejected_with(
        &doc,
        "a fixture with a partial replay must expect refusal partial_replay",
    );
}

#[test]
fn fixture_with_conflicting_approvals_must_expect_refusal() {
    let mut doc = fixture("conflicting-approvals-are-invalid-replay");
    doc["expected"]["refusal"] = Value::Null;
    doc["expected"]["governing"] = serde_json::json!(["snap_a", "snap_b"]);
    rejected_with(
        &doc,
        "conflicting approvals (a conflict:true record between two approved snapshots) must expect refusal invalid_replay",
    );
}

#[test]
fn fixture_with_one_source_approved_twice_must_expect_refusal() {
    let mut doc = fixture("same-source-approved-twice-is-invalid-replay");
    doc["expected"]["refusal"] = Value::Null;
    doc["expected"]["governing"] = serde_json::json!(["snap_b"]);
    rejected_with(
        &doc,
        "one source approved twice must expect refusal invalid_replay",
    );
}

#[test]
fn refusal_needs_a_matching_fixture_condition() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["expected"] = serde_json::json!({
        "refusal": {"reason": "invalid_replay"},
        "governing": [], "excluded": [], "typed_unknown": []
    });
    rejected_with(
        &doc,
        "refusal invalid_replay has no matching fixture condition",
    );
}

#[test]
fn a_missing_check_record_may_also_refuse_the_read() {
    // "scoped unknown/refusal": both outcomes are acceptable for a missing pair.
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    doc["expected"] = serde_json::json!({
        "refusal": {"reason": "invalid_replay"},
        "governing": [], "excluded": [], "typed_unknown": []
    });
    check_all("missing-check-record-is-scoped-unknown", &doc).expect("refusal is also allowed");
}

#[test]
fn approved_sources_with_equal_digests_have_no_outcome_yet() {
    let mut doc = fixture("two-approved-non-conflicting");
    let a = doc["snapshots"][0]["governed_domain"].clone();
    let b = &mut doc["snapshots"][1];
    for key in [
        "claims",
        "atoms",
        "claim_assertions",
        "relations",
        "source_digest",
    ] {
        b["governed_domain"][key] = a[key].clone();
    }
    b["signoff"]["domain_digest"] = a["source_digest"].clone();
    b["signoff"]["check_records"] = serde_json::json!([]);
    doc["check_records"] = serde_json::json!([]);
    rejected_with(&doc, "of different sources share a digest");
}

#[test]
fn a_refusal_lists_nothing() {
    let mut doc = fixture("as-of-is-not-supported");
    doc["expected"]["governing"] = serde_json::json!(["snap_a"]);
    rejected_with(
        &doc,
        "a refusal lists nothing as governing, excluded or unknown",
    );
}

// Accounting and labels.

#[test]
fn expected_must_not_name_an_unknown_snapshot() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["excluded"] =
        serde_json::json!([{"snapshot_addr": "snap_zz", "reason": "not_approved"}]);
    rejected_with(&doc, "expected names an unknown snapshot snap_zz");
}

#[test]
fn a_tracked_snapshot_listed_as_governing_is_rejected() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["governing"] = serde_json::json!(["snap_t1"]);
    doc["expected"]["excluded"] = serde_json::json!([]);
    rejected_with(&doc, "only an approved snapshot may govern");
}

#[test]
fn an_exclusion_reason_must_be_a_test_local_label() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["excluded"][0]["reason"] = Value::from("draft");
    rejected_with(&doc, "exclusion reason must be a test-local label");
}

#[test]
fn an_exclusion_reason_must_match_the_replayed_state() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["excluded"][0]["reason"] = Value::from("superseded");
    rejected_with(
        &doc,
        "excluded snap_t1 as superseded needs replayed_state superseded",
    );
}

#[test]
fn an_approved_snapshot_cannot_be_excluded() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["expected"]["governing"] = serde_json::json!(["snap_a"]);
    doc["expected"]["excluded"] =
        serde_json::json!([{"snapshot_addr": "snap_b", "reason": "not_approved"}]);
    rejected_with(
        &doc,
        "excluded snap_b as not_approved needs replayed_state tracked",
    );
}

#[test]
fn a_typed_unknown_label_must_be_test_local() {
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    doc["expected"]["typed_unknown"][0]["label"] = Value::from("stale_source");
    rejected_with(&doc, "typed_unknown must be a test-local label");
}

#[test]
fn a_typed_unknown_names_at_least_one_snapshot() {
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    doc["expected"]["typed_unknown"]
        .as_array_mut()
        .expect("unknowns")
        .push(serde_json::json!({"label": "missing_check_record", "snapshot_addrs": []}));
    rejected_with(&doc, "a typed unknown names at least one snapshot");
}

#[test]
fn missing_check_record_cannot_name_a_cleared_snapshot() {
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    doc["expected"]["governing"] = serde_json::json!([]);
    doc["expected"]["typed_unknown"][0]["snapshot_addrs"] =
        serde_json::json!(["snap_a", "snap_b", "snap_c"]);
    rejected_with(&doc, "missing_check_record names snap_c");
}

#[test]
fn missing_check_record_cannot_name_a_tracked_snapshot() {
    let mut doc = fixture("tracked-is-not-governing");
    doc["expected"]["excluded"] = serde_json::json!([]);
    doc["expected"]["typed_unknown"] =
        serde_json::json!([{"label": "missing_check_record", "snapshot_addrs": ["snap_t1"]}]);
    rejected_with(&doc, "missing_check_record names snap_t1");
}

#[test]
fn a_snapshot_is_accounted_for_exactly_once() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["expected"]["governing"] = serde_json::json!(["snap_a", "snap_b", "snap_a"]);
    rejected_with(&doc, "snap_a is listed more than once");
}

#[test]
fn every_snapshot_must_be_accounted_for() {
    let mut doc = fixture("supersession-with-later-draft");
    doc["expected"]["excluded"]
        .as_array_mut()
        .expect("excluded")
        .pop();
    rejected_with(
        &doc,
        "every snapshot is governing, excluded or in a typed unknown",
    );
}

// Pair coverage (ARCH-0004).

#[test]
fn a_governing_snapshot_needs_a_clear_check_with_every_other_approved_source() {
    let mut doc = fixture("missing-check-record-is-scoped-unknown");
    let a = doc["snapshots"][0]["governed_domain"]["source_digest"].clone();
    drop_records_citing(&mut doc["check_records"], &a);
    drop_records_citing(&mut doc["snapshots"][2]["signoff"]["check_records"], &a);
    rejected_with(
        &doc,
        "governing snap_c has no conflict:false check record with approved snap_a",
    );
}

#[test]
fn two_approved_sources_without_any_check_record_are_rejected() {
    let mut doc = fixture("two-approved-non-conflicting");
    doc["check_records"] = serde_json::json!([]);
    doc["snapshots"][1]["signoff"]["check_records"] = serde_json::json!([]);
    rejected_with(&doc, "has no conflict:false check record");
}

// ── Resolver rows: BLOCKED until the resolver lands (ARCH-0005) ─────────
//
// Each test loads its fixture so a deleted or renamed fixture still fails
// loudly, then stops. When a resolver exists, call it here with the fixture's
// `snapshots`, `check_records`, `access_context`, `replay_fault` (if present)
// and `query`, and compare with
// `expected`. A skipped test is not a PASS.

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn tracked_snapshot_never_governs() {
    let doc = fixture("tracked-is-not-governing");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn supersession_resolves_to_the_approved_successor_and_a_later_draft_never_wins() {
    let doc = fixture("supersession-with-later-draft");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn two_approved_non_conflicting_sources_both_govern() {
    let doc = fixture("two-approved-non-conflicting");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn conflicting_approvals_refuse_the_read_as_invalid_replay() {
    let doc = fixture("conflicting-approvals-are-invalid-replay");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn a_missing_check_record_is_a_scoped_unknown_never_no_conflict() {
    let doc = fixture("missing-check-record-is-scoped-unknown");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42; as_of is Not decided in ARCH-0005 and needs decision-owner sign-off)"]
fn a_historical_as_of_read_is_refused() {
    let doc = fixture("as-of-is-not-supported");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn a_partial_replay_is_refused_never_an_empty_read() {
    let doc = fixture("partial-replay-is-refused");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}

#[test]
#[ignore = "BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)"]
fn one_source_approved_twice_refuses_the_read_never_picks_by_date() {
    let doc = fixture("same-source-approved-twice-is-invalid-replay");
    let _expected = &doc["expected"];
    unimplemented!("wire the resolver here once ARCH-0005 has an implementation");
}
