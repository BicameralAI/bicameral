#!/usr/bin/env python3
"""Verify Cloud's vendored dashboard against the immutable Bot-published artifact."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import tarfile
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[1]
STATIC = ROOT / "src" / "bicameral_cloud" / "static" / "dashboard"
BOT_DESCRIPTOR = ROOT / "release-inputs" / "bot-release-descriptor.json"
GENERATED_DASHBOARD_METADATA = frozenset({"version.json", "release-identity.json"})
SHA = re.compile(r"^[a-f0-9]{40}$")
HEX_DIGEST = re.compile(r"^[a-f0-9]{64}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")


def canonical_digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def content_tree_digest(entries: Iterable[tuple[str, bytes]]) -> str:
    hasher = hashlib.sha256()
    seen = False
    for name, content in sorted(entries):
        seen = True
        hasher.update(name.encode())
        hasher.update(b"\0")
        hasher.update(content)
    if not seen:
        raise ValueError("dashboard tree is empty")
    return "sha256:" + hasher.hexdigest()


def directory_dashboard_digest(root: Path) -> str:
    entries: list[tuple[str, bytes]] = []
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        if relative in GENERATED_DASHBOARD_METADATA:
            continue
        entries.append((relative, path.read_bytes()))
    return content_tree_digest(entries)


def artifact_dashboard_entries(artifact: Path) -> tuple[list[tuple[str, bytes]], bytes]:
    prefix = "dashboard/dist/"
    descriptor_name = "release-dashboard/bot-release-descriptor.json"
    entries: list[tuple[str, bytes]] = []
    descriptor_bytes: bytes | None = None
    seen_names: set[str] = set()

    with tarfile.open(artifact, "r:gz") as archive:
        for member in archive.getmembers():
            if not member.isfile():
                continue
            if member.name in seen_names:
                raise ValueError(f"immutable dashboard artifact contains duplicate member {member.name}")
            seen_names.add(member.name)
            extracted = archive.extractfile(member)
            if extracted is None:
                raise ValueError(f"cannot read immutable dashboard member {member.name}")
            content = extracted.read()
            if member.name == descriptor_name:
                descriptor_bytes = content
            if member.name.startswith(prefix):
                relative = member.name.removeprefix(prefix)
                if not relative or relative in GENERATED_DASHBOARD_METADATA:
                    continue
                entries.append((relative, content))

    if descriptor_bytes is None:
        raise ValueError("immutable dashboard artifact omitted Bot release descriptor")
    return entries, descriptor_bytes


def validate_bot_descriptor(payload: object, expected_commit: str) -> list[str]:
    if not isinstance(payload, dict):
        return ["Bot release descriptor must be an object"]
    errors: list[str] = []
    if payload.get("schema_version") != 1 or payload.get("component") != "bot":
        errors.append("Bot release descriptor schema/component mismatch")
    if payload.get("commit") != expected_commit:
        errors.append("Bot release descriptor commit does not match expected Bot commit")
    artifacts = payload.get("artifacts")
    if not isinstance(artifacts, dict) or not DIGEST.fullmatch(
        str(artifacts.get("hosted_spa", ""))
    ):
        errors.append("Bot release descriptor lacks a valid hosted_spa digest")
    claimed = payload.get("descriptor_digest")
    unsigned = {key: value for key, value in payload.items() if key != "descriptor_digest"}
    if claimed != canonical_digest(unsigned):
        errors.append("Bot release descriptor digest does not bind descriptor content")
    return errors


def verify(
    root: Path,
    *,
    artifact: Path,
    expected_bot_commit: str,
) -> tuple[list[str], dict[str, Any] | None]:
    if not SHA.fullmatch(expected_bot_commit):
        return ["expected Bot commit must be a full lowercase git SHA"], None
    if not artifact.is_file():
        return ["immutable Bot dashboard artifact is missing"], None

    static = root / "src" / "bicameral_cloud" / "static" / "dashboard"
    checked_descriptor_path = root / "release-inputs" / "bot-release-descriptor.json"
    version_path = static / "version.json"
    if not version_path.is_file() or not checked_descriptor_path.is_file():
        return ["vendored dashboard metadata or Bot descriptor is missing"], None

    try:
        version = json.loads(version_path.read_text(encoding="utf-8"))
        checked_descriptor = json.loads(checked_descriptor_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError) as error:
        return [f"cannot parse vendored dashboard provenance: {error}"], None

    errors: list[str] = []
    if not isinstance(version, dict):
        return ["dashboard version metadata must be an object"], None
    if version.get("source_repository") != "BicameralAI/bicameral-bot":
        errors.append("dashboard source repository is not BicameralAI/bicameral-bot")
    if version.get("bot_commit") != expected_bot_commit:
        errors.append("dashboard version Bot commit does not match expected Bot commit")

    artifact_sha256 = hashlib.sha256(artifact.read_bytes()).hexdigest()
    recorded_artifact = version.get("artifact_sha256")
    if not isinstance(recorded_artifact, str) or not HEX_DIGEST.fullmatch(recorded_artifact):
        errors.append("dashboard version metadata lacks immutable artifact SHA-256")
    elif recorded_artifact != artifact_sha256:
        errors.append("downloaded immutable dashboard artifact digest does not match version metadata")

    try:
        artifact_entries, artifact_descriptor_bytes = artifact_dashboard_entries(artifact)
        artifact_descriptor = json.loads(artifact_descriptor_bytes.decode("utf-8"))
    except (ValueError, UnicodeDecodeError, json.JSONDecodeError, tarfile.TarError) as error:
        errors.append(str(error))
        return errors, None

    errors.extend(validate_bot_descriptor(artifact_descriptor, expected_bot_commit))
    errors.extend(validate_bot_descriptor(checked_descriptor, expected_bot_commit))

    if checked_descriptor != artifact_descriptor:
        errors.append("checked-in Bot release descriptor differs from immutable Bot artifact")

    descriptor_digest = artifact_descriptor.get("descriptor_digest")
    if version.get("bot_descriptor_digest") != descriptor_digest:
        errors.append("dashboard version Bot descriptor digest differs from immutable artifact")

    try:
        artifact_bundle = content_tree_digest(artifact_entries)
        vendored_bundle = directory_dashboard_digest(static)
    except ValueError as error:
        errors.append(str(error))
        return errors, None

    descriptor_artifacts = artifact_descriptor.get("artifacts")
    descriptor_bundle = (
        descriptor_artifacts.get("hosted_spa")
        if isinstance(descriptor_artifacts, dict)
        else None
    )
    if descriptor_bundle != artifact_bundle:
        errors.append("immutable Bot descriptor hosted_spa digest differs from artifact bytes")
    if vendored_bundle != artifact_bundle:
        errors.append("vendored Cloud dashboard differs from immutable Bot dashboard bytes")
    if "sha256:" + str(version.get("bundle_sha256", "")) != artifact_bundle:
        errors.append("dashboard version bundle digest differs from immutable Bot dashboard bytes")

    if errors:
        return errors, None

    receipt = {
        "schema_version": 1,
        "status": "verified",
        "bot_commit": expected_bot_commit,
        "bot_descriptor_digest": descriptor_digest,
        "immutable_dashboard_artifact_sha256": "sha256:" + artifact_sha256,
        "hosted_spa_sha256": artifact_bundle,
        "vendored_dashboard_sha256": vendored_bundle,
        "authority_ceiling": "release_provenance_evidence_only",
    }
    return [], receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--expected-bot-commit", required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    errors, receipt = verify(
        ROOT,
        artifact=args.artifact,
        expected_bot_commit=args.expected_bot_commit,
    )
    if errors:
        print("Served dashboard provenance rejected:")
        for error in errors:
            print(f"- {error}")
        return 1
    assert receipt is not None
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print(args.output)
    else:
        print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
