from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).parents[1]


def test_v0126_regression_fixture_pins_wrong_and_correct_dashboard_identities() -> None:
    fixture = json.loads((ROOT / "tests/fixtures/v0126-metadata-relabel.json").read_text())
    assert fixture["declared_bot_commit"] == "6be12a6a75fec61bf840a243fbbb41f19db33174"
    assert fixture["stale_hosted_spa"] == (
        "sha256:9fc38bb8bd8cb86287b6772a6cce359bf13fa8ab2580a32134fc93f498709899"
    )
    assert fixture["immutable_dashboard_artifact_sha256"] == (
        "sha256:d3dbbf504b8e3ecf8cdba0955939fe8fc1882767432a917044ecbd3f4c652769"
    )
    assert fixture["immutable_bot_descriptor_digest"] == (
        "sha256:859df211f00c82620310defc915f030ece078ba7e8d133d37d9debeddf440c06"
    )
    assert fixture["immutable_hosted_spa"] == (
        "sha256:9f85daa11b845c0dacacf614801f283eec533f5d4561348629325f1c1890736e"
    )
    assert fixture["stale_hosted_spa"] != fixture["immutable_hosted_spa"]
    assert fixture["expected"] == "reject_before_deployment_evidence"
