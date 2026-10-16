"""Product-selection contract for the operator-scoped daemon."""

from __future__ import annotations

import json
from typing import cast

import pytest

import server
from coverage_guard import check_coverage
from daemon_client import DaemonClient
from tool_request import build_tool_request


class _Daemon:
    def __init__(self) -> None:
        self.requests: list[dict] = []

    async def capabilities(self) -> dict:
        return {
            "toolrequest_protocol_version": "v2",
            "supported_commands": ["search.query", "preflight.run"],
            "deferred_commands": [],
            "workspace_binding_available": True,
        }

    async def send_tool_request(self, tool_request: dict) -> dict:
        self.requests.append(tool_request)
        return {
            "request_id": tool_request["request_id"],
            "status": "ok",
            "result": {"matches": []},
            "responded_at": "2026-07-28T00:00:00Z",
        }


@pytest.fixture(autouse=True)
def _reset_product_selection(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(server, "_selected_product_id", None, raising=False)


def test_product_id_is_a_top_level_toolrequest_field() -> None:
    request = build_tool_request(
        command_name="search.query",
        params={"query": "routing marker", "product_id": "must-not-be-a-param"},
        authority={"actor_id": "operator"},
        product_id="prd_alpha",
    )

    assert request["product_id"] == "prd_alpha"
    assert "product_id" not in request["command"]["params"]


@pytest.mark.asyncio
async def test_product_select_is_advertised_as_local_control_tool(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    names = {tool.name for tool in await server.list_tools()}

    assert "bicameral.product.select" in names


@pytest.mark.asyncio
async def test_product_select_is_daemon_validated_before_it_is_retained(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    content = await server.call_tool("bicameral.product.select", {"product_id": "prd_alpha"})
    payload = json.loads(content[0].text)

    assert payload["status"] == "ok"
    assert payload["selected_product_id"] == "prd_alpha"
    assert server._selected_product_id == "prd_alpha"
    assert len(daemon.requests) == 1
    assert daemon.requests[0]["product_id"] == "prd_alpha"
    assert daemon.requests[0]["command"] == {
        "name": "search.query",
        "params": {
            "query": "__bicameral_product_route_validation__",
            "scope": "all",
            "limit": 1,
        },
    }


@pytest.mark.asyncio
async def test_product_scoped_tool_fails_closed_without_a_selection(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    content = await server.call_tool("bicameral.preflight", {"files": ["src/lib.rs"]})
    payload = json.loads(content[0].text)

    assert payload["status"] == "error"
    assert payload["error_code"] == "product_context_required"
    assert daemon.requests == []


@pytest.mark.asyncio
async def test_selected_product_is_injected_into_later_toolrequests(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)

    await server.call_tool("bicameral.product.select", {"product_id": "prd_alpha"})
    await server.call_tool("bicameral.preflight", {"files": ["src/lib.rs"]})

    assert len(daemon.requests) == 2
    routed_request = daemon.requests[1]
    assert routed_request["product_id"] == "prd_alpha"
    assert "product_id" not in routed_request["command"]["params"]


@pytest.mark.asyncio
async def test_preflight_coverage_probe_uses_the_selected_product() -> None:
    daemon = _Daemon()

    await check_coverage(
        client=cast(DaemonClient, daemon),
        files=["src/lib.rs"],
        supported_commands=("lookup.query",),
        product_id="prd_alpha",
    )

    assert daemon.requests[0]["product_id"] == "prd_alpha"


@pytest.mark.asyncio
async def test_failed_reselection_clears_the_previous_product(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)
    await server.call_tool("bicameral.product.select", {"product_id": "prd_alpha"})

    async def reject(tool_request: dict) -> dict:
        daemon.requests.append(tool_request)
        return {
            "request_id": tool_request["request_id"],
            "status": "rejected",
            "message": "product_not_registered",
            "responded_at": "2026-07-28T00:00:00Z",
        }

    monkeypatch.setattr(daemon, "send_tool_request", reject)
    content = await server.call_tool("bicameral.product.select", {"product_id": "prd_missing"})

    assert json.loads(content[0].text)["status"] == "rejected"
    assert server._selected_product_id is None
