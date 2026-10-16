"""Fail-closed verification for the Bot onboarding contract (cloud#136).

Two boundary checks that mirror the daemon's authority without duplicating it:

* :func:`verify_join_ticket` — a symmetric verifier for the Cloud-signed
  ``ProductJoinTicket``. It enforces issuer, audience, product, account,
  verified domain, validity window, the ``nonce``/JTI single-use guard, and the
  registration reference, mapping each failure to the daemon's typed
  ``ProductJoinResult`` failure code. Bounded-claim enforcement itself is the
  model's ``extra="forbid"`` shape; this adds semantic enforcement.
* :func:`verify_creation_publication` — requires the authenticated Cloud
  account to match the daemon-signed creator binding from the
  ``ProductCreationResult`` and models retried publication as an idempotent
  upsert keyed by ``(product_id, idempotency_key)`` so a lost response can never
  imply a second Product or row.

Neither check makes Cloud canonical for Product identity, roles, membership, or
policy; the daemon remains authoritative (ADR-0005/0006).
"""

from __future__ import annotations

import base64
from dataclasses import dataclass
from datetime import datetime
from typing import Protocol

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from bicameral_cloud.models.onboarding import (
    JoinFailureCode,
    ProductCreationPublication,
    ProductCreationResult,
)
from bicameral_cloud.models.product_join_ticket import SignedProductJoinTicket
from bicameral_cloud.models.registration import assert_no_cloud_leakage


class TicketVerificationError(RuntimeError):
    """Fail-closed ticket rejection carrying the daemon-typed failure code."""

    def __init__(self, code: JoinFailureCode, message: str) -> None:
        super().__init__(message)
        self.code = code


class CreationPublicationError(RuntimeError):
    """Fail-closed rejection of a creation publication that fails its binding."""


class ReplayGuard(Protocol):
    """Single-use guard over ticket ``nonce``/JTI values."""

    def register(self, jti: str) -> bool:
        """Record ``jti``; return ``False`` if it was already consumed."""
        ...


class InMemoryReplayGuard:
    """Deterministic, process-local JTI guard (contract-evidence scope)."""

    def __init__(self) -> None:
        self._seen: set[str] = set()

    def register(self, jti: str) -> bool:
        if jti in self._seen:
            return False
        self._seen.add(jti)
        return True


def verify_join_ticket(
    signed: SignedProductJoinTicket,
    *,
    expected_issuer: str,
    expected_product_id: str,
    expected_account_id: str,
    expected_domain: str,
    trusted_keys: dict[str, bytes],
    now: datetime,
    replay_guard: ReplayGuard,
    expected_audience: str | None = None,
) -> None:
    """Verify a signed ticket fail-closed; raise :class:`TicketVerificationError`.

    ``expected_audience`` defaults to ``daemon:<product_id>``.
    """
    ticket = signed.ticket
    audience = expected_audience or f"daemon:{expected_product_id}"

    # 1. Trusted key + Ed25519 signature (untrusted issuer collapses here too).
    public_key_bytes = trusted_keys.get(signed.key_id)
    if public_key_bytes is None:
        raise TicketVerificationError(
            JoinFailureCode.INVALID_SIGNATURE, "unknown signing key id"
        )
    try:
        signature = base64.b64decode(signed.signature, validate=True)
        Ed25519PublicKey.from_public_bytes(public_key_bytes).verify(
            signature, ticket.signing_bytes()
        )
    except (InvalidSignature, ValueError) as exc:
        raise TicketVerificationError(
            JoinFailureCode.INVALID_SIGNATURE, "ticket signature is not valid"
        ) from exc

    if ticket.issuer != expected_issuer:
        raise TicketVerificationError(
            JoinFailureCode.INVALID_SIGNATURE, "ticket issuer is not trusted"
        )

    # 2. Validity window (issued_at <= now < expiry).
    if now < ticket.issued_at:
        raise TicketVerificationError(
            JoinFailureCode.NOT_YET_VALID, "ticket is not yet valid"
        )
    if now >= ticket.expiry:
        raise TicketVerificationError(
            JoinFailureCode.EXPIRED_TICKET, "ticket has expired"
        )

    # 3. Bound claims.
    if ticket.audience != audience:
        raise TicketVerificationError(
            JoinFailureCode.WRONG_AUDIENCE, "ticket audience mismatch"
        )
    if ticket.product_id != expected_product_id:
        raise TicketVerificationError(
            JoinFailureCode.WRONG_PRODUCT, "ticket product mismatch"
        )
    if ticket.cloud_account_id != expected_account_id:
        raise TicketVerificationError(
            JoinFailureCode.WRONG_ACCOUNT, "ticket account mismatch"
        )
    if ticket.verified_domain != expected_domain:
        raise TicketVerificationError(
            JoinFailureCode.WRONG_DOMAIN, "ticket verified-domain mismatch"
        )
    if not ticket.registration_ref.strip():
        raise TicketVerificationError(
            JoinFailureCode.MISSING_PAIRING, "ticket registration reference is absent"
        )
    if not ticket.nonce.strip():
        raise TicketVerificationError(
            JoinFailureCode.REPLAYED_TICKET, "ticket nonce/JTI is absent"
        )

    # 4. Single-use nonce/JTI guard (consumed only after everything else holds).
    if not replay_guard.register(ticket.nonce):
        raise TicketVerificationError(
            JoinFailureCode.REPLAYED_TICKET, "ticket nonce/JTI already consumed"
        )


@dataclass(frozen=True)
class PublicationIdentity:
    """Stable idempotency identity of a Product creation publication."""

    product_id: str
    idempotency_key: str


def publication_identity(
    publication: ProductCreationPublication,
) -> PublicationIdentity:
    """Return the ``(product_id, idempotency_key)`` upsert identity."""
    return PublicationIdentity(
        product_id=publication.product_id,
        idempotency_key=publication.idempotency_key,
    )


def verify_creation_publication(
    publication: ProductCreationPublication,
    creation_result: ProductCreationResult,
    *,
    authenticated_account_id: str,
) -> PublicationIdentity:
    """Verify creator binding and return the idempotent upsert identity.

    Raises :class:`CreationPublicationError` unless the authenticated account
    matches both the publication's declared creator and the daemon-signed
    creator binding, the publication is derived from the same canonical Product
    and creation receipt, and it stays explicitly non-canonical. Retrying the
    same identity is an idempotent upsert; it never implies a second Product.
    """
    if publication.creator_cloud_account_id != authenticated_account_id:
        raise CreationPublicationError(
            "authenticated account does not match the publication creator"
        )
    if publication.creator_cloud_account_id != creation_result.creator_cloud_account_id:
        raise CreationPublicationError(
            "publication creator does not match the daemon-signed creator binding"
        )
    if publication.product_id != creation_result.product_id:
        raise CreationPublicationError(
            "publication product_id does not match the daemon-signed Product"
        )
    if publication.canonical_creation_receipt_ref != creation_result.receipt_id:
        raise CreationPublicationError(
            "publication does not reference the daemon canonical creation receipt"
        )
    if publication.canonical is not False:
        raise CreationPublicationError(
            "a published directory row must be explicitly non-canonical"
        )

    # Bot-facing payload must stay opaque: no secrets, remotes, or local paths.
    assert_no_cloud_leakage(publication.idempotency_key, field="idempotency_key")
    assert_no_cloud_leakage(
        publication.canonical_creation_receipt_ref,
        field="canonical_creation_receipt_ref",
    )

    return publication_identity(publication)


__all__ = [
    "CreationPublicationError",
    "InMemoryReplayGuard",
    "PublicationIdentity",
    "ReplayGuard",
    "TicketVerificationError",
    "publication_identity",
    "verify_creation_publication",
    "verify_join_ticket",
]
