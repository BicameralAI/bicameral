//! Production OS-backed [`SecretsStore`] implementation.
//!
//! Persists secret material in the platform credential store via the
//! `keyring` crate: Windows Credential Manager (`windows-native`), macOS
//! Keychain (`apple-native`), and the Freedesktop Secret Service on Linux
//! (`sync-secret-service`). Keystore selection is compile-time via crate
//! features declared in the workspace manifest; no mock or in-memory
//! keystore is ever reachable from this type.
//!
//! There is deliberately no fallback path: when the OS keystore is
//! unavailable, reads surface as missing credentials so callers fail
//! closed, and writes/deletes return explicit non-secret errors. Callers
//! that need volatile storage for deterministic tests must inject
//! [`crate::InMemorySecretsStore`] explicitly.

use crate::connections::SecretsStore;
use keyring::Entry;

/// Service-name prefix for all Bicameral-owned credentials.
const SERVICE_PREFIX: &str = "bicameral";

/// Deterministic keyring coordinates for a `{source_id, key}` pair.
///
/// `{source_id, key}` maps to service `bicameral.<source_id>` and account
/// `<key>`. This mapping is part of the durable contract: changing it
/// would orphan previously stored credentials.
pub fn keyring_location(source_id: &str, key: &str) -> (String, String) {
    (format!("{SERVICE_PREFIX}.{source_id}"), key.to_string())
}

/// Production OS-keyring secrets store.
///
/// Construction is infallible and never probes the keystore; every
/// operation talks to the OS credential store directly. Secret values
/// never appear in error text, tracing fields, or panics.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeyringSecretsStore;

impl OsKeyringSecretsStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(source_id: &str, key: &str) -> keyring::Result<Entry> {
        let (service, account) = keyring_location(source_id, key);
        Entry::new(&service, &account)
    }
}

impl SecretsStore for OsKeyringSecretsStore {
    fn read_secret(&self, source_id: &str, key: &str) -> Option<String> {
        let entry = match Self::entry(source_id, key) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(source_id, key, error = %error, "OS keyring entry unavailable");
                return None;
            }
        };
        match entry.get_password() {
            Ok(value) => Some(value),
            Err(keyring::Error::NoEntry) => None,
            Err(error) => {
                // Keyring-layer errors describe the platform operation and
                // never contain the stored value.
                tracing::warn!(source_id, key, error = %error, "OS keyring read failed");
                None
            }
        }
    }

    fn write_secret(&self, source_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
        let entry = Self::entry(source_id, key).map_err(|error| {
            anyhow::anyhow!("OS keyring entry for {source_id}/{key} unavailable: {error}")
        })?;
        entry.set_password(value).map_err(|error| {
            anyhow::anyhow!("OS keyring write for {source_id}/{key} failed: {error}")
        })
    }

    fn delete_secret(&self, source_id: &str, key: &str) -> anyhow::Result<()> {
        let entry = Self::entry(source_id, key).map_err(|error| {
            anyhow::anyhow!("OS keyring entry for {source_id}/{key} unavailable: {error}")
        })?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow::anyhow!(
                "OS keyring delete for {source_id}/{key} failed: {error}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_location_is_deterministic_and_collision_free() {
        assert_eq!(
            keyring_location("connection.google", "oauth_token"),
            (
                "bicameral.connection.google".to_string(),
                "oauth_token".to_string()
            )
        );
        assert_eq!(
            keyring_location("google_drive", "oauth_token"),
            (
                "bicameral.google_drive".to_string(),
                "oauth_token".to_string()
            )
        );
        // Same strings repeat to the same coordinates.
        assert_eq!(
            keyring_location("connection.google", "oauth_token"),
            keyring_location("connection.google", "oauth_token")
        );
        // Distinct source/key splits must never merge.
        assert_ne!(
            keyring_location("connection.google", "oauth_token"),
            keyring_location("connection", "google.oauth_token")
        );
    }

    #[test]
    fn production_store_constructs_without_probe_or_fallback() {
        // Construction must be infallible on every supported platform and
        // must not touch the keystore. The type carries no alternate
        // storage, so there is nothing to silently fall back to.
        let _store = OsKeyringSecretsStore::new();
    }

    /// Real OS round trip: write, re-open through a fresh store value,
    /// read back, delete, then confirm the credential is gone.
    ///
    /// Ignored by default because CI runners have no unlocked platform
    /// keystore. Run locally with:
    /// `cargo test -p bicameral-config -- --ignored os_keyring_round_trip`
    #[test]
    #[ignore = "requires an unlocked OS keystore; run locally for terminal evidence"]
    fn os_keyring_round_trip_persists_across_store_instances() {
        let source_id = "connection.bicameral-test";
        let key = "os-keyring-round-trip";
        let value = "test-secret-value-not-a-real-credential";

        let writer = OsKeyringSecretsStore::new();
        writer.write_secret(source_id, key, value).unwrap();

        // A fresh store value simulates daemon restart at this layer.
        let reader = OsKeyringSecretsStore::new();
        assert_eq!(reader.read_secret(source_id, key).as_deref(), Some(value));

        reader.delete_secret(source_id, key).unwrap();
        assert_eq!(reader.read_secret(source_id, key), None);
        // Deleting a missing credential stays Ok.
        reader.delete_secret(source_id, key).unwrap();
    }
}
