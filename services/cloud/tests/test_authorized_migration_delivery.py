# ruff: noqa: E501
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]
# Loading the executor by file path does not otherwise put scripts/ on sys.path,
# while normal CLI execution does. Mirror that runtime shape for the unit tests.
sys.path.insert(0, str(ROOT / "scripts"))
SCRIPT = ROOT / "scripts" / "authorized_migration_delivery.py"
SPEC = importlib.util.spec_from_file_location("authorized_migration_delivery", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def write_migration(
    root: Path,
    version: str,
    name: str,
    *,
    compatibility: str,
) -> None:
    migrations = root / "migrations"
    metadata = root / "metadata"
    migrations.mkdir(exist_ok=True)
    metadata.mkdir(exist_ok=True)
    (migrations / f"{version}_{name}.sql").write_text(
        f"-- {name}\nselect 1;\n",
        encoding="utf-8",
    )
    (metadata / f"{version}.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "version": version,
                "compatibility": compatibility,
                "application_rollback_safe": True,
                "execution_policy": (
                    "automatic"
                    if compatibility == "expand"
                    else "separate_authorization"
                ),
                "description": name,
            }
        ),
        encoding="utf-8",
    )


def migration_list(local: list[str], remote: list[str]) -> str:
    rows = ["Local | Remote | Time (UTC)"]
    for index, version in enumerate(local):
        remote_version = remote[index] if index < len(remote) else ""
        rows.append(f"{version} | {remote_version} |")
    return "\n".join(rows)


def test_separate_authorization_applies_exact_sole_pending_version(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    write_migration(
        tmp_path,
        "202607120001",
        "historical_expand",
        compatibility="expand",
    )
    write_migration(
        tmp_path,
        "202610020001",
        "internal_admission",
        compatibility="backfill",
    )
    outputs = iter(
        [
            migration_list(
                ["202607120001", "202610020001"],
                ["202607120001"],
            ),
            "",
            "",
            migration_list(
                ["202607120001", "202610020001"],
                ["202607120001", "202610020001"],
            ),
            "",
        ]
    )
    calls: list[list[str]] = []

    def fake_supabase(arguments: list[str], *, database_url: str):
        calls.append(arguments)
        return subprocess.CompletedProcess(arguments, 0, next(outputs), "")

    monkeypatch.setattr(MODULE, "supabase", fake_supabase)
    result = MODULE.execute(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        version="202610020001",
        environment="production",
        database_url="postgresql://secret.example/database",
        cloud_commit="a" * 40,
        authorization_reference="https://github.com/BicameralAI/bicameral-cloud/issues/343",
    )

    assert result["state"] == "verified"
    assert result["remote_versions_before"] == ["202607120001"]
    assert result["remote_versions_after"] == ["202607120001", "202610020001"]
    assert result["authorizes_publish"] is False
    assert calls == [
        ["migration", "list"],
        ["db", "push", "--yes", "--dry-run"],
        ["db", "push", "--yes"],
        ["migration", "list"],
        ["db", "push", "--yes", "--dry-run"],
    ]
    assert "secret.example" not in json.dumps(result)


def test_separate_authorization_refuses_when_more_than_target_is_pending(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    write_migration(tmp_path, "202607120001", "historical", compatibility="expand")
    write_migration(tmp_path, "202610020001", "authorized", compatibility="backfill")
    write_migration(tmp_path, "202610030001", "later", compatibility="expand")

    def fake_supabase(arguments: list[str], *, database_url: str):
        return subprocess.CompletedProcess(
            arguments,
            0,
            migration_list(
                ["202607120001", "202610020001", "202610030001"],
                ["202607120001"],
            ),
            "",
        )

    monkeypatch.setattr(MODULE, "supabase", fake_supabase)
    result = MODULE.execute(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        version="202610020001",
        environment="production",
        database_url="postgresql://secret.example/database",
        cloud_commit="a" * 40,
        authorization_reference="https://github.com/BicameralAI/bicameral-cloud/issues/343",
    )

    assert result["state"] == "failed"
    assert result["failure"]["code"] == "authorized_version_not_sole_pending_migration"


def test_separate_authorization_is_idempotent_when_version_already_applied(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    write_migration(tmp_path, "202610020001", "authorized", compatibility="backfill")

    def fake_supabase(arguments: list[str], *, database_url: str):
        return subprocess.CompletedProcess(
            arguments,
            0,
            migration_list(["202610020001"], ["202610020001"]),
            "",
        )

    monkeypatch.setattr(MODULE, "supabase", fake_supabase)
    result = MODULE.execute(
        migrations_dir=tmp_path / "migrations",
        metadata_dir=tmp_path / "metadata",
        version="202610020001",
        environment="production",
        database_url="postgresql://secret.example/database",
        cloud_commit="a" * 40,
        authorization_reference="https://github.com/BicameralAI/bicameral-cloud/issues/343",
    )

    assert result["state"] == "verified_already_applied"


def test_separate_authorization_rejects_automatic_migration(tmp_path: Path) -> None:
    write_migration(tmp_path, "202610020001", "expand", compatibility="expand")

    with pytest.raises(ValueError, match="separate_authorization"):
        MODULE.execute(
            migrations_dir=tmp_path / "migrations",
            metadata_dir=tmp_path / "metadata",
            version="202610020001",
            environment="production",
            database_url="postgresql://secret.example/database",
            cloud_commit="a" * 40,
            authorization_reference="https://github.com/BicameralAI/bicameral-cloud/issues/343",
        )
