#!/usr/bin/env python3
"""Fail when the Bot-owned external-ingest distribution contract drifts."""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path


def _source_integer(source: str, name: str) -> int | None:
    match = re.search(rf"\b{name}\b[^=]*=\s*(\d+)", source)
    return int(match.group(1)) if match else None


def _source_string(source: str, name: str) -> str | None:
    match = re.search(rf'\b{name}\b[^=]*=\s*"([^"]+)"', source)
    return match.group(1) if match else None


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(root: Path) -> list[str]:
    errors: list[str] = []
    contract_path = root / "protocol/external-ingest/contract.json"
    fingerprint_path = root / "protocol/external-ingest/contract.sha256"
    contract = json.loads(contract_path.read_text())

    expected_fingerprint = _sha256(contract_path)
    recorded_fingerprint = fingerprint_path.read_text().strip().split()[0]
    if recorded_fingerprint != expected_fingerprint:
        errors.append("contract.sha256 does not match contract.json")

    schema_path = root / contract["request_schema"]["path"]
    schema_hash = _sha256(schema_path)
    if contract["request_schema"]["sha256"] != schema_hash:
        errors.append("request schema digest differs from contract manifest")

    schema = json.loads(schema_path.read_text())
    required = set(schema.get("required", []))
    if "redaction_receipt" not in required:
        errors.append("v2 request schema must require redaction_receipt")
    receipt = schema.get("properties", {}).get("redaction_receipt", {})
    if receipt.get("additionalProperties") is not False:
        errors.append("redaction receipt schema must reject unknown fields")

    current = contract["protocol_version"]
    supported = contract["supported_protocol_versions"]
    minimum = contract["minimum_supported_protocol_version"]
    if current not in supported or min(supported) != minimum:
        errors.append("supported protocol versions do not match current/minimum")
    if contract["compatibility_policy"] != "exact_match_fail_closed":
        errors.append("external write path must remain exact-match fail-closed")

    rust = (root / "crates/bicameral-api/src/external_ingest.rs").read_text()
    if _source_integer(rust, "EXTERNAL_INGEST_PROTOCOL_VERSION") != current:
        errors.append("Rust external-ingest protocol version differs from manifest")
    if (
        _source_integer(rust, "MINIMUM_SUPPORTED_EXTERNAL_INGEST_PROTOCOL_VERSION")
        != minimum
    ):
        errors.append("Rust minimum supported protocol differs from manifest")
    for constant, key in [
        ("EXTERNAL_INGEST_CONTRACT_ID", "contract_id"),
        ("EXTERNAL_INGEST_DELIVERY_ENDPOINT", "delivery_endpoint"),
        ("EXTERNAL_INGEST_CAPABILITIES_ENDPOINT", "capabilities_endpoint"),
    ]:
        if _source_string(rust, constant) != contract[key]:
            errors.append(f"Rust {constant} differs from manifest")
    if _source_string(rust, "EXTERNAL_INGEST_REQUEST_SCHEMA_SHA256") != schema_hash:
        errors.append("Rust request schema digest differs from generated schema")
    if _source_string(rust, "EXTERNAL_INGEST_CONTRACT_FINGERPRINT") != expected_fingerprint:
        errors.append("Rust contract fingerprint differs from manifest")

    return errors


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    errors = verify(root)
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    print(
        "external-ingest contract verified: "
        f"{_sha256(root / 'protocol/external-ingest/contract.json')}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
