//! Restart-safe Google Drive route composition for Row 5 (bot#1211).
//!
//! The existing Drive transport already sends a non-empty `client_secret` on
//! refresh grants. This module supplies that configuration after a real daemon
//! restart by reading the separately versioned Google Desktop credential
//! envelope from the same OS keyring boundary used by production provider
//! secrets. No Cloud request is required to reopen an existing grant.

use std::sync::Arc;

use bicameral_config::{
    GoogleClientCredentialEnvelopeV1, GoogleOAuthConfig, OsKeyringSecretsStore, SecretsStore,
    GOOGLE_CLIENT_CREDENTIAL_KEY,
};

fn credential_config_from_store(store: &dyn SecretsStore) -> Option<GoogleOAuthConfig> {
    let raw = store.read_secret(
        bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
        GOOGLE_CLIENT_CREDENTIAL_KEY,
    )?;
    let envelope = GoogleClientCredentialEnvelopeV1::from_secret_string(&raw).ok()?;
    Some(GoogleOAuthConfig {
        client_id: envelope.client_id,
        client_secret: envelope.client_secret,
        // Redirect URI/scopes are not used by refresh grants. They are kept
        // empty here deliberately rather than reconstructed from stale/public
        // application metadata during restart.
        redirect_uri: String::new(),
        scopes: Vec::new(),
    })
}

/// Open the durable Google Drive route with restart-safe client credential.
///
/// Explicit legacy env configuration retains precedence for the lab-only
/// override path. Normal production composition reads the Desktop credential
/// envelope locally and then delegates to the existing Drive transport. If the
/// envelope is missing/malformed, the old opener is used and any required
/// refresh fails closed as authentication rather than silently adopting a new
/// application identity or reaching Cloud.
pub fn open_google_drive_api_route(
    folder_id: &str,
) -> crate::event_store_adapters::DualEventStoreAdapter {
    if let Ok(concrete) = open_google_drive_api_adapter(folder_id) {
        return crate::event_store_adapters::DualEventStoreAdapter {
            event_store: concrete.clone(),
            atomic: concrete,
        };
    }

    crate::drive_api_transport_base::open_google_drive_api_route(folder_id)
}

/// Open the EXACT production Drive API adapter (concrete type retained) so the
/// create path can append with provider readback (bot#1087 resolver step 5).
///
/// Same credential/config resolution as [`open_google_drive_api_route`]; the
/// only difference is that the concrete adapter is returned instead of being
/// erased behind the dual adapter. No local, memory, or fixture substrate is
/// ever substituted here.
pub fn open_google_drive_api_adapter(
    folder_id: &str,
) -> Result<
    Arc<crate::drive_api_event_store::DriveApiAdapter>,
    bicameral_api::event_store::EventStoreError,
> {
    let store = Arc::new(OsKeyringSecretsStore::new());
    let oauth_config =
        GoogleOAuthConfig::from_env().or_else(|| credential_config_from_store(store.as_ref()));
    crate::drive_api_transport_base::production_drive_event_store(folder_id, store, oauth_config)
        .map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_config::InMemorySecretsStore;
    use chrono::Utc;

    #[test]
    fn credential_envelope_reconstructs_refresh_config_without_cloud() {
        let store = InMemorySecretsStore::new();
        let envelope = GoogleClientCredentialEnvelopeV1::new(
            "desktop-provider-secret",
            "desktop.apps.googleusercontent.com",
            "google-production-7",
            Utc::now(),
        )
        .unwrap();
        store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                GOOGLE_CLIENT_CREDENTIAL_KEY,
                &envelope.to_secret_string().unwrap(),
            )
            .unwrap();

        let config = credential_config_from_store(&store).unwrap();
        assert_eq!(config.client_id, "desktop.apps.googleusercontent.com");
        assert_eq!(config.client_secret, "desktop-provider-secret");
        assert!(config.redirect_uri.is_empty());
        assert!(config.scopes.is_empty());
    }

    #[test]
    fn malformed_or_missing_envelope_never_manufactures_refresh_identity() {
        let store = InMemorySecretsStore::new();
        assert!(credential_config_from_store(&store).is_none());
        store
            .write_secret(
                bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
                GOOGLE_CLIENT_CREDENTIAL_KEY,
                "not-json",
            )
            .unwrap();
        assert!(credential_config_from_store(&store).is_none());
    }
}
