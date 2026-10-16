"""Cloud configuration environment variables (ADR-0006).

Supabase Auth is optional: when SUPABASE_URL and either SUPABASE_JWT_SECRET
or SUPABASE_ANON_KEY are set, the API validates real Supabase access tokens.
Otherwise the in-memory auth backend is used for tests and local development.
"""

from __future__ import annotations

import os
from dataclasses import dataclass

_DEFAULT_GOOGLE_DRIVE_SCOPES = ("https://www.googleapis.com/auth/drive.file",)


@dataclass(frozen=True)
class CloudSettings:
    """Immutable snapshot of cloud environment configuration."""

    supabase_url: str | None
    supabase_jwt_secret: str | None
    supabase_anon_key: str | None
    supabase_service_key: str | None
    product_join_ticket_issuer: str | None = None
    product_join_ticket_ttl_seconds: int = 300
    product_join_signing_key_id: str | None = None
    product_join_signing_private_key_b64: str | None = None
    product_join_verification_keys_json: str | None = None
    # Daemon ProductCreationCommitProof trust set (cloud#227 / v0.1.17 T-C).
    # JSON list of {key_id, public_key_b64, state: active|retiring}.
    product_creation_proof_verification_keys_json: str | None = None
    # Cloud-owned, non-secret Google OAuth native/desktop application identity.
    # The daemon fetches this application configuration but keeps PKCE, auth
    # codes, access/refresh tokens, and Connection truth local (bot#1114).
    google_oauth_native_client_id: str | None = None
    google_oauth_environment: str | None = None
    google_oauth_config_version: str | None = None
    # Provider-required Google Desktop credential. This is secret request
    # material, never part of the public provider-app-config response. In
    # production it must arrive through the Cloud secret-management boundary.
    google_oauth_native_client_secret: str | None = None
    google_oauth_scope_set_version: str = "drive-file-v1"
    google_oauth_scopes: tuple[str, ...] = _DEFAULT_GOOGLE_DRIVE_SCOPES
    google_oauth_app_config_ttl_seconds: int = 3600
    google_oauth_app_config_enabled: bool = True
    # Interactive API docs and the OpenAPI schema are OFF by default so the
    # hosted surface is not publicly enumerable; enable only in non-production
    # (BICAMERAL_CLOUD_ENABLE_DOCS=1) per the pentest OpenAPI-exposure finding.
    docs_enabled: bool = False
    # Per-instance application-layer rate limit (defense-in-depth beneath the
    # authoritative edge limit). A non-positive request count disables it.
    rate_limit_requests: int = 300
    rate_limit_window_seconds: int = 60

    @property
    def supabase_auth_enabled(self) -> bool:
        return bool(
            self.supabase_url and (self.supabase_jwt_secret or self.supabase_anon_key)
        )

    @property
    def supabase_browser_auth_configured(self) -> bool:
        return bool(self.supabase_url and self.supabase_anon_key)

    @property
    def supabase_design_partner_allowlist_enabled(self) -> bool:
        return bool(self.supabase_url and self.supabase_service_key)

    @property
    def supabase_product_directory_enabled(self) -> bool:
        """Whether the durable Supabase Product directory is the production backend.

        Requires the service-role key (server-side only). When false, the
        in-memory directory is used for tests and local development (cloud#125).
        """
        return bool(self.supabase_url and self.supabase_service_key)


def _int_env(name: str, default: int) -> int:
    raw = os.environ.get(name)
    if raw is None:
        return default
    try:
        return int(raw)
    except ValueError:
        return 0


def _bool_env(name: str, default: bool) -> bool:
    raw = os.environ.get(name)
    if raw is None:
        return default
    return raw.strip().lower() in ("1", "true", "yes", "on")


def _csv_env(name: str, default: tuple[str, ...]) -> tuple[str, ...]:
    raw = os.environ.get(name)
    if raw is None:
        return default
    return tuple(part.strip() for part in raw.split(",") if part.strip())


def load_settings() -> CloudSettings:
    """Load settings from environment variables."""

    return CloudSettings(
        supabase_url=os.environ.get("SUPABASE_URL"),
        supabase_jwt_secret=os.environ.get("SUPABASE_JWT_SECRET"),
        supabase_anon_key=os.environ.get("SUPABASE_ANON_KEY"),
        supabase_service_key=os.environ.get("SUPABASE_SERVICE_KEY"),
        product_join_ticket_issuer=os.environ.get(
            "PRODUCT_JOIN_TICKET_ISSUER"
        ),
        product_join_ticket_ttl_seconds=_int_env(
            "PRODUCT_JOIN_TICKET_TTL_SECONDS", 300
        ),
        product_join_signing_key_id=os.environ.get(
            "PRODUCT_JOIN_SIGNING_KEY_ID"
        ),
        product_join_signing_private_key_b64=os.environ.get(
            "PRODUCT_JOIN_SIGNING_PRIVATE_KEY_B64"
        ),
        product_join_verification_keys_json=os.environ.get(
            "PRODUCT_JOIN_VERIFICATION_KEYS_JSON"
        ),
        product_creation_proof_verification_keys_json=os.environ.get(
            "PRODUCT_CREATION_PROOF_VERIFICATION_KEYS_JSON"
        ),
        google_oauth_native_client_id=os.environ.get(
            "BICAMERAL_GOOGLE_OAUTH_CLIENT_ID"
        ),
        google_oauth_environment=os.environ.get(
            "BICAMERAL_GOOGLE_OAUTH_ENVIRONMENT"
        ),
        google_oauth_config_version=os.environ.get(
            "BICAMERAL_GOOGLE_OAUTH_CONFIG_VERSION"
        ),
        google_oauth_native_client_secret=os.environ.get(
            "BICAMERAL_GOOGLE_OAUTH_CLIENT_SECRET"
        ),
        google_oauth_scope_set_version=os.environ.get(
            "BICAMERAL_GOOGLE_OAUTH_SCOPE_SET_VERSION", "drive-file-v1"
        ),
        google_oauth_scopes=_csv_env(
            "BICAMERAL_GOOGLE_OAUTH_SCOPES", _DEFAULT_GOOGLE_DRIVE_SCOPES
        ),
        google_oauth_app_config_ttl_seconds=_int_env(
            "BICAMERAL_GOOGLE_OAUTH_APP_CONFIG_TTL_SECONDS", 3600
        ),
        google_oauth_app_config_enabled=_bool_env(
            "BICAMERAL_GOOGLE_OAUTH_APP_CONFIG_ENABLED", True
        ),
        docs_enabled=_bool_env("BICAMERAL_CLOUD_ENABLE_DOCS", False),
        rate_limit_requests=_int_env(
            "BICAMERAL_CLOUD_RATE_LIMIT_REQUESTS", 300
        ),
        rate_limit_window_seconds=_int_env(
            "BICAMERAL_CLOUD_RATE_LIMIT_WINDOW_SECONDS", 60
        ),
    )


_settings: CloudSettings | None = None


def get_settings() -> CloudSettings:
    """Singleton settings accessor."""

    global _settings  # noqa: PLW0603
    if _settings is None:
        _settings = load_settings()
    return _settings


def reset_settings(settings: CloudSettings | None = None) -> CloudSettings:
    """Reset settings singleton, optionally overriding with provided settings."""

    global _settings  # noqa: PLW0603
    _settings = settings or load_settings()
    return _settings
