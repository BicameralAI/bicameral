//! Connection metadata registry for user-level provider connections.
//!
//! Stores non-secret connection metadata at the operator home level
//! (`~/.bicameral/connections.yaml`). Secret material (tokens, keys)
//! remains in `secrets_store`; this registry only tracks provider
//! identity, status, capabilities, and update timestamps.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Connection status for a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    ActionNeeded,
    NotConnected,
}

/// Authentication method used for this connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Oauth,
    DeviceFlow,
    ImportedToken,
    Pat,
}

/// Non-secret metadata for a single provider connection.
///
/// Secret material (OAuth tokens, refresh tokens, PATs) is never stored
/// here — it belongs in `secrets_store`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    pub status: ConnectionStatus,

    /// Human-readable account label, e.g. "jinhongkuan" or "user@example.com".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,

    /// Capability labels granted by this connection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,

    /// Last time this connection metadata was updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,

    /// How the connection was authenticated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_method: Option<AuthMethod>,
}

/// Top-level connection registry file shape.
///
/// Keyed by provider id (e.g. "github", "google").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionRegistry {
    #[serde(default)]
    pub connections: BTreeMap<String, ConnectionMetadata>,
}

impl ConnectionRegistry {
    /// Load the registry from a YAML file. Returns an empty registry if the
    /// file does not exist.
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let registry: Self = serde_yaml::from_str(&content)?;
        Ok(registry)
    }

    /// Persist the registry to a YAML file, creating parent directories as
    /// needed.
    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Look up a provider's connection metadata.
    pub fn get(&self, provider_id: &str) -> Option<&ConnectionMetadata> {
        self.connections.get(provider_id)
    }

    /// Determine the effective status for a provider. Returns
    /// `NotConnected` when the provider is absent from the registry.
    pub fn status(&self, provider_id: &str) -> ConnectionStatus {
        self.connections
            .get(provider_id)
            .map(|m| m.status.clone())
            .unwrap_or(ConnectionStatus::NotConnected)
    }

    /// Insert or replace connection metadata for a provider.
    pub fn upsert(&mut self, provider_id: impl Into<String>, meta: ConnectionMetadata) {
        self.connections.insert(provider_id.into(), meta);
    }
}

/// Legacy secret-key aliases preserved for backward compatibility.
///
/// Existing source-specific secrets remain readable through these
/// legacy keys. New code should use `connection.<provider>` keys.
pub mod legacy_aliases {
    /// Legacy Google Drive secret key.
    pub const GOOGLE_DRIVE_SOURCE_AUTH: &str = "google_drive";
    /// Legacy GitHub secret key.
    pub const GITHUB_API_KEY: &str = "github";
}

/// Canonical secret-key identifiers for provider connections.
pub mod connection_keys {
    /// Google connection secret source_id.
    pub const GOOGLE_SOURCE_ID: &str = "connection.google";
    /// Standard secret key for OAuth tokens.
    pub const OAUTH_TOKEN_KEY: &str = "oauth_token";
}

// ── URL encoding helper ───────────────────────────────────────────

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[(b >> 4) as usize]));
                out.push(char::from(HEX[(b & 0x0f) as usize]));
            }
        }
    }
    out
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

// ── Google OAuth configuration ────────────────────────────────────

/// Configuration for Google OAuth local-loopback flow.
///
/// Read from environment variables or config. When `client_id` is absent
/// the provider start endpoint reports `ActionNeeded` with a config hint.
#[derive(Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for GoogleOAuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleOAuthConfig")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &if self.client_secret.is_empty() {
                    "[none]"
                } else {
                    "[redacted]"
                },
            )
            .field("redirect_uri", &self.redirect_uri)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl GoogleOAuthConfig {
    /// Build a Google OAuth authorization URL for the local-loopback flow.
    ///
    /// `access_type=offline` plus `prompt=consent` is the provider
    /// contract for refreshable credentials: Google only issues a refresh
    /// token for offline access, and on re-authorization only when consent
    /// is re-prompted. Without both, a reconnect can silently downgrade to
    /// a non-refreshable access token.
    pub fn authorization_url(&self, state: &str) -> String {
        self.authorization_url_with_pkce(state, None)
    }

    /// Build the Google authorization URL with an optional RFC 7636 S256
    /// challenge. Production native-client flows must provide a challenge;
    /// the no-PKCE wrapper above remains only for compatibility with callers
    /// that have not yet migrated.
    pub fn authorization_url_with_pkce(&self, state: &str, code_challenge: Option<&str>) -> String {
        let scopes = self.scopes.join(" ");
        let pkce = code_challenge
            .map(|challenge| {
                format!(
                    "&code_challenge={}&code_challenge_method=S256",
                    percent_encode(challenge)
                )
            })
            .unwrap_or_default();
        format!(
            "https://accounts.google.com/o/oauth2/v2/auth\
             ?client_id={client_id}\
             &redirect_uri={redirect_uri}\
             &response_type=code\
             &scope={scopes}\
             &access_type=offline\
             &prompt=consent\
             &state={state}{pkce}",
            client_id = percent_encode(&self.client_id),
            redirect_uri = percent_encode(&self.redirect_uri),
            scopes = percent_encode(&scopes),
            state = percent_encode(state),
            pkce = pkce,
        )
    }

    /// Load from environment variables. Returns `None` if client_id is
    /// not set.
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("BICAMERAL_GOOGLE_CLIENT_ID").ok()?;
        let client_secret = std::env::var("BICAMERAL_GOOGLE_CLIENT_SECRET").unwrap_or_default();
        let redirect_uri = std::env::var("BICAMERAL_GOOGLE_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3000/api/v1/connections/google/callback".into());
        let scopes = std::env::var("BICAMERAL_GOOGLE_SCOPES")
            .unwrap_or_else(|_| "https://www.googleapis.com/auth/drive.file".into())
            .split(' ')
            .map(String::from)
            .collect();
        Some(Self {
            client_id,
            client_secret,
            redirect_uri,
            scopes,
        })
    }
}

// ── Secrets store trait ───────────────────────────────────────────

/// Trait for persisting secret material (tokens, keys).
///
/// Production implementations use OS keyring; tests use an in-memory
/// or file-based store. Secret values never appear in logs, HTTP
/// responses, or `ConnectionRegistry`.
pub trait SecretsStore: Send + Sync {
    /// Read a secret by source_id and key. Returns `None` if not found.
    fn read_secret(&self, source_id: &str, key: &str) -> Option<String>;
    /// Write a secret by source_id and key.
    fn write_secret(&self, source_id: &str, key: &str, value: &str) -> anyhow::Result<()>;
    /// Delete a secret by source_id and key.
    fn delete_secret(&self, source_id: &str, key: &str) -> anyhow::Result<()>;
}

/// In-memory secrets store for testing.
///
/// Thread-safe via `Mutex`. Never persists to disk.
#[derive(Default)]
pub struct InMemorySecretsStore {
    secrets: std::sync::Mutex<std::collections::HashMap<(String, String), String>>,
}

impl InMemorySecretsStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretsStore for InMemorySecretsStore {
    fn read_secret(&self, source_id: &str, key: &str) -> Option<String> {
        self.secrets
            .lock()
            .ok()?
            .get(&(source_id.to_string(), key.to_string()))
            .cloned()
    }

    fn write_secret(&self, source_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
        self.secrets
            .lock()
            .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?
            .insert((source_id.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    fn delete_secret(&self, source_id: &str, key: &str) -> anyhow::Result<()> {
        self.secrets
            .lock()
            .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?
            .remove(&(source_id.to_string(), key.to_string()));
        Ok(())
    }
}

// ── OAuth token envelope ──────────────────────────────────────────

/// Version tag for [`OAuthTokenEnvelopeV1`].
pub const OAUTH_TOKEN_ENVELOPE_VERSION: u32 = 1;

/// Versioned secret envelope for refreshable OAuth token material.
///
/// Stored as a JSON string in the [`SecretsStore`] value under the
/// connection-scoped key (e.g. `connection.google` / `oauth_token`).
/// Preserves the refresh token and expiry metadata the provider issued
/// so the daemon can decide when a refresh is required. Never serialized
/// into HTTP responses, `ConnectionRegistry` metadata, or logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokenEnvelopeV1 {
    /// Always [`OAUTH_TOKEN_ENVELOPE_VERSION`] for this shape.
    pub envelope_version: u32,
    pub access_token: String,
    /// Present when the provider issued a refresh token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Instant the current access token expires, derived from the
    /// provider `expires_in` at exchange time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Non-secret OAuth client identity that minted this envelope
    /// (public native-app metadata, never a secret). Lets the daemon
    /// refresh after restart without re-resolving application config;
    /// additive/optional so pre-existing envelopes keep parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// When the token material was obtained from the provider.
    pub obtained_at: DateTime<Utc>,
}

impl std::fmt::Debug for OAuthTokenEnvelopeV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokenEnvelopeV1")
            .field("envelope_version", &self.envelope_version)
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("token_type", &self.token_type)
            .field("scope", &self.scope)
            .field("client_id", &self.client_id)
            .field("obtained_at", &self.obtained_at)
            .finish()
    }
}

impl OAuthTokenEnvelopeV1 {
    /// Build a v1 envelope from provider token-exchange output.
    ///
    /// `expires_in_seconds` is the provider-relative lifetime; it is
    /// resolved to an absolute `expires_at` against `now` so restart and
    /// refresh decisions do not depend on process-local clocks.
    pub fn from_token_exchange(
        access_token: String,
        refresh_token: Option<String>,
        expires_in_seconds: Option<u64>,
        token_type: Option<String>,
        scope: Option<String>,
        now: DateTime<Utc>,
    ) -> Self {
        let expires_at = expires_in_seconds
            .and_then(|s| chrono::Duration::try_seconds(s.min(i64::MAX as u64) as i64))
            .map(|d| now + d);
        Self {
            envelope_version: OAUTH_TOKEN_ENVELOPE_VERSION,
            access_token,
            refresh_token: refresh_token.filter(|t| !t.is_empty()),
            expires_at,
            token_type,
            scope,
            client_id: None,
            obtained_at: now,
        }
    }

    /// Record the non-secret OAuth client identity that minted this
    /// envelope so daemon-local refresh works after restart without
    /// re-resolving application config. Empty values are dropped.
    pub fn with_client_id(mut self, client_id: Option<String>) -> Self {
        self.client_id = client_id.filter(|value| !value.trim().is_empty());
        self
    }

    /// Serialize to the JSON string stored in the secrets store.
    pub fn to_secret_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self)
            .map_err(|_| anyhow::anyhow!("token envelope serialization failed"))
    }

    /// A bundle is usable when it carries a non-empty access token or a
    /// refresh token that can mint one.
    pub fn is_usable(&self) -> bool {
        !self.access_token.is_empty()
            || self.refresh_token.as_deref().is_some_and(|t| !t.is_empty())
    }

    /// Whether the access token has expired relative to `now`. Envelopes
    /// without expiry metadata are treated as unexpired.
    pub fn access_token_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|at| now >= at)
    }
}

/// Parsed value of a stored `oauth_token` secret.
///
/// Existing deployments stored the plain access token string; newer
/// writes use [`OAuthTokenEnvelopeV1`]. Both must parse safely.
#[derive(Clone, PartialEq, Eq)]
pub enum StoredOAuthToken {
    Envelope(OAuthTokenEnvelopeV1),
    LegacyPlain { access_token: String },
}

impl std::fmt::Debug for StoredOAuthToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Envelope(env) => f.debug_tuple("Envelope").field(env).finish(),
            Self::LegacyPlain { .. } => f
                .debug_struct("LegacyPlain")
                .field("access_token", &"[redacted]")
                .finish(),
        }
    }
}

impl StoredOAuthToken {
    /// Current access token, if a non-empty one is present.
    pub fn access_token(&self) -> Option<&str> {
        let token = match self {
            Self::Envelope(env) => env.access_token.as_str(),
            Self::LegacyPlain { access_token } => access_token.as_str(),
        };
        (!token.is_empty()).then_some(token)
    }

    /// Refresh token, if one was preserved (never present for legacy values).
    pub fn refresh_token(&self) -> Option<&str> {
        match self {
            Self::Envelope(env) => env.refresh_token.as_deref().filter(|t| !t.is_empty()),
            Self::LegacyPlain { .. } => None,
        }
    }

    /// Usable credential evidence: an access token now, or a refresh
    /// token that can obtain one.
    pub fn is_usable(&self) -> bool {
        self.access_token().is_some() || self.refresh_token().is_some()
    }
}

/// Parse a stored `oauth_token` secret value without ever panicking.
///
/// Accepts, in order: a v1 envelope; any JSON object exposing a non-empty
/// `access_token` string (forward/foreign shapes degrade to legacy
/// semantics); a bare legacy access-token string. Returns `None` for
/// empty or unusable values.
pub fn parse_stored_oauth_token(raw: &str) -> Option<StoredOAuthToken> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') {
        if let Ok(envelope) = serde_json::from_str::<OAuthTokenEnvelopeV1>(trimmed) {
            if envelope.envelope_version == OAUTH_TOKEN_ENVELOPE_VERSION && envelope.is_usable() {
                return Some(StoredOAuthToken::Envelope(envelope));
            }
        }
        return serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .as_ref()
            .and_then(|v| v.get("access_token"))
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .map(|t| StoredOAuthToken::LegacyPlain {
                access_token: t.to_string(),
            });
    }
    Some(StoredOAuthToken::LegacyPlain {
        access_token: trimmed.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_connected() -> ConnectionMetadata {
        ConnectionMetadata {
            status: ConnectionStatus::Connected,
            account_label: Some("testuser".into()),
            capabilities: vec!["github.repo.read".into(), "github.repo.write".into()],
            updated_at: Some(Utc::now()),
            auth_method: Some(AuthMethod::Oauth),
        }
    }

    fn sample_action_needed() -> ConnectionMetadata {
        ConnectionMetadata {
            status: ConnectionStatus::ActionNeeded,
            account_label: Some("testuser@example.com".into()),
            capabilities: vec!["drive.file".into()],
            updated_at: Some(Utc::now()),
            auth_method: Some(AuthMethod::Oauth),
        }
    }

    fn sample_not_connected() -> ConnectionMetadata {
        ConnectionMetadata {
            status: ConnectionStatus::NotConnected,
            account_label: None,
            capabilities: Vec::new(),
            updated_at: None,
            auth_method: None,
        }
    }

    #[test]
    fn connected_status_round_trip() {
        let meta = sample_connected();
        assert_eq!(meta.status, ConnectionStatus::Connected);
        assert!(meta.account_label.is_some());
        assert!(!meta.capabilities.is_empty());
    }

    #[test]
    fn action_needed_status_round_trip() {
        let meta = sample_action_needed();
        assert_eq!(meta.status, ConnectionStatus::ActionNeeded);
        assert!(meta.account_label.is_some());
    }

    #[test]
    fn not_connected_status_round_trip() {
        let meta = sample_not_connected();
        assert_eq!(meta.status, ConnectionStatus::NotConnected);
        assert!(meta.account_label.is_none());
        assert!(meta.capabilities.is_empty());
        assert!(meta.auth_method.is_none());
    }

    #[test]
    fn registry_missing_provider_returns_not_connected() {
        let registry = ConnectionRegistry::default();
        assert_eq!(registry.status("github"), ConnectionStatus::NotConnected);
    }

    #[test]
    fn registry_upsert_and_lookup() {
        let mut registry = ConnectionRegistry::default();
        registry.upsert("github", sample_connected());
        assert_eq!(registry.status("github"), ConnectionStatus::Connected);
        assert_eq!(
            registry.get("github").unwrap().account_label.as_deref(),
            Some("testuser")
        );
    }

    #[test]
    fn yaml_round_trip_all_states() {
        let mut registry = ConnectionRegistry::default();
        registry.upsert("github", sample_connected());
        registry.upsert("google", sample_action_needed());
        registry.upsert("slack", sample_not_connected());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.yaml");

        registry.save_to_file(&path).unwrap();
        let loaded = ConnectionRegistry::load_from_file(&path).unwrap();

        assert_eq!(loaded.status("github"), ConnectionStatus::Connected);
        assert_eq!(loaded.status("google"), ConnectionStatus::ActionNeeded);
        assert_eq!(loaded.status("slack"), ConnectionStatus::NotConnected);

        assert_eq!(
            loaded.get("github").unwrap().capabilities,
            vec!["github.repo.read", "github.repo.write"]
        );
        assert!(loaded.get("github").unwrap().auth_method == Some(AuthMethod::Oauth));
    }

    #[test]
    fn load_nonexistent_file_returns_empty_registry() {
        let path = std::path::PathBuf::from("/tmp/nonexistent-connections.yaml");
        let registry = ConnectionRegistry::load_from_file(&path).unwrap();
        assert!(registry.connections.is_empty());
    }

    #[test]
    fn yaml_serialization_matches_spec_shape() {
        let mut registry = ConnectionRegistry::default();
        registry.upsert(
            "github",
            ConnectionMetadata {
                status: ConnectionStatus::Connected,
                account_label: Some("jinhongkuan".into()),
                capabilities: vec!["github.repo.read".into(), "github.repo.write".into()],
                updated_at: None,
                auth_method: None,
            },
        );

        let yaml = serde_yaml::to_string(&registry).unwrap();
        assert!(yaml.contains("github"));
        assert!(yaml.contains("connected"));
        assert!(yaml.contains("jinhongkuan"));
        assert!(yaml.contains("github.repo.read"));
        // Must not contain secret material
        assert!(!yaml.contains("oauth_token"));
        assert!(!yaml.contains("refresh_token"));
        assert!(!yaml.contains("api_key"));
    }

    // ── OAuth token envelope tests ────────────────────────────────

    fn sample_envelope() -> OAuthTokenEnvelopeV1 {
        OAuthTokenEnvelopeV1::from_token_exchange(
            "test-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(3599),
            Some("Bearer".into()),
            Some("https://www.googleapis.com/auth/drive.file".into()),
            Utc::now(),
        )
    }

    #[test]
    fn envelope_round_trips_through_secret_string() {
        let envelope = sample_envelope();
        let raw = envelope.to_secret_string().unwrap();
        let parsed = parse_stored_oauth_token(&raw).unwrap();
        assert_eq!(parsed, StoredOAuthToken::Envelope(envelope.clone()));
        assert_eq!(parsed.access_token(), Some("test-access-token-value"));
        assert_eq!(parsed.refresh_token(), Some("test-refresh-token-value"));
        assert!(parsed.is_usable());
    }

    /// The optional non-secret client identity is additive: envelopes
    /// that record it round-trip, empty values are dropped, and
    /// pre-existing envelopes without the field keep parsing.
    #[test]
    fn envelope_client_id_is_additive_and_optional() {
        let envelope = sample_envelope().with_client_id(Some("native-client-id".into()));
        assert_eq!(envelope.client_id.as_deref(), Some("native-client-id"));
        let raw = envelope.to_secret_string().unwrap();
        let StoredOAuthToken::Envelope(parsed) = parse_stored_oauth_token(&raw).unwrap() else {
            panic!("expected envelope");
        };
        assert_eq!(parsed.client_id.as_deref(), Some("native-client-id"));

        // Blank identities are dropped rather than stored.
        let blank = sample_envelope().with_client_id(Some("   ".into()));
        assert_eq!(blank.client_id, None);

        // A pre-existing serialized envelope (no client_id field) parses.
        let legacy = sample_envelope();
        assert_eq!(legacy.client_id, None);
        let raw = legacy.to_secret_string().unwrap();
        assert!(!raw.contains("client_id"));
        let parsed = parse_stored_oauth_token(&raw).unwrap();
        assert!(parsed.is_usable());
    }

    #[test]
    fn envelope_preserves_refresh_and_expiry_metadata() {
        let now = Utc::now();
        let envelope = OAuthTokenEnvelopeV1::from_token_exchange(
            "test-access-token-value".into(),
            Some("test-refresh-token-value".into()),
            Some(60),
            None,
            None,
            now,
        );
        assert_eq!(envelope.envelope_version, OAUTH_TOKEN_ENVELOPE_VERSION);
        assert_eq!(
            envelope.expires_at,
            Some(now + chrono::Duration::try_seconds(60).unwrap())
        );
        assert!(!envelope.access_token_expired(now));
        assert!(envelope.access_token_expired(now + chrono::Duration::try_seconds(61).unwrap()));
        assert!(envelope.is_usable());
    }

    #[test]
    fn legacy_plain_access_token_parses_safely() {
        let parsed = parse_stored_oauth_token("legacy-plain-access-token").unwrap();
        assert_eq!(parsed.access_token(), Some("legacy-plain-access-token"));
        assert_eq!(parsed.refresh_token(), None);
        assert!(parsed.is_usable());
    }

    #[test]
    fn foreign_json_with_access_token_degrades_to_legacy() {
        let parsed =
            parse_stored_oauth_token(r#"{"access_token":"tok-from-foreign-shape","extra":1}"#)
                .unwrap();
        assert_eq!(parsed.access_token(), Some("tok-from-foreign-shape"));
        assert_eq!(parsed.refresh_token(), None);
    }

    #[test]
    fn unusable_stored_values_parse_to_none() {
        assert!(parse_stored_oauth_token("").is_none());
        assert!(parse_stored_oauth_token("   ").is_none());
        assert!(parse_stored_oauth_token(r#"{"access_token":""}"#).is_none());
        assert!(parse_stored_oauth_token(r#"{"unrelated":"json"}"#).is_none());
        // Envelope-shaped but empty token material is unusable.
        let empty = OAuthTokenEnvelopeV1::from_token_exchange(
            String::new(),
            None,
            None,
            None,
            None,
            Utc::now(),
        );
        assert!(!empty.is_usable());
        assert!(parse_stored_oauth_token(&empty.to_secret_string().unwrap()).is_none());
    }

    #[test]
    fn envelope_debug_never_leaks_token_material() {
        let envelope = sample_envelope();
        let debug = format!("{envelope:?}");
        assert!(!debug.contains("test-access-token-value"));
        assert!(!debug.contains("test-refresh-token-value"));
        assert!(debug.contains("[redacted]"));

        let stored = StoredOAuthToken::LegacyPlain {
            access_token: "legacy-plain-access-token".into(),
        };
        let debug = format!("{stored:?}");
        assert!(!debug.contains("legacy-plain-access-token"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn authorization_url_requests_refreshable_credential_semantics() {
        let config = GoogleOAuthConfig {
            client_id: "client-id".into(),
            client_secret: String::new(),
            redirect_uri: "http://localhost:3000/cb".into(),
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
        };
        let url = config.authorization_url("state-token");
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
    }

    #[test]
    fn no_secret_fields_in_metadata() {
        let yaml = serde_yaml::to_string(&sample_connected()).unwrap();
        for secret_key in &[
            "oauth_token",
            "refresh_token",
            "api_key",
            "client_secret",
            "pat_value",
        ] {
            assert!(
                !yaml.contains(secret_key),
                "ConnectionMetadata must not contain secret field: {secret_key}"
            );
        }
    }
}
