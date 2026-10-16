"""Hosted dashboard workspace-binding UI-state derivation (issue #89).

The hosted dashboard renders a Project's *next action* for local workspace
binding. Cloud never owns binding — the local daemon is the sole binding
authority (ADR-0005) and ``workspace.bind`` is cloud-forbidden over the hosted
bridge (bicameral-bot#747). The browser therefore learns whether binding is
routable from the daemon's truthful ``workspace_binding_available`` capability
(``GET /v2/capabilities``, bicameral-bot#747) and routes the user to the local
MCP/daemon bind path (``bicameral.workspace.bind``, bicameral-mcp#706) instead
of attempting a bind itself.

This module is the single source of truth for that decision. It is pure, so it
is unit-testable, and the dashboard JS in :mod:`bicameral_cloud.routes.dashboard`
mirrors the same state names and next-action hints (the repo has no browser/JS
test substrate, so the Python mapping is the tested contract — mirroring the
existing pairing repair-hint pattern).

Mapping from the coarse Cloud ``BindingReadiness`` projection plus the daemon
capability signal to the rendered next-action state:

- ``ready``               -> the daemon reports the workspace bound/ready.
- ``repair_required``     -> daemon reachable but degraded or upgrade-required.
- ``binding_available``   -> unbound and the daemon advertises the bind path;
                             the Bind CTA is shown (routes to MCP/daemon).
- ``binding_unsupported`` -> unbound and the daemon does NOT advertise the bind
                             path; a clear fail-closed state (never a no-op).
- ``discoverable_unbound``-> unbound and the daemon capability is unknown (no
                             paired/reachable daemon yet); prompt to connect a
                             daemon. Never a silent no-op.

The Bind CTA is shown *only* in ``binding_available`` (acceptance: the CTA
appears only when the Project is unbound and the binding user path is
available).
"""

from __future__ import annotations

import enum

from bicameral_cloud.models.discovery import BindingReadiness

# Canonical MCP user-path tool for local workspace binding (bicameral-mcp#706).
# The browser cannot bind (cloud-forbidden); the CTA routes here.
WORKSPACE_BIND_COMMAND = "bicameral.workspace.bind"


class ProjectBindingUiState(enum.StrEnum):
    """Rendered next-action state for a Project's local workspace binding.

    UI/readiness projection states only (issue #89 state refinement): canonical
    binding truth stays in the daemon's local binding records, and coordination
    truth stays in Cloud tenancy records. These states never persist local
    binding, publish a workspace, or grant daemon authority.
    """

    DISCOVERABLE_UNBOUND = "discoverable_unbound"
    BINDING_AVAILABLE = "binding_available"
    BINDING_UNSUPPORTED = "binding_unsupported"
    READY = "ready"
    REPAIR_REQUIRED = "repair_required"


# Typed, safe next-action copy per state. Mirrored verbatim by the dashboard JS
# so the browser and the tested Python contract cannot drift. Contains no local
# paths, secrets, or daemon payloads.
BINDING_NEXT_ACTION_HINTS: dict[ProjectBindingUiState, str] = {
    ProjectBindingUiState.READY: (
        "This Project is bound to a local workspace and ready."
    ),
    ProjectBindingUiState.REPAIR_REQUIRED: (
        "The paired daemon needs attention before this Project can be used. "
        "Update or repair the local daemon, then retry."
    ),
    ProjectBindingUiState.BINDING_AVAILABLE: (
        "Bind this Project to a local folder from your machine using the "
        f"`{WORKSPACE_BIND_COMMAND}` tool via the local Bicameral daemon. "
        "Binding stays local; Cloud never receives the path."
    ),
    ProjectBindingUiState.BINDING_UNSUPPORTED: (
        "Local workspace binding is unavailable: the connected daemon does not "
        "advertise the binding capability. Update the local Bicameral daemon to "
        "a version that supports workspace binding, then reconnect."
    ),
    ProjectBindingUiState.DISCOVERABLE_UNBOUND: (
        "Connect your local Bicameral daemon to bind this Project to a local "
        "workspace. It stays discoverable and unbound until then."
    ),
}


def derive_binding_ui_state(
    binding_readiness: BindingReadiness,
    workspace_binding_available: bool | None,
) -> ProjectBindingUiState:
    """Derive the rendered binding next-action state (fail-closed).

    ``workspace_binding_available`` is the daemon's truthful capability signal
    (``True``/``False``), or ``None`` when it could not be determined (no
    reachable/paired daemon, or the capability query failed). An unknown
    capability never promotes to ``binding_available``: it falls back to
    ``discoverable_unbound`` so the UI always shows a clear next action.
    """

    if binding_readiness == BindingReadiness.READY:
        return ProjectBindingUiState.READY
    if binding_readiness in (
        BindingReadiness.DEGRADED,
        BindingReadiness.UPGRADE_REQUIRED,
    ):
        return ProjectBindingUiState.REPAIR_REQUIRED

    # Unbound-equivalent readiness (UNBOUND / UNPAIRED / UNKNOWN): the coarse
    # Cloud projection cannot bind on its own; the daemon capability decides
    # whether the local bind path is offered.
    if workspace_binding_available is True:
        return ProjectBindingUiState.BINDING_AVAILABLE
    if workspace_binding_available is False:
        return ProjectBindingUiState.BINDING_UNSUPPORTED
    return ProjectBindingUiState.DISCOVERABLE_UNBOUND


def bind_cta_available(state: ProjectBindingUiState) -> bool:
    """Return whether the Bind CTA should be shown for this state.

    Only ``binding_available`` shows the CTA: the Project is unbound and the
    daemon advertises a routable local bind path.
    """

    return state == ProjectBindingUiState.BINDING_AVAILABLE


def next_action_hint(state: ProjectBindingUiState) -> str:
    """Return the safe, typed next-action copy for a binding UI state."""

    return BINDING_NEXT_ACTION_HINTS[state]
