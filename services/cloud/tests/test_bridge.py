"""Tests for allowlisted bridge routing API (issue #20).

Acceptance criteria:
- Routing API validates team membership, session, origin/session binding,
  compatibility, schema version, and payload size.
- Allowlisted command fixtures route to daemon adapter/stub.
- Forbidden authority-bearing command fixtures are rejected before forwarding.
- Unknown command families are rejected by default.
- Tests cover allowed, forbidden, unknown, wrong-team, expired/revoked session,
  and incompatible daemon cases.
"""

from __future__ import annotations

from bicameral_cloud.bridge.allowlist import (
    ALLOWED_COMMAND_FAMILIES,
    FORBIDDEN_COMMAND_FAMILIES,
    MAX_PAYLOAD_BYTES,
)
from bicameral_cloud.bridge.routing import (
    ERR_FORBIDDEN_COMMAND,
    ERR_INCOMPATIBLE_DAEMON,
    ERR_INVALID_SESSION,
    ERR_PAIRING_INACTIVE,
    ERR_PAYLOAD_TOO_LARGE,
    ERR_SESSION_EXPIRED,
    ERR_TEAM_MISMATCH,
    ERR_UNKNOWN_COMMAND,
)
from tests.conftest import (
    PAIRING_ID,
    SESSION_ID,
    TEAM_ID,
    TOKEN_MEMBER,
    TOKEN_OUTSIDER,
)


def _bridge_payload(
    command_family: str = "projection.read",
    session_id: str = SESSION_ID,
    payload: dict | None = None,
) -> dict:
    return {
        "command_family": command_family,
        "session_id": session_id,
        "schema_version": "1.0",
        "payload": payload or {},
    }


class TestAllowedCommands:
    """Allowed dashboard-intent commands route successfully."""

    def test_projection_read_routes(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("projection.read"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["routed"] is True
        assert data["command_family"] == "projection.read"

    def test_all_allowed_families_route(self, client, seeded_store):
        for family in ALLOWED_COMMAND_FAMILIES:
            resp = client.post(
                "/api/v1/bridge/route",
                json=_bridge_payload(family),
                headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
            )
            assert resp.status_code == 200, (
                f"Allowed command '{family}' should route but got {resp.status_code}"
            )
            assert resp.json()["routed"] is True

    def test_routed_response_has_daemon_ack(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("readiness.refresh"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["daemon_response"]["status"] == "ack"


class TestForbiddenCommands:
    """Authority-bearing commands are rejected before forwarding."""

    def test_signoff_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("signoff.approve"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403
        detail = resp.json()["detail"]
        assert detail["error_code"] == ERR_FORBIDDEN_COMMAND
        assert detail["error_type"] == "authority_violation"

    def test_ledger_write_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("ledger.write"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403
        assert resp.json()["detail"]["error_code"] == ERR_FORBIDDEN_COMMAND

    def test_all_forbidden_families_rejected(self, client, seeded_store):
        for family in FORBIDDEN_COMMAND_FAMILIES:
            resp = client.post(
                "/api/v1/bridge/route",
                json=_bridge_payload(family),
                headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
            )
            assert resp.status_code == 403, (
                f"Forbidden command '{family}' should be rejected "
                f"but got {resp.status_code}"
            )
            assert resp.json()["detail"]["error_code"] == ERR_FORBIDDEN_COMMAND

    def test_source_binding_mutation_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("source_binding.mutate"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403

    def test_compliance_enforce_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("compliance.enforce"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 403


class TestUnknownCommands:
    """Unknown command families are rejected by default-deny policy."""

    def test_unknown_family_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("arbitrary.command"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 422
        detail = resp.json()["detail"]
        assert detail["error_code"] == ERR_UNKNOWN_COMMAND
        assert detail["error_type"] == "default_deny"

    def test_empty_family_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(""),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 422


class TestSessionValidation:
    """Session binding and lifecycle validation."""

    def test_invalid_session_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(session_id="nonexistent-session"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 404
        assert resp.json()["detail"]["error_code"] == ERR_INVALID_SESSION

    def test_expired_session_rejected(self, client, seeded_store):
        from bicameral_cloud.models.pairing import HostedSession, SessionStatus

        seeded_store.add_session(
            HostedSession(
                session_id="expired-session",
                user_id="user-1",
                team_id=TEAM_ID,
                pairing_id=PAIRING_ID,
                status=SessionStatus.EXPIRED,
            )
        )
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(session_id="expired-session"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 409
        assert resp.json()["detail"]["error_code"] == ERR_SESSION_EXPIRED

    def test_closed_session_rejected(self, client, seeded_store):
        from bicameral_cloud.models.pairing import HostedSession, SessionStatus

        seeded_store.add_session(
            HostedSession(
                session_id="closed-session",
                user_id="user-1",
                team_id=TEAM_ID,
                pairing_id=PAIRING_ID,
                status=SessionStatus.CLOSED,
            )
        )
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(session_id="closed-session"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 409


class TestTeamAuthorization:
    """Team membership enforcement for bridge routing."""

    def test_wrong_team_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(),
            headers={"Authorization": f"Bearer {TOKEN_OUTSIDER}"},
        )
        assert resp.status_code == 403
        assert resp.json()["detail"]["error_code"] == ERR_TEAM_MISMATCH

    def test_no_auth_rejected(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(),
        )
        assert resp.status_code == 422  # missing header


class TestPairingValidation:
    """Pairing state validation."""

    def test_revoked_pairing_rejected(self, client, seeded_store):
        from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus

        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="revoked-pairing",
                team_id=TEAM_ID,
                daemon_id="daemon-revoked",
                status=PairingStatus.REVOKED,
                daemon_version="1.0.0",
            )
        )
        from bicameral_cloud.models.pairing import HostedSession, SessionStatus

        seeded_store.add_session(
            HostedSession(
                session_id="session-revoked",
                user_id="user-1",
                team_id=TEAM_ID,
                pairing_id="revoked-pairing",
                status=SessionStatus.OPEN,
            )
        )
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(session_id="session-revoked"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 409
        assert resp.json()["detail"]["error_code"] == ERR_PAIRING_INACTIVE


class TestDaemonCompatibility:
    """Daemon compatibility enforcement for bridge routing."""

    def test_incompatible_daemon_rejected(self, client, seeded_store):
        from bicameral_cloud.models.pairing import (
            DaemonPairing,
            HostedSession,
            PairingStatus,
            SessionStatus,
        )

        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-old",
                team_id=TEAM_ID,
                daemon_id="daemon-old",
                status=PairingStatus.ACTIVE,
                daemon_version="0.1.0",
            )
        )
        seeded_store.add_session(
            HostedSession(
                session_id="session-old",
                user_id="user-1",
                team_id=TEAM_ID,
                pairing_id="pairing-old",
                status=SessionStatus.OPEN,
            )
        )
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(session_id="session-old"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 409
        assert resp.json()["detail"]["error_code"] == ERR_INCOMPATIBLE_DAEMON


class TestPayloadLimits:
    """Payload size validation."""

    def test_oversized_payload_rejected(self, client, seeded_store):
        big_payload = {"data": "x" * (MAX_PAYLOAD_BYTES + 1)}
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(payload=big_payload),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 400
        assert resp.json()["detail"]["error_code"] == ERR_PAYLOAD_TOO_LARGE

    def test_normal_payload_accepted(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload(payload={"key": "value"}),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200


class TestAuthorityBoundary:
    """Verify cloud never mints or conveys authority it doesn't own."""

    def test_routed_response_contains_no_authority_fields(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("projection.read"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        body = resp.text.lower()
        authority_indicators = [
            "signoff",
            "compliance",
            "ledger_write",
            "canonical",
            "binding_evidence",
            "decision_authority",
        ]
        for indicator in authority_indicators:
            assert indicator not in body, (
                f"Authority indicator '{indicator}' found in bridge response"
            )

    def test_error_response_includes_typed_error(self, client, seeded_store):
        resp = client.post(
            "/api/v1/bridge/route",
            json=_bridge_payload("signoff.approve"),
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        detail = resp.json()["detail"]
        assert "error_code" in detail
        assert "error_type" in detail
        assert "message" in detail
