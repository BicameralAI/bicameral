"""Tests for daemon pairing API (issue #17).

Covers: create, list, revoke, get, wrong-team rejection, role enforcement,
and daemon-issued pairing identity preservation.
"""

from __future__ import annotations

from bicameral_cloud.models.tenancy import Team
from tests.conftest import (
    OTHER_TEAM_ID,
    PAIRING_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
)


def test_create_pairing_as_owner(client, seeded_store):
    pairing_id = "pairing-daemon-owner"
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json={
            "pairing_id": pairing_id,
            "daemon_id": "daemon-new",
            "daemon_version": "2.0.0",
        },
    )
    assert resp.status_code == 201
    data = resp.json()
    assert data["team_id"] == TEAM_ID
    assert data["daemon_id"] == "daemon-new"
    assert data["daemon_version"] == "2.0.0"
    assert data["status"] == "active"
    assert data["pairing_id"] == pairing_id


def test_create_pairing_as_member(client, seeded_store):
    """Allowlisted-domain team members may create their own pairing."""
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={"pairing_id": "pairing-member", "daemon_id": "daemon-member"},
    )
    assert resp.status_code == 201
    data = resp.json()
    assert data["team_id"] == TEAM_ID
    assert data["daemon_id"] == "daemon-member"
    assert data["pairing_id"] == "pairing-member"
    assert data["status"] == "active"


def test_create_pairing_domain_eligible_without_membership(client, seeded_store):
    """Verified same-domain user may create a pairing with no membership row."""
    seeded_store.add_team(
        Team(
            team_id=TEAM_ID,
            display_name="Alpha",
            created_by="user-2",
            domain="example.com",
        )
    )
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        json={
            "pairing_id": "pairing-domain-eligible",
            "daemon_id": "daemon-domain-eligible",
        },
    )
    assert resp.status_code == 201
    assert resp.json()["pairing_id"] == "pairing-domain-eligible"
    assert resp.json()["daemon_id"] == "daemon-domain-eligible"
    assert resp.json()["team_id"] == TEAM_ID


def test_create_pairing_requires_daemon_issued_pairing_id(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json={"daemon_id": "daemon-missing-id"},
    )
    assert resp.status_code == 422


def test_create_pairing_exact_retry_is_idempotent(client, seeded_store):
    body = {"pairing_id": "pairing-retry", "daemon_id": "daemon-retry"}
    first = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json=body,
    )
    second = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json=body,
    )
    assert first.status_code == 201
    assert second.status_code == 201
    assert first.json()["pairing_id"] == "pairing-retry"
    assert second.json() == first.json()


def test_create_pairing_id_collision_fails_closed(client, seeded_store):
    pairing_id = "pairing-collision"
    first = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json={"pairing_id": pairing_id, "daemon_id": "daemon-a"},
    )
    assert first.status_code == 201

    collision = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json={"pairing_id": pairing_id, "daemon_id": "daemon-b"},
    )
    assert collision.status_code == 409
    assert collision.json()["detail"]["kind"] == "PairingIdConflict"


def test_create_pairing_wrong_team(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{OTHER_TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        json={"pairing_id": "pairing-wrong-team", "daemon_id": "daemon-x"},
    )
    assert resp.status_code == 403


def test_list_pairings(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["team_id"] == TEAM_ID
    assert len(data["pairings"]) == 1
    assert data["pairings"][0]["pairing_id"] == PAIRING_ID


def test_list_pairings_wrong_team(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{OTHER_TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 403


def test_get_pairing(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["pairing_id"] == PAIRING_ID
    assert data["status"] == "active"


def test_get_pairing_not_found(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/pairings/nonexistent",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 404


def test_revoke_pairing_as_owner(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/revoke",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "revoked"
    assert data["pairing_id"] == PAIRING_ID


def test_revoke_pairing_member_forbidden(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/revoke",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 403


def test_revoke_pairing_not_found(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/nonexistent/revoke",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 404


def test_outsider_cannot_list_pairings(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
    )
    assert resp.status_code == 403


def test_pairing_record_has_no_secrets(client, seeded_store):
    """Verify pairing responses contain no provider secrets or workspace paths."""
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    for forbidden_key in ("secret", "token", "password", "workspace_path", "provider"):
        assert forbidden_key not in data
