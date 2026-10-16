"""Hosted project create/join registration models (issue #66).

The hosted dashboard's Create/Join actions record a **coordination request**
in the Cloud alpha adapter; they never register a project or grant workspace
authority themselves (hosted-project-discovery-contract.md, ADR-0005/0006).

- **Create project** = a daemon-owner ``expose_project`` intent. Cloud records
  and routes the request via the hard-allowlist bridge; the local daemon
  remains authoritative for project registration and workspace binding.
- **Join project** = a conservative pending request to join the cloud team a
  project is discoverable under. There is no public self-service join, so the
  request stays ``pending`` until a team owner approves it (existing team
  membership API). Cloud team membership grants discovery visibility only.

Both requests are non-canonical coordination records. They must never carry
local filesystem/workspace paths, provider remotes, or secrets into Cloud
(ADR-0006); ``assert_no_cloud_leakage`` rejects such values at the boundary.
"""

from __future__ import annotations

import enum
import uuid
from datetime import UTC, datetime

from pydantic import BaseModel, Field, field_validator

from bicameral_cloud.models.tenancy import TeamRole

# Substrings that indicate a filesystem path, provider remote, or traversal —
# none of which may cross into Cloud coordination state (ADR-0006, issue #66
# acceptance: "no local provider secrets or workspace paths enter Cloud").
_LEAKAGE_SUBSTRINGS = ("/", "\\", "://", "..")
# Case-insensitive tokens that indicate secret/credential material.
_SECRET_TOKENS = (
    "secret",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "access_token",
    "refresh_token",
    "private_key",
    "credential",
    "jwt",
)


def assert_no_cloud_leakage(value: str, *, field: str) -> str:
    """Reject values that look like a local path, remote, or secret.

    Cloud is a coordination layer only; create/join inputs are opaque labels
    and daemon-local references, never paths or secrets (ADR-0006). Raises
    ``ValueError`` (surfaced by FastAPI as ``422``) when a value is unsafe.
    """
    lowered = value.lower()
    if any(sub in value for sub in _LEAKAGE_SUBSTRINGS):
        raise ValueError(
            f"{field} must not contain a filesystem path or remote; "
            "Cloud stores coordination labels only, not local paths."
        )
    if len(value) >= 2 and value[1] == ":":
        # Windows drive-letter path, e.g. C:\\repo
        raise ValueError(f"{field} must not contain a filesystem path.")
    if any(token in lowered for token in _SECRET_TOKENS):
        raise ValueError(
            f"{field} must not contain secret or credential material; "
            "provider secrets never enter Cloud."
        )
    return value


class ProjectRegistrationKind(enum.StrEnum):
    """The two hosted registration intents (issue #66)."""

    #: Daemon-owner ``expose_project`` intent routed to the paired daemon.
    CREATE_PROJECT = "create_project"
    #: Conservative request to join the cloud team a project is discoverable under.
    JOIN_PROJECT = "join_project"


class ProjectRegistrationStatus(enum.StrEnum):
    """Coordination lifecycle for a registration request.

    Alpha keeps a single durable ``pending`` state: Cloud records the request
    and defers the authoritative action (daemon expose/bind for create; team
    owner approval for join). Terminal states are owned by the authoritative
    actor and are out of scope for this slice.
    """

    PENDING = "pending"


class ProjectRegistrationNextStep(enum.StrEnum):
    """Machine-readable next step so the SPA can render an unambiguous CTA.

    Distinguishes the Cloud-membership axis from the local daemon workspace
    binding axis (issue #66 acceptance: UI must distinguish the two).
    """

    #: Cloud recorded the intent; the local daemon must expose/bind the project.
    LOCAL_DAEMON_BINDING_REQUIRED = "local_daemon_binding_required"
    #: Cloud recorded a pending join; a team owner must approve access.
    TEAM_OWNER_APPROVAL_REQUIRED = "team_owner_approval_required"


class ProjectRegistrationRequest(BaseModel):
    """A non-canonical coordination record for a create/join request.

    Persisted in the in-memory alpha adapter (``Store``, ADR-0008). It never
    mutates daemon workspace binding, creates a project, or grants team
    membership; it only records the routed/pending intent.
    """

    request_id: str = Field(
        default_factory=lambda: f"reg-{uuid.uuid4().hex[:12]}"
    )
    kind: ProjectRegistrationKind
    team_id: str
    requested_by: str
    status: ProjectRegistrationStatus = ProjectRegistrationStatus.PENDING
    display_name: str | None = None
    project_ref: str | None = None
    requested_role: TeamRole = TeamRole.MEMBER
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))


class CreateProjectRequest(BaseModel):
    """Body for the hosted Create action (``expose_project`` intent)."""

    display_name: str
    #: Opaque daemon-local reference for the workspace to expose; NOT a path.
    project_ref: str | None = None

    @field_validator("display_name")
    @classmethod
    def _validate_display_name(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("display_name must not be empty")
        return assert_no_cloud_leakage(v, field="display_name")

    @field_validator("project_ref")
    @classmethod
    def _validate_project_ref(cls, v: str | None) -> str | None:
        if v is None:
            return None
        v = v.strip()
        if not v:
            return None
        return assert_no_cloud_leakage(v, field="project_ref")


class JoinProjectRequest(BaseModel):
    """Body for the hosted Join action (conservative pending request).

    ``requested_role`` defaults to member; owner-granting is a team-owner
    action and is never self-granted through a join request.
    """

    requested_role: TeamRole = TeamRole.MEMBER

    @field_validator("requested_role")
    @classmethod
    def _no_self_grant_owner(cls, v: TeamRole) -> TeamRole:
        if v == TeamRole.OWNER:
            raise ValueError(
                "A join request cannot self-grant team_owner; request "
                "team_member access — owner promotion is an owner action."
            )
        return v


class RegistrationReceipt(BaseModel):
    """Coordination receipt returned for an accepted create/join request."""

    request_id: str
    kind: ProjectRegistrationKind
    team_id: str
    status: ProjectRegistrationStatus
    next_step: ProjectRegistrationNextStep
    message: str


class RegistrationRequestItem(BaseModel):
    """Safe SPA-facing view of a pending registration request."""

    request_id: str
    kind: ProjectRegistrationKind
    team_id: str
    requested_by: str
    status: ProjectRegistrationStatus
    display_name: str | None = None
    requested_role: TeamRole


class RegistrationRequestListResponse(BaseModel):
    """Team-scoped list of pending registration requests."""

    team_id: str
    requests: list[RegistrationRequestItem]
