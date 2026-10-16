#!/usr/bin/env python3
"""Apply one explicitly authorized non-automatic Supabase migration.

This is the narrow companion to migration_delivery.py's automatic lane. It never
selects a migration implicitly: the caller supplies one exact version and an
authorization reference, and the remote history must prove that version is the
sole pending migration before any database mutation occurs.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import migration_delivery

VERSION_RE = re.compile(r"^(?:[0-9]{12}|[0-9]{14})$")


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def receipt(
    *,
    state: str,
    version: str,
    environment: str,
    cloud_commit: str,
    authorization_reference: str,
    migration_set_id: str,
    remote_before: list[str],
    remote_after: list[str] | None = None,
    failure_code: str | None = None,
) -> dict[str, Any]:
    now = datetime.now(UTC).isoformat().replace("+00:00", "Z")
    value: dict[str, Any] = {
        "schema_version": 1,
        "state": state,
        "authorized_version": version,
        "authorization_reference": authorization_reference,
        "environment": environment,
        "cloud_commit": cloud_commit,
        "migration_set_id": migration_set_id,
        "remote_versions_before": remote_before,
        "remote_versions_after": remote_after or remote_before,
        "completed_at": now,
        "tool": {"name": "supabase", "version": "2.39.2"},
        "authorizes_publish": False,
    }
    if failure_code:
        value["failure"] = {"code": failure_code}
    return value


def supabase(
    arguments: list[str],
    *,
    database_url: str,
) -> subprocess.CompletedProcess[str]:
    command = [
        "npx",
        "--yes",
        "supabase@2.39.2",
        *arguments,
        "--db-url",
        database_url,
    ]
    return subprocess.run(command, check=False, text=True, capture_output=True)


def execute(
    *,
    migrations_dir: Path,
    metadata_dir: Path,
    version: str,
    environment: str,
    database_url: str,
    cloud_commit: str,
    authorization_reference: str,
) -> dict[str, Any]:
    if not VERSION_RE.fullmatch(version):
        raise ValueError("authorized migration version must be a 12- or 14-digit version")
    if environment not in {"production", "staging"}:
        raise ValueError("environment must be production or staging")
    if not database_url:
        raise ValueError("migration database URL is required")
    if not re.fullmatch(r"[a-f0-9]{40}", cloud_commit):
        raise ValueError("cloud commit must be a full SHA")
    if not authorization_reference.startswith("https://github.com/"):
        raise ValueError("authorization reference must be a GitHub URL")

    migration_set = migration_delivery.compile_migration_set(
        migrations_dir=migrations_dir,
        metadata_dir=metadata_dir,
        cloud_commit=cloud_commit,
    )
    migrations = migration_set["migrations"]
    expected_versions = [str(item["version"]) for item in migrations]
    selected = [item for item in migrations if item["version"] == version]
    if len(selected) != 1:
        raise ValueError("authorized migration version is not uniquely present in the set")
    item = selected[0]
    if item["execution_policy"] != "separate_authorization":
        raise ValueError("authorized lane may only execute separate_authorization migrations")

    before = supabase(["migration", "list"], database_url=database_url)
    if before.returncode != 0:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=[],
            failure_code="history_inspection_failed",
        )
    local_before, remote_before = migration_delivery.parse_migration_history(before.stdout)
    if local_before != expected_versions:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            failure_code="local_history_mismatch",
        )
    if remote_before == expected_versions:
        return receipt(
            state="verified_already_applied",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            remote_after=remote_before,
        )
    if remote_before != expected_versions[: len(remote_before)]:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            failure_code="remote_history_diverged",
        )
    pending_versions = expected_versions[len(remote_before) :]
    if pending_versions != [version]:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            failure_code="authorized_version_not_sole_pending_migration",
        )

    dry_run = supabase(["db", "push", "--yes", "--dry-run"], database_url=database_url)
    if dry_run.returncode != 0:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            failure_code="preflight_failed",
        )
    applied = supabase(["db", "push", "--yes"], database_url=database_url)
    if applied.returncode != 0:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            failure_code="apply_failed",
        )
    after = supabase(["migration", "list"], database_url=database_url)
    if after.returncode != 0:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            failure_code="verification_history_inspection_failed",
        )
    local_after, remote_after = migration_delivery.parse_migration_history(after.stdout)
    if local_after != expected_versions or remote_after != expected_versions:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            remote_after=remote_after,
            failure_code="remote_history_not_converged",
        )
    convergence = supabase(["db", "push", "--yes", "--dry-run"], database_url=database_url)
    if convergence.returncode != 0:
        return receipt(
            state="failed",
            version=version,
            environment=environment,
            cloud_commit=cloud_commit,
            authorization_reference=authorization_reference,
            migration_set_id=migration_set["migration_set_id"],
            remote_before=remote_before,
            remote_after=remote_after,
            failure_code="convergence_check_failed",
        )
    return receipt(
        state="verified",
        version=version,
        environment=environment,
        cloud_commit=cloud_commit,
        authorization_reference=authorization_reference,
        migration_set_id=migration_set["migration_set_id"],
        remote_before=remote_before,
        remote_after=remote_after,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--migrations-dir", type=Path, required=True)
    parser.add_argument("--metadata-dir", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--environment", choices=["production", "staging"], required=True)
    parser.add_argument("--database-url-env", required=True)
    parser.add_argument("--cloud-commit", required=True)
    parser.add_argument("--authorization-reference", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    try:
        value = execute(
            migrations_dir=args.migrations_dir,
            metadata_dir=args.metadata_dir,
            version=args.version,
            environment=args.environment,
            database_url=os.environ.get(args.database_url_env, ""),
            cloud_commit=args.cloud_commit,
            authorization_reference=args.authorization_reference,
        )
    except ValueError as exc:
        value = {"schema_version": 1, "state": "failed", "failure": {"code": str(exc)}}
    write_json(args.output, value)
    print(args.output)
    return 0 if value.get("state") in {"verified", "verified_already_applied"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
