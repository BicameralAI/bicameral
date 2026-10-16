#!/usr/bin/env python3
"""Fail a Bot release unless production Cloud serves byte-bound Bot provenance."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import sys
import tarfile
from pathlib import Path
from typing import Any, Iterable
from urllib.parse import urlencode
from urllib.request import urlopen

DEFAULT_VERSION_URL = "https://app.bicameral-ai.com/app/version.json"
DEFAULT_IDENTITY_URL = "https://app.bicameral-ai.com/app/release-identity.json"
FINGERPRINT_PATTERN = re.compile(r"^[0-9a-f]{64}$")
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")
MAX_ARTIFACT_BYTES = 64 * 1024 * 1024


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
        raise ValueError("immutable Bot dashboard tree is empty")
    return "sha256:" + hasher.hexdigest()


def _read_url(opener: Any, url: str, *, timeout: int, limit: int) -> bytes:
    with opener(url, timeout=timeout) as response:
        content = response.read(limit + 1)
    if len(content) > limit:
        raise ValueError(f"response exceeded {limit} byte limit")
    if not content:
        raise ValueError("response was empty")
    return content


def _artifact_expectations(content: bytes, expected_bot_commit: str) -> dict[str, str]:
    descriptor_name = "release-dashboard/bot-release-descriptor.json"
    dashboard_prefix = "dashboard/dist/"
    descriptor_bytes: bytes | None = None
    dashboard_entries: list[tuple[str, bytes]] = []
    seen_names: set[str] = set()

    try:
        with tarfile.open(fileobj=io.BytesIO(content), mode="r:gz") as archive:
            for member in archive.getmembers():
                if not member.isfile():
                    continue
                if member.name in seen_names:
                    raise ValueError(
                        f"immutable Bot dashboard artifact contains duplicate member {member.name}"
                    )
                seen_names.add(member.name)
                extracted = archive.extractfile(member)
                if extracted is None:
                    raise ValueError(f"cannot read immutable artifact member {member.name}")
                body = extracted.read()
                if member.name == descriptor_name:
                    descriptor_bytes = body
                elif member.name.startswith(dashboard_prefix):
                    relative = member.name.removeprefix(dashboard_prefix)
                    if relative:
                        dashboard_entries.append((relative, body))
    except tarfile.TarError as error:
        raise ValueError(
            f"immutable Bot dashboard artifact is not a valid tarball: {error}"
        ) from error

    if descriptor_bytes is None:
        raise ValueError("immutable Bot dashboard artifact omitted release descriptor")
    try:
        descriptor = json.loads(descriptor_bytes.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("immutable Bot release descriptor is not valid JSON") from error
    if not isinstance(descriptor, dict):
        raise ValueError("immutable Bot release descriptor must be an object")
    if descriptor.get("schema_version") != 1 or descriptor.get("component") != "bot":
        raise ValueError("immutable Bot release descriptor schema/component mismatch")
    if descriptor.get("commit") != expected_bot_commit:
        raise ValueError("immutable Bot release descriptor commit does not match release")
    claimed = descriptor.get("descriptor_digest")
    unsigned = {
        key: value for key, value in descriptor.items() if key != "descriptor_digest"
    }
    actual_descriptor_digest = canonical_digest(unsigned)
    if claimed != actual_descriptor_digest:
        raise ValueError("immutable Bot release descriptor digest does not bind content")
    artifacts = descriptor.get("artifacts")
    if not isinstance(artifacts, dict):
        raise ValueError("immutable Bot release descriptor omitted artifacts")
    hosted_spa = content_tree_digest(dashboard_entries)
    if artifacts.get("hosted_spa") != hosted_spa:
        raise ValueError("immutable Bot descriptor hosted_spa does not match artifact bytes")

    return {
        "artifact_sha256": "sha256:" + hashlib.sha256(content).hexdigest(),
        "descriptor_digest": actual_descriptor_digest,
        "hosted_spa": hosted_spa,
    }


def _cache_busted(url: str, query: dict[str, str]) -> str:
    separator = "&" if "?" in url else "?"
    return f"{url}{separator}{urlencode(query)}"


def _load_json_response(opener: Any, url: str) -> dict[str, Any]:
    content = _read_url(opener, url, timeout=10, limit=2 * 1024 * 1024)
    value = json.loads(content)
    if not isinstance(value, dict):
        raise ValueError("response must be a JSON object")
    return value


def verify(
    root: Path,
    *,
    expected_bot_commit: str,
    immutable_bot_artifact_bytes: bytes,
    version_url: str = DEFAULT_VERSION_URL,
    identity_url: str = DEFAULT_IDENTITY_URL,
    opener=urlopen,
) -> list[str]:
    if not COMMIT_PATTERN.fullmatch(expected_bot_commit):
        return ["expected Bot commit must be a full lowercase git SHA"]
    expected_bridge = (
        root / "protocol" / "hosted-bridge" / "contract.sha256"
    ).read_text().split()[0]
    if not FINGERPRINT_PATTERN.fullmatch(expected_bridge):
        return ["Bot hosted-bridge contract fingerprint is malformed"]

    try:
        expected = _artifact_expectations(
            immutable_bot_artifact_bytes, expected_bot_commit
        )
    except Exception as error:
        return [f"could not verify immutable Bot dashboard artifact: {error}"]

    query = {
        "release_contract": expected_bridge,
        "release_bot_commit": expected_bot_commit,
        "release_dashboard": expected["hosted_spa"],
    }
    try:
        version = _load_json_response(opener, _cache_busted(version_url, query))
    except Exception as error:
        return [f"could not read production Cloud version metadata: {error}"]
    try:
        identity = _load_json_response(opener, _cache_busted(identity_url, query))
    except Exception as error:
        return [f"could not read production Cloud release identity: {error}"]

    errors: list[str] = []

    deployed_bridge = version.get("hosted_bridge_contract_sha256")
    if not isinstance(deployed_bridge, str) or not FINGERPRINT_PATTERN.fullmatch(
        deployed_bridge
    ):
        errors.append(
            "production Cloud does not advertise a valid hosted-bridge contract fingerprint"
        )
    elif deployed_bridge != expected_bridge:
        errors.append(
            "production Cloud hosted-bridge contract does not match the Bot release: "
            f"expected {expected_bridge}, deployed {deployed_bridge}"
        )

    deployed_commit = version.get("bot_commit")
    if not isinstance(deployed_commit, str) or not COMMIT_PATTERN.fullmatch(
        deployed_commit
    ):
        errors.append("production Cloud does not advertise a valid Bot commit")
    elif deployed_commit != expected_bot_commit:
        errors.append(
            "production Cloud Bot commit does not match the release: "
            f"expected {expected_bot_commit}, deployed {deployed_commit}"
        )

    if "sha256:" + str(version.get("artifact_sha256", "")) != expected[
        "artifact_sha256"
    ]:
        errors.append(
            "production Cloud immutable dashboard artifact digest does not match Bot"
        )
    if version.get("bot_descriptor_digest") != expected["descriptor_digest"]:
        errors.append(
            "production Cloud Bot descriptor digest does not match immutable Bot artifact"
        )
    if "sha256:" + str(version.get("bundle_sha256", "")) != expected["hosted_spa"]:
        errors.append(
            "production Cloud version bundle digest does not match immutable Bot bytes"
        )

    if identity.get("schema_version") != 1:
        errors.append("production Cloud release identity schema_version must be 1")
    components = identity.get("components")
    if not isinstance(components, dict):
        errors.append("production Cloud release identity omitted components")
        return errors
    bot = components.get("bot")
    cloud = components.get("cloud")
    if not isinstance(bot, dict) or not isinstance(cloud, dict):
        errors.append("production Cloud release identity omitted Bot or Cloud component")
        return errors

    if bot.get("commit") != expected_bot_commit:
        errors.append("served release identity Bot commit does not match release")
    if bot.get("descriptor_digest") != expected["descriptor_digest"]:
        errors.append(
            "served release identity Bot descriptor does not match immutable artifact"
        )
    bot_artifacts = bot.get("artifacts")
    if not isinstance(bot_artifacts, dict) or bot_artifacts.get(
        "hosted_spa"
    ) != expected["hosted_spa"]:
        errors.append(
            "served release identity Bot hosted_spa does not match immutable artifact"
        )

    cloud_artifacts = cloud.get("artifacts")
    if not isinstance(cloud_artifacts, dict):
        errors.append("served release identity Cloud component omitted artifacts")
        return errors
    if cloud_artifacts.get("hosted_spa") != expected["hosted_spa"]:
        errors.append("served Cloud hosted_spa does not match immutable Bot bytes")
    if cloud_artifacts.get("immutable_bot_dashboard") != expected["artifact_sha256"]:
        errors.append(
            "served Cloud identity lacks exact immutable Bot dashboard artifact proof"
        )
    if cloud_artifacts.get("bot_release_descriptor") != expected["descriptor_digest"]:
        errors.append("served Cloud identity lacks exact immutable Bot descriptor proof")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-bot-commit", required=True)
    parser.add_argument("--immutable-bot-artifact", type=Path, required=True)
    parser.add_argument("--version-url", default=DEFAULT_VERSION_URL)
    parser.add_argument("--identity-url", default=DEFAULT_IDENTITY_URL)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        artifact_bytes = args.immutable_bot_artifact.read_bytes()
    except OSError as error:
        print(f"error: could not read immutable Bot dashboard artifact: {error}", file=sys.stderr)
        return 1
    if not artifact_bytes:
        print("error: immutable Bot dashboard artifact is empty", file=sys.stderr)
        return 1
    if len(artifact_bytes) > MAX_ARTIFACT_BYTES:
        print(
            f"error: immutable Bot dashboard artifact exceeded {MAX_ARTIFACT_BYTES} byte limit",
            file=sys.stderr,
        )
        return 1

    errors = verify(
        root,
        expected_bot_commit=args.expected_bot_commit,
        immutable_bot_artifact_bytes=artifact_bytes,
        version_url=args.version_url,
        identity_url=args.identity_url,
    )
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    fingerprint = (
        root / "protocol" / "hosted-bridge" / "contract.sha256"
    ).read_text().split()[0]
    print(
        "production Cloud byte-bound contract verified: "
        f"bot={args.expected_bot_commit} bridge={fingerprint}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
