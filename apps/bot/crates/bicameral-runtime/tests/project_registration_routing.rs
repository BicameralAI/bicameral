//! Topology conformance for operator-local Product registration and routing.

use std::sync::Arc;

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventType, ProductCreatedPayload,
};
use bicameral_api::product_creation::{HostedActorContext, ProductCreateParams};
use bicameral_api::product_registration::{
    ConnectionMetadata, ProjectRegistration, RegisteredProvider, RegisteredSubstrate,
};
use bicameral_runtime::product_creation::{build_publication, create_product};
use bicameral_runtime::project_registry::{
    EventStoreRoute, FileProjectRegistrationStore, ProjectEventStoreRouter,
    ProjectRegistrationStore, ProjectRouteRecord,
};
use chrono::Utc;

#[test]
fn persisted_registration_routes_publication_after_restart_from_another_workspace() {
    let product_workspace = tempfile::tempdir().unwrap();
    let unrelated_workspace = tempfile::tempdir().unwrap();
    let operator_home = tempfile::tempdir().unwrap();
    let product_events = product_workspace.path().join(".bicameral/events");
    let unrelated_events = unrelated_workspace.path().join(".bicameral/events");

    let product_adapter = bicameral_runtime::event_store_adapters::GitAdapter::new(&product_events);
    let actor = HostedActorContext {
        actor_id: "bridge:acct_1".into(),
        cloud_account_id: "acct_1".into(),
        verified_domain: None,
        paired: true,
        authorized: true,
    };
    let outcome = create_product(
        &ProductCreateParams {
            idempotency_key: "create-acme".into(),
            product_display_name: "Acme".into(),
            event_store_descriptor: None,
            asserted_cloud_account_id: Some("acct_1".into()),
            team_display_name: None,
            verified_domain: None,
            event_store_hint: None,
            required_connection: None,
        },
        Some(&actor),
        &product_adapter,
        Utc::now(),
    )
    .unwrap();

    let store: Arc<dyn ProjectRegistrationStore> = Arc::new(FileProjectRegistrationStore::new(
        operator_home.path().join("projects"),
    ));
    store
        .write(&ProjectRouteRecord {
            registration: ProjectRegistration {
                product_id: outcome.result.product_id.clone(),
                cloud_account_id: outcome.result.creator_cloud_account_id.clone(),
                registration_ref: format!("creation:{}", outcome.result.receipt_id),
                connection: ConnectionMetadata {
                    provider: RegisteredProvider::None,
                    substrate: RegisteredSubstrate::Git,
                    account_label: None,
                    descriptor_ref: format!("local:{}", outcome.result.product_id),
                },
                governance_policy_version: outcome.result.governance_policy_version.clone(),
                registered_at: Utc::now(),
            },
            event_store: EventStoreRoute::Git {
                events_dir: product_events,
                ci_merge_gate_configured: false,
            },
            first_event_commit_proof: None,
        })
        .unwrap();

    // A fresh router represents a daemon restart. The unrelated path models a
    // daemon launched from a different cwd; routing must ignore it.
    let restarted = ProjectEventStoreRouter::new(store);
    let routed = restarted.adapter_for(&outcome.result.product_id).unwrap();
    let publication = build_publication(
        &outcome.result.product_id,
        "publish-acme",
        routed.as_ref(),
        Utc::now(),
    )
    .unwrap();

    assert_eq!(publication.product_id, outcome.result.product_id);
    assert!(!unrelated_events.exists());
}

#[test]
fn one_daemon_routes_two_products_to_independent_git_stores() {
    let product_a = tempfile::tempdir().unwrap();
    let product_b = tempfile::tempdir().unwrap();
    let operator_home = tempfile::tempdir().unwrap();
    let events_a = product_a.path().join(".bicameral/events");
    let events_b = product_b.path().join(".bicameral/events");
    let adapter_a = bicameral_runtime::event_store_adapters::GitAdapter::new(&events_a);
    let adapter_b = bicameral_runtime::event_store_adapters::GitAdapter::new(&events_b);

    for (adapter, product_id, account) in [
        (&adapter_a, "prd_alpha", "acct_alpha"),
        (&adapter_b, "prd_beta", "acct_beta"),
    ] {
        adapter
            .append(EventStoreEntry::new(
                EventType::CreateProduct,
                serde_json::to_value(ProductCreatedPayload {
                    product_id: product_id.into(),
                    creator_cloud_account_id: account.into(),
                    creator_roles: vec!["Policy Owner".into(), "Product Owner".into()],
                    governance_policy_version: "gpv_1".into(),
                    receipt_id: format!("receipt_{product_id}"),
                    idempotency_key: format!("create_{product_id}"),
                    product_display_name: product_id.into(),
                    team_display_name: None,
                    verified_domain: None,
                    event_store_hint: None,
                    required_connection: None,
                    created_at: Utc::now().to_rfc3339(),
                })
                .unwrap(),
                format!("bridge:{account}"),
            ))
            .unwrap();
    }

    let store: Arc<dyn ProjectRegistrationStore> = Arc::new(FileProjectRegistrationStore::new(
        operator_home.path().join("projects"),
    ));
    for (product_id, account, events_dir) in [
        ("prd_alpha", "acct_alpha", events_a),
        ("prd_beta", "acct_beta", events_b),
    ] {
        store
            .write(&ProjectRouteRecord {
                registration: ProjectRegistration {
                    product_id: product_id.into(),
                    cloud_account_id: account.into(),
                    registration_ref: format!("registration_{product_id}"),
                    connection: ConnectionMetadata {
                        provider: RegisteredProvider::None,
                        substrate: RegisteredSubstrate::Git,
                        account_label: None,
                        descriptor_ref: format!("local:{product_id}"),
                    },
                    governance_policy_version: "gpv_1".into(),
                    registered_at: Utc::now(),
                },
                event_store: EventStoreRoute::Git {
                    events_dir,
                    ci_merge_gate_configured: false,
                },
                first_event_commit_proof: None,
            })
            .unwrap();
    }

    let router = ProjectEventStoreRouter::new(store);
    for product_id in ["prd_alpha", "prd_beta"] {
        let adapter = router.adapter_for(product_id).unwrap();
        let publication = build_publication(
            product_id,
            &format!("publish_{product_id}"),
            adapter.as_ref(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(publication.product_id, product_id);
    }
}
