#!/usr/bin/env python3
"""Fail when the Bot-owned hosted-bridge contract drifts from its consumers."""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path


def _source_integer(source: str, name: str) -> int | None:
    match = re.search(rf"\b{name}\b[^=]*=\s*(\d+)", source)
    return int(match.group(1)) if match else None


def verify(root: Path) -> list[str]:
    errors: list[str] = []
    contract_path = root / "protocol/hosted-bridge/contract.json"
    fingerprint_path = root / "protocol/hosted-bridge/contract.sha256"

    contract_bytes = contract_path.read_bytes()
    contract = json.loads(contract_bytes)
    expected_fingerprint = hashlib.sha256(contract_bytes).hexdigest()
    recorded_fingerprint = fingerprint_path.read_text().strip().split()[0]
    if recorded_fingerprint != expected_fingerprint:
        errors.append("contract.sha256 does not match contract.json")

    protocol_version = contract["hosted_bridge_protocol_version"]
    minimum_version = contract["minimum_supported_protocol_version"]
    if minimum_version > protocol_version:
        errors.append("minimum supported protocol exceeds current protocol")

    fields = contract["capability_fields"]
    if fields.get("daemon_build_version", {}).get("compatibility_key") is not False:
        errors.append("daemon_build_version must remain diagnostic-only")
    if fields.get("hosted_bridge_protocol_version", {}).get("compatibility_key") is not True:
        errors.append("hosted_bridge_protocol_version must remain the compatibility key")

    rust = (root / "crates/bicameral-api/src/hosted_bridge.rs").read_text()
    typescript = (root / "dashboard/src/lib/hosted-bridge-types.ts").read_text()
    if _source_integer(rust, "HOSTED_BRIDGE_PROTOCOL_VERSION") != protocol_version:
        errors.append("Rust hosted-bridge protocol version differs from manifest")
    if _source_integer(typescript, "HOSTED_BRIDGE_PROTOCOL_VERSION") != protocol_version:
        errors.append("TypeScript hosted-bridge protocol version differs from manifest")

    rust_window = _source_integer(rust, "HOSTED_DASHBOARD_SUPPORT_WINDOW")
    if rust_window is None or protocol_version - rust_window != minimum_version:
        errors.append("Rust compatibility window differs from manifest")

    return errors


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    errors = verify(root)
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    fingerprint = hashlib.sha256(
        (root / "protocol/hosted-bridge/contract.json").read_bytes()
    ).hexdigest()
    print(f"hosted-bridge contract verified: {fingerprint}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
