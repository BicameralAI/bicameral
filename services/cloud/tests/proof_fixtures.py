"""Shared ProductCreationCommitProof builders for tests (cloud#227)."""

from __future__ import annotations

import base64
import hashlib
import json
from datetime import UTC, datetime, timedelta
from typing import Any

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus
from bicameral_cloud.models.pairing_authorization import (
    PairingAuthorizationV1,
    pairing_authorization_digest_bytes,
)
from bicameral_cloud.models.product_directory import ProductCreationCommitProof

# Deterministic key so API tests can configure matching verification JSON.
PROOF_PRIVATE_KEY = Ed25519PrivateKey.from_private_bytes(bytes(range(1, 33)))
PROOF_PUBLIC_KEY = PROOF_PRIVATE_KEY.public_key().public_bytes(
    encoding=serialization.Encoding.Raw,
    format=serialization.PublicFormat.Raw,
)
PROOF_KEY_ID = f"sha256:{hashlib.sha256(PROOF_PUBLIC_KEY).hexdigest()}"
PROOF_VERIFICATION_KEYS_JSON = json.dumps(
    [
        {
            "key_id": PROOF_KEY_ID,
            "public_key_b64": base64.b64encode(PROOF_PUBLIC_KEY).decode(),
            "state": "active",
        }
    ]
)


def first_event_proof(
    product_id: str = "prod-1",
    *,
    substrate: str = "drive_folder",
    durability: str = "provider_confirmed_remote",
    event_id: str = "0198d2b4-8a5c-7f22-9e31-9bb014cc8200",
    event_digest: str | None = None,
    attempt_id: str = "create-attempt-1",
) -> ProductCreationCommitProof:
    """Mint a valid signed proof for the deterministic test key."""
    if event_digest is None:
        event_digest = f"sha256:{'2' * 64}"
    material: dict[str, Any] = {
        "schema_version": 2,
        "attempt_id": attempt_id,
        "product_id": product_id,
        "event_id": event_id,
        "substrate": substrate,
        "resource_fingerprint": f"sha256:{'1' * 64}",
        "provider_revision": "provider-revision-abc123",
        "event_digest": event_digest,
        "durability": durability,
        "confirmed_at": "2026-07-26T06:00:00Z",
        "signer_key_id": PROOF_KEY_ID,
        "signature_algorithm": "ed25519",
    }
    normalized = json.dumps(
        material, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    digest = hashlib.sha256(normalized).digest()
    return ProductCreationCommitProof(
        **material,
        proof_digest=f"sha256:{digest.hex()}",
        signature=base64.b64encode(PROOF_PRIVATE_KEY.sign(digest)).decode(),
    )


def v1_lab_digest_hex(proof: ProductCreationCommitProof) -> str:
    """v1 (kid/alg excluded) digest of the same facts. Must be rejected."""
    material = proof.model_dump(
        exclude={
            "proof_digest",
            "signature",
            "signer_key_id",
            "signature_algorithm",
        }
    )
    encoded = json.dumps(
        material, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    return hashlib.sha256(encoded).hexdigest()


def first_event_proof_dict(product_id: str = "prod-1", **kwargs: Any) -> dict[str, Any]:
    return first_event_proof(product_id, **kwargs).model_dump()


def enroll_identity_for_user(
    store: Any,
    user_id: str,
    *,
    pairing_id: str | None = None,
    team_id: str = "team-identity",
) -> None:
    """Bind the deterministic test proof key to this Cloud account."""
    existing = None
    for pairing in getattr(store, "list_pairings_for_team", lambda _tid: [])(team_id):
        if pairing.enrolled_by_user_id == user_id:
            existing = pairing
            break
    if existing is not None:
        store.enroll_daemon_identity(
            existing.pairing_id,
            user_id=user_id,
            identity_key_id=PROOF_KEY_ID,
            identity_public_key_b64=base64.b64encode(PROOF_PUBLIC_KEY).decode(),
        )
        return
    store.add_pairing(
        DaemonPairing(
            pairing_id=pairing_id or f"pairing-{user_id}",
            team_id=team_id,
            daemon_id="daemon-1",
            status=PairingStatus.ACTIVE,
            enrolled_by_user_id=user_id,
            identity_key_id=PROOF_KEY_ID,
            identity_public_key_b64=base64.b64encode(PROOF_PUBLIC_KEY).decode(),
        )
    )


def pairing_authorization_dict(
    *,
    pairing_id: str,
    team_id: str,
    cloud_account_id: str,
    private_key: Ed25519PrivateKey | None = None,
    public_key: bytes | None = None,
    identity_key_id: str | None = None,
    issuer_key_id: str | None = None,
    daemon_instance_id: str = "daemon-instance-1",
    nonce: str = "nonce-1",
    issued_at: datetime | None = None,
    ttl: timedelta = timedelta(minutes=5),
    protocol_session_id: str = "protocol-session-1",
    approved_origin: str = "https://dashboard.bicameral-ai.com",
    mutate: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Mint a signed PairingAuthorizationV1 for enrollment tests."""
    key = private_key or PROOF_PRIVATE_KEY
    pubkey = public_key or key.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    kid = identity_key_id or f"sha256:{hashlib.sha256(pubkey).hexdigest()}"
    issued = issued_at or datetime.now(UTC)
    expires = issued + ttl
    material: dict[str, Any] = {
        "schema_version": 1,
        "profile": "bicameral.pairing_authorization.v1",
        "pairing_id": pairing_id,
        "protocol_session_id": protocol_session_id,
        "cloud_account_id": cloud_account_id,
        "approved_origin": approved_origin,
        "daemon_instance_id": daemon_instance_id,
        "team_id": team_id,
        "identity_public_key_b64": base64.b64encode(pubkey).decode(),
        "identity_key_id": kid,
        "audience": "bicameral-cloud",
        "purpose": "identity_enrollment",
        "nonce": nonce,
        "issued_at": issued.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "expires_at": expires.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "issuer_key_id": issuer_key_id or kid,
        "signature_algorithm": "ed25519",
    }
    if mutate:
        material.update(mutate)
    digest = pairing_authorization_digest_bytes(material)
    material["signature_b64"] = base64.b64encode(key.sign(digest)).decode()
    # Validate shape for happy-path callers; mutation tests may skip.
    if mutate is None:
        PairingAuthorizationV1.model_validate(material)
    return material


def enroll_identity_body(
    *,
    pairing_id: str,
    team_id: str,
    cloud_account_id: str,
    **kwargs: Any,
) -> dict[str, Any]:
    """Request body with pubkey + required pairing_authorization."""
    auth = pairing_authorization_dict(
        pairing_id=pairing_id,
        team_id=team_id,
        cloud_account_id=cloud_account_id,
        **kwargs,
    )
    return {
        "identity_public_key_b64": auth["identity_public_key_b64"],
        "identity_key_id": auth["identity_key_id"],
        "pairing_authorization": auth,
    }
