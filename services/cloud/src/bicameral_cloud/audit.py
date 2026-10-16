"""Audit event emitter for cloud coordination routes (issue #21, ADR-0006).

Thin helper that builds and stores audit events. All route handlers call
``emit`` to record coordination metadata. The emitter validates that
detail payloads contain no forbidden fields before persisting.
"""

from __future__ import annotations

from bicameral_cloud.models.audit import (
    FORBIDDEN_AUDIT_FIELDS,
    AuditAction,
    AuditEvent,
)
from bicameral_cloud.store import Store


def emit(
    store: Store,
    *,
    action: AuditAction,
    actor_id: str,
    team_id: str | None = None,
    session_id: str | None = None,
    detail: dict[str, str | int | bool | None] | None = None,
) -> AuditEvent:
    """Create and persist an audit event, rejecting forbidden fields."""
    safe_detail = _strip_forbidden(detail or {})
    event = AuditEvent(
        action=action,
        team_id=team_id,
        session_id=session_id,
        actor_id=actor_id,
        detail=safe_detail,
    )
    store.add_audit_event(event)
    return event


def _strip_forbidden(
    detail: dict[str, str | int | bool | None],
) -> dict[str, str | int | bool | None]:
    """Remove any keys that match forbidden audit field names."""
    return {k: v for k, v in detail.items() if k not in FORBIDDEN_AUDIT_FIELDS}
