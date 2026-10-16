//! Mint PairingAuthorizationV1 at LOCAL_APPROVED using Option-1 key.

use crate::proof_signing::{verify_proof_signature, ProofSignError, ProofSigner};
use base64::Engine;
use bicameral_api::pairing_authorization::{
    derived_identity_key_id, format_instant_z, pairing_authorization_digest_bytes,
    validate_pairing_authorization_local, PairingAuthorizationV1, PAIRING_AUTHORIZATION_AUDIENCE,
    PAIRING_AUTHORIZATION_PROFILE, PAIRING_AUTHORIZATION_PURPOSE,
    PAIRING_AUTHORIZATION_SIGNATURE_ALGORITHM, PAIRING_AUTHORIZATION_TTL_SECS,
};

/// Inputs for minting an enrollment authorization at local approve.
pub struct MintPairingAuthorizationRequest<'a> {
    pub pairing_id: &'a str,
    pub protocol_session_id: &'a str,
    pub cloud_account_id: &'a str,
    pub approved_origin: &'a str,
    pub daemon_instance_id: &'a str,
    pub team_id: &'a str,
    pub now: chrono::DateTime<chrono::Utc>,
    pub ttl_secs: Option<i64>,
}

/// Mint failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintPairingAuthorizationError {
    Sign(ProofSignError),
    TeamIdRequired,
    Validation(&'static str),
}

impl std::fmt::Display for MintPairingAuthorizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sign(e) => write!(f, "{e}"),
            Self::TeamIdRequired => write!(
                f,
                "team_id is required to mint PairingAuthorizationV1 for Cloud enroll"
            ),
            Self::Validation(reason) => write!(f, "minted authorization invalid: {reason}"),
        }
    }
}

impl std::error::Error for MintPairingAuthorizationError {}

/// Mint a signed PairingAuthorizationV1 with the Option-1 key.
///
/// Alpha TOFU: `issuer_key_id` equals `identity_key_id` (same Option-1 kid).
pub fn mint_pairing_authorization_v1(
    req: &MintPairingAuthorizationRequest<'_>,
    signer: &dyn ProofSigner,
) -> Result<PairingAuthorizationV1, MintPairingAuthorizationError> {
    if req.team_id.trim().is_empty() {
        return Err(MintPairingAuthorizationError::TeamIdRequired);
    }

    let public_key = signer
        .public_key()
        .map_err(MintPairingAuthorizationError::Sign)?;
    let identity_public_key_b64 = base64::engine::general_purpose::STANDARD.encode(&public_key);
    let identity_key_id = derived_identity_key_id(&public_key);
    let issuer_key_id = identity_key_id.clone();
    let ttl = req.ttl_secs.unwrap_or(PAIRING_AUTHORIZATION_TTL_SECS);
    let expires_at = req.now + chrono::Duration::seconds(ttl);

    let mut auth = PairingAuthorizationV1 {
        schema_version: 1,
        profile: PAIRING_AUTHORIZATION_PROFILE.into(),
        pairing_id: req.pairing_id.to_string(),
        protocol_session_id: req.protocol_session_id.to_string(),
        cloud_account_id: req.cloud_account_id.to_string(),
        approved_origin: req.approved_origin.to_string(),
        daemon_instance_id: req.daemon_instance_id.to_string(),
        team_id: req.team_id.to_string(),
        identity_public_key_b64,
        identity_key_id,
        audience: PAIRING_AUTHORIZATION_AUDIENCE.into(),
        purpose: PAIRING_AUTHORIZATION_PURPOSE.into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        issued_at: format_instant_z(req.now),
        expires_at: format_instant_z(expires_at),
        issuer_key_id,
        signature_algorithm: PAIRING_AUTHORIZATION_SIGNATURE_ALGORITHM.into(),
        signature_b64: String::new(),
    };

    let digest = pairing_authorization_digest_bytes(&auth);
    auth.signature_b64 = signer
        .sign(&digest)
        .map_err(MintPairingAuthorizationError::Sign)?;

    validate_pairing_authorization_local(&auth, req.now)
        .map_err(|e| MintPairingAuthorizationError::Validation(e.reason))?;

    Ok(auth)
}

/// Verify signature_b64 against the Option-1 public key in the grant.
pub fn verify_pairing_authorization_signature(auth: &PairingAuthorizationV1) -> bool {
    let Ok(public_key) =
        base64::engine::general_purpose::STANDARD.decode(&auth.identity_public_key_b64)
    else {
        return false;
    };
    if public_key.len() != 32 {
        return false;
    }
    let digest = pairing_authorization_digest_bytes(auth);
    verify_proof_signature(&public_key, &digest, &auth.signature_b64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_signing::KeyringProofSigner;
    use bicameral_api::validate_pairing_authorization_local;
    use bicameral_config::InMemorySecretsStore;
    use chrono::TimeZone;
    use std::sync::Arc;

    fn mint_sample(signer: &dyn ProofSigner) -> PairingAuthorizationV1 {
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 0, 0).unwrap();
        mint_pairing_authorization_v1(
            &MintPairingAuthorizationRequest {
                pairing_id: "pairing-1",
                protocol_session_id: "protocol-1",
                cloud_account_id: "acct-1",
                approved_origin: "https://app.bicameral-ai.com",
                daemon_instance_id: "daemon-1",
                team_id: "team-1",
                now,
                ttl_secs: Some(300),
            },
            signer,
        )
        .unwrap()
    }

    #[test]
    fn mint_uses_option1_kid_and_verifies() {
        let signer = KeyringProofSigner::new(Arc::new(InMemorySecretsStore::new()));
        let auth = mint_sample(&signer);
        assert_eq!(auth.identity_key_id, signer.signer_key_id().unwrap());
        assert_eq!(auth.issuer_key_id, auth.identity_key_id);
        assert_eq!(auth.profile, PAIRING_AUTHORIZATION_PROFILE);
        assert!(verify_pairing_authorization_signature(&auth));
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 1, 0).unwrap();
        assert!(validate_pairing_authorization_local(&auth, now).is_ok());
    }

    #[test]
    fn mint_fails_closed_without_team_id() {
        let signer = KeyringProofSigner::new(Arc::new(InMemorySecretsStore::new()));
        let err = mint_pairing_authorization_v1(
            &MintPairingAuthorizationRequest {
                pairing_id: "pairing-1",
                protocol_session_id: "protocol-1",
                cloud_account_id: "acct-1",
                approved_origin: "https://app.bicameral-ai.com",
                daemon_instance_id: "daemon-1",
                team_id: "",
                now: chrono::Utc::now(),
                ttl_secs: None,
            },
            &signer,
        )
        .unwrap_err();
        assert_eq!(err, MintPairingAuthorizationError::TeamIdRequired);
    }

    #[test]
    fn local_validator_rejects_expired_minted_grant() {
        let signer = KeyringProofSigner::new(Arc::new(InMemorySecretsStore::new()));
        let auth = mint_sample(&signer);
        let later = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 10, 0).unwrap();
        assert_eq!(
            validate_pairing_authorization_local(&auth, later)
                .unwrap_err()
                .reason,
            "pairing_authorization expired"
        );
    }
}
