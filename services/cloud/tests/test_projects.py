"""Tests for Cloud project discovery (issue #65).

Acceptance criteria:
- Cloud API returns a deterministic project list for an authenticated allowed
  user.
- API exposes no provider secrets, workspace paths, source bodies, or local
  daemon payloads.
- Tests cover unauthorized (no/invalid token) and forbidden (disallowed domain)
  cases, plus no-projects, pending, and ready projects.
"""

from __future__ import annotations

import time

import jwt

from bicameral_cloud.config import CloudSettings, reset_settings
from bicameral_cloud.models.discovery import (
    FORBIDDEN_DISCOVERY_FIELDS,
    WorkspaceDiscoveryItem,
    WorkspaceReadiness,
)
from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus
from bicameral_cloud.models.tenancy import TeamMembership, TeamRole
from tests.conftest import (
    OTHER_TEAM_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
    USER_ID,
)

SUPABASE_TEST_SECRET = "test-jwt-secret-at-least-32-chars-long"


class TestProjectListSuccess:
    """Authenticated allowed users get a deterministic project list."""

    def test_member_sees_deterministic_project_list(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["selected_project_id"] is None
        assert [p["project_id"] for p in data["projects"]] == ["ws-1", "ws-2"]
        first = data["projects"][0]
        assert first == {
            "project_id": "ws-1",
            "team_id": TEAM_ID,
            "display_name": "Frontend App",
            "discovery_eligibility": "membership",
            "membership_status": "member",
            "membership_role": "team_member",
            "pairing_status": "paired",
            "binding_readiness": "ready",
            "daemon_compatible": True,
        }

    def test_ready_and_degraded_projects_resolved(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        readiness = {
            p["project_id"]: p["binding_readiness"] for p in resp.json()["projects"]
        }
        assert readiness == {"ws-1": "ready", "ws-2": "degraded"}

    def test_owner_role_reflected_and_list_identical(self, client, seeded_store):
        """No per-user filtering: owner sees the same projects, own role."""
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
        )
        data = resp.json()
        assert [p["project_id"] for p in data["projects"]] == ["ws-1", "ws-2"]
        assert {p["membership_role"] for p in data["projects"]} == {"team_owner"}

    def test_deterministic_across_repeated_calls(self, client, seeded_store):
        first = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        ).json()
        second = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        ).json()
        assert first == second

    def test_selected_project_hint_echoed_when_discoverable(
        self, client, seeded_store
    ):
        resp = client.get(
            "/api/v1/projects",
            params={"selected_project_id": "ws-2"},
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.json()["selected_project_id"] == "ws-2"

    def test_selected_project_hint_dropped_when_not_discoverable(
        self, client, seeded_store
    ):
        resp = client.get(
            "/api/v1/projects",
            params={"selected_project_id": "ws-nope"},
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.json()["selected_project_id"] is None


class TestProjectListEdgeCases:
    """No projects, pending (unbound), and unpaired readiness cases."""

    def test_no_projects_for_user_without_teams(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 200
        assert resp.json() == {"projects": [], "selected_project_id": None}

    def test_pending_project_reports_unbound(self, client, seeded_store):
        seeded_store.set_workspaces(
            TEAM_ID,
            [
                WorkspaceDiscoveryItem(
                    workspace_id="ws-pending",
                    display_name="Pending Project",
                    pairing_status="active",
                    readiness=WorkspaceReadiness.UNAVAILABLE,
                ),
            ],
        )
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        project = resp.json()["projects"][0]
        assert project["project_id"] == "ws-pending"
        assert project["binding_readiness"] == "unbound"
        assert project["daemon_compatible"] is True

    def test_project_unpaired_when_team_has_no_active_pairing(
        self, client, seeded_store
    ):
        seeded_store.revoke_pairing("pairing-1")
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        for project in resp.json()["projects"]:
            assert project["pairing_status"] == "unpaired"
            assert project["binding_readiness"] == "unpaired"
            assert project["daemon_compatible"] is False

    def test_project_upgrade_required_for_incompatible_daemon(
        self, client, seeded_store
    ):
        seeded_store.revoke_pairing("pairing-1")
        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-old",
                team_id=TEAM_ID,
                daemon_id="daemon-old",
                status=PairingStatus.ACTIVE,
                daemon_version="0.1.0",
            )
        )
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        for project in resp.json()["projects"]:
            assert project["pairing_status"] == "paired"
            assert project["binding_readiness"] == "upgrade_required"
            assert project["daemon_compatible"] is False

    def test_projects_aggregate_across_multiple_teams(self, client, seeded_store):
        seeded_store.add_membership(
            TeamMembership(
                user_id=USER_ID, team_id=OTHER_TEAM_ID, role=TeamRole.OWNER
            )
        )
        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-beta",
                team_id=OTHER_TEAM_ID,
                daemon_id="daemon-beta",
                status=PairingStatus.ACTIVE,
                daemon_version="1.2.0",
            )
        )
        seeded_store.set_workspaces(
            OTHER_TEAM_ID,
            [
                WorkspaceDiscoveryItem(
                    workspace_id="ws-beta",
                    display_name="Beta Project",
                    pairing_status="active",
                    readiness=WorkspaceReadiness.READY,
                ),
            ],
        )
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        projects = resp.json()["projects"]
        # sorted by (team_id, project_id): team-alpha before team-beta
        assert [(p["team_id"], p["project_id"]) for p in projects] == [
            (TEAM_ID, "ws-1"),
            (TEAM_ID, "ws-2"),
            (OTHER_TEAM_ID, "ws-beta"),
        ]


class TestProjectListUnauthorized:
    """Unauthorized: missing or invalid bearer token."""

    def test_missing_auth_header_rejected(self, client, seeded_store):
        resp = client.get("/api/v1/projects")
        assert resp.status_code == 422  # missing required header

    def test_non_bearer_header_rejected(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": "Basic abc123"},
        )
        assert resp.status_code == 401

    def test_invalid_token_rejected(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": "Bearer not-a-real-token"},
        )
        assert resp.status_code == 401


class TestProjectListForbiddenDomain:
    """Forbidden: authenticated but disallowed design-partner domain."""

    def _make_jwt(self, email: str) -> str:
        return jwt.encode(
            {
                "sub": "sb-project-user",
                "email": email,
                "aud": "authenticated",
                "exp": int(time.time()) + 3600,
            },
            SUPABASE_TEST_SECRET,
            algorithm="HS256",
        )

    def _enable_supabase_table_allowlist(self) -> None:
        reset_settings(
            CloudSettings(
                supabase_url="https://example.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key="anon-key",
                supabase_service_key="service-role-key",
            )
        )

    def test_disallowed_domain_rejected_403(self, client, store, monkeypatch):
        import httpx

        def fake_get(url, params, headers, timeout):
            # domain not in allowlist; attach request for httpx.raise_for_status
            return httpx.Response(
                200, json=[], request=httpx.Request("GET", url)
            )

        monkeypatch.setattr(
            "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
        )
        self._enable_supabase_table_allowlist()
        try:
            token = self._make_jwt("intruder@not-allowed.example")
            resp = client.get(
                "/api/v1/projects",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 403
            assert resp.json()["detail"] == "Design partner access required"
        finally:
            reset_settings()

    def test_allowed_domain_via_supabase_allowlist(self, client, store, monkeypatch):
        import httpx

        def fake_get(url, params, headers, timeout):
            return httpx.Response(
                200,
                json=[{"id": "allow-domain"}],
                request=httpx.Request("GET", url),
            )

        monkeypatch.setattr(
            "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
        )
        self._enable_supabase_table_allowlist()
        try:
            store.add_membership(
                TeamMembership(
                    user_id="sb-project-user",
                    team_id=TEAM_ID,
                    role=TeamRole.MEMBER,
                )
            )
            store.set_workspaces(
                TEAM_ID,
                [
                    WorkspaceDiscoveryItem(
                        workspace_id="ws-1",
                        display_name="Frontend App",
                        pairing_status="active",
                        readiness=WorkspaceReadiness.READY,
                    ),
                ],
            )
            store.add_pairing(
                DaemonPairing(
                    pairing_id="pairing-sb",
                    team_id=TEAM_ID,
                    daemon_id="daemon-sb",
                    status=PairingStatus.ACTIVE,
                    daemon_version="1.2.0",
                )
            )
            token = self._make_jwt("alice@bicameral-ai.com")
            resp = client.get(
                "/api/v1/projects",
                headers={"Authorization": f"Bearer {token}"},
            )
            assert resp.status_code == 200
            assert [p["project_id"] for p in resp.json()["projects"]] == ["ws-1"]
        finally:
            reset_settings()


class TestProjectListNoLeakage:
    """No forbidden fields, paths, or secret-like values in the response."""

    def test_only_safe_fields_present(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        safe_fields = {
            "project_id",
            "team_id",
            "display_name",
            "discovery_eligibility",
            "membership_status",
            "membership_role",
            "pairing_status",
            "binding_readiness",
            "daemon_compatible",
        }
        for project in resp.json()["projects"]:
            assert set(project.keys()) == safe_fields

    def test_no_forbidden_fields_in_body(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        body = resp.text
        for field in FORBIDDEN_DISCOVERY_FIELDS:
            assert field not in body, f"Forbidden field '{field}' found in response"

    def test_no_path_or_secret_indicators(self, client, seeded_store):
        resp = client.get(
            "/api/v1/projects",
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
            "private_key",
        ]
        for indicator in leak_indicators:
            assert indicator not in body, (
                f"Potential leak indicator '{indicator}' found in response"
            )
