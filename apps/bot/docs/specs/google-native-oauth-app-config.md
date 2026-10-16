# Google Native OAuth Application Configuration Contract

Status: accepted implementation contract for BicameralAI/bicameral-bot#1113 and #1114.

Parent connection contract: `docs/specs/oauth-and-integration-connections.md`.

## Decision

Bicameral uses a Cloud-owned Google OAuth **application identity/configuration** for the installed/local daemon path while preserving daemon-local Google user authorization and provider Connection authority.

This contract refines the phrase "credential authority moves to Cloud" into two distinct credential boundaries:

1. **OAuth application identity/configuration** is Cloud-owned.
2. **OAuth user/provider credentials** are daemon-owned.

Cloud ownership of the application identity does not authorize Cloud to mint, receive, persist, refresh, or interpret a user's Google access or refresh token and does not make Cloud a provider Connection authority.

## Authority split

### Cloud is authoritative for

- Bicameral's Google OAuth application registration and lifecycle;
- consent-screen, branding, verification, and environment promotion;
- the active public/native Google `client_id` for each Bicameral environment;
- the approved Google OAuth scope set and its version;
- app-config version, expiry, rotation, and disablement;
- publication of the bounded non-secret app-config payload.

### Local daemon is authoritative for

- PKCE verifier/challenge generation;
- OAuth `state` generation and CSRF validation;
- loopback redirect listener and callback validation;
- Google authorization-code exchange;
- Google access/refresh token receipt;
- approved local keyring/secret-envelope storage;
- token refresh/revocation handling;
- `ConnectionRegistry` and `ConnectionCompletionOutcome` truth.

### Cloud must not become

- storage for Google user access or refresh tokens;
- recipient of Google authorization codes or PKCE verifiers in the default architecture;
- an authorization-code/token exchange broker;
- a second `ConnectionRegistry`;
- provider readiness authority;
- a proxy required for normal Drive API calls;
- Product, SourceBinding, signoff, or Decision Ledger authority.

## Google client model

The installed/local daemon path uses the Google native/desktop public-client model. A confidential static Google client secret is not a security boundary for installed applications and is therefore not required by this contract.

The daemon must not require `BICAMERAL_GOOGLE_CLIENT_SECRET` for the governed native-client path. A development/operator override may exist only when explicitly allowed by the consuming implementation and must never silently outrank valid production Cloud app configuration.

## V1 payload

Canonical Rust type: `bicameral_config::GoogleNativeAppConfigV1`.

Machine-readable schema: `docs/specs/google-native-oauth-app-config.schema.json`.

Example:

```json
{
  "schema_version": 1,
  "provider": "google",
  "client_type": "native",
  "client_id": "123456.apps.googleusercontent.com",
  "environment": "alpha",
  "config_version": "google-alpha-7",
  "scope_set_version": "drive-alpha-v1",
  "scopes": [
    "https://www.googleapis.com/auth/drive.file"
  ],
  "issued_at": "2026-08-12T20:00:00Z",
  "expires_at": "2026-08-13T20:00:00Z",
  "enabled": true
}
```

The V1 payload is intentionally closed. Unknown fields are invalid. In particular, `client_secret`, `authorization_code`, `access_token`, `refresh_token`, `pkce_verifier`, Cloud bearer tokens, and daemon Connection state are forbidden by construction.

## Validation

A daemon must reject app configuration when any of the following is true:

- schema version is unsupported;
- provider is not exactly `google`;
- client type is not exactly `native`;
- `client_id` is empty;
- payload environment does not exactly match the daemon's expected Bicameral environment;
- config version or scope-set version is empty;
- scope list is empty, contains blank entries, or contains duplicate entries;
- `expires_at <= issued_at`;
- config is disabled;
- config is expired;
- payload contains an unknown field.

Rejected app configuration is a provider-connection bootstrap failure. It is never interpreted as `connected` and never mutates `ConnectionRegistry`.

## Bootstrap transport

The app-config payload is non-secret. V1 does not embed a payload signature or provider credential. Authenticity and integrity are enforced by the production bootstrap transport and Cloud deployment boundary.

The production implementation should expose the configuration from the canonical Bicameral Cloud origin over HTTPS. The exact Cloud route belongs to the Cloud implementation contract, but it must:

- return only the V1 payload;
- bind the response to the requested/current Bicameral environment;
- support rotation and disablement without a Bot binary release;
- avoid a circular dependency on Google Drive authorization itself;
- never accept or return Google user credential material.

A previously validated config may be cached locally only through its `expires_at`. Cache persistence is app configuration, not provider credential storage and not Connection truth. Once expired or disabled, the daemon must re-bootstrap and fail closed if it cannot obtain a valid replacement.

## OAuth execution

After obtaining and validating V1 config, the daemon:

1. creates a fresh PKCE verifier/challenge and OAuth `state`;
2. constructs the Google authorization URL using the Cloud-published `client_id`, approved scopes, and daemon-owned loopback redirect URI;
3. validates callback state locally;
4. exchanges the authorization code with Google locally;
5. writes Google token material only to the approved daemon-local secret store/keyring envelope;
6. updates `ConnectionRegistry` only after durable secret storage and existing terminal provider-auth checks succeed.

The Cloud app-config payload never becomes proof of provider authorization.

## Failure and recovery semantics

The implementation must keep these outcomes distinct and non-connected:

- app config unavailable;
- app config malformed;
- app config disabled;
- app config expired;
- wrong environment/client type;
- app config transport/authenticity failure;
- OAuth denied/cancelled;
- callback/state mismatch;
- PKCE/code-exchange failure;
- provider unavailable;
- token refresh/revocation failure.

Temporary Cloud unavailability may use a still-valid cached config. Cloud unavailability after cache expiry blocks a new Google authorization attempt but does not erase an already valid daemon-local Google token envelope or rewrite Connection truth. Existing token refresh semantics remain governed by the provider connection implementation.

## Security invariants

- Provider authentication is not Product/source approval.
- App configuration is not user credential storage.
- Connection metadata is not secret storage.
- Cloud app-config authority cannot author `connected`.
- Browser state cannot author `connected`.
- No Google user credential crosses into Cloud in the default architecture.
- No confidential Google client secret is embedded in the Bot binary for the governed native-client path.
- Unknown app-config fields fail closed.

## Evidence boundary

Component/contract tests for this contract are supporting evidence only. Terminal acceptance requires a real-process/provider topology proving that a fresh operator daemon with no manually injected `BICAMERAL_GOOGLE_CLIENT_ID` or `BICAMERAL_GOOGLE_CLIENT_SECRET` can obtain valid Cloud app config, complete real Google Drive OAuth, persist token material locally, and reach daemon-authored `ConnectionRegistry: connected`.

Terminal evidence issue: `BicameralAI/bicameral-cloud#263`.
