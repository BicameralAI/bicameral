"""Codex CLI pre-work adapter.

Uses the documented Codex lifecycle hooks mechanism in
``$CODEX_HOME/hooks.json`` (default ``~/.codex/hooks.json``):

* ``SessionStart`` with ``source=startup`` is the genuine pre-work / activation
  boundary.
* ``Stop`` joined with ``permission_mode=plan`` is the Planning-complete
  capture event. Plan bytes come from exact ``last_assistant_message`` /
  ``last-assistant-message`` only.

Evidence for this host is independent: it does not imply Claude Code support.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from .base import HostAdapter, HostConfigError, HostSessionEvent
from .planning import extract_codex_plan_bytes


class CodexAdapter(HostAdapter):
    host_id = "codex"
    display_name = "Codex CLI"
    official_mechanism = "Codex lifecycle hooks (SessionStart + Stop plan-turn) in hooks.json"

    def default_home(self) -> Path:
        codex_home = os.environ.get("CODEX_HOME")
        if codex_home:
            return Path(codex_home)
        return Path.home() / ".codex"

    def config_path(self) -> Path:
        return self.home() / "hooks.json"

    def _write_hook(self, runner_invocation: str) -> None:
        # Validate the operator-owned hooks file before creating the supporting
        # user config layer, preserving fail-closed behavior on malformed JSON.
        self._read_config()
        self._ensure_user_config_layer()
        super()._write_hook(runner_invocation)

    def _ensure_user_config_layer(self) -> None:
        """Make user hooks discoverable without bypassing Codex hook trust."""
        config = self.home() / "config.toml"
        if config.exists():
            return
        try:
            config.parent.mkdir(parents=True, exist_ok=True)
            config.open("xb").close()
        except FileExistsError:
            return
        except OSError as exc:
            raise HostConfigError(
                f"Cannot create Codex user config layer {config}; refusing to install the hook."
            ) from exc

    def planning_hook_event(self) -> str:
        return "Stop"

    def is_planning_boundary(self, event: HostSessionEvent) -> bool:
        return (
            event.hook_event_name == "Stop"
            and event.permission_mode == "plan"
            and bool(event.session_id)
        )

    def extract_plan_bytes(self, payload: dict[str, Any]) -> bytes | None:
        return extract_codex_plan_bytes(payload)

    def parse_event(self, payload: dict[str, Any]) -> HostSessionEvent:
        hook_event_name = str(payload.get("hook_event_name") or "")
        session_id = str(payload.get("session_id") or "")
        source = str(payload.get("source") or "")
        cwd_raw = payload.get("cwd")
        cwd = str(cwd_raw) if isinstance(cwd_raw, str) and cwd_raw else None
        turn_id = str(payload.get("turn_id") or "")
        permission_mode = str(payload.get("permission_mode") or "")
        return HostSessionEvent(
            hook_event_name=hook_event_name,
            session_id=session_id,
            source=source,
            cwd=cwd,
            turn_id=turn_id,
            permission_mode=permission_mode,
        )
