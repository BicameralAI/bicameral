from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import sys
import tarfile
from pathlib import Path

SCRIPT = Path(__file__).parents[1] / "scripts" / "verify_bot_dashboard_provenance.py"
SPEC = importlib.util.spec_from_file_location("verify_bot_dashboard_provenance", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

BOT_COMMIT = "a" * 40


def _tree_digest(files: dict[str, bytes]) -> str:
    return MODULE.content_tree_digest(files.items())


def _descriptor(files: dict[str, bytes]) -> dict[str, object]:
    payload: dict[str, object] = {
        "schema_version": 1,
        "component": "bot",
        "commit": BOT_COMMIT,
        "artifacts": {
            "daemon": "sha256:" + "d" * 64,
            "hosted_spa": _tree_digest(files),
        },
        "interfaces": {
            "hosted_bridge": "sha256:" + "e" * 64,
            "integration_ingress": "sha256:" + "f" * 64,
            "mcp_tool_request": "sha256:" + "1" * 64,
        },
    }
    payload["descriptor_digest"] = MODULE.canonical_digest(payload)
    return payload


def _write_artifact(path: Path, files: dict[str, bytes], descriptor: dict[str, object]) -> None:
    descriptor_bytes = (json.dumps(descriptor, indent=2, sort_keys=True) + "\n").encode()
    with tarfile.open(path, "w:gz") as archive:
        for name, content in {
            **{f"dashboard/dist/{key}": value for key, value in files.items()},
            "release-dashboard/bot-release-descriptor.json": descriptor_bytes,
        }.items():
            info = tarfile.TarInfo(name)
            info.size = len(content)
            archive.addfile(info, io.BytesIO(content))


def _materialize_root(
    root: Path,
    *,
    vendored_files: dict[str, bytes],
    descriptor: dict[str, object],
    artifact: Path,
) -> None:
    static = root / "src/bicameral_cloud/static/dashboard"
    static.mkdir(parents=True)
    for name, content in vendored_files.items():
        target = static / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)

    checked = root / "release-inputs/bot-release-descriptor.json"
    checked.parent.mkdir(parents=True)
    checked.write_text(json.dumps(descriptor, indent=2, sort_keys=True) + "\n")

    version = {
        "artifact_sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
        "bot_commit": BOT_COMMIT,
        "bot_descriptor_digest": descriptor["descriptor_digest"],
        "bundle_sha256": _tree_digest(vendored_files).removeprefix("sha256:"),
        "hosted_bridge_contract_sha256": "2" * 64,
        "source_repository": "BicameralAI/bicameral-bot",
        "source_run_url": "https://github.com/BicameralAI/bicameral-bot/actions/runs/1",
    }
    (static / "version.json").write_text(json.dumps(version, indent=2) + "\n")


def test_accepts_exact_immutable_bot_artifact(tmp_path: Path) -> None:
    files = {"index.html": b"new", "assets/app.js": b"console.log('new')"}
    descriptor = _descriptor(files)
    artifact = tmp_path / "dashboard.tar.gz"
    _write_artifact(artifact, files, descriptor)
    root = tmp_path / "root"
    _materialize_root(root, vendored_files=files, descriptor=descriptor, artifact=artifact)

    errors, receipt = MODULE.verify(root, artifact=artifact, expected_bot_commit=BOT_COMMIT)

    assert errors == []
    assert receipt is not None
    assert receipt["bot_commit"] == BOT_COMMIT
    assert receipt["hosted_spa_sha256"] == descriptor["artifacts"]["hosted_spa"]


def test_rejects_v0126_style_metadata_only_relabel(tmp_path: Path) -> None:
    immutable_files = {"index.html": b"new", "assets/app.js": b"console.log('new')"}
    immutable_descriptor = _descriptor(immutable_files)
    artifact = tmp_path / "dashboard.tar.gz"
    _write_artifact(artifact, immutable_files, immutable_descriptor)

    stale_files = {"index.html": b"old", "assets/app.js": b"console.log('old')"}
    relabeled_descriptor = _descriptor(stale_files)
    root = tmp_path / "root"
    _materialize_root(
        root,
        vendored_files=stale_files,
        descriptor=relabeled_descriptor,
        artifact=artifact,
    )

    errors, receipt = MODULE.verify(root, artifact=artifact, expected_bot_commit=BOT_COMMIT)

    assert receipt is None
    assert any("checked-in Bot release descriptor differs" in error for error in errors)
    assert any("vendored Cloud dashboard differs" in error for error in errors)


def test_rejects_forged_artifact_digest(tmp_path: Path) -> None:
    files = {"index.html": b"new", "assets/app.js": b"console.log('new')"}
    descriptor = _descriptor(files)
    artifact = tmp_path / "dashboard.tar.gz"
    _write_artifact(artifact, files, descriptor)
    root = tmp_path / "root"
    _materialize_root(root, vendored_files=files, descriptor=descriptor, artifact=artifact)

    version_path = root / "src/bicameral_cloud/static/dashboard/version.json"
    version = json.loads(version_path.read_text())
    version["artifact_sha256"] = "0" * 64
    version_path.write_text(json.dumps(version) + "\n")

    errors, receipt = MODULE.verify(root, artifact=artifact, expected_bot_commit=BOT_COMMIT)

    assert receipt is None
    assert any("artifact digest does not match" in error for error in errors)
