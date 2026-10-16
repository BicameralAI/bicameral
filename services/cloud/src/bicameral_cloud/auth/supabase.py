"""Supabase Auth token validation (ADR-0006).

When SUPABASE_JWT_SECRET is configured, tokens are validated locally. Hosted
signup can also validate browser access tokens through Supabase Auth's user
endpoint with SUPABASE_URL and SUPABASE_ANON_KEY, avoiding a dashboard-only
JWT secret requirement for the alpha deployment.
"""

from __future__ import annotations

from typing import Any

import httpx
import jwt
from jwt.exceptions import InvalidTokenError

from bicameral_cloud.config import CloudSettings


def decode_supabase_jwt(
    token: str,
    settings: CloudSettings,
) -> dict[str, Any]:
    """Validate a Supabase Auth access token and return identity claims."""

    if settings.supabase_jwt_secret:
        return jwt.decode(
            token,
            settings.supabase_jwt_secret,
            algorithms=["HS256"],
            audience="authenticated",
            options={"require": ["sub", "exp", "aud"]},
        )

    if settings.supabase_url and settings.supabase_anon_key:
        return _fetch_supabase_user(token, settings)

    raise InvalidTokenError("Supabase token validation not configured")


def _fetch_supabase_user(token: str, settings: CloudSettings) -> dict[str, Any]:
    if not settings.supabase_url or not settings.supabase_anon_key:
        raise InvalidTokenError("Supabase Auth API not configured")

    url = settings.supabase_url.rstrip('/') + '/auth/v1/user'
    headers = {
        'apikey': settings.supabase_anon_key,
        'authorization': f'Bearer {token}',
        'accept': 'application/json',
    }

    try:
        response = httpx.get(url, headers=headers, timeout=5.0)
    except httpx.HTTPError as exc:
        raise InvalidTokenError("Supabase Auth API unavailable") from exc

    if response.status_code != 200:
        raise InvalidTokenError("Supabase Auth token rejected")

    try:
        payload = response.json()
    except ValueError as exc:
        raise InvalidTokenError("Supabase Auth API returned invalid JSON") from exc

    user_id = payload.get('id')
    email = payload.get('email') or ''
    if not isinstance(user_id, str) or not user_id:
        raise InvalidTokenError("Supabase Auth API returned no user id")

    metadata = payload.get('user_metadata') or {}
    if not isinstance(metadata, dict):
        metadata = {}

    return {
        'sub': user_id,
        'email': email,
        'email_confirmed_at': payload.get('email_confirmed_at'),
        'user_metadata': metadata,
    }
