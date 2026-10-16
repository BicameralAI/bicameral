#!/usr/bin/env python3
"""Build manifest-bound hosted-release evidence without granting promotion authority."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any

SHA = re.compile(r"^[a-f0-9]{40}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")
JOURNEY_ID = "hosted_dashboard_browser_v1"
PROFILE_VERSION = "v1"


def canonical_digest(value: object) -> str:
    return (
        "sha256:"
        + hashlib.sha256(
            json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
    )


def _load(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _write(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def _component(descriptor: object, expected_name: str) -> dict[str, object]:
    if not isinstance(descriptor, dict):
        raise ValueError(f"{expected_name} descriptor must be an object")
    if (
        descriptor.get("schema_version") != 1
        or descriptor.get("component") != expected_name
    ):
        raise ValueError(f"{expected_name} descriptor schema/component mismatch")
    commit = descriptor.get("commit")
    artifacts = descriptor.get("artifacts")
    interfaces = descriptor.get("interfaces")
    claimed = descriptor.get("descriptor_digest")
    unsigned = {
        key: value for key, value in descriptor.items() if key != "descriptor_digest"
    }
    if not isinstance(commit, str) or not SHA.fullmatch(commit):
        raise ValueError(f"{expected_name} descriptor commit must be a full SHA")
    for label, values in (("artifacts", artifacts), ("interfaces", interfaces)):
        if not isinstance(values, dict) or not values:
            raise ValueError(f"{expected_name} descriptor {label} must be non-empty")
        if any(
            not isinstance(value, str) or not DIGEST.fullmatch(value)
            for value in values.values()
        ):
            raise ValueError(
                f"{expected_name} descriptor {label} contains an invalid digest"
            )
    if claimed != canonical_digest(unsigned):
        raise ValueError(f"{expected_name} descriptor digest does not bind its content")
    return {
        "commit": commit,
        "artifacts": artifacts,
        "interfaces": interfaces,
        "descriptor_digest": claimed,
    }


def build_manifest(
    bot_descriptor: object, cloud_descriptor: object
) -> dict[str, object]:
    bot = _component(bot_descriptor, "bot")
    cloud = _component(cloud_descriptor, "cloud")
    if bot["artifacts"].get("hosted_spa") != cloud["artifacts"].get("hosted_spa"):  # type: ignore[union-attr]
        raise ValueError("Bot and Cloud hosted SPA artifact digests differ")
    if (
        isinstance(cloud_descriptor, dict)
        and "selected_bot_commit" in cloud_descriptor
        and cloud_descriptor.get("selected_bot_commit") != bot["commit"]
    ):
        raise ValueError("Cloud descriptor does not select the manifest Bot commit")
    components = {"bot": bot, "cloud": cloud}
    closure_input = {"components": components, "profile_version": PROFILE_VERSION}
    return {
        "schema_version": 1,
        "release_id": f"hosted-dashboard-v1-{cloud['commit']}",
        "components": components,
        "closures": {
            JOURNEY_ID: {
                "components": ["bot", "cloud"],
                "profile_version": PROFILE_VERSION,
                "closure_digest": canonical_digest(closure_input),
            }
        },
        "required_journeys": [JOURNEY_ID],
    }


def bind_journey_receipt(manifest: object, raw_receipt: object) -> dict[str, object]:
    if not isinstance(manifest, dict) or not isinstance(raw_receipt, dict):
        raise ValueError("manifest and raw topology receipt must be objects")
    if (
        raw_receipt.get("status") != "passed"
        or raw_receipt.get("terminal_evidence_complete") is not True
    ):
        raise ValueError("raw topology receipt is not passed terminal evidence")
    components = manifest.get("components")
    raw_components = raw_receipt.get("components")
    if not isinstance(components, dict) or not isinstance(raw_components, dict):
        raise ValueError("manifest or raw receipt omitted components")
    expected_commits = {
        name: component["commit"]
        for name, component in components.items()
        if isinstance(component, dict)
    }
    if any(
        raw_components.get(name) != commit for name, commit in expected_commits.items()
    ):
        raise ValueError(
            "raw topology receipt component revisions do not match manifest"
        )
    dashboard = raw_receipt.get("dashboard")
    bot = components.get("bot")
    if not isinstance(dashboard, dict) or not isinstance(bot, dict):
        raise ValueError("raw topology receipt omitted dashboard evidence")
    expected_spa = bot["artifacts"]["hosted_spa"]
    if "sha256:" + str(dashboard.get("bundle_sha256")) != expected_spa:
        raise ValueError(
            "raw topology receipt dashboard digest does not match manifest"
        )
    closure = manifest["closures"][JOURNEY_ID]
    return {
        "schema_version": 1,
        "release_id": manifest["release_id"],
        "journey_id": JOURNEY_ID,
        "closure_digest": closure["closure_digest"],
        "profile_version": closure["profile_version"],
        "outcome": "passed",
    }


def build_served_identity(manifest: object) -> dict[str, object]:
    if not isinstance(manifest, dict) or not isinstance(
        manifest.get("components"), dict
    ):
        raise ValueError("manifest omitted components")
    return {
        "schema_version": 1,
        "release_id": manifest["release_id"],
        "components": {
            name: {
                "commit": component["commit"],
                "descriptor_digest": component["descriptor_digest"],
                "artifacts": component["artifacts"],
                "interfaces": component["interfaces"],
            }
            for name, component in manifest["components"].items()
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    manifest_parser = subparsers.add_parser("manifest")
    manifest_parser.add_argument("--bot-descriptor", type=Path, required=True)
    manifest_parser.add_argument("--cloud-descriptor", type=Path, required=True)
    manifest_parser.add_argument("--output", type=Path, required=True)
    receipt_parser = subparsers.add_parser("receipt")
    receipt_parser.add_argument("--manifest", type=Path, required=True)
    receipt_parser.add_argument("--raw-receipt", type=Path, required=True)
    receipt_parser.add_argument("--output", type=Path, required=True)
    identity_parser = subparsers.add_parser("identity")
    identity_parser.add_argument("--manifest", type=Path, required=True)
    identity_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "manifest":
        value = build_manifest(_load(args.bot_descriptor), _load(args.cloud_descriptor))
    elif args.command == "receipt":
        value = bind_journey_receipt(_load(args.manifest), _load(args.raw_receipt))
    else:
        value = build_served_identity(_load(args.manifest))
    _write(args.output, value)
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
