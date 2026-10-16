//! Daemon-canonical hosted Product creation contract types (issue #766).
//!
//! The local daemon is authoritative for Product identity, roles, membership,
//! policy evaluation, and canonical events. A paired hosted user creates a
//! canonical Product **through the daemon**: creation mints the Product
//! identity, materializes the creator's roles atomically (exactly
//! [`ProductRole::PolicyOwner`] + [`ProductRole::ProductOwner`] in a single
//! membership event), and persists an initial Governance Policy Version. Only
//! after the canonical creation commit does hosted Cloud publish a
//! non-canonical [`HostedProjectPreview`] directory row.
//!
//! This module carries the protocol-level types plus the pure authority
//! classification for `product.create`. The runtime crate owns the canonical
//! event materialization, idempotency, and replay reconstruction.
//!
//! See `docs/specs/domain-product-onboarding-contract.md` and the JSON Schemas
//! under `protocol/onboarding/schemas/`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// Canonical Product role.
///
/// `product.create` grants the creator exactly [`ProductRole::PolicyOwner`] and
/// [`ProductRole::ProductOwner`], atomically, in a single membership event.
/// [`ProductRole::ProductMember`] is the downstream join role and is never
/// granted by creation (same-domain member joining is out of scope for #766).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductRole {
    #[serde(rename = "Policy Owner")]
    PolicyOwner,
    #[serde(rename = "Product Owner")]
    ProductOwner,
    #[serde(rename = "Product Member")]
    ProductMember,
}

impl ProductRole {
    /// The canonical wire label used in [`ProductCreationResult::creator_roles`].
    pub fn wire_label(self) -> &'static str {
        match self {
            ProductRole::PolicyOwner => "Policy Owner",
            ProductRole::ProductOwner => "Product Owner",
            ProductRole::ProductMember => "Product Member",
        }
    }
}

/// The exact atomic creator role set materialized by `product.create`.
///
/// Never a partial grant: creation either materializes both roles in one
/// membership event or writes no event at all.
pub const CREATOR_ROLES: [ProductRole; 2] = [ProductRole::PolicyOwner, ProductRole::ProductOwner];

/// The creator roles as canonical wire labels.
pub fn creator_role_labels() -> Vec<String> {
    CREATOR_ROLES
        .iter()
        .map(|r| r.wire_label().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Safe (non-canonical) hosted hints
// ---------------------------------------------------------------------------

/// Safe event-store hint carried in hosted projections.
///
/// Only coarse, non-secret substrate/provider labels — never credentials,
/// tokens, local paths, or raw event-store connection strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventStoreHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substrate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// Safe required-connection hint carried in hosted previews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredConnection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

// ---------------------------------------------------------------------------
// Event-store descriptor (provider-edge freeze, bot#1023)
// ---------------------------------------------------------------------------

/// Explicit, non-secret provider resource selected before `product.create`.
///
/// Routing input only — not canonical Product authority. Tokens resolve inside
/// the daemon's operator-local secrets backend. Alpha freeze: Google Drive folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductEventStoreDescriptor {
    Google { folder_id: String },
}

impl ProductEventStoreDescriptor {
    pub fn substrate(&self) -> crate::event_store::SubstrateKind {
        match self {
            Self::Google { .. } => crate::event_store::SubstrateKind::DriveFolder,
        }
    }
}

// ---------------------------------------------------------------------------
// Command params
// ---------------------------------------------------------------------------

/// Params for `product.create`.
///
/// The caller never supplies authority: the creator's Cloud account is bound by
/// the daemon from the validated hosted session ([`HostedActorContext`]).
/// `asserted_cloud_account_id`, when present, is compared to the bound account
/// and a mismatch is a typed [`ProductCreateDenialReason::WrongCloudAccount`]
/// denial — it can never widen authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCreateParams {
    /// Idempotency key. Retrying with the same key returns the existing
    /// canonical Product and writes no second creation/role event.
    pub idempotency_key: String,

    /// Human-facing Product display name.
    pub product_display_name: String,

    /// Provider resource validated and resolved before canonical append.
    /// Optional during the v0.1.17 restack window; production hosted create must
    /// require it once the Drive adapter path is wired (bot#1023).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store_descriptor: Option<ProductEventStoreDescriptor>,

    /// Cloud account the SPA asserts it is acting for. Optional; when present it
    /// must equal the daemon-bound session account or creation is denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asserted_cloud_account_id: Option<String>,

    /// Non-canonical team/company label for the hosted directory row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_display_name: Option<String>,

    /// Verified domain used for same-domain discovery and the initial canonical
    /// auto-join policy. Product creation persists the policy input atomically;
    /// ticket verification and membership materialization remain downstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_domain: Option<String>,

    /// Safe event-store hint surfaced in the hosted preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store_hint: Option<EventStoreHint>,

    /// Safe required-connection hint surfaced in the hosted preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_connection: Option<RequiredConnection>,
}

/// Params for `product.publish` — the retryable hosted publication step.
///
/// Keyed by the canonical `product_id`; retrying never re-runs canonical
/// creation. The daemon derives the safe publication descriptor from the
/// already-committed canonical creation event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductPublishParams {
    /// Canonical Product id from a prior `ProductCreationResult`.
    pub product_id: String,
    /// Publication idempotency key.
    pub idempotency_key: String,
}

// ---------------------------------------------------------------------------
// Daemon-confirmed actor context
// ---------------------------------------------------------------------------

/// Daemon-confirmed hosted actor context.
///
/// Built by the daemon from the validated hosted bridge session — never
/// caller-supplied. This is the seam that keeps creation authority local: the
/// SPA routes a `ToolRequest`, but the account binding, paired state, and
/// authorization come from the daemon's view of the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedActorContext {
    /// Daemon-derived actor identity (e.g. `bridge:<cloud_account_id>`).
    pub actor_id: String,
    /// Cloud account bound to the validated session.
    pub cloud_account_id: String,
    /// Verified domain bound to the session, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_domain: Option<String>,
    /// Whether the underlying session is paired.
    pub paired: bool,
    /// Whether the bound actor is authorized to create Products.
    pub authorized: bool,
}

// ---------------------------------------------------------------------------
// Typed denials
// ---------------------------------------------------------------------------

/// Why a `product.create` request was denied.
///
/// Every denial writes **no** canonical event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductCreateDenialReason {
    /// The hosted session is not paired.
    UnpairedSession,
    /// The asserted Cloud account does not match the daemon-bound account.
    WrongCloudAccount,
    /// No daemon-confirmed actor context was available.
    MissingActorContext,
    /// The bound actor is not authorized to create Products.
    UnauthorizedActor,
    /// The request payload is structurally invalid or carries unsafe material.
    MalformedRequest,
}

impl ProductCreateDenialReason {
    /// Wire-format code string for `ToolResponse` rejection messages.
    pub fn code(self) -> &'static str {
        match self {
            ProductCreateDenialReason::UnpairedSession => "unpaired_session",
            ProductCreateDenialReason::WrongCloudAccount => "wrong_cloud_account",
            ProductCreateDenialReason::MissingActorContext => "missing_actor_context",
            ProductCreateDenialReason::UnauthorizedActor => "unauthorized_actor",
            ProductCreateDenialReason::MalformedRequest => "malformed_request",
        }
    }
}

/// A typed `product.create` denial with a structured reason and message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductCreateDenial {
    pub reason: ProductCreateDenialReason,
    pub message: String,
}

impl ProductCreateDenial {
    pub fn new(reason: ProductCreateDenialReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }

    /// Wire-format code string.
    pub fn code(&self) -> &'static str {
        self.reason.code()
    }
}

impl std::fmt::Display for ProductCreateDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message)
    }
}

// ---------------------------------------------------------------------------
// Results / projections
// ---------------------------------------------------------------------------

/// Daemon-canonical result of `product.create`.
///
/// Matches `protocol/onboarding/schemas/product-creation-result.schema.json`.
/// `creator_roles` is always exactly `["Policy Owner", "Product Owner"]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCreationResult {
    pub product_id: String,
    pub creator_cloud_account_id: String,
    pub creator_roles: Vec<String>,
    pub membership_event_ref: String,
    pub governance_policy_version: String,
    pub receipt_id: String,
    pub created_at: String,
}

/// Bounded witness that the exact canonical `CreateProduct` event was read
/// back from the selected provider (bot#1023 freeze shape for T-C consumers).
///
/// Not Product state and never replayed as a canonical event. Mint only after
/// provider-confirmed append + exact read-back. Local-only durability is never
/// a valid production proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCreationCommitProof {
    pub schema_version: u32,
    pub attempt_id: String,
    pub product_id: String,
    /// Exact CreateProduct event id (UUID string for wire stability).
    pub event_id: String,
    /// e.g. `drive_folder`
    pub substrate: String,
    pub resource_fingerprint: String,
    pub provider_revision: String,
    pub event_digest: String,
    /// e.g. `provider_confirmed_remote` — never local-only in production.
    pub durability: String,
    pub confirmed_at: String,
    pub proof_digest: String,
    /// Stable SHA-256 fingerprint of the daemon proof-signing public key.
    pub signer_key_id: String,
    /// Always `ed25519` in schema version 2.
    pub signature_algorithm: String,
    /// Base64 Ed25519 signature over the raw 32-byte `proof_digest`.
    pub signature: String,
}

/// Locked schema version for `ProductCreationCommitProof` (factory #901 row 1).
pub const PROOF_SCHEMA_VERSION_V2: u32 = 2;

/// Compact sorted JSON of every proof field except `proof_digest` and `signature`.
///
/// `signer_key_id` and `signature_algorithm` are under the wax. `schema_version`
/// is a JSON number. This is the v2 digest contract (bot#1193).
pub fn proof_canonical_bytes_v2(proof: &ProductCreationCommitProof) -> Vec<u8> {
    proof_canonical_bytes(proof, false)
}

/// Lab v1 bytes (kid/alg excluded). Production verifiers must reject this hash.
pub fn proof_canonical_bytes_v1_lab(proof: &ProductCreationCommitProof) -> Vec<u8> {
    proof_canonical_bytes(proof, true)
}

fn proof_canonical_bytes(proof: &ProductCreationCommitProof, exclude_kid_and_alg: bool) -> Vec<u8> {
    let mut value = serde_json::to_value(proof).expect("proof serializes");
    let obj = value
        .as_object_mut()
        .expect("ProductCreationCommitProof is a JSON object");
    obj.remove("proof_digest");
    obj.remove("signature");
    if exclude_kid_and_alg {
        obj.remove("signer_key_id");
        obj.remove("signature_algorithm");
    }
    let sorted: std::collections::BTreeMap<_, _> =
        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    serde_json::to_vec(&sorted).expect("canonical proof JSON serializes")
}

/// `sha256:<hex>` of [`proof_canonical_bytes_v2`].
pub fn proof_digest_v2(proof: &ProductCreationCommitProof) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(proof_canonical_bytes_v2(proof))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

impl ProductCreationCommitProof {
    /// True when this proof claims remote provider confirmation (not local).
    pub fn is_provider_confirmed_remote(&self) -> bool {
        self.durability == "provider_confirmed_remote"
            || self.durability == "ProviderConfirmedRemote"
    }
}

/// Safe, non-canonical hosted directory projection.
///
/// Matches `protocol/onboarding/schemas/hosted-project-preview.schema.json`.
/// `canonical` is always `false`; the projection never carries canonical
/// membership, roles, or project settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedProjectPreview {
    pub preview_id: String,
    pub product_id: String,
    pub product_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store_hint: Option<EventStoreHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_connection: Option<RequiredConnection>,
    /// Always false.
    pub canonical: bool,
    pub expires_at: String,
}

/// Non-canonical Cloud publication descriptor for a Product.
///
/// Matches `protocol/onboarding/schemas/product-creation-publication.schema.json`.
/// Derived from the canonical creation receipt; retrying with the same
/// `product_id` + `idempotency_key` never creates a second Product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCreationPublication {
    pub publication_id: String,
    pub product_id: String,
    /// Signed equality claim binding the publication to the authenticated
    /// creator Cloud account from the [`ProductCreationResult`]. Never the full
    /// hosted (Supabase) session token, a refresh token, or a provider secret.
    pub creator_cloud_account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store_hint: Option<EventStoreHint>,
    /// Always false.
    pub canonical: bool,
    pub idempotency_key: String,
    pub canonical_creation_receipt_ref: String,
    pub published_at: String,
    /// Signed, provider-confirmed witness for the canonical `CreateProduct`
    /// event this publication derives from (bot#1087 / #1193). Present only
    /// when the daemon minted a schema-version-2 proof after exact provider
    /// readback; Cloud publication fails closed without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_event_commit_proof: Option<ProductCreationCommitProof>,
}

// ---------------------------------------------------------------------------
// Redaction / hosted-safe validation
// ---------------------------------------------------------------------------

/// Substrings that must never appear in receipts, previews, publications, or
/// logs: provider secrets, Supabase tokens, raw event-store credentials, and
/// local filesystem paths.
pub const FORBIDDEN_RECEIPT_SUBSTRINGS: &[&str] = &[
    "eyj",           // JWT / Supabase access + refresh token prefix (lowercased)
    "sb-",           // Supabase auth cookie/token prefix
    "supabase",      // provider/token identifiers
    "service_role",  // Supabase service-role key
    "anon_key",      // Supabase anon key label
    "access_token",  // OAuth/provider access token
    "refresh_token", // OAuth/provider refresh token
    "postgres://",   // raw event-store credential DSN
    "postgresql://", // raw event-store credential DSN
    "/home/",        // local filesystem path
    "/users/",       // local filesystem path (macOS)
    "/var/",         // local filesystem path
    "/tmp/",         // local filesystem path
    "c:\\",          // local filesystem path (Windows)
];

/// Whether `value` contains forbidden secret/credential/path material.
///
/// Case-insensitive substring scan over [`FORBIDDEN_RECEIPT_SUBSTRINGS`].
pub fn contains_forbidden_material(value: &str) -> bool {
    let lower = value.to_lowercase();
    FORBIDDEN_RECEIPT_SUBSTRINGS
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Validate that the hosted-safe, caller-supplied fields of a create request
/// carry no secret/credential/path material.
///
/// Returns a [`ProductCreateDenialReason::MalformedRequest`] denial when unsafe
/// material is detected, so it never reaches a canonical event or a receipt.
pub fn validate_hosted_safe(params: &ProductCreateParams) -> Result<(), ProductCreateDenial> {
    let mut candidates: Vec<&str> = vec![&params.product_display_name, &params.idempotency_key];
    if let Some(v) = &params.team_display_name {
        candidates.push(v);
    }
    if let Some(v) = &params.verified_domain {
        candidates.push(v);
    }
    if let Some(hint) = &params.event_store_hint {
        if let Some(v) = &hint.substrate {
            candidates.push(v);
        }
        if let Some(v) = &hint.provider {
            candidates.push(v);
        }
    }
    if let Some(rc) = &params.required_connection {
        if let Some(v) = &rc.provider {
            candidates.push(v);
        }
    }

    for value in candidates {
        if contains_forbidden_material(value) {
            return Err(ProductCreateDenial::new(
                ProductCreateDenialReason::MalformedRequest,
                "hosted product creation fields must not contain provider secrets, \
                 Supabase tokens, raw event-store credentials, or local paths",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Authority classification
// ---------------------------------------------------------------------------

/// Pure authority classification for `product.create`.
///
/// Returns the daemon-confirmed actor context on success, or a typed denial
/// that the caller must surface **without writing any event**:
///
/// - no actor context               -> [`ProductCreateDenialReason::MissingActorContext`]
/// - unpaired session               -> [`ProductCreateDenialReason::UnpairedSession`]
/// - unauthorized actor             -> [`ProductCreateDenialReason::UnauthorizedActor`]
/// - asserted account mismatch      -> [`ProductCreateDenialReason::WrongCloudAccount`]
/// - unsafe/secret-bearing payload  -> [`ProductCreateDenialReason::MalformedRequest`]
pub fn classify_product_create<'a>(
    params: &ProductCreateParams,
    actor: Option<&'a HostedActorContext>,
) -> Result<&'a HostedActorContext, ProductCreateDenial> {
    let Some(actor) = actor else {
        return Err(ProductCreateDenial::new(
            ProductCreateDenialReason::MissingActorContext,
            "product.create requires a daemon-confirmed hosted actor context",
        ));
    };

    if !actor.paired {
        return Err(ProductCreateDenial::new(
            ProductCreateDenialReason::UnpairedSession,
            "product.create requires a paired hosted session",
        ));
    }

    if !actor.authorized {
        return Err(ProductCreateDenial::new(
            ProductCreateDenialReason::UnauthorizedActor,
            "the bound actor is not authorized to create Products",
        ));
    }

    if let Some(asserted) = &params.asserted_cloud_account_id {
        if asserted != &actor.cloud_account_id {
            return Err(ProductCreateDenial::new(
                ProductCreateDenialReason::WrongCloudAccount,
                "asserted Cloud account does not match the daemon-bound session account",
            ));
        }
    }

    validate_hosted_safe(params)?;

    Ok(actor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ProductCreateParams {
        ProductCreateParams {
            idempotency_key: "idem-1".into(),
            product_display_name: "Acme Checkout".into(),
            event_store_descriptor: Some(ProductEventStoreDescriptor::Google {
                folder_id: "folder-test".into(),
            }),
            asserted_cloud_account_id: None,
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

    #[test]
    fn creator_roles_are_exactly_policy_and_product_owner() {
        assert_eq!(
            creator_role_labels(),
            vec!["Policy Owner".to_string(), "Product Owner".to_string()]
        );
    }

    #[test]
    fn classify_ok_for_paired_authorized_actor() {
        let a = actor();
        assert!(classify_product_create(&params(), Some(&a)).is_ok());
    }

    #[test]
    fn classify_missing_actor_context() {
        let denial = classify_product_create(&params(), None).unwrap_err();
        assert_eq!(
            denial.reason,
            ProductCreateDenialReason::MissingActorContext
        );
    }

    #[test]
    fn classify_unpaired_session() {
        let mut a = actor();
        a.paired = false;
        let denial = classify_product_create(&params(), Some(&a)).unwrap_err();
        assert_eq!(denial.reason, ProductCreateDenialReason::UnpairedSession);
    }

    #[test]
    fn classify_unauthorized_actor() {
        let mut a = actor();
        a.authorized = false;
        let denial = classify_product_create(&params(), Some(&a)).unwrap_err();
        assert_eq!(denial.reason, ProductCreateDenialReason::UnauthorizedActor);
    }

    #[test]
    fn classify_wrong_cloud_account() {
        let a = actor();
        let mut p = params();
        p.asserted_cloud_account_id = Some("acct_other".into());
        let denial = classify_product_create(&p, Some(&a)).unwrap_err();
        assert_eq!(denial.reason, ProductCreateDenialReason::WrongCloudAccount);
    }

    #[test]
    fn classify_rejects_secret_bearing_fields() {
        let a = actor();
        let mut p = params();
        p.team_display_name = Some("token eyJabc.def.ghi".into());
        let denial = classify_product_create(&p, Some(&a)).unwrap_err();
        assert_eq!(denial.reason, ProductCreateDenialReason::MalformedRequest);
    }

    #[test]
    fn classify_rejects_local_path_fields() {
        let a = actor();
        let mut p = params();
        p.product_display_name = "/home/ubuntu/secret".into();
        let denial = classify_product_create(&p, Some(&a)).unwrap_err();
        assert_eq!(denial.reason, ProductCreateDenialReason::MalformedRequest);
    }

    #[test]
    fn forbidden_material_detects_supabase_and_paths() {
        assert!(contains_forbidden_material("SUPABASE_SERVICE_ROLE=abc"));
        assert!(contains_forbidden_material("postgres://u:p@h/db"));
        assert!(contains_forbidden_material("/Users/alice/.bicameral"));
        assert!(!contains_forbidden_material("Acme Checkout"));
    }

    #[test]
    fn commit_proof_round_trips_wire_shape() {
        let proof = ProductCreationCommitProof {
            schema_version: 2,
            attempt_id: "attempt-1".into(),
            product_id: "prod_1".into(),
            event_id: "00000000-0000-4000-8000-000000000001".into(),
            substrate: "drive_folder".into(),
            resource_fingerprint: "sha256:aaaa".into(),
            provider_revision: "rev-1".into(),
            event_digest: "sha256:bbbb".into(),
            durability: "provider_confirmed_remote".into(),
            confirmed_at: "2026-08-03T00:00:00Z".into(),
            proof_digest: "sha256:cccc".into(),
            signer_key_id: "sha256:dddd".into(),
            signature_algorithm: "ed25519".into(),
            signature: "dGVzdA==".into(),
        };
        assert!(proof.is_provider_confirmed_remote());
        let json = serde_json::to_string(&proof).expect("serialize");
        let back: ProductCreationCommitProof = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, proof);
        assert_eq!(back.schema_version, 2);
        assert_eq!(back.signature_algorithm, "ed25519");
    }

    #[test]
    fn v2_proof_digest_includes_kid_and_alg_and_rejects_v1_hash() {
        let mut proof = ProductCreationCommitProof {
            schema_version: PROOF_SCHEMA_VERSION_V2,
            attempt_id: "attempt-1".into(),
            product_id: "prd_01hzx8checkout".into(),
            event_id: "00000000-0000-4000-8000-000000000001".into(),
            substrate: "drive_folder".into(),
            resource_fingerprint: format!("sha256:{}", "a".repeat(64)),
            provider_revision: "rev-1".into(),
            event_digest: format!("sha256:{}", "b".repeat(64)),
            durability: "provider_confirmed_remote".into(),
            confirmed_at: "2026-07-26T06:00:00Z".into(),
            proof_digest: format!("sha256:{}", "c".repeat(64)),
            signer_key_id: format!("sha256:{}", "d".repeat(64)),
            signature_algorithm: "ed25519".into(),
            signature: "dGVzdA==".into(),
        };
        let v2 = proof_digest_v2(&proof);
        proof.proof_digest = v2.clone();
        assert_eq!(
            proof_digest_v2(&proof),
            v2,
            "digest does not hash proof_digest"
        );
        let v1 = {
            use sha2::{Digest, Sha256};
            let hex: String = Sha256::digest(proof_canonical_bytes_v1_lab(&proof))
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!("sha256:{hex}")
        };
        assert_ne!(v2, v1, "v2 must bind kid/alg; v1 lab hash must not verify");
        let bytes = proof_canonical_bytes_v2(&proof);
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("\"signer_key_id\""));
        assert!(text.contains("\"signature_algorithm\""));
        assert!(!text.contains("\"proof_digest\""));
        assert!(!text.contains("\"signature\":"));
        assert!(text.contains("\"schema_version\":2"));
    }

    #[test]
    fn event_digest_is_sha256_of_append_bytes() {
        use crate::event_store::{
            event_store_entry_digest, serialize_event_store_entry, EventStoreEntry, EventType,
            ProductCreatedPayload,
        };
        use chrono::{TimeZone, Utc};
        let payload = ProductCreatedPayload {
            product_id: "prd_01hzx8checkout".into(),
            creator_cloud_account_id: "acct_01hzx8priya".into(),
            creator_roles: vec!["Policy Owner".into(), "Product Owner".into()],
            governance_policy_version: "gpv_1".into(),
            receipt_id: "rcpt_01hzx8create".into(),
            idempotency_key: "create-attempt-1".into(),
            product_display_name: "Checkout".into(),
            team_display_name: Some("Acme".into()),
            verified_domain: Some("bicameral-ai.com".into()),
            event_store_hint: Some(EventStoreHint {
                substrate: Some("drive_folder".into()),
                provider: Some("google".into()),
            }),
            required_connection: Some(RequiredConnection {
                provider: Some("google".into()),
            }),
            created_at: "2026-07-12T08:59:00Z".into(),
        };
        let entry = EventStoreEntry {
            id: uuid::Uuid::parse_str("0198d2b4-8a5c-7f22-9e31-9bb014cc8200").unwrap(),
            event_type: EventType::CreateProduct,
            payload: serde_json::to_value(&payload).unwrap(),
            timestamp: Utc.with_ymd_and_hms(2026, 7, 12, 8, 59, 0).unwrap(),
            actor: "actor-1".into(),
            governance_result_id: None,
        };
        let digest = event_store_entry_digest(&entry).expect("digest");
        let bytes = serialize_event_store_entry(&entry).expect("bytes");
        use sha2::{Digest, Sha256};
        let expected: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(digest, format!("sha256:{expected}"));
        assert_eq!(entry.event_type, EventType::CreateProduct);
        let as_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(as_json["event_type"], "create_product");
        assert!(as_json.get("payload").is_some());
        assert!(
            as_json.get("receipt_id").is_none(),
            "result fields must not leak onto the envelope"
        );
    }

    #[test]
    fn v2_golden_vector_matches_literal_canonical_bytes_and_digest() {
        let raw = include_str!(
            "../../../protocol/onboarding/vectors/product-creation-commit-proof-v2.golden.json"
        );
        let golden: serde_json::Value = serde_json::from_str(raw).expect("golden json");
        let proof_value = golden.get("proof").expect("proof");
        let proof: ProductCreationCommitProof =
            serde_json::from_value(proof_value.clone()).expect("proof type");
        let canonical = String::from_utf8(proof_canonical_bytes_v2(&proof)).expect("utf8");
        assert_eq!(
            canonical,
            golden["canonical_json"].as_str().expect("canonical_json"),
            "Bot canonical bytes must match the shared literal vector"
        );
        assert_eq!(
            proof_digest_v2(&proof),
            golden["proof_digest"].as_str().expect("proof_digest"),
            "Bot digest must match the shared literal vector"
        );
        assert_eq!(proof.schema_version, PROOF_SCHEMA_VERSION_V2);
        assert!(canonical.contains("\"signer_key_id\""));
        assert!(canonical.contains("\"signature_algorithm\""));
    }

    #[test]
    fn event_store_descriptor_google_is_drive_folder() {
        let d = ProductEventStoreDescriptor::Google {
            folder_id: "fld".into(),
        };
        assert_eq!(
            d.substrate(),
            crate::event_store::SubstrateKind::DriveFolder
        );
    }
}
