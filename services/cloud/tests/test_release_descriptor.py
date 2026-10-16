from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SCRIPT = Path(__file__).parents[1] / "scripts" / "emit_release_descriptor.py"
SPEC = importlib.util.spec_from_file_location("cloud_release_descriptor", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def test_descriptor_digest_rejects_mutation() -> None:
    payload = {
        "schema_version": 1,
        "component": "cloud",
        "commit": "a" * 40,
        "selected_bot_commit": "b" * 40,
        "artifacts": {
            "hosted_spa": "sha256:" + "c" * 64,
            "immutable_bot_dashboard": "sha256:" + "e" * 64,
            "bot_release_descriptor": "sha256:" + "f" * 64,
        },
        "interfaces": {"deployment": "sha256:" + "d" * 64},
        "source_artifact_sha256": "e" * 64,
        "source_bot_descriptor_digest": "sha256:" + "f" * 64,
    }
    payload["descriptor_digest"] = MODULE.canonical_digest(payload)
    assert MODULE.validate_descriptor(payload) == []
    payload["selected_bot_commit"] = "e" * 40
    assert "descriptor_digest does not bind the descriptor" in (
        MODULE.validate_descriptor(payload)
    )


def test_committed_dashboard_metadata_matches_served_bytes() -> None:
    descriptor = MODULE.build_descriptor("a" * 40)
    migration_set = MODULE.load_migration_delivery().compile_migration_set(
        migrations_dir=Path(__file__).parents[1] / "supabase" / "migrations",
        metadata_dir=Path(__file__).parents[1] / "supabase" / "migration-metadata",
        cloud_commit="a" * 40,
    )
    assert descriptor["selected_bot_commit"]
    assert descriptor["artifacts"]["hosted_spa"].startswith("sha256:")
    assert descriptor["artifacts"]["immutable_bot_dashboard"].startswith("sha256:")
    assert descriptor["artifacts"]["bot_release_descriptor"].startswith("sha256:")
    assert (
        descriptor["artifacts"]["database_migration_set"]
        == migration_set["migration_set_id"]
    )
    assert descriptor["interfaces"]["migration_delivery"].startswith("sha256:")


def test_tracked_runtime_digest_can_exclude_replaced_vendored_dashboard(
    tmp_path: Path, monkeypatch,
) -> None:
    runtime = tmp_path / "src" / "bicameral_cloud" / "app.py"
    runtime.parent.mkdir(parents=True)
    runtime.write_text("runtime\n", encoding="utf-8")

    tracked = (
        b"src/bicameral_cloud/app.py\0"
        b"src/bicameral_cloud/static/dashboard/assets/old-bundle.js\0"
    )

    monkeypatch.setattr(MODULE, "ROOT", tmp_path)
    monkeypatch.setattr(MODULE.subprocess, "check_output", lambda *args, **kwargs: tracked)

    digest = MODULE.tracked_tree_digest(
        "src",
        exclude_roots=("src/bicameral_cloud/static/dashboard",),
    )

    assert digest.startswith("sha256:")
