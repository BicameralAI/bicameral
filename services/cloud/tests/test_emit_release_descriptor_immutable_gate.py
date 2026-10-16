from __future__ import annotations

import importlib.util
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]


def _load_release_descriptor_module():
    path = ROOT / "scripts/emit_release_descriptor.py"
    spec = importlib.util.spec_from_file_location("emit_release_descriptor_test", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_descriptor_cli_separates_validation_from_producer_proof() -> None:
    script = (ROOT / "scripts/emit_release_descriptor.py").read_text()
    assert "--immutable-bot-artifact" in script
    assert "--validation-only" in script
    assert "verify_immutable_dashboard" in script
    assert "served-dashboard-provenance.json" in script
    assert "immutable_bot_dashboard" in script
    assert "bot_release_descriptor" in script
    assert "choose exactly one" in script


def test_trusted_deploy_reconstructs_exact_private_bot_before_materializing_release() -> None:
    workflow = (ROOT / ".github/workflows/deploy-cloud-run.yml").read_text()
    assert "Check out exact private Bot source" in workflow
    assert "repository: BicameralAI/bicameral-bot" in workflow
    assert "ref: ${{ steps.bot_source.outputs.sha }}" in workflow
    assert "BICAMERAL_CROSS_REPO_TOKEN || secrets.BICAMERAL_BOT_READ_TOKEN" in workflow
    assert "cargo build --release --locked --bin bicameral" in workflow
    assert "npm ci --silent && npm run build" in workflow
    assert "tar --sort=name --mtime=@0" in workflow
    assert (
        '--immutable-bot-artifact "${BOT_CANONICAL_SOURCE_PATH}/release-dashboard/bicameral-dashboard.tar.gz"'
        in workflow
    )


def test_cloud_runtime_digest_excludes_materialized_bot_dashboard(tmp_path: Path) -> None:
    runtime = tmp_path / "src/bicameral_cloud/runtime.py"
    dashboard = (
        tmp_path
        / "src/bicameral_cloud/static/dashboard/assets/index-old-dashboard.js"
    )
    runtime.parent.mkdir(parents=True, exist_ok=True)
    dashboard.parent.mkdir(parents=True, exist_ok=True)
    runtime.write_text("RUNTIME = True\n", encoding="utf-8")
    dashboard.write_text("console.log('old dashboard')\n", encoding="utf-8")

    subprocess.run(["git", "init"], cwd=tmp_path, check=True, capture_output=True)
    subprocess.run(["git", "add", "src"], cwd=tmp_path, check=True)

    module = _load_release_descriptor_module()
    module.ROOT = tmp_path

    # Trusted deploy materialization replaces hashed Vite assets after checkout.
    # The old tracked dashboard path therefore disappears from the worktree.
    dashboard.unlink()

    digest = module.tracked_tree_digest(
        "src", exclude_roots=(module.BOT_DASHBOARD_TRACKED_ROOT,)
    )
    assert module.DIGEST.fullmatch(digest)

    # Excluding the Bot-owned dashboard must not hide missing Cloud runtime source.
    runtime.unlink()
    with pytest.raises(ValueError, match="tracked release input is missing"):
        module.tracked_tree_digest(
            "src", exclude_roots=(module.BOT_DASHBOARD_TRACKED_ROOT,)
        )


def test_pr_release_provenance_is_explicitly_validation_only() -> None:
    workflow = (ROOT / ".github/workflows/release-provenance.yml").read_text()
    assert "python scripts/emit_release_descriptor.py --validation-only" in workflow
    assert "trusted producer reconstruction is enforced by deploy-cloud-run.yml before deployment" in workflow
