//! PairingAuthorizationV1 — daemon-minted enrollment grant (factory #901 row 12).
//!
//! Wire companion to Cloud #305. Short TTL, one-shot consume on Cloud. Signature
//! covers SHA-256 of compact sorted JSON excluding `signature_b64`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Wire profile for enrollment authorization.
pub const PAIRING_AUTHORIZATION_PROFILE: &str = "bicameral.pairing_authorization.v1";
/// Cloud audience for enrollment grants.
pub const PAIRING_AUTHORIZATION_AUDIENCE: &str = "bicameral-cloud";
/// Purpose bound into the grant.
pub const PAIRING_AUTHORIZATION_PURPOSE: &str = "identity_enrollment";
/// Fixed signature algorithm.
pub const PAIRING_AUTHORIZATION_SIGNATURE_ALGORITHM: &str = "ed25519";
/// Alpha default grant TTL (~5 minutes).
pub const PAIRING_AUTHORIZATION_TTL_SECS: i64 = 300;
/// Cloud appraisal maximum TTL (10 minutes).
pub const PAIRING_AUTHORIZATION_MAX_TTL_SECS: i64 = 600;

/// Daemon-minted enrollment authorization (Cloud #305 wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingAuthorizationV1 {
    pub schema_version: u32,
    pub profile: String,
    pub pairing_id: String,
    pub protocol_session_id: String,
    pub cloud_account_id: String,
    pub approved_origin: String,
    pub daemon_instance_id: String,
    pub team_id: String,
    pub identity_public_key_b64: String,
    pub identity_key_id: String,
    pub audience: String,
    pub purpose: String,
    pub nonce: String,
    pub issued_at: String,
    pub expires_at: String,
    pub issuer_key_id: String,
    pub signature_algorithm: String,
    pub signature_b64: String,
}

/// Local validation failure for PairingAuthorizationV1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingAuthorizationValidationError {
    pub reason: &'static str,
}

impl std::fmt::Display for PairingAuthorizationValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for PairingAuthorizationValidationError {}

/// Compact sorted JSON of every field except `signature_b64`, then SHA-256.
pub fn pairing_authorization_digest_bytes(auth: &PairingAuthorizationV1) -> [u8; 32] {
    let value = serde_json::to_value(auth).expect("PairingAuthorizationV1 serializes");
    let mut map = match value {
        serde_json::Value::Object(map) => map,
        _ => panic!("PairingAuthorizationV1 must serialize as object"),
    };
    map.remove("signature_b64");
    let canonical = canonical_json_string(&serde_json::Value::Object(map));
    Sha256::digest(canonical.as_bytes()).into()
}

/// Hex form of [`pairing_authorization_digest_bytes`].
pub fn pairing_authorization_digest_hex(auth: &PairingAuthorizationV1) -> String {
    pairing_authorization_digest_bytes(auth)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Derive `sha256:<hex>` kid from a raw 32-byte Ed25519 public key.
pub fn derived_identity_key_id(public_key: &[u8]) -> String {
    let hex: String = Sha256::digest(public_key)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// Format an instant as ISO-8601 with `Z` (no offset, no fractional seconds).
pub fn format_instant_z(instant: chrono::DateTime<chrono::Utc>) -> String {
    instant.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_instant_z(value: &str) -> Result<chrono::DateTime<chrono::Utc>, ()> {
    let trimmed = value.trim();
    if !trimmed.ends_with('Z') {
        return Err(());
    }
    let with_offset = trimmed.replacen('Z', "+00:00", 1);
    chrono::DateTime::parse_from_rfc3339(&with_offset)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| ())
}

fn require_non_empty(
    value: &str,
    reason: &'static str,
) -> Result<(), PairingAuthorizationValidationError> {
    if value.trim().is_empty() {
        Err(PairingAuthorizationValidationError { reason })
    } else {
        Ok(())
    }
}

/// Local fail-closed validator (schema, bindings, freshness). Does not verify
/// the Ed25519 signature — callers that hold the public key should verify
/// separately.
pub fn validate_pairing_authorization_local(
    auth: &PairingAuthorizationV1,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), PairingAuthorizationValidationError> {
    if auth.schema_version != 1 {
        return Err(PairingAuthorizationValidationError {
            reason: "schema_version must be 1",
        });
    }
    if auth.profile != PAIRING_AUTHORIZATION_PROFILE {
        return Err(PairingAuthorizationValidationError {
            reason: "profile must be bicameral.pairing_authorization.v1",
        });
    }
    if auth.audience != PAIRING_AUTHORIZATION_AUDIENCE {
        return Err(PairingAuthorizationValidationError {
            reason: "audience must be bicameral-cloud",
        });
    }
    if auth.purpose != PAIRING_AUTHORIZATION_PURPOSE {
        return Err(PairingAuthorizationValidationError {
            reason: "purpose must be identity_enrollment",
        });
    }
    if auth.signature_algorithm != PAIRING_AUTHORIZATION_SIGNATURE_ALGORITHM {
        return Err(PairingAuthorizationValidationError {
            reason: "signature_algorithm must be ed25519",
        });
    }
    require_non_empty(&auth.pairing_id, "pairing_id missing")?;
    require_non_empty(&auth.protocol_session_id, "protocol_session_id missing")?;
    require_non_empty(&auth.cloud_account_id, "cloud_account_id missing")?;
    require_non_empty(&auth.approved_origin, "approved_origin missing")?;
    require_non_empty(&auth.daemon_instance_id, "daemon_instance_id missing")?;
    require_non_empty(&auth.team_id, "team_id missing")?;
    require_non_empty(
        &auth.identity_public_key_b64,
        "identity_public_key_b64 missing",
    )?;
    require_non_empty(&auth.identity_key_id, "identity_key_id missing")?;
    require_non_empty(&auth.nonce, "nonce missing")?;
    require_non_empty(&auth.issuer_key_id, "issuer_key_id missing")?;
    require_non_empty(&auth.signature_b64, "signature_b64 missing")?;

    let issued_at =
        parse_instant_z(&auth.issued_at).map_err(|_| PairingAuthorizationValidationError {
            reason: "issued_at must be ISO-8601 with Z",
        })?;
    let expires_at =
        parse_instant_z(&auth.expires_at).map_err(|_| PairingAuthorizationValidationError {
            reason: "expires_at must be ISO-8601 with Z",
        })?;
    if expires_at <= issued_at {
        return Err(PairingAuthorizationValidationError {
            reason: "expires_at must be after issued_at",
        });
    }
    let ttl = (expires_at - issued_at).num_seconds();
    if ttl > PAIRING_AUTHORIZATION_MAX_TTL_SECS {
        return Err(PairingAuthorizationValidationError {
            reason: "TTL exceeds maximum",
        });
    }
    if now >= expires_at {
        return Err(PairingAuthorizationValidationError {
            reason: "pairing_authorization expired",
        });
    }
    Ok(())
}

fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<&String, &serde_json::Value> = map.iter().collect();
            let mut out = String::from("{");
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("string key serializes"));
                out.push(':');
                out.push_str(&canonical_json_string(v));
            }
            out.push('}');
            out
        }
        serde_json::Value::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_json_string(item));
            }
            out.push(']');
            out
        }
        other => serde_json::to_string(other).expect("scalar serializes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_auth(expires_at: &str) -> PairingAuthorizationV1 {
        PairingAuthorizationV1 {
            schema_version: 1,
            profile: PAIRING_AUTHORIZATION_PROFILE.into(),
            pairing_id: "pairing-1".into(),
            protocol_session_id: "protocol-1".into(),
            cloud_account_id: "acct-1".into(),
            approved_origin: "https://app.bicameral-ai.com".into(),
            daemon_instance_id: "daemon-1".into(),
            team_id: "team-1".into(),
            identity_public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            identity_key_id: "sha256:deadbeef".into(),
            audience: PAIRING_AUTHORIZATION_AUDIENCE.into(),
            purpose: PAIRING_AUTHORIZATION_PURPOSE.into(),
            nonce: "nonce-1".into(),
            issued_at: "2026-09-02T12:00:00Z".into(),
            expires_at: expires_at.into(),
            issuer_key_id: "sha256:deadbeef".into(),
            signature_algorithm: PAIRING_AUTHORIZATION_SIGNATURE_ALGORITHM.into(),
            signature_b64: "sig".into(),
        }
    }

    #[test]
    fn digest_excludes_signature_and_is_stable() {
        let mut auth = sample_auth("2026-09-02T12:05:00Z");
        let d1 = pairing_authorization_digest_hex(&auth);
        auth.signature_b64 = "different".into();
        let d2 = pairing_authorization_digest_hex(&auth);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn validator_rejects_expired_and_missing_team() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 10, 0).unwrap();
        let expired = sample_auth("2026-09-02T12:05:00Z");
        assert_eq!(
            validate_pairing_authorization_local(&expired, now)
                .unwrap_err()
                .reason,
            "pairing_authorization expired"
        );

        let mut missing_team = sample_auth("2026-09-02T12:15:00Z");
        missing_team.team_id.clear();
        assert_eq!(
            validate_pairing_authorization_local(&missing_team, now)
                .unwrap_err()
                .reason,
            "team_id missing"
        );
    }

    #[test]
    fn validator_accepts_fresh_grant() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 1, 0).unwrap();
        let auth = sample_auth("2026-09-02T12:05:00Z");
        assert!(validate_pairing_authorization_local(&auth, now).is_ok());
    }
}
