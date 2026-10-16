"""Tests for team-visible workspace discovery (issue #19).

Acceptance criteria:
- Team members can request a team-visible workspace list.
- Non-members cannot access the team's discovery route.
- Response shape matches daemon fixtures / stubbed adapter.
- API does not expose forbidden rich metadata by default.
- Tests cover owner/member visibility and non-member rejection.
"""

from __future__ import annotations

from bicameral_cloud.models.discovery import FORBIDDEN_DISCOVERY_FIELDS
from tests.conftest import (
    OTHER_TEAM_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
)


class TestWorkspaceDiscoverySuccess:
    """Team members see the same workspace list."""

    def test_member_sees_workspaces(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["team_id"] == TEAM_ID
        assert len(data["workspaces"]) == 2
        ws = data["workspaces"][0]
        assert set(ws.keys()) == {
            "workspace_id",
            "display_name",
            "pairing_status",
            "readiness",
        }

    def test_owner_sees_same_workspaces(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert len(data["workspaces"]) == 2

    def test_member_and_owner_see_identical_list(self, client, seeded_store):
        """No per-user filtering in alpha — everyone sees the same list."""
        resp_member = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        resp_owner = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        assert resp_member.json() == resp_owner.json()


class TestWorkspaceDiscoveryRejection:
    """Non-members and unauthenticated users are rejected."""

    def test_non_member_rejected(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403

    def test_no_auth_rejected(self, client, seeded_store):
        resp = client.get(f"/api/v1/teams/{TEAM_ID}/workspaces")
        assert resp.status_code == 422  # missing header

    def test_bad_token_rejected(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": "Bearer invalid-token"},
        )
        assert resp.status_code == 401


class TestWorkspaceDiscoveryEdgeCases:
    """Edge cases: no pairing, empty workspaces, incompatible daemon."""

    def test_no_pairing_returns_empty(self, client, seeded_store):
        """Team with no active pairing returns empty workspace list."""
        seeded_store.add_membership(
            __import__(
                "bicameral_cloud.models.tenancy", fromlist=["TeamMembership"]
            ).TeamMembership(
                user_id="user-outsider",
                team_id=OTHER_TEAM_ID,
                role=__import__(
                    "bicameral_cloud.models.tenancy", fromlist=["TeamRole"]
                ).TeamRole.MEMBER,
            )
        )
        resp = client.get(
            f"/api/v1/teams/{OTHER_TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 200
        assert resp.json()["workspaces"] == []

    def test_upgrade_required_daemon_rejected(self, client, seeded_store):
        """Daemon requiring upgrade blocks discovery."""
        from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus

        seeded_store.add_membership(
            __import__(
                "bicameral_cloud.models.tenancy", fromlist=["TeamMembership"]
            ).TeamMembership(
                user_id="user-outsider",
                team_id=OTHER_TEAM_ID,
                role=__import__(
                    "bicameral_cloud.models.tenancy", fromlist=["TeamRole"]
                ).TeamRole.MEMBER,
            )
        )
        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-old",
                team_id=OTHER_TEAM_ID,
                daemon_id="daemon-old",
                status=PairingStatus.ACTIVE,
                daemon_version="0.1.0",
            )
        )
        resp = client.get(
            f"/api/v1/teams/{OTHER_TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 409


class TestNoMetadataLeakage:
    """Verify no forbidden rich metadata leaks into discovery response."""

    def test_response_contains_no_forbidden_fields(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        for field in FORBIDDEN_DISCOVERY_FIELDS:
            assert field not in data, f"Forbidden field '{field}' found in response"

    def test_workspace_items_have_only_safe_fields(self, client, seeded_store):
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        safe_fields = {"workspace_id", "display_name", "pairing_status", "readiness"}
        for ws in resp.json()["workspaces"]:
            assert set(ws.keys()) == safe_fields

    def test_no_path_or_secret_in_response_body(self, client, seeded_store):
        """Ensure no filesystem paths or secret-like values appear."""
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/workspaces",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        body = resp.text.lower()
        leak_indicators = [
            "/home/",
            "/var/",
            "/tmp/",
            "/usr/",
            "c:\\",
            ".ssh",
            "secret",
            "password",
            "credential",
            "api_key",
            "token",
            "private_key",
        ]
        for indicator in leak_indicators:
            assert indicator not in body, (
                f"Potential leak indicator '{indicator}' found in response"
            )
