"""Auth dependencies for Bicameral Cloud (ADR-0006).

When Supabase Auth is configured, the API resolves user identity from
Supabase access tokens. Otherwise the in-memory store
provides the same interface for tests and local development.
"""

from __future__ import annotations

from fastapi import Depends, Header, HTTPException
from jwt.exceptions import InvalidTokenError

from bicameral_cloud.auth.design_partner_allowlist import (
    DesignPartnerAllowlist,
    DesignPartnerAllowlistUnavailable,
)
from bicameral_cloud.auth.supabase import decode_supabase_jwt
from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.models.tenancy import (
    AuthenticatedUser,
    TeamEligibilitySource,
    TeamMembership,
    TeamRole,
)
from bicameral_cloud.store import Store, get_store


async def get_current_user(
    authorization: str = Header(..., alias='Authorization'),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
    settings: CloudSettings = Depends(get_settings),  # noqa: B008
) -> AuthenticatedUser:
    """Resolve authenticated user from a bearer token."""

    if not authorization.startswith('Bearer '):
        raise HTTPException(status_code=401, detail='Invalid authorization header')

    token = authorization.removeprefix('Bearer ')
    if settings.supabase_auth_enabled:
        return _resolve_from_supabase(token, settings, store)

    user = store.get_user_by_token(token)
    if user is None:
        raise HTTPException(status_code=401, detail='Invalid or expired token')

    _require_design_partner_access(user, settings)
    return user


def _resolve_from_supabase(
    token: str,
    settings: CloudSettings,
    store: Store,
) -> AuthenticatedUser:
    """Validate Supabase JWT and upsert the user into the local store."""

    try:
        payload = decode_supabase_jwt(token, settings)
    except InvalidTokenError as exc:
        raise HTTPException(status_code=401, detail='Invalid or expired token') from exc

    user_id: str = payload['sub']
    email: str = payload.get('email', '')
    metadata = payload.get('user_metadata') or {}
    display_name: str | None = metadata.get('full_name') or metadata.get('name')
    email_verified = payload.get('email_verified')
    if not isinstance(email_verified, bool):
        email_verified = metadata.get('email_verified')
    if not isinstance(email_verified, bool):
        confirmed_at = payload.get('email_confirmed_at')
        if 'email_confirmed_at' in payload:
            email_verified = isinstance(confirmed_at, str) and bool(confirmed_at)
        else:
            email_verified = True
    user = AuthenticatedUser(
        user_id=user_id,
        email=email,
        display_name=display_name,
        email_verified=email_verified,
    )
    _require_design_partner_access(user, settings)
    store.upsert_user(user)
    return user


def _require_design_partner_access(
    user: AuthenticatedUser,
    settings: CloudSettings,
) -> None:
    try:
        allowed = DesignPartnerAllowlist(settings).allows(user)
    except DesignPartnerAllowlistUnavailable as exc:
        raise HTTPException(
            status_code=503,
            detail='Design partner allowlist unavailable',
        ) from exc

    if not allowed:
        raise HTTPException(status_code=403, detail='Design partner access required')


async def require_team_membership(
    team_id: str,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> TeamMembership:
    """Require the current user to be a member of the specified team."""

    membership = store.get_membership(user.user_id, team_id)
    if membership is None:
        raise HTTPException(
            status_code=403,
            detail='Not member of team',
        )
    return membership


async def require_team_coordination_access(
    team_id: str,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> TeamMembership:
    """Require the current user to be eligible to coordinate in a team.

    Eligibility is the cloud#95 domain/team model: an explicit team membership,
    or the team is scoped to the user's verified email domain. A domain-eligible
    user gets a synthesized ``team_member`` membership so create/coordination
    no longer requires a prior membership row or a join code. Users with neither
    are rejected with 403 (the tenancy boundary stays fail-closed, ADR-0007).
    """

    membership = store.get_membership(user.user_id, team_id)
    if membership is not None:
        return membership

    for eligible in store.list_eligible_teams(
        user.user_id, user.verified_email_domain
    ):
        if (
            eligible.team_id == team_id
            and eligible.source == TeamEligibilitySource.DOMAIN
        ):
            return TeamMembership(
                user_id=user.user_id,
                team_id=team_id,
                role=eligible.role,
            )

    raise HTTPException(
        status_code=403,
        detail='Not eligible for team',
    )


async def require_team_owner(
    team_id: str,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> TeamMembership:
    """Require the current user to be an owner of the specified team."""

    membership = store.get_membership(user.user_id, team_id)
    if membership is None:
        raise HTTPException(
            status_code=403,
            detail='Not member of team',
        )
    if membership.role != TeamRole.OWNER:
        raise HTTPException(
            status_code=403,
            detail='Team owner role required',
        )
    return membership
