#!/usr/bin/env python3
"""Fail closed when a repository artifact drifts from an active contract pair."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any


def fingerprint(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def _repo_name(value: Any) -> str | None:
    if not isinstance(value, dict):
        return None
    owner = value.get("owner")
    name = value.get("name")
    if not isinstance(owner, str) or not isinstance(name, str):
        return None
    return f"{owner}/{name}"


def verify_repository(
    registry: dict[str, Any],
    *,
    contract_id: str,
    repository: str,
    repo_root: Path,
) -> list[str]:
    contracts = registry.get("contracts")
    if not isinstance(contracts, list):
        return ["registry contracts must be an array"]

    matches = [item for item in contracts if isinstance(item, dict) and item.get("contract_id") == contract_id]
    if len(matches) != 1:
        return [f"contract {contract_id!r} must resolve to exactly one registry entry"]

    contract = matches[0]
    if contract.get("status") != "active":
        return [f"contract {contract_id!r} is not active"]

    fingerprint_source = contract.get("fingerprint_source")
    if not isinstance(fingerprint_source, dict):
        return [f"{contract_id}: fingerprint_source must be an object"]
    kind = fingerprint_source.get("kind")
    if kind != "artifact_specific":
        return [f"{contract_id}: unsupported fingerprint kind {kind!r}"]
    if fingerprint_source.get("digest") != "sha256":
        return [f"{contract_id}: unsupported fingerprint digest {fingerprint_source.get('digest')!r}"]

    if repository == _repo_name(contract.get("producer_repo")):
        path = fingerprint_source.get("source_path")
        role = "producer"
    elif repository == _repo_name(contract.get("consumer_repo")):
        path = contract.get("artifact_location")
        role = "consumer"
    else:
        return [f"repository {repository!r} is not registered for contract {contract_id!r}"]

    if not isinstance(path, str) or not path:
        return [f"{contract_id}: {role} artifact path is missing"]

    resolved_root = repo_root.resolve()
    artifact = (repo_root / path).resolve()
    try:
        artifact.relative_to(resolved_root)
    except ValueError:
        return [f"{contract_id}: {role} artifact path escapes repository root: {path}"]
    if not artifact.is_file():
        return [f"{contract_id}: missing {role} artifact: {artifact}"]

    expected = contract.get("current_fingerprint")
    actual = fingerprint(artifact)
    if actual != expected:
        return [f"{contract_id}: {role} fingerprint mismatch: registry {expected}, computed {actual}"]
    return []


def verify_pair(
    registry: dict[str, Any],
    *,
    contract_id: str,
    producer_root: Path,
    consumer_root: Path,
) -> list[str]:
    contracts = registry.get("contracts")
    if not isinstance(contracts, list):
        return ["registry contracts must be an array"]
    matches = [item for item in contracts if isinstance(item, dict) and item.get("contract_id") == contract_id]
    if len(matches) != 1:
        return [f"contract {contract_id!r} must resolve to exactly one registry entry"]
    producer = _repo_name(matches[0].get("producer_repo"))
    consumer = _repo_name(matches[0].get("consumer_repo"))
    if producer is None or consumer is None:
        return [f"contract {contract_id!r} has invalid producer or consumer repository metadata"]
    return verify_repository(
        registry,
        contract_id=contract_id,
        repository=producer,
        repo_root=producer_root,
    ) + verify_repository(
        registry,
        contract_id=contract_id,
        repository=consumer,
        repo_root=consumer_root,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", required=True)
    parser.add_argument("--contract-id", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--repo-root", default=".")
    args = parser.parse_args(argv)

    registry = json.loads(Path(args.registry).read_text(encoding="utf-8"))
    problems = verify_repository(
        registry,
        contract_id=args.contract_id,
        repository=args.repository,
        repo_root=Path(args.repo_root),
    )
    for problem in problems:
        print(f"FAIL: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"OK: {args.repository} conforms to {args.contract_id}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
