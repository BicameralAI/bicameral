#!/usr/bin/env python3
"""Emit and verify the immutable Bot inputs used by a release manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SHA = re.compile(r"^[a-f0-9]{40}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")


def sha256_bytes(content: bytes) -> str:
    return "sha256:" + hashlib.sha256(content).hexdigest()


def sha256_file(path: Path) -> str:
    if not path.is_file():
        raise ValueError(f"required release artifact is missing: {path}")
    return sha256_bytes(path.read_bytes())


def tree_digest(root: Path) -> str:
    if not root.is_dir():
        raise ValueError(f"required release artifact tree is missing: {root}")
    files = sorted(path for path in root.rglob("*") if path.is_file())
    if not files:
        raise ValueError(f"required release artifact tree is empty: {root}")
    hasher = hashlib.sha256()
    for path in files:
        hasher.update(path.relative_to(root).as_posix().encode())
        hasher.update(b"\0")
        hasher.update(path.read_bytes())
    return "sha256:" + hasher.hexdigest()


def git_commit() -> str:
    return (
        os.environ.get("RELEASE_SOURCE_COMMIT")
        or os.environ.get("GITHUB_SHA")
        or subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()
    )


def canonical_digest(payload: object) -> str:
    return sha256_bytes(
        json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    )


def build_descriptor(*, commit: str, daemon_binary: Path) -> dict[str, Any]:
    if not SHA.fullmatch(commit):
        raise ValueError("release source commit must be a full lowercase git SHA")
    descriptor: dict[str, Any] = {
        "schema_version": 1,
        "component": "bot",
        "commit": commit,
        "artifacts": {
            "daemon": sha256_file(daemon_binary),
            "hosted_spa": tree_digest(ROOT / "dashboard" / "dist"),
        },
        "interfaces": {
            "hosted_bridge": tree_digest(ROOT / "protocol" / "hosted-bridge"),
            "integration_ingress": tree_digest(
                ROOT / "crates" / "bicameral-gateway" / "src"
            ),
            "mcp_tool_request": tree_digest(
                ROOT / "crates" / "bicameral-api" / "src"
            ),
        },
    }
    descriptor["descriptor_digest"] = canonical_digest(descriptor)
    return descriptor


def validate_descriptor(payload: object) -> list[str]:
    if not isinstance(payload, dict):
        return ["descriptor must be an object"]
    errors: list[str] = []
    expected = {
        "schema_version",
        "component",
        "commit",
        "artifacts",
        "interfaces",
        "descriptor_digest",
    }
    if set(payload) != expected:
        errors.append(f"descriptor keys must be exactly {sorted(expected)}")
    if payload.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if payload.get("component") != "bot":
        errors.append("component must be bot")
    if not isinstance(payload.get("commit"), str) or not SHA.fullmatch(
        payload["commit"]
    ):
        errors.append("commit must be a full lowercase git SHA")
    for section, names in {
        "artifacts": {"daemon", "hosted_spa"},
        "interfaces": {"hosted_bridge", "integration_ingress", "mcp_tool_request"},
    }.items():
        values = payload.get(section)
        if not isinstance(values, dict) or set(values) != names:
            errors.append(f"{section} keys must be exactly {sorted(names)}")
            continue
        for name, value in values.items():
            if not isinstance(value, str) or not DIGEST.fullmatch(value):
                errors.append(f"{section}.{name} must be sha256:<64 lowercase hex>")
    claimed = payload.get("descriptor_digest")
    unsigned = {key: value for key, value in payload.items() if key != "descriptor_digest"}
    if claimed != canonical_digest(unsigned):
        errors.append("descriptor_digest does not bind the descriptor")
    return errors


def validate_hosted_bundle() -> list[str]:
    dist = ROOT / "dashboard" / "dist"
    scripts = sorted((dist / "assets").glob("*.js"))
    if not scripts:
        return ["built hosted SPA has no JavaScript assets"]
    content = b"\n".join(path.read_bytes() for path in scripts)
    errors: list[str] = []
    for marker in (
        b"/api/v1/hosted-bridge/pair",
        b"/api/v1/hosted-bridge/status",
        b"bicameral_hosted_bridge",
    ):
        if marker not in content:
            errors.append(f"built hosted SPA lacks real pairing marker {marker.decode()}")
    for marker in (
        b"fixture-project-alpha",
        b"fixture-project-beta",
        b"fixture-decision-001",
    ):
        if marker in content:
            errors.append(f"built hosted SPA contains forbidden fixture marker {marker.decode()}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--daemon-binary", type=Path, default=ROOT / "target" / "release" / "bicameral"
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "release-dashboard" / "bot-release-descriptor.json",
    )
    parser.add_argument("--verify", type=Path)
    parser.add_argument("--verify-hosted-bundle", action="store_true")
    args = parser.parse_args()

    errors: list[str] = []
    if args.verify:
        errors.extend(validate_descriptor(json.loads(args.verify.read_text())))
    else:
        payload = build_descriptor(commit=git_commit(), daemon_binary=args.daemon_binary)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(args.output)
    if args.verify_hosted_bundle:
        errors.extend(validate_hosted_bundle())
    if errors:
        print("Bot release provenance rejected:")
        for error in errors:
            print(f"- {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
