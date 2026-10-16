"""Daemon pairing API (issue #17, ADR-0005, ADR-0006, ADR-0007).

Cloud coordinates daemon pairing records scoped to team/user authorization.
Pairing records do not include provider secrets, workspace paths, or Decision
Ledger state. Cloud may create, list, and revoke coordination records but must
not expose every local workspace automatically.
"""

from __future__ import annotations

import base64
import binascii

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field

from bicameral_cloud.audit import emit
from bicameral_cloud.auth.dependencies import (
    get_current_user,
    require_team_coordination_access,
    require_team_membership,
)
from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus
from bicameral_cloud.models.pairing_authorization import (
    PairingAuthorizationAppraisalError,
    PairingAuthorizationV1,
    appraise_alpha_tofu,
    appraise_pairing_authorization,
    derived_identity_key_id,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser, TeamMembership, TeamRole
from bicameral_cloud.store import Store, get_store

router = APIRouter(prefix="/api/v1/teams", tags=["pairing"])


class CreatePairingRequest(BaseModel):
    """Request body for creating a daemon pairing."""

    pairing_id: str = Field(min_length=1)
    daemon_id: str
    daemon_version: str | None = None


class EnrollIdentityRequest(BaseModel):
    """Public half of the daemon identity key plus required enrollment grant."""

    identity_public_key_b64: str
    identity_key_id: str | None = None
    pairing_authorization: PairingAuthorizationV1


class PairingResponse(BaseModel):
    """Serialized daemon pairing record (no secrets or workspace paths)."""

    pairing_id: str
    team_id: str
    daemon_id: str
    status: str
    daemon_version: str | None
    created_at: str


class PairingListResponse(BaseModel):
    """List of pairing records for a team."""

    team_id: str
    pairings: list[PairingResponse]


def _pairing_to_response(pairing: DaemonPairing) -> PairingResponse:
    return PairingResponse(
        pairing_id=pairing.pairing_id,
        team_id=pairing.team_id,
        daemon_id=pairing.daemon_id,
        status=pairing.status.value,
        daemon_version=pairing.daemon_version,
        created_at=pairing.created_at.isoformat(),
    )


@router.post("/{team_id}/pairings", status_code=201)
async def create_pairing(
    team_id: str,
    body: CreatePairingRequest,
    membership: TeamMembership = Depends(require_team_coordination_access),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> PairingResponse:
    """Create a new daemon pairing for the team.

    Any allowlisted-domain eligible user may create their own pairing.
    ``team_owner`` is not required. The daemon-issued opaque ``pairing_id`` is
    carried unchanged into Cloud; Cloud never mints or substitutes relationship
    identity. An exact retry by the same caller is idempotent, while a collision
    with different tenancy or daemon identity fails closed.
    """
    pairing_id = body.pairing_id.strip()
    if not pairing_id:
        raise HTTPException(status_code=422, detail="pairing_id must be non-empty")

    existing = store.get_pairing(pairing_id)
    if existing is not None:
        if (
            existing.team_id == team_id
            and existing.daemon_id == body.daemon_id
            and existing.created_by_user_id == membership.user_id
        ):
            return _pairing_to_response(existing)
        raise HTTPException(
            status_code=409,
            detail={
                "kind": "PairingIdConflict",
                "message": "pairing_id is already bound to a different pairing",
            },
        )

    pairing = DaemonPairing(
        pairing_id=pairing_id,
        team_id=team_id,
        daemon_id=body.daemon_id,
        status=PairingStatus.ACTIVE,
        daemon_version=body.daemon_version,
        created_by_user_id=membership.user_id,
    )
    store.add_pairing(pairing)
    emit(
        store,
        action=AuditAction.PAIRING_CREATED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={"pairing_id": pairing.pairing_id, "daemon_id": body.daemon_id},
    )
    return _pairing_to_response(pairing)


@router.post("/{team_id}/pairings/{pairing_id}/identity")
async def enroll_daemon_identity(
    team_id: str,
    pairing_id: str,
    body: EnrollIdentityRequest,
    user: AuthenticatedUser = Depends(get_current_user),  # noqa: B008
    membership: TeamMembership = Depends(require_team_coordination_access),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> PairingResponse:
    """Enroll this daemon's identity public key for the authenticated caller.

    Factory #901 row 12 / cloud#302/#305: coordination metadata only (ADR-0006).
    Private keys never persist. Join-ticket keys are not reused.

    Requires a verified PairingAuthorizationV1 (not HostedSession alone).
    Domain-eligible callers may enroll their own pairing; ``team_owner`` is
    not required. First enrollment is the pairing creator only. A later
    update is only the enrolled account. ``identity_key_id`` is derived as
    ``sha256:<hex>`` of the raw 32-byte Ed25519 public key. Alpha TOFU is
    scoped by authenticated account + team + daemon_instance_id and forbids
    identity-key rotation.
    """
    pairing = store.get_pairing(pairing_id)
    if pairing is None or pairing.team_id != team_id:
        raise HTTPException(status_code=404, detail="Pairing not found")
    if pairing.status != PairingStatus.ACTIVE:
        raise HTTPException(status_code=409, detail="Pairing is not active")
    if not _caller_may_enroll(store, pairing, user.user_id):
        raise HTTPException(
            status_code=403,
            detail="Daemon identity enrollment is bound to the pairing session owner",
        )
    try:
        public_key = base64.b64decode(body.identity_public_key_b64, validate=True)
    except (ValueError, binascii.Error) as exc:
        raise HTTPException(
            status_code=422, detail="identity_public_key_b64 must be 32-byte Ed25519"
        ) from exc
    if len(public_key) != 32:
        raise HTTPException(
            status_code=422, detail="identity_public_key_b64 must be 32-byte Ed25519"
        )
    derived_key_id = derived_identity_key_id(public_key)
    authorization = body.pairing_authorization
    try:
        digest = appraise_pairing_authorization(
            authorization,
            pairing_id=pairing_id,
            team_id=team_id,
            cloud_account_id=user.user_id,
            request_public_key_b64=body.identity_public_key_b64,
            request_identity_key_id=body.identity_key_id,
        )
        appraise_alpha_tofu(
            authorization,
            established_issuer_key_id=store.get_daemon_tofu_root(
                cloud_account_id=user.user_id,
                team_id=team_id,
                daemon_instance_id=authorization.daemon_instance_id,
            ),
        )
    except PairingAuthorizationAppraisalError as exc:
        raise HTTPException(status_code=exc.status_code, detail=exc.detail) from exc
    if not store.consume_pairing_authorization(
        pairing_id=pairing_id,
        nonce=authorization.nonce,
        digest_hex=digest.hex(),
    ):
        raise HTTPException(
            status_code=409, detail="pairing_authorization already consumed"
        )
    enrolled = store.enroll_daemon_identity(
        pairing_id,
        user_id=user.user_id,
        identity_key_id=derived_key_id,
        identity_public_key_b64=body.identity_public_key_b64,
    )
    if enrolled is None:
        raise HTTPException(status_code=409, detail="Pairing is not active")
    store.establish_daemon_tofu_root(
        cloud_account_id=user.user_id,
        team_id=team_id,
        daemon_instance_id=authorization.daemon_instance_id,
        issuer_key_id=authorization.issuer_key_id,
    )
    emit(
        store,
        action=AuditAction.PAIRING_IDENTITY_ENROLLED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={
            "pairing_id": pairing_id,
            "identity_key_id": derived_key_id,
            "daemon_instance_id": authorization.daemon_instance_id,
        },
    )
    return _pairing_to_response(enrolled)


def _caller_may_enroll(store: Store, pairing: DaemonPairing, user_id: str) -> bool:
    """Creator-bound first enroll; no cross-account rebind.

    A Cloud ``HostedSession`` is not daemon possession (factory #901 row 12).
    First enrollment is only the pairing creator. Later updates only the
    enrolled Cloud account. ``store`` is unused; kept for call-site stability.
    """
    del store
    if pairing.enrolled_by_user_id:
        return pairing.enrolled_by_user_id == user_id
    return pairing.created_by_user_id == user_id


@router.get("/{team_id}/pairings")
async def list_pairings(
    team_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> PairingListResponse:
    """List all daemon pairings for the team."""
    pairings = store.list_pairings_for_team(team_id)
    return PairingListResponse(
        team_id=team_id,
        pairings=[_pairing_to_response(p) for p in pairings],
    )


@router.get("/{team_id}/pairings/{pairing_id}")
async def get_pairing(
    team_id: str,
    pairing_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> PairingResponse:
    """Get a specific pairing record."""
    pairing = store.get_pairing(pairing_id)
    if pairing is None or pairing.team_id != team_id:
        raise HTTPException(status_code=404, detail="Pairing not found")
    return _pairing_to_response(pairing)


@router.post("/{team_id}/pairings/{pairing_id}/revoke")
async def revoke_pairing(
    team_id: str,
    pairing_id: str,
    membership: TeamMembership = Depends(require_team_membership),  # noqa: B008
    store: Store = Depends(get_store),  # noqa: B008
) -> PairingResponse:
    """Revoke an existing daemon pairing.

    Only team owners may revoke pairings.
    """
    if membership.role != TeamRole.OWNER:
        raise HTTPException(
            status_code=403,
            detail="Only team owners may revoke daemon pairings",
        )
    pairing = store.get_pairing(pairing_id)
    if pairing is None or pairing.team_id != team_id:
        raise HTTPException(status_code=404, detail="Pairing not found")
    store.revoke_pairing(pairing_id)
    emit(
        store,
        action=AuditAction.PAIRING_REVOKED,
        actor_id=membership.user_id,
        team_id=team_id,
        detail={"pairing_id": pairing_id},
    )
    return _pairing_to_response(pairing)
