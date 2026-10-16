from __future__ import annotations

import importlib.util
import io
import sys
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "validate_customer_distribution_boundary.py"
SPEC = importlib.util.spec_from_file_location("customer_distribution_boundary", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
boundary = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = boundary
SPEC.loader.exec_module(boundary)


def _write_tar(path: Path, members: dict[str, bytes]) -> None:
    with tarfile.open(path, "w") as archive:
        for name, data in members.items():
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))


def test_repository_distribution_contract_is_deny_by_default() -> None:
    assert boundary.validate_dockerignore() == []
    assert boundary.validate_dockerfile() == []
    assert boundary.validate_runtime_source() == []


def test_final_container_accepts_clean_runtime_package(tmp_path: Path) -> None:
    image = tmp_path / "image.tar"
    _write_tar(
        image,
        {
            "usr/local/lib/python3.11/site-packages/bicameral_cloud/app.py": b"APP = 'cloud'\n",
            "app/.keep": b"",
        },
    )

    assert boundary.validate_container_tar(image) == []


def test_final_container_rejects_build_only_repository_metadata(tmp_path: Path) -> None:
    image = tmp_path / "image.tar"
    _write_tar(image, {"app/README.md": b"internal repository documentation"})

    violations = boundary.validate_container_tar(image)
    assert any("build-only repository material" in item.rule for item in violations)


def test_final_container_rejects_factory_reference_in_runtime_package(tmp_path: Path) -> None:
    image = tmp_path / "image.tar"
    _write_tar(
        image,
        {
            "usr/local/lib/python3.11/site-packages/bicameral_cloud/example.py": (
                b"https://github.com/BicameralAI/bicameral-factory/issues/243\n"
            )
        },
    )

    violations = boundary.validate_container_tar(image)
    assert any("forbidden reference" in item.rule for item in violations)


def test_retained_artifact_scan_rejects_factory_evidence(tmp_path: Path) -> None:
    artifact = tmp_path / "browser-summary.json"
    artifact.write_text(
        '{"source":".bicameral/factory-attestations/example.json"}\n',
        encoding="utf-8",
    )

    violations = boundary.validate_paths([artifact])
    assert any("forbidden reference" in item.rule for item in violations)
