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
