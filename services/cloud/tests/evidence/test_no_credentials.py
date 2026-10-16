"""The whole tool must run and parse with no live credentials (cloud#141).

Absence of every CLI is a typed ``environment_inconclusive`` bundle — never a crash
and never a false pass — and that bundle still validates against the schema.
"""

from __future__ import annotations

from pathlib import Path

from bicameral_cloud.evidence.bundle import validate_bundle
from bicameral_cloud.evidence.collect import collect_all
from bicameral_cloud.evidence.comparator import OverallStatus, compare
from bicameral_cloud.evidence.outcomes import CollectionStatus, InconclusiveReason
from bicameral_cloud.evidence.redaction import Redactor
from bicameral_cloud.evidence.runner import SubprocessRunner
from bicameral_cloud.evidence.tracked_state import derive_tracked_state
from tests.evidence.helpers import MissingToolRunner

_REPO = Path(__file__).resolve().parents[2]


def test_missing_tools_produce_all_inconclusive() -> None:
    bundle = collect_all(
        MissingToolRunner(),
        Redactor(),
        environment_id="ci",
        environment_description="no credentials",
        timestamp="2026-07-15T00:00:00Z",
    )
    assert bundle.items, "expected collectors to run"
    assert all(
        item.status == CollectionStatus.ENVIRONMENT_INCONCLUSIVE
        for item in bundle.items
    )
    assert all(
        item.reason == InconclusiveReason.TOOL_NOT_FOUND.value for item in bundle.items
    )
    assert validate_bundle(bundle.to_json()) == []


def test_inconclusive_bundle_compares_as_inconclusive() -> None:
    bundle = collect_all(
        MissingToolRunner(),
        Redactor(),
        environment_id="ci",
        environment_description="no credentials",
        timestamp="2026-07-15T00:00:00Z",
    )
    tracked = derive_tracked_state(
        _REPO / "supabase" / "migrations",
        _REPO / "docs" / "evidence" / "expected-config.json",
    )
    report = compare(tracked, bundle)
    assert report.overall == OverallStatus.ENVIRONMENT_INCONCLUSIVE


def test_real_subprocess_runner_without_creds_does_not_raise() -> None:
    # SubprocessRunner with an empty environment resolves nothing; missing env
    # placeholders and absent tools must still yield a valid, typed bundle.
    bundle = collect_all(
        SubprocessRunner(environ={}),
        Redactor(),
        environment_id="ci",
        environment_description="empty environment",
        timestamp="2026-07-15T00:00:00Z",
    )
    assert validate_bundle(bundle.to_json()) == []
    assert not any(item.status == CollectionStatus.COLLECTED for item in bundle.items)
