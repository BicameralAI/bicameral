#!/usr/bin/env python3
"""Compile and execute manifest-bound Cloud database migrations."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

SHA = re.compile(r"^[a-f0-9]{40}$")
MIGRATION_FILENAME = re.compile(
    r"^(?P<version>(?:[0-9]{12}|[0-9]{14}))_(?P<name>[a-z0-9_]+)\.sql$"
)
COMPATIBILITY_CLASSES = {"expand", "backfill", "contract", "destructive"}
METADATA_KEYS = {
    "schema_version",
    "version",
    "compatibility",
    "application_rollback_safe",
    "execution_policy",
    "description",
}


def canonical_digest(value: object) -> str:
    return (
        "sha256:"
        + hashlib.sha256(
            json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
    )


def file_digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def validate_metadata(metadata: object, *, version: str) -> dict[str, Any]:
    if not isinstance(metadata, dict) or set(metadata) != METADATA_KEYS:
        raise ValueError(
            f"migration metadata {version} keys must be exactly {sorted(METADATA_KEYS)}"
        )
    if metadata["schema_version"] != 1 or metadata["version"] != version:
        raise ValueError(f"migration metadata {version} schema/version mismatch")
    compatibility = metadata["compatibility"]
    if compatibility not in COMPATIBILITY_CLASSES:
        raise ValueError(f"migration metadata {version} compatibility is invalid")
    if not isinstance(metadata["description"], str) or not metadata["description"]:
        raise ValueError(f"migration metadata {version} description must be non-empty")
    if not isinstance(metadata["application_rollback_safe"], bool):
        raise ValueError(
            f"migration metadata {version} application_rollback_safe must be boolean"
        )
    if metadata["execution_policy"] not in {
        "automatic",
        "separate_authorization",
    }:
        raise ValueError(f"migration metadata {version} execution_policy is invalid")
    if metadata["execution_policy"] == "automatic" and (
        compatibility != "expand" or metadata["application_rollback_safe"] is not True
    ):
        raise ValueError("only backward-compatible expand migrations may be automatic")
    if compatibility != "expand" and metadata["execution_policy"] != (
        "separate_authorization"
    ):
        raise ValueError(
            "non-expand migrations require separate_authorization execution policy"
        )
    return metadata


def compile_migration_set(
    *,
    migrations_dir: Path,
    metadata_dir: Path,
    cloud_commit: str,
) -> dict[str, Any]:
    if not SHA.fullmatch(cloud_commit):
        raise ValueError("cloud_commit must be a full lowercase git SHA")
    migrations: list[dict[str, Any]] = []
    for path in sorted(migrations_dir.glob("*.sql")):
        match = MIGRATION_FILENAME.fullmatch(path.name)
        if not match:
            raise ValueError(f"invalid migration filename: {path.name}")
        version = match.group("version")
        metadata_path = metadata_dir / f"{version}.json"
        if not metadata_path.is_file():
            raise ValueError(f"missing migration metadata: {metadata_path.name}")
        metadata = validate_metadata(
            json.loads(metadata_path.read_text(encoding="utf-8")),
            version=version,
        )
        migrations.append(
            {
                "version": version,
                "path": path.relative_to(migrations_dir.parent).as_posix(),
                "sql_digest": file_digest(path),
                "metadata_digest": file_digest(metadata_path),
                "compatibility": metadata["compatibility"],
                "application_rollback_safe": metadata["application_rollback_safe"],
                "execution_policy": metadata["execution_policy"],
                "description": metadata["description"],
            }
        )
    if not migrations:
        raise ValueError("migration set must contain at least one migration")
    payload: dict[str, Any] = {
        "schema_version": 1,
        "cloud_commit": cloud_commit,
        "migrations": migrations,
        "state": "planned",
        "authorizes_apply": False,
    }
    return {"migration_set_id": canonical_digest(payload), **payload}


def validate_manifest_binding(
    manifest: object,
    migration_set: object,
) -> None:
    if not isinstance(manifest, dict) or not isinstance(migration_set, dict):
        raise ValueError("manifest and migration set must be objects")
    claimed_set_id = migration_set.get("migration_set_id")
    unsigned_set = {
        key: value for key, value in migration_set.items() if key != "migration_set_id"
    }
    if claimed_set_id != canonical_digest(unsigned_set):
        raise ValueError("migration set identity does not bind its content")
    components = manifest.get("components")
    cloud = components.get("cloud") if isinstance(components, dict) else None
    if not isinstance(cloud, dict):
        raise ValueError("manifest does not select a Cloud component")
    if cloud.get("commit") != migration_set.get("cloud_commit"):
        raise ValueError("manifest Cloud commit does not match migration set")
    artifacts = cloud.get("artifacts")
    if (
        not isinstance(artifacts, dict)
        or artifacts.get("database_migration_set") != claimed_set_id
    ):
        raise ValueError("manifest database migration set does not match")


def validate_source_binding(
    migration_set: object,
    *,
    migrations_dir: Path,
    metadata_dir: Path,
) -> None:
    if not isinstance(migration_set, dict):
        raise ValueError("migration set must be an object")
    cloud_commit = migration_set.get("cloud_commit")
    if not isinstance(cloud_commit, str):
        raise ValueError("migration set omitted cloud_commit")
    rebuilt = compile_migration_set(
        migrations_dir=migrations_dir,
        metadata_dir=metadata_dir,
        cloud_commit=cloud_commit,
    )
    if rebuilt != migration_set:
        raise ValueError("migration set source bytes differ from the compiled plan")


def parse_migration_history(output: str) -> tuple[list[str], list[str]]:
    local: list[str] = []
    remote: list[str] = []
    for line in output.splitlines():
        parts = [part.strip() for part in line.split("|")]
        if len(parts) < 2:
            continue
        local_version, remote_version = parts[:2]
        if re.fullmatch(r"(?:[0-9]{12}|[0-9]{14})", local_version):
            local.append(local_version)
        if re.fullmatch(r"(?:[0-9]{12}|[0-9]{14})", remote_version):
            remote.append(remote_version)
    return local, remote


def _run_checked(
    run_command: Callable[[list[str]], subprocess.CompletedProcess[str]],
    command: list[str],
    *,
    phase: str,
) -> subprocess.CompletedProcess[str]:
    result = run_command(command)
    if result.returncode != 0:
        raise RuntimeError(f"Supabase migration {phase} failed")
    return result


def _supabase_command(
    *arguments: str,
    database_url: str,
    executable: str,
) -> list[str]:
    return [
        executable,
        "--yes",
        "supabase@2.39.2",
        *arguments,
        "--db-url",
        database_url,
    ]


def run_subprocess(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        check=False,
        text=True,
        capture_output=True,
    )


def _failure_receipt(
    *,
    manifest: dict[str, Any],
    migration_set: dict[str, Any],
    environment: str,
    repository: str,
    workflow_run_id: str,
    workflow_sha: str,
    timestamp: str,
    state: str,
    code: str,
    remote_versions_before: list[str] | None = None,
    pending_versions: list[str] | None = None,
    remote_versions_after: list[str] | None = None,
    phase: str = "preflight",
) -> dict[str, Any]:
    state_history = [{"state": "planned", "at": timestamp}]
    if phase in {"apply", "verification"}:
        state_history.append({"state": "preflight_passed", "at": timestamp})
    if phase == "verification":
        state_history.append({"state": "applied", "at": timestamp})
    state_history.append({"state": state, "at": timestamp})
    payload: dict[str, Any] = {
        "schema_version": 1,
        "migration_set_id": migration_set["migration_set_id"],
        "release_id": manifest["release_id"],
        "cloud_commit": migration_set["cloud_commit"],
        "environment": environment,
        "state": state,
        "state_history": state_history,
        "preflight": {
            "status": "failed" if phase == "preflight" else "passed",
            "remote_versions_before": remote_versions_before or [],
            "pending_versions": pending_versions or [],
        },
        "failure": {"code": code, "phase": phase},
        "tool": {"name": "supabase", "version": "2.39.2"},
        "workflow": {
            "repository": repository,
            "run_id": workflow_run_id,
            "sha": workflow_sha,
            "url": f"https://github.com/{repository}/actions/runs/{workflow_run_id}",
        },
        "started_at": timestamp,
        "completed_at": timestamp,
        "deployment_prerequisite_satisfied": False,
        "authorizes_publish": False,
    }
    if phase in {"apply", "verification"}:
        payload["apply"] = {
            "status": "failed" if phase == "apply" else "applied",
            "applied_versions": [] if phase == "apply" else pending_versions or [],
        }
    if phase == "verification":
        payload["verification"] = {
            "status": "failed",
            "remote_versions_after": remote_versions_after or [],
        }
    return {"receipt_id": canonical_digest(payload), **payload}


def execute_migration_set(
    *,
    manifest: object,
    migration_set: object,
    environment: str,
    database_url: str,
    repository: str,
    workflow_run_id: str,
    workflow_sha: str,
    supabase_executable: str = "npx",
    run_command: Callable[
        [list[str]], subprocess.CompletedProcess[str]
    ] = run_subprocess,
    now: Callable[[], datetime] = lambda: datetime.now(UTC),
) -> dict[str, Any]:
    if not isinstance(manifest, dict) or not isinstance(migration_set, dict):
        raise ValueError("manifest and migration set must be objects")
    if environment not in {"staging", "production"}:
        raise ValueError("environment must be staging or production")
    timestamp = now().isoformat().replace("+00:00", "Z")
    try:
        validate_manifest_binding(manifest, migration_set)
    except ValueError:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="manifest_binding_mismatch",
        )
    if not database_url:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="migration_database_url_missing",
        )
    migrations = migration_set.get("migrations")
    if not isinstance(migrations, list) or not migrations:
        raise ValueError("migration set must contain migrations")

    expected_versions = [str(item["version"]) for item in migrations]
    try:
        before = _run_checked(
            run_command,
            _supabase_command(
                "migration",
                "list",
                database_url=database_url,
                executable=supabase_executable,
            ),
            phase="history inspection",
        )
    except RuntimeError:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="supabase_history_inspection_failed",
        )
    local_before, remote_before = parse_migration_history(before.stdout)
    if local_before != expected_versions:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="local_history_mismatch",
            remote_versions_before=remote_before,
        )
    if remote_before != expected_versions[: len(remote_before)]:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="remote_history_diverged",
            remote_versions_before=remote_before,
        )
    pending_versions = expected_versions[len(remote_before) :]
    pending_migrations = migrations[len(remote_before) :]
    if any(
        not isinstance(item, dict)
        or item.get("compatibility") != "expand"
        or item.get("execution_policy") != "automatic"
        or item.get("application_rollback_safe") is not True
        for item in pending_migrations
    ):
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="pending_migration_requires_separate_authorization",
            remote_versions_before=remote_before,
        )

    try:
        _run_checked(
            run_command,
            _supabase_command(
                "db",
                "push",
                "--yes",
                "--dry-run",
                database_url=database_url,
                executable=supabase_executable,
            ),
            phase="preflight",
        )
    except RuntimeError:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="preflight_failed",
            code="supabase_preflight_failed",
            remote_versions_before=remote_before,
        )
    try:
        _run_checked(
            run_command,
            _supabase_command(
                "db",
                "push",
                "--yes",
                database_url=database_url,
                executable=supabase_executable,
            ),
            phase="apply",
        )
    except RuntimeError:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="apply_failed",
            code="supabase_apply_failed",
            remote_versions_before=remote_before,
            pending_versions=pending_versions,
            phase="apply",
        )
    try:
        after = _run_checked(
            run_command,
            _supabase_command(
                "migration",
                "list",
                database_url=database_url,
                executable=supabase_executable,
            ),
            phase="post-apply history inspection",
        )
    except RuntimeError:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="verification_failed",
            code="supabase_verification_failed",
            remote_versions_before=remote_before,
            pending_versions=pending_versions,
            phase="verification",
        )
    local_after, remote_after = parse_migration_history(after.stdout)
    if local_after != expected_versions or remote_after != expected_versions:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="verification_failed",
            code="remote_history_not_converged",
            remote_versions_before=remote_before,
            pending_versions=pending_versions,
            remote_versions_after=remote_after,
            phase="verification",
        )
    try:
        _run_checked(
            run_command,
            _supabase_command(
                "db",
                "push",
                "--yes",
                "--dry-run",
                database_url=database_url,
                executable=supabase_executable,
            ),
            phase="convergence verification",
        )
    except RuntimeError:
        return _failure_receipt(
            manifest=manifest,
            migration_set=migration_set,
            environment=environment,
            repository=repository,
            workflow_run_id=workflow_run_id,
            workflow_sha=workflow_sha,
            timestamp=timestamp,
            state="verification_failed",
            code="supabase_convergence_check_failed",
            remote_versions_before=remote_before,
            pending_versions=pending_versions,
            remote_versions_after=remote_after,
            phase="verification",
        )

    payload: dict[str, Any] = {
        "schema_version": 1,
        "migration_set_id": migration_set["migration_set_id"],
        "release_id": manifest["release_id"],
        "cloud_commit": migration_set["cloud_commit"],
        "environment": environment,
        "state": "verified",
        "state_history": [
            {"state": "planned", "at": timestamp},
            {"state": "preflight_passed", "at": timestamp},
            {"state": "applied", "at": timestamp},
            {"state": "verified", "at": timestamp},
        ],
        "preflight": {
            "status": "passed",
            "remote_versions_before": remote_before,
            "pending_versions": pending_versions,
        },
        "apply": {
            "status": "applied",
            "applied_versions": pending_versions,
        },
        "verification": {
            "status": "passed",
            "remote_versions_after": remote_after,
        },
        "tool": {"name": "supabase", "version": "2.39.2"},
        "workflow": {
            "repository": repository,
            "run_id": workflow_run_id,
            "sha": workflow_sha,
            "url": f"https://github.com/{repository}/actions/runs/{workflow_run_id}",
        },
        "started_at": timestamp,
        "completed_at": timestamp,
        "deployment_prerequisite_satisfied": True,
        "authorizes_publish": False,
    }
    return {"receipt_id": canonical_digest(payload), **payload}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    compile_parser = subparsers.add_parser("compile")
    compile_parser.add_argument("--migrations-dir", type=Path, required=True)
    compile_parser.add_argument("--metadata-dir", type=Path, required=True)
    compile_parser.add_argument("--cloud-commit", required=True)
    compile_parser.add_argument("--output", type=Path, required=True)
    execute_parser = subparsers.add_parser("execute")
    execute_parser.add_argument("--manifest", type=Path, required=True)
    execute_parser.add_argument("--migration-set", type=Path, required=True)
    execute_parser.add_argument("--migrations-dir", type=Path, required=True)
    execute_parser.add_argument("--metadata-dir", type=Path, required=True)
    execute_parser.add_argument(
        "--environment",
        choices=["staging", "production"],
        required=True,
    )
    execute_parser.add_argument("--database-url-env", required=True)
    execute_parser.add_argument("--repository", required=True)
    execute_parser.add_argument("--workflow-run-id", required=True)
    execute_parser.add_argument("--workflow-sha", required=True)
    execute_parser.add_argument("--supabase-executable", default="npx")
    execute_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if args.command == "compile":
        value = compile_migration_set(
            migrations_dir=args.migrations_dir,
            metadata_dir=args.metadata_dir,
            cloud_commit=args.cloud_commit,
        )
        write_json(args.output, value)
        print(args.output)
        return 0
    if args.command == "execute":
        database_url = os.environ.get(args.database_url_env, "")
        migration_set = json.loads(args.migration_set.read_text(encoding="utf-8"))
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        try:
            validate_source_binding(
                migration_set,
                migrations_dir=args.migrations_dir,
                metadata_dir=args.metadata_dir,
            )
        except ValueError:
            timestamp = datetime.now(UTC).isoformat().replace("+00:00", "Z")
            value = _failure_receipt(
                manifest=manifest,
                migration_set=migration_set,
                environment=args.environment,
                repository=args.repository,
                workflow_run_id=args.workflow_run_id,
                workflow_sha=args.workflow_sha,
                timestamp=timestamp,
                state="preflight_failed",
                code="source_binding_mismatch",
            )
            write_json(args.output, value)
            print(args.output)
            return 1
        value = execute_migration_set(
            manifest=manifest,
            migration_set=migration_set,
            environment=args.environment,
            database_url=database_url,
            repository=args.repository,
            workflow_run_id=args.workflow_run_id,
            workflow_sha=args.workflow_sha,
            supabase_executable=args.supabase_executable,
        )
        write_json(args.output, value)
        print(args.output)
        return 0 if value.get("state") == "verified" else 1
    raise AssertionError(f"unhandled command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
