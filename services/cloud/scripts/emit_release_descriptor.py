#!/usr/bin/env python3
"""Emit and verify Cloud's immutable release-manifest inputs."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SHA = re.compile(r"^[a-f0-9]{40}$")
HEX_DIGEST = re.compile(r"^[a-f0-9]{64}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")
STATIC = ROOT / "src" / "bicameral_cloud" / "static" / "dashboard"
GENERATED_DASHBOARD_METADATA = frozenset({"version.json", "release-identity.json"})
BOT_DESCRIPTOR = ROOT / "release-inputs" / "bot-release-descriptor.json"
BOT_DASHBOARD_TRACKED_ROOT = "src/bicameral_cloud/static/dashboard"


def load_migration_delivery() -> Any:
    path = ROOT / "scripts" / "migration_delivery.py"
    spec = importlib.util.spec_from_file_location("cloud_migration_delivery", path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_dashboard_provenance_verifier() -> Any:
    path = ROOT / "scripts" / "verify_bot_dashboard_provenance.py"
    spec = importlib.util.spec_from_file_location("cloud_dashboard_provenance", path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def tree_digest(root: Path, *, exclude: frozenset[str] = frozenset()) -> str:
    files = sorted(path for path in root.rglob("*") if path.is_file())
    if not root.is_dir() or not files:
        raise ValueError(f"required release tree is missing or empty: {root}")
    hasher = hashlib.sha256()
    for path in files:
        relative = path.relative_to(root).as_posix()
        if relative in exclude:
            continue
        hasher.update(relative.encode())
        hasher.update(b"\0")
        hasher.update(path.read_bytes())
    return "sha256:" + hasher.hexdigest()


def files_digest(paths: list[Path]) -> str:
    hasher = hashlib.sha256()
    for path in sorted(paths):
        if not path.is_file():
            raise ValueError(f"required release input is missing: {path}")
        hasher.update(path.relative_to(ROOT).as_posix().encode())
        hasher.update(b"\0")
        hasher.update(path.read_bytes())
    return "sha256:" + hasher.hexdigest()


def tracked_tree_digest(
    relative_root: str, *, exclude_roots: tuple[str, ...] = ()
) -> str:
    output = subprocess.check_output(
        ["git", "ls-files", "-z", "--", relative_root], cwd=ROOT
    )
    relative_paths = sorted(Path(raw.decode()) for raw in output.split(b"\0") if raw)
    normalized_excludes = tuple(Path(root).as_posix().rstrip("/") for root in exclude_roots)
    relative_paths = [
        relative
        for relative in relative_paths
        if not any(
            relative.as_posix() == excluded
            or relative.as_posix().startswith(f"{excluded}/")
            for excluded in normalized_excludes
        )
    ]
    if not relative_paths:
        raise ValueError(f"required tracked release tree is empty: {relative_root}")
    hasher = hashlib.sha256()
    for relative in relative_paths:
        path = ROOT / relative
        if not path.is_file():
            raise ValueError(f"tracked release input is missing: {relative}")
        hasher.update(relative.as_posix().encode())
        hasher.update(b"\0")
        hasher.update(path.read_bytes())
    return "sha256:" + hasher.hexdigest()


def canonical_digest(value: object) -> str:
    return digest_bytes(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    )


def source_commit() -> str:
    return (
        os.environ.get("RELEASE_SOURCE_COMMIT")
        or os.environ.get("GITHUB_SHA")
        or subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()
    )


def dashboard_bot_commit() -> str:
    version = json.loads((STATIC / "version.json").read_text(encoding="utf-8"))
    commit = version.get("bot_commit") if isinstance(version, dict) else None
    if not isinstance(commit, str) or not SHA.fullmatch(commit):
        raise ValueError("Cloud dashboard version metadata lacks a full Bot commit")
    return commit


def verify_immutable_dashboard(artifact: Path, expected_bot_commit: str) -> dict[str, Any]:
    verifier = load_dashboard_provenance_verifier()
    errors, receipt = verifier.verify(
        ROOT,
        artifact=artifact,
        expected_bot_commit=expected_bot_commit,
    )
    if errors:
        raise ValueError("; ".join(errors))
    if not isinstance(receipt, dict) or receipt.get("status") != "verified":
        raise ValueError("immutable Bot dashboard verifier did not produce verified evidence")
    return receipt


def build_descriptor(commit: str) -> dict[str, Any]:
    if not SHA.fullmatch(commit):
        raise ValueError("Cloud source commit must be a full lowercase git SHA")
    version = json.loads((STATIC / "version.json").read_text(encoding="utf-8"))
    if not isinstance(version, dict) or not SHA.fullmatch(
        str(version.get("bot_commit", ""))
    ):
        raise ValueError("Cloud dashboard version metadata lacks a full Bot commit")

    artifact_sha256 = str(version.get("artifact_sha256", ""))
    bot_descriptor_digest = str(version.get("bot_descriptor_digest", ""))
    if not HEX_DIGEST.fullmatch(artifact_sha256):
        raise ValueError("Cloud dashboard version metadata lacks immutable artifact SHA-256")
    if not DIGEST.fullmatch(bot_descriptor_digest):
        raise ValueError("Cloud dashboard version metadata lacks Bot descriptor digest")

    bot_descriptor = json.loads(BOT_DESCRIPTOR.read_text(encoding="utf-8"))
    if not isinstance(bot_descriptor, dict):
        raise ValueError("Bot release descriptor must be an object")
    if bot_descriptor.get("commit") != version["bot_commit"]:
        raise ValueError("Bot release descriptor commit does not match dashboard version")
    if bot_descriptor.get("descriptor_digest") != bot_descriptor_digest:
        raise ValueError("Bot release descriptor digest does not match dashboard version")
    unsigned_bot = {
        key: value for key, value in bot_descriptor.items() if key != "descriptor_digest"
    }
    if canonical_digest(unsigned_bot) != bot_descriptor_digest:
        raise ValueError("Bot release descriptor digest does not bind descriptor content")

    actual_bundle = tree_digest(STATIC, exclude=GENERATED_DASHBOARD_METADATA)
    if "sha256:" + str(version.get("bundle_sha256")) != actual_bundle:
        raise ValueError("Cloud dashboard version metadata does not match served bytes")
    bot_artifacts = bot_descriptor.get("artifacts")
    if not isinstance(bot_artifacts, dict) or bot_artifacts.get("hosted_spa") != actual_bundle:
        raise ValueError("Bot release descriptor hosted_spa does not match served bytes")

    migration_set = load_migration_delivery().compile_migration_set(
        migrations_dir=ROOT / "supabase" / "migrations",
        metadata_dir=ROOT / "supabase" / "migration-metadata",
        cloud_commit=commit,
    )
    payload: dict[str, Any] = {
        "schema_version": 1,
        "component": "cloud",
        "commit": commit,
        "artifacts": {
            "hosted_spa": actual_bundle,
            "immutable_bot_dashboard": "sha256:" + artifact_sha256,
            "bot_release_descriptor": bot_descriptor_digest,
            "cloud_image_inputs": files_digest(
                [ROOT / "Dockerfile", ROOT / "cloudbuild.yaml", ROOT / "pyproject.toml"]
            ),
            # The deploy workflow replaces the checked-in dashboard with the exact
            # Bot producer artifact before this descriptor is emitted. Those served
            # bytes are already bound above by hosted_spa and immutable_bot_dashboard,
            # so Cloud runtime provenance must cover only Cloud-owned runtime source.
            "cloud_runtime": tracked_tree_digest(
                "src", exclude_roots=(BOT_DASHBOARD_TRACKED_ROOT,)
            ),
            "database_migrations": tracked_tree_digest("supabase/migrations"),
            "database_migration_set": migration_set["migration_set_id"],
        },
        "interfaces": {
            "hosted_bridge": files_digest(
                [
                    ROOT
                    / "src"
                    / "bicameral_cloud"
                    / "contracts"
                    / "hosted_bridge_contract.json",
                    ROOT
                    / "src"
                    / "bicameral_cloud"
                    / "contracts"
                    / "hosted_bridge_contract.sha256",
                ]
            ),
            "migration_delivery": files_digest(
                [
                    ROOT / "schemas" / "database-migration-set.schema.json",
                    ROOT
                    / "schemas"
                    / "database-migration-execution-receipt.schema.json",
                    ROOT / "scripts" / "migration_delivery.py",
                ]
            ),
            "deployment": files_digest(
                [
                    ROOT / ".github" / "workflows" / "deploy-cloud-run.yml",
                    ROOT
                    / ".github"
                    / "workflows"
                    / "terminal-hosted-product-journey.yml",
                    ROOT / "scripts" / "migration_delivery.py",
                    ROOT / "scripts" / "verify_bot_dashboard_provenance.py",
                ]
            ),
        },
        "selected_bot_commit": version["bot_commit"],
        "source_artifact_sha256": artifact_sha256,
        "source_bot_descriptor_digest": bot_descriptor_digest,
    }
    payload["descriptor_digest"] = canonical_digest(payload)
    return payload


def validate_descriptor(payload: object) -> list[str]:
    if not isinstance(payload, dict):
        return ["descriptor must be an object"]
    errors: list[str] = []
    if payload.get("schema_version") != 1 or payload.get("component") != "cloud":
        errors.append("descriptor schema/component mismatch")
    for field in ("commit", "selected_bot_commit"):
        if not isinstance(payload.get(field), str) or not SHA.fullmatch(payload[field]):
            errors.append(f"{field} must be a full lowercase git SHA")
    for section in ("artifacts", "interfaces"):
        values = payload.get(section)
        if not isinstance(values, dict) or not values:
            errors.append(f"{section} must be a non-empty object")
            continue
        for name, value in values.items():
            if not isinstance(value, str) or not DIGEST.fullmatch(value):
                errors.append(f"{section}.{name} must be sha256:<64 lowercase hex>")
    if not isinstance(payload.get("source_artifact_sha256"), str) or not HEX_DIGEST.fullmatch(
        payload["source_artifact_sha256"]
    ):
        errors.append("source_artifact_sha256 must be 64 lowercase hex")
    if not isinstance(payload.get("source_bot_descriptor_digest"), str) or not DIGEST.fullmatch(
        payload["source_bot_descriptor_digest"]
    ):
        errors.append("source_bot_descriptor_digest must be sha256:<64 lowercase hex>")
    claimed = payload.get("descriptor_digest")
    unsigned = {
        key: value for key, value in payload.items() if key != "descriptor_digest"
    }
    if claimed != canonical_digest(unsigned):
        errors.append("descriptor_digest does not bind the descriptor")
    return errors


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "release-cloud" / "cloud-release-descriptor.json",
    )
    parser.add_argument("--verify", type=Path)
    parser.add_argument("--immutable-bot-artifact", type=Path)
    parser.add_argument(
        "--validation-only",
        action="store_true",
        help="Validate Cloud-local descriptor consistency without claiming producer provenance.",
    )
    parser.add_argument(
        "--provenance-output",
        type=Path,
        default=ROOT / "release-cloud" / "served-dashboard-provenance.json",
    )
    args = parser.parse_args()
    if args.verify:
        errors = validate_descriptor(json.loads(args.verify.read_text()))
        if errors:
            for error in errors:
                print(f"- {error}")
            return 1
        return 0

    if bool(args.immutable_bot_artifact) == bool(args.validation_only):
        print(
            "Cloud release provenance rejected: choose exactly one of "
            "--immutable-bot-artifact or --validation-only"
        )
        return 1

    try:
        bot_commit = dashboard_bot_commit()
        if args.immutable_bot_artifact:
            receipt = verify_immutable_dashboard(args.immutable_bot_artifact, bot_commit)
            _write_json(args.provenance_output, receipt)
        else:
            print(
                "Cloud release descriptor validation-only: producer-authentic proof is "
                "deferred to the trusted dev/main deployment workflow."
            )
        payload = build_descriptor(source_commit())
    except (OSError, ValueError) as error:
        print(f"Cloud release provenance rejected: {error}")
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
