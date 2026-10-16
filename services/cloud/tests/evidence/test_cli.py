"""CLI surface tests: plan, export (no creds), validate, compare."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from bicameral_cloud.evidence.cli import main
from tests.evidence.helpers import scenario_bundle


def test_plan_lists_all_collectors(capsys: pytest.CaptureFixture[str]) -> None:
    assert main(["plan"]) == 0
    document = json.loads(capsys.readouterr().out)
    ids = {entry["id"] for entry in document["commands"]}
    assert "gcp.cloud_run.iam_policy" in ids
    assert "supabase.runtime_principal" in ids


def test_export_without_credentials_is_inconclusive(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert main(["export", "--environment", "ci"]) == 0
    document = json.loads(capsys.readouterr().out)
    statuses = {item["status"] for item in document["items"]}
    assert statuses == {"environment_inconclusive"}


def test_export_fail_on_inconclusive_exits_nonzero() -> None:
    assert main(["export", "--environment", "ci", "--fail-on-inconclusive"]) == 3


def test_validate_and_compare_roundtrip(tmp_path: Path) -> None:
    bundle_path = tmp_path / "bundle.json"
    bundle_path.write_text(json.dumps(scenario_bundle().to_json()), encoding="utf-8")
    assert main(["validate", str(bundle_path)]) == 0
    report_path = tmp_path / "report.json"
    assert main(["compare", str(bundle_path), "--output", str(report_path)]) == 0
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["overall"] == "agreement"


def test_compare_fail_on_drift(tmp_path: Path) -> None:
    bundle = scenario_bundle(overrides={"supabase.rls": "supabase/rls.drift.json"})
    bundle_path = tmp_path / "bundle.json"
    bundle_path.write_text(json.dumps(bundle.to_json()), encoding="utf-8")
    assert main(["compare", str(bundle_path), "--fail-on-drift"]) == 4
