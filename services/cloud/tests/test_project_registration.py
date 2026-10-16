"""Tests for hosted project create/join registration (issue #66).

Covers the acceptance criteria: Create/Join record durable coordination
requests (not no-ops), the alpha adapter persists them, responses expose only
safe coordination fields (no local paths/secrets), and neither action mutates
daemon workspace binding or grants team membership.
"""

from __future__ import annotations

import pytest

from bicameral_cloud.models.audit import AuditAction
from bicameral_cloud.models.registration import ProjectRegistrationKind
from bicameral_cloud.models.tenancy import TeamRole

from .conftest import (
    OTHER_USER_ID,
    OUTSIDER_USER_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
)

CREATE_URL = f"/api/v1/teams/{TEAM_ID}/projects"
JOIN_URL = f"/api/v1/teams/{TEAM_ID}/join-requests"
LIST_URL = f"/api/v1/teams/{TEAM_ID}/registration-requests"


def _auth(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


# ── Create project (expose_project intent) ──────────────────────────────────


class TestCreateProject:
    def test_owner_create_returns_pending_receipt(self, client, seeded_store):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )

        assert resp.status_code == 202
        body = resp.json()
        assert body["kind"] == "create_project"
        assert body["team_id"] == TEAM_ID
        assert body["status"] == "pending"
        assert body["next_step"] == "local_daemon_binding_required"
        assert body["request_id"].startswith("reg-")
        assert isinstance(body["message"], str) and body["message"]

    def test_create_persists_request_in_store(self, client, seeded_store):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service", "project_ref": "ws-checkout"},
            headers=_auth(TOKEN_OWNER),
        )
        request_id = resp.json()["request_id"]

        stored = seeded_store.get_registration_request(request_id)
        assert stored is not None
        assert stored.kind == ProjectRegistrationKind.CREATE_PROJECT
        assert stored.team_id == TEAM_ID
        assert stored.requested_by == OTHER_USER_ID
        assert stored.display_name == "Checkout Service"
        assert stored.project_ref == "ws-checkout"

    def test_create_emits_audit_event(self, client, seeded_store):
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )

        events = seeded_store.list_audit_events_for_team(TEAM_ID)
        actions = [e.action for e in events]
        assert AuditAction.PROJECT_REGISTRATION_REQUESTED in actions

    def test_member_cannot_create(self, client, seeded_store):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_MEMBER),
        )
        assert resp.status_code == 403

    def test_create_does_not_mutate_daemon_binding(self, client, seeded_store):
        workspaces_before = list(seeded_store.get_workspaces(TEAM_ID))
        pairings_before = list(seeded_store.list_pairings_for_team(TEAM_ID))
        members_before = list(seeded_store.list_team_members(TEAM_ID))

        client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )

        assert seeded_store.get_workspaces(TEAM_ID) == workspaces_before
        assert seeded_store.list_pairings_for_team(TEAM_ID) == pairings_before
        assert seeded_store.list_team_members(TEAM_ID) == members_before

    @pytest.mark.parametrize(
        "value",
        [
            "/Users/me/repo",
            "C:\\repo\\checkout",
            "git@github.com:acme/checkout.git",
            "https://github.com/acme/checkout",
            "../secrets",
        ],
    )
    def test_create_rejects_path_or_remote_project_ref(
        self, client, seeded_store, value
    ):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "Checkout", "project_ref": value},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 422

    @pytest.mark.parametrize(
        "value",
        ["my api_key", "prod secret", "service credential"],
    )
    def test_create_rejects_secret_like_display_name(
        self, client, seeded_store, value
    ):
        resp = client.post(
            CREATE_URL,
            json={"display_name": value},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 422

    def test_create_rejects_empty_display_name(self, client, seeded_store):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "   "},
            headers=_auth(TOKEN_OWNER),
        )
        assert resp.status_code == 422

    def test_create_unauthorized_without_token(self, client, seeded_store):
        resp = client.post(CREATE_URL, json={"display_name": "Checkout"})
        assert resp.status_code in (401, 403, 422)

    def test_create_invalid_token(self, client, seeded_store):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "Checkout"},
            headers=_auth("tok-nonexistent"),
        )
        assert resp.status_code == 401


class TestCreateSurfacesDiscoverableRecord:
    """cloud#118: create must make the Project discoverable via /app's refetch.

    The canonical ``/app`` SPA posts to ``.../projects`` and then refetches
    ``GET /api/v1/projects``. Create therefore also persists the discoverable
    coordination record (owned by the creator) it refers to, projected as
    ``unbound`` — without granting any local workspace-binding authority.
    """

    def test_create_persists_discoverable_coordination_record(
        self, client, seeded_store
    ):
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )

        records = seeded_store.list_projects_for_team(TEAM_ID)
        assert [r.display_name for r in records] == ["Checkout Service"]
        record = records[0]
        assert record.created_by == OTHER_USER_ID
        members = seeded_store.list_project_members(record.project_id)
        assert [m.user_id for m in members] == [OTHER_USER_ID]

    def test_created_project_is_discoverable_as_unbound(
        self, client, seeded_store
    ):
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )

        resp = client.get("/api/v1/projects", headers=_auth(TOKEN_OWNER))
        assert resp.status_code == 200
        projects = resp.json()["projects"]
        created = [p for p in projects if p["display_name"] == "Checkout Service"]
        assert len(created) == 1
        assert created[0]["binding_readiness"] == "unbound"

    def test_create_emits_project_record_created_audit(
        self, client, seeded_store
    ):
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )

        actions = [e.action for e in seeded_store.list_audit_events_for_team(TEAM_ID)]
        assert AuditAction.PROJECT_RECORD_CREATED in actions


# ── Join project (conservative pending request) ─────────────────────────────


class TestJoinProject:
    def test_non_member_join_returns_pending_receipt(self, client, seeded_store):
        resp = client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))

        assert resp.status_code == 202
        body = resp.json()
        assert body["kind"] == "join_project"
        assert body["team_id"] == TEAM_ID
        assert body["status"] == "pending"
        assert body["next_step"] == "team_owner_approval_required"

    def test_join_persists_but_does_not_grant_membership(self, client, seeded_store):
        resp = client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))
        request_id = resp.json()["request_id"]

        stored = seeded_store.get_registration_request(request_id)
        assert stored is not None
        assert stored.kind == ProjectRegistrationKind.JOIN_PROJECT
        assert stored.requested_by == OUTSIDER_USER_ID
        # Cloud membership is NOT granted by a join request.
        assert seeded_store.get_membership(OUTSIDER_USER_ID, TEAM_ID) is None

    def test_join_emits_audit_event(self, client, seeded_store):
        client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))

        actions = [e.action for e in seeded_store.list_audit_events_for_team(TEAM_ID)]
        assert AuditAction.PROJECT_JOIN_REQUESTED in actions

    def test_join_is_idempotent(self, client, seeded_store):
        first = client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))
        second = client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))

        assert first.json()["request_id"] == second.json()["request_id"]
        pending = seeded_store.list_registration_requests_for_team(
            TEAM_ID, kind=ProjectRegistrationKind.JOIN_PROJECT
        )
        assert len(pending) == 1

    def test_existing_member_join_conflicts(self, client, seeded_store):
        resp = client.post(JOIN_URL, json={}, headers=_auth(TOKEN_MEMBER))
        assert resp.status_code == 409

    def test_join_cannot_self_grant_owner(self, client, seeded_store):
        resp = client.post(
            JOIN_URL,
            json={"requested_role": TeamRole.OWNER.value},
            headers=_auth(TOKEN_OUTSIDER),
        )
        assert resp.status_code == 422

    def test_join_invalid_token(self, client, seeded_store):
        resp = client.post(JOIN_URL, json={}, headers=_auth("tok-nonexistent"))
        assert resp.status_code == 401

    def test_join_does_not_mutate_daemon_binding(self, client, seeded_store):
        workspaces_before = list(seeded_store.get_workspaces(TEAM_ID))
        pairings_before = list(seeded_store.list_pairings_for_team(TEAM_ID))

        client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))

        assert seeded_store.get_workspaces(TEAM_ID) == workspaces_before
        assert seeded_store.list_pairings_for_team(TEAM_ID) == pairings_before


# ── List registration requests ──────────────────────────────────────────────


class TestListRegistrationRequests:
    def test_member_lists_pending_requests(self, client, seeded_store):
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service"},
            headers=_auth(TOKEN_OWNER),
        )
        client.post(JOIN_URL, json={}, headers=_auth(TOKEN_OUTSIDER))

        resp = client.get(LIST_URL, headers=_auth(TOKEN_MEMBER))
        assert resp.status_code == 200
        body = resp.json()
        assert body["team_id"] == TEAM_ID
        kinds = sorted(r["kind"] for r in body["requests"])
        assert kinds == ["create_project", "join_project"]

    def test_non_member_cannot_list(self, client, seeded_store):
        resp = client.get(LIST_URL, headers=_auth(TOKEN_OUTSIDER))
        assert resp.status_code == 403


# ── No leakage of local paths/secrets in responses ──────────────────────────

_FORBIDDEN_SUBSTRINGS = ("/Users/", "\\", "://", "secret", "api_key", "token")


class TestRegistrationNoLeakage:
    def test_receipt_has_only_safe_fields(self, client, seeded_store):
        resp = client.post(
            CREATE_URL,
            json={"display_name": "Checkout Service", "project_ref": "ws-checkout"},
            headers=_auth(TOKEN_OWNER),
        )
        body = resp.json()
        assert set(body) == {
            "request_id",
            "kind",
            "team_id",
            "status",
            "next_step",
            "message",
        }

    def test_list_item_has_no_project_ref_or_paths(self, client, seeded_store):
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout", "project_ref": "ws-checkout"},
            headers=_auth(TOKEN_OWNER),
        )
        resp = client.get(LIST_URL, headers=_auth(TOKEN_MEMBER))
        item = resp.json()["requests"][0]
        # project_ref is a daemon-local reference and is never projected back.
        assert "project_ref" not in item

    def test_user_supplied_paths_never_reach_store(self, client, seeded_store):
        # Path-like inputs are rejected at the boundary (422), so nothing unsafe
        # is ever persisted.
        client.post(
            CREATE_URL,
            json={"display_name": "Checkout", "project_ref": "/Users/me/repo"},
            headers=_auth(TOKEN_OWNER),
        )
        pending = seeded_store.list_registration_requests_for_team(TEAM_ID)
        assert pending == []
