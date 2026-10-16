"""Hosted session API (issue #17, ADR-0005, ADR-0006).

Cloud tracks authenticated dashboard-to-daemon session state but must not
become canonical daemon session authority. Sessions bind an authenticated
user to a team pairing and record intended origin/session metadata.
"""

from __future__ import annotations

import uuid

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import get_current_user, require_team_membership
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.pairing import (
    HostedSession,
    PairingStatus,
    SessionStatus,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser, TeamMembership, TeamRole
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["session"])


class OpenSessionRequest(BaseModel):
    """Request body for opening a hosted session."""

    pairing_id: str
    origin: str | None = None
    schema_version: str = "1.0"


class SessionResponse(BaseModel):
    """Serialized hosted session record."""

    session_id: str
    user_id: str
    team_id: str
    pairing_id: str
    status: str
    origin: str | None
    schema_version: str
    created_at: str


class SessionListResponse(BaseModel):
    """List of session records for a team."""

    team_id: str
    sessions: list[SessionResponse]


def _session_to_response(session: HostedSession) -> SessionResponse:
    return SessionResponse(
        session_id=session.session_id,
        user_id=session.user_id,
        team_id=session.team_id,
        pairing_id=session.pairing_id,
        status=session.status.value,
        origin=session.origin,
        schema_version=session.schema_version,
        created_at=session.created_at.isoformat(),
    )


@router.post("/{team_id}/sessions", status_code=201)
async def open_session(
    team_id: str,
    body: OpenSessionRequest,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> SessionResponse:
    """Open a new hosted session for the authenticated user.

    Requires an active pairing belonging to the team.
    """
    pairing = store.get_pairing(body.pairing_id)
    if pairing is None or pairing.team_id != team_id:
        raise HTTPException(
            status_code=404, detail="Pairing not found for this team"
        )
    if pairing.status != PairingStatus.ACTIVE:
        raise HTTPException(
            status_code=409, detail="Pairing is not active"
        )
    session = HostedSession(
        session_id=f"session-{uuid.uuid4().hex[:12]}",
        user_id=user.user_id,
        team_id=team_id,
        pairing_id=body.pairing_id,
        status=SessionStatus.OPEN,
        origin=body.origin,
        schema_version=body.schema_version,
    )
    store.add_session(session)
    emit(
        store,
        action=AuditAction.SESSION_OPENED,
        actor_id=user.user_id,
        team_id=team_id,
        session_id=session.session_id,
        detail={"pairing_id": body.pairing_id},
    )
    return _session_to_response(session)


@router.get("/{team_id}/sessions")
async def list_sessions(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> SessionListResponse:
    """List all hosted sessions for the team."""
    sessions = store.list_sessions_for_team(team_id)
    return SessionListResponse(
        team_id=team_id,
        sessions=[_session_to_response(s) for s in sessions],
    )


@router.get("/{team_id}/sessions/{session_id}")
async def get_session_status(
    team_id: str,
    session_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> SessionResponse:
    """Get a specific session's status."""
    session = store.get_session(session_id)
    if session is None or session.team_id != team_id:
        raise HTTPException(status_code=404, detail="Session not found")
    return _session_to_response(session)


@router.post("/{team_id}/sessions/{session_id}/close")
async def close_session(
    team_id: str,
    session_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> SessionResponse:
    """Close a hosted session.

    Only the session owner or a team owner may close a session.
    """
    session = store.get_session(session_id)
    if session is None or session.team_id != team_id:
        raise HTTPException(status_code=404, detail="Session not found")
    if session.status != SessionStatus.OPEN:
        raise HTTPException(
            status_code=409, detail="Session is not open"
        )
    if session.user_id != user.user_id and membership.role != TeamRole.OWNER:
        raise HTTPException(
            status_code=403,
            detail="Only session owner or team owner may close this session",
        )
    store.close_session(session_id)
    emit(
        store,
        action=AuditAction.SESSION_CLOSED,
        actor_id=user.user_id,
        team_id=team_id,
        session_id=session_id,
    )
    return _session_to_response(session)
