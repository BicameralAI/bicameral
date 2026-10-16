"""Pairing-bound daemon identity enrollment (factory #901 row 12 / cloud#302/#305)."""

from __future__ import annotations

import base64
from datetime import UTC, datetime, timedelta

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from fastapi.testclient import TestClient

from bicameral_cloud.models.pairing import HostedSession, PairingStatus, SessionStatus
from bicameral_cloud.models.product_directory import ProductCreationCommitProof
from bicameral_cloud.models.tenancy import Team
from tests.conftest import (
    OTHER_USER_ID,
    OUTSIDER_USER_ID,
    PAIRING_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
    USER_ID,
)
from tests.proof_fixtures import (
    PROOF_KEY_ID,
    PROOF_PRIVATE_KEY,
    PROOF_PUBLIC_KEY,
    enroll_identity_body,
    first_event_proof,
    first_event_proof_dict,
    pairing_authorization_dict,
    v1_lab_digest_hex,
)


def _enroll(
    client: TestClient,
    *,
    token: str = TOKEN_MEMBER,
    cloud_account_id: str = USER_ID,
    **kwargs,
):
    body = enroll_identity_body(
        pairing_id=PAIRING_ID,
        team_id=TEAM_ID,
        cloud_account_id=cloud_account_id,
        **kwargs,
    )
    return client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {token}"},
        json=body,
    )


def test_enroll_identity_then_publish_succeeds(client: TestClient, seeded_store) -> None:
    enroll = _enroll(client)
    assert enroll.status_code == 200, enroll.text
    pairing = seeded_store.get_pairing(PAIRING_ID)
    assert pairing is not None
    assert pairing.enrolled_by_user_id == USER_ID
    assert pairing.identity_key_id == PROOF_KEY_ID

    body = {
        "canonical_product_id": "prod-enroll-1",
        "display_name": "Checkout",
        "eligible_domain": "example.com",
        "creation_receipt": "receipt-abc",
        "first_event_commit_proof": first_event_proof_dict("prod-enroll-1"),
    }
    published = client.post(
        "/api/v1/products",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json=body,
    )
    assert published.status_code == 201, published.text


def test_publish_without_enrollment_is_unavailable(
    client: TestClient, seeded_store
) -> None:
    body = {
        "canonical_product_id": "prod-no-enroll",
        "display_name": "Checkout",
        "eligible_domain": "example.com",
        "creation_receipt": "receipt-abc",
        "first_event_commit_proof": first_event_proof_dict("prod-no-enroll"),
    }
    published = client.post(
        "/api/v1/products",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json=body,
    )
    assert published.status_code == 503


def test_global_json_key_does_not_authorize_unenrolled_caller(
    client: TestClient, seeded_store
) -> None:
    """Leftover PRODUCT_CREATION_PROOF_VERIFICATION_KEYS_JSON is not enough."""
    assert seeded_store.get_pairing(PAIRING_ID) is not None
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None
    body = {
        "canonical_product_id": "prod-json-key",
        "display_name": "Checkout",
        "eligible_domain": "example.com",
        "creation_receipt": "receipt-abc",
        "first_event_commit_proof": first_event_proof_dict("prod-json-key"),
    }
    published = client.post(
        "/api/v1/products",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json=body,
    )
    assert published.status_code == 503


def test_revoked_pairing_key_stops_verifying(client: TestClient, seeded_store) -> None:
    assert _enroll(client).status_code == 200
    seeded_store.revoke_pairing(PAIRING_ID)
    assert seeded_store.get_pairing(PAIRING_ID).status == PairingStatus.REVOKED
    published = client.post(
        "/api/v1/products",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={
            "canonical_product_id": "prod-revoked",
            "display_name": "Checkout",
            "eligible_domain": "example.com",
            "creation_receipt": "receipt-abc",
            "first_event_commit_proof": first_event_proof_dict("prod-revoked"),
        },
    )
    assert published.status_code == 503


def test_v2_model_rejects_v1_digest() -> None:
    proof = first_event_proof("prod-v1")
    dumped = proof.model_dump()
    dumped["proof_digest"] = f"sha256:{v1_lab_digest_hex(proof)}"
    try:
        ProductCreationCommitProof.model_validate(dumped)
        raised = False
    except Exception:
        raised = True
    assert raised, "v1 digest of the same facts must not validate as v2"


def test_domain_eligible_user_can_enroll_own_pairing(
    client: TestClient, seeded_store
) -> None:
    """Domain eligibility is enough to enroll a pairing the caller created."""
    seeded_store.add_team(
        Team(
            team_id=TEAM_ID,
            display_name="Alpha",
            created_by=OTHER_USER_ID,
            domain="example.com",
        )
    )
    pairing_id = "pairing-outsider"
    created = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        json={"pairing_id": pairing_id, "daemon_id": "daemon-outsider"},
    )
    assert created.status_code == 201, created.text
    assert created.json()["pairing_id"] == pairing_id
    enroll = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{pairing_id}/identity",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        json=enroll_identity_body(
            pairing_id=pairing_id,
            team_id=TEAM_ID,
            cloud_account_id=OUTSIDER_USER_ID,
            nonce="nonce-domain-eligible",
            daemon_instance_id="daemon-instance-outsider",
        ),
    )
    assert enroll.status_code == 200, enroll.text
    pairing = seeded_store.get_pairing(pairing_id)
    assert pairing is not None
    assert pairing.enrolled_by_user_id == OUTSIDER_USER_ID


def test_member_cannot_capture_first_enrollment_via_cloud_session(
    client: TestClient, seeded_store
) -> None:
    """Cloud HostedSession is not possession. Only the pairing creator may first-enroll."""
    pairing = seeded_store.get_pairing(PAIRING_ID)
    assert pairing is not None
    pairing.created_by_user_id = OTHER_USER_ID
    capture = _enroll(client)
    assert capture.status_code == 403, capture.text
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None
    assert seeded_store.get_pairing(PAIRING_ID).enrolled_by_user_id is None


def test_member_cannot_rebind_another_accounts_enrollment(
    client: TestClient, seeded_store
) -> None:
    first = _enroll(client)
    assert first.status_code == 200, first.text
    assert seeded_store.get_pairing(PAIRING_ID).enrolled_by_user_id == USER_ID

    seeded_store.add_session(
        HostedSession(
            session_id="session-owner-hijack",
            user_id=OTHER_USER_ID,
            team_id=TEAM_ID,
            pairing_id=PAIRING_ID,
            status=SessionStatus.OPEN,
        )
    )
    hijack = _enroll(
        client,
        token=TOKEN_OWNER,
        cloud_account_id=OTHER_USER_ID,
        nonce="nonce-hijack",
        daemon_instance_id="daemon-instance-hijack",
    )
    assert hijack.status_code == 403, hijack.text
    assert seeded_store.get_pairing(PAIRING_ID).enrolled_by_user_id == USER_ID
    assert OTHER_USER_ID != USER_ID


def test_identity_key_id_must_match_public_key_fingerprint(
    client: TestClient, seeded_store
) -> None:
    public_b64 = base64.b64encode(PROOF_PUBLIC_KEY).decode()
    auth = pairing_authorization_dict(
        pairing_id=PAIRING_ID,
        team_id=TEAM_ID,
        cloud_account_id=USER_ID,
    )
    mismatch = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={
            "identity_key_id": "sha256:" + ("e" * 64),
            "identity_public_key_b64": public_b64,
            "pairing_authorization": auth,
        },
    )
    assert mismatch.status_code == 422, mismatch.text
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None


def test_enrollment_is_process_local(seeded_store) -> None:
    """ADR-0008: a second Store instance does not see this process's enrollment."""
    from bicameral_cloud.store import Store

    seeded_store.enroll_daemon_identity(
        PAIRING_ID,
        user_id=USER_ID,
        identity_key_id=PROOF_KEY_ID,
        identity_public_key_b64=base64.b64encode(PROOF_PUBLIC_KEY).decode(),
    )
    other = Store()
    assert other.get_pairing(PAIRING_ID) is None
    assert other.identity_keys_for_user(USER_ID) == []


def test_missing_pairing_authorization_is_422(client: TestClient, seeded_store) -> None:
    public_b64 = base64.b64encode(PROOF_PUBLIC_KEY).decode()
    response = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={"identity_public_key_b64": public_b64},
    )
    assert response.status_code == 422, response.text
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None


def test_hosted_session_alone_still_insufficient(
    client: TestClient, seeded_store
) -> None:
    """Open HostedSession + pubkey without authorization must not enroll."""
    assert seeded_store.get_session("session-1") is not None
    public_b64 = base64.b64encode(PROOF_PUBLIC_KEY).decode()
    response = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={
            "identity_key_id": PROOF_KEY_ID,
            "identity_public_key_b64": public_b64,
        },
    )
    assert response.status_code == 422, response.text
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None


def test_bad_signature_is_rejected(client: TestClient, seeded_store) -> None:
    body = enroll_identity_body(
        pairing_id=PAIRING_ID,
        team_id=TEAM_ID,
        cloud_account_id=USER_ID,
    )
    # Flip a signature byte after minting.
    sig = bytearray(base64.b64decode(body["pairing_authorization"]["signature_b64"]))
    sig[0] ^= 0x01
    body["pairing_authorization"]["signature_b64"] = base64.b64encode(bytes(sig)).decode()
    response = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json=body,
    )
    assert response.status_code in {403, 422}, response.text
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None


def test_expired_authorization_is_403(client: TestClient, seeded_store) -> None:
    issued = datetime.now(UTC) - timedelta(minutes=20)
    response = _enroll(
        client,
        issued_at=issued,
        ttl=timedelta(minutes=5),
        nonce="nonce-expired",
    )
    assert response.status_code == 403, response.text
    assert "expired" in response.text.lower()
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None


def test_double_consume_is_409(client: TestClient, seeded_store) -> None:
    body = enroll_identity_body(
        pairing_id=PAIRING_ID,
        team_id=TEAM_ID,
        cloud_account_id=USER_ID,
        nonce="nonce-once",
    )
    first = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json=body,
    )
    assert first.status_code == 200, first.text
    # Clear enrollment so creator-bound path would otherwise allow a retry;
    # consume must still reject the same authorization.
    pairing = seeded_store.get_pairing(PAIRING_ID)
    assert pairing is not None
    pairing.enrolled_by_user_id = None
    pairing.identity_key_id = None
    pairing.identity_public_key_b64 = None
    second = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/identity",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json=body,
    )
    assert second.status_code == 409, second.text


def test_happy_path_enroll_with_valid_authorization(
    client: TestClient, seeded_store
) -> None:
    response = _enroll(client, nonce="nonce-happy")
    assert response.status_code == 200, response.text
    pairing = seeded_store.get_pairing(PAIRING_ID)
    assert pairing is not None
    assert pairing.identity_key_id == PROOF_KEY_ID
    assert pairing.enrolled_by_user_id == USER_ID
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=USER_ID,
            team_id=TEAM_ID,
            daemon_instance_id="daemon-instance-1",
        )
        == PROOF_KEY_ID
    )


def test_tofu_mismatch_on_second_key_without_prior_trust(
    client: TestClient, seeded_store
) -> None:
    """Without an established TOFU root, issuer_key_id must equal identity_key_id."""
    other = Ed25519PrivateKey.from_private_bytes(bytes(range(2, 34)))
    response = _enroll(
        client,
        private_key=other,
        issuer_key_id=PROOF_KEY_ID,  # claims proof key issued a different identity
        nonce="nonce-tofu-mismatch",
        daemon_instance_id="daemon-instance-untrusted",
    )
    assert response.status_code == 403, response.text
    assert "tofu" in response.text.lower() or "issuer" in response.text.lower()
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id is None
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=USER_ID,
            team_id=TEAM_ID,
            daemon_instance_id="daemon-instance-untrusted",
        )
        is None
    )
    # Self-issued first enroll still works for that instance.
    ok = _enroll(
        client,
        private_key=other,
        nonce="nonce-tofu-ok",
        daemon_instance_id="daemon-instance-untrusted",
    )
    assert ok.status_code == 200, ok.text


def test_tofu_rejects_new_key_claiming_established_issuer(
    client: TestClient, seeded_store
) -> None:
    """Later grant signed by a different key cannot claim the established root id."""
    first = _enroll(client, nonce="nonce-tofu-root", daemon_instance_id="daemon-tofu-rot")
    assert first.status_code == 200, first.text
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=USER_ID,
            team_id=TEAM_ID,
            daemon_instance_id="daemon-tofu-rot",
        )
        == PROOF_KEY_ID
    )

    other = Ed25519PrivateKey.from_private_bytes(bytes(range(3, 35)))
    # Self-sign under a new key while asserting the known established issuer id.
    spoof = _enroll(
        client,
        private_key=other,
        issuer_key_id=PROOF_KEY_ID,
        nonce="nonce-tofu-spoof",
        daemon_instance_id="daemon-tofu-rot",
    )
    assert spoof.status_code == 403, spoof.text
    detail = spoof.text.lower()
    assert "tofu" in detail or "issuer" in detail or "rotation" in detail
    # Established root and enrolled identity remain the original key.
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=USER_ID,
            team_id=TEAM_ID,
            daemon_instance_id="daemon-tofu-rot",
        )
        == PROOF_KEY_ID
    )
    assert seeded_store.get_pairing(PAIRING_ID).identity_key_id == PROOF_KEY_ID


def test_tofu_roots_do_not_collide_across_tenants(
    client: TestClient, seeded_store
) -> None:
    """Same wire daemon_instance_id must not share TOFU roots across tenancy."""
    from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus
    from bicameral_cloud.models.tenancy import TeamMembership, TeamRole
    from tests.conftest import OTHER_TEAM_ID, OTHER_USER_ID, TOKEN_OWNER

    shared_daemon = "daemon-instance-shared"
    first = _enroll(
        client,
        nonce="nonce-tenant-a",
        daemon_instance_id=shared_daemon,
    )
    assert first.status_code == 200, first.text
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=USER_ID,
            team_id=TEAM_ID,
            daemon_instance_id=shared_daemon,
        )
        == PROOF_KEY_ID
    )

    other_pairing_id = "pairing-tenant-b"
    seeded_store.add_membership(
        TeamMembership(
            user_id=OTHER_USER_ID, team_id=OTHER_TEAM_ID, role=TeamRole.OWNER
        )
    )
    seeded_store.add_pairing(
        DaemonPairing(
            pairing_id=other_pairing_id,
            team_id=OTHER_TEAM_ID,
            daemon_id="daemon-tenant-b",
            status=PairingStatus.ACTIVE,
            created_by_user_id=OTHER_USER_ID,
        )
    )
    other_key = Ed25519PrivateKey.from_private_bytes(bytes(range(4, 36)))
    other_body = enroll_identity_body(
        pairing_id=other_pairing_id,
        team_id=OTHER_TEAM_ID,
        cloud_account_id=OTHER_USER_ID,
        private_key=other_key,
        nonce="nonce-tenant-b",
        daemon_instance_id=shared_daemon,
    )
    other_enroll = client.post(
        f"/api/v1/teams/{OTHER_TEAM_ID}/pairings/{other_pairing_id}/identity",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json=other_body,
    )
    assert other_enroll.status_code == 200, other_enroll.text
    other_kid = other_body["identity_key_id"]
    assert other_kid != PROOF_KEY_ID
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=OTHER_USER_ID,
            team_id=OTHER_TEAM_ID,
            daemon_instance_id=shared_daemon,
        )
        == other_kid
    )
    # Tenant A's root is unchanged and remains distinct.
    assert (
        seeded_store.get_daemon_tofu_root(
            cloud_account_id=USER_ID,
            team_id=TEAM_ID,
            daemon_instance_id=shared_daemon,
        )
        == PROOF_KEY_ID
    )
