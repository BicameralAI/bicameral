//! Daemon-canonical hosted Product creation (issue #766).
//!
//! The daemon is the sole canonical writer for Product identity, creator roles,
//! and the initial Governance Policy Version. `product.create`:
//!
//! 1. classifies authority against the daemon-confirmed hosted actor context
//!    (paired session, bound Cloud account, authorization) — an invalid
//!    condition returns a typed denial and **writes no event**;
//! 2. is idempotent by `idempotency_key`: a retry returns the existing Product
//!    and never appends a second creation/role event;
//! 3. materializes canonical state atomically — a single
//!    [`EventType::CreateProduct`] event mints the Product identity, grants the
//!    creator exactly Policy Owner + Product Owner in one membership event, and
//!    records the initial Governance Policy Version.
//!
//! Replay reconstructs Product identity and creator authority from the canonical
//! event store alone — Cloud is never consulted as canonical state. Hosted
//! publication ([`build_publication`]) derives a non-canonical directory row
//! from the committed creation receipt and can be retried independently by
//! canonical Product id without re-running creation.

use std::collections::BTreeMap;
use std::sync::Arc;

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, EventType, ProductCreatedPayload,
};
use bicameral_api::product_creation::{
    classify_product_create, creator_role_labels, EventStoreHint, HostedActorContext,
    HostedProjectPreview, ProductCreateDenial, ProductCreateParams, ProductCreationCommitProof,
    ProductCreationPublication, ProductCreationResult, ProductEventStoreDescriptor,
    RequiredConnection,
};
use bicameral_gateway::proof_signing::{ProofSignError, ProofSigner};

use crate::drive_api_event_store::ConfirmedAppend;
use crate::product_creation_proof::{mint_product_creation_commit_proof, ProofMintError};
use bicameral_api::tool_request::ToolRequest;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

/// Default hosted preview TTL: 24 hours.
const PREVIEW_TTL_HOURS: i64 = 24;

/// Initial Governance Policy Version created with every Product.
const INITIAL_GOVERNANCE_POLICY_VERSION: &str = "gpv_1";

/// A canonical Product reconstructed from the event store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRecord {
    pub product_id: String,
    pub creator_cloud_account_id: String,
    pub creator_roles: Vec<String>,
    pub governance_policy_version: String,
    pub receipt_id: String,
    pub membership_event_ref: String,
    pub idempotency_key: String,
    pub product_display_name: String,
    pub team_display_name: Option<String>,
    pub verified_domain: Option<String>,
    pub event_store_hint: Option<EventStoreHint>,
    pub required_connection: Option<RequiredConnection>,
    pub created_at: String,
}

impl ProductRecord {
    /// The canonical creation result for this record.
    pub fn to_result(&self) -> ProductCreationResult {
        ProductCreationResult {
            product_id: self.product_id.clone(),
            creator_cloud_account_id: self.creator_cloud_account_id.clone(),
            creator_roles: self.creator_roles.clone(),
            membership_event_ref: self.membership_event_ref.clone(),
            governance_policy_version: self.governance_policy_version.clone(),
            receipt_id: self.receipt_id.clone(),
            created_at: self.created_at.clone(),
        }
    }

    /// A safe, non-canonical hosted directory projection for this record.
    pub fn to_preview(&self, now: DateTime<Utc>) -> HostedProjectPreview {
        HostedProjectPreview {
            preview_id: format!("hpv_{}", Uuid::new_v4().simple()),
            product_id: self.product_id.clone(),
            product_display_name: self.product_display_name.clone(),
            team_display_name: self.team_display_name.clone(),
            verified_domain: self.verified_domain.clone(),
            event_store_hint: self.event_store_hint.clone(),
            required_connection: self.required_connection.clone(),
            canonical: false,
            expires_at: (now + Duration::hours(PREVIEW_TTL_HOURS)).to_rfc3339(),
        }
    }
}

/// Canonical Product projection reconstructed by replay.
#[derive(Debug, Clone, Default)]
pub struct ProductLedger {
    /// Products keyed by canonical `product_id`.
    pub by_id: BTreeMap<String, ProductRecord>,
    /// Idempotency key -> canonical `product_id`.
    pub by_idempotency: BTreeMap<String, String>,
    /// Canonical Product creation events encountered during replay.
    pub create_event_count: usize,
    /// Product creation events skipped because their payload was malformed.
    pub malformed_create_event_count: usize,
    /// The exact canonical `CreateProduct` entry per Product, kept so a retry
    /// or publication can re-confirm the same provider object (bot#1087).
    pub create_entries: BTreeMap<String, EventStoreEntry>,
}

impl ProductLedger {
    /// Look up an existing Product by its creation idempotency key.
    pub fn find_by_idempotency(&self, key: &str) -> Option<&ProductRecord> {
        self.by_idempotency
            .get(key)
            .and_then(|id| self.by_id.get(id))
    }
}

/// Reconstruct the canonical Product ledger from the event store.
///
/// Deterministic and Cloud-free: identity and creator authority come from
/// [`EventType::CreateProduct`] events alone. Re-emitted events with an
/// already-seen `idempotency_key` are ignored so replay is idempotent.
pub fn replay_products(adapter: &dyn EventStoreAdapter) -> Result<ProductLedger, EventStoreError> {
    let entries = adapter.replay()?;
    let mut ledger = ProductLedger::default();

    for entry in entries {
        if entry.event_type != EventType::CreateProduct {
            continue;
        }
        ledger.create_event_count += 1;

        let Ok(payload) = serde_json::from_value::<ProductCreatedPayload>(entry.payload.clone())
        else {
            // A malformed payload is skipped rather than aborting replay of the
            // rest of the canonical ledger.
            ledger.malformed_create_event_count += 1;
            continue;
        };

        // Idempotent replay: first event for a key wins; product_id also dedups.
        if ledger.by_idempotency.contains_key(&payload.idempotency_key)
            || ledger.by_id.contains_key(&payload.product_id)
        {
            continue;
        }

        let record = ProductRecord {
            product_id: payload.product_id.clone(),
            creator_cloud_account_id: payload.creator_cloud_account_id,
            creator_roles: payload.creator_roles,
            governance_policy_version: payload.governance_policy_version,
            receipt_id: payload.receipt_id,
            membership_event_ref: format!("evt_{}", entry.id.simple()),
            idempotency_key: payload.idempotency_key.clone(),
            product_display_name: payload.product_display_name,
            team_display_name: payload.team_display_name,
            verified_domain: payload.verified_domain,
            event_store_hint: payload.event_store_hint,
            required_connection: payload.required_connection,
            created_at: payload.created_at,
        };

        let product_id = payload.product_id.clone();
        ledger
            .by_idempotency
            .insert(payload.idempotency_key, product_id.clone());
        ledger.by_id.insert(product_id.clone(), record);
        ledger.create_entries.insert(product_id, entry);
    }

    Ok(ledger)
}

/// Error materializing a Product.
#[derive(Debug, Clone)]
pub enum ProductCreateError {
    /// Typed authority/validation denial — no event was written.
    Denied(ProductCreateDenial),
    /// Event-store failure (append/replay).
    Store(String),
    /// The daemon proof signer could not provide its stable key. Checked
    /// before any append, so no canonical event was written.
    ProofSignerUnavailable(String),
    /// Exact provider readback of the `CreateProduct` object failed or did not
    /// match the canonical bytes. No proof was minted.
    ProviderReadback(String),
    /// The event is provider-confirmed but a signed proof could not be minted.
    /// The canonical Product exists; a retry with the same idempotency key
    /// reconciles it without a second append.
    ProofMint(String),
}

impl ProductCreateError {
    /// Redaction-safe description (never key material or credentials).
    pub fn safe_message(&self) -> String {
        match self {
            Self::Denied(denial) => format!("{}: {}", denial.code(), denial.message),
            Self::Store(message)
            | Self::ProofSignerUnavailable(message)
            | Self::ProviderReadback(message)
            | Self::ProofMint(message) => message.clone(),
        }
    }
}

/// Provider-confirmed commit handles for the production create path
/// (bot#1087): the exact Drive API adapter's readback seam plus the
/// daemon-owned proof signer. Absent on non-v2 substrates (Git, synced
/// folder, memory), which append without a proof.
#[derive(Clone)]
pub struct ProductCreateCommit {
    pub confirmed_append: Arc<dyn ConfirmedAppend>,
    pub signer: Arc<dyn ProofSigner>,
}

impl std::fmt::Debug for ProductCreateCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProductCreateCommit { confirmed_append, signer }")
    }
}

/// Outcome of `create_product`.
#[derive(Debug, Clone)]
pub struct ProductCreationOutcome {
    /// Canonical creation result.
    pub result: ProductCreationResult,
    /// Safe, non-canonical hosted directory projection.
    pub preview: HostedProjectPreview,
    /// `true` when a new canonical event was appended; `false` when an existing
    /// Product was returned for an already-seen idempotency key.
    pub created: bool,
    /// Signed v2 commit proof, present only when the create ran through a
    /// provider-confirmed commit (exact readback + daemon signature).
    pub first_event_commit_proof: Option<ProductCreationCommitProof>,
}

/// Confirm the exact canonical entry on the provider and mint its proof.
///
/// `append_confirmed_entry` is idempotent on the canonical event id: a new
/// append and a re-confirmation of an existing object both end in an exact
/// byte readback, so a retry never creates a second provider object.
fn confirm_and_mint(
    commit: &ProductCreateCommit,
    entry: &EventStoreEntry,
    attempt_id: &str,
    product_id: &str,
) -> Result<ProductCreationCommitProof, ProductCreateError> {
    let confirmation = commit
        .confirmed_append
        .append_confirmed_entry(entry)
        .map_err(|error| match error {
            EventStoreError::Conflict { .. } => {
                ProductCreateError::ProviderReadback(error.to_string())
            }
            other => ProductCreateError::Store(other.to_string()),
        })?;
    mint_product_creation_commit_proof(
        &confirmation,
        attempt_id,
        product_id,
        &entry.id.to_string(),
        commit.signer.as_ref(),
    )
    .map_err(|error| match error {
        ProofMintError::Sign(ProofSignError::KeyUnavailable { .. }) => {
            ProductCreateError::ProofSignerUnavailable(error.to_string())
        }
        other => ProductCreateError::ProofMint(other.to_string()),
    })
}

/// Re-confirm an existing Product's canonical `CreateProduct` object on the
/// provider and mint its proof, without appending anything.
///
/// Returns `Ok(None)` when no canonical Product with that id is in the ledger.
pub fn reconcile_commit_proof(
    product_id: &str,
    adapter: &dyn EventStoreAdapter,
    commit: &ProductCreateCommit,
) -> Result<Option<ProductCreationCommitProof>, ProductCreateError> {
    let ledger = replay_products(adapter).map_err(|e| ProductCreateError::Store(e.to_string()))?;
    let (Some(record), Some(entry)) = (
        ledger.by_id.get(product_id),
        ledger.create_entries.get(product_id),
    ) else {
        return Ok(None);
    };
    confirm_and_mint(commit, entry, &record.idempotency_key, product_id).map(Some)
}

/// Create a canonical Product, atomically materializing the creator's roles.
///
/// Ordering is authority-first: classification runs before any store access, so
/// a denied request never appends an event. Idempotency is enforced by replay
/// before append, so retrying the same key returns the existing Product and its
/// original membership event reference.
pub fn create_product(
    params: &ProductCreateParams,
    actor: Option<&HostedActorContext>,
    adapter: &dyn EventStoreAdapter,
    now: DateTime<Utc>,
) -> Result<ProductCreationOutcome, ProductCreateError> {
    create_product_with_commit(params, actor, adapter, None, now)
}

/// [`create_product`] with an optional provider-confirmed commit (bot#1087).
///
/// With `commit` present the canonical event is appended through the exact
/// provider adapter, read back byte-for-byte, and a signed v2 proof is minted
/// from that confirmation. The signer's stable key is checked before any
/// append, so an unavailable signer writes nothing. A retry with the same
/// idempotency key re-confirms the existing provider object and mints a proof
/// over the same facts instead of appending again.
pub fn create_product_with_commit(
    params: &ProductCreateParams,
    actor: Option<&HostedActorContext>,
    adapter: &dyn EventStoreAdapter,
    commit: Option<&ProductCreateCommit>,
    now: DateTime<Utc>,
) -> Result<ProductCreationOutcome, ProductCreateError> {
    // 1. Authority + payload classification. Denials write no event.
    let actor = classify_product_create(params, actor).map_err(ProductCreateError::Denied)?;

    // 2. Idempotency: a prior creation with this key returns the existing
    //    Product without appending a second creation/role event. With a
    //    commit, the existing provider object is re-confirmed for its proof.
    let ledger = replay_products(adapter).map_err(|e| ProductCreateError::Store(e.to_string()))?;
    if let Some(existing) = ledger.find_by_idempotency(&params.idempotency_key) {
        let first_event_commit_proof =
            match (commit, ledger.create_entries.get(&existing.product_id)) {
                (Some(commit), Some(entry)) => Some(confirm_and_mint(
                    commit,
                    entry,
                    &params.idempotency_key,
                    &existing.product_id,
                )?),
                _ => None,
            };
        return Ok(ProductCreationOutcome {
            result: existing.to_result(),
            preview: existing.to_preview(now),
            created: false,
            first_event_commit_proof,
        });
    }

    // 2b. Fail closed before any append when the daemon signer cannot provide
    //     its stable key: a provider fact must never exist without the
    //     ability to prove it.
    if let Some(commit) = commit {
        commit
            .signer
            .signer_key_id()
            .map_err(|error| ProductCreateError::ProofSignerUnavailable(error.to_string()))?;
    }

    // 3. Materialize canonical state atomically: one event mints the Product
    //    identity, grants both creator roles, and records the initial
    //    Governance Policy Version.
    let product_id = format!("prd_{}", Uuid::new_v4().simple());
    let receipt_id = format!("rcpt_{}", Uuid::new_v4().simple());
    let created_at = now.to_rfc3339();
    let creator_roles = creator_role_labels();

    let payload = ProductCreatedPayload {
        product_id: product_id.clone(),
        creator_cloud_account_id: actor.cloud_account_id.clone(),
        creator_roles: creator_roles.clone(),
        governance_policy_version: INITIAL_GOVERNANCE_POLICY_VERSION.to_string(),
        receipt_id: receipt_id.clone(),
        idempotency_key: params.idempotency_key.clone(),
        product_display_name: params.product_display_name.clone(),
        team_display_name: params.team_display_name.clone(),
        verified_domain: params.verified_domain.clone(),
        event_store_hint: params.event_store_hint.clone(),
        required_connection: params.required_connection.clone(),
        created_at: created_at.clone(),
    };

    let payload_json = serde_json::to_value(&payload)
        .map_err(|e| ProductCreateError::Store(format!("payload serialization failed: {e}")))?;

    let entry = EventStoreEntry::new(
        EventType::CreateProduct,
        payload_json,
        actor.actor_id.clone(),
    );
    let membership_event_ref = format!("evt_{}", entry.id.simple());

    let first_event_commit_proof = match commit {
        Some(commit) => Some(confirm_and_mint(
            commit,
            &entry,
            &params.idempotency_key,
            &product_id,
        )?),
        None => {
            adapter
                .append(entry)
                .map_err(|e| ProductCreateError::Store(e.to_string()))?;
            None
        }
    };

    let record = ProductRecord {
        product_id,
        creator_cloud_account_id: actor.cloud_account_id.clone(),
        creator_roles,
        governance_policy_version: INITIAL_GOVERNANCE_POLICY_VERSION.to_string(),
        receipt_id,
        membership_event_ref,
        idempotency_key: params.idempotency_key.clone(),
        product_display_name: params.product_display_name.clone(),
        team_display_name: params.team_display_name.clone(),
        verified_domain: params.verified_domain.clone(),
        event_store_hint: params.event_store_hint.clone(),
        required_connection: params.required_connection.clone(),
        created_at,
    };

    Ok(ProductCreationOutcome {
        result: record.to_result(),
        preview: record.to_preview(now),
        created: true,
        first_event_commit_proof,
    })
}

/// Error building a hosted publication.
#[derive(Debug, Clone)]
pub enum ProductPublishError {
    /// No canonical Product exists for the supplied `product_id`.
    NotFound {
        canonical_product_count: usize,
        create_event_count: usize,
        malformed_create_event_count: usize,
    },
    /// Event-store failure (replay).
    Store(String),
}

/// Build a retryable, non-canonical hosted publication for an already-created
/// Product, keyed by the canonical `product_id`.
///
/// This is a read-only projection over the canonical creation event: it appends
/// **no** event and never re-runs creation, so a failed Cloud publication can be
/// retried independently with the original canonical Product id + safe receipt.
pub fn build_publication(
    product_id: &str,
    idempotency_key: &str,
    adapter: &dyn EventStoreAdapter,
    now: DateTime<Utc>,
) -> Result<ProductCreationPublication, ProductPublishError> {
    let ledger = replay_products(adapter).map_err(|e| ProductPublishError::Store(e.to_string()))?;
    let record = ledger
        .by_id
        .get(product_id)
        .ok_or(ProductPublishError::NotFound {
            canonical_product_count: ledger.by_id.len(),
            create_event_count: ledger.create_event_count,
            malformed_create_event_count: ledger.malformed_create_event_count,
        })?;

    Ok(ProductCreationPublication {
        publication_id: format!("pub_{}", Uuid::new_v4().simple()),
        product_id: record.product_id.clone(),
        creator_cloud_account_id: record.creator_cloud_account_id.clone(),
        team_display_name: record.team_display_name.clone(),
        verified_domain: record.verified_domain.clone(),
        event_store_hint: record.event_store_hint.clone(),
        canonical: false,
        idempotency_key: idempotency_key.to_string(),
        canonical_creation_receipt_ref: record.receipt_id.clone(),
        published_at: now.to_rfc3339(),
        first_event_commit_proof: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::product_creation::{
        contains_forbidden_material, ProductCreateDenialReason, ProductPublishParams,
    };
    use std::sync::Mutex;

    /// Minimal in-memory adapter for tests.
    struct MemStore {
        entries: Mutex<Vec<EventStoreEntry>>,
    }
    impl MemStore {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }
    }
    impl EventStoreAdapter for MemStore {
        fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
            self.entries.lock().unwrap().push(entry);
            Ok(())
        }
        fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            Ok(self.entries.lock().unwrap().clone())
        }
        fn substrate_kind(&self) -> bicameral_api::event_store::SubstrateKind {
            bicameral_api::event_store::SubstrateKind::Memory
        }
        fn health_check(
            &self,
        ) -> Result<bicameral_api::event_store::SubstrateHealth, EventStoreError> {
            Ok(bicameral_api::event_store::SubstrateHealth {
                kind: bicameral_api::event_store::SubstrateKind::Memory,
                reachable: true,
                last_synced: None,
                entry_count: self.entries.lock().unwrap().len(),
                freshness: None,
                warnings: Vec::new(),
                capabilities: Default::default(),
            })
        }
        fn capabilities(
            &self,
        ) -> std::collections::BTreeSet<bicameral_api::event_store::SubstrateCapability> {
            Default::default()
        }
    }

    fn params() -> ProductCreateParams {
        ProductCreateParams {
            idempotency_key: "idem-acme".into(),
            product_display_name: "Acme Checkout".into(),
            event_store_descriptor: None,
            asserted_cloud_account_id: Some("acct_1".into()),
            team_display_name: Some("Acme".into()),
            verified_domain: Some("acme.example".into()),
            event_store_hint: Some(EventStoreHint {
                substrate: Some("git".into()),
                provider: Some("github".into()),
            }),
            required_connection: Some(RequiredConnection {
                provider: Some("github".into()),
            }),
        }
    }

    fn actor() -> HostedActorContext {
        HostedActorContext {
            actor_id: "bridge:acct_1".into(),
            cloud_account_id: "acct_1".into(),
            verified_domain: Some("acme.example".into()),
            paired: true,
            authorized: true,
        }
    }

    fn count_create_events(store: &MemStore) -> usize {
        store
            .replay()
            .unwrap()
            .iter()
            .filter(|e| e.event_type == EventType::CreateProduct)
            .count()
    }

    #[test]
    fn create_grants_exactly_policy_and_product_owner() {
        let store = MemStore::new();
        let a = actor();
        let out = create_product(&params(), Some(&a), &store, Utc::now()).unwrap();
        assert!(out.created);
        assert_eq!(
            out.result.creator_roles,
            vec!["Policy Owner".to_string(), "Product Owner".to_string()]
        );
        assert_eq!(out.result.creator_cloud_account_id, "acct_1");
        assert_eq!(out.result.governance_policy_version, "gpv_1");
        assert!(out.result.membership_event_ref.starts_with("evt_"));
        assert!(!out.preview.canonical);
        assert_eq!(count_create_events(&store), 1);
    }

    #[test]
    fn denials_write_no_event() {
        for mutate in [
            // missing actor context
            (None, params()),
            // unpaired
            (
                Some(HostedActorContext {
                    paired: false,
                    ..actor()
                }),
                params(),
            ),
            // unauthorized
            (
                Some(HostedActorContext {
                    authorized: false,
                    ..actor()
                }),
                params(),
            ),
        ] {
            let store = MemStore::new();
            let (a, p) = mutate;
            let err = create_product(&p, a.as_ref(), &store, Utc::now()).unwrap_err();
            assert!(matches!(err, ProductCreateError::Denied(_)));
            assert_eq!(count_create_events(&store), 0);
        }
    }

    #[test]
    fn wrong_cloud_account_is_denied_without_event() {
        let store = MemStore::new();
        let a = actor();
        let mut p = params();
        p.asserted_cloud_account_id = Some("acct_other".into());
        let err = create_product(&p, Some(&a), &store, Utc::now()).unwrap_err();
        match err {
            ProductCreateError::Denied(d) => {
                assert_eq!(d.reason, ProductCreateDenialReason::WrongCloudAccount)
            }
            other => panic!("expected denial, got {other:?}"),
        }
        assert_eq!(count_create_events(&store), 0);
    }

    #[test]
    fn duplicate_idempotency_key_does_not_create_second_product() {
        let store = MemStore::new();
        let a = actor();
        let first = create_product(&params(), Some(&a), &store, Utc::now()).unwrap();
        let second = create_product(&params(), Some(&a), &store, Utc::now()).unwrap();
        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.result.product_id, second.result.product_id);
        assert_eq!(
            first.result.membership_event_ref,
            second.result.membership_event_ref
        );
        assert_eq!(count_create_events(&store), 1);
    }

    #[test]
    fn replay_restores_product_and_authority() {
        let store = MemStore::new();
        let a = actor();
        let out = create_product(&params(), Some(&a), &store, Utc::now()).unwrap();

        let ledger = replay_products(&store).unwrap();
        let record = ledger.by_id.get(&out.result.product_id).unwrap();
        assert_eq!(record.creator_cloud_account_id, "acct_1");
        assert_eq!(
            record.creator_roles,
            vec!["Policy Owner".to_string(), "Product Owner".to_string()]
        );
        assert_eq!(record.governance_policy_version, "gpv_1");
        assert_eq!(record.to_result(), out.result);
    }

    #[test]
    fn publication_retry_does_not_rerun_creation() {
        let store = MemStore::new();
        let a = actor();
        let out = create_product(&params(), Some(&a), &store, Utc::now()).unwrap();
        let pid = out.result.product_id.clone();

        let pub1 = build_publication(&pid, "publish-1", &store, Utc::now()).unwrap();
        let pub2 = build_publication(&pid, "publish-1", &store, Utc::now()).unwrap();
        assert!(!pub1.canonical);
        assert_eq!(pub1.product_id, pid);
        assert_eq!(pub1.canonical_creation_receipt_ref, out.result.receipt_id);
        assert_eq!(pub2.product_id, pid);
        // Publication is read-only: no additional canonical events.
        assert_eq!(count_create_events(&store), 1);
    }

    #[test]
    fn publication_for_unknown_product_is_not_found() {
        let store = MemStore::new();
        let err = build_publication("prd_missing", "publish-1", &store, Utc::now()).unwrap_err();
        assert!(matches!(err, ProductPublishError::NotFound { .. }));
    }

    #[test]
    fn receipt_and_preview_contain_no_forbidden_material() {
        let store = MemStore::new();
        let a = actor();
        let out = create_product(&params(), Some(&a), &store, Utc::now()).unwrap();
        let result_json = serde_json::to_string(&out.result).unwrap();
        let preview_json = serde_json::to_string(&out.preview).unwrap();
        assert!(!contains_forbidden_material(&result_json));
        assert!(!contains_forbidden_material(&preview_json));

        let pubn = build_publication(&out.result.product_id, "p", &store, Utc::now()).unwrap();
        let pub_json = serde_json::to_string(&pubn).unwrap();
        assert!(!contains_forbidden_material(&pub_json));
        let _ = ProductPublishParams {
            product_id: out.result.product_id,
            idempotency_key: "p".into(),
        };
    }
}

// ---------------------------------------------------------------------------
// Operator-scoped create routing (#1264)
// ---------------------------------------------------------------------------

/// Typed rejection when an operator-scoped `product.create` carries no
/// durable event-store descriptor at all.
pub(crate) const OPERATOR_CREATE_DESCRIPTOR_REQUIRED: &str =
    "product_event_store_required: operator-scoped product.create must first select and validate a durable event-store descriptor";

/// Typed rejection when the descriptor is present but was not validated by
/// this daemon's own resource-selection flow (missing, stale, or mismatched
/// validation).
pub(crate) const OPERATOR_CREATE_DESCRIPTOR_UNVALIDATED: &str =
    "product_event_store_unvalidated: the selected Google Drive folder was not validated by this daemon; choose the folder through Bicameral's Google Drive selection and try again";

/// Typed rejection when the descriptor is structurally unusable.
pub(crate) const OPERATOR_CREATE_DESCRIPTOR_INVALID: &str =
    "product_event_store_invalid: the event-store descriptor does not identify a Google Drive folder";

fn authority_metadata<'a>(
    authority: &'a bicameral_api::tool_request::AuthorityContext,
    key: &str,
) -> Option<&'a str> {
    authority
        .audit_metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
}

/// Google Drive file ids are opaque URL-safe tokens. Reject anything that
/// looks like a path, a query, or whitespace so a descriptor can never be
/// repurposed as a filesystem or request-path substitution.
pub(crate) fn is_plausible_drive_folder_id(folder_id: &str) -> bool {
    let trimmed = folder_id.trim();
    !trimmed.is_empty()
        && trimmed == folder_id
        && folder_id.len() <= 256
        && folder_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Resolve the durable event-store route an operator-scoped daemon may use
/// for `product.create`.
///
/// Fail-closed contract:
/// - no descriptor: `product_event_store_required`;
/// - descriptor that is not a plausible Drive folder id: `product_event_store_invalid`;
/// - descriptor whose folder was not validated by this daemon: the gateway
///   stamps `DESCRIPTOR_VALIDATION_METADATA_KEY = valid` plus the exact folder
///   id into `VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY` only from a live,
///   daemon-authored `Ready` selection session; anything else is
///   `product_event_store_unvalidated`.
///
/// The browser transports the descriptor; only the daemon's own validation
/// makes it usable.
pub(crate) fn operator_create_route(
    request: &ToolRequest,
    params: &ProductCreateParams,
) -> Result<crate::project_registry::EventStoreRoute, &'static str> {
    use bicameral_api::product_registration::{
        DescriptorValidation, DESCRIPTOR_VALIDATION_METADATA_KEY,
        VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY,
    };

    let Some(descriptor) = &params.event_store_descriptor else {
        return Err(OPERATOR_CREATE_DESCRIPTOR_REQUIRED);
    };
    let ProductEventStoreDescriptor::Google { folder_id } = descriptor;
    if !is_plausible_drive_folder_id(folder_id) {
        return Err(OPERATOR_CREATE_DESCRIPTOR_INVALID);
    }
    let validation = DescriptorValidation::from_metadata(authority_metadata(
        &request.authority,
        DESCRIPTOR_VALIDATION_METADATA_KEY,
    ));
    let validated_folder = authority_metadata(
        &request.authority,
        VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY,
    );
    if validation != DescriptorValidation::Valid || validated_folder != Some(folder_id.as_str()) {
        return Err(OPERATOR_CREATE_DESCRIPTOR_UNVALIDATED);
    }
    Ok(crate::project_registry::EventStoreRoute::GoogleDriveApi {
        folder_id: folder_id.clone(),
    })
}

#[cfg(test)]
mod operator_create_route_tests {
    use super::*;
    use bicameral_api::product_registration::{
        DESCRIPTOR_VALIDATION_METADATA_KEY, VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY,
    };
    use bicameral_api::tool_request::{AuthMethod, AuthorityContext, ToolCommand, ToolRequest};

    fn params(descriptor: Option<ProductEventStoreDescriptor>) -> ProductCreateParams {
        ProductCreateParams {
            idempotency_key: "k".into(),
            product_display_name: "P".into(),
            event_store_descriptor: descriptor,
            asserted_cloud_account_id: Some("acct".into()),
            team_display_name: None,
            verified_domain: None,
            event_store_hint: None,
            required_connection: None,
        }
    }

    fn request(metadata: &[(&str, &str)]) -> ToolRequest {
        let mut audit = serde_json::Map::new();
        for (key, value) in metadata {
            audit.insert((*key).into(), serde_json::Value::String((*value).into()));
        }
        ToolRequest::new(
            ToolCommand::ProductCreate(params(None)),
            AuthorityContext {
                actor_id: "bridge:acct".into(),
                auth_method: AuthMethod::HostedBridge,
                session_id: Some("s".into()),
                workspace: None,
                policy_scope: vec!["hosted_bridge".into()],
                audit_metadata: audit,
            },
        )
    }

    fn google(folder_id: &str) -> Option<ProductEventStoreDescriptor> {
        Some(ProductEventStoreDescriptor::Google {
            folder_id: folder_id.into(),
        })
    }

    #[test]
    fn missing_descriptor_fails_closed() {
        let err = operator_create_route(&request(&[]), &params(None)).unwrap_err();
        assert!(err.starts_with("product_event_store_required"));
    }

    #[test]
    fn descriptor_without_daemon_validation_fails_closed() {
        let err = operator_create_route(&request(&[]), &params(google("1AbC_-xyz"))).unwrap_err();
        assert!(err.starts_with("product_event_store_unvalidated"));
    }

    #[test]
    fn descriptor_with_mismatched_validated_folder_fails_closed() {
        let req = request(&[
            (DESCRIPTOR_VALIDATION_METADATA_KEY, "valid"),
            (VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY, "other-folder"),
        ]);
        let err = operator_create_route(&req, &params(google("1AbC_-xyz"))).unwrap_err();
        assert!(err.starts_with("product_event_store_unvalidated"));
    }

    #[test]
    fn descriptor_with_non_valid_state_fails_closed() {
        let req = request(&[
            (DESCRIPTOR_VALIDATION_METADATA_KEY, "invalid"),
            (VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY, "1AbC_-xyz"),
        ]);
        let err = operator_create_route(&req, &params(google("1AbC_-xyz"))).unwrap_err();
        assert!(err.starts_with("product_event_store_unvalidated"));
    }

    #[test]
    fn path_like_or_malformed_folder_ids_are_rejected_before_validation() {
        for bad in [
            "",
            " 1AbC",
            "../events",
            "/tmp/bicameral-events",
            "1AbC?x=1",
            "a b",
        ] {
            let req = request(&[
                (DESCRIPTOR_VALIDATION_METADATA_KEY, "valid"),
                (VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY, bad),
            ]);
            let err = operator_create_route(&req, &params(google(bad))).unwrap_err();
            assert!(
                err.starts_with("product_event_store_invalid"),
                "{bad:?} -> {err}"
            );
        }
    }

    #[test]
    fn daemon_validated_descriptor_routes_to_the_google_drive_api_folder() {
        let req = request(&[
            (DESCRIPTOR_VALIDATION_METADATA_KEY, "valid"),
            (VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY, "1AbC_-xyz"),
        ]);
        let route = operator_create_route(&req, &params(google("1AbC_-xyz"))).unwrap();
        assert_eq!(
            route,
            crate::project_registry::EventStoreRoute::GoogleDriveApi {
                folder_id: "1AbC_-xyz".into()
            }
        );
    }
}
