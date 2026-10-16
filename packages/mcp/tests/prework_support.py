"""Shared test-only helpers for host pre-work adapter tests.

Not a pytest test module: importing ``tests.test_prework_adapters`` from
another test file makes mypy see the same source under two module names.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from preflight_adapters import get_adapter, supported_hosts
from version import TOOLREQUEST_PROTOCOL_VERSION

HOSTS = list(supported_hosts())


class FakeDaemon:
    """Test-only daemon command runner (never a fake production hook path)."""

    def __init__(
        self,
        *,
        protocol_version: str | None = TOOLREQUEST_PROTOCOL_VERSION,
        supported: list[str] | None = None,
        deferred: list[str] | None = None,
        raise_on_capabilities: Exception | None = None,
        resolved_product_id: str | None = None,
        resolve_error: str | None = None,
        resolve_state: str | None = None,
    ) -> None:
        self.protocol_version = protocol_version
        self.supported = (
            supported if supported is not None else ["preflight.run", "workspace.resolve"]
        )
        self.deferred = deferred or []
        self.raise_on_capabilities = raise_on_capabilities
        self.resolved_product_id = resolved_product_id
        self.resolve_error = resolve_error
        self.resolve_state = resolve_state
        self.requests: list[dict[str, Any]] = []

    async def capabilities(self) -> dict[str, Any]:
        if self.raise_on_capabilities is not None:
            raise self.raise_on_capabilities
        return {
            "toolrequest_protocol_version": self.protocol_version,
            "supported_commands": self.supported,
            "deferred_commands": self.deferred,
        }

    async def send_tool_request(self, tool_request: dict[str, Any]) -> dict[str, Any]:
        self.requests.append(tool_request)
        name = tool_request.get("command", {}).get("name")
        if name == "workspace.resolve":
            if self.resolve_error:
                match_count = 2 if self.resolve_error == "ambiguous" else 0
                if self.resolve_error == "repair_required":
                    match_count = 1
                return {
                    "request_id": tool_request["request_id"],
                    "status": "rejected",
                    "result": {
                        "error": self.resolve_error,
                        "message": f"workspace.resolve {self.resolve_error}",
                        "state": self.resolve_state
                        or (
                            "local_workspace_repair_required"
                            if self.resolve_error == "repair_required"
                            else "local_workspace_unbound"
                        ),
                        "match_count": match_count,
                    },
                    "message": f"{self.resolve_error}: workspace.resolve {self.resolve_error}",
                }
            if self.resolved_product_id:
                return {
                    "request_id": tool_request["request_id"],
                    "status": "ok",
                    "result": {
                        "status": "resolved",
                        "outcome": {
                            "product_id": self.resolved_product_id,
                            "state": "local_workspace_bound",
                            "display": {"display_name": "Test Product"},
                        },
                    },
                }
            return {
                "request_id": tool_request["request_id"],
                "status": "rejected",
                "result": {
                    "error": "unbound",
                    "message": "No local workspace binding matches this folder.",
                    "state": "local_workspace_unbound",
                    "match_count": 0,
                },
                "message": "unbound: No local workspace binding matches this folder.",
            }
        return {"request_id": tool_request["request_id"], "status": "ok", "result": {}}


def home_for(host: str, root: Path) -> Path:
    return root / f".{'claude' if host == 'claude' else 'codex'}"


def install(host: str, root: Path):
    adapter = get_adapter(host, home=home_for(host, root))
    result = adapter.install(consent=True)
    return adapter, result


def startup_event(cwd: Path) -> dict[str, Any]:
    return {
        "session_id": "sess-1",
        "source": "startup",
        "cwd": str(cwd),
        "hook_event_name": "SessionStart",
        "transcript_path": "/should/not/be/read/transcript.jsonl",
    }
