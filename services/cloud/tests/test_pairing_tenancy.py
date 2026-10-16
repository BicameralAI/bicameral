"""Pair initiation tenancy resolver (bot#1290 / CONF-PAIR-INIT-TEAM-01)."""

from __future__ import annotations

from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.tenancy import Team, TeamMembership, TeamRole
from tests.conftest import TEAM_ID, TOKEN_MEMBER, TOKEN_OUTSIDER, USER_ID


def test_pairing_tenancy_returns_existing_membership(client, seeded_store):
    resp = client.post(
        "/api/v1/auth/pairing-tenancy",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["user_id"] == USER_ID
    assert data["team_id"] == TEAM_ID
    assert data["source"] == "existing"


def test_pairing_tenancy_auto_creates_domain_team(client, seeded_store):
    resp = client.post(
        "/api/v1/auth/pairing-tenancy",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["source"] == "created"
    assert data["team_id"]
    team = seeded_store.get_team(data["team_id"])
    assert team is not None
    assert team.domain == "example.com"

    created_events = [
        e
        for e in seeded_store.list_audit_events_for_team(data["team_id"])
        if e.action == AuditAction.TEAM_CREATED
    ]
    assert len(created_events) == 1

    again = client.post(
        "/api/v1/auth/pairing-tenancy",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
    )
    assert again.status_code == 200
    assert again.json()["team_id"] == data["team_id"]
    assert again.json()["source"] == "existing"
    created_events = [
        e
        for e in seeded_store.list_audit_events_for_team(data["team_id"])
        if e.action == AuditAction.TEAM_CREATED
    ]
    assert len(created_events) == 1


def test_pairing_tenancy_rejects_multiple_teams_with_repair(client, seeded_store):
    seeded_store.add_team(
        Team(
            team_id="team-extra",
            display_name="Extra",
            created_by=USER_ID,
        )
    )
    seeded_store.add_membership(
        TeamMembership(
            user_id=USER_ID, team_id="team-extra", role=TeamRole.MEMBER
        )
    )
    resp = client.post(
        "/api/v1/auth/pairing-tenancy",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 409
    detail = resp.json()["detail"]
    assert detail["kind"] == "PairingTenancyAmbiguous"
    assert "exactly one Cloud team" in detail["repair"]
