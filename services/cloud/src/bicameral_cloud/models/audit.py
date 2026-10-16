"""Audit event models for cloud coordination (issue #21, ADR-0006).

Audit events record coordination/transport metadata for team-scoped cloud
operations. Audit events must never contain provider secrets, workspace
paths, source contents, or Decision Ledger material.
"""

from __future__ import annotations

import enum
import uuid
from datetime import UTC, datetime

from pydantic import BaseModel, Field


class AuditAction(enum.StrEnum):
    """Cloud coordination actions that produce audit events."""

    AUTH_SESSION_RESOLVED = "auth.session_resolved"
    TEAM_CREATED = "team.created"
    TEAM_MEMBER_ADDED = "team.member_added"
    TEAM_MEMBER_REMOVED = "team.member_removed"
    PAIRING_CREATED = "pairing.created"
    PAIRING_REVOKED = "pairing.revoked"
    PAIRING_IDENTITY_ENROLLED = "pairing.identity_enrolled"
    SESSION_OPENED = "session.opened"
    SESSION_CLOSED = "session.closed"
    BRIDGE_ROUTED = "bridge.routed"
    BRIDGE_REJECTED = "bridge.rejected"
    COMPATIBILITY_CHECKED = "compatibility.checked"
    PROJECT_REGISTRATION_REQUESTED = "project.registration_requested"
    PROJECT_JOIN_REQUESTED = "project.join_requested"
    PROJECT_RECORD_CREATED = "project.record_created"
    PROJECT_RECORD_JOINED = "project.record_joined"


# Fields that must NEVER appear in audit event detail payloads.
FORBIDDEN_AUDIT_FIELDS = frozenset(
    {
        "provider_secret",
        "api_key",
        "access_token",
        "refresh_token",
        "jwt_secret",
        "service_key",
        "workspace_path",
        "local_path",
        "filesystem_path",
        "file_path",
        "source_content",
        "source_body",
        "source_evidence_body",
        "decision_ledger_payload",
        "decision_event",
        "ledger_entry",
        "ledger_material",
        "signoff_state",
        "compliance_state",
        "binding_evidence",
    }
)


class AuditEvent(BaseModel):
    """Immutable audit record for a cloud coordination event.

    Scoped to team and optionally to session. Detail payloads are
    restricted to safe coordination metadata only.
    """

    event_id: str = Field(default_factory=lambda: f"audit-{uuid.uuid4().hex[:12]}")
    action: AuditAction
    team_id: str | None = None
    session_id: str | None = None
    actor_id: str
    timestamp: datetime = Field(default_factory=lambda: datetime.now(UTC))
    detail: dict[str, str | int | bool | None] = Field(default_factory=dict)


class AuditEventResponse(BaseModel):
    """Serialized audit event for API responses."""

    event_id: str
    action: str
    team_id: str | None
    session_id: str | None
    actor_id: str
    timestamp: str
    detail: dict[str, str | int | bool | None]


class AuditListResponse(BaseModel):
    """Paginated list of audit events."""

    team_id: str
    events: list[AuditEventResponse]
    count: int


def audit_event_to_response(event: AuditEvent) -> AuditEventResponse:
    return AuditEventResponse(
        event_id=event.event_id,
        action=event.action.value,
        team_id=event.team_id,
        session_id=event.session_id,
        actor_id=event.actor_id,
        timestamp=event.timestamp.isoformat(),
        detail=event.detail,
    )
