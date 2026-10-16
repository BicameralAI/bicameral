"""MCP #871 read-only workspace.resolve Product-routing seam."""

from __future__ import annotations

import json
from typing import Any

import pytest

import server
from daemon_client import DaemonConnectionError
from workspace_resolve_surface import render_response, shape_params


class _Daemon:
    def __init__(
        self,
        *,
        protocol_version: str | None = "v2",
        supported: tuple[str, ...] = ("workspace.resolve", "search.query", "preflight.run"),
        resolve_result: dict[str, Any] | None = None,
        capabilities_error: Exception | None = None,
    ) -> None:
        self.protocol_version = protocol_version
        self.supported = supported
        self.resolve_result = resolve_result or {
            "status": "ok",
            "result": {
                "status": "resolved",
                "outcome": {
                    "product_id": "prd_alpha",
                    "state": "local_workspace_bound",
                    "display": {"display_name": "Alpha", "future_local_detail": "must-not-leak"},
                },
            },
        }
        self.capabilities_error = capabilities_error
        self.requests: list[dict[str, Any]] = []

    async def capabilities(self) -> dict[str, Any]:
        if self.capabilities_error is not None:
            raise self.capabilities_error
        return {
            "toolrequest_protocol_version": self.protocol_version,
            "supported_commands": list(self.supported),
            "deferred_commands": [],
            "workspace_binding_available": True,
        }

    async def send_tool_request(self, tool_request: dict[str, Any]) -> dict[str, Any]:
        self.requests.append(tool_request)
        command = tool_request["command"]["name"]
        if command == "workspace.resolve":
            return {"request_id": tool_request["request_id"], **self.resolve_result}
        return {
            "request_id": tool_request["request_id"],
            "status": "ok",
            "result": {"matches": []},
        }


@pytest.fixture(autouse=True)
def _reset_product_selection(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(server, "_selected_product_id", None, raising=False)


def _payload(contents: list[Any]) -> dict[str, Any]:
    return json.loads(contents[0].text)


async def _resolve(candidate_path: str) -> dict[str, Any]:
    contents = await server.call_tool(
        "bicameral.workspace.resolve",
        {"candidate_path": candidate_path},
    )
    return _payload(contents)


def test_surface_requires_nonempty_path_but_does_not_interpret_it() -> None:
    with pytest.raises(ValueError):
        shape_params({"candidate_path": "   "})

    path = "./relative-is-daemon-policy"
    assert shape_params({"candidate_path": path, "secret": "discard"}) == {"candidate_path": path}


def test_renderer_never_echoes_path_or_display_metadata() -> None:
    response = {
        "request_id": "r-1",
        "status": "ok",
        "result": {
            "outcome": {
                "product_id": "prd_alpha",
                "state": "local_workspace_bound",
                "display": {
                    "display_name": "Alpha",
                    "candidate_path": "/secret/workspace",
                },
            }
        },
    }
    payload = render_response(response)
    text = json.dumps(payload)
    assert payload["product_id"] == "prd_alpha"
    assert payload["canonical_write"] == "none"
    assert payload["product_selection_changed"] is False
    assert "candidate_path" not in text
    assert "/secret/workspace" not in text
    assert "display" not in payload


@pytest.mark.parametrize(
    ("error_code", "state", "match_count"),
    [
        ("unbound", "local_workspace_unbound", 0),
        ("unbound", "local_workspace_unbound", 1),
        ("repair_required", "local_workspace_repair_required", 1),
        ("ambiguous", "local_workspace_bound", 2),
        ("unsafe_path", "local_workspace_unbound", 0),
    ],
)
def test_renderer_preserves_coherent_typed_fail_closed_outcomes(
    error_code: str,
    state: str,
    match_count: int,
) -> None:
    response = {
        "request_id": "r-2",
        "status": "rejected",
        "result": {
            "error": error_code,
            "message": "daemon detail intentionally omitted by MCP",
            "state": state,
            "match_count": match_count,
        },
    }
    payload = render_response(response)
    assert payload == {
        "status": "error",
        "error_code": error_code,
        "state": state,
        "match_count": match_count,
        "request_id": "r-2",
        "canonical_write": "none",
        "product_selection_changed": False,
    }


@pytest.mark.parametrize(
    ("error_code", "state", "match_count"),
    [
        ("repair_required", "local_workspace_bound", 1),
        ("ambiguous", "local_workspace_unbound", 2),
        ("unsafe_path", "local_workspace_unbound", 1),
        ("unbound", "local_workspace_bound", 0),
        ("ambiguous", "local_workspace_bound", 1),
    ],
)
def test_renderer_rejects_crossed_or_impossible_error_tuples(
    error_code: str,
    state: str,
    match_count: int,
) -> None:
    response = {
        "request_id": "r-crossed",
        "status": "rejected",
        "result": {
            "error": error_code,
            "state": state,
            "match_count": match_count,
        },
    }
    payload = render_response(response)
    assert payload["status"] == "error"
    assert payload["error_code"] == "workspace_resolve_invalid_response"
    assert "product_id" not in payload


def test_renderer_rejects_malformed_daemon_success() -> None:
    wrong_state = {
        "request_id": "r-wrong-state",
        "status": "ok",
        "result": {
            "outcome": {
                "product_id": "prd_alpha",
                "state": "local_workspace_unbound",
            }
        },
    }
    assert render_response(wrong_state)["error_code"] == "workspace_resolve_invalid_response"

    empty_product = {
        "request_id": "r-empty-product",
        "status": "ok",
        "result": {
            "outcome": {
                "product_id": " ",
                "state": "local_workspace_bound",
            }
        },
    }
    assert render_response(empty_product)["error_code"] == "workspace_resolve_invalid_response"


@pytest.mark.asyncio
async def test_tool_advertised_only_when_daemon_advertises_resolve(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    supported = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: supported)
    names = {tool.name for tool in await server.list_tools()}
    assert "bicameral.workspace.resolve" in names

    unsupported = _Daemon(supported=("search.query", "preflight.run"))
    monkeypatch.setattr(server, "_client", lambda: unsupported)
    names = {tool.name for tool in await server.list_tools()}
    assert "bicameral.workspace.resolve" not in names


@pytest.mark.asyncio
async def test_direct_resolve_call_preserves_capability_and_protocol_failures(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    unsupported = _Daemon(supported=("search.query", "preflight.run"))
    monkeypatch.setattr(server, "_client", lambda: unsupported)
    payload = await _resolve("/workspace/alpha")
    assert payload["error_code"] == "daemon_capability_error"
    assert unsupported.requests == []

    mismatched = _Daemon(protocol_version="v999")
    monkeypatch.setattr(server, "_client", lambda: mismatched)
    payload = await _resolve("/workspace/alpha")
    assert payload["error_code"] == "daemon_protocol_mismatch"
    assert mismatched.requests == []


@pytest.mark.asyncio
async def test_direct_resolve_call_preserves_daemon_unavailable(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    unavailable = _Daemon(capabilities_error=DaemonConnectionError("daemon unavailable for test"))
    monkeypatch.setattr(server, "_client", lambda: unavailable)

    payload = await _resolve("/workspace/alpha")

    assert payload["error_code"] == "daemon_unavailable"
    assert unavailable.requests == []
    assert "/workspace/alpha" not in json.dumps(payload)


@pytest.mark.asyncio
async def test_resolve_forwards_only_candidate_path_and_does_not_select(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    contents = await server.call_tool(
        "bicameral.workspace.resolve",
        {
            "candidate_path": "/workspace/alpha",
            "actor_id": "must-not-be-command-param",
            "product_id": "must-not-authorize",
            "unexpected": "drop-me",
        },
    )
    payload = _payload(contents)

    assert payload["status"] == "ok"
    assert payload["product_id"] == "prd_alpha"
    assert server._selected_product_id is None
    assert len(daemon.requests) == 1
    request = daemon.requests[0]
    assert "product_id" not in request
    assert request["command"] == {
        "name": "workspace.resolve",
        "params": {"candidate_path": "/workspace/alpha"},
    }
    assert "/workspace/alpha" not in json.dumps(payload)


@pytest.mark.asyncio
async def test_resolve_alone_never_authorizes_product_scoped_tools(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    resolved = await _resolve("/workspace/alpha")
    assert resolved["product_id"] == "prd_alpha"
    assert server._selected_product_id is None

    blocked = _payload(await server.call_tool("bicameral.preflight", {}))
    assert blocked["error_code"] == "product_context_required"
    assert len(daemon.requests) == 1


@pytest.mark.asyncio
async def test_resolve_then_explicit_select_then_preflight(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    resolved = await _resolve("/workspace/alpha")
    selected = _payload(
        await server.call_tool(
            "bicameral.product.select",
            {"product_id": resolved["product_id"]},
        )
    )
    await server.call_tool("bicameral.preflight", {})

    assert selected["selected_product_id"] == "prd_alpha"
    assert server._selected_product_id == "prd_alpha"
    assert [request["command"]["name"] for request in daemon.requests] == [
        "workspace.resolve",
        "search.query",
        "preflight.run",
    ]
    assert "product_id" not in daemon.requests[0]
    assert daemon.requests[1]["product_id"] == "prd_alpha"
    assert daemon.requests[2]["product_id"] == "prd_alpha"


@pytest.mark.asyncio
async def test_resolve_does_not_replace_an_existing_selection_on_success_or_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)
    monkeypatch.setattr(server, "_selected_product_id", "prd_existing")

    payload = await _resolve("/workspace/alpha")
    assert payload["product_id"] == "prd_alpha"
    assert server._selected_product_id == "prd_existing"

    daemon.resolve_result = {
        "status": "rejected",
        "result": {
            "error": "unbound",
            "state": "local_workspace_unbound",
            "match_count": 0,
        },
    }
    payload = await _resolve("/workspace/missing")
    assert payload["error_code"] == "unbound"
    assert server._selected_product_id == "prd_existing"


@pytest.mark.asyncio
async def test_resolve_rejects_empty_path_before_daemon_access(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    payload = await _resolve("")

    assert payload["status"] == "error"
    assert payload["error_code"] == "candidate_path_invalid"
    assert daemon.requests == []
