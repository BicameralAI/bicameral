from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path

import pytest
from jsonschema import validate

ROOT = Path(__file__).parents[1]
SCRIPT = ROOT / "scripts" / "migration_delivery.py"
MIGRATION_SET_SCHEMA = json.loads(
    (ROOT / "schemas" / "database-migration-set.schema.json").read_text()
)
EXECUTION_RECEIPT_SCHEMA = json.loads(
    (ROOT / "schemas" / "database-migration-execution-receipt.schema.json").read_text()
)
SPEC = importlib.util.spec_from_file_location("migration_delivery", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def write_migration(
    root: Path,
    version: str,
    name: str,
    sql: str,
    *,
    compatibility: str = "expand",
) -> None:
    migrations = root / "migrations"
    metadata = root / "metadata"
    migrations.mkdir(exist_ok=True)
    metadata.mkdir(exist_ok=True)
    (migrations / f"{version}_{name}.sql").write_text(sql, encoding="utf-8")
    (metadata / f"{version}.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "version": version,
                "compatibility": compatibility,
                "application_rollback_safe": compatibility == "expand",
                "execution_policy": (
                    "automatic"
                    if compatibility == "expand"
                    else "separate_authorization"
                ),
                "description": name.replace("_", " "),
            }
        ),
        encoding="utf-8",
    )


def test_migration_set_binds_exact_ordered_sql_metadata_and_cloud_commit(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    write_migration(
        tmp_path,
        "202607120002",
        "product_join",
        "alter table product_directory add column enabled boolean;",
    )

    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )

    assert migration_set["schema_version"] == 1
    assert migration_set["state"] == "planned"
    assert migration_set["authorizes_apply"] is False
    assert [item["version"] for item in migration_set["migrations"]] == [
        "202607120001",
        "202607120002",
    ]
    assert all(
        item["sql_digest"].startswith("sha256:")
        and item["metadata_digest"].startswith("sha256:")
        for item in migration_set["migrations"]
    )
    assert migration_set["migration_set_id"] == MODULE.canonical_digest(
        {
            key: value
            for key, value in migration_set.items()
            if key != "migration_set_id"
        }
    )
    validate(migration_set, MIGRATION_SET_SCHEMA)


def test_migration_set_rejects_non_expand_migration_in_automatic_lane(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "drop_legacy_column",
        "alter table product_directory drop column legacy;",
        compatibility="contract",
    )
    metadata_path = tmp_path / "metadata" / "202607120001.json"
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    metadata["execution_policy"] = "automatic"
    metadata_path.write_text(json.dumps(metadata), encoding="utf-8")

    with pytest.raises(
        ValueError,
        match="only backward-compatible expand migrations may be automatic",
    ):
        MODULE.compile_migration_set(
            migrations_dir=tmp_path / "migrations",
            metadata_dir=tmp_path / "metadata",
            cloud_commit="a" * 40,
        )


def test_manifest_binding_rejects_different_migration_set(tmp_path: Path) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": "sha256:" + "b" * 64,
                },
            }
        },
    }

    with pytest.raises(
        ValueError,
        match="manifest database migration set does not match",
    ):
        MODULE.validate_manifest_binding(manifest, migration_set)


def test_execution_preflights_applies_exact_pending_prefix_and_verifies(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    write_migration(
        tmp_path,
        "202607120002",
        "product_join",
        "alter table product_directory add column enabled boolean;",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    calls: list[list[str]] = []
    list_outputs = iter(
        [
            """
              Local        | Remote       | Time (UTC)
             ---------------|--------------|-----------
              202607120001 | 202607120001 | 2026-07-12
              202607120002 |              |
            """,
            """
              Local        | Remote       | Time (UTC)
             ---------------|--------------|-----------
              202607120001 | 202607120001 | 2026-07-12
              202607120002 | 202607120002 | 2026-07-12
            """,
        ]
    )

    def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
        calls.append(command)
        output = next(list_outputs) if "list" in command else ""
        return subprocess.CompletedProcess(command, 0, output, "")

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="staging",
        database_url="postgresql://secret.example/database",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=run_command,
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "verified"
    assert receipt["preflight"]["remote_versions_before"] == ["202607120001"]
    assert receipt["preflight"]["pending_versions"] == ["202607120002"]
    assert receipt["apply"]["applied_versions"] == ["202607120002"]
    assert receipt["verification"]["remote_versions_after"] == [
        "202607120001",
        "202607120002",
    ]
    assert receipt["deployment_prerequisite_satisfied"] is True
    assert receipt["authorizes_publish"] is False
    assert receipt["receipt_id"] == MODULE.canonical_digest(
        {key: value for key, value in receipt.items() if key != "receipt_id"}
    )
    assert [command[-1] for command in calls] == [
        "postgresql://secret.example/database",
        "postgresql://secret.example/database",
        "postgresql://secret.example/database",
        "postgresql://secret.example/database",
        "postgresql://secret.example/database",
    ]
    assert "list" in calls[0]
    assert "--dry-run" in calls[1]
    assert "--dry-run" not in calls[2]
    assert "list" in calls[3]
    assert "--dry-run" in calls[4]
    assert "postgresql://secret.example/database" not in json.dumps(receipt)
    validate(receipt, EXECUTION_RECEIPT_SCHEMA)


def test_execution_fails_closed_before_apply_on_remote_history_divergence(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    calls: list[list[str]] = []

    def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
        calls.append(command)
        return subprocess.CompletedProcess(
            command,
            0,
            """
              Local        | Remote       | Time (UTC)
             ---------------|--------------|-----------
              202607120001 | 202607110999 | 2026-07-11
            """,
            "",
        )

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="production",
        database_url="postgresql://secret.example/database",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=run_command,
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "preflight_failed"
    assert receipt["failure"]["code"] == "remote_history_diverged"
    assert receipt["deployment_prerequisite_satisfied"] is False
    assert receipt["authorizes_publish"] is False
    assert len(calls) == 1
    assert "postgresql://secret.example/database" not in json.dumps(receipt)
    validate(receipt, EXECUTION_RECEIPT_SCHEMA)


def test_execution_allows_historical_non_expand_but_not_as_pending_work(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "historical_hardening",
        "revoke all on schema public from anon;",
        compatibility="contract",
    )
    write_migration(
        tmp_path,
        "202607120002",
        "new_nullable_column",
        "alter table product_directory add column proof text;",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    list_outputs = iter(
        [
            """
              Local        | Remote       | Time (UTC)
              202607120001 | 202607120001 | 2026-07-12
              202607120002 |              |
            """,
            """
              Local        | Remote       | Time (UTC)
              202607120001 | 202607120001 | 2026-07-12
              202607120002 | 202607120002 | 2026-07-12
            """,
        ]
    )

    def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(
            command,
            0,
            next(list_outputs) if "list" in command else "",
            "",
        )

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="production",
        database_url="postgresql://secret.example/database",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=run_command,
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "verified"
    assert receipt["apply"]["applied_versions"] == ["202607120002"]


def test_execution_records_preflight_command_failure_without_applying(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    calls: list[list[str]] = []

    def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
        calls.append(command)
        if "list" in command:
            return subprocess.CompletedProcess(
                command,
                0,
                "202607120001 | |",
                "",
            )
        return subprocess.CompletedProcess(command, 1, "", "secret-bearing diagnostics")

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="production",
        database_url="postgresql://secret.example/database",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=run_command,
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "preflight_failed"
    assert receipt["failure"] == {
        "code": "supabase_preflight_failed",
        "phase": "preflight",
    }
    assert len(calls) == 2
    assert "secret-bearing diagnostics" not in json.dumps(receipt)
    assert "postgresql://secret.example/database" not in json.dumps(receipt)
    validate(receipt, EXECUTION_RECEIPT_SCHEMA)


def test_execution_records_missing_database_url_without_invoking_supabase(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    calls: list[list[str]] = []

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="production",
        database_url="",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=lambda command: calls.append(command),
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "preflight_failed"
    assert receipt["failure"] == {
        "code": "migration_database_url_missing",
        "phase": "preflight",
    }
    assert receipt["deployment_prerequisite_satisfied"] is False
    assert calls == []
    validate(receipt, EXECUTION_RECEIPT_SCHEMA)


def test_execution_records_apply_failure_after_successful_preflight(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    calls: list[list[str]] = []

    def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
        calls.append(command)
        if "list" in command:
            return subprocess.CompletedProcess(command, 0, "202607120001 | |", "")
        if "--dry-run" in command:
            return subprocess.CompletedProcess(command, 0, "", "")
        return subprocess.CompletedProcess(command, 1, "", "private diagnostics")

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="staging",
        database_url="postgresql://secret.example/database",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=run_command,
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "apply_failed"
    assert receipt["failure"] == {
        "code": "supabase_apply_failed",
        "phase": "apply",
    }
    assert receipt["preflight"]["status"] == "passed"
    assert receipt["preflight"]["pending_versions"] == ["202607120001"]
    assert receipt["deployment_prerequisite_satisfied"] is False
    assert len(calls) == 3
    assert "private diagnostics" not in json.dumps(receipt)
    validate(receipt, EXECUTION_RECEIPT_SCHEMA)


def test_execution_records_verification_failure_when_history_does_not_converge(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }

    def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
        if "list" in command:
            return subprocess.CompletedProcess(command, 0, "202607120001 | |", "")
        return subprocess.CompletedProcess(command, 0, "", "")

    receipt = MODULE.execute_migration_set(
        manifest=manifest,
        migration_set=migration_set,
        environment="staging",
        database_url="postgresql://secret.example/database",
        repository="BicameralAI/bicameral-cloud",
        workflow_run_id="12345",
        workflow_sha="a" * 40,
        run_command=run_command,
        now=lambda: datetime(2026, 7, 30, 1, 2, 3, tzinfo=UTC),
    )

    assert receipt["state"] == "verification_failed"
    assert receipt["failure"] == {
        "code": "remote_history_not_converged",
        "phase": "verification",
    }
    assert receipt["apply"]["status"] == "applied"
    assert receipt["deployment_prerequisite_satisfied"] is False
    validate(receipt, EXECUTION_RECEIPT_SCHEMA)


def test_compile_cli_writes_the_immutable_migration_set(tmp_path: Path) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    output = tmp_path / "migration-set.json"

    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "compile",
            "--migrations-dir",
            str(tmp_path / "migrations"),
            "--metadata-dir",
            str(tmp_path / "metadata"),
            "--cloud-commit",
            "a" * 40,
            "--output",
            str(output),
        ],
        check=False,
        text=True,
        capture_output=True,
    )

    assert result.returncode == 0, result.stderr
    payload = json.loads(output.read_text(encoding="utf-8"))
    assert payload["migration_set_id"].startswith("sha256:")
    assert result.stdout.strip() == str(output)


def test_execute_cli_writes_verified_environment_receipt(tmp_path: Path) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    manifest = {
        "release_id": "release-v1",
        "components": {
            "cloud": {
                "commit": "a" * 40,
                "artifacts": {
                    "database_migration_set": migration_set["migration_set_id"],
                },
            }
        },
    }
    manifest_path = tmp_path / "manifest.json"
    migration_set_path = tmp_path / "migration-set.json"
    receipt_path = tmp_path / "receipt.json"
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    migration_set_path.write_text(json.dumps(migration_set), encoding="utf-8")
    fake_state = tmp_path / "fake-state"
    fake_supabase = tmp_path / "fake-supabase"
    fake_supabase.write_text(
        """#!/usr/bin/env python3
import os
from pathlib import Path
import sys

state = Path(os.environ["FAKE_SUPABASE_STATE"])
if "migration" in sys.argv and "list" in sys.argv:
    remote = "202607120001" if state.exists() else ""
    print(f"202607120001 | {remote} |")
elif "push" in sys.argv and "--dry-run" not in sys.argv:
    state.write_text("applied", encoding="utf-8")
""",
        encoding="utf-8",
    )
    fake_supabase.chmod(0o755)
    environment = {
        **os.environ,
        "MIGRATION_DATABASE_URL": "postgresql://secret.example/database",
        "FAKE_SUPABASE_STATE": str(fake_state),
    }

    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "execute",
            "--manifest",
            str(manifest_path),
            "--migration-set",
            str(migration_set_path),
            "--migrations-dir",
            str(tmp_path / "migrations"),
            "--metadata-dir",
            str(tmp_path / "metadata"),
            "--environment",
            "staging",
            "--database-url-env",
            "MIGRATION_DATABASE_URL",
            "--repository",
            "BicameralAI/bicameral-cloud",
            "--workflow-run-id",
            "12345",
            "--workflow-sha",
            "a" * 40,
            "--supabase-executable",
            str(fake_supabase),
            "--output",
            str(receipt_path),
        ],
        check=False,
        text=True,
        capture_output=True,
        env=environment,
    )

    assert result.returncode == 0, result.stderr
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    assert receipt["state"] == "verified"
    assert receipt["environment"] == "staging"
    assert "postgresql://secret.example/database" not in receipt_path.read_text()


def test_execution_rejects_sql_bytes_changed_after_set_compilation(
    tmp_path: Path,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "product_directory",
        "create table product_directory(id uuid primary key);",
    )
    migration_set = MODULE.compile_migration_set(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        cloud_commit="a" * 40,
    )
    (tmp_path / "migrations" / "202607120001_product_directory.sql").write_text(
        "create table product_directory(id text primary key);",
        encoding="utf-8",
    )

    with pytest.raises(ValueError, match="source bytes differ"):
        MODULE.validate_source_binding(
            migration_set,
            migrations_dir=tmp_path / "migrations",
            metadata_dir=tmp_path / "metadata",
        )
