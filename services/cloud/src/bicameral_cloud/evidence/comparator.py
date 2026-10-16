"""Deterministic repository-state comparator.

Maps the repository's tracked expectations against a produced evidence bundle and
reports, per field: agreement, drift, live-only configuration, tracked-only
configuration, or inconclusive. Whenever the backing live evidence is inconclusive
the finding is ``inconclusive`` — a missing environment can never be reported as
agreement (no false pass).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from bicameral_cloud.evidence.bundle import EvidenceBundle, EvidenceItem
from bicameral_cloud.evidence.outcomes import CollectionStatus
from bicameral_cloud.evidence.tracked_state import TrackedState


class Disposition(StrEnum):
    AGREEMENT = "agreement"
    DRIFT = "drift"
    LIVE_ONLY = "live_only"
    TRACKED_ONLY = "tracked_only"
    INCONCLUSIVE = "inconclusive"


class OverallStatus(StrEnum):
    AGREEMENT = "agreement"
    DRIFT_DETECTED = "drift_detected"
    ENVIRONMENT_INCONCLUSIVE = "environment_inconclusive"


@dataclass(frozen=True)
class ComparisonFinding:
    dimension: str
    key: str
    disposition: Disposition
    detail: str = ""
    tracked: object = None
    observed: object = None

    def to_json(self) -> dict[str, object]:
        return {
            "dimension": self.dimension,
            "key": self.key,
            "disposition": self.disposition.value,
            "detail": self.detail,
            "tracked": self.tracked,
            "observed": self.observed,
        }


@dataclass(frozen=True)
class ComparisonReport:
    overall: OverallStatus
    findings: list[ComparisonFinding] = field(default_factory=list)

    def by_disposition(self, disposition: Disposition) -> list[ComparisonFinding]:
        return [f for f in self.findings if f.disposition == disposition]

    def summary(self) -> dict[str, int]:
        counts = {disposition.value: 0 for disposition in Disposition}
        for finding in self.findings:
            counts[finding.disposition.value] += 1
        return counts

    def to_json(self) -> dict[str, object]:
        return {
            "overall": self.overall.value,
            "summary": self.summary(),
            "findings": [finding.to_json() for finding in self.findings],
        }


def _index(bundle: EvidenceBundle) -> dict[str, EvidenceItem]:
    return {item.id: item for item in bundle.items}


def _collected_payload(item: EvidenceItem | None) -> dict[str, object] | None:
    if (
        item is None
        or item.status != CollectionStatus.COLLECTED
        or item.payload is None
    ):
        return None
    return item.payload


def _inconclusive(
    dimension: str, key: str, item: EvidenceItem | None
) -> ComparisonFinding:
    reason = "no evidence item present"
    if item is not None:
        reason = f"live evidence {item.status.value}"
        if item.detail:
            reason += f": {item.detail}"
    return ComparisonFinding(dimension, key, Disposition.INCONCLUSIVE, reason)


def _compare_iam(
    items: dict[str, EvidenceItem], tracked: TrackedState
) -> list[ComparisonFinding]:
    item = items.get("gcp.cloud_run.iam_policy")
    payload = _collected_payload(item)
    if payload is None:
        return [_inconclusive("iam", "public_invoker", item)]
    observed = bool(payload.get("public_invoker"))
    expected = tracked.cloud_run_public_invoker_expected
    if expected is None:
        return [
            ComparisonFinding(
                "iam",
                "public_invoker",
                Disposition.LIVE_ONLY,
                "repository does not pin invoker exposure",
                None,
                observed,
            )
        ]
    disposition = Disposition.AGREEMENT if observed == expected else Disposition.DRIFT
    detail = (
        ""
        if disposition is Disposition.AGREEMENT
        else "public invoker exposure differs from pinned posture"
    )
    return [
        ComparisonFinding(
            "iam", "public_invoker", disposition, detail, expected, observed
        )
    ]


def _compare_ingress(
    items: dict[str, EvidenceItem], tracked: TrackedState
) -> list[ComparisonFinding]:
    item = items.get("gcp.cloud_run.ingress")
    payload = _collected_payload(item)
    if payload is None:
        return [_inconclusive("ingress", "cloud_run", item)]
    observed = str(payload.get("ingress", ""))
    expected = tracked.cloud_run_ingress_expected
    if expected is not None:
        disposition = (
            Disposition.AGREEMENT if observed == expected else Disposition.DRIFT
        )
        return [
            ComparisonFinding(
                "ingress", "cloud_run", disposition, "", expected, observed
            )
        ]
    if tracked.cloud_run_ingress_allowed:
        if observed in tracked.cloud_run_ingress_allowed:
            return [
                ComparisonFinding(
                    "ingress",
                    "cloud_run",
                    Disposition.AGREEMENT,
                    "within allowed ingress posture",
                    list(tracked.cloud_run_ingress_allowed),
                    observed,
                )
            ]
        return [
            ComparisonFinding(
                "ingress",
                "cloud_run",
                Disposition.DRIFT,
                "ingress outside allowed posture",
                list(tracked.cloud_run_ingress_allowed),
                observed,
            )
        ]
    return [
        ComparisonFinding(
            "ingress",
            "cloud_run",
            Disposition.LIVE_ONLY,
            "repository does not pin ingress",
            None,
            observed,
        )
    ]


def _compare_schema(
    items: dict[str, EvidenceItem], tracked: TrackedState
) -> list[ComparisonFinding]:
    item = items.get("supabase.tables")
    payload = _collected_payload(item)
    if payload is None:
        return [_inconclusive("schema", "public_tables", item)]
    raw_tables = payload.get("tables")
    observed = (
        {name for name in raw_tables if isinstance(name, str)}
        if isinstance(raw_tables, list)
        else set()
    )
    tracked_tables = tracked.table_names()
    findings: list[ComparisonFinding] = []
    for name in sorted(tracked_tables | observed):
        in_tracked = name in tracked_tables
        in_live = name in observed
        if in_tracked and in_live:
            findings.append(ComparisonFinding("schema", name, Disposition.AGREEMENT))
        elif in_tracked:
            findings.append(
                ComparisonFinding(
                    "schema", name, Disposition.TRACKED_ONLY, "table not observed live"
                )
            )
        else:
            findings.append(
                ComparisonFinding(
                    "schema",
                    name,
                    Disposition.LIVE_ONLY,
                    "table present live but not tracked",
                )
            )
    return findings


def _observed_rls(payload: dict[str, object]) -> dict[str, bool]:
    result: dict[str, bool] = {}
    raw = payload.get("tables")
    if isinstance(raw, list):
        for row in raw:
            if isinstance(row, dict) and isinstance(row.get("table_name"), str):
                result[row["table_name"]] = bool(row.get("rls_enabled"))
    return result


def _compare_rls(
    items: dict[str, EvidenceItem], tracked: TrackedState
) -> list[ComparisonFinding]:
    item = items.get("supabase.rls")
    payload = _collected_payload(item)
    if payload is None:
        return [_inconclusive("rls", "public_tables", item)]
    observed = _observed_rls(payload)
    findings: list[ComparisonFinding] = []
    for name, expectation in sorted(tracked.tables.items()):
        if not expectation.rls_enabled:
            continue
        if name not in observed:
            findings.append(
                ComparisonFinding(
                    "rls",
                    name,
                    Disposition.TRACKED_ONLY,
                    "table not observed live",
                    True,
                    None,
                )
            )
        elif observed[name]:
            findings.append(
                ComparisonFinding("rls", name, Disposition.AGREEMENT, "", True, True)
            )
        else:
            findings.append(
                ComparisonFinding(
                    "rls",
                    name,
                    Disposition.DRIFT,
                    "RLS disabled live but enabled in migrations",
                    True,
                    False,
                )
            )
    return findings


def _observed_principals(payload: dict[str, object]) -> dict[str, dict[str, bool]]:
    result: dict[str, dict[str, bool]] = {}
    principals = payload.get("principals")
    if not isinstance(principals, dict):
        return result
    for principal, rows in principals.items():
        table_map: dict[str, bool] = {}
        if isinstance(rows, list):
            for row in rows:
                if isinstance(row, dict) and isinstance(row.get("table_name"), str):
                    table_map[row["table_name"]] = bool(row.get("select_granted"))
        result[principal] = table_map
    return result


def _compare_grants(
    items: dict[str, EvidenceItem], tracked: TrackedState
) -> list[ComparisonFinding]:
    item = items.get("supabase.runtime_principal")
    payload = _collected_payload(item)
    if payload is None:
        return [_inconclusive("grant", "runtime_principal", item)]
    observed = _observed_principals(payload)
    findings: list[ComparisonFinding] = []
    for name, expectation in sorted(tracked.tables.items()):
        for principal, expected in sorted(expectation.principal_select.items()):
            key = f"{name}:{principal}:select"
            table_map = observed.get(principal)
            if table_map is None or name not in table_map:
                findings.append(
                    ComparisonFinding(
                        "grant",
                        key,
                        Disposition.TRACKED_ONLY,
                        "principal/table not observed live",
                        expected,
                        None,
                    )
                )
                continue
            actual = table_map[name]
            if actual == expected:
                findings.append(
                    ComparisonFinding(
                        "grant", key, Disposition.AGREEMENT, "", expected, actual
                    )
                )
            else:
                findings.append(
                    ComparisonFinding(
                        "grant",
                        key,
                        Disposition.DRIFT,
                        "principal grant mismatch",
                        expected,
                        actual,
                    )
                )
    return findings


def compare(tracked: TrackedState, bundle: EvidenceBundle) -> ComparisonReport:
    items = _index(bundle)
    findings: list[ComparisonFinding] = []
    findings.extend(_compare_iam(items, tracked))
    findings.extend(_compare_ingress(items, tracked))
    findings.extend(_compare_schema(items, tracked))
    findings.extend(_compare_rls(items, tracked))
    findings.extend(_compare_grants(items, tracked))

    dispositions = {finding.disposition for finding in findings}
    if Disposition.INCONCLUSIVE in dispositions:
        overall = OverallStatus.ENVIRONMENT_INCONCLUSIVE
    elif dispositions & {
        Disposition.DRIFT,
        Disposition.LIVE_ONLY,
        Disposition.TRACKED_ONLY,
    }:
        overall = OverallStatus.DRIFT_DETECTED
    else:
        overall = OverallStatus.AGREEMENT
    return ComparisonReport(overall=overall, findings=findings)
