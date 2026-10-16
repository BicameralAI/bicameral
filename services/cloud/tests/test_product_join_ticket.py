"""Security and bot-contract compatibility tests for cloud#126."""

from __future__ import annotations

import base64
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

from bicameral_cloud.app import app
from bicameral_cloud.config import CloudSettings
from bicameral_cloud.contracts.onboarding import load_schema
from bicameral_cloud.models.product_directory import (
    ProductDirectoryRecord,
    ProductPublication,
)
from tests.proof_fixtures import first_event_proof
from bicameral_cloud.models.product_join_ticket import (
    ProductJoinTicket,
    ProductJoinVerificationMetadata,
    SignedProductJoinTicket,
)
from bicameral_cloud.models.tenancy import AuthenticatedUser
from bicameral_cloud.product_directory_repository import (
    InMemoryProductDirectoryRepository,
    ProductDirectoryUnavailable,
    get_product_directory_repository,
)
from bicameral_cloud.product_join_ticket import (
    Ed25519ProductJoinTicketSigner,
    ProductJoinSignerError,
    ProductJoinSignerUnavailable,
    VerificationKey,
)
from bicameral_cloud.routes.product_join_ticket import (
    get_join_ticket_signer_dependency,
)
from bicameral_cloud.routes.projects import (
    DurableDirectoryProjectSource,
    get_project_discovery_source,
)

FIXTURE_DIR = Path(__file__).parent / "fixtures" / "onboarding"
TOKEN = "join-ticket-token"
PRODUCT_ID = "prd_01HZX8CHECKOUT"
ACCOUNT_ID = "acct_01HZX8DANA"
DOMAIN = "acme.example"
ISSUER = "https://cloud.bicameral.example"
ACTIVE_KEY_ID = "join-2026-07"
ACTIVE_SEED = bytes(range(1, 33))


def _signer(
    *,
    key_id: str = ACTIVE_KEY_ID,
    seed: bytes = ACTIVE_SEED,
    additional_keys: tuple[VerificationKey, ...] = (),
) -> Ed25519ProductJoinTicketSigner:
    return Ed25519ProductJoinTicketSigner(
        issuer=ISSUER,
        ticket_ttl_seconds=300,
        active_key_id=key_id,
        active_private_key=Ed25519PrivateKey.from_private_bytes(seed),
        additional_keys=additional_keys,
    )


@pytest.fixture()
def directory() -> InMemoryProductDirectoryRepository:
    repository = InMemoryProductDirectoryRepository()
    repository.publish(
        ProductPublication(
            first_event_commit_proof=first_event_proof(PRODUCT_ID),
            canonical_product_id=PRODUCT_ID,
            display_name="Checkout",
            eligible_domain=DOMAIN,
            creation_receipt="receipt-1",
            registration_ref="reg_opaque_9f2c",
            provider_hint="github",
            substrate_hint="drive_folder",
        )
    )
    return repository


@pytest.fixture()
def join_client(client, store, directory):
    store.add_user(
        TOKEN,
        AuthenticatedUser(
            user_id=ACCOUNT_ID,
            email=f"Dana@{DOMAIN.upper()}",
            email_verified=True,
        ),
    )
    app.dependency_overrides[get_product_directory_repository] = lambda: directory
    app.dependency_overrides[get_join_ticket_signer_dependency] = _signer
    yield client
    app.dependency_overrides.pop(get_product_directory_repository, None)
    app.dependency_overrides.pop(get_join_ticket_signer_dependency, None)


def _headers() -> dict[str, str]:
    return {"Authorization": f"Bearer {TOKEN}"}


def _ticket(*, issuer: str = ISSUER) -> ProductJoinTicket:
    issued_at = datetime(2026, 7, 12, 12, 0, tzinfo=UTC)
    return ProductJoinTicket(
        issuer=issuer,
        audience=f"daemon:{PRODUCT_ID}",
        product_id=PRODUCT_ID,
        cloud_account_id=ACCOUNT_ID,
        verified_domain=DOMAIN,
        issued_at=issued_at,
        expiry=issued_at + timedelta(minutes=5),
        nonce="jti_test",
        registration_ref="reg_opaque_9f2c",
    )


def _verify_with_metadata(
    envelope: SignedProductJoinTicket,
    metadata: ProductJoinVerificationMetadata,
) -> None:
    public_keys = {
        key.key_id: key.public_key_b64 for key in metadata.keys
    }
    encoded_key = public_keys.get(envelope.key_id)
    if encoded_key is None:
        raise ValueError("unknown or retired key")
    public_key = Ed25519PublicKey.from_public_bytes(
        base64.b64decode(encoded_key, validate=True)
    )
    public_key.verify(
        base64.b64decode(envelope.signature, validate=True),
        envelope.ticket.signing_bytes(),
    )


class TestProductJoinTicketIssuance:
    def test_eligible_verified_account_gets_daemon_verifiable_ticket(
        self, join_client, store
    ):
        response = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers=_headers(),
        )

        assert response.status_code == 200
        envelope = response.json()
        assert envelope["key_id"] == ACTIVE_KEY_ID
        ticket = envelope["ticket"]
        assert ticket == {
            "issuer": ISSUER,
            "audience": f"daemon:{PRODUCT_ID}",
            "product_id": PRODUCT_ID,
            "cloud_account_id": ACCOUNT_ID,
            "verified_domain": DOMAIN,
            "issued_at": ticket["issued_at"],
            "expiry": ticket["expiry"],
            "nonce": ticket["nonce"],
            "registration_ref": "reg_opaque_9f2c",
            "provider_hint": "github",
            "substrate_hint": "drive_folder",
        }
        assert ticket["nonce"].startswith("jti_")

        claims = ProductJoinTicket.model_validate(ticket)
        signature = base64.b64decode(envelope["signature"], validate=True)
        public_key = _signer().verification_metadata().keys[0].public_key_b64
        Ed25519PublicKey.from_public_bytes(
            base64.b64decode(public_key, validate=True)
        ).verify(signature, claims.signing_bytes())

        issued_at = datetime.fromisoformat(ticket["issued_at"].replace("Z", "+00:00"))
        expiry = datetime.fromisoformat(ticket["expiry"].replace("Z", "+00:00"))
        assert issued_at <= datetime.now(UTC)
        assert expiry - issued_at == timedelta(
            seconds=_signer().ticket_ttl_seconds
        )
        assert store.list_registration_requests_for_team("team-any") == []
        assert store.get_membership(ACCOUNT_ID, "team-any") is None

    def test_ticket_is_minimized_and_contains_no_hosted_or_local_secret(
        self, join_client
    ):
        payload = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers=_headers(),
        ).json()
        serialized = json.dumps(payload).lower()
        for forbidden in (
            "access_token",
            "refresh_token",
            "supabase",
            "credential",
            "private_key",
            "event_store",
            "local_path",
            "workspace_path",
            "membership",
            "role",
        ):
            assert forbidden not in serialized

    def test_vendored_operator_home_compatibility_path(self, join_client):
        response = join_client.post(
            f"/api/v1/products/{PRODUCT_ID}/join-ticket",
            json={"preview_id": "preview_non_authoritative"},
            headers=_headers(),
        )
        assert response.status_code == 200
        assert response.json()["ticket"]["product_id"] == PRODUCT_ID

    def test_discovery_exposes_only_noncanonical_join_preview(
        self, join_client, directory
    ):
        app.dependency_overrides[get_project_discovery_source] = lambda: (
            DurableDirectoryProjectSource(directory)
        )
        try:
            response = join_client.get("/api/v1/projects", headers=_headers())
        finally:
            app.dependency_overrides.pop(get_project_discovery_source, None)
        assert response.status_code == 200
        item = response.json()["projects"][0]
        assert item["is_member"] is False
        assert item["verified_domain"] == DOMAIN
        assert item["preview"] == {
            "preview_id": item["preview"]["preview_id"],
            "product_id": PRODUCT_ID,
            "product_display_name": "Checkout",
            "verified_domain": DOMAIN,
            "event_store_hint": {
                "substrate": "drive_folder",
                "provider": "github",
            },
            "canonical": False,
            "expires_at": item["preview"]["expires_at"],
        }
        serialized = json.dumps(item["preview"]).lower()
        for forbidden in ("token", "credential", "local_path", "membership"):
            assert forbidden not in serialized

    def test_nonce_is_unique_per_issuance(self, join_client):
        first = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers=_headers(),
        ).json()
        second = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers=_headers(),
        ).json()
        assert first["ticket"]["nonce"] != second["ticket"]["nonce"]

    def test_product_domain_is_normalized_before_eligibility_check(
        self, join_client
    ):
        record = ProductDirectoryRecord(
            canonical_product_id=PRODUCT_ID,
            display_name="Checkout",
            eligible_domain=" @ACME.EXAMPLE ",
            registration_ref="reg_opaque_9f2c",
        )
        app.dependency_overrides[get_product_directory_repository] = lambda: (
            StaticDirectory(record)
        )
        response = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers=_headers(),
        )
        assert response.status_code == 200
        assert response.json()["ticket"]["verified_domain"] == DOMAIN

    @pytest.mark.parametrize(
        ("headers", "status_code", "code"),
        [
            ({}, 401, "authentication_required"),
            (
                {"Authorization": "Bearer invalid"},
                401,
                "authentication_required",
            ),
        ],
    )
    def test_unauthenticated_is_typed_and_redacted(
        self, join_client, headers, status_code, code
    ):
        response = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers=headers,
        )
        assert response.status_code == status_code
        assert response.json()["detail"]["code"] == code
        assert "invalid" not in json.dumps(response.json()).lower()

    def test_unverified_email_is_rejected(self, join_client, store):
        store.add_user(
            "unverified-token",
            AuthenticatedUser(
                user_id="acct-unverified",
                email=f"person@{DOMAIN}",
                email_verified=False,
            ),
        )
        response = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers={"Authorization": "Bearer unverified-token"},
        )
        assert response.status_code == 403
        assert response.json()["detail"]["code"] == "email_unverified"

    def test_cross_domain_and_cross_product_tenant_are_rejected(
        self, join_client, store, directory
    ):
        directory.publish(
            ProductPublication(
                first_event_commit_proof=first_event_proof("prd_other_tenant"),
                canonical_product_id="prd_other_tenant",
                display_name="Other tenant",
                eligible_domain="other.example",
                creation_receipt="receipt-other",
                registration_ref="reg_other",
            )
        )
        response = join_client.post(
            "/api/v1/projects/prd_other_tenant/join-ticket",
            headers=_headers(),
        )
        assert response.status_code == 403
        assert response.json()["detail"] == {
            "code": "product_ineligible",
            "message": "The authenticated account is not eligible for this Product",
        }

        store.add_user(
            "other-domain-token",
            AuthenticatedUser(
                user_id="acct-other",
                email="other@other.example",
                email_verified=True,
            ),
        )
        response = join_client.post(
            f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
            headers={"Authorization": "Bearer other-domain-token"},
        )
        assert response.status_code == 403
        assert response.json()["detail"]["code"] == "product_ineligible"

    def test_missing_disabled_and_not_ready_products_are_typed(self, join_client):
        missing = join_client.post(
            "/api/v1/projects/prd_missing/join-ticket",
            headers=_headers(),
        )
        assert missing.status_code == 404
        assert missing.json()["detail"]["code"] == "product_not_found"

        app.dependency_overrides[get_product_directory_repository] = lambda: (
            StaticDirectory(
                ProductDirectoryRecord(
                    canonical_product_id="prd_disabled",
                    display_name="Disabled",
                    eligible_domain=DOMAIN,
                    registration_ref="reg_disabled",
                    enabled=False,
                )
            )
        )
        disabled = join_client.post(
            "/api/v1/projects/prd_disabled/join-ticket",
            headers=_headers(),
        )
        assert disabled.status_code == 409
        assert disabled.json()["detail"]["code"] == "product_disabled"

        app.dependency_overrides[get_product_directory_repository] = lambda: (
            StaticDirectory(
                ProductDirectoryRecord(
                    canonical_product_id="prd_not_ready",
                    display_name="Not ready",
                    eligible_domain=DOMAIN,
                )
            )
        )
        not_ready = join_client.post(
            "/api/v1/projects/prd_not_ready/join-ticket",
            headers=_headers(),
        )
        assert not_ready.status_code == 409
        assert not_ready.json()["detail"]["code"] == "product_not_joinable"


class UnavailableDirectory:
    def get_by_canonical_id(self, canonical_product_id: str):
        raise ProductDirectoryUnavailable("secret backend descriptor")


class StaticDirectory:
    def __init__(self, record: ProductDirectoryRecord) -> None:
        self._record = record

    def get_by_canonical_id(
        self, canonical_product_id: str
    ) -> ProductDirectoryRecord | None:
        if self._record.canonical_product_id == canonical_product_id:
            return self._record
        return None


class ExplodingSigner(Ed25519ProductJoinTicketSigner):
    def sign(self, ticket: ProductJoinTicket):
        raise ProductJoinSignerError("private key leaked if surfaced")


def test_backend_and_signer_errors_are_redacted(join_client, caplog):
    app.dependency_overrides[get_product_directory_repository] = UnavailableDirectory
    response = join_client.post(
        f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
        headers=_headers(),
    )
    assert response.status_code == 503
    assert response.json()["detail"]["code"] == "product_directory_unavailable"
    assert "secret backend descriptor" not in response.text

    app.dependency_overrides[get_product_directory_repository] = lambda: (
        InMemoryProductDirectoryRepository()
    )
    directory = InMemoryProductDirectoryRepository()
    directory.publish(
        ProductPublication(
            first_event_commit_proof=first_event_proof(PRODUCT_ID),
            canonical_product_id=PRODUCT_ID,
            display_name="Checkout",
            eligible_domain=DOMAIN,
            creation_receipt="receipt",
            registration_ref="reg",
        )
    )
    app.dependency_overrides[get_product_directory_repository] = lambda: directory
    def _exploding_signer() -> ExplodingSigner:
        return ExplodingSigner(
            issuer=ISSUER,
            ticket_ttl_seconds=300,
            active_key_id=ACTIVE_KEY_ID,
            active_private_key=Ed25519PrivateKey.from_private_bytes(ACTIVE_SEED),
        )

    app.dependency_overrides[get_join_ticket_signer_dependency] = (
        _exploding_signer
    )
    response = join_client.post(
        f"/api/v1/projects/{PRODUCT_ID}/join-ticket",
        headers=_headers(),
    )
    assert response.status_code == 503
    assert response.json()["detail"]["code"] == "ticket_signer_unavailable"
    assert "private key" not in response.text.lower()
    assert "private key" not in caplog.text.lower()


def test_key_rotation_overlap_then_retirement_fails_closed():
    old_seed = bytes(range(33, 65))
    old_private = Ed25519PrivateKey.from_private_bytes(old_seed)
    old_public = old_private.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    overlapping = _signer(
        additional_keys=(
            VerificationKey(
                key_id="join-2026-06",
                public_key=old_public,
                state="overlap",
            ),
        )
    )
    overlap_ids = {
        key.key_id for key in overlapping.verification_metadata().keys
    }
    assert overlap_ids == {ACTIVE_KEY_ID, "join-2026-06"}
    old_signer = _signer(key_id="join-2026-06", seed=old_seed)
    old_envelope = old_signer.sign(_ticket())
    _verify_with_metadata(old_envelope, overlapping.verification_metadata())

    retired = _signer(
        additional_keys=(
            VerificationKey(
                key_id="join-2026-06",
                public_key=old_public,
                state="retired",
            ),
        )
    )
    retired_ids = {key.key_id for key in retired.verification_metadata().keys}
    assert retired_ids == {ACTIVE_KEY_ID}
    with pytest.raises(ValueError, match="unknown or retired"):
        _verify_with_metadata(old_envelope, retired.verification_metadata())

    unknown_envelope = old_envelope.model_copy(update={"key_id": "unknown-key"})
    with pytest.raises(ValueError, match="unknown or retired"):
        _verify_with_metadata(
            unknown_envelope,
            overlapping.verification_metadata(),
        )


def test_signer_configuration_fails_closed():
    settings = CloudSettings(
        supabase_url=None,
        supabase_jwt_secret=None,
        supabase_anon_key=None,
        supabase_service_key=None,
        product_join_ticket_issuer=ISSUER,
        product_join_ticket_ttl_seconds=301,
        product_join_signing_key_id=ACTIVE_KEY_ID,
        product_join_signing_private_key_b64=base64.b64encode(
            ACTIVE_SEED
        ).decode(),
    )
    with pytest.raises(ProductJoinSignerUnavailable):
        Ed25519ProductJoinTicketSigner.from_settings(settings)


def test_bot_shared_fixture_canonical_bytes_and_signature_are_compatible():
    fixture = json.loads(
        (FIXTURE_DIR / "signed-product-join-ticket.valid.json").read_text()
    )
    schema = load_schema("product-join-ticket.schema.json")
    ticket = ProductJoinTicket.model_validate(fixture["ticket"])
    assert ticket.signing_bytes().decode() == fixture["canonical_signing_json"]

    public_key = base64.b64decode(fixture["public_key_b64"], validate=True)
    signature = base64.b64decode(fixture["signature"], validate=True)
    Ed25519PublicKey.from_public_bytes(public_key).verify(
        signature, ticket.signing_bytes()
    )
    # Bind to the pinned, fingerprint-verified bot schema — no shadow schema.
    assert schema["additionalProperties"] is False
    assert set(fixture["ticket"]) == set(schema["properties"])
    assert set(schema["required"]).issubset(fixture["ticket"])


def test_verification_metadata_endpoint_publishes_no_private_material(join_client):
    response = join_client.get("/api/v1/product-join/verification-keys")
    assert response.status_code == 200
    payload = response.json()
    assert payload["version"] == 1
    assert payload["issuer"] == ISSUER
    assert payload["ticket_ttl_seconds"] == 300
    assert payload["keys"][0]["key_id"] == ACTIVE_KEY_ID
    assert payload["keys"][0]["state"] == "active"
    assert "private" not in json.dumps(payload).lower()


def test_vendored_operator_home_uses_per_product_ticket_not_legacy_join():
    assets = (
        Path(__file__).parents[1]
        / "src"
        / "bicameral_cloud"
        / "static"
        / "dashboard"
        / "assets"
    )
    javascript = "\n".join(
        path.read_text() for path in assets.glob("*.js")
    )
    assert "join-ticket" in javascript
    assert "join-requests" not in javascript
    assert "join-project-form" not in javascript
