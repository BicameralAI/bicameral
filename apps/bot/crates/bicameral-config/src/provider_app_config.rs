//! Cloud-owned provider application configuration consumed by the local daemon.
//!
//! This module deliberately models only non-secret OAuth *application* metadata.
//! Google user authorization codes, PKCE verifiers, access tokens, refresh tokens,
//! and provider Connection truth remain daemon-local and must never be represented
//! by this contract.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// Current schema version for [`GoogleNativeAppConfigV1`].
pub const GOOGLE_NATIVE_APP_CONFIG_SCHEMA_VERSION: u32 = 1;
/// Canonical provider id used by the Connection service.
pub const GOOGLE_PROVIDER_ID: &str = "google";
/// Google OAuth client class used by the installed/local daemon path.
pub const GOOGLE_NATIVE_CLIENT_TYPE: &str = "native";

/// Bounded, non-secret Google OAuth application configuration published by Cloud.
///
/// Cloud is authoritative for this application identity/configuration. The local
/// daemon remains authoritative for PKCE generation, callback/state validation,
/// authorization-code exchange, Google token custody/refresh, and
/// `ConnectionRegistry` state.
///
/// Unknown fields are rejected intentionally. In particular, a publisher that
/// accidentally adds a `client_secret`, authorization code, provider token, or
/// other credential-bearing field must fail closed at deserialization rather
/// than being silently accepted by an older daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleNativeAppConfigV1 {
    /// Closed schema version. Must equal [`GOOGLE_NATIVE_APP_CONFIG_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Must equal [`GOOGLE_PROVIDER_ID`].
    pub provider: String,
    /// Must equal [`GOOGLE_NATIVE_CLIENT_TYPE`].
    pub client_type: String,
    /// Public Google OAuth client id. This is application identity, not a secret.
    pub client_id: String,
    /// Bicameral deployment environment this application identity belongs to.
    pub environment: String,
    /// Cloud-controlled rotation/version identifier for this configuration.
    pub config_version: String,
    /// Version identifier for the approved/minimized Google scope set.
    pub scope_set_version: String,
    /// Approved OAuth scopes for this application configuration.
    pub scopes: Vec<String>,
    /// Cloud publication timestamp.
    pub issued_at: DateTime<Utc>,
    /// Time after which a daemon must reject this configuration and re-bootstrap.
    pub expires_at: DateTime<Utc>,
    /// Cloud-controlled kill switch for this application configuration.
    pub enabled: bool,
}

impl GoogleNativeAppConfigV1 {
    /// Validate this payload for a daemon running in `expected_environment`.
    ///
    /// This validates payload semantics only. Transport authentication or signed
    /// artifact verification remains the responsibility of the bootstrap layer.
    pub fn validate_for(
        &self,
        expected_environment: &str,
        now: DateTime<Utc>,
    ) -> Result<(), GoogleNativeAppConfigError> {
        if self.schema_version != GOOGLE_NATIVE_APP_CONFIG_SCHEMA_VERSION {
            return Err(GoogleNativeAppConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
            });
        }
        if self.provider != GOOGLE_PROVIDER_ID {
            return Err(GoogleNativeAppConfigError::UnexpectedProvider {
                found: self.provider.clone(),
            });
        }
        if self.client_type != GOOGLE_NATIVE_CLIENT_TYPE {
            return Err(GoogleNativeAppConfigError::UnexpectedClientType {
                found: self.client_type.clone(),
            });
        }
        if self.client_id.trim().is_empty() {
            return Err(GoogleNativeAppConfigError::MissingClientId);
        }
        if self.environment.trim().is_empty() {
            return Err(GoogleNativeAppConfigError::MissingEnvironment);
        }
        if expected_environment.trim().is_empty() {
            return Err(GoogleNativeAppConfigError::MissingExpectedEnvironment);
        }
        if self.environment != expected_environment {
            return Err(GoogleNativeAppConfigError::EnvironmentMismatch {
                expected: expected_environment.to_string(),
                found: self.environment.clone(),
            });
        }
        if self.config_version.trim().is_empty() {
            return Err(GoogleNativeAppConfigError::MissingConfigVersion);
        }
        if self.scope_set_version.trim().is_empty() {
            return Err(GoogleNativeAppConfigError::MissingScopeSetVersion);
        }
        if self.scopes.is_empty() {
            return Err(GoogleNativeAppConfigError::EmptyScopes);
        }

        let mut distinct_scopes = BTreeSet::new();
        for (index, scope) in self.scopes.iter().enumerate() {
            if scope.trim().is_empty() {
                return Err(GoogleNativeAppConfigError::BlankScope { index });
            }
            if !distinct_scopes.insert(scope.as_str()) {
                return Err(GoogleNativeAppConfigError::DuplicateScope {
                    scope: scope.clone(),
                });
            }
        }

        if self.expires_at <= self.issued_at {
            return Err(GoogleNativeAppConfigError::InvalidValidityWindow);
        }
        if !self.enabled {
            return Err(GoogleNativeAppConfigError::Disabled);
        }
        if now >= self.expires_at {
            return Err(GoogleNativeAppConfigError::Expired {
                expired_at: self.expires_at,
            });
        }

        Ok(())
    }
}

/// Fail-closed reasons for a Cloud-published Google application config payload.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GoogleNativeAppConfigError {
    #[error("unsupported Google app-config schema version: {found}")]
    UnsupportedSchemaVersion { found: u32 },
    #[error("unexpected provider in Google app config: {found}")]
    UnexpectedProvider { found: String },
    #[error("unexpected Google OAuth client type: {found}")]
    UnexpectedClientType { found: String },
    #[error("Google OAuth client id is missing")]
    MissingClientId,
    #[error("Google app-config environment is missing")]
    MissingEnvironment,
    #[error("daemon expected environment is missing")]
    MissingExpectedEnvironment,
    #[error("Google app-config environment mismatch: expected {expected}, found {found}")]
    EnvironmentMismatch { expected: String, found: String },
    #[error("Google app-config version is missing")]
    MissingConfigVersion,
    #[error("Google scope-set version is missing")]
    MissingScopeSetVersion,
    #[error("Google app config contains no OAuth scopes")]
    EmptyScopes,
    #[error("Google app config contains a blank OAuth scope at index {index}")]
    BlankScope { index: usize },
    #[error("Google app config contains duplicate OAuth scope: {scope}")]
    DuplicateScope { scope: String },
    #[error("Google app-config expiry must be later than its issue time")]
    InvalidValidityWindow,
    #[error("Google app config has been disabled by Cloud")]
    Disabled,
    #[error("Google app config expired at {expired_at}")]
    Expired { expired_at: DateTime<Utc> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn valid_config() -> GoogleNativeAppConfigV1 {
        GoogleNativeAppConfigV1 {
            schema_version: GOOGLE_NATIVE_APP_CONFIG_SCHEMA_VERSION,
            provider: GOOGLE_PROVIDER_ID.into(),
            client_type: GOOGLE_NATIVE_CLIENT_TYPE.into(),
            client_id: "123456.apps.googleusercontent.com".into(),
            environment: "alpha".into(),
            config_version: "google-alpha-7".into(),
            scope_set_version: "drive-alpha-v1".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
            issued_at: Utc.with_ymd_and_hms(2026, 8, 12, 20, 0, 0).unwrap(),
            expires_at: Utc.with_ymd_and_hms(2026, 8, 13, 20, 0, 0).unwrap(),
            enabled: true,
        }
    }

    #[test]
    fn accepts_valid_non_secret_native_app_config() {
        let config = valid_config();
        let now = Utc.with_ymd_and_hms(2026, 8, 12, 22, 0, 0).unwrap();
        assert_eq!(config.validate_for("alpha", now), Ok(()));
    }

    #[test]
    fn rejects_wrong_environment() {
        let config = valid_config();
        let now = Utc.with_ymd_and_hms(2026, 8, 12, 22, 0, 0).unwrap();
        assert_eq!(
            config.validate_for("prod", now),
            Err(GoogleNativeAppConfigError::EnvironmentMismatch {
                expected: "prod".into(),
                found: "alpha".into(),
            })
        );
    }

    #[test]
    fn rejects_expired_or_disabled_config() {
        let mut config = valid_config();
        let after_expiry = Utc.with_ymd_and_hms(2026, 8, 14, 0, 0, 0).unwrap();
        assert!(matches!(
            config.validate_for("alpha", after_expiry),
            Err(GoogleNativeAppConfigError::Expired { .. })
        ));

        config.expires_at = Utc.with_ymd_and_hms(2026, 8, 15, 20, 0, 0).unwrap();
        config.enabled = false;
        assert_eq!(
            config.validate_for("alpha", after_expiry),
            Err(GoogleNativeAppConfigError::Disabled)
        );
    }

    #[test]
    fn rejects_duplicate_or_blank_scopes() {
        let now = Utc.with_ymd_and_hms(2026, 8, 12, 22, 0, 0).unwrap();
        let mut config = valid_config();
        config.scopes.push(config.scopes[0].clone());
        assert!(matches!(
            config.validate_for("alpha", now),
            Err(GoogleNativeAppConfigError::DuplicateScope { .. })
        ));

        config.scopes = vec![" ".into()];
        assert_eq!(
            config.validate_for("alpha", now),
            Err(GoogleNativeAppConfigError::BlankScope { index: 0 })
        );
    }

    #[test]
    fn rejects_credential_bearing_unknown_fields() {
        let raw = r#"{
            "schema_version":1,
            "provider":"google",
            "client_type":"native",
            "client_id":"123456.apps.googleusercontent.com",
            "environment":"alpha",
            "config_version":"google-alpha-7",
            "scope_set_version":"drive-alpha-v1",
            "scopes":["https://www.googleapis.com/auth/drive.file"],
            "issued_at":"2026-08-12T20:00:00Z",
            "expires_at":"2026-08-13T20:00:00Z",
            "enabled":true,
            "client_secret":"must-not-cross-this-boundary"
        }"#;

        let error = serde_json::from_str::<GoogleNativeAppConfigV1>(raw).unwrap_err();
        assert!(error.to_string().contains("unknown field `client_secret`"));
    }
}
