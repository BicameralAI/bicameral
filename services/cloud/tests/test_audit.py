"""Tests for coordination audit subsystem (issue #21, ADR-0006).

Verifies:
- Audit model and store layer
- Audit events emitted from auth/team/pairing/session/routing/compatibility
- Events scoped to team and session
- Forbidden fields (secrets, paths, ledger material) excluded
- Query endpoint requires team membership
"""

from __future__ import annotations

from bicameral_cloud.audit import emit
from bicameral_cloud.models.audit import (
    FORBIDDEN_AUDIT_FIELDS,
    AuditAction,
    AuditEvent,
    audit_event_to_response,
)

from .conftest import (
    PAIRING_ID,
    SESSION_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
    TOKEN_OWNER,
    USER_ID,
)

# --- audit status probe ---


def test_audit_status(client, store):
    resp = client.get("/api/v1/audit/status")
    assert resp.status_code == 200
    data = resp.json()
    assert data["audit"] == "enabled"
    assert data["status"] == "alpha"


def test_audit_status_no_auth_required(client, store):
    resp = client.get("/api/v1/audit/status")
    assert resp.status_code == 200


# --- audit model ---


def test_audit_event_has_required_fields():
    event = AuditEvent(
        action=AuditAction.TEAM_CREATED,
        actor_id="user-1",
        team_id="team-1",
    )
    assert event.event_id.startswith("audit-")
    assert event.action == AuditAction.TEAM_CREATED
    assert event.actor_id == "user-1"
    assert event.team_id == "team-1"
    assert event.session_id is None
    assert event.timestamp is not None


def test_audit_event_to_response_serialization():
    event = AuditEvent(
        action=AuditAction.SESSION_OPENED,
        actor_id="user-1",
        team_id="team-1",
        session_id="session-1",
        detail={"pairing_id": "pairing-1"},
    )
    resp = audit_event_to_response(event)
    assert resp.action == "session.opened"
    assert resp.team_id == "team-1"
    assert resp.session_id == "session-1"
    assert resp.detail["pairing_id"] == "pairing-1"


# --- store layer ---


def test_store_add_and_list_audit_events(store):
    event = AuditEvent(
        action=AuditAction.TEAM_CREATED,
        actor_id="user-1",
        team_id="team-alpha",
    )
    store.add_audit_event(event)
    events = store.list_audit_events_for_team("team-alpha")
    assert len(events) == 1
    assert events[0].event_id == event.event_id


def test_store_list_filters_by_team(store):
    store.add_audit_event(
        AuditEvent(action=AuditAction.TEAM_CREATED, actor_id="u1", team_id="t1")
    )
    store.add_audit_event(
        AuditEvent(action=AuditAction.TEAM_CREATED, actor_id="u2", team_id="t2")
    )
    assert len(store.list_audit_events_for_team("t1")) == 1
    assert len(store.list_audit_events_for_team("t2")) == 1
    assert len(store.list_audit_events_for_team("t3")) == 0


def test_store_list_filters_by_session(store):
    store.add_audit_event(
        AuditEvent(
            action=AuditAction.BRIDGE_ROUTED,
            actor_id="u1",
            team_id="t1",
            session_id="s1",
        )
    )
    store.add_audit_event(
        AuditEvent(
            action=AuditAction.BRIDGE_ROUTED,
            actor_id="u1",
            team_id="t1",
            session_id="s2",
        )
    )
    result = store.list_audit_events_for_team("t1", session_id="s1")
    assert len(result) == 1
    assert result[0].session_id == "s1"


def test_store_list_filters_by_action(store):
    store.add_audit_event(
        AuditEvent(action=AuditAction.TEAM_CREATED, actor_id="u1", team_id="t1")
    )
    store.add_audit_event(
        AuditEvent(
            action=AuditAction.TEAM_MEMBER_ADDED, actor_id="u1", team_id="t1"
        )
    )
    result = store.list_audit_events_for_team("t1", action="team.created")
    assert len(result) == 1


# --- emitter strips forbidden fields ---


def test_emit_strips_forbidden_fields(store):
    event = emit(
        store,
        action=AuditAction.TEAM_CREATED,
        actor_id="u1",
        team_id="t1",
        detail={
            "display_name": "My Team",
            "provider_secret": "SHOULD_BE_STRIPPED",
            "workspace_path": "/home/user/ws",
            "source_content": "secret code",
            "decision_ledger_payload": '{"events": []}',
            "signoff_state": "approved",
        },
    )
    assert "display_name" in event.detail
    for forbidden in FORBIDDEN_AUDIT_FIELDS:
        assert forbidden not in event.detail


def test_emit_preserves_safe_fields(store):
    event = emit(
        store,
        action=AuditAction.PAIRING_CREATED,
        actor_id="u1",
        team_id="t1",
        detail={"pairing_id": "p1", "daemon_id": "d1"},
    )
    assert event.detail["pairing_id"] == "p1"
    assert event.detail["daemon_id"] == "d1"


# --- audit query endpoint ---


def test_audit_query_requires_auth(client, store):
    resp = client.get(f"/api/v1/teams/{TEAM_ID}/audit")
    assert resp.status_code in (401, 403, 422)


def test_audit_query_requires_team_membership(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/audit",
        headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
    )
    assert resp.status_code == 403


def test_audit_query_returns_team_events(client, seeded_store):
    emit(
        seeded_store,
        action=AuditAction.TEAM_CREATED,
        actor_id=USER_ID,
        team_id=TEAM_ID,
        detail={"display_name": "Alpha Team"},
    )
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/audit",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["team_id"] == TEAM_ID
    assert data["count"] >= 1
    assert any(e["action"] == "team.created" for e in data["events"])


def test_audit_query_filters_by_session(client, seeded_store):
    emit(
        seeded_store,
        action=AuditAction.BRIDGE_ROUTED,
        actor_id=USER_ID,
        team_id=TEAM_ID,
        session_id=SESSION_ID,
        detail={"command_family": "projection.read"},
    )
    emit(
        seeded_store,
        action=AuditAction.BRIDGE_ROUTED,
        actor_id=USER_ID,
        team_id=TEAM_ID,
        session_id="other-session",
        detail={"command_family": "readiness.refresh"},
    )
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/audit",
        params={"session_id": SESSION_ID},
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["count"] == 1
    assert data["events"][0]["session_id"] == SESSION_ID


def test_audit_query_filters_by_action(client, seeded_store):
    emit(
        seeded_store,
        action=AuditAction.PAIRING_CREATED,
        actor_id=USER_ID,
        team_id=TEAM_ID,
    )
    emit(
        seeded_store,
        action=AuditAction.PAIRING_REVOKED,
        actor_id=USER_ID,
        team_id=TEAM_ID,
    )
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/audit",
        params={"action": "pairing.created"},
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert all(e["action"] == "pairing.created" for e in data["events"])


# --- route-level audit emission ---


def test_team_create_emits_audit(client, store):
    user_id = "user-creator"
    store.add_user(
        "tok-creator",
        __import__(
            "bicameral_cloud.models.tenancy", fromlist=["AuthenticatedUser"]
        ).AuthenticatedUser(
            user_id=user_id, email="creator@example.com"
        ),
    )
    resp = client.post(
        "/api/v1/teams",
        json={"display_name": "AuditTest"},
        headers={"Authorization": "Bearer tok-creator"},
    )
    assert resp.status_code == 200
    team_id = resp.json()["team_id"]
    events = store.list_audit_events_for_team(team_id)
    assert any(e.action == AuditAction.TEAM_CREATED for e in events)


def test_add_member_emits_audit(client, seeded_store):
    new_user_id = "user-new"
    seeded_store.upsert_user(
        __import__(
            "bicameral_cloud.models.tenancy", fromlist=["AuthenticatedUser"]
        ).AuthenticatedUser(
            user_id=new_user_id, email="new@example.com"
        ),
    )
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/members",
        json={"user_id": new_user_id},
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 200
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="team.member_added"
    )
    assert len(events) >= 1
    assert events[-1].detail.get("added_user_id") == new_user_id


def test_remove_member_emits_audit(client, seeded_store):
    resp = client.delete(
        f"/api/v1/teams/{TEAM_ID}/members/{USER_ID}",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 200
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="team.member_removed"
    )
    assert len(events) >= 1
    assert events[-1].detail.get("removed_user_id") == USER_ID


def test_create_pairing_emits_audit(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings",
        json={
            "pairing_id": "pairing-audit-test",
            "daemon_id": "daemon-audit-test",
        },
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 201
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="pairing.created"
    )
    assert len(events) >= 1
    assert events[-1].detail.get("pairing_id") == "pairing-audit-test"
    assert events[-1].detail.get("daemon_id") == "daemon-audit-test"


def test_revoke_pairing_emits_audit(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/pairings/{PAIRING_ID}/revoke",
        headers={"Authorization": f"Bearer {TOKEN_OWNER}"},
    )
    assert resp.status_code == 200
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="pairing.revoked"
    )
    assert len(events) >= 1


def test_open_session_emits_audit(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions",
        json={"pairing_id": PAIRING_ID},
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 201
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="session.opened"
    )
    assert len(events) >= 1
    assert events[-1].session_id is not None


def test_close_session_emits_audit(client, seeded_store):
    resp = client.post(
        f"/api/v1/teams/{TEAM_ID}/sessions/{SESSION_ID}/close",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="session.closed"
    )
    assert len(events) >= 1
    assert events[-1].session_id == SESSION_ID


def test_bridge_route_emits_audit(client, seeded_store):
    resp = client.post(
        "/api/v1/bridge/route",
        json={
            "command_family": "projection.read",
            "session_id": SESSION_ID,
            "payload": {},
        },
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="bridge.routed"
    )
    assert len(events) >= 1
    assert events[-1].session_id == SESSION_ID
    assert events[-1].detail.get("command_family") == "projection.read"


def test_compatibility_check_emits_audit(client, seeded_store):
    resp = client.get(
        f"/api/v1/teams/{TEAM_ID}/compatibility",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    events = seeded_store.list_audit_events_for_team(
        TEAM_ID, action="compatibility.checked"
    )
    assert len(events) >= 1
    assert "verdict" in events[-1].detail


def test_auth_session_emits_audit(client, seeded_store):
    resp = client.get(
        "/api/v1/auth/session",
        headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
    )
    assert resp.status_code == 200
    all_events = seeded_store._audit_events
    auth_events = [
        e for e in all_events if e.action == AuditAction.AUTH_SESSION_RESOLVED
    ]
    assert len(auth_events) >= 1


# --- cross-team isolation ---


def test_audit_events_isolated_across_teams(store):
    emit(store, action=AuditAction.TEAM_CREATED, actor_id="u1", team_id="team-A")
    emit(store, action=AuditAction.TEAM_CREATED, actor_id="u2", team_id="team-B")
    assert len(store.list_audit_events_for_team("team-A")) == 1
    assert len(store.list_audit_events_for_team("team-B")) == 1


# --- forbidden field exclusion across all event types ---


def test_no_audit_event_contains_forbidden_fields(seeded_store, client):
    """Trigger multiple audit-producing routes, verify no forbidden fields leak."""
    headers_member = {"Authorization": f"Bearer {TOKEN_MEMBER}"}

    # trigger various audited operations
    client.get("/api/v1/auth/session", headers=headers_member)
    client.get(f"/api/v1/teams/{TEAM_ID}/compatibility", headers=headers_member)
    client.post(
        "/api/v1/bridge/route",
        json={
            "command_family": "projection.read",
            "session_id": SESSION_ID,
            "payload": {},
        },
        headers=headers_member,
    )

    for event in seeded_store._audit_events:
        for key in event.detail:
            assert key not in FORBIDDEN_AUDIT_FIELDS, (
                f"Forbidden field '{key}' found in audit event {event.action}"
            )


# --- audit events never contain local authority payloads ---


def test_audit_events_exclude_local_authority_payloads(seeded_store, client):
    """Ensure audit detail payloads never carry local authority data."""
    local_authority_fields = {
        "signoff_state",
        "compliance_state",
        "binding_evidence",
        "decision_ledger_payload",
        "decision_event",
        "ledger_entry",
        "ledger_material",
    }

    headers = {"Authorization": f"Bearer {TOKEN_MEMBER}"}
    client.get("/api/v1/auth/session", headers=headers)
    client.get(f"/api/v1/teams/{TEAM_ID}/compatibility", headers=headers)
    client.post(
        "/api/v1/bridge/route",
        json={
            "command_family": "projection.read",
            "session_id": SESSION_ID,
            "payload": {},
        },
        headers=headers,
    )

    for event in seeded_store._audit_events:
        for key in event.detail:
            assert key not in local_authority_fields, (
                f"Local authority field '{key}' found in audit event {event.action}"
            )
