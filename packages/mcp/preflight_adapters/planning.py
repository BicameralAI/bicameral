"""Planning-complete boundary for CONF-MCP-PLAN-HOOK-01.

Planning completion is a *second* host hook, distinct from SessionStart
activation. This module hashes exact Plan bytes and never treats Plan prose,
cwd, branch, mtime, or transcript scrape as Product truth.

``ManagedPlanningBoundaryV1`` identity fields are assembled here and passed
through ``preflight.run``; accepted SMR/hypergraph resolution remains the
daemon's responsibility (bot#1205).
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import Any

#: Allowlisted keys that may carry completed Plan bytes. Nothing else —
#: including transcript, messages, or concatenated assistant chatter — is a
#: recovery path for a missing plan.
CLAUDE_PLAN_FIELD = "tool_response.plan"
CODEX_PLAN_FIELDS: tuple[str, ...] = (
    "last_assistant_message",
    "last-assistant-message",
)

HOST_KIND = {
    "claude": "claude_code",
    "codex": "codex",
}


def compute_plan_digest(plan_bytes: bytes) -> str:
    """Return ``sha256:<hex>`` of the exact completed Plan bytes."""
    return "sha256:" + hashlib.sha256(plan_bytes).hexdigest()


def extract_claude_plan_bytes(payload: dict[str, Any]) -> bytes | None:
    """Read exact ``tool_response.plan`` only. Never scrape the transcript."""
    tool_response = payload.get("tool_response")
    if not isinstance(tool_response, dict):
        return None
    return _as_plan_bytes(tool_response.get("plan"))


def extract_codex_plan_bytes(payload: dict[str, Any]) -> bytes | None:
    """Read exact last-assistant-message only. Never scrape the transcript."""
    for key in CODEX_PLAN_FIELDS:
        plan = _as_plan_bytes(payload.get(key))
        if plan is not None:
            return plan
    return None


def _as_plan_bytes(value: Any) -> bytes | None:
    if isinstance(value, str) and value:
        return value.encode("utf-8")
    if isinstance(value, bytes) and value:
        return value
    return None


def planning_correlation_id(*, host: str, session_id: str, turn_id: str, plan_digest: str) -> str:
    """Exactly-once key for ``(host, session, turn, plan_digest)``."""
    return f"{host}:{session_id}:{turn_id}:{plan_digest}"


@dataclass(frozen=True)
class PlanningContext:
    """Bounded Planning-complete context. Does not carry Plan prose."""

    host: str
    correlation_id: str
    host_session_id: str
    host_turn_id: str
    plan_digest: str
    product_id: str | None = None
    workspace: str | None = None

    def to_preflight_arguments(self) -> dict[str, Any]:
        """Build ``preflight.run`` params. No SessionStart ``checkpoint_hint``."""
        managed: dict[str, Any] = {
            "host_kind": HOST_KIND.get(self.host, self.host),
            "host_session_id": self.host_session_id,
            "host_turn_id": self.host_turn_id,
            "plan_digest": self.plan_digest,
        }
        if self.product_id:
            managed["product_id"] = self.product_id
        return {"managed_planning": managed}

    def audit_metadata(self) -> dict[str, Any]:
        return {
            "correlation_id": self.correlation_id,
            "idempotency_key": self.correlation_id,
            "checkpoint": "planning_complete",
            "automation": "mcp_host_planning_adapter",
        }

    def describe(self) -> str:
        parts = [
            "task_boundary=planning_complete",
            f"plan_digest={self.plan_digest}",
        ]
        if self.host_turn_id:
            parts.append(f"host_turn_id={self.host_turn_id}")
        return ", ".join(parts)
