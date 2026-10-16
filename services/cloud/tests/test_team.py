"""Tests for team management routes (issue #16).

Covers team CRUD, membership management, role enforcement, and
cross-team data access rejection.
"""

from __future__ import annotations

from tests.conftest import (
    OUTSIDER_USER_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
    USER_ID,
)


class TestGetTeamInfo:
    """GET /api/v1/teams/{team_id}"""

    def test_member_gets_team_info(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["team_id"] == TEAM_ID
        assert data["user_id"] == USER_ID
        assert data["role"] == "team_member"

    def test_owner_gets_team_info(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 200
        assert resp.json()["role"] == "team_owner"

    def test_non_member_rejected(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403


class TestCreateTeam:
    """POST /api/v1/teams"""

    def test_create_team_returns_owner_role(self, client, seeded_store):
        resp = client.post(
            "/api/v1/teams",
            json={"display_name": "New Team"},
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["display_name"] == "New Team"
        assert data["role"] == "team_owner"
        assert data["created_by"] == USER_ID
        assert data["team_id"].startswith("team-")

    def test_creator_can_access_new_team(self, client, seeded_store):
        create_resp = client.post(
            "/api/v1/teams",
            json={"display_name": "My Team"},
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        team_id = create_resp.json()["team_id"]
        info_resp = client.get(
            f"/api/v1/teams/{team_id}",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert info_resp.status_code == 200
        assert info_resp.json()["role"] == "team_owner"

    def test_create_team_requires_auth(self, client, seeded_store):
        resp = client.post(
            "/api/v1/teams",
            json={"display_name": "Fail Team"},
        )
        assert resp.status_code == 422


class TestListMembers:
    """GET /api/v1/teams/{team_id}/members"""

    def test_member_lists_members(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/members",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["team_id"] == TEAM_ID
        user_ids = [m["user_id"] for m in data["members"]]
        assert USER_ID in user_ids

    def test_non_member_cannot_list(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/members",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403


class TestAddMember:
    """POST /api/v1/teams/{team_id}/members"""

    def test_owner_adds_member(self, client, seeded_store):
        resp = client.post(
            f"/api/v1/teams/{TEAM_ID}/members",
            json={"user_id": OUTSIDER_USER_ID, "role": "team_member"},
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["user_id"] == OUTSIDER_USER_ID
        assert data["role"] == "team_member"

    def test_member_cannot_add_member(self, client, seeded_store):
        resp = client.post(
            f"/api/v1/teams/{TEAM_ID}/members",
            json={"user_id": OUTSIDER_USER_ID, "role": "team_member"},
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403

    def test_add_duplicate_member_rejected(self, client, seeded_store):
        resp = client.post(
            f"/api/v1/teams/{TEAM_ID}/members",
            json={"user_id": USER_ID, "role": "team_member"},
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 409

    def test_non_member_cannot_add(self, client, seeded_store):
        resp = client.post(
            f"/api/v1/teams/{TEAM_ID}/members",
            json={"user_id": "random-user", "role": "team_member"},
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403


class TestRemoveMember:
    """DELETE /api/v1/teams/{team_id}/members/{user_id}"""

    def test_owner_removes_member(self, client, seeded_store):
        resp = client.delete(
            f"/api/v1/teams/{TEAM_ID}/members/{USER_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 200
        assert resp.json()["removed"] == "true"

    def test_member_cannot_remove(self, client, seeded_store):
        resp = client.delete(
            f"/api/v1/teams/{TEAM_ID}/members/{USER_ID}",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403

    def test_owner_cannot_remove_self(self, client, seeded_store):
        from tests.conftest import OTHER_USER_ID

        resp = client.delete(
            f"/api/v1/teams/{TEAM_ID}/members/{OTHER_USER_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 400

    def test_remove_nonexistent_member(self, client, seeded_store):
        resp = client.delete(
            f"/api/v1/teams/{TEAM_ID}/members/nonexistent-user",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 404

    def test_removed_member_loses_access(self, client, seeded_store):
        client.delete(
            f"/api/v1/teams/{TEAM_ID}/members/{USER_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403


class TestCrossTeamDataAccess:
    """Verify team tenancy isolation: cross-team access rejected."""

    def test_outsider_cannot_access_team_info(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_outsider_cannot_list_members(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/members",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_outsider_cannot_add_member(self, client, seeded_store):
        resp = client.post(
            f"/api/v1/teams/{TEAM_ID}/members",
            json={"user_id": "infiltrator", "role": "team_member"},
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_outsider_cannot_remove_member(self, client, seeded_store):
        resp = client.delete(
            f"/api/v1/teams/{TEAM_ID}/members/{USER_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_member_of_one_team_cannot_access_another(self, client, seeded_store):
        from bicameral_cloud.models.tenancy import TeamMembership, TeamRole

        seeded_store.add_membership(
            TeamMembership(
                user_id=OUTSIDER_USER_ID,
                team_id="team-other",
                role=TeamRole.OWNER,
            )
        )
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_workspace_discovery_cross_team_rejected(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_pairing_cross_team_rejected(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/pairings",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403
