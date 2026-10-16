"""Hosted project create/join registration routes (issue #66).

Wires the hosted dashboard's Create/Join actions to Cloud coordination:

- ``POST /api/v1/teams/{team_id}/projects`` — a team owner records an
  ``expose_project`` intent. Cloud persists a pending coordination request,
  creates the discoverable Project coordination record it refers to (cloud#88,
  so the canonical ``/app`` SPA surfaces the Project as ``unbound`` immediately
  after create -> refetch), and audits both. The local daemon remains
  authoritative for registering the project and binding the workspace
  (ADR-0005): the record stays ``unbound`` until a paired daemon exposes it.
  Returns ``202 Accepted`` with the unchanged registration receipt.
- ``POST /api/v1/teams/{team_id}/join-requests`` — an authenticated
  design-partner user records a conservative pending request to join a team.
  There is no public self-service join, so it stays pending until a team owner
  approves via the existing membership API. Returns ``202 Accepted``.
- ``GET /api/v1/teams/{team_id}/registration-requests`` — a team member lists
  the team's pending requests.

None of these mutate daemon workspace binding, register a project, or grant
team membership. Cloud membership grants discovery visibility only, never
local workspace authority (ADR-0006).
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
from bicameral_cloud.models.project import (
    ProjectCoordinationRecord,
    ProjectMembership,
)
from bicameral_cloud.models.registration import (
    CreateProjectRequest,
    JoinProjectRequest,
    ProjectRegistrationKind,
    ProjectRegistrationNextStep,
    ProjectRegistrationRequest,
    ProjectRegistrationStatus,
    RegistrationReceipt,
    RegistrationRequestItem,
    RegistrationRequestListResponse,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser, TeamMembership
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["project-registration"])

_CREATE_NEXT_STEP_MESSAGE = (
    "Request recorded. Your local Bicameral daemon must expose and bind this "
    "workspace before it appears; Cloud never binds local workspaces."
)
_JOIN_NEXT_STEP_MESSAGE = (
    "Join request recorded and pending. A team owner must approve access; "
    "Cloud membership grants project discovery visibility only, not local "
    "daemon workspace authority."
)


def _receipt(request: ProjectRegistrationRequest) -> RegistrationReceipt:
    if request.kind == ProjectRegistrationKind.CREATE_PROJECT:
        next_step = ProjectRegistrationNextStep.LOCAL_DAEMON_BINDING_REQUIRED
        message = _CREATE_NEXT_STEP_MESSAGE
    else:
        next_step = ProjectRegistrationNextStep.TEAM_OWNER_APPROVAL_REQUIRED
        message = _JOIN_NEXT_STEP_MESSAGE
    return RegistrationReceipt(
        request_id=request.request_id,
        kind=request.kind,
        team_id=request.team_id,
        status=request.status,
        next_step=next_step,
        message=message,
    )


@router.post("/{team_id}/projects", status_code=202)
async def create_project(
    team_id: str,
    body: CreateProjectRequest,
    membership: TeamMembership = Depends(require_team_owner),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> RegistrationReceipt:
    """Record a team owner's ``expose_project`` intent.

    Cloud persists the pending expose request **and** the discoverable Project
    coordination record it refers to (cloud#88), so the canonical ``/app`` SPA
    surfaces the new Project (``unbound``) immediately on its post-create
    refetch. Neither step registers the project or binds a local workspace:
    the record stays ``unbound`` until a paired daemon exposes it (ADR-0005),
    keeping local workspace-binding authority with the daemon.
    """
    request = ProjectRegistrationRequest(
        kind=ProjectRegistrationKind.CREATE_PROJECT,
        team_id=team_id,
        requested_by=membership.user_id,
        display_name=body.display_name,
        project_ref=body.project_ref,
    )
    store.add_registration_request(request)

    # Make the Project discoverable now (cloud#88): the SPA renders coordination
    # records via GET /api/v1/projects as `unbound` until a daemon binds them.
    record = ProjectCoordinationRecord(
        team_id=team_id,
        display_name=body.display_name,
        created_by=membership.user_id,
    )
    store.add_project(record)
    store.add_project_member(
        ProjectMembership(
            project_id=record.project_id,
            team_id=team_id,
            user_id=membership.user_id,
        )
    )
    emit(
        store,
        action=AuditAction.PROJECT_REGISTRATION_REQUESTED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={
            "request_id": request.request_id,
            "kind": request.kind.value,
            "display_name": body.display_name,
            "has_project_ref": body.project_ref is not None,
            "project_id": record.project_id,
        },
    )
    emit(
        store,
        action=AuditAction.PROJECT_RECORD_CREATED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={
            "project_id": record.project_id,
            "display_name": record.display_name,
        },
    )
    return _receipt(request)


@router.post(
    "/{team_id}/join-requests",
    status_code=202,
    deprecated=True,
    summary="Legacy owner-reviewed team join request",
)
async def request_join(
    team_id: str,
    body: JoinProjectRequest,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> RegistrationReceipt:
    """Legacy compatibility route, not the primary Product onboarding path."""
    if store.get_membership(user.user_id, team_id) is not None:
        raise HTTPException(
            status_code=409,
            detail="Already a member of this team",
        )

    existing = store.find_pending_registration_request(
        team_id=team_id,
        requested_by=user.user_id,
        kind=ProjectRegistrationKind.JOIN_PROJECT,
    )
    if existing is not None:
        return _receipt(existing)

    request = ProjectRegistrationRequest(
        kind=ProjectRegistrationKind.JOIN_PROJECT,
        team_id=team_id,
        requested_by=user.user_id,
        requested_role=body.requested_role,
    )
    store.add_registration_request(request)
    emit(
        store,
        action=AuditAction.PROJECT_JOIN_REQUESTED,
        actor_id=user.user_id,
        team_id=team_id,
        detail={
            "request_id": request.request_id,
            "kind": request.kind.value,
            "requested_role": request.requested_role.value,
        },
    )
    return _receipt(request)


@router.get("/{team_id}/registration-requests")
async def list_registration_requests(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> RegistrationRequestListResponse:
    """List the team's pending create/join requests for a member."""
    requests = store.list_registration_requests_for_team(
        team_id,
        status=ProjectRegistrationStatus.PENDING,
    )
    return RegistrationRequestListResponse(
        team_id=team_id,
        requests=[
            RegistrationRequestItem(
                request_id=r.request_id,
                kind=r.kind,
                team_id=r.team_id,
                requested_by=r.requested_by,
                status=r.status,
                display_name=r.display_name,
                requested_role=r.requested_role,
            )
            for r in requests
        ],
    )
