//! Production Product-create commit composition (bot#1087 / #1193):
//! validated folder -> exact provider adapter -> canonical `CreateProduct`
//! append -> exact provider readback -> canonical event digest -> signed
//! `ProductCreationCommitProof` v2 -> proof returned to the caller.
//!
//! Provider behaviour is a deterministic in-memory `DriveApiTransport` double
//! with explicit fault knobs (readback corruption, missing provider revision).
//! The signer is the fixed test key shared with the Bot<->Cloud golden vector.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bicameral_api::event_store::{
    event_store_entry_digest, serialize_event_store_entry, EventStoreAdapter, EventStoreEntry,
    EventType,
};
use bicameral_api::product_creation::{
    contains_forbidden_material, proof_canonical_bytes_v2, proof_digest_v2, EventStoreHint,
    HostedActorContext, ProductCreateParams, ProductCreationCommitProof,
    ProductEventStoreDescriptor, RequiredConnection, PROOF_SCHEMA_VERSION_V2,
};
use bicameral_api::tool_request::*;
use bicameral_gateway::proof_signing::{
    public_key_fingerprint, FixedSeedProofSigner, ProofSigner, UnavailableProofSigner,
};
use bicameral_runtime::dispatch::{dispatch_with_hosted_product, HostedProductDispatchContext};
use bicameral_runtime::drive_api_event_store::{
    DriveApiAdapter, DriveApiTransport, DriveRemoteObject, DriveTransportError,
    DriveTransportErrorKind,
};
use bicameral_runtime::product_creation::{
    create_product_with_commit, reconcile_commit_proof, replay_products, ProductCreateCommit,
    ProductCreateError,
};
use bicameral_runtime::product_creation_proof::verify_product_creation_commit_proof;
use chrono::Utc;

const FOLDER: &str = "1FolderProofTest";
const SECRET_MARKER: &str = "ya29.SUPER-SECRET-ACCESS-TOKEN";

#[derive(Default)]
struct FakeDriveState {
    files: BTreeMap<String, (String, Vec<u8>, u64)>, // file_id -> (name, bytes, revision)
    next_id: u64,
}

/// In-memory Google Drive folder with explicit fault injection.
struct FakeDrive {
    state: Mutex<FakeDriveState>,
    creates: AtomicUsize,
    /// Serve corrupted bytes on readback (provider mismatch).
    corrupt_readback: bool,
    /// Omit the provider revision from created objects.
    omit_revision: bool,
    /// Pretend the transport holds a credential that must never leak.
    secret: String,
}

impl FakeDrive {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeDriveState::default()),
            creates: AtomicUsize::new(0),
            corrupt_readback: false,
            omit_revision: false,
            secret: SECRET_MARKER.into(),
        })
    }

    fn with(corrupt_readback: bool, omit_revision: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeDriveState::default()),
            creates: AtomicUsize::new(0),
            corrupt_readback,
            omit_revision,
            secret: SECRET_MARKER.into(),
        })
    }

    fn object_count(&self) -> usize {
        self.state.lock().unwrap().files.len()
    }

    fn stored_bytes(&self, name: &str) -> Vec<u8> {
        self.state
            .lock()
            .unwrap()
            .files
            .values()
            .find(|(n, _, _)| n == name)
            .map(|(_, bytes, _)| bytes.clone())
            .expect("object stored")
    }
}

impl DriveApiTransport for FakeDrive {
    fn find_children_by_name(
        &self,
        folder_id: &str,
        name: &str,
    ) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
        if folder_id != FOLDER {
            return Err(DriveTransportError::new(
                DriveTransportErrorKind::NotFound,
                "folder is not reachable with this credential",
            ));
        }
        let state = self.state.lock().unwrap();
        Ok(state
            .files
            .iter()
            .filter(|(_, (n, _, _))| n == name)
            .map(|(id, (n, _, rev))| DriveRemoteObject {
                file_id: id.clone(),
                name: n.clone(),
                version: (!self.omit_revision).then(|| rev.to_string()),
            })
            .collect())
    }

    fn create_json_file(
        &self,
        folder_id: &str,
        name: &str,
        body: &[u8],
    ) -> Result<DriveRemoteObject, DriveTransportError> {
        if folder_id != FOLDER {
            return Err(DriveTransportError::new(
                DriveTransportErrorKind::NotFound,
                "folder is not reachable with this credential",
            ));
        }
        self.creates.fetch_add(1, Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        state.next_id += 1;
        let file_id = format!("file-{}", state.next_id);
        let revision = 100 + state.next_id;
        state
            .files
            .insert(file_id.clone(), (name.to_string(), body.to_vec(), revision));
        Ok(DriveRemoteObject {
            file_id,
            name: name.to_string(),
            version: (!self.omit_revision).then(|| revision.to_string()),
        })
    }

    fn download_file(&self, file_id: &str) -> Result<Vec<u8>, DriveTransportError> {
        let state = self.state.lock().unwrap();
        let (_, bytes, _) = state.files.get(file_id).ok_or_else(|| {
            DriveTransportError::new(DriveTransportErrorKind::NotFound, "object missing")
        })?;
        if self.corrupt_readback {
            let mut corrupted = bytes.clone();
            corrupted.extend_from_slice(b"\n");
            return Ok(corrupted);
        }
        Ok(bytes.clone())
    }

    fn list_children(
        &self,
        folder_id: &str,
    ) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
        if folder_id != FOLDER {
            return Err(DriveTransportError::new(
                DriveTransportErrorKind::NotFound,
                "folder is not reachable with this credential",
            ));
        }
        let state = self.state.lock().unwrap();
        Ok(state
            .files
            .iter()
            .map(|(id, (n, _, rev))| DriveRemoteObject {
                file_id: id.clone(),
                name: n.clone(),
                version: Some(rev.to_string()),
            })
            .collect())
    }

    fn probe_folder(&self, folder_id: &str) -> Result<(), DriveTransportError> {
        if folder_id == FOLDER {
            Ok(())
        } else {
            Err(DriveTransportError::new(
                DriveTransportErrorKind::NotFound,
                "folder is not reachable with this credential",
            ))
        }
    }
}

fn golden_signer() -> Arc<FixedSeedProofSigner> {
    let mut seed = [0u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = (index + 1) as u8;
    }
    Arc::new(FixedSeedProofSigner::from_seed(&seed).unwrap())
}

fn adapter_for(drive: &Arc<FakeDrive>, folder: &str) -> Arc<DriveApiAdapter> {
    Arc::new(DriveApiAdapter::new(folder, drive.clone() as Arc<dyn DriveApiTransport>).unwrap())
}

fn commit(adapter: &Arc<DriveApiAdapter>, signer: Arc<dyn ProofSigner>) -> ProductCreateCommit {
    ProductCreateCommit {
        confirmed_append: adapter.clone(),
        signer,
    }
}

fn params(idempotency_key: &str, folder: &str) -> ProductCreateParams {
    ProductCreateParams {
        idempotency_key: idempotency_key.into(),
        product_display_name: "Acme Checkout".into(),
        event_store_descriptor: Some(ProductEventStoreDescriptor::Google {
            folder_id: folder.into(),
        }),
        asserted_cloud_account_id: Some("acct_1".into()),
        team_display_name: Some("Acme".into()),
        verified_domain: Some("acme.example".into()),
        event_store_hint: Some(EventStoreHint {
            substrate: Some("drive_folder".into()),
            provider: Some("google".into()),
        }),
        required_connection: Some(RequiredConnection {
            provider: Some("google".into()),
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

fn create_request(idempotency_key: &str) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::ProductCreate(params(idempotency_key, FOLDER)),
        AuthorityContext {
            actor_id: "bridge:acct_1".into(),
            auth_method: AuthMethod::HostedBridge,
            session_id: Some("bridge-sess".into()),
            workspace: None,
            policy_scope: vec!["hosted_bridge".into()],
            audit_metadata: serde_json::Map::new(),
        },
    )
}

// 1 + 2 + 3: valid resource -> provider append/readback -> signed proof whose
// signature verifies and whose digest binds the exact canonical bytes.
#[test]
fn validated_folder_append_readback_yields_signed_proof_bound_to_exact_event_bytes() {
    let drive = FakeDrive::new();
    let adapter = adapter_for(&drive, FOLDER);
    let signer = golden_signer();
    let commit = commit(&adapter, signer.clone());

    let outcome = create_product_with_commit(
        &params("idem-1", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap();
    assert!(outcome.created);
    let proof = outcome
        .first_event_commit_proof
        .expect("provider-confirmed create mints a proof");

    // Frozen v2 shape.
    assert_eq!(proof.schema_version, PROOF_SCHEMA_VERSION_V2);
    assert_eq!(proof.substrate, "drive_folder");
    assert_eq!(proof.durability, "provider_confirmed_remote");
    assert_eq!(proof.signature_algorithm, "ed25519");
    assert_eq!(proof.product_id, outcome.result.product_id);
    assert_eq!(proof.attempt_id, "idem-1");
    assert_eq!(proof.provider_revision, "101");
    assert!(proof.confirmed_at.ends_with('Z'));

    // Signature verifies against the daemon public key; kid is its fingerprint.
    let public = signer.public_key().unwrap();
    assert_eq!(proof.signer_key_id, public_key_fingerprint(&public));
    assert!(verify_product_creation_commit_proof(&proof, &public));
    assert_eq!(proof.proof_digest, proof_digest_v2(&proof));
    assert!(!proof_canonical_bytes_v2(&proof).is_empty());

    // The event digest is SHA-256 of the exact bytes the provider holds, which
    // are the canonical serialization of the replayed CreateProduct entry.
    let entries = adapter.replay().unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.event_type == EventType::CreateProduct)
        .unwrap();
    assert_eq!(proof.event_id, entry.id.to_string());
    assert_eq!(proof.event_digest, event_store_entry_digest(entry).unwrap());
    let stored = drive.stored_bytes(&format!("{}.json", entry.id));
    assert_eq!(stored, serialize_event_store_entry(entry).unwrap());
    assert_eq!(
        proof.resource_fingerprint.len(),
        71,
        "resource fingerprint is sha256-prefixed"
    );
    assert_eq!(drive.creates.load(Ordering::SeqCst), 1);
}

// 4: wrong / unvalidated folder -> no append, no proof.
#[test]
fn wrong_folder_yields_no_append_and_no_proof() {
    let drive = FakeDrive::new();
    let adapter = adapter_for(&drive, "1SomeOtherFolder");
    let commit = commit(&adapter, golden_signer());

    let error = create_product_with_commit(
        &params("idem-wrong", "1SomeOtherFolder"),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap_err();
    assert!(
        matches!(error, ProductCreateError::Store(_)),
        "unexpected error: {error:?}"
    );
    assert_eq!(drive.creates.load(Ordering::SeqCst), 0);
    assert_eq!(drive.object_count(), 0);
}

// 5: provider readback mismatch -> no proof, no success.
#[test]
fn provider_readback_mismatch_yields_no_proof_and_no_success() {
    let drive = FakeDrive::with(true, false);
    let adapter = adapter_for(&drive, FOLDER);
    let commit = commit(&adapter, golden_signer());

    let error = create_product_with_commit(
        &params("idem-mismatch", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap_err();
    assert!(
        matches!(error, ProductCreateError::ProviderReadback(_)),
        "unexpected error: {error:?}"
    );

    // Through dispatch the response is a typed error, never Ok.
    let ctx = HostedProductDispatchContext {
        actor: Some(actor()),
        commit: Some(commit),
    };
    let response = dispatch_with_hosted_product(
        &create_request("idem-mismatch-2"),
        adapter.as_ref(),
        None,
        None,
        &ctx,
    );
    assert_ne!(response.status, ToolResponseStatus::Ok);
    assert!(response
        .message
        .as_deref()
        .unwrap_or("")
        .starts_with("provider_readback_mismatch"));
}

// 5b: a provider confirmation without a revision cannot mint a proof.
#[test]
fn missing_provider_revision_yields_no_proof() {
    let drive = FakeDrive::with(false, true);
    let adapter = adapter_for(&drive, FOLDER);
    let commit = commit(&adapter, golden_signer());
    let error = create_product_with_commit(
        &params("idem-norev", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap_err();
    assert!(matches!(error, ProductCreateError::ProofMint(_)));
}

// 6: signer unavailable -> no append, no false confirmed success.
#[test]
fn unavailable_signer_writes_nothing_and_claims_no_success() {
    let drive = FakeDrive::new();
    let adapter = adapter_for(&drive, FOLDER);
    let commit = commit(&adapter, Arc::new(UnavailableProofSigner));

    let error = create_product_with_commit(
        &params("idem-nosigner", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ProductCreateError::ProofSignerUnavailable(_)
    ));
    assert_eq!(drive.creates.load(Ordering::SeqCst), 0);
    assert_eq!(drive.object_count(), 0);

    let ctx = HostedProductDispatchContext {
        actor: Some(actor()),
        commit: Some(commit),
    };
    let response = dispatch_with_hosted_product(
        &create_request("idem-nosigner"),
        adapter.as_ref(),
        None,
        None,
        &ctx,
    );
    assert_ne!(response.status, ToolResponseStatus::Ok);
    assert!(response
        .message
        .as_deref()
        .unwrap_or("")
        .starts_with("proof_signer_unavailable"));
    assert_eq!(drive.object_count(), 0);
}

// 7: same idempotency key retry -> one Product, one event, one provider
// object, and a proof over the same facts (reconciled, not re-appended).
#[test]
fn same_idempotency_key_retry_converges_on_one_product_event_and_object() {
    let drive = FakeDrive::new();
    let adapter = adapter_for(&drive, FOLDER);
    let signer = golden_signer();
    let commit = commit(&adapter, signer.clone());

    let first = create_product_with_commit(
        &params("idem-retry", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap();
    let second = create_product_with_commit(
        &params("idem-retry", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap();

    assert!(first.created);
    assert!(!second.created);
    assert_eq!(first.result.product_id, second.result.product_id);
    assert_eq!(
        first.result.membership_event_ref,
        second.result.membership_event_ref
    );
    assert_eq!(drive.creates.load(Ordering::SeqCst), 1);
    assert_eq!(drive.object_count(), 1);
    let ledger = replay_products(adapter.as_ref()).unwrap();
    assert_eq!(ledger.create_event_count, 1);

    let a = first.first_event_commit_proof.unwrap();
    let b = second.first_event_commit_proof.unwrap();
    for proof in [&a, &b] {
        assert!(verify_product_creation_commit_proof(
            proof,
            &signer.public_key().unwrap()
        ));
    }
    assert_eq!(a.event_id, b.event_id);
    assert_eq!(a.event_digest, b.event_digest);
    assert_eq!(a.resource_fingerprint, b.resource_fingerprint);
    assert_eq!(a.provider_revision, b.provider_revision);
    assert_eq!(a.product_id, b.product_id);

    // Publication-time reconciliation re-confirms the same object.
    let reconciled = reconcile_commit_proof(&a.product_id, adapter.as_ref(), &commit)
        .unwrap()
        .expect("existing Product reconciles to a proof");
    assert_eq!(reconciled.event_id, a.event_id);
    assert_eq!(reconciled.event_digest, a.event_digest);
    assert_eq!(drive.creates.load(Ordering::SeqCst), 1);
    assert!(
        reconcile_commit_proof("prd_unknown", adapter.as_ref(), &commit)
            .unwrap()
            .is_none()
    );
}

// 8: secret material cannot enter the proof, the response, or log text.
#[test]
fn proof_and_response_carry_no_secret_material() {
    let drive = FakeDrive::new();
    let adapter = adapter_for(&drive, FOLDER);
    let signer = golden_signer();
    let commit = commit(&adapter, signer.clone());
    let ctx = HostedProductDispatchContext {
        actor: Some(actor()),
        commit: Some(commit),
    };
    let response = dispatch_with_hosted_product(
        &create_request("idem-secret"),
        adapter.as_ref(),
        None,
        None,
        &ctx,
    );
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let body = serde_json::to_string(&response).unwrap();
    let seed_b64 = {
        use base64::Engine;
        let mut seed = [0u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = (index + 1) as u8;
        }
        base64::engine::general_purpose::STANDARD.encode(seed)
    };
    assert!(!body.contains(&drive.secret), "transport credential leaked");
    assert!(!body.contains(&seed_b64), "signing seed leaked");
    assert!(
        !contains_forbidden_material(&body),
        "forbidden material in response: {body}"
    );
    let proof: ProductCreationCommitProof =
        serde_json::from_value(response.result.unwrap()["first_event_commit_proof"].clone())
            .unwrap();
    let proof_text = serde_json::to_string(&proof).unwrap();
    assert!(!proof_text.contains(SECRET_MARKER));
    assert!(!contains_forbidden_material(&proof_text));
    // The Debug rendering of the commit handles never prints key material.
    let debug = format!("{:?}", ctx);
    assert!(!debug.contains(&seed_b64));
}

// 3b: a proof over a different CreateProduct entry does not verify against
// the canonical bytes of this one (digest binds the exact event).
#[test]
fn proof_digest_binds_the_exact_canonical_event() {
    let drive = FakeDrive::new();
    let adapter = adapter_for(&drive, FOLDER);
    let signer = golden_signer();
    let commit = commit(&adapter, signer.clone());
    let outcome = create_product_with_commit(
        &params("idem-bind", FOLDER),
        Some(&actor()),
        adapter.as_ref(),
        Some(&commit),
        Utc::now(),
    )
    .unwrap();
    let proof = outcome.first_event_commit_proof.unwrap();

    let other = EventStoreEntry::new(
        EventType::CreateProduct,
        serde_json::json!({"product_id": "prd_other"}),
        "bridge:acct_1".into(),
    );
    assert_ne!(
        proof.event_digest,
        event_store_entry_digest(&other).unwrap()
    );

    let mut forged = proof.clone();
    forged.event_digest = event_store_entry_digest(&other).unwrap();
    assert!(!verify_product_creation_commit_proof(
        &forged,
        &signer.public_key().unwrap()
    ));
}
