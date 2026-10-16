'''Auth routes (issue #16, ADR-0006).

Supabase Auth session endpoints. Google login is configured in Supabase; these
routes expose public browser config and validate bearer tokens for the hosted
dashboard.
'''

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import get_current_user
from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.tenancy import AuthenticatedUser
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix='/api/v1/auth', tags=['auth'])


@router.get('/config')
async def auth_config(
    settings: CloudSettings = Depends(get_settings),  # noqa: B008
) -> dict[str, str | bool | None]:
    '''Return public browser auth configuration without server-side secrets.'''

    return {
        'supabase_url': settings.supabase_url,
        'supabase_anon_key': settings.supabase_anon_key,
        'supabase_auth_enabled': settings.supabase_auth_enabled,
        'supabase_browser_auth_configured': settings.supabase_browser_auth_configured,
    }


@router.get('/session')
async def session_info(
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str | None]:
    '''Return current authenticated user's session identity.'''

    emit(
        store,
        action=AuditAction.AUTH_SESSION_RESOLVED,
        actor_id=user.user_id,
    )
    return {
        'user_id': user.user_id,
        'email': user.email,
        'display_name': user.display_name,
    }


@router.get('/me/teams')
async def my_teams(
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, object]:
    '''List teams the current user is eligible to discover Projects in.

    Returns explicit cloud team memberships unioned with teams scoped to the
    user's verified email domain (cloud#95). ``source`` distinguishes the two;
    domain-eligible teams have no membership timestamp so ``joined_at`` is null.
    '''

    memberships = {m.team_id: m for m in store.list_user_teams(user.user_id)}
    eligible = store.list_eligible_teams(
        user.user_id, user.verified_email_domain
    )
    return {
        'user_id': user.user_id,
        'teams': [
            {
                'team_id': e.team_id,
                'role': e.role.value,
                'source': e.source.value,
                'joined_at': (
                    memberships[e.team_id].joined_at.isoformat()
                    if e.team_id in memberships
                    else None
                ),
            }
            for e in eligible
        ],
    }


@router.post('/pairing-tenancy')
async def pairing_tenancy(
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str]:
    '''Resolve the sole Alpha Cloud team for hosted pair initiation (bot#1290).

    Auto-creates a domain-scoped team when the signed-in user has none.
    Fails closed with typed, distinguishable 409s when tenancy cannot be
    resolved. The multiple-team case includes the operator repair contract;
    Cloud never guesses which coordination team should own the pairing.
    '''

    try:
        team_id, source = store.resolve_pairing_tenancy(user)
    except ValueError as exc:
        code = str(exc)
        if code == 'multiple_teams':
            raise HTTPException(
                status_code=409,
                detail={
                    'kind': 'PairingTenancyAmbiguous',
                    'message': (
                        'Multiple Cloud teams exist for this verified domain.'
                    ),
                    'repair': (
                        'Reconcile the domain to exactly one Cloud team, then '
                        'retry pairing.'
                    ),
                },
            ) from exc
        if code == 'missing_domain':
            raise HTTPException(
                status_code=409,
                detail={
                    'kind': 'PairingVerifiedDomainRequired',
                    'message': (
                        'A verified email domain is required to resolve pairing '
                        'tenancy.'
                    ),
                    'repair': (
                        'Sign in with an allowlisted account whose email domain '
                        'is verified.'
                    ),
                },
            ) from exc
        raise HTTPException(
            status_code=409,
            detail={
                'kind': 'PairingTenancyUnavailable',
                'message': 'Pairing tenancy could not be resolved.',
                'repair': 'Retry after the Cloud tenancy state is reconciled.',
            },
        ) from exc
    if source == 'created':
        team = store.get_team(team_id)
        emit(
            store,
            action=AuditAction.TEAM_CREATED,
            actor_id=user.user_id,
            team_id=team_id,
            detail={'display_name': team.display_name if team else team_id},
        )
    return {
        'user_id': user.user_id,
        'team_id': team_id,
        'source': source,
    }
