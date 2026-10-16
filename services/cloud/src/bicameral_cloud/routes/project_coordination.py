"""Hosted Project create/join coordination routes (issue #88, cloud#95).

Adds the hosted Project create/join API backed by the Supabase-authenticated
Cloud tenancy model. A **Project coordination record** is a Cloud
tenancy/coordination object — never canonical local project truth. Creating or
joining one records coordination state and exposes the Project in team
discovery; it never registers a project, binds a local workspace, or grants
daemon authority (ADR-0005/0006).

- ``POST /api/v1/teams/{team_id}/project-records`` — a design-partner-allowed
  user who is eligible to coordinate in the team (an explicit member, or the
  team is scoped to their verified email domain) creates a Project coordination
  record. Returns ``201`` with the record's safe view. The response still
  carries a minted ``join_code`` for **legacy manual-invite compatibility**
  only; per cloud#95 the default discovery/join path is domain/team eligibility
  and does not depend on the join code.
- ``POST /api/v1/teams/{team_id}/project-records/{project_id}/join`` —
  **DEPRECATED / legacy manual-invite compatibility.** Lets an eligible team
  member join an existing Project by presenting its ``join_code``. This is not
  the default discovery/join path and is explicitly excluded from Project
  Discovery PRD conformance (cloud#95); domain/team eligibility makes eligible
  Projects discoverable without a user-entered code.

Coordination access is governed by cloud#95 domain/team eligibility
(``require_team_coordination_access``): users who are neither members nor
domain-eligible are rejected (403) before any create/join work runs. A
created/joined Project reports ``unbound`` local-workspace readiness until a
paired daemon projects readiness for it (ADR-0007).
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Response

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import require_team_coordination_access
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.discovery import BindingReadiness
from bicameral_cloud.models.project import (
    CreateProjectCoordinationRequest,
    JoinProjectByCodeRequest,
    ProjectCoordinationRecord,
    ProjectCreatedResponse,
    ProjectMembership,
    ProjectRecordView,
)
from bicameral_cloud.models.tenancy import TeamMembership
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["project-coordination"])


@router.post("/{team_id}/project-records", status_code=201)
async def create_project_record(
    team_id: str,
    body: CreateProjectCoordinationRequest,
    membership: TeamMembership = Depends(  # noqa: B008
        require_team_coordination_access
    ),
    store: Store = Depends(get_store),  # noqa: B008
) -> ProjectCreatedResponse:
    """Create a team-scoped Project coordination record.

    Open to any user eligible to coordinate in the team under cloud#95
    domain/team gating (an explicit member or a verified-domain match); the
    created Project immediately appears in domain/team discovery for every
    eligible user. The returned ``join_code`` is legacy manual-invite metadata
    only and is never required for others to discover the Project.
    """
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
        action=AuditAction.PROJECT_RECORD_CREATED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={
            "project_id": record.project_id,
            "display_name": record.display_name,
        },
    )
    return ProjectCreatedResponse(
        project_id=record.project_id,
        team_id=record.team_id,
        display_name=record.display_name,
        binding_readiness=BindingReadiness.UNBOUND,
        join_code=record.join_code,
    )


@router.post(
    "/{team_id}/project-records/{project_id}/join",
    deprecated=True,
)
async def join_project_record(
    team_id: str,
    project_id: str,
    body: JoinProjectByCodeRequest,
    response: Response,
    membership: TeamMembership = Depends(  # noqa: B008
        require_team_coordination_access
    ),
    store: Store = Depends(get_store),  # noqa: B008
) -> ProjectRecordView:
    """Join an existing Project by join code — DEPRECATED legacy path.

    Retained only for manual-invite compatibility (cloud#95): it is not the
    default discovery/join mechanism and is excluded from Project Discovery PRD
    conformance. Domain/team eligibility already makes eligible Projects
    discoverable without a user-entered code. The response carries a
    ``Deprecation: true`` header to signal the legacy status to clients.
    """
    response.headers["Deprecation"] = "true"
    record = store.get_project(project_id)
    if record is None or record.team_id != team_id:
        raise HTTPException(status_code=404, detail="Project not found")
    if body.join_code != record.join_code:
        raise HTTPException(status_code=403, detail="Invalid join code")

    store.add_project_member(
        ProjectMembership(
            project_id=record.project_id,
            team_id=team_id,
            user_id=membership.user_id,
        )
    )
    emit(
        store,
        action=AuditAction.PROJECT_RECORD_JOINED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={"project_id": record.project_id},
    )
    return ProjectRecordView(
        project_id=record.project_id,
        team_id=record.team_id,
        display_name=record.display_name,
        binding_readiness=BindingReadiness.UNBOUND,
    )
