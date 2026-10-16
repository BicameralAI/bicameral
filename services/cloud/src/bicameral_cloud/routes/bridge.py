"""Allowlisted bridge routing API (issue #20, ADR-0007).

Cloud bridge routing is hard allowlist and default-deny. Alpha may route only
explicitly allowed dashboard-intent command families. Authority-bearing commands
are rejected before any daemon forwarding attempt. Unknown command families are
rejected by default. Cloud validates origin, session binding, schema version,
compatibility envelope, and size limits; the daemon authorizes routed intent.
"""

from __future__ import annotations

import json

from fastapi import APIRouter, Depends, HTTPException

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import get_current_user
from bicameral_cloud.bridge.allowlist import MAX_PAYLOAD_BYTES, classify_command
from bicameral_cloud.bridge.routing import (
    ERR_FORBIDDEN_COMMAND,
    ERR_INCOMPATIBLE_DAEMON,
    ERR_INVALID_SESSION,
    ERR_PAIRING_INACTIVE,
    ERR_PAYLOAD_TOO_LARGE,
    ERR_SESSION_EXPIRED,
    ERR_TEAM_MISMATCH,
    ERR_UNKNOWN_COMMAND,
    BridgeError,
    BridgeRequest,
    BridgeResult,
)
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.compatibility import (
    CompatibilityVerdict,
    evaluate_compatibility,
)
from bicameral_cloud.models.pairing import PairingStatus, SessionStatus
from bicameral_cloud.models.tenancy import AuthenticatedUser
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/bridge", tags=["bridge"])


def _reject(
    status_code: int,
    error_code: str,
    error_type: str,
    message: str,
    command_family: str | None = None,
) -> HTTPException:
    """Build a typed error HTTPException."""
    body = BridgeError(
        error_code=error_code,
        error_type=error_type,
        message=message,
        command_family=command_family,
    )
    raise HTTPException(status_code=status_code, detail=body.model_dump())


@router.post(
    "/route",
    response_model=BridgeResult,
    summary="Route a dashboard-intent command through the bridge",
    responses={
        400: {"description": "Payload too large or schema mismatch"},
        403: {"description": "Forbidden authority-bearing command or team mismatch"},
        404: {"description": "Session or pairing not found"},
        409: {"description": "Incompatible daemon or inactive pairing"},
        422: {"description": "Unknown command family (default-deny)"},
    },
)
async def route_bridge_command(
    request: BridgeRequest,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> BridgeResult:
    """Route an allowlisted dashboard-intent command to a paired daemon.

    Validation order:
    1. Classify command family (forbidden → 403, unknown → 422)
    2. Validate payload size
    3. Validate session exists and is open
    4. Validate user is a member of the session's team
    5. Validate pairing is active
    6. Validate daemon compatibility
    7. Forward to daemon adapter (stub in alpha)
    """
    # 1. Command family classification — reject before any forwarding
    classification = classify_command(request.command_family)

    if classification == "forbidden":
        _reject(
            403,
            ERR_FORBIDDEN_COMMAND,
            "authority_violation",
            f"Authority-bearing command '{request.command_family}' "
            "must not be routed through cloud (ADR-0007)",
            command_family=request.command_family,
        )

    if classification == "unknown":
        _reject(
            422,
            ERR_UNKNOWN_COMMAND,
            "default_deny",
            f"Unknown command family '{request.command_family}' "
            "is rejected by default-deny policy",
            command_family=request.command_family,
        )

    # 2. Payload size limit
    payload_size = len(json.dumps(request.payload).encode("utf-8"))
    if payload_size > MAX_PAYLOAD_BYTES:
        _reject(
            400,
            ERR_PAYLOAD_TOO_LARGE,
            "validation_error",
            f"Payload exceeds {MAX_PAYLOAD_BYTES} byte limit ({payload_size} bytes)",
            command_family=request.command_family,
        )

    # 3. Session validation
    session = store.get_session(request.session_id)
    if session is None:
        _reject(
            404,
            ERR_INVALID_SESSION,
            "session_error",
            "Hosted session not found",
            command_family=request.command_family,
        )
        return  # type: ignore[return-value]  # unreachable

    if session.status == SessionStatus.EXPIRED:
        _reject(
            409,
            ERR_SESSION_EXPIRED,
            "session_error",
            "Hosted session has expired",
            command_family=request.command_family,
        )

    if session.status == SessionStatus.CLOSED:
        _reject(
            409,
            ERR_SESSION_EXPIRED,
            "session_error",
            "Hosted session is closed",
            command_family=request.command_family,
        )

    # 4. Team membership check
    membership = store.get_membership(user.user_id, session.team_id)
    if membership is None:
        _reject(
            403,
            ERR_TEAM_MISMATCH,
            "authorization_error",
            "User is not a member of the session's team",
            command_family=request.command_family,
        )

    # 5. Pairing validation
    pairing = store.get_pairing(session.pairing_id)
    if pairing is None or pairing.status != PairingStatus.ACTIVE:
        _reject(
            409,
            ERR_PAIRING_INACTIVE,
            "pairing_error",
            "Daemon pairing is not active",
            command_family=request.command_family,
        )
        return  # type: ignore[return-value]  # unreachable

    # 6. Daemon compatibility check
    compat = evaluate_compatibility(pairing.daemon_version, pairing.capabilities)
    if compat.verdict == CompatibilityVerdict.UPGRADE_REQUIRED:
        _reject(
            409,
            ERR_INCOMPATIBLE_DAEMON,
            "compatibility_error",
            "Paired daemon requires upgrade; bridge routing unavailable",
            command_family=request.command_family,
        )

    # 7. Forward to daemon adapter (stub for alpha — returns ack)
    emit(
        store,
        action=AuditAction.BRIDGE_ROUTED,
        actor_id=user.user_id,
        team_id=session.team_id,
        session_id=request.session_id,
        detail={"command_family": request.command_family},
    )
    return BridgeResult(
        routed=True,
        command_family=request.command_family,
        daemon_response={"status": "ack", "source": "daemon_stub"},
    )
