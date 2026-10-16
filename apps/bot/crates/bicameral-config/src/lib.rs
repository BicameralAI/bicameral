//! Configuration loading and workspace discovery for the Bicameral runtime.
//!
//! Configuration is rooted in `.bicameral/` within a workspace or a user-level
//! config path (`~/.config/bicameral/` on Linux/macOS). This crate provides
//! discovery, loading, validation, and the typed config schema.
//!
//! Derived from ZeroClaw's config layer, narrowed to Bicameral's governance domain.
//! See NOTICE and runtime/UPSTREAM-ZEROCLAW.md for attribution.

pub mod connections;
pub mod google_client_credential;
/// Legacy/internal module. See `docs/specs/project-setup-ui.md` for the current
/// provider-based project discovery direction.
pub mod invite;
pub mod os_keyring;
pub mod paths;
pub mod provider_app_config;
pub mod schema;
pub mod workspace;

pub use connections::{
    parse_stored_oauth_token, AuthMethod, ConnectionMetadata, ConnectionRegistry, ConnectionStatus,
    GoogleOAuthConfig, InMemorySecretsStore, OAuthTokenEnvelopeV1, SecretsStore, StoredOAuthToken,
    OAUTH_TOKEN_ENVELOPE_VERSION,
};
pub use google_client_credential::{
    GoogleClientCredentialEnvelopeError, GoogleClientCredentialEnvelopeV1,
    GOOGLE_CLIENT_CREDENTIAL_ENVELOPE_VERSION, GOOGLE_CLIENT_CREDENTIAL_KEY,
};
#[allow(deprecated)]
pub use invite::{ConfigPreview, InviteBundle, InviteBundleError};
pub use os_keyring::OsKeyringSecretsStore;
pub use paths::BicameralPaths;
pub use provider_app_config::{
    GoogleNativeAppConfigError, GoogleNativeAppConfigV1, GOOGLE_NATIVE_APP_CONFIG_SCHEMA_VERSION,
    GOOGLE_NATIVE_CLIENT_TYPE, GOOGLE_PROVIDER_ID,
};
pub use schema::{
    is_safe_gateway_host, BicameralConfig, EventStoreConfig, GatewayConfig, GovernancePolicyConfig,
    ProductJoinTrustConfig, ProductJoinVerifyingKeyConfig, DEFAULT_GATEWAY_PORT,
};
pub use workspace::WorkspaceDiscovery;
