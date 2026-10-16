"""Shared helpers for evidence-tooling tests: fixture loading and scenario runners."""

from __future__ import annotations

from pathlib import Path

from bicameral_cloud.evidence.bundle import CommandSpec, EvidenceBundle, build_bundle
from bicameral_cloud.evidence.collect import ALL_COLLECTORS
from bicameral_cloud.evidence.outcomes import (
    CollectionResult,
    CollectionStatus,
    InconclusiveReason,
)
from bicameral_cloud.evidence.redaction import Redactor
from bicameral_cloud.evidence.runner import RunOutput

FIXTURES = Path(__file__).parent / "fixtures"


def load_fixture(relative: str) -> str:
    return (FIXTURES / relative).read_text(encoding="utf-8")


# Maps every collector id to the fixture that feeds its parser for a fully
# populated, hardened environment. The four Cloud Run collectors share one
# service-describe fixture, exactly like the real command.
HARDENED_FIXTURES: dict[str, str] = {
    "gcp.cloud_run.service": "gcp/service.hardened.json",
    "gcp.cloud_run.ingress": "gcp/service.hardened.json",
    "gcp.cloud_run.revision": "gcp/service.hardened.json",
    "gcp.cloud_run.service_account": "gcp/service.hardened.json",
    "gcp.cloud_run.iam_policy": "gcp/iam_policy.private.json",
    "gcp.load_balancer.backend_services": "gcp/backend_services.present.json",
    "gcp.iap.web_iam_policy": "gcp/iap.present.json",
    "supabase.schemas": "supabase/schemas.baseline.json",
    "supabase.tables": "supabase/tables.baseline.json",
    "supabase.grants": "supabase/grants.baseline.json",
    "supabase.default_privileges": "supabase/default_privileges.baseline.json",
    "supabase.rls": "supabase/rls.enabled.json",
    "supabase.policies": "supabase/policies.missing.json",
    "supabase.runtime_principal": "supabase/runtime_principal.hardened.json",
}


def scenario_bundle(
    overrides: dict[str, str] | None = None,
    *,
    inconclusive: set[str] | None = None,
    redactor: Redactor | None = None,
) -> EvidenceBundle:
    """Build a bundle from fixtures, applying per-collector overrides.

    ``overrides`` maps a collector id to a fixture path to use instead of the
    hardened default. Ids listed in ``inconclusive`` are emitted as typed
    ``environment_inconclusive`` items (as if the CLI/credential were absent).
    """
    overrides = overrides or {}
    inconclusive = inconclusive or set()
    redactor = redactor or Redactor()
    collected: list[tuple[CommandSpec, CollectionResult]] = []
    for collector in ALL_COLLECTORS:
        if collector.spec.id in inconclusive:
            result = CollectionResult.inconclusive(InconclusiveReason.TOOL_NOT_FOUND)
        else:
            fixture = overrides.get(
                collector.spec.id, HARDENED_FIXTURES[collector.spec.id]
            )
            result = collector.parse(load_fixture(fixture), redactor)
        collected.append((collector.spec, result))
    return build_bundle(
        collected,
        environment_id="test",
        environment_description="scenario",
        timestamp="2026-07-15T00:00:00Z",
    )


class MissingToolRunner:
    """A runner that behaves as if every CLI is absent (no credentials path)."""

    def run(self, command: list[str]) -> RunOutput:
        return RunOutput(returncode=127, stdout="", stderr="", tool_missing=True)


class MappedRunner:
    """A runner that returns canned output based on the command's tool token."""

    def __init__(self, outputs: dict[str, RunOutput]):
        self._outputs = outputs

    def run(self, command: list[str]) -> RunOutput:
        tool = command[0] if command else ""
        return self._outputs.get(
            tool, RunOutput(returncode=1, stdout="", stderr="unmapped")
        )


def status_of(bundle: EvidenceBundle, item_id: str) -> CollectionStatus:
    for item in bundle.items:
        if item.id == item_id:
            return item.status
    raise KeyError(item_id)
