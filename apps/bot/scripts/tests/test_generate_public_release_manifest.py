from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path

import pytest

SCRIPT = Path(__file__).parents[1] / "generate_public_release_manifest.py"
ROOT = SCRIPT.parents[1]
SPEC = importlib.util.spec_from_file_location("generate_public_release_manifest", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

BOT_COMMIT = "a" * 40
MCP_COMMIT = "b" * 40
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)


def binding() -> dict[str, object]:
    value: dict[str, object] = {
        "schema_version": 1,
        "release_id": f"sha256:{'c' * 64}",
        "release_manifest_digest": f"sha256:{'d' * 64}",
        "bot_commit": BOT_COMMIT,
        "bot_version": "0.1.14",
        "mcp": {
            "package": "bicameral-mcp",
            "commit": MCP_COMMIT,
            "version": "0.17.0",
            "wheel_url": (
                "https://files.pythonhosted.org/packages/example/"
                "bicameral_mcp-0.17.0-py3-none-any.whl"
            ),
            "wheel_sha256": "e" * 64,
            "release_descriptor_digest": f"sha256:{'f' * 64}",
            "tool_request_protocol": "v2",
            "python_requires": ">=3.11",
        },
    }
    value["binding_digest"] = MODULE.canonical_digest(value)
    return value


def release_dir(tmp_path: Path) -> Path:
    root = tmp_path / "release"
    root.mkdir(parents=True)
    for target in TARGETS:
        (root / f"bicameral-{target}.tar.gz").write_bytes(target.encode())
    return root


def test_compiler_binds_exact_release_and_public_mcp_artifact(tmp_path: Path) -> None:
    compiled = MODULE.compile_manifest(
        binding=binding(),
        release_dir=release_dir(tmp_path),
        tag="v0.1.14",
        bot_commit=BOT_COMMIT,
        bot_version="0.1.14",
        download_base_url="https://downloads.example.test/bicameral",
        released_at="2026-07-30T00:00:00Z",
    )

    assert compiled["schema_version"] == 2
    assert compiled["version"] == "0.1.14"
    assert compiled["tag"] == "v0.1.14"
    assert compiled["bot_commit"] == BOT_COMMIT
    assert compiled["release_id"] == f"sha256:{'c' * 64}"
    assert compiled["release_manifest_digest"] == f"sha256:{'d' * 64}"
    assert compiled["mcp"] == {
        "package": "bicameral-mcp",
        "version": "0.17.0",
        "wheel_url": (
            "https://files.pythonhosted.org/packages/example/"
            "bicameral_mcp-0.17.0-py3-none-any.whl"
        ),
        "wheel_sha256": "e" * 64,
        "release_descriptor_digest": f"sha256:{'f' * 64}",
        "tool_request_protocol": "v2",
        "python_requires": ">=3.11",
    }
    assert list(compiled["assets"]) == sorted(TARGETS)
    target = TARGETS[0]
    assert compiled["assets"][target]["sha256"] == hashlib.sha256(target.encode()).hexdigest()


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("bot_commit", "9" * 40, "Bot commit"),
        ("bot_version", "0.1.13", "Bot version"),
        ("binding_digest", f"sha256:{'0' * 64}", "binding digest"),
    ],
)
def test_compiler_rejects_stale_or_mutated_binding(
    tmp_path: Path, field: str, value: str, message: str
) -> None:
    candidate = binding()
    candidate[field] = value
    with pytest.raises(ValueError, match=message):
        MODULE.compile_manifest(
            binding=candidate,
            release_dir=release_dir(tmp_path),
            tag="v0.1.14",
            bot_commit=BOT_COMMIT,
            bot_version="0.1.14",
            download_base_url="https://downloads.example.test/bicameral",
            released_at="2026-07-30T00:00:00Z",
        )


def test_compiler_rejects_non_exact_wheel_and_asset_matrix(tmp_path: Path) -> None:
    candidate = binding()
    candidate["mcp"]["wheel_url"] = "https://example.test/bicameral_mcp-latest.whl"
    candidate["binding_digest"] = MODULE.canonical_digest(
        {key: value for key, value in candidate.items() if key != "binding_digest"}
    )
    with pytest.raises(ValueError, match="wheel filename"):
        MODULE.compile_manifest(
            binding=candidate,
            release_dir=release_dir(tmp_path),
            tag="v0.1.14",
            bot_commit=BOT_COMMIT,
            bot_version="0.1.14",
            download_base_url="https://downloads.example.test/bicameral",
            released_at="2026-07-30T00:00:00Z",
        )

    incomplete = release_dir(tmp_path / "incomplete")
    (incomplete / f"bicameral-{TARGETS[0]}.tar.gz").unlink()
    with pytest.raises(ValueError, match="asset matrix"):
        MODULE.compile_manifest(
            binding=binding(),
            release_dir=incomplete,
            tag="v0.1.14",
            bot_commit=BOT_COMMIT,
            bot_version="0.1.14",
            download_base_url="https://downloads.example.test/bicameral",
            released_at="2026-07-30T00:00:00Z",
        )


def test_binding_round_trips_as_closed_json(tmp_path: Path) -> None:
    path = tmp_path / "binding.json"
    path.write_text(json.dumps(binding()), encoding="utf-8")
    loaded = MODULE.load_binding(path)
    assert loaded == binding()


def test_public_repo_does_not_carry_release_workflows() -> None:
    for name in ("tag-release-on-main-version.yml", "release.yml"):
        assert not (ROOT / ".github/workflows" / name).exists()
