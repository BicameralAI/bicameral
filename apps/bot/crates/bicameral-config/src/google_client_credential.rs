//! Daemon-local Google Desktop client-credential envelope (bot#1211).
//!
//! Google's Desktop OAuth registration remains a native/public client and PKCE
//! remains mandatory. This envelope stores provider-required request material
//! only. It is distinct from user OAuth tokens and from ConnectionRegistry
//! metadata, and production persistence belongs exclusively to the OS keyring.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GOOGLE_CLIENT_CREDENTIAL_ENVELOPE_VERSION: u32 = 1;
pub const GOOGLE_CLIENT_CREDENTIAL_KEY: &str = "client_secret";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleClientCredentialEnvelopeV1 {
    pub envelope_version: u32,
    pub client_secret: String,
    pub client_id: String,
    pub config_version: String,
    pub obtained_at: DateTime<Utc>,
}

impl std::fmt::Debug for GoogleClientCredentialEnvelopeV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleClientCredentialEnvelopeV1")
            .field("envelope_version", &self.envelope_version)
            .field("client_secret", &"[REDACTED]")
            .field("client_id", &self.client_id)
            .field("config_version", &self.config_version)
            .field("obtained_at", &self.obtained_at)
            .finish()
    }
}

impl GoogleClientCredentialEnvelopeV1 {
    pub fn new(
        client_secret: impl Into<String>,
        client_id: impl Into<String>,
        config_version: impl Into<String>,
        obtained_at: DateTime<Utc>,
    ) -> Result<Self, GoogleClientCredentialEnvelopeError> {
        let envelope = Self {
            envelope_version: GOOGLE_CLIENT_CREDENTIAL_ENVELOPE_VERSION,
            client_secret: client_secret.into(),
            client_id: client_id.into(),
            config_version: config_version.into(),
            obtained_at,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), GoogleClientCredentialEnvelopeError> {
        if self.envelope_version != GOOGLE_CLIENT_CREDENTIAL_ENVELOPE_VERSION {
            return Err(GoogleClientCredentialEnvelopeError::UnsupportedVersion(
                self.envelope_version,
            ));
        }
        if self.client_secret.trim().is_empty() {
            return Err(GoogleClientCredentialEnvelopeError::MissingClientSecret);
        }
        if self.client_id.trim().is_empty() {
            return Err(GoogleClientCredentialEnvelopeError::MissingClientId);
        }
        if self.config_version.trim().is_empty() {
            return Err(GoogleClientCredentialEnvelopeError::MissingConfigVersion);
        }
        Ok(())
    }

    pub fn matches_binding(&self, client_id: &str, config_version: &str) -> bool {
        self.validate().is_ok()
            && self.client_id == client_id
            && self.config_version == config_version
    }

    pub fn to_secret_string(&self) -> Result<String, GoogleClientCredentialEnvelopeError> {
        self.validate()?;
        serde_json::to_string(self)
            .map_err(|error| GoogleClientCredentialEnvelopeError::Serialization(error.to_string()))
    }

    pub fn from_secret_string(value: &str) -> Result<Self, GoogleClientCredentialEnvelopeError> {
        let envelope: Self = serde_json::from_str(value).map_err(|error| {
            GoogleClientCredentialEnvelopeError::Serialization(error.to_string())
        })?;
        envelope.validate()?;
        Ok(envelope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GoogleClientCredentialEnvelopeError {
    #[error("unsupported Google client-credential envelope version: {0}")]
    UnsupportedVersion(u32),
    #[error("Google Desktop client credential is missing")]
    MissingClientSecret,
    #[error("Google Desktop credential client id is missing")]
    MissingClientId,
    #[error("Google Desktop credential config version is missing")]
    MissingConfigVersion,
    #[error("Google Desktop credential envelope serialization failed: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_binding_without_debug_secret_leak() {
        let envelope = GoogleClientCredentialEnvelopeV1::new(
            "provider-secret",
            "client.apps.googleusercontent.com",
            "google-production-7",
            Utc::now(),
        )
        .unwrap();
        let serialized = envelope.to_secret_string().unwrap();
        let parsed = GoogleClientCredentialEnvelopeV1::from_secret_string(&serialized).unwrap();
        assert_eq!(parsed, envelope);
        assert!(parsed.matches_binding("client.apps.googleusercontent.com", "google-production-7"));
        let debug = format!("{parsed:?}");
        assert!(!debug.contains("provider-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn mismatch_and_incomplete_envelopes_fail_closed() {
        let envelope = GoogleClientCredentialEnvelopeV1::new(
            "provider-secret",
            "client.apps.googleusercontent.com",
            "google-production-7",
            Utc::now(),
        )
        .unwrap();
        assert!(
            !envelope.matches_binding("other.apps.googleusercontent.com", "google-production-7")
        );
        assert!(
            !envelope.matches_binding("client.apps.googleusercontent.com", "google-production-8")
        );
        assert!(matches!(
            GoogleClientCredentialEnvelopeV1::new(
                "",
                "client.apps.googleusercontent.com",
                "google-production-7",
                Utc::now(),
            ),
            Err(GoogleClientCredentialEnvelopeError::MissingClientSecret)
        ));
    }
}
