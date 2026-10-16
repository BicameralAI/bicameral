"""Ticket verifier + creation-publication creator-binding tests (cloud#136)."""

from __future__ import annotations

import base64
import copy
from datetime import datetime

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from bicameral_cloud.contracts.onboarding import load_fixture
from bicameral_cloud.models.onboarding import (
    JoinFailureCode,
    ProductCreationPublication,
    ProductCreationResult,
)
from bicameral_cloud.models.product_join_ticket import (
    ProductJoinTicket,
    SignedProductJoinTicket,
)
from bicameral_cloud.onboarding_verification import (
    CreationPublicationError,
    InMemoryReplayGuard,
    TicketVerificationError,
    publication_identity,
    verify_creation_publication,
    verify_join_ticket,
)

KEY_ID = "join-2026-07"
SEED = bytes(range(1, 33))
ISSUER = "https://cloud.bicameral.example"
PRODUCT_ID = "prd_01HZX8CHECKOUT"
ACCOUNT_ID = "acct_01HZX8DANA"
DOMAIN = "acme.example"
NOW = datetime.fromisoformat("2026-07-12T09:01:00+00:00")


def _private_key() -> Ed25519PrivateKey:
    return Ed25519PrivateKey.from_private_bytes(SEED)


def _public_bytes() -> bytes:
    return _private_key().public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )


def _sign(ticket: ProductJoinTicket) -> SignedProductJoinTicket:
    signature = _private_key().sign(ticket.signing_bytes())
    return SignedProductJoinTicket(
        ticket=ticket,
        key_id=KEY_ID,
        signature=base64.b64encode(signature).decode(),
    )


def _ticket(**overrides: object) -> ProductJoinTicket:
    data = copy.deepcopy(load_fixture("product-join-ticket.valid.json"))
    data.update(overrides)
    return ProductJoinTicket.model_validate(data)


def _verify(signed: SignedProductJoinTicket, **kwargs: object) -> None:
    params: dict[str, object] = {
        "expected_issuer": ISSUER,
        "expected_product_id": PRODUCT_ID,
        "expected_account_id": ACCOUNT_ID,
        "expected_domain": DOMAIN,
        "trusted_keys": {KEY_ID: _public_bytes()},
        "now": NOW,
        "replay_guard": InMemoryReplayGuard(),
    }
    params.update(kwargs)
    verify_join_ticket(signed, **params)  # type: ignore[arg-type]


def test_valid_ticket_verifies() -> None:
    _verify(_sign(_ticket()))


def test_unknown_key_is_invalid_signature() -> None:
    signed = _sign(_ticket())
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed, trusted_keys={})
    assert exc.value.code is JoinFailureCode.INVALID_SIGNATURE


def test_tampered_signature_is_invalid() -> None:
    signed = _sign(_ticket())
    other = Ed25519PrivateKey.from_private_bytes(bytes(range(2, 34)))
    bad = other.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed, trusted_keys={KEY_ID: bad})
    assert exc.value.code is JoinFailureCode.INVALID_SIGNATURE


def test_untrusted_issuer_rejected() -> None:
    signed = _sign(_ticket(issuer="https://evil.example"))
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed)
    assert exc.value.code is JoinFailureCode.INVALID_SIGNATURE


def test_not_yet_valid() -> None:
    signed = _sign(_ticket())
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed, now=datetime.fromisoformat("2026-07-12T08:59:00+00:00"))
    assert exc.value.code is JoinFailureCode.NOT_YET_VALID


def test_expired_ticket() -> None:
    signed = _sign(_ticket())
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed, now=datetime.fromisoformat("2026-07-12T09:06:00+00:00"))
    assert exc.value.code is JoinFailureCode.EXPIRED_TICKET


def test_wrong_audience() -> None:
    signed = _sign(_ticket(audience="daemon:prd_other"))
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed)
    assert exc.value.code is JoinFailureCode.WRONG_AUDIENCE


def test_wrong_product() -> None:
    signed = _sign(_ticket(product_id="prd_other", audience="daemon:prd_other"))
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed, expected_audience="daemon:prd_other")
    assert exc.value.code is JoinFailureCode.WRONG_PRODUCT


def test_wrong_account() -> None:
    signed = _sign(_ticket(cloud_account_id="acct_other"))
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed)
    assert exc.value.code is JoinFailureCode.WRONG_ACCOUNT


def test_wrong_domain() -> None:
    signed = _sign(_ticket(verified_domain="evil.example"))
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed)
    assert exc.value.code is JoinFailureCode.WRONG_DOMAIN


def test_missing_registration_ref() -> None:
    signed = _sign(_ticket(registration_ref="   "))
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed)
    assert exc.value.code is JoinFailureCode.MISSING_PAIRING


def test_replayed_nonce_rejected() -> None:
    guard = InMemoryReplayGuard()
    signed = _sign(_ticket())
    _verify(signed, replay_guard=guard)
    with pytest.raises(TicketVerificationError) as exc:
        _verify(signed, replay_guard=guard)
    assert exc.value.code is JoinFailureCode.REPLAYED_TICKET


# --- Creation publication creator binding + idempotency -------------------- #


def _creation_result() -> ProductCreationResult:
    return ProductCreationResult.model_validate(
        load_fixture("product-creation-result.valid.json")
    )


def _publication(name: str) -> ProductCreationPublication:
    return ProductCreationPublication.model_validate(load_fixture(name))


def test_publication_creator_binding_verifies() -> None:
    result = _creation_result()
    pub = _publication("product-creation-publication.valid.json")
    identity = verify_creation_publication(
        pub, result, authenticated_account_id=pub.creator_cloud_account_id
    )
    assert identity.product_id == result.product_id


def test_publication_rejects_non_creator_account() -> None:
    result = _creation_result()
    pub = _publication("product-creation-publication.valid.json")
    with pytest.raises(CreationPublicationError):
        verify_creation_publication(
            pub, result, authenticated_account_id="acct_someone_else"
        )


def test_publication_rejects_creator_binding_drift() -> None:
    result = _creation_result()
    pub = _publication("product-creation-publication.valid.json")
    tampered = result.model_copy(update={"creator_cloud_account_id": "acct_zzz"})
    with pytest.raises(CreationPublicationError):
        verify_creation_publication(
            pub, tampered, authenticated_account_id=pub.creator_cloud_account_id
        )


def test_retry_is_idempotent_upsert_not_a_second_product() -> None:
    result = _creation_result()
    first = _publication("product-creation-publication.valid.json")
    retry = _publication("product-creation-publication.retry.json")
    account = first.creator_cloud_account_id

    id_first = verify_creation_publication(
        first, result, authenticated_account_id=account
    )
    id_retry = verify_creation_publication(
        retry, result, authenticated_account_id=account
    )
    # A lost response never implies a second Product/row: same identity.
    assert id_first == id_retry
    assert first.publication_id != retry.publication_id
    assert publication_identity(first) == publication_identity(retry)
