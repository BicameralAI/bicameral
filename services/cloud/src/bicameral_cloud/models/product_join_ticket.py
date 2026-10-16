"""Bounded Cloud-signed Product join ticket wire models (cloud#126)."""

from __future__ import annotations

import base64
import json
from datetime import datetime
from enum import StrEnum

from pydantic import BaseModel, ConfigDict, field_serializer


class ProviderHint(StrEnum):
    GITHUB = "github"
    GOOGLE = "google"


class SubstrateHint(StrEnum):
    GIT = "git"
    DRIVE_FOLDER = "drive_folder"


class ProductJoinTicket(BaseModel):
    """Minimum daemon-verifiable claims; extra claims fail closed."""

    model_config = ConfigDict(extra="forbid")

    issuer: str
    audience: str
    product_id: str
    cloud_account_id: str
    verified_domain: str
    issued_at: datetime
    expiry: datetime
    nonce: str
    registration_ref: str
    provider_hint: ProviderHint | None = None
    substrate_hint: SubstrateHint | None = None

    @field_serializer("issued_at", "expiry", when_used="json")
    def _serialize_time(self, value: datetime) -> str:
        return value.isoformat().replace("+00:00", "Z")

    def signing_bytes(self) -> bytes:
        """Match the bot verifier's sorted, compact canonical JSON."""

        payload = self.model_dump(mode="json", exclude_none=True)
        return json.dumps(
            payload,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        ).encode()


class SignedProductJoinTicket(BaseModel):
    """Ticket claims plus the detached Ed25519 signature expected by the bot."""

    ticket: ProductJoinTicket
    key_id: str
    signature: str


class ProductJoinVerificationKey(BaseModel):
    """Published public key metadata suitable for daemon trust pinning."""

    key_id: str
    algorithm: str = "Ed25519"
    public_key_b64: str
    state: str

    @property
    def jwks_x(self) -> str:
        raw = base64.b64decode(self.public_key_b64, validate=True)
        return base64.urlsafe_b64encode(raw).decode().rstrip("=")


class ProductJoinVerificationMetadata(BaseModel):
    """Versioned trust metadata; retired keys are deliberately absent."""

    version: int = 1
    issuer: str
    ticket_ttl_seconds: int
    keys: list[ProductJoinVerificationKey]
