"""Pure-Python local-binding client mirroring the MCP/daemon user path.

Models the capability-gated ``workspace.bind`` user path (bicameral-mcp#706)
plus the dashboard's fail-closed daemon-capability read (routes/dashboard.py
``queryDaemonBindingCapability``, bicameral-bot#747). Both live in other repos;
this client reproduces their runtime contract against the loopback
``fake_binding_daemon`` so the Cloud conformance suite can prove the split
authority model end to end without heavy cross-repo execution.

Fail-closed is the whole point:

- an unreachable, non-200, or malformed capability response is ``None``
  (unknown), never ``True``;
- the bind is attempted only when the daemon advertises the capability
  (``True``). Unknown or unsupported never binds — it fails closed with a
  typed reason.
"""

from __future__ import annotations

from dataclasses import dataclass

import httpx

from tests.conformance.fake_binding_daemon import (
    CAPABILITIES_PATH,
    WORKSPACE_BIND_PATH,
)


@dataclass(frozen=True)
class BindOutcome:
    """Normalized result of the capability-gated local bind user path.

    ``bound`` is the daemon-owned binding truth. ``readiness`` is the safe
    projection the daemon returns for a successful bind. ``fail_closed_reason``
    is set (and ``bound`` is ``False``) whenever the path refused to bind:
    ``capability_unavailable`` (unknown/unreachable) or ``binding_unsupported``
    (daemon reachable but capability ``False``). ``projection`` is the safe
    daemon response body, or ``None``; it never carries the caller's local path.
    """

    bound: bool
    readiness: str | None
    fail_closed_reason: str | None
    projection: dict[str, object] | None


def query_binding_capability(
    daemon_base_url: str, *, timeout: float = 2.0
) -> bool | None:
    """Read the daemon's truthful ``workspace_binding_available`` capability.

    Mirrors the dashboard JS fail-closed contract: any transport error,
    non-200, or a body without an explicit boolean yields ``None`` (unknown),
    which never promotes a Project to the bindable state.
    """

    url = daemon_base_url.rstrip("/") + CAPABILITIES_PATH
    try:
        response = httpx.get(url, timeout=timeout)
    except httpx.HTTPError:
        return None
    if not response.is_success:
        return None
    try:
        body = response.json()
    except ValueError:
        return None
    if not isinstance(body, dict):
        return None
    available = body.get("workspace_binding_available")
    if available is True:
        return True
    if available is False:
        return False
    return None


def attempt_workspace_bind(
    daemon_base_url: str,
    *,
    project_id: str,
    local_path: str,
    timeout: float = 2.0,
) -> BindOutcome:
    """Attempt the capability-gated local workspace bind (MCP user path).

    ``local_path`` is the user's local folder. It is sent only to the local
    daemon over loopback (the daemon is the sole binding authority, ADR-0005)
    and is never returned in ``projection``; the redaction invariant is asserted
    by the conformance suite.
    """

    capability = query_binding_capability(daemon_base_url, timeout=timeout)
    if capability is None:
        # Unknown/unreachable daemon: never bind.
        return BindOutcome(
            bound=False,
            readiness=None,
            fail_closed_reason="capability_unavailable",
            projection=None,
        )
    if capability is False:
        # Daemon reachable but does not advertise binding: fail closed.
        return BindOutcome(
            bound=False,
            readiness=None,
            fail_closed_reason="binding_unsupported",
            projection=None,
        )

    url = daemon_base_url.rstrip("/") + WORKSPACE_BIND_PATH
    try:
        response = httpx.post(
            url,
            json={"project_id": project_id, "local_path": local_path},
            timeout=timeout,
        )
    except httpx.HTTPError:
        return BindOutcome(
            bound=False,
            readiness=None,
            fail_closed_reason="capability_unavailable",
            projection=None,
        )

    if not response.is_success:
        payload = _safe_json(response)
        reason = payload.get("error_type") if isinstance(payload, dict) else None
        return BindOutcome(
            bound=False,
            readiness=None,
            fail_closed_reason=(
                reason if isinstance(reason, str) else "binding_unsupported"
            ),
            projection=None,
        )

    projection = _safe_json(response)
    if not isinstance(projection, dict) or projection.get("bound") is not True:
        return BindOutcome(
            bound=False,
            readiness=None,
            fail_closed_reason="binding_unsupported",
            projection=None,
        )

    readiness = projection.get("readiness")
    return BindOutcome(
        bound=True,
        readiness=readiness if isinstance(readiness, str) else None,
        fail_closed_reason=None,
        projection=projection,
    )


def _safe_json(response: httpx.Response) -> object:
    try:
        return response.json()
    except ValueError:
        return {}
