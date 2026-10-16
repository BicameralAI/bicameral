"""#771: retire bicameral.search and bicameral.review.candidates (no preflight alias)."""

from __future__ import annotations

import json

import pytest

import server
from tool_request import MCP_TOOL_COMMANDS
from tool_schemas import SUPPORTED_TOOLS

RETIRED = ("bicameral.search", "bicameral.review.candidates")


def test_retired_tools_absent_from_command_map_and_schemas():
    names = {tool.name for tool in SUPPORTED_TOOLS}
    for retired in RETIRED:
        assert retired not in MCP_TOOL_COMMANDS
        assert retired not in names


@pytest.mark.asyncio
async def test_list_tools_never_advertises_retired_names(monkeypatch):
    class _Daemon:
        async def capabilities(self):
            return {
                "toolrequest_protocol_version": "v2",
                "supported_commands": list(set(MCP_TOOL_COMMANDS.values()) | {"search.query"}),
                "deferred_commands": [],
                "workspace_binding_available": True,
            }

        async def send_tool_request(self, tool_request):
            raise AssertionError("list_tools must not dispatch ToolRequests")

    monkeypatch.setattr(server, "_client", lambda: _Daemon())
    tools = await server.list_tools()
    names = {tool.name for tool in tools}
    for retired in RETIRED:
        assert retired not in names
    assert "bicameral.preflight" in names


@pytest.mark.asyncio
@pytest.mark.parametrize("name", RETIRED)
async def test_direct_call_returns_unsupported_tool_without_dispatch(name, monkeypatch):
    class _Daemon:
        def __init__(self):
            self.requests = []

        async def capabilities(self):
            return {
                "toolrequest_protocol_version": "v2",
                "supported_commands": ["search.query", "preflight.run"],
                "deferred_commands": [],
                "workspace_binding_available": True,
            }

        async def send_tool_request(self, tool_request):
            self.requests.append(tool_request)
            return {"status": "ok", "request_id": tool_request["request_id"]}

    daemon = _Daemon()
    monkeypatch.setattr(server, "_client", lambda: daemon)
    monkeypatch.setattr(server, "_selected_product_id", "prd_test", raising=False)

    content = await server.call_tool(name, {"query": "x"})
    payload = json.loads(content[0].text)

    assert payload["status"] == "error"
    assert payload["error_code"] == "unsupported_tool"
    assert name in payload["message"]
    assert daemon.requests == []
    # Must not alias to preflight
    assert all(r["command"]["name"] != "preflight.run" for r in daemon.requests)
