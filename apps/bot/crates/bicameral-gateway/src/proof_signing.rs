//! Option-1 Ed25519 proof-signing key (daemon.proof_signing / ed25519_pkcs8).
//!
//! Same keyring coordinates as the #1087 mint path. Do not use join-ticket keys.

use base64::Engine;
use bicameral_config::SecretsStore;
use ring::signature::KeyPair;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Keyring coordinates for the daemon proof-signing key (OQ1 / Option-1).
pub const PROOF_SIGNING_SOURCE_ID: &str = "daemon.proof_signing";
pub const PROOF_SIGNING_KEY: &str = "ed25519_pkcs8";

/// Typed signing failures. Messages never contain key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofSignError {
    KeyUnavailable { safe_message: String },
    KeyInvalid,
    SignatureFailed,
}

impl std::fmt::Display for ProofSignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyUnavailable { safe_message } => {
                write!(f, "proof signing key unavailable: {safe_message}")
            }
            Self::KeyInvalid => write!(f, "stored proof signing key is not usable"),
            Self::SignatureFailed => write!(f, "proof signature could not be produced"),
        }
    }
}

impl std::error::Error for ProofSignError {}

/// Source of Option-1 proof / enrollment signatures.
pub trait ProofSigner: Send + Sync {
    fn signer_key_id(&self) -> Result<String, ProofSignError>;
    fn sign(&self, digest32: &[u8; 32]) -> Result<String, ProofSignError>;
    fn public_key(&self) -> Result<Vec<u8>, ProofSignError>;
}

/// Keyring-backed Option-1 signer (generate-once with read-back convergence).
pub struct KeyringProofSigner {
    secrets_store: Arc<dyn SecretsStore>,
}

impl KeyringProofSigner {
    pub fn new(secrets_store: Arc<dyn SecretsStore>) -> Self {
        Self { secrets_store }
    }

    fn load_or_generate_pkcs8(&self) -> Result<Vec<u8>, ProofSignError> {
        let decode = |value: String| {
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .map_err(|_| ProofSignError::KeyInvalid)
        };

        if let Some(stored) = self
            .secrets_store
            .read_secret(PROOF_SIGNING_SOURCE_ID, PROOF_SIGNING_KEY)
        {
            return decode(stored);
        }

        let rng = ring::rand::SystemRandom::new();
        let document = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|_| ProofSignError::SignatureFailed)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(document.as_ref());
        self.secrets_store
            .write_secret(PROOF_SIGNING_SOURCE_ID, PROOF_SIGNING_KEY, &encoded)
            .map_err(|error| ProofSignError::KeyUnavailable {
                safe_message: format!("keyring write failed: {error}"),
            })?;

        let stored = self
            .secrets_store
            .read_secret(PROOF_SIGNING_SOURCE_ID, PROOF_SIGNING_KEY)
            .ok_or_else(|| ProofSignError::KeyUnavailable {
                safe_message: "keyring readback after generate returned nothing".into(),
            })?;
        decode(stored)
    }

    fn keypair(&self) -> Result<ring::signature::Ed25519KeyPair, ProofSignError> {
        let pkcs8 = self.load_or_generate_pkcs8()?;
        ring::signature::Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| ProofSignError::KeyInvalid)
    }
}

impl ProofSigner for KeyringProofSigner {
    fn signer_key_id(&self) -> Result<String, ProofSignError> {
        Ok(public_key_fingerprint(&self.public_key()?))
    }

    fn sign(&self, digest32: &[u8; 32]) -> Result<String, ProofSignError> {
        let keypair = self.keypair()?;
        let signature = keypair.sign(digest32);
        Ok(base64::engine::general_purpose::STANDARD.encode(signature.as_ref()))
    }

    fn public_key(&self) -> Result<Vec<u8>, ProofSignError> {
        Ok(self.keypair()?.public_key().as_ref().to_vec())
    }
}

/// Deterministic Ed25519 signer over a fixed 32-byte seed.
///
/// For deterministic tests and shared golden vectors only (the Bot<->Cloud v2
/// vector uses seed bytes `01..20`). Production daemons must use
/// [`KeyringProofSigner`]; nothing in the production composition constructs
/// this type.
pub struct FixedSeedProofSigner {
    keypair: ring::signature::Ed25519KeyPair,
}

impl FixedSeedProofSigner {
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self, ProofSignError> {
        let keypair = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| ProofSignError::KeyInvalid)?;
        Ok(Self { keypair })
    }
}

impl ProofSigner for FixedSeedProofSigner {
    fn signer_key_id(&self) -> Result<String, ProofSignError> {
        Ok(public_key_fingerprint(self.keypair.public_key().as_ref()))
    }

    fn sign(&self, digest32: &[u8; 32]) -> Result<String, ProofSignError> {
        let signature = self.keypair.sign(digest32);
        Ok(base64::engine::general_purpose::STANDARD.encode(signature.as_ref()))
    }

    fn public_key(&self) -> Result<Vec<u8>, ProofSignError> {
        Ok(self.keypair.public_key().as_ref().to_vec())
    }
}

/// Signer whose key is never available; every operation fails closed with
/// [`ProofSignError::KeyUnavailable`]. Test double for the "secure store cannot
/// produce the daemon signing key" branch.
pub struct UnavailableProofSigner;

impl ProofSigner for UnavailableProofSigner {
    fn signer_key_id(&self) -> Result<String, ProofSignError> {
        Err(ProofSignError::KeyUnavailable {
            safe_message: "proof signing key store is unavailable".into(),
        })
    }

    fn sign(&self, _digest32: &[u8; 32]) -> Result<String, ProofSignError> {
        self.signer_key_id().map(|_| String::new())
    }

    fn public_key(&self) -> Result<Vec<u8>, ProofSignError> {
        self.signer_key_id().map(|_| Vec::new())
    }
}

/// `sha256:<hex>` fingerprint of a raw Ed25519 public key; the `signer_key_id`
/// carried under the proof signature.
pub fn public_key_fingerprint(public_key: &[u8]) -> String {
    let hex: String = Sha256::digest(public_key)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// Verify a base64 Ed25519 signature over a raw 32-byte digest.
pub fn verify_proof_signature(public_key: &[u8], digest32: &[u8; 32], signature_b64: &str) -> bool {
    let Ok(signature) = base64::engine::general_purpose::STANDARD.decode(signature_b64) else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(digest32, &signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_config::InMemorySecretsStore;

    #[test]
    fn generate_once_stable_across_instances() {
        let store: Arc<dyn SecretsStore> = Arc::new(InMemorySecretsStore::new());
        let first = KeyringProofSigner::new(store.clone());
        let second = KeyringProofSigner::new(store);
        assert_eq!(
            first.signer_key_id().unwrap(),
            second.signer_key_id().unwrap()
        );
        assert_eq!(first.public_key().unwrap(), second.public_key().unwrap());
    }

    #[test]
    fn fixed_seed_signer_matches_shared_v2_golden_vector_key() {
        let mut seed = [0u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = (index + 1) as u8;
        }
        let signer = FixedSeedProofSigner::from_seed(&seed).unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD.encode(signer.public_key().unwrap()),
            "ebVWLo/mVPlAeLES6KmLp5AfhTrmlb7X4OORC60ElmQ="
        );
        assert_eq!(
            signer.signer_key_id().unwrap(),
            "sha256:65b60673d6ed884bf01c2c222d82ada0740f29ac3355d6a925c81f17f47a27b8"
        );
    }

    #[test]
    fn unavailable_signer_fails_closed() {
        assert!(matches!(
            UnavailableProofSigner.signer_key_id(),
            Err(ProofSignError::KeyUnavailable { .. })
        ));
        assert!(UnavailableProofSigner.sign(&[0u8; 32]).is_err());
    }

    #[test]
    fn sign_verify_round_trip() {
        let signer = KeyringProofSigner::new(Arc::new(InMemorySecretsStore::new()));
        let digest = [0x42u8; 32];
        let signature = signer.sign(&digest).unwrap();
        assert!(verify_proof_signature(
            &signer.public_key().unwrap(),
            &digest,
            &signature
        ));
        assert!(!verify_proof_signature(
            &signer.public_key().unwrap(),
            &[0x43u8; 32],
            &signature
        ));
    }
}
