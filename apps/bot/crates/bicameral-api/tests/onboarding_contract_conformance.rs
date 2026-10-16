//! Production ser/de conformance for the onboarding contract fixtures.
//!
//! Every success and failure fixture under `protocol/onboarding/fixtures/` is
//! deserialized into the **production** `bicameral-api` wire type, re-serialized,
//! and re-deserialized to prove the round trip is stable. No hand-maintained
//! test-only shadow types are used: the types under test are the same ones the
//! runtime and gateway ship (issue #787).
//!
//! The fixtures are the pinned cross-repo contract shapes; the schemas they
//! validate against carry a versioned `$id` and are drift-checked by
//! `scripts/validate_onboarding_contract_fixtures.py`.

use std::path::PathBuf;

use bicameral_api::event_store::{EventStoreEntry, EventType, ProductCreatedPayload};
use bicameral_api::product_creation::{
    HostedProjectPreview, ProductCreationPublication, ProductCreationResult,
};
use bicameral_api::product_join::{
    GrantedRole, JoinOutcome, JoinRejectionCode, ProductJoinRequest, ProductJoinResult,
    ProductJoinTicket,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/onboarding/fixtures")
        .canonicalize()
        .expect("fixture directory resolves")
}

fn read_fixture(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Deserialize a fixture into the production type `T`, re-serialize, and
/// re-deserialize, asserting the round trip is stable.
fn round_trip<T>(name: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug,
{
    let raw = read_fixture(name);
    let value: T = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{name}: production deserialize failed: {e}"));
    let serialized =
        serde_json::to_string(&value).unwrap_or_else(|e| panic!("{name}: serialize failed: {e}"));
    let reparsed: T = serde_json::from_str(&serialized)
        .unwrap_or_else(|e| panic!("{name}: re-deserialize failed: {e}"));
    assert_eq!(
        value, reparsed,
        "{name}: production round trip changed the value"
    );
    value
}

#[test]
fn hosted_project_preview_round_trips() {
    let preview: HostedProjectPreview = round_trip("hosted-project-preview.valid.json");
    // A directory projection is explicitly non-canonical.
    assert!(!preview.canonical, "preview must be non-canonical");
}

#[test]
fn product_join_ticket_round_trips() {
    let _: ProductJoinTicket = round_trip("product-join-ticket.valid.json");
}

#[test]
fn product_join_request_round_trips() {
    let request: ProductJoinRequest = round_trip("product-join-request.valid.json");
    // The inbound request carries the bounded ticket claims.
    assert!(!request.ticket.product_id.is_empty());
    assert!(!request.local_actor_ref.is_empty());
}

#[test]
fn create_product_ledger_event_round_trips_production_entry() {
    let entry: EventStoreEntry = round_trip("create-product-ledger-event.valid.json");
    assert_eq!(entry.event_type, EventType::CreateProduct);
    let payload: ProductCreatedPayload = serde_json::from_value(entry.payload.clone())
        .expect("ledger payload is ProductCreatedPayload");
    assert_eq!(
        payload.creator_roles,
        vec!["Policy Owner".to_string(), "Product Owner".to_string()]
    );
    assert_eq!(payload.product_id, "prd_01HZX8CHECKOUT");
    assert!(
        serde_json::to_value(&entry)
            .unwrap()
            .get("receipt_id")
            .is_none(),
        "ProductCreationResult fields must not sit on the ledger envelope"
    );
}

#[test]
fn product_creation_result_round_trips_with_atomic_creator_roles() {
    let result: ProductCreationResult = round_trip("product-creation-result.valid.json");
    // Creator roles are materialized atomically: exactly Policy Owner + Product
    // Owner, never a partial grant.
    assert_eq!(
        result.creator_roles,
        vec!["Policy Owner".to_string(), "Product Owner".to_string()],
        "creator roles must be exactly Policy Owner + Product Owner"
    );
}

#[test]
fn product_creation_publication_binds_creator_account_and_product_id() {
    let result: ProductCreationResult = round_trip("product-creation-result.valid.json");
    let publication: ProductCreationPublication =
        round_trip("product-creation-publication.valid.json");
    // Publication is non-canonical and bound to the canonical Product identity
    // and the authenticated creator Cloud account from the creation result.
    assert!(!publication.canonical, "publication must be non-canonical");
    assert_eq!(
        publication.product_id, result.product_id,
        "publication must bind the canonical product_id"
    );
    assert_eq!(
        publication.creator_cloud_account_id, result.creator_cloud_account_id,
        "publication must bind the creator Cloud account"
    );
}

#[test]
fn product_creation_publication_carries_shared_v2_commit_proof() {
    use bicameral_api::product_creation::{proof_digest_v2, PROOF_SCHEMA_VERSION_V2};
    let publication: ProductCreationPublication =
        round_trip("product-creation-publication.with-proof.json");
    let proof = publication
        .first_event_commit_proof
        .as_ref()
        .expect("fixture carries the v2 commit proof");
    // The proof binds the canonical Product this publication describes and is
    // the literal Bot<->Cloud golden vector (same canonical bytes and digest).
    assert_eq!(proof.product_id, publication.product_id);
    assert_eq!(proof.schema_version, PROOF_SCHEMA_VERSION_V2);
    assert_eq!(proof.substrate, "drive_folder");
    assert_eq!(proof.proof_digest, proof_digest_v2(proof));
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol/onboarding/vectors/product-creation-commit-proof-v2.golden.json"
    ))
    .unwrap();
    assert_eq!(serde_json::to_value(proof).unwrap(), golden["proof"]);
    // Publications without a proof still round-trip (non-v2 substrates).
    let plain: ProductCreationPublication = round_trip("product-creation-publication.valid.json");
    assert!(plain.first_event_commit_proof.is_none());
}

#[test]
fn product_creation_publication_retry_preserves_product_identity() {
    let base: ProductCreationPublication = round_trip("product-creation-publication.valid.json");
    let retry: ProductCreationPublication = round_trip("product-creation-publication.retry.json");
    // Repeating publication preserves the original product_id + idempotency key
    // (never a second Product), while the row itself may be re-minted.
    assert_eq!(
        retry.product_id, base.product_id,
        "retry must reuse product_id"
    );
    assert_eq!(
        retry.idempotency_key, base.idempotency_key,
        "retry must reuse the idempotency key"
    );
    assert_eq!(
        retry.creator_cloud_account_id, base.creator_cloud_account_id,
        "retry must keep the creator-account binding"
    );
    assert_ne!(
        retry.publication_id, base.publication_id,
        "retry is a distinct publication row"
    );
}

#[test]
fn product_join_granted_grants_product_member_only() {
    let result: ProductJoinResult = round_trip("product-join-result.granted.json");
    assert_eq!(result.outcome, JoinOutcome::Granted);
    assert_eq!(result.granted_role, Some(GrantedRole::ProductMember));
    assert!(result.membership_event_ref.is_some());
    assert!(
        result.failure.is_none(),
        "granted result carries no failure"
    );
}

#[test]
fn product_join_rejections_round_trip_with_typed_codes() {
    let cases = [
        (
            "product-join-result.expired-ticket.json",
            JoinRejectionCode::ExpiredTicket,
        ),
        (
            "product-join-result.wrong-domain.json",
            JoinRejectionCode::WrongDomain,
        ),
        (
            "product-join-result.wrong-account.json",
            JoinRejectionCode::WrongAccount,
        ),
        (
            "product-join-result.wrong-product.json",
            JoinRejectionCode::WrongProduct,
        ),
        (
            "product-join-result.replayed-ticket.json",
            JoinRejectionCode::ReplayedTicket,
        ),
        (
            "product-join-result.invalid-signature.json",
            JoinRejectionCode::InvalidSignature,
        ),
        (
            "product-join-result.wrong-audience.json",
            JoinRejectionCode::WrongAudience,
        ),
        (
            "product-join-result.missing-pairing.json",
            JoinRejectionCode::MissingPairing,
        ),
        (
            "product-join-result.policy-changed.json",
            JoinRejectionCode::PolicyChanged,
        ),
        (
            "product-join-result.not-yet-valid.json",
            JoinRejectionCode::NotYetValid,
        ),
    ];
    for (name, expected) in cases {
        let result: ProductJoinResult = round_trip(name);
        assert_eq!(
            result.outcome,
            JoinOutcome::Rejected,
            "{name}: must be rejected"
        );
        assert!(
            result.granted_role.is_none(),
            "{name}: rejection grants no role"
        );
        assert!(
            result.membership_event_ref.is_none(),
            "{name}: rejection creates no membership"
        );
        let failure = result.failure.expect("rejection carries a typed failure");
        assert_eq!(failure.code, expected, "{name}: unexpected failure code");
    }
}

#[test]
fn onboarding_types_fail_closed_on_undeclared_fields() {
    // Every pinned onboarding type rejects an undeclared field: the schemas are
    // `additionalProperties: false` and the production types carry
    // `#[serde(deny_unknown_fields)]`.
    let inject = |name: &str| -> String {
        let raw = read_fixture(name);
        let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("smuggled_field".to_string(), serde_json::json!("x"));
        serde_json::to_string(&value).unwrap()
    };

    assert!(
        serde_json::from_str::<ProductJoinTicket>(&inject("product-join-ticket.valid.json"))
            .is_err(),
        "ticket must fail closed on an undeclared field"
    );
    assert!(
        serde_json::from_str::<ProductJoinResult>(&inject("product-join-result.granted.json"))
            .is_err(),
        "join result must fail closed on an undeclared field"
    );
    assert!(
        serde_json::from_str::<ProductCreationResult>(&inject(
            "product-creation-result.valid.json"
        ))
        .is_err(),
        "creation result must fail closed on an undeclared field"
    );
    let raw = read_fixture("create-product-ledger-event.valid.json");
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .get_mut("payload")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("smuggled_field".to_string(), serde_json::json!("x"));
    assert!(
        serde_json::from_value::<ProductCreatedPayload>(value["payload"].clone()).is_err(),
        "CreateProduct payload must fail closed on an undeclared field"
    );
    assert!(
        serde_json::from_str::<ProductCreationPublication>(&inject(
            "product-creation-publication.valid.json"
        ))
        .is_err(),
        "publication must fail closed on an undeclared field"
    );
    assert!(
        serde_json::from_str::<HostedProjectPreview>(&inject("hosted-project-preview.valid.json"))
            .is_err(),
        "preview must fail closed on an undeclared field"
    );
}
