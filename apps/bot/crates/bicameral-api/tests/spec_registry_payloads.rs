use bicameral_api::spec_registry::{
    RejectSnapshot, ResolveConflict, SupersedeSnapshot, TrackSource, UntrackSource,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

fn payload() -> Value {
    json!({
        "source_uri": "file:spec.md",
        "snapshot_addr": "sha256:snapshot",
        "person_id": "person-1",
        "daemon_id": "daemon-1"
    })
}

#[test]
fn track_source_preserves_frozen_payload() {
    let payload = payload();
    let track: TrackSource = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(serde_json::to_value(track).unwrap(), payload);
}

fn assert_frozen_fields<T: DeserializeOwned + Serialize>(schema: &str, valid: Value) {
    let schema: Value = serde_json::from_str(schema).unwrap();
    let parsed: T = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), valid);
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"].as_object().unwrap().len(),
        valid.as_object().unwrap().len()
    );
    for field in schema["required"].as_array().unwrap() {
        let field = field.as_str().unwrap();
        assert_eq!(schema["properties"][field]["minLength"], 1);
        for invalid in [json!(""), Value::Null, json!(7), json!(false)] {
            let mut bad = valid.clone();
            bad[field] = invalid;
            assert!(serde_json::from_value::<T>(bad).is_err(), "{field}");
        }
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<T>(missing).is_err(), "{field}");
    }
    let mut extra = valid;
    extra["role"] = json!("owner");
    assert!(serde_json::from_value::<T>(extra).is_err());
}

#[test]
fn track_source_enforces_frozen_fields() {
    assert_frozen_fields::<TrackSource>(
        include_str!("../../../protocol/spec-registry/track-source.schema.json"),
        payload(),
    );
}

#[test]
fn untrack_source_enforces_frozen_fields() {
    assert_frozen_fields::<UntrackSource>(
        include_str!("../../../protocol/spec-registry/untrack-source.schema.json"),
        payload(),
    );
}

#[test]
fn reject_snapshot_enforces_frozen_fields() {
    assert_frozen_fields::<RejectSnapshot>(
        include_str!("../../../protocol/spec-registry/reject-snapshot.schema.json"),
        payload(),
    );
}

#[test]
fn supersede_snapshot_requires_successor_and_frozen_fields() {
    let mut valid = payload();
    valid["successor_snapshot_addr"] = json!("sha256:successor");
    assert_frozen_fields::<SupersedeSnapshot>(
        include_str!("../../../protocol/spec-registry/supersede-snapshot.schema.json"),
        valid,
    );
}

fn resolution(states: [&str; 2]) -> Value {
    json!({
        "person_id": "owner-1",
        "daemon_id": "daemon-1",
        "members": [
            {"source_uri": "file:a.md", "snapshot_addr": "sha256:a", "state": states[0]},
            {"source_uri": "file:b.md", "snapshot_addr": "sha256:b", "state": states[1]}
        ]
    })
}

#[test]
fn resolve_conflict_preserves_frozen_bundle() {
    for states in [
        ["approved", "rejected"],
        ["superseded", "approved"],
        ["rejected", "rejected"],
    ] {
        let valid = resolution(states);
        let decoded: ResolveConflict = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), valid);
    }
}

#[test]
fn resolve_conflict_rejects_invalid_member_counts_and_outcomes() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../protocol/spec-registry/resolve-conflict.schema.json"
    ))
    .unwrap();
    assert_eq!(schema["properties"]["members"]["minItems"], 2);
    let valid = resolution(["approved", "rejected"]);
    for count in 0..2 {
        let mut bad = valid.clone();
        bad["members"].as_array_mut().unwrap().truncate(count);
        assert!(serde_json::from_value::<ResolveConflict>(bad).is_err());
    }
    for states in [
        ["approved", "approved"],
        ["superseded", "rejected"],
        ["superseded", "superseded"],
    ] {
        assert!(serde_json::from_value::<ResolveConflict>(resolution(states)).is_err());
    }
}

#[test]
fn resolve_conflict_rejects_malformed_fields_and_member_authority() {
    let valid = resolution(["approved", "rejected"]);
    for pointer in [
        "/person_id",
        "/daemon_id",
        "/members",
        "/members/0/source_uri",
        "/members/0/snapshot_addr",
        "/members/0/state",
    ] {
        for invalid in [json!(""), Value::Null, json!(7), json!(false)] {
            let mut bad = valid.clone();
            *bad.pointer_mut(pointer).unwrap() = invalid;
            assert!(
                serde_json::from_value::<ResolveConflict>(bad).is_err(),
                "{pointer}"
            );
        }
    }
    for (pointer, fields) in [
        ("", vec!["person_id", "daemon_id", "members"]),
        ("/members/0", vec!["source_uri", "snapshot_addr", "state"]),
    ] {
        for field in fields {
            let mut bad = valid.clone();
            bad.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(
                serde_json::from_value::<ResolveConflict>(bad).is_err(),
                "{pointer}/{field}"
            );
        }
    }
    for field in ["person_id", "daemon_id", "role"] {
        let mut bad = valid.clone();
        bad["members"][0][field] = json!("owner");
        assert!(serde_json::from_value::<ResolveConflict>(bad).is_err());
    }
    for state in ["tracked", "no_standing", "proposed", "Approved"] {
        let mut bad = valid.clone();
        bad["members"][0]["state"] = json!(state);
        assert!(serde_json::from_value::<ResolveConflict>(bad).is_err());
    }
    let mut bad = valid;
    bad["role"] = json!("owner");
    assert!(serde_json::from_value::<ResolveConflict>(bad).is_err());
}

#[test]
fn registry_untrack_does_not_reinterpret_legacy_decision_events() {
    use bicameral_api::event_store::{EventType, UntrackSourcePayload};

    let legacy = json!({
        "decision_id": "123e4567-e89b-42d3-a456-426614174000",
        "reason": "explicit_untrack"
    });
    let decoded: UntrackSourcePayload = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
    assert_eq!(
        serde_json::to_value(EventType::UntrackSource).unwrap(),
        "untrack_source"
    );
    assert!(serde_json::from_value::<UntrackSource>(legacy.clone()).is_err());
    assert!(serde_json::from_value::<UntrackSourcePayload>(payload()).is_err());

    let mut mixed = payload();
    mixed
        .as_object_mut()
        .unwrap()
        .extend(legacy.as_object().unwrap().clone());
    assert!(serde_json::from_value::<UntrackSource>(mixed).is_err());
}

mod domain_payloads {
    use bicameral_api::spec_registry::{
        encode_checked, ApprovePolicy, CheckRecord, GovernedDomain, Signoff, SourceSnapshot,
        SpecSource,
    };
    use serde::{de::DeserializeOwned, Serialize};
    use serde_json::{json, Value};

    fn round_trips<T: DeserializeOwned + Serialize>(value: &Value) -> bool {
        match serde_json::from_value::<T>(value.clone()) {
            Ok(parsed) => {
                assert_eq!(&serde_json::to_value(parsed).unwrap(), value);
                true
            }
            Err(_) => false,
        }
    }

    fn with(mut value: Value, field: &str, replacement: Value) -> Value {
        value[field] = replacement;
        value
    }

    fn without(mut value: Value, field: &str) -> Value {
        value.as_object_mut().unwrap().remove(field).unwrap();
        value
    }

    /// Every schema property is a field the type decodes, and no other field is accepted.
    fn assert_schema_fields<T: DeserializeOwned + Serialize>(schema: &str, full: &Value) {
        let schema: Value = serde_json::from_str(schema).unwrap();
        assert_eq!(schema["additionalProperties"], false);
        let properties: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        for property in &properties {
            assert!(full.get(property.as_str()).is_some() || *property == "extract_error");
        }
        assert!(round_trips::<T>(full));
        assert!(!round_trips::<T>(&with(full.clone(), "unknown", json!(1))));
        for field in schema["required"].as_array().unwrap() {
            assert!(!round_trips::<T>(&without(
                full.clone(),
                field.as_str().unwrap()
            )));
        }
    }

    fn captured_domain() -> Value {
        json!({
            "product_id": "product-1",
            "snapshot_addr": "sha256:snapshot",
            "claims": [], "atoms": [], "claim_assertions": [], "relations": []
        })
    }

    fn completed_domain() -> Value {
        json!({
            "product_id": "product-1",
            "snapshot_addr": "sha256:snapshot",
            "source_digest": "digest",
            "route": "openai_compatible",
            "model_id": "model",
            "prompt_id": "prompt",
            "prompt_digest": "prompt-digest",
            "claims": [{"claim_id": "c1", "revision": 0, "context_statement": ""}],
            "atoms": [{"atom_id": "a1", "atom_kind": "kind", "semantic_key": "key"}],
            "claim_assertions": [
                {"assertion_id": "s1", "claim_id": "c1", "atom_id": "a1", "polarity": "positive"}
            ],
            "relations": [{
                "relation_id": "r1", "relation_kind": "kind",
                "from_atom_id": "a1", "to_atom_id": "a1", "supporting_claim_ids": ["c1"]
            }]
        })
    }

    const COMPLETION: [&str; 5] = [
        "source_digest",
        "route",
        "model_id",
        "prompt_id",
        "prompt_digest",
    ];

    #[test]
    fn governed_domain_matches_the_frozen_schema() {
        assert_schema_fields::<GovernedDomain>(
            include_str!("../../../protocol/spec-registry/governed-domain.schema.json"),
            &completed_domain(),
        );
        assert!(round_trips::<GovernedDomain>(&captured_domain()));
    }

    #[test]
    fn completed_extract_metadata_is_all_or_none() {
        let mut empty_completion = captured_domain();
        for field in COMPLETION {
            empty_completion[field] = completed_domain()[field].clone();
        }
        assert!(
            round_trips::<GovernedDomain>(&empty_completion),
            "zero-claim completion"
        );
        for field in COMPLETION {
            assert!(
                !round_trips::<GovernedDomain>(&without(completed_domain(), field)),
                "{field}"
            );
            assert!(!round_trips::<GovernedDomain>(&without(
                empty_completion.clone(),
                field
            )));
            for invalid in [json!(""), Value::Null, json!(7)] {
                assert!(!round_trips::<GovernedDomain>(&with(
                    completed_domain(),
                    field,
                    invalid
                )));
            }
        }
        assert!(!round_trips::<GovernedDomain>(&with(
            completed_domain(),
            "route",
            json!("byok")
        )));
        let mut claims_without_completion = completed_domain();
        for field in COMPLETION {
            claims_without_completion = without(claims_without_completion, field);
        }
        assert!(!round_trips::<GovernedDomain>(&claims_without_completion));
    }

    #[test]
    fn extract_error_excludes_a_completed_extract() {
        let error = json!({"reason": "model timeout", "failed_at": "2026-11-17T12:00:00Z"});
        let failed = with(captured_domain(), "extract_error", error.clone());
        assert!(round_trips::<GovernedDomain>(&failed));
        assert!(!round_trips::<GovernedDomain>(&with(
            completed_domain(),
            "extract_error",
            error
        )));
        for field in COMPLETION {
            let partial = with(failed.clone(), field, completed_domain()[field].clone());
            assert!(!round_trips::<GovernedDomain>(&partial), "{field}");
        }
        for bad in [
            Value::Null,
            json!({"reason": "", "failed_at": "2026-11-17T12:00:00Z"}),
            json!({"reason": "x", "failed_at": "yesterday"}),
            json!({"reason": "x"}),
            json!({"reason": "x", "failed_at": "2026-11-17T12:00:00Z", "retry": true}),
        ] {
            assert!(!round_trips::<GovernedDomain>(&with(
                captured_domain(),
                "extract_error",
                bad
            )));
        }
    }

    #[test]
    fn claim_set_rows_reject_unknown_and_empty_identities() {
        for (list, field) in [
            ("claims", "claim_id"),
            ("atoms", "semantic_key"),
            ("claim_assertions", "polarity"),
            ("relations", "to_atom_id"),
        ] {
            let mut empty = completed_domain();
            empty[list][0][field] = json!("");
            assert!(!round_trips::<GovernedDomain>(&empty), "{list}.{field}");
            let mut extra = completed_domain();
            extra[list][0]["atom_ids"] = json!([]);
            assert!(!round_trips::<GovernedDomain>(&extra), "{list}");
        }
        let mut negative = completed_domain();
        negative["claims"][0]["revision"] = json!(-1);
        assert!(!round_trips::<GovernedDomain>(&negative));
        let mut empty_support = completed_domain();
        empty_support["relations"][0]["supporting_claim_ids"] = json!([""]);
        assert!(!round_trips::<GovernedDomain>(&empty_support));
    }

    #[test]
    fn claim_set_digest_follows_arch_0003() {
        let captured: GovernedDomain = serde_json::from_value(captured_domain()).unwrap();
        assert_eq!(
            captured.claim_set_digest(),
            "db4cd0ee0b2a1dbdbe2e0aea7ae0d7564d8ca2fb47d5b5ab458eb22c1114cbd9"
        );

        let completed: GovernedDomain = serde_json::from_value(completed_domain()).unwrap();
        let digest = completed.claim_set_digest();
        assert_ne!(digest, captured.claim_set_digest());
        // Provenance is outside the hash; stored array order is inside it.
        let other_route = with(completed_domain(), "model_id", json!("another-model"));
        let other_route: GovernedDomain = serde_json::from_value(other_route).unwrap();
        assert_eq!(other_route.claim_set_digest(), digest);
        let mut reordered = completed_domain();
        let second = json!({"claim_id": "c0", "revision": 1, "context_statement": "x"});
        reordered["claims"]
            .as_array_mut()
            .unwrap()
            .push(second.clone());
        let appended: GovernedDomain = serde_json::from_value(reordered.clone()).unwrap();
        reordered["claims"].as_array_mut().unwrap().reverse();
        let reversed: GovernedDomain = serde_json::from_value(reordered).unwrap();
        assert_ne!(appended.claim_set_digest(), reversed.claim_set_digest());
    }

    /// The independently written authority-resolution fixtures store digests of the same rule.
    #[test]
    fn claim_set_digest_agrees_with_the_authority_resolution_fixtures() {
        let mut compared = 0;
        for fixture in [
            include_str!("../../../protocol/authority-resolution/fixtures/tracked-is-not-governing.json"),
            include_str!("../../../protocol/authority-resolution/fixtures/supersession-with-later-draft.json"),
        ] {
            let fixture: Value = serde_json::from_str(fixture).unwrap();
            for snapshot in fixture["snapshots"].as_array().unwrap() {
                let stored = snapshot["governed_domain"].clone();
                if stored.get("source_digest").is_none() {
                    continue;
                }
                let domain: GovernedDomain = serde_json::from_value(stored).unwrap();
                assert_eq!(Some(domain.claim_set_digest()), domain.source_digest);
                compared += 1;
            }
        }
        assert!(compared >= 4, "compared {compared}");
    }

    fn check_record() -> Value {
        json!({"domain_digest": "aa", "other_domain_digest": "ab", "conflict": true, "daemon_id": "daemon-1"})
    }

    #[test]
    fn check_record_is_one_ordered_pair_with_a_boolean() {
        assert_schema_fields::<CheckRecord>(
            include_str!("../../../protocol/spec-registry/check-record.schema.json"),
            &check_record(),
        );
        assert!(round_trips::<CheckRecord>(&with(
            check_record(),
            "conflict",
            json!(false)
        )));
        assert!(!round_trips::<CheckRecord>(&with(
            check_record(),
            "other_domain_digest",
            json!("aa")
        )));
        assert!(!round_trips::<CheckRecord>(&with(
            check_record(),
            "other_domain_digest",
            json!("a")
        )));
        for invalid in [Value::Null, json!("false"), json!(0)] {
            assert!(!round_trips::<CheckRecord>(&with(
                check_record(),
                "conflict",
                invalid
            )));
        }
        for field in ["domain_digest", "other_domain_digest", "daemon_id"] {
            assert!(!round_trips::<CheckRecord>(&with(
                check_record(),
                field,
                json!("")
            )));
        }
    }

    fn manual_signoff() -> Value {
        json!({
            "snapshot_addr": "sha256:snapshot", "domain_digest": "aa", "daemon_id": "daemon-1",
            "actor": "manual", "role": "member", "person_id": "person-1",
            "check_records": [check_record()]
        })
    }

    #[test]
    fn signoff_binds_identity_to_the_actor_kind() {
        assert_schema_fields::<Signoff>(
            include_str!("../../../protocol/spec-registry/signoff.schema.json"),
            &manual_signoff(),
        );
        let policy = without(without(manual_signoff(), "role"), "person_id");
        let policy = with(policy, "actor", json!("policy"));
        assert!(round_trips::<Signoff>(&with(
            policy.clone(),
            "check_records",
            json!([])
        )));
        assert!(!round_trips::<Signoff>(&without(manual_signoff(), "role")));
        assert!(!round_trips::<Signoff>(&without(
            manual_signoff(),
            "person_id"
        )));
        assert!(!round_trips::<Signoff>(&with(
            manual_signoff(),
            "person_id",
            json!("")
        )));
        assert!(!round_trips::<Signoff>(&with(
            manual_signoff(),
            "role",
            Value::Null
        )));
        assert!(!round_trips::<Signoff>(&with(
            manual_signoff(),
            "role",
            json!("admin")
        )));
        assert!(!round_trips::<Signoff>(&with(
            manual_signoff(),
            "actor",
            json!("policy")
        )));
        assert!(!round_trips::<Signoff>(&with(
            policy.clone(),
            "role",
            json!("owner")
        )));
        assert!(!round_trips::<Signoff>(&with(
            policy,
            "person_id",
            json!("person-1")
        )));
        let unordered = json!([with(check_record(), "other_domain_digest", json!("a"))]);
        assert!(!round_trips::<Signoff>(&with(
            manual_signoff(),
            "check_records",
            unordered
        )));
    }

    fn cell(role: &str, in_conflict: bool, allow: bool) -> Value {
        json!({"role": role, "in_conflict": in_conflict, "allow": allow})
    }

    fn policy(cells: Vec<Value>) -> Value {
        json!({"auto_approve": true, "approve_permission": cells})
    }

    #[test]
    fn approve_policy_covers_each_cell_once_and_never_allows_conflict() {
        let alpha_default = policy(vec![
            cell("owner", false, true),
            cell("owner", true, false),
            cell("member", false, true),
            cell("member", true, false),
        ]);
        assert_schema_fields::<ApprovePolicy>(
            include_str!("../../../protocol/spec-registry/approve-policy.schema.json"),
            &alpha_default,
        );
        let mut any_order = alpha_default["approve_permission"]
            .as_array()
            .unwrap()
            .clone();
        any_order.reverse();
        any_order[1]["allow"] = json!(false);
        assert!(round_trips::<ApprovePolicy>(&policy(any_order)));

        let cells = |index: usize, replacement: Option<Value>| {
            let mut cells = alpha_default["approve_permission"]
                .as_array()
                .unwrap()
                .clone();
            match replacement {
                Some(value) => cells[index] = value,
                None => drop(cells.remove(index)),
            }
            policy(cells)
        };
        assert!(!round_trips::<ApprovePolicy>(&cells(
            1,
            Some(cell("owner", true, true))
        )));
        assert!(!round_trips::<ApprovePolicy>(&cells(
            3,
            Some(cell("member", true, true))
        )));
        assert!(!round_trips::<ApprovePolicy>(&cells(
            3,
            Some(cell("owner", true, false))
        )));
        assert!(!round_trips::<ApprovePolicy>(&cells(3, None)));
        assert!(!round_trips::<ApprovePolicy>(&cells(
            0,
            Some(cell("admin", false, true))
        )));
        let mut five = alpha_default["approve_permission"]
            .as_array()
            .unwrap()
            .clone();
        five.push(cell("member", false, false));
        assert!(!round_trips::<ApprovePolicy>(&policy(five)));
    }

    fn spec_source() -> Value {
        json!({"uri": "file:spec.md", "source_type": "", "label": "Spec", "source_kind": "file"})
    }

    #[test]
    fn spec_source_is_a_file_or_a_tracker() {
        assert_schema_fields::<SpecSource>(
            include_str!("../../../protocol/spec-registry/spec-source.schema.json"),
            &spec_source(),
        );
        assert!(round_trips::<SpecSource>(&without(spec_source(), "label")));
        assert!(round_trips::<SpecSource>(&with(
            spec_source(),
            "source_kind",
            json!("tracker")
        )));
        for kind in [
            json!("text"),
            json!("mcp_session"),
            json!("code_snapshot"),
            Value::Null,
        ] {
            assert!(!round_trips::<SpecSource>(&with(
                spec_source(),
                "source_kind",
                kind
            )));
        }
        assert!(!round_trips::<SpecSource>(&with(
            spec_source(),
            "uri",
            json!("")
        )));
        assert!(!round_trips::<SpecSource>(&with(
            spec_source(),
            "label",
            Value::Null
        )));
    }

    fn source_snapshot() -> Value {
        json!({
            "snapshot_addr": "sha256:snapshot",
            "snapshot_ref": "",
            "captured_at": "2026-11-18T12:00:00Z",
            "content": "The spec text.",
            "content_hash": "hash",
            "capture_method": "manual",
            "source_uri": "file:spec.md"
        })
    }

    #[test]
    fn source_snapshot_requires_its_source_uri() {
        assert_schema_fields::<SourceSnapshot>(
            include_str!("../../../protocol/spec-registry/source-snapshot.schema.json"),
            &source_snapshot(),
        );
        let mut minimal = source_snapshot();
        for optional in ["content", "content_hash", "capture_method"] {
            minimal = without(minimal, optional);
            assert!(!round_trips::<SourceSnapshot>(&with(
                source_snapshot(),
                optional,
                Value::Null
            )));
        }
        assert!(round_trips::<SourceSnapshot>(&minimal));
        for field in ["snapshot_addr", "source_uri"] {
            assert!(!round_trips::<SourceSnapshot>(&with(
                source_snapshot(),
                field,
                json!("")
            )));
        }
        for bad in [json!("yesterday"), json!("2026-11-18"), json!(0)] {
            assert!(!round_trips::<SourceSnapshot>(&with(
                source_snapshot(),
                "captured_at",
                bad
            )));
        }
        let unknown_method = with(source_snapshot(), "capture_method", json!("scrape"));
        assert!(!round_trips::<SourceSnapshot>(&unknown_method));
        // A legacy snapshot has no source_uri and is not guessed into the registry form.
        assert!(!round_trips::<SourceSnapshot>(&without(
            source_snapshot(),
            "source_uri"
        )));
    }

    #[test]
    fn capture_creates_an_empty_domain() {
        let domain = GovernedDomain::captured("product-1".into(), "sha256:snapshot".into());
        assert_eq!(serde_json::to_value(&domain).unwrap(), captured_domain());
        assert_eq!(
            domain.source_digest, None,
            "a capture is not a completed extract"
        );
        assert_eq!(
            domain.claim_set_digest(),
            "db4cd0ee0b2a1dbdbe2e0aea7ae0d7564d8ca2fb47d5b5ab458eb22c1114cbd9"
        );
    }

    #[test]
    fn a_payload_that_would_not_decode_is_not_encoded() {
        // Cross-field rules run on every serialization of a checked payload.
        let mut partial: GovernedDomain = serde_json::from_value(completed_domain()).unwrap();
        partial.route = None;
        assert!(serde_json::to_value(&partial).is_err());
        assert!(encode_checked(&partial).is_err());
        let mut unordered: CheckRecord = serde_json::from_value(check_record()).unwrap();
        std::mem::swap(
            &mut unordered.domain_digest,
            &mut unordered.other_domain_digest,
        );
        assert!(serde_json::to_string(&unordered).is_err());

        // Field rules run only at decode, so plain serialization lets these through.
        let unnamed = GovernedDomain::captured(String::new(), "sha256:snapshot".into());
        assert!(serde_json::to_value(&unnamed).is_ok());
        assert!(encode_checked(&unnamed).is_err());
        let mut orphan: SourceSnapshot = serde_json::from_value(source_snapshot()).unwrap();
        orphan.source_uri.clear();
        assert!(encode_checked(&orphan).is_err());

        let captured = GovernedDomain::captured("product-1".into(), "sha256:snapshot".into());
        assert_eq!(encode_checked(&captured).unwrap(), captured_domain());
        let snapshot: SourceSnapshot = serde_json::from_value(source_snapshot()).unwrap();
        assert_eq!(encode_checked(&snapshot).unwrap(), source_snapshot());
    }
}
