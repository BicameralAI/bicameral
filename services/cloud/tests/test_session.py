"""Tests for hosted session API (issue #17).

Covers: open, list, get status, close, wrong-team rejection,
revoked pairing rejection, expired/closed session behavior.
"""

from __future__ import annotations

from bicameral_cloud.models.pairing import SessionStatus
from tests.conftest import (
    OTHER_TEAM_ID,
    PAIRING_ID,
    SESSION_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
)


def test_open_session(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={
            "pairing_id": PAIRING_ID,
            "origin": "https://dashboard.bicameral-ai.com",
            "schema_version": "1.0",
        },
    )
    assert resp.status_code == 201
    data = resp.json()
    assert data["team_id"] == TEAM_ID
    assert data["pairing_id"] == PAIRING_ID
    assert data["status"] == "open"
    assert data["origin"] == "https://dashboard.bicameral-ai.com"
    assert data["session_id"].startswith("session-")


def test_open_session_revoked_pairing(client, seeded_store):
    """Cannot open a session on a revoked pairing."""
    seeded_store.revoke_pairing(PAIRING_ID)
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={"pairing_id": PAIRING_ID},
    )
    assert resp.status_code == 409


def test_open_session_wrong_team(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{OTHER_TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={"pairing_id": PAIRING_ID},
    )
    assert resp.status_code == 403


def test_open_session_pairing_not_found(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        json={"pairing_id": "nonexistent"},
    )
    assert resp.status_code == 404


def test_list_sessions(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["team_id"] == TEAM_ID
    assert len(data["sessions"]) == 1
    assert data["sessions"][0]["session_id"] == SESSION_ID


def test_list_sessions_wrong_team(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{OTHER_TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 403


def test_get_session_status(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/sessions/{SESSION_ID}",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["session_id"] == SESSION_ID
    assert data["status"] == "open"


def test_get_session_not_found(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/sessions/nonexistent",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 404


def test_close_session_by_owner(client, seeded_store):
    """Session owner can close their own session."""
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions/{SESSION_ID}/close",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "closed"


def test_close_session_by_team_owner(client, seeded_store):
    """Team owner can close any session in the team."""
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions/{SESSION_ID}/close",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "closed"


def test_close_session_already_closed(client, seeded_store):
    """Closing an already-closed session returns 409."""
    seeded_store.close_session(SESSION_ID)
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions/{SESSION_ID}/close",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 409


def test_close_session_expired(client, seeded_store):
    """Closing an expired session returns 409."""
    session = seeded_store.get_session(SESSION_ID)
    assert session is not None
    session.status = SessionStatus.EXPIRED
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions/{SESSION_ID}/close",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 409


def test_close_session_wrong_team(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{OTHER_TEAM_ID}/sessions/{SESSION_ID}/close",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 403


def test_outsider_cannot_open_session(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        json={"pairing_id": PAIRING_ID},
    )
    assert resp.status_code == 403


def test_outsider_cannot_list_sessions(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
    )
    assert resp.status_code == 403
