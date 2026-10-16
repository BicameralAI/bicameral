"""Bridge routing logic (ADR-0007, issue #20).

Cloud validates origin, route, session binding, schema version, compatibility
envelope, and size limits; the daemon authorizes routed intent semantics.
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class BridgeRequest(BaseModel):
    """Inbound bridge routing request from hosted dashboard."""

    command_family: str
    payload: dict[str, object] = Field(default_factory=dict)
    session_id: str
    schema_version: str = "1.0"


class BridgeError(BaseModel):
    """Typed error response for rejected bridge requests."""

    error_code: str
    error_type: str
    message: str
    command_family: str | None = None


class BridgeResult(BaseModel):
    """Successful bridge routing result."""

    routed: bool
    command_family: str
    daemon_response: dict[str, object] | None = None


# Error codes
ERR_FORBIDDEN_COMMAND = "FORBIDDEN_COMMAND"
ERR_UNKNOWN_COMMAND = "UNKNOWN_COMMAND"
ERR_INVALID_SESSION = "INVALID_SESSION"
ERR_TEAM_MISMATCH = "TEAM_MISMATCH"
ERR_SESSION_EXPIRED = "SESSION_EXPIRED"
ERR_INCOMPATIBLE_DAEMON = "INCOMPATIBLE_DAEMON"
ERR_PAYLOAD_TOO_LARGE = "PAYLOAD_TOO_LARGE"
ERR_SCHEMA_MISMATCH = "SCHEMA_MISMATCH"
ERR_PAIRING_INACTIVE = "PAIRING_INACTIVE"
