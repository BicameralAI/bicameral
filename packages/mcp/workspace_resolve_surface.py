"""Read-only MCP surface for daemon-owned workspace-to-Product routing.

The caller supplies a local host path only as a routing hint. The daemon owns
all path canonicalization, workspace-binding lookup, ambiguity detection, and
Product routing facts. This module never reads workspace-binding storage,
selects a Product, or mutates canonical state.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

WORKSPACE_RESOLVE_TOOL = "bicameral.workspace.resolve"
WORKSPACE_RESOLVE_COMMAND = "workspace.resolve"

_LOCAL_UNBOUND = "local_workspace_unbound"
_LOCAL_BOUND = "local_workspace_bound"
_LOCAL_REPAIR = "local_workspace_repair_required"


@dataclass(frozen=True)
class WorkspaceResolveSurfaceError(ValueError):
    """Typed local surface failure before daemon dispatch."""

    code: str
    detail: str

    def __str__(self) -> str:
        return f"{self.code}: {self.detail}"


def tool_definition() -> dict[str, Any]:
    """Return the provider-neutral MCP tool definition for workspace resolution."""
    return {
        "name": WORKSPACE_RESOLVE_TOOL,
        "description": (
            "Resolve a local host workspace/project path hint through the daemon's "
            "operator-owned LocalWorkspaceBinding registry. Returns at most one "
            "canonical product_id routing fact. This read does not select a Product, "
            "create a binding, or write canonical state."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "candidate_path": {
                    "type": "string",
                    "minLength": 1,
                    "description": (
                        "Local host workspace/project path routing hint. The path is "
                        "not Product identity and is never echoed in the response."
                    ),
                }
            },
            "required": ["candidate_path"],
            "additionalProperties": False,
        },
    }


def shape_params(arguments: Mapping[str, Any]) -> dict[str, Any]:
    """Allowlist the single daemon parameter without interpreting the path."""
    candidate_path = arguments.get("candidate_path")
    if not isinstance(candidate_path, str) or not candidate_path.strip():
        raise WorkspaceResolveSurfaceError(
            "candidate_path_invalid",
            "bicameral.workspace.resolve requires a non-empty candidate_path routing hint.",
        )
    return {"candidate_path": candidate_path}


def render_response(response: Mapping[str, Any]) -> dict[str, Any]:
    """Project daemon workspace.resolve into a minimal transport-safe result.

    The daemon contract already forbids echoing candidate_path. MCP narrows it
    further to the routing fact/state needed by host clients and intentionally
    omits daemon display/message/future fields so operator-local details cannot
    expand accidentally at this boundary.
    """
    request_id = response.get("request_id")
    status = str(response.get("status") or "").lower()
    result = response.get("result")
    result_obj = result if isinstance(result, Mapping) else None

    if status == "ok" and result_obj is not None:
        outcome = result_obj.get("outcome")
        outcome_obj = outcome if isinstance(outcome, Mapping) else result_obj
        product_id = outcome_obj.get("product_id")
        state = outcome_obj.get("state")
        if isinstance(product_id, str) and product_id.strip() and state == _LOCAL_BOUND:
            return {
                "status": "ok",
                "resolution": "resolved",
                "product_id": product_id,
                "state": state,
                "request_id": request_id,
                "canonical_write": "none",
                "product_selection_changed": False,
            }
        return _invalid_response(request_id)

    if status in {"rejected", "error"} and result_obj is not None:
        error = result_obj.get("error")
        state = result_obj.get("state")
        match_count = result_obj.get("match_count")
        if _valid_error_tuple(error, state, match_count):
            return {
                "status": "error",
                "error_code": error,
                "state": state,
                "match_count": match_count,
                "request_id": request_id,
                "canonical_write": "none",
                "product_selection_changed": False,
            }

    return _invalid_response(request_id)


def _valid_error_tuple(error: Any, state: Any, match_count: Any) -> bool:
    """Validate the correlated daemon resolve outcome, not fields independently."""
    if not isinstance(match_count, int) or isinstance(match_count, bool) or match_count < 0:
        return False
    if error == "unsafe_path":
        return state == _LOCAL_UNBOUND and match_count == 0
    if error == "repair_required":
        return state == _LOCAL_REPAIR and match_count == 1
    if error == "ambiguous":
        return state == _LOCAL_BOUND and match_count >= 2
    if error == "unbound":
        # The daemon returns count 0 for no record, or count 1 when one matching
        # record exists but is still in the durable unbound state.
        return state == _LOCAL_UNBOUND and match_count in {0, 1}
    return False


def _invalid_response(request_id: Any) -> dict[str, Any]:
    return {
        "status": "error",
        "error_code": "workspace_resolve_invalid_response",
        "request_id": request_id,
        "canonical_write": "none",
        "product_selection_changed": False,
    }
