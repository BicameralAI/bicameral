"""Audit routes for cloud coordination events (issue #21, ADR-0006).

Query endpoints for coordination/transport audit metadata scoped to team
membership. Audit events never contain provider secrets, workspace paths,
source contents, or Decision Ledger material.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends

from bicameral_cloud.auth.dependencies import require_team_membership
from bicameral_cloud.models.audit import (
    AuditListResponse,
    audit_event_to_response,
)
from bicameral_cloud.models.tenancy import TeamMembership
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1", tags=["audit"])


@router.get("/audit/status")
async def audit_status() -> dict[str, str]:
    """Return audit subsystem readiness.

    No authentication required for the status probe.
    """
    return {"audit": "enabled", "status": "alpha"}


@router.get("/teams/{team_id}/audit")
async def list_team_audit_events(
    team_id: str,
    session_id: str | None = None,
    action: str | None = None,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> AuditListResponse:
    """List audit events scoped to a team.

    Requires team membership. Optional filters: session_id, action.
    """
    events = store.list_audit_events_for_team(
        team_id,
        session_id=session_id,
        action=action,
    )
    responses = [audit_event_to_response(e) for e in events]
    return AuditListResponse(
        team_id=team_id,
        events=responses,
        count=len(responses),
    )
