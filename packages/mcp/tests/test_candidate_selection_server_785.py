"""Runtime integration coverage for the Managed MCP candidate-selection surface."""

from __future__ import annotations

import json
from collections.abc import Iterator
from typing import Any

import pytest
from mcp import types

import server
from candidate_selection_surface import (
    COMMIT_TOOL,
    END_TOOL,
    HEARTBEAT_TOOL,
    PREPARE_TOOL,
)
from daemon_client import CapabilityReport
from version import TOOLREQUEST_PROTOCOL_VERSION


@pytest.fixture(autouse=True)
def _reset_selected_product() -> Iterator[None]:
    server._selected_product_id = None
    yield
    server._selected_product_id = None


def _report(
    *,
    supported: tuple[str, ...],
    deferred: tuple[str, ...] = (),
) -> CapabilityReport:
    return CapabilityReport(
        daemon_protocol_version=TOOLREQUEST_PROTOCOL_VERSION,
        mcp_protocol_version=TOOLREQUEST_PROTOCOL_VERSION,
        supported_commands=supported,
        deferred_commands=deferred,
        daemon_endpoint="http://127.0.0.1:7777",
        workspace_binding_available=False,
    )


def _candidate_set() -> dict[str, Any]:
    return {
        "contract_version": 1,
        "candidate_set_id": "00000000-0000-0000-0000-000000000101",
        "candidate_set_generation": 3,
        "candidate_set_digest": "sha256:" + "a" * 64,
        "binding": {
            "product_id": "prod-1",
            "host_session_id": "host-1",
        },
        "session_lease_id": "lease-1",
        "lease_expires_at": "2026-08-03T17:00:00Z",
        "candidates": [
            {
                "candidate_id": "00000000-0000-0000-0000-000000000201",
                "candidate_generation": 1,
                "proposed_decision": "Use bounded retries.",
                "source_spans": [
                    {
                        "artifact_path": "docs/spec.md",
                        "start_byte": 10,
                        "end_byte": 30,
                        "future_span_proof": {"opaque": True},
                    }
                ],
                "contradiction_refs": [],
                "dependency_candidate_ids": [],
                "claim_trace_digest": "sha256:" + "b" * 64,
                "proposal_digest": "sha256:" + "c" * 64,
                "disposition": "pending",
                "future_candidate_proof": {"opaque": True},
            }
        ],
        "coverage_limitations": [],
        "future_set_proof": {"opaque": True},
    }


def test_candidate_tools_are_exposed_only_when_daemon_advertises_them() -> None:
    tools = server._filter_tools_by_capability(
        _report(
            supported=(
                "candidate_selection.prepare",
                "candidate_selection.commit",
                "candidate_session.heartbeat",
                "candidate_session.end",
            ),
            deferred=("candidate_session.end",),
        )
    )
    names = {tool.name for tool in tools}

    assert PREPARE_TOOL in names
    assert COMMIT_TOOL in names
    assert HEARTBEAT_TOOL in names
    assert END_TOOL not in names


class _FakeClient:
    def __init__(self, *, commands: tuple[str, ...], response: dict[str, Any]) -> None:
        self.commands = commands
        self.response = response
        self.requests: list[dict[str, Any]] = []
        self.capability_calls = 0

    async def capabilities(self) -> dict[str, Any]:
        self.capability_calls += 1
        return {
            "toolrequest_protocol_version": TOOLREQUEST_PROTOCOL_VERSION,
            "supported_commands": list(self.commands),
            "deferred_commands": [],
        }

    async def send_tool_request(self, request: dict[str, Any]) -> dict[str, Any]:
        self.requests.append(request)
        return self.response


@pytest.mark.asyncio
async def test_prepare_dispatch_preserves_exact_actor_and_stable_ids(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    response = {
        "status": "ok",
        "result": {
            "candidate_selection_preview": {
                "contract_version": 1,
                "selection_operation_id": "00000000-0000-0000-0000-000000000301",
                "selection_digest": "sha256:" + "d" * 64,
                "display_digest": "sha256:" + "e" * 64,
                "candidate_set_id": "00000000-0000-0000-0000-000000000101",
                "candidate_set_generation": 3,
                "candidate_set_digest": "sha256:" + "a" * 64,
                "product_id": "prod-1",
                "host_session_id": "host-1",
                "actor_id": "owner-1",
                "governing_spec_binding_digest": "sha256:" + "f" * 64,
                "accepted_model_base_position": 7,
                "accepted_model_base_digest": "sha256:" + "1" * 64,
                "policy_revision": "policy-7",
                "promote_effects": [],
                "reject_effects": [],
                "pending_candidate_count": 1,
                "resulting_signoff": "proposed",
                "confirmation_challenge": {
                    "challenge_id": "00000000-0000-0000-0000-000000000401",
                    "restart_epoch": "00000000-0000-0000-0000-000000000402",
                    "secret": "opaque-single-use-token",
                    "expires_at": "2026-08-03T17:00:00Z",
                },
            }
        },
    }
    client = _FakeClient(commands=("candidate_selection.prepare",), response=response)
    monkeypatch.setattr(server, "_client", lambda: client)
    server._selected_product_id = "prod-1"

    contents = await server.call_tool(
        PREPARE_TOOL,
        {
            "contract_version": 1,
            "selection_operation_id": "00000000-0000-0000-0000-000000000301",
            "candidate_set_id": "00000000-0000-0000-0000-000000000101",
            "candidate_set_generation": 3,
            "candidate_set_digest": "sha256:" + "a" * 64,
            "product_id": "prod-1",
            "host_session_id": "host-1",
            "session_lease_id": "lease-1",
            "promote_candidate_ids": ["00000000-0000-0000-0000-000000000201"],
            "reject_candidate_ids": [],
            "actor_id": "owner-1",
        },
    )

    assert len(client.requests) == 1
    request = client.requests[0]
    assert request["command"]["name"] == "candidate_selection.prepare"
    assert request["command"]["params"]["actor_id"] == "owner-1"
    assert request["command"]["params"]["promote_candidate_ids"] == [
        "00000000-0000-0000-0000-000000000201"
    ]
    assert request["product_id"] == "prod-1"

    rendered = json.loads(contents[0].text)
    assert rendered["status"] == "confirmation_required"
    assert rendered["human_action"]["agent_may_confirm"] is False
    assert rendered["canonical_write"] == "none"


@pytest.mark.asyncio
async def test_positional_selection_refuses_before_daemon_access(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def _unexpected_client() -> Any:
        raise AssertionError("local positional-authority refusal must happen before daemon access")

    monkeypatch.setattr(server, "_client", _unexpected_client)
    server._selected_product_id = "prod-1"

    contents = await server.call_tool(
        PREPARE_TOOL,
        {
            "candidate_set_id": "00000000-0000-0000-0000-000000000101",
            "candidate_set_generation": 3,
            "candidate_set_digest": "sha256:" + "a" * 64,
            "product_id": "prod-1",
            "host_session_id": "host-1",
            "session_lease_id": "lease-1",
            "candidate_positions": [1],
            "promote_candidate_ids": ["00000000-0000-0000-0000-000000000201"],
            "actor_id": "owner-1",
        },
    )

    rendered = json.loads(contents[0].text)
    assert rendered["error_code"] == "positional_authority_forbidden"


@pytest.mark.asyncio
async def test_candidate_product_mismatch_refuses_before_daemon_access(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def _unexpected_client() -> Any:
        raise AssertionError("Product mismatch must happen before daemon access")

    monkeypatch.setattr(server, "_client", _unexpected_client)
    server._selected_product_id = "prod-1"

    contents = await server.call_tool(
        PREPARE_TOOL,
        {
            "candidate_set_id": "00000000-0000-0000-0000-000000000101",
            "candidate_set_generation": 3,
            "candidate_set_digest": "sha256:" + "a" * 64,
            "product_id": "prod-2",
            "host_session_id": "host-1",
            "session_lease_id": "lease-1",
            "promote_candidate_ids": ["00000000-0000-0000-0000-000000000201"],
            "actor_id": "owner-1",
        },
    )

    rendered = json.loads(contents[0].text)
    assert rendered["error_code"] == "product_context_mismatch"


def test_preflight_adds_candidate_surface_without_replacing_normal_output(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        server,
        "format_preflight_response",
        lambda response: types.TextContent(type="text", text="normal preflight"),
    )
    response = {
        "status": "ok",
        "result": {
            "managed_preflight": {
                "candidate_set": _candidate_set(),
                "exact_spec_text": "The exact governed text.\n",
            }
        },
    }

    contents = server._format_preflight_contents(response)

    assert len(contents) == 2
    assert contents[0].text == "normal preflight"
    rendered = json.loads(contents[1].text)
    surface = rendered["managed_mcp_candidate_set"]
    assert surface["exact_spec"]["text"] == "The exact governed text.\n"
    assert surface["daemon_candidate_set"]["future_set_proof"] == {"opaque": True}
    assert surface["presentation"]["candidates"][0]["selected"] is False
    assert rendered["display_position_is_authority"] is False


@pytest.mark.parametrize(
    ("tool_name", "command_name", "response", "expected_phase"),
    [
        (
            HEARTBEAT_TOOL,
            "candidate_session.heartbeat",
            {
                "status": "ok",
                "result": {
                    "contract_version": 1,
                    "candidate_set_id": "00000000-0000-0000-0000-000000000101",
                    "candidate_set_generation": 3,
                    "candidate_set_digest": "sha256:" + "a" * 64,
                    "lease_expires_at": "2026-08-03T17:05:00Z",
                    "pending_count": 1,
                    "rejected_count": 0,
                    "promoted_count": 0,
                },
            },
            "heartbeat",
        ),
        (
            END_TOOL,
            "candidate_session.end",
            {
                "status": "ok",
                "result": {
                    "contract_version": 1,
                    "candidate_set_id": "00000000-0000-0000-0000-000000000101",
                    "candidate_set_generation": 3,
                    "erased_candidate_count": 1,
                    "erased_pending_count": 1,
                    "erased_rejected_count": 0,
                    "erased_promoted_disposition_count": 0,
                },
            },
            "end",
        ),
    ],
)
@pytest.mark.asyncio
async def test_session_tools_route_exact_access_and_render_operation(
    monkeypatch: pytest.MonkeyPatch,
    tool_name: str,
    command_name: str,
    response: dict[str, Any],
    expected_phase: str,
) -> None:
    client = _FakeClient(commands=(command_name,), response=response)
    monkeypatch.setattr(server, "_client", lambda: client)
    server._selected_product_id = "prod-1"

    contents = await server.call_tool(
        tool_name,
        {
            "access": {
                "contract_version": 1,
                "product_id": "prod-1",
                "host_session_id": "host-1",
                "session_lease_id": "lease-1",
                "candidate_set_generation": 3,
            }
        },
    )

    assert client.requests[0]["command"] == {
        "name": command_name,
        "params": {
            "access": {
                "contract_version": 1,
                "product_id": "prod-1",
                "host_session_id": "host-1",
                "session_lease_id": "lease-1",
                "candidate_set_generation": 3,
            }
        },
    }
    rendered = json.loads(contents[0].text)
    assert rendered["phase"] == expected_phase
    assert rendered["canonical_write"] == "none"
    if expected_phase == "end":
        assert rendered["erasure_is_rejection"] is False
