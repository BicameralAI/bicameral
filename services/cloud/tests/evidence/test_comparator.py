"""Comparator scenario tests covering IAM, ingress, schema, grant, and RLS drift.

These are the sanitized-fixture CI cases required by cloud#141: public Cloud Run,
IAP/load-balancer, missing policy, RLS drift, and principal-grant mismatch. The
central invariant is that inconclusive live evidence never produces agreement.
"""

from __future__ import annotations

from pathlib import Path

from bicameral_cloud.evidence.comparator import Disposition, OverallStatus, compare
from bicameral_cloud.evidence.tracked_state import derive_tracked_state
from tests.evidence.helpers import scenario_bundle

_REPO = Path(__file__).resolve().parents[2]
_TRACKED = derive_tracked_state(
    _REPO / "supabase" / "migrations",
    _REPO / "docs" / "evidence" / "expected-config.json",
)


def test_hardened_environment_agrees() -> None:
    report = compare(_TRACKED, scenario_bundle())
    assert report.overall == OverallStatus.AGREEMENT
    assert report.by_disposition(Disposition.DRIFT) == []
    assert report.by_disposition(Disposition.INCONCLUSIVE) == []


def test_public_cloud_run_is_iam_and_ingress_drift() -> None:
    bundle = scenario_bundle(
        overrides={
            "gcp.cloud_run.iam_policy": "gcp/iam_policy.public.json",
            "gcp.cloud_run.ingress": "gcp/service.public.json",
        }
    )
    report = compare(_TRACKED, bundle)
    assert report.overall == OverallStatus.DRIFT_DETECTED
    drift_dims = {f.dimension for f in report.by_disposition(Disposition.DRIFT)}
    assert "iam" in drift_dims
    assert "ingress" in drift_dims


def test_rls_drift_detected() -> None:
    bundle = scenario_bundle(overrides={"supabase.rls": "supabase/rls.drift.json"})
    report = compare(_TRACKED, bundle)
    rls_drift = [
        f for f in report.by_disposition(Disposition.DRIFT) if f.dimension == "rls"
    ]
    assert any(f.key == "product_directory" for f in rls_drift)
    assert report.overall == OverallStatus.DRIFT_DETECTED


def test_principal_grant_mismatch_detected() -> None:
    bundle = scenario_bundle(
        overrides={
            "supabase.runtime_principal": "supabase/runtime_principal.mismatch.json"
        }
    )
    report = compare(_TRACKED, bundle)
    grant_drift = [
        f for f in report.by_disposition(Disposition.DRIFT) if f.dimension == "grant"
    ]
    assert any(f.key == "product_directory:anon:select" for f in grant_drift)


def test_schema_drift_reports_live_only_and_tracked_only() -> None:
    bundle = scenario_bundle(
        overrides={"supabase.tables": "supabase/tables.drift.json"}
    )
    report = compare(_TRACKED, bundle)
    live_only = {
        f.key
        for f in report.by_disposition(Disposition.LIVE_ONLY)
        if f.dimension == "schema"
    }
    tracked_only = {
        f.key
        for f in report.by_disposition(Disposition.TRACKED_ONLY)
        if f.dimension == "schema"
    }
    assert "shadow_projects" in live_only
    assert "product_directory" in tracked_only


def test_inconclusive_live_evidence_never_reports_agreement() -> None:
    # No false pass: if the Supabase and IAM facts could not be observed, the
    # comparator must not claim agreement.
    bundle = scenario_bundle(
        inconclusive={
            "gcp.cloud_run.iam_policy",
            "gcp.cloud_run.ingress",
            "supabase.tables",
            "supabase.rls",
            "supabase.runtime_principal",
        }
    )
    report = compare(_TRACKED, bundle)
    assert report.overall == OverallStatus.ENVIRONMENT_INCONCLUSIVE
    assert report.by_disposition(Disposition.AGREEMENT) == []
