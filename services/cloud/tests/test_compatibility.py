"""Tests for daemon compatibility support window (issue #18, ADR-0007).

Covers: current, previous-minor, too-old, newer-compatible, and newer-unknown.
"""

from __future__ import annotations

from bicameral_cloud.contracts import (
    HOSTED_BRIDGE_CONTRACT_FINGERPRINT,
    HOSTED_BRIDGE_PROTOCOL_VERSION,
    load_hosted_bridge_contract,
)
from bicameral_cloud.models.compatibility import (
    CURRENT_PROTOCOL_MAJOR,
    CURRENT_PROTOCOL_MINOR,
    MIN_SUPPORTED_MINOR,
    CompatibilityVerdict,
    DaemonCapabilities,
    MutableControlsState,
    evaluate_compatibility,
)
from bicameral_cloud.models.pairing import DaemonPairing, PairingStatus
from bicameral_cloud.models.tenancy import TeamMembership, TeamRole
from tests.conftest import OTHER_TEAM_ID, TEAM_ID, TOKEN_MEMBER

# ---------------------------------------------------------------------------
# Unit tests for evaluate_compatibility
# ---------------------------------------------------------------------------


def test_compatibility_constants_are_derived_from_pinned_bot_contract() -> None:
    contract = load_hosted_bridge_contract()

    assert HOSTED_BRIDGE_PROTOCOL_VERSION == 4
    assert contract["hosted_bridge_protocol_version"] == 4
    assert contract["minimum_supported_protocol_version"] == 3
    assert len(HOSTED_BRIDGE_CONTRACT_FINGERPRINT) == 64
    envelope = contract["legacy_cloud_envelope"]
    assert envelope["major"] == CURRENT_PROTOCOL_MAJOR
    assert envelope["current_minor"] == CURRENT_PROTOCOL_MINOR
    assert envelope["minimum_supported_minor"] == MIN_SUPPORTED_MINOR


class TestEvaluateCompatibilityCurrent:
    """Current daemon minor version is fully compatible."""

    def test_current_version_compatible(self) -> None:
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{CURRENT_PROTOCOL_MINOR}.0"
        )
        assert result.verdict == CompatibilityVerdict.COMPATIBLE
        assert result.mutable_controls == MutableControlsState.ENABLED

    def test_current_version_patch_variant(self) -> None:
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{CURRENT_PROTOCOL_MINOR}.5"
        )
        assert result.verdict == CompatibilityVerdict.COMPATIBLE
        assert result.mutable_controls == MutableControlsState.ENABLED


class TestEvaluateCompatibilityPreviousMinor:
    """Previous minor version is within the support window."""

    def test_previous_minor_compatible(self) -> None:
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{MIN_SUPPORTED_MINOR}.0"
        )
        assert result.verdict == CompatibilityVerdict.COMPATIBLE
        assert result.mutable_controls == MutableControlsState.ENABLED

    def test_previous_minor_with_patch(self) -> None:
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{MIN_SUPPORTED_MINOR}.9"
        )
        assert result.verdict == CompatibilityVerdict.COMPATIBLE
        assert result.mutable_controls == MutableControlsState.ENABLED


class TestEvaluateCompatibilityTooOld:
    """Daemon below the support window returns upgrade-required + blocked."""

    def test_too_old_minor(self) -> None:
        too_old_minor = MIN_SUPPORTED_MINOR - 1
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{too_old_minor}.0"
        )
        assert result.verdict == CompatibilityVerdict.UPGRADE_REQUIRED
        assert result.mutable_controls == MutableControlsState.BLOCKED
        assert result.message is not None
        assert "upgrade" in result.message.lower()

    def test_old_major_version(self) -> None:
        result = evaluate_compatibility("0.9.0")
        assert result.verdict == CompatibilityVerdict.UPGRADE_REQUIRED
        assert result.mutable_controls == MutableControlsState.BLOCKED

    def test_none_version(self) -> None:
        result = evaluate_compatibility(None)
        assert result.verdict == CompatibilityVerdict.UPGRADE_REQUIRED
        assert result.mutable_controls == MutableControlsState.BLOCKED

    def test_unparseable_version(self) -> None:
        result = evaluate_compatibility("garbage")
        assert result.verdict == CompatibilityVerdict.UPGRADE_REQUIRED
        assert result.mutable_controls == MutableControlsState.BLOCKED


class TestEvaluateCompatibilityNewerCompatible:
    """Newer daemon with compatible capabilities is fully compatible."""

    def test_newer_minor_with_capabilities(self) -> None:
        newer_minor = CURRENT_PROTOCOL_MINOR + 1
        caps = DaemonCapabilities(
            supports_protocol_major=CURRENT_PROTOCOL_MAJOR,
            supports_protocol_minor_min=MIN_SUPPORTED_MINOR,
            supports_protocol_minor_max=newer_minor,
        )
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{newer_minor}.0",
            capabilities=caps,
        )
        assert result.verdict == CompatibilityVerdict.COMPATIBLE
        assert result.mutable_controls == MutableControlsState.ENABLED

    def test_newer_major_with_backwards_compatible_caps(self) -> None:
        caps = DaemonCapabilities(
            supports_protocol_major=CURRENT_PROTOCOL_MAJOR,
            supports_protocol_minor_min=0,
            supports_protocol_minor_max=CURRENT_PROTOCOL_MINOR + 5,
        )
        result = evaluate_compatibility("2.0.0", capabilities=caps)
        assert result.verdict == CompatibilityVerdict.COMPATIBLE
        assert result.mutable_controls == MutableControlsState.ENABLED


class TestEvaluateCompatibilityNewerUnknown:
    """Newer daemon without capabilities degrades controls."""

    def test_newer_minor_no_capabilities(self) -> None:
        newer_minor = CURRENT_PROTOCOL_MINOR + 1
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{newer_minor}.0"
        )
        assert result.verdict == CompatibilityVerdict.DEGRADED
        assert result.mutable_controls == MutableControlsState.DISABLED
        assert result.message is not None
        assert "disabled" in result.message.lower()

    def test_newer_major_no_capabilities(self) -> None:
        result = evaluate_compatibility("2.0.0")
        assert result.verdict == CompatibilityVerdict.DEGRADED
        assert result.mutable_controls == MutableControlsState.DISABLED

    def test_newer_minor_incompatible_capabilities(self) -> None:
        newer_minor = CURRENT_PROTOCOL_MINOR + 1
        caps = DaemonCapabilities(
            supports_protocol_major=CURRENT_PROTOCOL_MAJOR + 1,
            supports_protocol_minor_min=0,
            supports_protocol_minor_max=5,
        )
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{newer_minor}.0",
            capabilities=caps,
        )
        assert result.verdict == CompatibilityVerdict.DEGRADED
        assert result.mutable_controls == MutableControlsState.DISABLED

    def test_newer_caps_range_does_not_cover_cloud(self) -> None:
        newer_minor = CURRENT_PROTOCOL_MINOR + 2
        caps = DaemonCapabilities(
            supports_protocol_major=CURRENT_PROTOCOL_MAJOR,
            supports_protocol_minor_min=CURRENT_PROTOCOL_MINOR + 1,
            supports_protocol_minor_max=newer_minor,
        )
        result = evaluate_compatibility(
            f"{CURRENT_PROTOCOL_MAJOR}.{newer_minor}.0",
            capabilities=caps,
        )
        assert result.verdict == CompatibilityVerdict.DEGRADED
        assert result.mutable_controls == MutableControlsState.DISABLED


# ---------------------------------------------------------------------------
# Integration tests via the API route
# ---------------------------------------------------------------------------


class TestCompatibilityRouteCurrent:
    """Route returns compatible for current-version daemon."""

    def test_current_daemon_compatible(self, client, seeded_store) -> None:
        resp = client.get(
            f"/api/v1/teams/{TEAM_ID}/compatibility",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["verdict"] == "compatible"
        assert data["mutable_controls"] == "enabled"
        assert data["team_id"] == TEAM_ID


class TestCompatibilityRouteNoPairing:
    """Route returns blocked state when no pairing exists."""

    def test_no_pairing_blocked(self, client, seeded_store) -> None:
        seeded_store.add_membership(
            TeamMembership(
                user_id="user-1", team_id=OTHER_TEAM_ID, role=TeamRole.MEMBER
            )
        )
        resp = client.get(
            f"/api/v1/teams/{OTHER_TEAM_ID}/compatibility",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["verdict"] == "upgrade_required"
        assert data["mutable_controls"] == "blocked"


class TestCompatibilityRouteTooOld:
    """Route returns upgrade-required for too-old daemon."""

    def test_too_old_daemon(self, client, seeded_store) -> None:
        too_old_minor = MIN_SUPPORTED_MINOR - 1
        seeded_store.add_membership(
            TeamMembership(
                user_id="user-1", team_id=OTHER_TEAM_ID, role=TeamRole.MEMBER
            )
        )
        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-old",
                team_id=OTHER_TEAM_ID,
                daemon_id="daemon-old",
                status=PairingStatus.ACTIVE,
                daemon_version=f"{CURRENT_PROTOCOL_MAJOR}.{too_old_minor}.0",
            )
        )
        resp = client.get(
            f"/api/v1/teams/{OTHER_TEAM_ID}/compatibility",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["verdict"] == "upgrade_required"
        assert data["mutable_controls"] == "blocked"
        assert data["message"] is not None


class TestCompatibilityRouteNewerDaemon:
    """Route returns appropriate state for newer daemons."""

    def test_newer_without_capabilities_degraded(
        self, client, seeded_store
    ) -> None:
        newer_minor = CURRENT_PROTOCOL_MINOR + 1
        seeded_store.add_membership(
            TeamMembership(
                user_id="user-1", team_id=OTHER_TEAM_ID, role=TeamRole.MEMBER
            )
        )
        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-new",
                team_id=OTHER_TEAM_ID,
                daemon_id="daemon-new",
                status=PairingStatus.ACTIVE,
                daemon_version=f"{CURRENT_PROTOCOL_MAJOR}.{newer_minor}.0",
            )
        )
        resp = client.get(
            f"/api/v1/teams/{OTHER_TEAM_ID}/compatibility",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["verdict"] == "degraded"
        assert data["mutable_controls"] == "disabled"

    def test_newer_with_compatible_capabilities(
        self, client, seeded_store
    ) -> None:
        newer_minor = CURRENT_PROTOCOL_MINOR + 1
        seeded_store.add_membership(
            TeamMembership(
                user_id="user-1", team_id=OTHER_TEAM_ID, role=TeamRole.MEMBER
            )
        )
        seeded_store.add_pairing(
            DaemonPairing(
                pairing_id="pairing-new-compat",
                team_id=OTHER_TEAM_ID,
                daemon_id="daemon-new-compat",
                status=PairingStatus.ACTIVE,
                daemon_version=f"{CURRENT_PROTOCOL_MAJOR}.{newer_minor}.0",
                capabilities=DaemonCapabilities(
                    supports_protocol_major=CURRENT_PROTOCOL_MAJOR,
                    supports_protocol_minor_min=MIN_SUPPORTED_MINOR,
                    supports_protocol_minor_max=newer_minor,
                ),
            )
        )
        resp = client.get(
            f"/api/v1/teams/{OTHER_TEAM_ID}/compatibility",
            headers={"Authorization": f"Bearer {TOKEN_MEMBER}"},
        )
        assert resp.status_code == 200
        data = resp.json()
        assert data["verdict"] == "compatible"
        assert data["mutable_controls"] == "enabled"
