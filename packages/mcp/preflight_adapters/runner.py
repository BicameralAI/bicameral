"""Host-hook runner: pre-work (SessionStart) and Planning-complete.

Responsibilities, in order:

1. Parse the host event into an allowlisted view (no transcript/secrets).
2. Dispatch SessionStart pre-work separately from Planning-complete. SessionStart
   never submits ``ManagedPlanningBoundaryV1``.
3. Deduplicate: pre-work once per session; Planning once per
   ``(host, session, turn, plan_digest)``.
4. Perform a daemon capability/protocol handshake before invoking.
5. On any failure, unsupported capability, daemon unavailability, or protocol
   mismatch, surface a visible message pointing at explicit/manual preflight and
   never claim preflight ran.

The runner never renders, chooses, promotes, or confirms candidates and never
writes canonical Decision state. That authority is the daemon's. Plan prose is
never sent as Product truth.

Product context for SessionStart is a daemon reverse-lookup of a unique
``LocalWorkspaceBinding`` for the host-reported workspace folder. The hook
never infers Product from cwd and never walks the operator-local binding
store. ``BICAMERAL_PRODUCT_ID`` is an optional override after that lookup.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from typing import Any, Protocol

from authority import build_authority_context
from daemon_client import (
    DaemonCapabilityError,
    DaemonClient,
    DaemonClientError,
    DaemonProtocolError,
)
from tool_request import build_tool_request
from version import TOOLREQUEST_PROTOCOL_VERSION

from .base import HostAdapter, HostSessionEvent
from .context import PreworkContext
from .planning import PlanningContext
from .registry import get_adapter
from .state import AdapterState

PREFLIGHT_COMMAND = "preflight.run"
RESOLVE_COMMAND = "workspace.resolve"

MANUAL_FALLBACK = (
    "Automatic pre-work preflight did not run. Run bicameral.preflight "
    "explicitly (or the 'preflight' MCP prompt) to get constraint/readiness "
    "context before you start."
)


class PreworkOutcome(StrEnum):
    INVOKED = "invoked"
    SKIPPED_NOT_PREWORK = "skipped_not_prework"
    SKIPPED_ALREADY_FIRED = "skipped_already_fired"
    SKIPPED_DISABLED = "skipped_disabled"
    SKIPPED_NO_PLAN_BYTES = "skipped_no_plan_bytes"
    FALLBACK_DAEMON_UNAVAILABLE = "fallback_daemon_unavailable"
    FALLBACK_PROTOCOL_MISMATCH = "fallback_protocol_mismatch"
    FALLBACK_CAPABILITY_UNSUPPORTED = "fallback_capability_unsupported"
    FALLBACK_PRODUCT_CONTEXT_REQUIRED = "fallback_product_context_required"
    FALLBACK_ERROR = "fallback_error"


_FALLBACK_OUTCOMES = frozenset(
    {
        PreworkOutcome.FALLBACK_DAEMON_UNAVAILABLE,
        PreworkOutcome.FALLBACK_PROTOCOL_MISMATCH,
        PreworkOutcome.FALLBACK_CAPABILITY_UNSUPPORTED,
        PreworkOutcome.FALLBACK_PRODUCT_CONTEXT_REQUIRED,
        PreworkOutcome.FALLBACK_ERROR,
    }
)


@dataclass(frozen=True)
class PreworkResult:
    outcome: PreworkOutcome
    host: str
    message: str
    correlation_id: str | None = None
    preflight_invoked: bool = False
    forwarded_context: str | None = None
    daemon_status: str | None = None
    details: dict[str, Any] = field(default_factory=dict)

    @property
    def is_fallback(self) -> bool:
        return self.outcome in _FALLBACK_OUTCOMES

    def to_dict(self) -> dict[str, Any]:
        return {
            "outcome": self.outcome.value,
            "host": self.host,
            "message": self.message,
            "correlation_id": self.correlation_id,
            "preflight_invoked": self.preflight_invoked,
            "forwarded_context": self.forwarded_context,
            "daemon_status": self.daemon_status,
            "details": self.details,
        }


class _ClientLike(Protocol):
    async def capabilities(self) -> dict[str, Any]: ...

    async def send_tool_request(self, tool_request: dict[str, Any]) -> dict[str, Any]: ...


def _default_client_factory() -> _ClientLike:
    return DaemonClient.from_env()


async def run_prework(
    host_id: str,
    payload: dict[str, Any],
    *,
    home: Path | None = None,
    client_factory: Any = None,
) -> PreworkResult:
    adapter = get_adapter(host_id, home=home)
    event = adapter.parse_event(payload)

    status = adapter.status()
    if status.state is not AdapterState.INSTALLED_ENABLED:
        return PreworkResult(
            outcome=PreworkOutcome.SKIPPED_DISABLED,
            host=host_id,
            message=(
                f"{adapter.display_name} pre-work adapter is not enabled; "
                "skipping automatic preflight."
            ),
        )

    if adapter.is_planning_boundary(event):
        return await _run_planning(
            adapter,
            event,
            payload,
            client_factory=client_factory,
        )

    if not adapter.is_prework_boundary(event):
        return PreworkResult(
            outcome=PreworkOutcome.SKIPPED_NOT_PREWORK,
            host=host_id,
            message=(
                f"Event source {event.source!r} is not a pre-work or "
                "Planning-complete boundary; no automatic preflight."
            ),
        )

    context = adapter.build_context(event)
    correlation_id = context.correlation_id
    store = adapter._store()
    metadata = store.load()
    provenance = status.package_provenance
    if metadata is None or provenance is None or status.contract_version is None:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_ERROR,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Adapter activation provenance is unavailable. {MANUAL_FALLBACK}",
        )
    managed_command = adapter._managed_command(metadata.runner_invocation)
    try:
        activation_receipt = store.record_activation(
            host=host_id,
            hook_event_name=event.hook_event_name,
            source=event.source,
            correlation_id=correlation_id,
            package_version=provenance.package_version,
            contract_version=status.contract_version,
            managed_command=managed_command,
        )
    except OSError:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_ERROR,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Durable host-activation evidence could not be recorded. {MANUAL_FALLBACK}",
        )
    factory = client_factory or _default_client_factory
    client = factory()

    handshake = await _handshake(client)
    if not isinstance(handshake, dict):
        return PreworkResult(
            outcome=handshake[0],
            host=host_id,
            correlation_id=correlation_id,
            message=f"{handshake[1]} {MANUAL_FALLBACK}",
            details=handshake[2],
        )

    try:
        store.confirm_activation_handshake(activation_receipt)
    except OSError:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_ERROR,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Host handshake evidence could not be recorded. {MANUAL_FALLBACK}",
        )
    if store.has_fired(correlation_id):
        return PreworkResult(
            outcome=PreworkOutcome.SKIPPED_ALREADY_FIRED,
            host=host_id,
            correlation_id=correlation_id,
            message=(
                "Pre-work preflight already ran for this task boundary "
                f"({correlation_id}); skipping to keep exactly-once semantics."
            ),
        )

    try:
        product_id, resolve_details = await _resolve_product_id(
            client, context, capabilities=handshake
        )
    except DaemonCapabilityError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_CAPABILITY_UNSUPPORTED,
            host=host_id,
            correlation_id=correlation_id,
            message=(f"Daemon does not support workspace.resolve ({exc}). {MANUAL_FALLBACK}"),
        )
    except DaemonProtocolError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_PROTOCOL_MISMATCH,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Daemon protocol mismatch ({exc}). {MANUAL_FALLBACK}",
        )
    except DaemonClientError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_DAEMON_UNAVAILABLE,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Daemon unavailable ({exc}). {MANUAL_FALLBACK}",
        )

    if not isinstance(product_id, str) or not product_id.strip():
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_PRODUCT_CONTEXT_REQUIRED,
            host=host_id,
            correlation_id=correlation_id,
            message=(
                "Automatic pre-work preflight has no unique daemon "
                f"LocalWorkspaceBinding for this folder. {MANUAL_FALLBACK}"
            ),
            details={"error_code": "product_context_required", **resolve_details},
        )

    try:
        response = await _invoke_preflight(client, context, product_id=product_id)
    except DaemonCapabilityError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_CAPABILITY_UNSUPPORTED,
            host=host_id,
            correlation_id=correlation_id,
            message=(f"Daemon does not support the preflight command ({exc}). {MANUAL_FALLBACK}"),
        )
    except DaemonProtocolError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_PROTOCOL_MISMATCH,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Daemon protocol mismatch ({exc}). {MANUAL_FALLBACK}",
        )
    except DaemonClientError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_DAEMON_UNAVAILABLE,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Daemon unavailable ({exc}). {MANUAL_FALLBACK}",
        )

    daemon_status = str(response.get("status")) if isinstance(response, dict) else None
    store.mark_fired(
        correlation_id,
        {
            "task_boundary": context.task_boundary,
            "daemon_status": daemon_status,
            "forwarded_context": context.describe(),
        },
    )
    return PreworkResult(
        outcome=PreworkOutcome.INVOKED,
        host=host_id,
        correlation_id=correlation_id,
        preflight_invoked=True,
        forwarded_context=context.describe(),
        daemon_status=daemon_status,
        message=(
            f"Invoked bicameral.preflight once for {adapter.display_name} "
            f"pre-work boundary with bounded context ({context.describe()})."
        ),
    )


async def _handshake(
    client: _ClientLike,
) -> dict[str, Any] | tuple[PreworkOutcome, str, dict[str, Any]]:
    """Return capabilities when compatible, else a fallback ``(outcome, msg, details)``."""
    try:
        capabilities = await client.capabilities()
    except DaemonClientError as exc:
        return (
            PreworkOutcome.FALLBACK_DAEMON_UNAVAILABLE,
            f"Daemon unavailable during capability handshake ({exc}).",
            {},
        )
    protocol_version = capabilities.get("toolrequest_protocol_version") or capabilities.get(
        "protocol_version"
    )
    if protocol_version != TOOLREQUEST_PROTOCOL_VERSION:
        return (
            PreworkOutcome.FALLBACK_PROTOCOL_MISMATCH,
            (
                "Daemon ToolRequest protocol "
                f"{protocol_version!r} is incompatible with MCP "
                f"{TOOLREQUEST_PROTOCOL_VERSION!r}."
            ),
            {
                "daemon_protocol_version": protocol_version,
                "mcp_protocol_version": TOOLREQUEST_PROTOCOL_VERSION,
            },
        )
    supported = tuple(capabilities.get("supported_commands", []))
    deferred = tuple(capabilities.get("deferred_commands", []))
    if PREFLIGHT_COMMAND in deferred or PREFLIGHT_COMMAND not in supported:
        return (
            PreworkOutcome.FALLBACK_CAPABILITY_UNSUPPORTED,
            f"Daemon does not advertise the {PREFLIGHT_COMMAND} command.",
            {"supported_commands": list(supported), "deferred_commands": list(deferred)},
        )
    return capabilities


def _env_product_override() -> str | None:
    value = os.environ.get("BICAMERAL_PRODUCT_ID")
    if isinstance(value, str) and value.strip():
        return value.strip()
    return None


def _unique_resolved_product_id(response: dict[str, Any]) -> str | None:
    """Return the daemon's unique bound product_id, or None on any miss."""
    if str(response.get("status") or "").lower() != "ok":
        return None
    result = response.get("result")
    if not isinstance(result, dict):
        return None
    outcome = result.get("outcome")
    if not isinstance(outcome, dict):
        outcome = result
    product_id = outcome.get("product_id")
    state = outcome.get("state")
    if isinstance(product_id, str) and product_id.strip() and state == "local_workspace_bound":
        return product_id.strip()
    return None


def _resolve_error_kind(response: dict[str, Any]) -> str:
    result = response.get("result")
    if isinstance(result, dict):
        error = result.get("error")
        if isinstance(error, str) and error.strip():
            return error.strip()
    message = response.get("message")
    if isinstance(message, str) and message:
        kind = message.split(":", 1)[0].strip()
        if kind:
            return kind
    return "unbound"


async def _resolve_product_id(
    client: _ClientLike,
    context: PreworkContext,
    *,
    capabilities: dict[str, Any],
) -> tuple[str | None, dict[str, Any]]:
    """Daemon reverse-lookup first; env override only after a typed miss.

    The hook never walks operator-local binding records on disk. Host payload
    ``product_id`` is ignored: SessionStart is a different process from MCP
    ``product.select`` session memory.
    """
    env_override = _env_product_override()
    workspace = context.workspace
    if not isinstance(workspace, str) or not workspace.strip():
        if env_override:
            return env_override, {
                "product_id_source": "env_override",
                "resolve_error": "missing_workspace",
            }
        return None, {"resolve_error": "missing_workspace"}

    supported = tuple(capabilities.get("supported_commands", []))
    if RESOLVE_COMMAND not in supported:
        if env_override:
            return env_override, {
                "product_id_source": "env_override",
                "resolve_error": "unsupported_command",
            }
        return None, {"resolve_error": "unsupported_command"}

    authority = build_authority_context(
        "bicameral.preflight",
        {"workspace": workspace},
    )
    tool_request = build_tool_request(
        command_name=RESOLVE_COMMAND,
        params={"candidate_path": workspace},
        authority=authority,
    )
    response = await client.send_tool_request(tool_request)
    product_id = _unique_resolved_product_id(response)
    if product_id:
        return product_id, {"product_id_source": "workspace_resolve"}

    resolve_error = _resolve_error_kind(response)
    if env_override:
        return env_override, {
            "product_id_source": "env_override",
            "resolve_error": resolve_error,
        }
    return None, {"resolve_error": resolve_error}


async def _run_planning(
    adapter: HostAdapter,
    event: HostSessionEvent,
    payload: dict[str, Any],
    *,
    client_factory: Any,
) -> PreworkResult:
    """Submit one ManagedPlanningBoundaryV1 at a Planning-complete event."""
    host_id = adapter.host_id
    plan_bytes = adapter.extract_plan_bytes(payload)
    if not plan_bytes:
        return PreworkResult(
            outcome=PreworkOutcome.SKIPPED_NO_PLAN_BYTES,
            host=host_id,
            message=(
                "Planning-complete event has no allowlisted Plan bytes; "
                "refusing transcript scrape or Plan-as-spec fallback."
            ),
        )

    context = adapter.build_planning_context(event, payload, plan_bytes)
    correlation_id = context.correlation_id
    store = adapter._store()
    if store.has_fired(correlation_id):
        return PreworkResult(
            outcome=PreworkOutcome.SKIPPED_ALREADY_FIRED,
            host=host_id,
            correlation_id=correlation_id,
            message=(
                "Planning-complete preflight already ran for this "
                f"(host, session, turn, plan_digest) ({correlation_id}); "
                "skipping to keep exactly-once semantics."
            ),
        )

    factory = client_factory or _default_client_factory
    client = factory()

    handshake = await _handshake(client)
    if not isinstance(handshake, dict):
        return PreworkResult(
            outcome=handshake[0],
            host=host_id,
            correlation_id=correlation_id,
            message=f"{handshake[1]} {MANUAL_FALLBACK}",
            details=handshake[2],
        )

    try:
        response = await _invoke_preflight(
            client,
            context,
            product_id=context.product_id,
        )
    except DaemonCapabilityError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_CAPABILITY_UNSUPPORTED,
            host=host_id,
            correlation_id=correlation_id,
            message=(f"Daemon does not support the preflight command ({exc}). {MANUAL_FALLBACK}"),
        )
    except DaemonProtocolError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_PROTOCOL_MISMATCH,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Daemon protocol mismatch ({exc}). {MANUAL_FALLBACK}",
        )
    except DaemonClientError as exc:
        return PreworkResult(
            outcome=PreworkOutcome.FALLBACK_DAEMON_UNAVAILABLE,
            host=host_id,
            correlation_id=correlation_id,
            message=f"Daemon unavailable ({exc}). {MANUAL_FALLBACK}",
        )

    daemon_status = str(response.get("status")) if isinstance(response, dict) else None
    store.mark_fired(
        correlation_id,
        {
            "task_boundary": "planning_complete",
            "daemon_status": daemon_status,
            "forwarded_context": context.describe(),
            "plan_digest": context.plan_digest,
        },
    )
    return PreworkResult(
        outcome=PreworkOutcome.INVOKED,
        host=host_id,
        correlation_id=correlation_id,
        preflight_invoked=True,
        forwarded_context=context.describe(),
        daemon_status=daemon_status,
        message=(
            f"Invoked bicameral.preflight once for {adapter.display_name} "
            f"Planning-complete boundary with bounded context ({context.describe()})."
        ),
    )


async def _invoke_preflight(
    client: _ClientLike,
    context: PreworkContext | PlanningContext,
    *,
    product_id: str | None,
) -> dict[str, Any]:
    params = context.to_preflight_arguments()
    authority = build_authority_context(
        "bicameral.preflight",
        {"workspace": context.workspace} if context.workspace else {},
    )
    # Correlation/idempotency metadata rides in the audit metadata so the daemon
    # can join the operational witness without changing the command contract.
    audit_metadata = authority.get("audit_metadata")
    if isinstance(audit_metadata, dict):
        audit_metadata.update(context.audit_metadata())
    tool_request = build_tool_request(
        command_name=PREFLIGHT_COMMAND,
        params=params,
        authority=authority,
        product_id=product_id,
    )
    return await client.send_tool_request(tool_request)
