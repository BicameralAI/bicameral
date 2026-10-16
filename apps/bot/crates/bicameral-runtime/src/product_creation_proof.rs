//! `ProductCreationCommitProof` v2 minting from exact provider confirmation
//! (bot#1087, contract frozen by bot#1193 / PR #1194).
//!
//! A proof is minted only from a [`DriveAppendConfirmation`], which the Drive
//! API adapter produces solely after reading the exact `CreateProduct` object
//! back from the provider and comparing it byte-for-byte with the canonical
//! serialization. Provider "append returned success" alone never reaches this
//! module. The canonical bytes and digest come from the shared
//! `bicameral_api::product_creation` helpers so Bot and Cloud hash the same
//! material; no second serialization is introduced here.

use bicameral_api::product_creation::{
    proof_digest_v2, ProductCreationCommitProof, PROOF_SCHEMA_VERSION_V2,
};
use bicameral_gateway::proof_signing::{
    public_key_fingerprint, verify_proof_signature, ProofSignError, ProofSigner,
};
use chrono::SecondsFormat;

use crate::drive_api_event_store::DriveAppendConfirmation;

/// Substrate label carried by every production v2 proof.
pub const PROOF_SUBSTRATE_DRIVE_FOLDER: &str = "drive_folder";
/// Durability label carried by every production v2 proof.
pub const PROOF_DURABILITY_PROVIDER_CONFIRMED_REMOTE: &str = "provider_confirmed_remote";
/// Signature algorithm committed under the proof digest.
pub const PROOF_SIGNATURE_ALGORITHM: &str = "ed25519";

/// Typed minting failures. Messages never carry key material or credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofMintError {
    /// The provider confirmation lacks a fact the proof must bind.
    ConfirmationIncomplete { missing: &'static str },
    /// The canonical digest could not be decoded into 32 bytes.
    DigestMalformed,
    /// The daemon signer refused or failed.
    Sign(ProofSignError),
}

impl std::fmt::Display for ProofMintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfirmationIncomplete { missing } => {
                write!(f, "provider confirmation is incomplete: missing {missing}")
            }
            Self::DigestMalformed => write!(f, "proof digest is malformed"),
            Self::Sign(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProofMintError {}

impl From<ProofSignError> for ProofMintError {
    fn from(error: ProofSignError) -> Self {
        Self::Sign(error)
    }
}

fn require<'a>(value: &'a str, missing: &'static str) -> Result<&'a str, ProofMintError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ProofMintError::ConfirmationIncomplete { missing });
    }
    Ok(trimmed)
}

fn with_sha256_prefix(value: &str) -> String {
    if value.starts_with("sha256:") {
        value.to_string()
    } else {
        format!("sha256:{value}")
    }
}

/// Decode a `sha256:<64 hex>` digest into its raw 32 bytes.
pub fn decode_sha256_digest(value: &str) -> Option<[u8; 32]> {
    let hex = value.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        out[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(out)
}

/// Mint a signed schema-version-2 proof from an exact provider confirmation.
///
/// Every bound fact comes from the confirmation witness or the canonical
/// creation identifiers; a missing provider revision, resource fingerprint, or
/// event digest is a typed failure, never a placeholder. The signature covers
/// the raw 32-byte `proof_digest` computed by [`proof_digest_v2`] over every
/// proof field except `proof_digest` and `signature` (kid and algorithm are
/// under the signature).
pub fn mint_product_creation_commit_proof(
    confirmation: &DriveAppendConfirmation,
    attempt_id: &str,
    product_id: &str,
    event_id: &str,
    signer: &dyn ProofSigner,
) -> Result<ProductCreationCommitProof, ProofMintError> {
    let attempt_id = require(attempt_id, "attempt_id")?;
    let product_id = require(product_id, "product_id")?;
    let event_id = require(event_id, "event_id")?;
    let resource_fingerprint = require(&confirmation.resource_fingerprint, "resource_fingerprint")?;
    let event_digest = require(&confirmation.event_digest, "event_digest")?;
    let provider_revision = confirmation
        .provider_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ProofMintError::ConfirmationIncomplete {
            missing: "provider_revision",
        })?;

    let mut proof = ProductCreationCommitProof {
        schema_version: PROOF_SCHEMA_VERSION_V2,
        attempt_id: attempt_id.to_string(),
        product_id: product_id.to_string(),
        event_id: event_id.to_string(),
        substrate: PROOF_SUBSTRATE_DRIVE_FOLDER.to_string(),
        resource_fingerprint: with_sha256_prefix(resource_fingerprint),
        provider_revision: provider_revision.to_string(),
        event_digest: with_sha256_prefix(event_digest),
        durability: PROOF_DURABILITY_PROVIDER_CONFIRMED_REMOTE.to_string(),
        confirmed_at: confirmation
            .confirmed_at
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        proof_digest: String::new(),
        signer_key_id: signer.signer_key_id()?,
        signature_algorithm: PROOF_SIGNATURE_ALGORITHM.to_string(),
        signature: String::new(),
    };

    let digest = proof_digest_v2(&proof);
    let digest32 = decode_sha256_digest(&digest).ok_or(ProofMintError::DigestMalformed)?;
    proof.signature = signer.sign(&digest32)?;
    proof.proof_digest = digest;
    Ok(proof)
}

/// Verify a v2 proof against a raw Ed25519 public key.
///
/// Checks the frozen field commitments, recomputes the canonical digest, binds
/// `signer_key_id` to the supplied public key fingerprint, and verifies the
/// Ed25519 signature over the raw digest bytes.
pub fn verify_product_creation_commit_proof(
    proof: &ProductCreationCommitProof,
    public_key: &[u8],
) -> bool {
    if proof.schema_version != PROOF_SCHEMA_VERSION_V2
        || proof.substrate != PROOF_SUBSTRATE_DRIVE_FOLDER
        || proof.durability != PROOF_DURABILITY_PROVIDER_CONFIRMED_REMOTE
        || proof.signature_algorithm != PROOF_SIGNATURE_ALGORITHM
    {
        return false;
    }
    if proof.signer_key_id != public_key_fingerprint(public_key) {
        return false;
    }
    if proof_digest_v2(proof) != proof.proof_digest {
        return false;
    }
    let Some(digest32) = decode_sha256_digest(&proof.proof_digest) else {
        return false;
    };
    verify_proof_signature(public_key, &digest32, &proof.signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_gateway::proof_signing::{FixedSeedProofSigner, UnavailableProofSigner};
    use chrono::{TimeZone, Utc};

    fn golden_signer() -> FixedSeedProofSigner {
        let mut seed = [0u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = (index + 1) as u8;
        }
        FixedSeedProofSigner::from_seed(&seed).unwrap()
    }

    fn confirmation() -> DriveAppendConfirmation {
        DriveAppendConfirmation {
            provider_file_id: "file-123".into(),
            provider_version: Some("provider-revision-abc123".into()),
            resource_fingerprint: "1".repeat(64),
            event_digest: "2".repeat(64),
            confirmed_at: Utc.with_ymd_and_hms(2026, 7, 26, 6, 0, 0).unwrap(),
            created: true,
        }
    }

    #[test]
    fn mint_reproduces_the_shared_v2_golden_vector_bytes_digest_and_signature() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../protocol/onboarding/vectors/product-creation-commit-proof-v2.golden.json"
        ))
        .unwrap();
        let proof = mint_product_creation_commit_proof(
            &confirmation(),
            "create-attempt-1",
            "prd_01hzx8checkout",
            "0198d2b4-8a5c-7f22-9e31-9bb014cc8200",
            &golden_signer(),
        )
        .unwrap();
        let expected: ProductCreationCommitProof =
            serde_json::from_value(golden["proof"].clone()).unwrap();
        assert_eq!(
            proof, expected,
            "minted proof must equal the literal golden vector"
        );
        assert_eq!(
            String::from_utf8(bicameral_api::product_creation::proof_canonical_bytes_v2(
                &proof
            ))
            .unwrap(),
            golden["canonical_json"].as_str().unwrap()
        );
        assert!(verify_product_creation_commit_proof(
            &proof,
            &golden_signer().public_key().unwrap()
        ));
    }

    #[test]
    fn verification_rejects_tampered_fields_and_foreign_keys() {
        let signer = golden_signer();
        let proof = mint_product_creation_commit_proof(
            &confirmation(),
            "attempt",
            "prd_x",
            "0198d2b4-8a5c-7f22-9e31-9bb014cc8200",
            &signer,
        )
        .unwrap();
        let public = signer.public_key().unwrap();
        assert!(verify_product_creation_commit_proof(&proof, &public));

        let mut tampered = proof.clone();
        tampered.event_digest = format!("sha256:{}", "3".repeat(64));
        assert!(!verify_product_creation_commit_proof(&tampered, &public));

        let mut resigned_kid = proof.clone();
        resigned_kid.signer_key_id = format!("sha256:{}", "4".repeat(64));
        assert!(!verify_product_creation_commit_proof(
            &resigned_kid,
            &public
        ));

        let other = FixedSeedProofSigner::from_seed(&[7u8; 32]).unwrap();
        assert!(!verify_product_creation_commit_proof(
            &proof,
            &other.public_key().unwrap()
        ));
    }

    #[test]
    fn missing_provider_revision_fails_closed() {
        let mut incomplete = confirmation();
        incomplete.provider_version = None;
        let error = mint_product_creation_commit_proof(
            &incomplete,
            "attempt",
            "prd_x",
            "event",
            &golden_signer(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ProofMintError::ConfirmationIncomplete {
                missing: "provider_revision"
            }
        );
    }

    #[test]
    fn unavailable_signer_never_yields_a_proof() {
        let error = mint_product_creation_commit_proof(
            &confirmation(),
            "attempt",
            "prd_x",
            "event",
            &UnavailableProofSigner,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ProofMintError::Sign(ProofSignError::KeyUnavailable { .. })
        ));
    }
}
