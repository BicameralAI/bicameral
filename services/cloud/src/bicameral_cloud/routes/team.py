"""Team management routes (issue #16, ADR-0006).

Cloud team tenancy. team_owner may manage membership; team_member may
read team info and member list. Neither role implies daemon workspace
authority.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import (
    get_current_user,
    require_team_membership,
    require_team_owner,
)
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.tenancy import (
    AddMemberRequest,
    AuthenticatedUser,
    CreateTeamRequest,
    TeamMembership,
    TeamRole,
)
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["team"])


@router.post("")
async def create_team(
    body: CreateTeamRequest,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str]:
    """Create a new team. The caller becomes team_owner.

    An optional ``domain`` scopes the team so design-partner-allowed users with
    a matching verified email domain discover its Projects without an explicit
    membership or join code (cloud#95 domain-gated eligibility).
    """
    team = store.create_team(
        display_name=body.display_name,
        created_by=user.user_id,
        domain=body.domain,
    )
    emit(
        store,
        action=AuditAction.TEAM_CREATED,
        actor_id=user.user_id,
        team_id=team.team_id,
        detail={"display_name": team.display_name},
    )
    return {
        "team_id": team.team_id,
        "display_name": team.display_name,
        "created_by": team.created_by,
        "role": TeamRole.OWNER.value,
    }


@router.get("/{team_id}")
async def get_team_info(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str | None]:
    """Return basic team info for an authenticated member."""
    team = store.get_team(team_id)
    return {
        "team_id": team_id,
        "display_name": team.display_name if team else None,
        "role": membership.role.value,
        "user_id": membership.user_id,
    }


@router.get("/{team_id}/members")
async def list_members(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, object]:
    """List all members of a team. Requires team membership."""
    members = store.list_team_members(team_id)
    return {
        "team_id": team_id,
        "members": [
            {
                "user_id": m.user_id,
                "role": m.role.value,
                "joined_at": m.joined_at.isoformat(),
            }
            for m in members
        ],
    }


@router.post("/{team_id}/members")
async def add_member(
    team_id: str,
    body: AddMemberRequest,
    owner: TeamMembership = Depends(require_team_owner),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str]:
    """Add a member to a team. Requires team_owner role."""
    existing = store.get_membership(body.user_id, team_id)
    if existing is not None:
        raise HTTPException(status_code=409, detail="User is already a team member")
    membership = TeamMembership(
        user_id=body.user_id,
        team_id=team_id,
        role=body.role,
    )
    store.add_membership(membership)
    emit(
        store,
        action=AuditAction.TEAM_MEMBER_ADDED,
        actor_id=owner.user_id,
        team_id=team_id,
        detail={"added_user_id": body.user_id, "role": body.role.value},
    )
    return {
        "team_id": team_id,
        "user_id": body.user_id,
        "role": body.role.value,
    }


@router.delete("/{team_id}/members/{user_id}")
async def remove_member(
    team_id: str,
    user_id: str,
    owner: TeamMembership = Depends(require_team_owner),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str]:
    """Remove a member from a team. Requires team_owner role."""
    if user_id == owner.user_id:
        raise HTTPException(status_code=400, detail="Cannot remove yourself as owner")
    removed = store.remove_membership(user_id, team_id)
    if not removed:
        raise HTTPException(status_code=404, detail="Membership not found")
    emit(
        store,
        action=AuditAction.TEAM_MEMBER_REMOVED,
        actor_id=owner.user_id,
        team_id=team_id,
        detail={"removed_user_id": user_id},
    )
    return {"team_id": team_id, "user_id": user_id, "removed": "true"}
