//! Daemon-local Google OAuth PKCE helpers (bot#1114).
//!
//! PKCE verifier material is ephemeral provider-auth session state. It never
//! belongs in Cloud, browser persistence, ConnectionRegistry, logs, or durable
//! Product state.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Generate a PKCE verifier using only RFC 7636 unreserved characters.
///
/// Two UUID v4 values encoded as lowercase hex produce 64 characters, safely
/// inside the RFC 7636 43..=128 verifier length requirement.
pub fn generate_pkce_verifier() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Compute the S256 PKCE challenge for a verifier.
pub fn pkce_s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Reject malformed or missing verifier material before a token exchange.
pub fn validate_pkce_verifier(verifier: &str) -> Result<(), &'static str> {
    let len = verifier.len();
    if !(43..=128).contains(&len) {
        return Err("PKCE verifier length is outside 43..=128 characters");
    }
    if !verifier.bytes().all(|b| {
        matches!(
            b,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'.'
                | b'_'
                | b'~'
        )
    }) {
        return Err("PKCE verifier contains characters outside the unreserved set");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_verifier_is_valid_and_unique() {
        let first = generate_pkce_verifier();
        let second = generate_pkce_verifier();
        validate_pkce_verifier(&first).unwrap();
        validate_pkce_verifier(&second).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn s256_challenge_matches_rfc_7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_s256_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn malformed_verifier_fails_closed() {
        assert!(validate_pkce_verifier("short").is_err());
        assert!(validate_pkce_verifier(&"a".repeat(129)).is_err());
        assert!(validate_pkce_verifier(&format!("{}!", "a".repeat(42))).is_err());
    }
}
