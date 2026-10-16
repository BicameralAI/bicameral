"""PairingAuthorizationV1 — daemon-minted enrollment grant (cloud#305).

Distinct from HostedSession / DaemonPairing coordination. Short TTL + one-shot
consume. Signature binds compact sorted JSON of every field except signature_b64.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import re
from datetime import UTC, datetime, timedelta
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, field_validator, model_validator

PROFILE = "bicameral.pairing_authorization.v1"
AUDIENCE = "bicameral-cloud"
PURPOSE = "identity_enrollment"
SIGNATURE_ALGORITHM = "ed25519"
MAX_TTL = timedelta(minutes=10)
MAX_ISSUED_AT_FUTURE = timedelta(minutes=2)

_ISO_Z = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z")


def _require_iso_z(value: str, field_name: str) -> str:
    normalized = value.strip()
    if not _ISO_Z.fullmatch(normalized):
        raise ValueError(f"{field_name} must be ISO-8601 with Z")
    try:
        datetime.fromisoformat(normalized.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValueError(f"{field_name} must be a valid ISO-8601 instant") from exc
    return normalized


def _require_text(value: str, field_name: str) -> str:
    normalized = value.strip()
    if not normalized:
        raise ValueError(f"{field_name} must not be empty")
    return normalized


def pairing_authorization_digest_bytes(payload: dict[str, Any]) -> bytes:
    """SHA-256 of compact sorted JSON excluding signature_b64."""
    material = {k: v for k, v in payload.items() if k != "signature_b64"}
    encoded = json.dumps(
        material,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode()
    return hashlib.sha256(encoded).digest()


def derived_identity_key_id(public_key: bytes) -> str:
    return f"sha256:{hashlib.sha256(public_key).hexdigest()}"


class PairingAuthorizationV1(BaseModel):
    """Local-minimum enrollment authorization minted at daemon local approve."""

    model_config = ConfigDict(extra="forbid")

    schema_version: Literal[1]
    profile: Literal["bicameral.pairing_authorization.v1"]
    pairing_id: str
    protocol_session_id: str
    cloud_account_id: str
    approved_origin: str
    daemon_instance_id: str
    team_id: str
    identity_public_key_b64: str
    identity_key_id: str
    audience: Literal["bicameral-cloud"]
    purpose: Literal["identity_enrollment"]
    nonce: str
    issued_at: str
    expires_at: str
    issuer_key_id: str
    signature_algorithm: Literal["ed25519"]
    signature_b64: str

    @field_validator(
        "pairing_id",
        "protocol_session_id",
        "cloud_account_id",
        "approved_origin",
        "daemon_instance_id",
        "team_id",
        "identity_key_id",
        "nonce",
        "issuer_key_id",
    )
    @classmethod
    def _non_empty(cls, value: str) -> str:
        return _require_text(value, "field")

    @field_validator("issued_at", "expires_at")
    @classmethod
    def _timestamps(cls, value: str) -> str:
        return _require_iso_z(value, "timestamp")

    @field_validator("identity_public_key_b64")
    @classmethod
    def _ed25519_pubkey(cls, value: str) -> str:
        try:
            decoded = base64.b64decode(value, validate=True)
        except (ValueError, binascii.Error) as exc:
            raise ValueError("identity_public_key_b64 must be 32-byte Ed25519") from exc
        if len(decoded) != 32:
            raise ValueError("identity_public_key_b64 must be 32-byte Ed25519")
        return value

    @field_validator("signature_b64")
    @classmethod
    def _ed25519_signature(cls, value: str) -> str:
        try:
            decoded = base64.b64decode(value, validate=True)
        except (ValueError, binascii.Error) as exc:
            raise ValueError("signature_b64 must be canonical base64") from exc
        if len(decoded) != 64:
            raise ValueError("Ed25519 signature must be 64 bytes")
        return value

    @model_validator(mode="after")
    def _key_id_matches_pubkey(self) -> PairingAuthorizationV1:
        public_key = base64.b64decode(self.identity_public_key_b64, validate=True)
        expected = derived_identity_key_id(public_key)
        if self.identity_key_id != expected:
            raise ValueError(
                "identity_key_id must equal sha256 of the raw Ed25519 public key"
            )
        return self

    def digest_bytes(self) -> bytes:
        return pairing_authorization_digest_bytes(self.model_dump())

    def digest_hex(self) -> str:
        return self.digest_bytes().hex()


class PairingAuthorizationAppraisalError(Exception):
    """Fail-closed appraisal rejection with HTTP status hint."""

    def __init__(self, status_code: int, detail: str) -> None:
        super().__init__(detail)
        self.status_code = status_code
        self.detail = detail


def parse_instant(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(UTC)


def appraise_pairing_authorization(
    authorization: PairingAuthorizationV1,
    *,
    pairing_id: str,
    team_id: str,
    cloud_account_id: str,
    request_public_key_b64: str,
    request_identity_key_id: str | None,
    now: datetime | None = None,
) -> bytes:
    """Verify schema-bound wire fields, freshness, and Ed25519 signature.

    Returns digest bytes on success. Does not perform one-shot consume or TOFU.
    """
    if authorization.profile != PROFILE or authorization.schema_version != 1:
        raise PairingAuthorizationAppraisalError(
            422, "pairing_authorization schema/profile rejected"
        )
    if authorization.audience != AUDIENCE or authorization.purpose != PURPOSE:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization audience/purpose rejected"
        )
    if authorization.signature_algorithm != SIGNATURE_ALGORITHM:
        raise PairingAuthorizationAppraisalError(
            422, "pairing_authorization signature_algorithm rejected"
        )
    if authorization.pairing_id != pairing_id:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization pairing_id does not match path"
        )
    if authorization.team_id != team_id:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization team_id does not match path"
        )
    if authorization.cloud_account_id != cloud_account_id:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization cloud_account_id does not match caller"
        )
    if authorization.identity_public_key_b64 != request_public_key_b64:
        raise PairingAuthorizationAppraisalError(
            422, "identity_public_key_b64 must match pairing_authorization"
        )
    public_key = base64.b64decode(authorization.identity_public_key_b64, validate=True)
    derived = derived_identity_key_id(public_key)
    if authorization.identity_key_id != derived:
        raise PairingAuthorizationAppraisalError(
            422, "identity_key_id must equal sha256 of the raw Ed25519 public key"
        )
    supplied = (request_identity_key_id or "").strip()
    if supplied and supplied != derived:
        raise PairingAuthorizationAppraisalError(
            422, "identity_key_id must equal sha256 of the raw Ed25519 public key"
        )

    instant = now.astimezone(UTC) if now is not None else datetime.now(UTC)
    issued_at = parse_instant(authorization.issued_at)
    expires_at = parse_instant(authorization.expires_at)
    if expires_at <= issued_at:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization expires_at must be after issued_at"
        )
    if expires_at - issued_at > MAX_TTL:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization TTL exceeds maximum"
        )
    if issued_at > instant + MAX_ISSUED_AT_FUTURE:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization issued_at is in the far future"
        )
    if instant >= expires_at:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization expired"
        )

    digest = authorization.digest_bytes()
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

        signature = base64.b64decode(authorization.signature_b64, validate=True)
        Ed25519PublicKey.from_public_bytes(public_key).verify(signature, digest)
    except (InvalidSignature, ValueError, binascii.Error) as exc:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization signature invalid"
        ) from exc
    return digest


def appraise_alpha_tofu(
    authorization: PairingAuthorizationV1,
    *,
    established_issuer_key_id: str | None,
) -> None:
    """Alpha TOFU: first enroll is self-issued; later grants forbid key rotation.

    Continuity is cryptographic, not string-only: signature appraisal already
    verifies under ``identity_public_key_b64``, and ``identity_key_id`` is the
    derived kid of that key. After a root is established, subsequent grants must
    use the same signing identity (``identity_key_id == established root``) and
    the same issuer id. Claiming an old ``issuer_key_id`` while signing with a
    new key therefore fails closed.
    """
    if established_issuer_key_id is None:
        if authorization.issuer_key_id != authorization.identity_key_id:
            raise PairingAuthorizationAppraisalError(
                403,
                "Alpha TOFU requires issuer_key_id == identity_key_id on first trust",
            )
        return
    if authorization.issuer_key_id != established_issuer_key_id:
        raise PairingAuthorizationAppraisalError(
            403, "pairing_authorization issuer does not match established TOFU root"
        )
    # Forbid rotation: signing identity must remain the established root.
    if authorization.identity_key_id != established_issuer_key_id:
        raise PairingAuthorizationAppraisalError(
            403,
            "pairing_authorization identity does not match established TOFU root "
            "(key rotation forbidden)",
        )
