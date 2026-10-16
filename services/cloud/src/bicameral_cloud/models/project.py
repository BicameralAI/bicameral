"""Hosted Project coordination-record models (issue #88).

A **Project coordination record** is a Cloud tenancy/coordination record for a
project that a team wants to collaborate on. It is created and joined through
the hosted API and shows up in team discovery, but it is **not** canonical
local project truth: project registration and workspace binding remain
daemon-owned (ADR-0005/0006). A newly created or joined Project therefore
reports ``unbound`` local-workspace readiness until a paired daemon projects
readiness for it.

Distinct from the issue #66 ``ProjectRegistrationRequest`` (a pending
expose/join *intent*), a coordination record is the durable, discoverable
Project object the SPA renders.

Records carry only safe coordination labels. Filesystem paths, provider
remotes, and secrets are rejected at the boundary by
``assert_no_cloud_leakage`` (reused from the registration model) so they never
enter Cloud (ADR-0006). ``join_code`` is an opaque invite token minted by
Cloud; it is returned only to the caller who creates the Project and is never
projected into discovery metadata.
"""

from __future__ import annotations

import secrets
import uuid
from datetime import UTC, datetime

from pydantic import BaseModel, Field, field_validator

from bicameral_cloud.models.discovery import BindingReadiness
from bicameral_cloud.models.registration import assert_no_cloud_leakage


def _generate_project_id() -> str:
    return f"proj-{uuid.uuid4().hex[:12]}"


def _generate_join_code() -> str:
    """Mint an opaque, URL-safe join code (not derived from any secret)."""
    return secrets.token_urlsafe(9)


class ProjectCoordinationRecord(BaseModel):
    """A non-canonical Cloud coordination record for a team Project.

    Persisted in the in-memory alpha adapter (``Store``, ADR-0008); the model
    is shaped for a future Supabase-backed ``projects`` table. Creating or
    joining this record never registers a project or binds a local workspace.
    """

    project_id: str = Field(default_factory=_generate_project_id)
    team_id: str
    display_name: str
    created_by: str
    join_code: str = Field(default_factory=_generate_join_code)
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))


class ProjectMembership(BaseModel):
    """Project-level participation record (future ``project_members`` table).

    Distinct from cloud team membership: it records who joined a Project, not
    who may discover it (team membership governs discovery visibility per
    ADR-0007). It grants no daemon workspace authority.
    """

    project_id: str
    team_id: str
    user_id: str
    joined_at: datetime = Field(default_factory=lambda: datetime.now(UTC))


class CreateProjectCoordinationRequest(BaseModel):
    """Body for creating a Project coordination record."""

    display_name: str

    @field_validator("display_name")
    @classmethod
    def _validate_display_name(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("display_name must not be empty")
        return assert_no_cloud_leakage(v, field="display_name")


class JoinProjectByCodeRequest(BaseModel):
    """Body for joining an existing Project via its join code."""

    join_code: str

    @field_validator("join_code")
    @classmethod
    def _validate_join_code(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("join_code must not be empty")
        return v


class ProjectRecordView(BaseModel):
    """Safe SPA-facing view of a Project coordination record.

    Excludes ``join_code``, ``created_by``, and any member list: only safe
    coordination metadata crosses the boundary, and local-workspace readiness
    starts ``unbound`` until a paired daemon reports otherwise (ADR-0007).
    """

    project_id: str
    team_id: str
    display_name: str
    binding_readiness: BindingReadiness = BindingReadiness.UNBOUND


class ProjectCreatedResponse(ProjectRecordView):
    """Create response; carries the ``join_code`` back to the creator only."""

    join_code: str
