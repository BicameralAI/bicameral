"""Daemon compatibility evaluation models (ADR-0007).

Alpha supports the current compatible protocol envelope plus one previous
daemon minor version. Too-old daemons receive a blocking upgrade state.
Newer daemons are usable only when capability negotiation proves the
required action contract is compatible; otherwise mutable controls are
disabled or blocked.
"""

from __future__ import annotations

import enum
import re

from pydantic import BaseModel

from bicameral_cloud.contracts import LEGACY_CLOUD_ENVELOPE


class CompatibilityVerdict(enum.StrEnum):
    """Outcome of daemon protocol compatibility evaluation."""

    COMPATIBLE = "compatible"
    UPGRADE_REQUIRED = "upgrade_required"
    DEGRADED = "degraded"


class MutableControlsState(enum.StrEnum):
    """State of cloud UI/API mutable controls for this daemon."""

    ENABLED = "enabled"
    DISABLED = "disabled"
    BLOCKED = "blocked"


# Alpha supported protocol range.
# Window: [MAJOR].[MIN_SUPPORTED_MINOR .. CURRENT_PROTOCOL_MINOR]
CURRENT_PROTOCOL_MAJOR = LEGACY_CLOUD_ENVELOPE["major"]
CURRENT_PROTOCOL_MINOR = LEGACY_CLOUD_ENVELOPE["current_minor"]
MIN_SUPPORTED_MINOR = LEGACY_CLOUD_ENVELOPE["minimum_supported_minor"]


class DaemonCapabilities(BaseModel):
    """Capability report from a daemon for compatibility negotiation.

    A newer daemon may advertise backwards-compatible protocol support.
    Cloud uses this to decide whether mutable controls are safe to enable.
    """

    supports_protocol_major: int
    supports_protocol_minor_min: int
    supports_protocol_minor_max: int


class CompatibilityResult(BaseModel):
    """Full result of daemon compatibility evaluation."""

    daemon_version: str | None
    verdict: CompatibilityVerdict
    mutable_controls: MutableControlsState
    cloud_protocol: str
    message: str | None = None


def evaluate_compatibility(
    daemon_version: str | None,
    capabilities: DaemonCapabilities | None = None,
) -> CompatibilityResult:
    """Evaluate daemon protocol compatibility for alpha.

    Rules (ADR-0007):
    - Current minor and one previous minor are fully compatible.
    - Older than the support window: upgrade required, controls blocked.
    - Newer with compatible capabilities: compatible, controls enabled.
    - Newer without capabilities or incompatible: degraded, controls disabled.
    """
    cloud_protocol = f"{CURRENT_PROTOCOL_MAJOR}.{CURRENT_PROTOCOL_MINOR}"

    if daemon_version is None:
        return CompatibilityResult(
            daemon_version=None,
            verdict=CompatibilityVerdict.UPGRADE_REQUIRED,
            mutable_controls=MutableControlsState.BLOCKED,
            cloud_protocol=cloud_protocol,
            message="Daemon version unknown. Upgrade required.",
        )

    match = re.match(r"^(\d+)\.(\d+)", daemon_version)
    if not match:
        return CompatibilityResult(
            daemon_version=daemon_version,
            verdict=CompatibilityVerdict.UPGRADE_REQUIRED,
            mutable_controls=MutableControlsState.BLOCKED,
            cloud_protocol=cloud_protocol,
            message="Daemon version unrecognized. Upgrade required.",
        )

    major, minor = int(match.group(1)), int(match.group(2))

    # Old major: upgrade required
    if major < CURRENT_PROTOCOL_MAJOR:
        return CompatibilityResult(
            daemon_version=daemon_version,
            verdict=CompatibilityVerdict.UPGRADE_REQUIRED,
            mutable_controls=MutableControlsState.BLOCKED,
            cloud_protocol=cloud_protocol,
            message=(
                f"Daemon protocol {major}.{minor} is below the supported "
                f"range ({CURRENT_PROTOCOL_MAJOR}.{MIN_SUPPORTED_MINOR}+). "
                "Please upgrade the daemon."
            ),
        )

    # Same major, below support window: upgrade required
    if major == CURRENT_PROTOCOL_MAJOR and minor < MIN_SUPPORTED_MINOR:
        return CompatibilityResult(
            daemon_version=daemon_version,
            verdict=CompatibilityVerdict.UPGRADE_REQUIRED,
            mutable_controls=MutableControlsState.BLOCKED,
            cloud_protocol=cloud_protocol,
            message=(
                f"Daemon protocol {major}.{minor} is below the supported "
                f"minor version window ({MIN_SUPPORTED_MINOR}–"
                f"{CURRENT_PROTOCOL_MINOR}). Please upgrade the daemon."
            ),
        )

    # Same major, within support window: fully compatible
    if major == CURRENT_PROTOCOL_MAJOR and minor <= CURRENT_PROTOCOL_MINOR:
        return CompatibilityResult(
            daemon_version=daemon_version,
            verdict=CompatibilityVerdict.COMPATIBLE,
            mutable_controls=MutableControlsState.ENABLED,
            cloud_protocol=cloud_protocol,
        )

    # Newer daemon (higher major or higher minor within same major):
    # check capability negotiation
    if capabilities is not None and _capabilities_cover_cloud_protocol(capabilities):
        return CompatibilityResult(
            daemon_version=daemon_version,
            verdict=CompatibilityVerdict.COMPATIBLE,
            mutable_controls=MutableControlsState.ENABLED,
            cloud_protocol=cloud_protocol,
            message="Newer daemon with compatible capabilities.",
        )

    # Newer without compatible capabilities: degrade
    return CompatibilityResult(
        daemon_version=daemon_version,
        verdict=CompatibilityVerdict.DEGRADED,
        mutable_controls=MutableControlsState.DISABLED,
        cloud_protocol=cloud_protocol,
        message=(
            f"Daemon protocol {major}.{minor} is newer than cloud "
            f"({cloud_protocol}). Mutable controls disabled until "
            "capability negotiation confirms compatibility."
        ),
    )


def _capabilities_cover_cloud_protocol(caps: DaemonCapabilities) -> bool:
    """Check if daemon capabilities cover the cloud's current protocol."""
    if caps.supports_protocol_major != CURRENT_PROTOCOL_MAJOR:
        return False
    return (
        caps.supports_protocol_minor_min <= CURRENT_PROTOCOL_MINOR
        and caps.supports_protocol_minor_max >= CURRENT_PROTOCOL_MINOR
    )
