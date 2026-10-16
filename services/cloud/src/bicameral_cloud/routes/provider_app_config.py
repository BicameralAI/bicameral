"""Google provider application bootstrap boundaries (cloud#262 / cloud#309).

Cloud owns the Google OAuth application registration/configuration boundary.
The public app-config route publishes only native-client metadata required by a
local daemon. The separate credential route requires the caller's existing
Cloud bearer and returns the provider-required Desktop credential only to that
authenticated bootstrap request. Neither route receives Google authorization
codes, PKCE verifiers, access/refresh tokens, or daemon Connection state.
"""

from __future__ import annotations

import hashlib
from datetime import UTC, datetime, timedelta

from fastapi import APIRouter, Depends, HTTPException, Response, status

from bicameral_cloud.auth.dependencies import get_current_user
from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.models.tenancy import AuthenticatedUser

router = APIRouter(prefix="/api/v1", tags=["provider-app-config"])

_GOOGLE_PROVIDER = "google"
_GOOGLE_NATIVE_CLIENT_TYPE = "native"
_SCHEMA_VERSION = 1


def _derived_config_version(settings: CloudSettings) -> str:
    """Derive a safe rotation identifier when one is not explicitly supplied."""

    material = "\n".join(
        [
            settings.google_oauth_native_client_id or "",
            settings.google_oauth_environment or "",
            settings.google_oauth_scope_set_version,
            *settings.google_oauth_scopes,
        ]
    ).encode("utf-8")
    return f"sha256:{hashlib.sha256(material).hexdigest()[:24]}"


def _resolved_public_binding(settings: CloudSettings) -> tuple[str, str, str]:
    """Resolve the exact public application binding shared by both routes."""

    if not settings.google_oauth_app_config_enabled:
        raise HTTPException(
            status_code=status.HTTP_410_GONE,
            detail="Google OAuth application configuration is disabled",
        )

    client_id = (settings.google_oauth_native_client_id or "").strip()
    environment = (settings.google_oauth_environment or "").strip()
    config_version = (
        settings.google_oauth_config_version.strip()
        if settings.google_oauth_config_version
        else _derived_config_version(settings)
    )

    missing: list[str] = []
    if not client_id:
        missing.append("client_id")
    if not environment:
        missing.append("environment")
    if not config_version:
        missing.append("config_version")
    if missing:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail=(
                "Google OAuth application configuration unavailable: "
                + ",".join(missing)
            ),
        )
    return client_id, environment, config_version


@router.get("/provider-app-config/google")
async def google_native_app_config(
    response: Response,
    settings: CloudSettings = Depends(get_settings),  # noqa: B008
) -> dict[str, object]:
    """Return bounded Google native OAuth app metadata for daemon bootstrap.

    This endpoint is intentionally not team-authenticated: the native client id
    and scope policy are non-secret application metadata, and requiring a prior
    provider or team credential would create a bootstrap cycle. HTTPS and the
    deployed Cloud origin provide transport integrity; the daemon separately
    validates environment, schema, expiry, and disablement semantics.
    """

    client_id, environment, config_version = _resolved_public_binding(settings)
    scope_set_version = settings.google_oauth_scope_set_version.strip()
    scopes = tuple(
        scope.strip() for scope in settings.google_oauth_scopes if scope.strip()
    )
    ttl_seconds = settings.google_oauth_app_config_ttl_seconds

    missing: list[str] = []
    if not scope_set_version:
        missing.append("scope_set_version")
    if not scopes:
        missing.append("scopes")
    if ttl_seconds <= 0:
        missing.append("ttl")

    if missing:
        # Missing operator provisioning is an availability/configuration failure,
        # never a reason to return a partial payload or invent defaults.
        detail = (
            "Google OAuth application configuration unavailable: "
            + ",".join(missing)
        )
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail=detail,
        )

    now = datetime.now(UTC)
    expires_at = now + timedelta(seconds=ttl_seconds)

    # The daemon owns bounded caching through expires_at. Intermediaries should
    # not create an independent cache lifecycle that can mask rotation/disablement.
    response.headers["Cache-Control"] = "no-store"

    return {
        "schema_version": _SCHEMA_VERSION,
        "provider": _GOOGLE_PROVIDER,
        "client_type": _GOOGLE_NATIVE_CLIENT_TYPE,
        "client_id": client_id,
        "environment": environment,
        "config_version": config_version,
        "scope_set_version": scope_set_version,
        "scopes": list(scopes),
        "issued_at": now.isoformat().replace("+00:00", "Z"),
        "expires_at": expires_at.isoformat().replace("+00:00", "Z"),
        "enabled": True,
    }


@router.get("/provider-app-credential/google")
async def google_native_app_credential(
    response: Response,
    _user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    settings: CloudSettings = Depends(get_settings),  # noqa: B008
) -> dict[str, object]:
    """Return the provider-required Google Desktop credential to an auth user.

    The Supabase/Cloud bearer is request authority only. It is not exchanged for
    a Google token, does not become daemon identity, and must not be persisted as
    Google provider state. The response carries the exact public application
    binding so the daemon can reject client/environment/version mismatches before
    storing or using the credential.
    """

    client_id, environment, config_version = _resolved_public_binding(settings)
    client_secret = (settings.google_oauth_native_client_secret or "").strip()
    if not client_secret:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Google OAuth application credential unavailable: client_secret",
        )

    # Credential responses must never be cached by browsers, proxies, or other
    # intermediaries. The daemon stores only the validated credential envelope
    # in its OS keyring after exact binding checks.
    response.headers["Cache-Control"] = "no-store"
    response.headers["Pragma"] = "no-cache"

    return {
        "schema_version": _SCHEMA_VERSION,
        "provider": _GOOGLE_PROVIDER,
        "client_type": _GOOGLE_NATIVE_CLIENT_TYPE,
        "client_id": client_id,
        "environment": environment,
        "config_version": config_version,
        "client_secret": client_secret,
    }
