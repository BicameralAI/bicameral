"""Compatibility route (issue #18, ADR-0007).

Evaluates daemon protocol compatibility for the support window. Returns
verdict, mutable controls state, and upgrade messaging. Cloud does not
mutate daemon state or guess command compatibility.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import require_team_membership
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.compatibility import (
    CURRENT_PROTOCOL_MAJOR,
    CURRENT_PROTOCOL_MINOR,
    CompatibilityResult,
    CompatibilityVerdict,
    MutableControlsState,
    evaluate_compatibility,
)
from bicameral_cloud.models.tenancy import TeamMembership
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["compatibility"])


@router.get("/{team_id}/compatibility")
async def check_compatibility(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> dict[str, str | bool | None]:
    """Check protocol compatibility for the team's paired daemon.

    Returns the compatibility verdict, mutable controls state, and any
    upgrade messaging. Does not mutate daemon state.
    """
    cloud_protocol = f"{CURRENT_PROTOCOL_MAJOR}.{CURRENT_PROTOCOL_MINOR}"

    pairing = store.get_active_pairing_for_team(team_id)
    if pairing is None:
        return {
            "team_id": team_id,
            "verdict": CompatibilityVerdict.UPGRADE_REQUIRED.value,
            "mutable_controls": MutableControlsState.BLOCKED.value,
            "daemon_version": None,
            "cloud_protocol": cloud_protocol,
            "message": "No active daemon pairing for this team.",
        }

    result: CompatibilityResult = evaluate_compatibility(
        pairing.daemon_version, pairing.capabilities
    )
    emit(
        store,
        action=AuditAction.COMPATIBILITY_CHECKED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={
            "verdict": result.verdict.value,
            "daemon_version": pairing.daemon_version or "unknown",
        },
    )
    return {
        "team_id": team_id,
        "verdict": result.verdict.value,
        "mutable_controls": result.mutable_controls.value,
        "daemon_version": pairing.daemon_version or "unknown",
        "cloud_protocol": result.cloud_protocol,
        "message": result.message,
    }
