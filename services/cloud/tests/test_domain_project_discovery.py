"""Focused tests for Supabase domain-gated Project discovery (cloud#95).

Exercises the domain/team eligibility layer that replaces invite-code join as
the default discovery mechanism, at both the store/adapter unit level and the
``GET /api/v1/projects`` + create-then-discover API level:

- a team scoped to a verified email domain is discoverable to matching users
  with no explicit membership and no join code (allowed-domain discovery);
- a design-partner-allowed user on a different domain sees nothing for the team
  (domain mismatch — the tenancy boundary stays fail-closed);
- explicit membership and domain eligibility union deterministically, with
  membership role winning;
- creating a Project refreshes discovery — it appears in the same domain/team
  list with no join code or forbidden fields leaked.
"""

from __future__ import annotations

import time

import httpx
import jwt

from bicameral_cloud.config import CloudSettings, reset_settings
from bicameral_cloud.models.discovery import (
    FORBIDDEN_DISCOVERY_FIELDS,
    WorkspaceDiscoveryItem,
    WorkspaceReadiness,
)
from bicameral_cloud.models.tenancy import (
    AuthenticatedUser,
    Team,
    TeamEligibilitySource,
    TeamMembership,
    TeamRole,
)
from bicameral_cloud.project_discovery import LocalAlphaProjectAdapter
from bicameral_cloud.store import Store

SUPABASE_TEST_SECRET = "test-jwt-secret-at-least-32-chars-long"
ALLOWED_DOMAIN = "bicameral-ai.com"
OTHER_ALLOWED_DOMAIN = "other-partner.com"


def _user(user_id: str, email: str) -> AuthenticatedUser:
    return AuthenticatedUser(user_id=user_id, email=email)


def _domain_team(store: Store, team_id: str, domain: str) -> None:
    store.add_team(
        Team(
            team_id=team_id,
            display_name=f"Team {team_id}",
            created_by="seed",
            domain=domain,
        )
    )


class TestListEligibleTeams:
    def test_domain_match_grants_eligibility_without_membership(self) -> None:
        store = Store()
        _domain_team(store, "team-a", ALLOWED_DOMAIN)

        eligible = store.list_eligible_teams("user-1", ALLOWED_DOMAIN)

        assert [e.team_id for e in eligible] == ["team-a"]
        assert eligible[0].role == TeamRole.MEMBER
        assert eligible[0].source == TeamEligibilitySource.DOMAIN

    def test_domain_mismatch_grants_nothing(self) -> None:
        store = Store()
        _domain_team(store, "team-a", ALLOWED_DOMAIN)

        assert store.list_eligible_teams("user-1", OTHER_ALLOWED_DOMAIN) == []
        assert store.list_eligible_teams("user-1", None) == []

    def test_membership_and_domain_union_with_membership_role_winning(self) -> None:
        store = Store()
        _domain_team(store, "team-a", ALLOWED_DOMAIN)
        _domain_team(store, "team-b", ALLOWED_DOMAIN)
        # Explicit owner membership on team-a; domain-only eligibility on team-b.
        store.add_membership(
            TeamMembership(user_id="user-1", team_id="team-a", role=TeamRole.OWNER)
        )

        eligible = {e.team_id: e for e in store.list_eligible_teams(
            "user-1", ALLOWED_DOMAIN
        )}

        assert set(eligible) == {"team-a", "team-b"}
        assert eligible["team-a"].role == TeamRole.OWNER
        assert eligible["team-a"].source == TeamEligibilitySource.MEMBERSHIP
        assert eligible["team-b"].role == TeamRole.MEMBER
        assert eligible["team-b"].source == TeamEligibilitySource.DOMAIN

    def test_ordering_is_deterministic_by_team_id(self) -> None:
        store = Store()
        for team_id in ("team-c", "team-a", "team-b"):
            _domain_team(store, team_id, ALLOWED_DOMAIN)

        eligible = store.list_eligible_teams("user-1", ALLOWED_DOMAIN)

        assert [e.team_id for e in eligible] == ["team-a", "team-b", "team-c"]

    def test_domain_comparison_is_case_insensitive(self) -> None:
        store = Store()
        _domain_team(store, "team-a", ALLOWED_DOMAIN)

        eligible = store.list_eligible_teams("user-1", ALLOWED_DOMAIN.upper())

        assert [e.team_id for e in eligible] == ["team-a"]


class TestAdapterDomainDiscovery:
    def test_matching_domain_user_discovers_domain_scoped_project(self) -> None:
        store = Store()
        _domain_team(store, "team-a", ALLOWED_DOMAIN)
        store.set_workspaces(
            "team-a",
            [
                WorkspaceDiscoveryItem(
                    workspace_id="ws-1",
                    display_name="Frontend App",
                    pairing_status="paired",
                    readiness=WorkspaceReadiness.READY,
                )
            ],
        )

        adapter = LocalAlphaProjectAdapter(store)
        items = adapter.list_projects_for_user(_user("u1", f"a@{ALLOWED_DOMAIN}"))

        assert [i.project_id for i in items] == ["ws-1"]
        assert items[0].membership_role == TeamRole.MEMBER

    def test_domain_mismatch_user_discovers_nothing(self) -> None:
        store = Store()
        _domain_team(store, "team-a", ALLOWED_DOMAIN)
        store.set_workspaces(
            "team-a",
            [
                WorkspaceDiscoveryItem(
                    workspace_id="ws-1",
                    display_name="Frontend App",
                    pairing_status="paired",
                    readiness=WorkspaceReadiness.READY,
                )
            ],
        )

        adapter = LocalAlphaProjectAdapter(store)
        items = adapter.list_projects_for_user(
            _user("u2", f"b@{OTHER_ALLOWED_DOMAIN}")
        )

        assert items == []


class TestCreateThenDiscoverByDomain:
    """API-level: create writes a coordination record and refreshes discovery."""

    def _make_jwt(self, sub: str, email: str) -> str:
        return jwt.encode(
            {
                "sub": sub,
                "email": email,
                "aud": "authenticated",
                "exp": int(time.time()) + 3600,
            },
            SUPABASE_TEST_SECRET,
            algorithm="HS256",
        )

    def _enable_allowlist(self, monkeypatch) -> None:
        def fake_get(url, params, headers, timeout):
            domain = (params or {}).get("domain", "")
            allowed = domain in (
                f"eq.{ALLOWED_DOMAIN}",
                f"eq.{OTHER_ALLOWED_DOMAIN}",
            )
            return httpx.Response(
                200,
                json=[{"id": "row"}] if allowed else [],
                request=httpx.Request("GET", url),
            )

        monkeypatch.setattr(
            "bicameral_cloud.auth.design_partner_allowlist.httpx.get", fake_get
        )
        reset_settings(
            CloudSettings(
                supabase_url="https://example.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key="anon-key",
                supabase_service_key="service-role-key",
            )
        )

    def test_create_then_discover_by_domain_without_join_code(
        self, client, store, monkeypatch
    ):
        self._enable_allowlist(monkeypatch)
        try:
            store.add_team(
                Team(
                    team_id="team-a",
                    display_name="Team A",
                    created_by="seed",
                    domain=ALLOWED_DOMAIN,
                )
            )
            creator = self._make_jwt("u-creator", f"creator@{ALLOWED_DOMAIN}")
            peer = self._make_jwt("u-peer", f"peer@{ALLOWED_DOMAIN}")
            outsider = self._make_jwt("u-out", f"out@{OTHER_ALLOWED_DOMAIN}")

            created = client.post(
                "/api/v1/teams/team-a/project-records",
                json={"display_name": "Checkout"},
                headers={"Authorization": f"Bearer {creator}"},
            )
            assert created.status_code == 201
            project_id = created.json()["project_id"]

            # A different same-domain user discovers it with no join code.
            resp = client.get(
                "/api/v1/projects",
                headers={"Authorization": f"Bearer {peer}"},
            )
            assert resp.status_code == 200
            body = resp.text
            assert any(
                p["project_id"] == project_id for p in resp.json()["projects"]
            )
            assert "join_code" not in body
            for field in FORBIDDEN_DISCOVERY_FIELDS:
                assert field not in body

            # A domain-mismatched (but allowed) user sees no tenant data.
            denied = client.get(
                "/api/v1/projects",
                headers={"Authorization": f"Bearer {outsider}"},
            )
            assert denied.status_code == 200
            assert denied.json()["projects"] == []
        finally:
            reset_settings()
