"""Pinned cross-repository contracts consumed by Bicameral Cloud."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, cast

_CONTRACT_PATH = Path(__file__).with_name("hosted_bridge_contract.json")
_FINGERPRINT_PATH = Path(__file__).with_name("hosted_bridge_contract.sha256")


def load_hosted_bridge_contract() -> dict[str, Any]:
    """Load the Bot-owned contract, refusing a stale or edited fingerprint."""
    content = _CONTRACT_PATH.read_bytes()
    actual = hashlib.sha256(content).hexdigest()
    expected = _FINGERPRINT_PATH.read_text().strip().split()[0]
    if actual != expected:
        raise RuntimeError(
            "pinned hosted-bridge contract fingerprint mismatch: "
            f"expected {expected}, got {actual}"
        )
    return cast(dict[str, Any], json.loads(content))


_HOSTED_BRIDGE_CONTRACT = load_hosted_bridge_contract()
HOSTED_BRIDGE_CONTRACT_FINGERPRINT = _FINGERPRINT_PATH.read_text().strip().split()[0]
HOSTED_BRIDGE_PROTOCOL_VERSION = int(
    _HOSTED_BRIDGE_CONTRACT["hosted_bridge_protocol_version"]
)
HOSTED_BRIDGE_MINIMUM_SUPPORTED_PROTOCOL_VERSION = int(
    _HOSTED_BRIDGE_CONTRACT["minimum_supported_protocol_version"]
)
LEGACY_CLOUD_ENVELOPE = cast(
    dict[str, int], _HOSTED_BRIDGE_CONTRACT["legacy_cloud_envelope"]
)

__all__ = [
    "HOSTED_BRIDGE_CONTRACT_FINGERPRINT",
    "HOSTED_BRIDGE_MINIMUM_SUPPORTED_PROTOCOL_VERSION",
    "HOSTED_BRIDGE_PROTOCOL_VERSION",
    "LEGACY_CLOUD_ENVELOPE",
    "load_hosted_bridge_contract",
]
