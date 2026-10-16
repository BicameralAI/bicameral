"""Deterministic two-user hosted onboarding cross-service journey (cloud#127).

Proves the alpha hosted-onboarding authority boundary end to end without live
Supabase, Figma, or deployed credentials:

- Cloud authenticates, projects a safe Product directory row, and issues a
  short-lived signed Product join ticket.
- The paired *local daemon* (``FakeProductDaemon``) stays authoritative for
  canonical Product creation, membership, roles, canonical events, and replay.
- The browser/Cloud only ever see redacted projections.

Owner path:  verified-domain sign-in -> daemon creates the canonical Product
(creator becomes Policy Owner *and* Product Owner) -> Cloud publishes the safe
directory row -> the Cloud app is recreated and the Product is still
discoverable.

Peer path:   a second verified same-domain account discovers the Product as
available -> requests a signed ticket -> submits it through the paired daemon ->
becomes Product Member *only* -> resumes provider access / ProjectRegistration
without minting a second membership event -> replay proves Product Member only
-> Cloud reflects membership and it survives a restart.

Wrong-domain:  a third identity on a *different* allowlisted domain cannot
discover the Product and is refused a ticket, with no cross-tenant data exposed.

End-to-end redaction: no Supabase access token reaches the daemon; no provider
secret or local filesystem path reaches Cloud/browser/logs.
"""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from fastapi.testclient import TestClient

from bicameral_cloud.app import app
from tests.proof_fixtures import (
    enroll_identity_for_user,
    first_event_proof_dict,
    PROOF_VERIFICATION_KEYS_JSON,
)
from bicameral_cloud.models.discovery import WorkspaceDiscoveryItem, WorkspaceReadiness
from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus
from bicameral_cloud.models.tenancy import AuthenticatedUser, Team
from bicameral_cloud.product_join_ticket import Ed25519ProductJoinTicketSigner
from bicameral_cloud.routes.product_join_ticket import (
    get_join_ticket_signer_dependency,
)
from bicameral_cloud.store import Store
from tests.conformance.fake_product_daemon import (
    POLICY_OWNER,
    PRODUCT_MEMBER,
    PRODUCT_OWNER,
    FakeProductDaemon,
)

ISSUER = "https://cloud.bicameral.example"
ACTIVE_KEY_ID = "join-2026-07"
ACTIVE_SEED = bytes(range(1, 33))

OWNER_DOMAIN = "acme.example"
OTHER_DOMAIN = "rival.example"

OWNER_TOKEN = "tok-owner"
OWNER_ID = "acct_owner"
PEER_TOKEN = "tok-peer"
PEER_ID = "acct_peer"
WRONG_TOKEN = "tok-wrong"
WRONG_ID = "acct_wrong"

PEER_DOMAIN_TEAM = "team-acme-domain"

# Redaction denylist: none of these may appear in any Cloud/browser-visible
# payload, ticket, join result, projection, or captured artifact.
FORBIDDEN_SUBSTRINGS = (
    "provider_secret",
    "api_key",
    "refresh_token",
    "jwt_secret",
    "service_key",
    "supabase_service_key",
    "source_body",
    "source_content",
    "events.jsonl",
    "graph_index",
    ".bicameral/",
    "/home/",
    "local_path",
    "filesystem_path",
    "workspace_path",
    "ghp_",
)


def _signer() -> Ed25519ProductJoinTicketSigner:
    return Ed25519ProductJoinTicketSigner(
        issuer=ISSUER,
        ticket_ttl_seconds=300,
        active_key_id=ACTIVE_KEY_ID,
        active_private_key=Ed25519PrivateKey.from_private_bytes(ACTIVE_SEED),
    )


def _daemon() -> FakeProductDaemon:
    public_key = (
        Ed25519PrivateKey.from_private_bytes(ACTIVE_SEED)
        .public_key()
        .public_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PublicFormat.Raw,
        )
    )
    return FakeProductDaemon(
        verification_keys={ACTIVE_KEY_ID: public_key}, issuer=ISSUER
    )


@pytest.fixture()
def onboarding(store: Store):
    """Seed three verified identities and pin the Cloud ticket signer."""

    store.add_user(
        OWNER_TOKEN,
        AuthenticatedUser(
            user_id=OWNER_ID, email=f"owner@{OWNER_DOMAIN}", email_verified=True
        ),
    )
    store.add_user(
        PEER_TOKEN,
        AuthenticatedUser(
            user_id=PEER_ID, email=f"peer@{OWNER_DOMAIN}", email_verified=True
        ),
    )
    store.add_user(
        WRONG_TOKEN,
        AuthenticatedUser(
            user_id=WRONG_ID, email=f"intruder@{OTHER_DOMAIN}", email_verified=True
        ),
    )
    # Domain-scoped team so the peer is discovery-eligible; membership rows are
    # only projected once the daemon confirms the join (see below).
    store.add_team(
        Team(
            team_id=PEER_DOMAIN_TEAM,
            display_name="Acme",
            created_by="seed",
            domain=OWNER_DOMAIN,
        )
    )
    enroll_identity_for_user(store, OWNER_ID, team_id=PEER_DOMAIN_TEAM)
    app.dependency_overrides[get_join_ticket_signer_dependency] = _signer
    try:
        yield store
    finally:
        app.dependency_overrides.pop(get_join_ticket_signer_dependency, None)


def _auth(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def _discovered_ids(client: TestClient, token: str) -> list[str]:
    response = client.get("/api/v1/projects", headers=_auth(token))
    assert response.status_code == 200, response.text
    return [item["project_id"] for item in response.json()["projects"]]


def _item(client: TestClient, token: str, product_id: str) -> dict | None:
    response = client.get("/api/v1/projects", headers=_auth(token))
    assert response.status_code == 200, response.text
    for item in response.json()["projects"]:
        if item["project_id"] == product_id:
            return item
    return None


def test_two_user_hosted_onboarding_journey(onboarding: Store) -> None:
    store = onboarding
    daemon = _daemon()
    captured: list[str] = []  # every Cloud/browser-visible payload, for redaction

    # --- Owner: daemon creates canonical Product; creator is dual-owner --------
    creation = daemon.create_product(
        creator_actor_ref=OWNER_ID,
        display_name="Checkout",
        verified_domain=OWNER_DOMAIN,
    )
    owner_roles = daemon.replay_roles(creation.product_id)[OWNER_ID]
    assert owner_roles == sorted([POLICY_OWNER, PRODUCT_OWNER])

    client = TestClient(app)

    # --- Owner: publish the safe directory row through Cloud -------------------
    publish = client.post(
        "/api/v1/products",
        headers=_auth(OWNER_TOKEN),
        json={
            "canonical_product_id": creation.product_id,
            "display_name": creation.display_name,
            "eligible_domain": OWNER_DOMAIN,
            "creation_receipt": creation.creation_receipt,
            "registration_ref": creation.registration_ref,
            "provider_hint": creation.provider_hint,
            "substrate_hint": creation.substrate_hint,
            "first_event_commit_proof": first_event_proof_dict(
                creation.product_id,
                substrate=creation.substrate_hint or "drive_folder",
            ),
        },
    )
    assert publish.status_code == 201, publish.text
    captured.append(publish.text)

    assert creation.product_id in _discovered_ids(client, OWNER_TOKEN)

    # --- Recreate the Cloud app; Product remains discoverable (durability) -----
    client.close()
    client = TestClient(app)
    assert creation.product_id in _discovered_ids(client, OWNER_TOKEN)

    # --- Peer: discovers the Product as available, not a member ----------------
    peer_view = _item(client, PEER_TOKEN, creation.product_id)
    assert peer_view is not None
    assert peer_view["is_member"] is False
    assert peer_view["verified_domain"] == OWNER_DOMAIN
    captured.append(json.dumps(peer_view))

    # --- Peer: request a signed Cloud join ticket ------------------------------
    ticket_resp = client.post(
        f"/api/v1/products/{creation.product_id}/join-ticket",
        headers=_auth(PEER_TOKEN),
    )
    assert ticket_resp.status_code == 200, ticket_resp.text
    signed_ticket = ticket_resp.json()
    captured.append(ticket_resp.text)
    claims = signed_ticket["ticket"]
    assert claims["product_id"] == creation.product_id
    assert claims["cloud_account_id"] == PEER_ID
    assert claims["verified_domain"] == OWNER_DOMAIN
    # The narrow ticket never carries the Supabase access token.
    assert PEER_TOKEN not in ticket_resp.text

    # --- Peer: submit the ticket through the paired daemon ---------------------
    result = daemon.product_join(
        signed_ticket=signed_ticket, local_actor_ref=PEER_ID
    )
    captured.append(json.dumps(result))
    assert result["outcome"] == "granted"
    assert result["granted_role"] == PRODUCT_MEMBER
    assert result["membership_event_ref"]

    # --- Replay proves Product Member ONLY for the peer ------------------------
    replay = daemon.replay_roles(creation.product_id)
    assert replay[PEER_ID] == [PRODUCT_MEMBER]
    assert POLICY_OWNER not in replay[PEER_ID]
    assert PRODUCT_OWNER not in replay[PEER_ID]
    assert replay[OWNER_ID] == sorted([POLICY_OWNER, PRODUCT_OWNER])

    # --- Interrupted/resumed setup does not mint a second membership event -----
    daemon.continue_provider_access(
        product_id=creation.product_id, local_actor_ref=PEER_ID
    )
    daemon.continue_provider_access(
        product_id=creation.product_id, local_actor_ref=PEER_ID
    )
    assert daemon.membership_event_count(creation.product_id, PEER_ID) == 1

    # --- A replayed ticket is refused and mints no membership ------------------
    replayed = daemon.product_join(
        signed_ticket=signed_ticket, local_actor_ref=PEER_ID
    )
    assert replayed["outcome"] == "rejected"
    assert replayed["failure"]["code"] == "replayed_ticket"
    assert daemon.membership_event_count(creation.product_id, PEER_ID) == 1

    # --- Cloud reflects the daemon-confirmed membership ------------------------
    store.add_pairing(
        DaemonPairing(
            pairing_id="pairing-acme",
            team_id=PEER_DOMAIN_TEAM,
            daemon_id="daemon-acme",
            status=PairingStatus.ACTIVE,
            daemon_version="1.4.0",
        )
    )
    store.set_workspaces(
        PEER_DOMAIN_TEAM,
        [
            WorkspaceDiscoveryItem(
                workspace_id=creation.product_id,
                display_name=creation.display_name,
                pairing_status="active",
                readiness=WorkspaceReadiness.READY,
            )
        ],
    )
    member_view = _item(client, PEER_TOKEN, creation.product_id)
    assert member_view is not None
    # Member rows omit is_member (not `false`), so the SPA groups it under
    # "Your products" rather than "Available".
    assert member_view.get("is_member") is not False
    # Exactly one row for the Product (the available directory item deduped).
    assert _discovered_ids(client, PEER_TOKEN).count(creation.product_id) == 1

    # --- Recreate Cloud again; membership projection persists ------------------
    client.close()
    client = TestClient(app)
    assert creation.product_id in _discovered_ids(client, PEER_TOKEN)

    # --- Wrong-domain identity: no discovery, no ticket, no cross-tenant data --
    assert creation.product_id not in _discovered_ids(client, WRONG_TOKEN)
    wrong_ticket = client.post(
        f"/api/v1/products/{creation.product_id}/join-ticket",
        headers=_auth(WRONG_TOKEN),
    )
    assert wrong_ticket.status_code == 403, wrong_ticket.text
    assert wrong_ticket.json()["detail"]["code"] == "product_ineligible"
    captured.append(wrong_ticket.text)

    # --- End-to-end redaction --------------------------------------------------
    provider_secret, event_store_path = daemon.local_only_material(
        creation.product_id
    )
    blob = "\n".join(captured)
    for needle in FORBIDDEN_SUBSTRINGS:
        assert needle not in blob, f"leaked {needle!r} into a Cloud/browser payload"
    assert provider_secret not in blob
    assert event_store_path not in blob
    # The daemon's safe projection also excludes secrets, paths, and roles.
    projection = json.dumps(daemon.safe_projection(creation.product_id))
    for needle in (*FORBIDDEN_SUBSTRINGS, POLICY_OWNER, PRODUCT_OWNER, PRODUCT_MEMBER):
        assert needle not in projection

    client.close()


def test_daemon_rejects_tampered_and_mismatched_tickets(onboarding: Store) -> None:
    """The daemon fails closed on signature, account, domain, and product drift."""

    daemon = _daemon()
    creation = daemon.create_product(
        creator_actor_ref=OWNER_ID,
        display_name="Checkout",
        verified_domain=OWNER_DOMAIN,
    )
    client = TestClient(app)
    client.post(
        "/api/v1/products",
        headers=_auth(OWNER_TOKEN),
        json={
            "canonical_product_id": creation.product_id,
            "display_name": creation.display_name,
            "eligible_domain": OWNER_DOMAIN,
            "creation_receipt": creation.creation_receipt,
            "registration_ref": creation.registration_ref,
            "first_event_commit_proof": first_event_proof_dict(creation.product_id),
        },
    )
    signed = client.post(
        f"/api/v1/products/{creation.product_id}/join-ticket",
        headers=_auth(PEER_TOKEN),
    ).json()

    # wrong account: ticket minted for the peer, submitted for a different actor
    wrong_account = daemon.product_join(
        signed_ticket=signed, local_actor_ref="acct_someone_else"
    )
    assert wrong_account["failure"]["code"] == "wrong_account"

    # tampered signature
    tampered = json.loads(json.dumps(signed))
    tampered["ticket"]["verified_domain"] = OTHER_DOMAIN
    bad = daemon.product_join(signed_ticket=tampered, local_actor_ref=PEER_ID)
    assert bad["outcome"] == "rejected"
    assert bad["failure"]["code"] in {"bad_signature", "wrong_domain"}

    # expired ticket (validated against a moment past expiry)
    expired = daemon.product_join(
        signed_ticket=signed,
        local_actor_ref=PEER_ID,
        now=datetime.now(UTC) + timedelta(seconds=600),
    )
    assert expired["failure"]["code"] == "expired_ticket"

    # no membership was ever minted by a rejected join
    assert daemon.membership_event_count(creation.product_id, PEER_ID) == 0
    client.close()
