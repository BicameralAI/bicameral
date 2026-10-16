"""Ed25519 Product join-ticket signing and rotation support (cloud#126)."""

from __future__ import annotations

import base64
import binascii
import json
from dataclasses import dataclass
from typing import Protocol

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
)

from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.models.product_join_ticket import (
    ProductJoinTicket,
    ProductJoinVerificationKey,
    ProductJoinVerificationMetadata,
    SignedProductJoinTicket,
)


class ProductJoinSignerError(RuntimeError):
    """Base error for fail-closed signer configuration or operation failures."""


class ProductJoinSignerUnavailable(ProductJoinSignerError):
    """Raised when no safe active signing configuration is available."""


class ProductJoinTicketSigner(Protocol):
    """Issuer boundary used by the route and deterministic tests."""

    @property
    def issuer(self) -> str:
        ...

    @property
    def ticket_ttl_seconds(self) -> int:
        ...

    def sign(self, ticket: ProductJoinTicket) -> SignedProductJoinTicket:
        ...

    def verification_metadata(self) -> ProductJoinVerificationMetadata:
        ...


@dataclass(frozen=True)
class VerificationKey:
    key_id: str
    public_key: bytes
    state: str


class Ed25519ProductJoinTicketSigner:
    """Dedicated active key plus overlap keys for bounded rotation."""

    def __init__(
        self,
        *,
        issuer: str,
        ticket_ttl_seconds: int,
        active_key_id: str,
        active_private_key: Ed25519PrivateKey,
        additional_keys: tuple[VerificationKey, ...] = (),
    ) -> None:
        if not issuer.strip():
            raise ProductJoinSignerUnavailable("ticket issuer is not configured")
        if not 1 <= ticket_ttl_seconds <= 300:
            raise ProductJoinSignerUnavailable(
                "ticket TTL must be between 1 and 300 seconds"
            )
        if not active_key_id.strip():
            raise ProductJoinSignerUnavailable("active key ID is not configured")
        all_key_ids = (active_key_id, *(key.key_id for key in additional_keys))
        if any(not key_id.strip() for key_id in all_key_ids):
            raise ProductJoinSignerUnavailable("ticket key ID is invalid")
        if len(set(all_key_ids)) != len(all_key_ids):
            raise ProductJoinSignerUnavailable("duplicate ticket key ID")
        if any(key.state not in {"overlap", "retired"} for key in additional_keys):
            raise ProductJoinSignerUnavailable("invalid ticket key state")
        if any(len(key.public_key) != 32 for key in additional_keys):
            raise ProductJoinSignerUnavailable("invalid ticket public key")

        self._issuer = issuer
        self._ticket_ttl_seconds = ticket_ttl_seconds
        self._active_key_id = active_key_id
        self._active_private_key = active_private_key
        self._additional_keys = additional_keys

    @property
    def issuer(self) -> str:
        return self._issuer

    @property
    def ticket_ttl_seconds(self) -> int:
        return self._ticket_ttl_seconds

    @property
    def _active_public_key(self) -> bytes:
        return self._active_private_key.public_key().public_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PublicFormat.Raw,
        )

    def sign(self, ticket: ProductJoinTicket) -> SignedProductJoinTicket:
        if ticket.issuer != self._issuer:
            raise ProductJoinSignerError("ticket issuer mismatch")
        signature = self._active_private_key.sign(ticket.signing_bytes())
        return SignedProductJoinTicket(
            ticket=ticket,
            key_id=self._active_key_id,
            signature=base64.b64encode(signature).decode(),
        )

    def verification_metadata(self) -> ProductJoinVerificationMetadata:
        keys = [
            ProductJoinVerificationKey(
                key_id=self._active_key_id,
                public_key_b64=base64.b64encode(self._active_public_key).decode(),
                state="active",
            )
        ]
        keys.extend(
            ProductJoinVerificationKey(
                key_id=key.key_id,
                public_key_b64=base64.b64encode(key.public_key).decode(),
                state=key.state,
            )
            for key in self._additional_keys
            if key.state == "overlap"
        )
        return ProductJoinVerificationMetadata(
            issuer=self._issuer,
            ticket_ttl_seconds=self._ticket_ttl_seconds,
            keys=keys,
        )

    @classmethod
    def from_settings(
        cls, settings: CloudSettings
    ) -> Ed25519ProductJoinTicketSigner:
        issuer = settings.product_join_ticket_issuer
        key_id = settings.product_join_signing_key_id
        private_key_b64 = settings.product_join_signing_private_key_b64
        if not issuer or not key_id or not private_key_b64:
            raise ProductJoinSignerUnavailable(
                "Product join-ticket signing is not configured"
            )
        try:
            private_key_raw = base64.b64decode(private_key_b64, validate=True)
            private_key = Ed25519PrivateKey.from_private_bytes(private_key_raw)
            additional_keys = _load_additional_keys(
                settings.product_join_verification_keys_json
            )
        except (ValueError, TypeError, binascii.Error, json.JSONDecodeError) as exc:
            raise ProductJoinSignerUnavailable(
                "Product join-ticket signing configuration is invalid"
            ) from exc
        return cls(
            issuer=issuer,
            ticket_ttl_seconds=settings.product_join_ticket_ttl_seconds,
            active_key_id=key_id,
            active_private_key=private_key,
            additional_keys=additional_keys,
        )


def _load_additional_keys(raw_json: str | None) -> tuple[VerificationKey, ...]:
    if not raw_json:
        return ()
    payload: object = json.loads(raw_json)
    if not isinstance(payload, list):
        raise ValueError("verification keys must be a list")
    result: list[VerificationKey] = []
    for item in payload:
        if not isinstance(item, dict):
            raise ValueError("verification key must be an object")
        key_id = item.get("key_id")
        public_key_b64 = item.get("public_key_b64")
        state = item.get("state")
        if (
            not isinstance(key_id, str)
            or not isinstance(public_key_b64, str)
            or not isinstance(state, str)
        ):
            raise ValueError("verification key fields are invalid")
        public_key = base64.b64decode(public_key_b64, validate=True)
        if len(public_key) != 32:
            raise ValueError("Ed25519 public keys must be 32 bytes")
        result.append(
            VerificationKey(
                key_id=key_id,
                public_key=public_key,
                state=state,
            )
        )
    return tuple(result)


def get_product_join_ticket_signer() -> ProductJoinTicketSigner:
    """Resolve the dedicated signer; malformed or absent config fails closed."""

    return Ed25519ProductJoinTicketSigner.from_settings(get_settings())
