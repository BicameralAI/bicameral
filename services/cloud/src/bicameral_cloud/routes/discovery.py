"""Team-visible workspace discovery route (issue #19, ADR-0007).

Every member of a cloud team sees the same daemon-exposed workspace list for
that team. Cloud renders only minimal discovery metadata by default and does
not grant workspace action authority. No per-user workspace filtering in alpha.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException

from bicameral_cloud.auth.dependencies import get_current_user, require_team_membership
from bicameral_cloud.models.compatibility import (
    CompatibilityVerdict,
    evaluate_compatibility,
)
from bicameral_cloud.models.discovery import (
    WorkspaceDiscoveryResponse,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser, TeamMembership
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["discovery"])


@router.get(
    "/{team_id}/workspaces",
    response_model=WorkspaceDiscoveryResponse,
    summary="List team-visible workspaces",
)
async def list_team_workspaces(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> WorkspaceDiscoveryResponse:
    """Return the daemon-exposed workspace list for a team.

    Visibility is gated by team membership (ADR-0006). All team members
    see the same list — no per-user filtering in alpha.

    Response contains only safe minimal metadata: workspace_id,
    display_name, pairing_status, and coarse readiness. No file paths,
    secrets, evidence bodies, or authority fields are included.
    """
    pairing = store.get_active_pairing_for_team(team_id)
    if pairing is None:
        return WorkspaceDiscoveryResponse(team_id=team_id, workspaces=[])

    compat = evaluate_compatibility(pairing.daemon_version, pairing.capabilities)
    if compat.verdict == CompatibilityVerdict.UPGRADE_REQUIRED:
        raise HTTPException(
            status_code=409,
            detail="Paired daemon requires upgrade; discovery unavailable",
        )

    workspaces = store.get_workspaces(team_id)
    return WorkspaceDiscoveryResponse(
        team_id=team_id,
        workspaces=workspaces,
    )
