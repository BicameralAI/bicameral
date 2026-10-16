"""Typed collection outcomes for the reconciliation evidence tooling.

Every collector returns a :class:`CollectionResult`. The single most important
invariant is that *the absence of credentials or infrastructure is a typed,
non-passing outcome* — :data:`CollectionStatus.ENVIRONMENT_INCONCLUSIVE` — never a
silent success and never an unhandled exception. Downstream, the comparator maps an
inconclusive collection to an inconclusive finding so a missing environment can never
be mistaken for agreement.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

# Bumped whenever the bundle schema or a collector command contract changes so a
# produced evidence bundle records the exact tool contract it was generated under.
TOOL_VERSION = "1.0.0"


class CollectionStatus(StrEnum):
    """Typed outcome of a single evidence collection step."""

    # The command ran and returned parseable evidence.
    COLLECTED = "collected"
    # The command ran and authoritatively proved the resource does not exist
    # (e.g. no IAP/load-balancer configured). This is a real, non-drift fact.
    ABSENT = "absent"
    # Credentials, permissions, the CLI tool, or the infrastructure were not
    # available, so the fact could not be observed. NOT a pass and NOT drift.
    ENVIRONMENT_INCONCLUSIVE = "environment_inconclusive"
    # The command ran but its output could not be parsed into the expected shape.
    PARSE_ERROR = "parse_error"

    def is_conclusive(self) -> bool:
        """Whether the outcome carries a usable fact for comparison."""
        return self in (CollectionStatus.COLLECTED, CollectionStatus.ABSENT)


# Deterministic reason codes so callers/tests can branch on *why* a collection was
# inconclusive without string-matching human-readable prose.
class InconclusiveReason(StrEnum):
    TOOL_NOT_FOUND = "tool_not_found"
    NOT_AUTHENTICATED = "not_authenticated"
    PERMISSION_DENIED = "permission_denied"
    NETWORK_UNAVAILABLE = "network_unavailable"
    UNKNOWN = "unknown"


@dataclass(frozen=True)
class CollectionResult:
    """Outcome of running one collector.

    ``payload`` is the sanitized, redaction-safe parsed evidence and is only
    populated for :data:`CollectionStatus.COLLECTED`. ``reason`` explains a
    non-conclusive outcome with a deterministic code.
    """

    status: CollectionStatus
    payload: dict[str, object] | None = None
    reason: InconclusiveReason | None = None
    detail: str = ""
    notes: list[str] = field(default_factory=list)

    @classmethod
    def collected(
        cls, payload: dict[str, object], notes: list[str] | None = None
    ) -> CollectionResult:
        return cls(
            status=CollectionStatus.COLLECTED,
            payload=payload,
            notes=notes or [],
        )

    @classmethod
    def absent(cls, detail: str = "") -> CollectionResult:
        return cls(status=CollectionStatus.ABSENT, detail=detail)

    @classmethod
    def inconclusive(
        cls, reason: InconclusiveReason, detail: str = ""
    ) -> CollectionResult:
        return cls(
            status=CollectionStatus.ENVIRONMENT_INCONCLUSIVE,
            reason=reason,
            detail=detail,
        )

    @classmethod
    def parse_error(cls, detail: str) -> CollectionResult:
        return cls(status=CollectionStatus.PARSE_ERROR, detail=detail)
