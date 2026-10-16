"""Machine-readable evidence bundle assembly and validation.

A bundle is the single artifact an authorized owner attaches to the Cloud #106 gate.
Each :class:`EvidenceItem` records the **exact command**, a **timestamp**, an
**environment identifier**, the typed collection **status**, and a **sanitized
output plus its digest**. Validation is stdlib-only and fail-closed so CI can assert
bundle shape without extra dependencies.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime

from bicameral_cloud.evidence import jsonutil
from bicameral_cloud.evidence.outcomes import (
    TOOL_VERSION,
    CollectionResult,
    CollectionStatus,
)
from bicameral_cloud.evidence.redaction import sanitized_digest

SCHEMA_VERSION = 1

# Evidence categories the bundle can carry, aligned to the Cloud #106 required facts.
CATEGORIES = frozenset(
    {
        "cloud_run_service",
        "cloud_run_ingress",
        "cloud_run_revision",
        "cloud_run_service_account",
        "cloud_run_iam_policy",
        "load_balancer",
        "iap",
        "supabase_schemas",
        "supabase_tables",
        "supabase_grants",
        "supabase_default_privileges",
        "supabase_rls",
        "supabase_policies",
        "runtime_principal",
    }
)


@dataclass(frozen=True)
class CommandSpec:
    """A single collectable fact and the exact command that observes it.

    ``command`` is stored in *placeholder* form (e.g. ``${GCP_PROJECT}``) so the
    bundle documents precisely what to run without ever embedding a resolved secret
    or confidential identifier.
    """

    id: str
    category: str
    description: str
    command: list[str]
    required_env: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        if self.category not in CATEGORIES:
            raise ValueError(f"unknown evidence category: {self.category}")


@dataclass(frozen=True)
class EvidenceItem:
    id: str
    category: str
    description: str
    command: list[str]
    environment_id: str
    timestamp: str
    status: CollectionStatus
    reason: str | None
    detail: str
    payload: dict[str, object] | None
    digest: str | None
    notes: list[str]

    def to_json(self) -> dict[str, object]:
        return {
            "id": self.id,
            "category": self.category,
            "description": self.description,
            "command": list(self.command),
            "environment_id": self.environment_id,
            "timestamp": self.timestamp,
            "status": self.status.value,
            "reason": self.reason,
            "detail": self.detail,
            "payload": self.payload,
            "digest": self.digest,
            "notes": list(self.notes),
        }

    @classmethod
    def from_json(cls, document: dict[str, object]) -> EvidenceItem:
        payload = document.get("payload")
        reason = document.get("reason")
        return cls(
            id=jsonutil.get_str(document, "id"),
            category=jsonutil.get_str(document, "category"),
            description=jsonutil.get_str(document, "description"),
            command=[
                c for c in jsonutil.get_array(document, "command") if isinstance(c, str)
            ],
            environment_id=jsonutil.get_str(document, "environment_id"),
            timestamp=jsonutil.get_str(document, "timestamp"),
            status=CollectionStatus(jsonutil.get_str(document, "status")),
            reason=reason if isinstance(reason, str) else None,
            detail=jsonutil.get_str(document, "detail"),
            payload=payload if isinstance(payload, dict) else None,
            digest=jsonutil.get_str(document, "digest") or None,
            notes=[
                n for n in jsonutil.get_array(document, "notes") if isinstance(n, str)
            ],
        )


@dataclass(frozen=True)
class EvidenceBundle:
    schema_version: int
    tool_version: str
    generated_at: str
    environment_id: str
    environment_description: str
    items: list[EvidenceItem]

    def summary(self) -> dict[str, int]:
        counts: dict[str, int] = {status.value: 0 for status in CollectionStatus}
        for item in self.items:
            counts[item.status.value] += 1
        return counts

    def has_inconclusive(self) -> bool:
        return any(
            item.status == CollectionStatus.ENVIRONMENT_INCONCLUSIVE
            for item in self.items
        )

    def to_json(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "tool_version": self.tool_version,
            "generated_at": self.generated_at,
            "environment": {
                "id": self.environment_id,
                "description": self.environment_description,
            },
            "summary": self.summary(),
            "items": [item.to_json() for item in self.items],
        }

    @classmethod
    def from_json(cls, document: object) -> EvidenceBundle:
        root = jsonutil.as_object(document, context="bundle")
        environment = jsonutil.get_object(root, "environment")
        version = root.get("schema_version")
        return cls(
            schema_version=version if isinstance(version, int) else SCHEMA_VERSION,
            tool_version=jsonutil.get_str(root, "tool_version"),
            generated_at=jsonutil.get_str(root, "generated_at"),
            environment_id=jsonutil.get_str(environment, "id"),
            environment_description=jsonutil.get_str(environment, "description"),
            items=[
                EvidenceItem.from_json(jsonutil.as_object(item, context="item"))
                for item in jsonutil.get_array(root, "items")
            ],
        )


def _now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def build_item(
    spec: CommandSpec,
    result: CollectionResult,
    *,
    environment_id: str,
    timestamp: str | None = None,
) -> EvidenceItem:
    payload = result.payload if result.status == CollectionStatus.COLLECTED else None
    digest = sanitized_digest(payload) if payload is not None else None
    return EvidenceItem(
        id=spec.id,
        category=spec.category,
        description=spec.description,
        command=list(spec.command),
        environment_id=environment_id,
        timestamp=timestamp or _now_iso(),
        status=result.status,
        reason=result.reason.value if result.reason is not None else None,
        detail=result.detail,
        payload=payload,
        digest=digest,
        notes=list(result.notes),
    )


def build_bundle(
    collected: list[tuple[CommandSpec, CollectionResult]],
    *,
    environment_id: str,
    environment_description: str,
    timestamp: str | None = None,
) -> EvidenceBundle:
    items = [
        build_item(spec, result, environment_id=environment_id, timestamp=timestamp)
        for spec, result in collected
    ]
    return EvidenceBundle(
        schema_version=SCHEMA_VERSION,
        tool_version=TOOL_VERSION,
        generated_at=timestamp or _now_iso(),
        environment_id=environment_id,
        environment_description=environment_description,
        items=items,
    )


_ALLOWED_STATUSES = {status.value for status in CollectionStatus}


def validate_bundle(document: object) -> list[str]:
    """Return a list of schema problems; empty means the bundle is valid."""
    errors: list[str] = []
    try:
        root = jsonutil.as_object(document, context="bundle")
    except jsonutil.JsonShapeError as exc:
        return [str(exc)]

    if root.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}")
    if not jsonutil.get_str(root, "tool_version"):
        errors.append("tool_version must be a non-empty string")
    if not jsonutil.get_str(root, "generated_at"):
        errors.append("generated_at must be a non-empty string")

    environment = jsonutil.get_object(root, "environment")
    if not jsonutil.get_str(environment, "id"):
        errors.append("environment.id must be a non-empty string")

    items = root.get("items")
    if not isinstance(items, list):
        errors.append("items must be an array")
        return errors

    seen_ids: set[str] = set()
    for index, raw_item in enumerate(items):
        prefix = f"items[{index}]"
        if not isinstance(raw_item, dict):
            errors.append(f"{prefix} must be an object")
            continue
        item_id = jsonutil.get_str(raw_item, "id")
        if not item_id:
            errors.append(f"{prefix}.id must be a non-empty string")
        elif item_id in seen_ids:
            errors.append(f"{prefix}.id duplicates an earlier item: {item_id}")
        else:
            seen_ids.add(item_id)
        if raw_item.get("category") not in CATEGORIES:
            errors.append(f"{prefix}.category must be one of the known categories")
        if not isinstance(raw_item.get("command"), list):
            errors.append(f"{prefix}.command must be an array of argv tokens")
        status = raw_item.get("status")
        if status not in _ALLOWED_STATUSES:
            errors.append(f"{prefix}.status must be a known collection status")
        payload = raw_item.get("payload")
        digest = raw_item.get("digest")
        if status == CollectionStatus.COLLECTED.value:
            if not isinstance(payload, dict):
                errors.append(f"{prefix}.payload is required when status is collected")
            if not isinstance(digest, str) or not digest.startswith("sha256:"):
                errors.append(
                    f"{prefix}.digest must be a sha256: digest when collected"
                )
            if isinstance(payload, dict) and isinstance(digest, str):
                expected = sanitized_digest(payload)
                if expected != digest:
                    errors.append(f"{prefix}.digest does not match payload content")
        else:
            if payload is not None:
                errors.append(
                    f"{prefix}.payload must be null unless status is collected"
                )
    return errors
