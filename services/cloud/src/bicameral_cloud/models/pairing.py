"""Daemon pairing and hosted session models.

Cloud coordinates pairing/session state but does not grant workspace authority
or accept provider secrets (ADR-0006, ADR-0007).
"""

from __future__ import annotations

import enum
from datetime import UTC, datetime

from pydantic import BaseModel, Field

from bicameral_cloud.models.compatibility import DaemonCapabilities


class PairingStatus(enum.StrEnum):
    ACTIVE = "active"
    REVOKED = "revoked"


class SessionStatus(enum.StrEnum):
    OPEN = "open"
    CLOSED = "closed"
    EXPIRED = "expired"


class DaemonPairing(BaseModel):
    """Cloud coordination record for a paired daemon."""

    pairing_id: str
    team_id: str
    daemon_id: str
    status: PairingStatus = PairingStatus.ACTIVE
    daemon_version: str | None = None
    capabilities: DaemonCapabilities | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
    created_by_user_id: str | None = None
    # Coordination-only identity (factory #901 row 12 / cloud#302). Public half
    # of the daemon Option-1 key; never a private key, event body, or join key.
    enrolled_by_user_id: str | None = None
    identity_key_id: str | None = None
    identity_public_key_b64: str | None = None


class HostedSession(BaseModel):
    """Authenticated dashboard-to-daemon session record."""

    session_id: str
    user_id: str
    team_id: str
    pairing_id: str
    status: SessionStatus = SessionStatus.OPEN
    origin: str | None = None
    schema_version: str = "1.0"
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))
