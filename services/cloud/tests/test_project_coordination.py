"""Tests for the hosted Project create/join coordination API (issue #88).

Covers the acceptance criteria:
- an authenticated team member can create a Project coordination record;
- an authenticated eligible member can join via the join code;
- created/joined Projects appear in team discovery with safe metadata only and
  ``unbound`` local-workspace readiness;
- outsiders/non-members are rejected;
- discovery leaks no ``FORBIDDEN_DISCOVERY_FIELDS`` and never exposes the join
  code or member lists;
- local workspace binding stays ``unbound`` until a paired daemon reports
  readiness.
"""

from __future__ import annotations

from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.discovery import FORBIDDEN_DISCOVERY_FIELDS

from .conftest import (
    OTHER_USER_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
    USER_ID,
)

CREATE_URL = f"/api/v1/teams/{TEAM_ID}/project-records"
PROJECTS_URL = "/api/v1/projects"


def _auth(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def _join_url(project_id: str) -> str:
    return f"/api/v1/teams/{TEAM_ID}/project-records/{project_id}/join"


def _create(client, token: str = TOKEN_MEMBER, name: str = "Checkout"):
    return client.post(CREATE_URL, json={"display_name": name}, headers=_auth(token))


# ── Create ──────────────────────────────────────────────────────────────────


class TestCreateProjectRecord:
    def test_member_can_create_record(self, client, seeded_store):
        resp = _create(client, TOKEN_MEMBER, "Checkout Service")
        assert resp.status_code == 201
        body = resp.json()
        assert body["team_id"] == TEAM_ID
        assert body["display_name"] == "Checkout Service"
        assert body["project_id"].startswith("proj-")
        assert body["binding_readiness"] == "unbound"
        assert isinstance(body["join_code"], str) and body["join_code"]

    def test_create_persists_record_and_creator_membership(
        self, client, seeded_store
    ):
        project_id = _create(client, TOKEN_MEMBER).json()["project_id"]
        record = seeded_store.get_project(project_id)
        assert record is not None
        assert record.team_id == TEAM_ID
        assert record.created_by == USER_ID
        assert seeded_store.is_project_member(project_id, USER_ID)

    def test_create_emits_audit_event(self, client, seeded_store):
        _create(client, TOKEN_MEMBER)
        actions = [e.action for e in seeded_store.list_audit_events_for_team(TEAM_ID)]
        assert AuditAction.PROJECT_RECORD_CREATED in actions

    def test_outsider_cannot_create(self, client, seeded_store):
        resp = _create(client, TOKEN_OUTSIDER)
        assert resp.status_code == 403

    def test_create_requires_auth(self, client, seeded_store):
        resp = client.post(CREATE_URL, json={"display_name": "Checkout"})
        assert resp.status_code == 422
        resp = client.post(
            CREATE_URL, json={"display_name": "Checkout"}, headers=_auth("nope")
        )
        assert resp.status_code == 401

    def test_create_rejects_empty_display_name(self, client, seeded_store):
        resp = _create(client, TOKEN_MEMBER, "   ")
        assert resp.status_code == 422

    def test_create_rejects_path_or_secret_display_name(self, client, seeded_store):
        for value in ("/Users/me/repo", "prod secret", "svc api_key"):
            resp = _create(client, TOKEN_MEMBER, value)
            assert resp.status_code == 422

    def test_create_does_not_mutate_daemon_binding(self, client, seeded_store):
        workspaces_before = list(seeded_store.get_workspaces(TEAM_ID))
        pairings_before = list(seeded_store.list_pairings_for_team(TEAM_ID))
        members_before = list(seeded_store.list_team_members(TEAM_ID))
        _create(client, TOKEN_MEMBER)
        assert seeded_store.get_workspaces(TEAM_ID) == workspaces_before
        assert seeded_store.list_pairings_for_team(TEAM_ID) == pairings_before
        assert seeded_store.list_team_members(TEAM_ID) == members_before


# ── Join ──────────────────────────────────────────────────────────────────


class TestJoinProjectRecord:
    def test_eligible_member_joins_with_code(self, client, seeded_store):
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.post(
            _join_url(created["project_id"]),
            json={"join_code": created["join_code"]},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["project_id"] == created["project_id"]
        assert body["binding_readiness"] == "unbound"
        assert seeded_store.is_project_member(created["project_id"], OTHER_USER_ID)

    def test_join_emits_audit_event(self, client, seeded_store):
        created = _create(client, TOKEN_MEMBER).json()
        client.post(
            _join_url(created["project_id"]),
            json={"join_code": created["join_code"]},
            headers=_auth(TOKEN_OWNER),
        )
        actions = [e.action for e in seeded_store.list_audit_events_for_team(TEAM_ID)]
        assert AuditAction.PROJECT_RECORD_JOINED in actions

    def test_join_wrong_code_rejected(self, client, seeded_store):
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.post(
            _join_url(created["project_id"]),
            json={"join_code": "wrong-code"},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 403
        assert not seeded_store.is_project_member(
            created["project_id"], OTHER_USER_ID
        )

    def test_outsider_cannot_join(self, client, seeded_store):
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.post(
            _join_url(created["project_id"]),
            json={"join_code": created["join_code"]},
            headers=_auth(TOKEN_OUTSIDER),
        )
        assert resp.status_code == 403

    def test_join_unknown_project_404(self, client, seeded_store):
        resp = client.post(
            _join_url("proj-does-not-exist"),
            json={"join_code": "abc"},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 404

    def test_join_by_code_is_marked_deprecated_legacy(self, client, seeded_store):
        """cloud#95: invite-code join is retained only as a legacy path.

        A successful legacy join still works but carries a ``Deprecation``
        header so clients know it is not the default discovery/join mechanism.
        """
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.post(
            _join_url(created["project_id"]),
            json={"join_code": created["join_code"]},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 200
        assert resp.headers.get("Deprecation") == "true"


# ── Discovery integration ────────────────────────────────────────────────────


class TestCreatedProjectAppearsInDiscovery:
    def test_created_project_listed_unbound_for_team_members(
        self, client, seeded_store
    ):
        created = _create(client, TOKEN_MEMBER, "Coordination Project").json()
        resp = client.get(PROJECTS_URL, headers=_auth(TOKEN_OWNER))
        assert resp.status_code == 200
        listed = {p["project_id"]: p for p in resp.json()["projects"]}
        assert created["project_id"] in listed
        item = listed[created["project_id"]]
        assert item["display_name"] == "Coordination Project"
        assert item["binding_readiness"] == "unbound"
        assert item["membership_role"] == "team_owner"

    def test_stays_unbound_even_with_active_compatible_pairing(
        self, client, seeded_store
    ):
        # seeded_store already has an active, compatible pairing for the team.
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.get(PROJECTS_URL, headers=_auth(TOKEN_MEMBER))
        item = next(
            p
            for p in resp.json()["projects"]
            if p["project_id"] == created["project_id"]
        )
        assert item["pairing_status"] == "paired"
        assert item["binding_readiness"] == "unbound"

    def test_outsider_does_not_see_created_project(self, client, seeded_store):
        _create(client, TOKEN_MEMBER)
        resp = client.get(PROJECTS_URL, headers=_auth(TOKEN_OUTSIDER))
        assert resp.json() == {"projects": [], "selected_project_id": None}


# ── No leakage ───────────────────────────────────────────────────────────────


class TestNoLeakage:
    def test_discovery_omits_join_code_and_forbidden_fields(
        self, client, seeded_store
    ):
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.get(PROJECTS_URL, headers=_auth(TOKEN_MEMBER))
        body = resp.text
        assert created["join_code"] not in body
        assert "join_code" not in body
        for field in FORBIDDEN_DISCOVERY_FIELDS:
            assert field not in body

    def test_create_response_has_only_safe_fields(self, client, seeded_store):
        body = _create(client, TOKEN_MEMBER).json()
        assert set(body) == {
            "project_id",
            "team_id",
            "display_name",
            "binding_readiness",
            "join_code",
        }

    def test_join_response_omits_join_code(self, client, seeded_store):
        created = _create(client, TOKEN_MEMBER).json()
        resp = client.post(
            _join_url(created["project_id"]),
            json={"join_code": created["join_code"]},
            headers=_auth(TOKEN_OWNER),
        )
        assert set(resp.json()) == {
            "project_id",
            "team_id",
            "display_name",
            "binding_readiness",
        }
