"""Drive collectors through a runner into typed results and an evidence bundle."""

from __future__ import annotations

from bicameral_cloud.evidence import gcp, supabase
from bicameral_cloud.evidence.bundle import CommandSpec, EvidenceBundle, build_bundle
from bicameral_cloud.evidence.gcp import Collector
from bicameral_cloud.evidence.jsonutil import JsonShapeError
from bicameral_cloud.evidence.outcomes import (
    CollectionResult,
    InconclusiveReason,
)
from bicameral_cloud.evidence.redaction import Redactor
from bicameral_cloud.evidence.runner import (
    CommandRunner,
    MissingEnvError,
    classify_failure,
)

ALL_COLLECTORS: tuple[Collector, ...] = (*gcp.COLLECTORS, *supabase.COLLECTORS)


def run_collector(
    collector: Collector, runner: CommandRunner, redactor: Redactor
) -> CollectionResult:
    """Execute one collector, mapping every failure mode to a typed result."""
    try:
        output = runner.run(collector.spec.command)
    except MissingEnvError as exc:
        return CollectionResult.inconclusive(
            InconclusiveReason.UNKNOWN,
            detail=f"required environment variable not set: {exc.name}",
        )
    if output.tool_missing or output.returncode != 0:
        reason = classify_failure(output)
        detail = (output.stderr or output.stdout).strip().splitlines()
        return CollectionResult.inconclusive(
            reason, detail=detail[0] if detail else f"exit code {output.returncode}"
        )
    try:
        return collector.parse(output.stdout, redactor)
    except (JsonShapeError, ValueError) as exc:
        return CollectionResult.parse_error(str(exc))


def collect_all(
    runner: CommandRunner,
    redactor: Redactor,
    *,
    environment_id: str,
    environment_description: str,
    collectors: tuple[Collector, ...] = ALL_COLLECTORS,
    timestamp: str | None = None,
) -> EvidenceBundle:
    collected: list[tuple[CommandSpec, CollectionResult]] = [
        (collector.spec, run_collector(collector, runner, redactor))
        for collector in collectors
    ]
    return build_bundle(
        collected,
        environment_id=environment_id,
        environment_description=environment_description,
        timestamp=timestamp,
    )
