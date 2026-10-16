"""Claude Code pre-work adapter.

Uses the documented Claude Code hooks mechanism in the user settings file
(``~/.claude/settings.json``):

* ``SessionStart`` is the genuine pre-work / activation boundary.
* ``PostToolUse`` matcher ``ExitPlanMode`` is the Planning-complete capture
  event. Plan bytes come from exact ``tool_response.plan`` only.

Evidence for this host is independent: it does not imply Codex support.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .base import HostAdapter, HostSessionEvent
from .planning import extract_claude_plan_bytes


class ClaudeCodeAdapter(HostAdapter):
    host_id = "claude"
    display_name = "Claude Code"
    official_mechanism = (
        "Claude Code hooks (SessionStart + PostToolUse ExitPlanMode) in settings.json"
    )

    def default_home(self) -> Path:
        return Path.home() / ".claude"

    def config_path(self) -> Path:
        return self.home() / "settings.json"

    def planning_hook_event(self) -> str:
        return "PostToolUse"

    def planning_hook_group(self, handler: dict[str, Any]) -> dict[str, Any]:
        return {"matcher": "ExitPlanMode", "hooks": [dict(handler)]}

    def is_planning_boundary(self, event: HostSessionEvent) -> bool:
        return (
            event.hook_event_name == "PostToolUse"
            and event.tool_name == "ExitPlanMode"
            and bool(event.session_id)
        )

    def extract_plan_bytes(self, payload: dict[str, Any]) -> bytes | None:
        return extract_claude_plan_bytes(payload)

    def parse_event(self, payload: dict[str, Any]) -> HostSessionEvent:
        hook_event_name = str(payload.get("hook_event_name") or "")
        session_id = str(payload.get("session_id") or "")
        source = str(payload.get("source") or "")
        cwd_raw = payload.get("cwd")
        cwd = str(cwd_raw) if isinstance(cwd_raw, str) and cwd_raw else None
        tool_name = str(payload.get("tool_name") or "")
        turn_id = str(payload.get("turn_id") or payload.get("tool_use_id") or "")
        permission_mode = str(payload.get("permission_mode") or "")
        return HostSessionEvent(
            hook_event_name=hook_event_name,
            session_id=session_id,
            source=source,
            cwd=cwd,
            tool_name=tool_name,
            turn_id=turn_id,
            permission_mode=permission_mode,
        )
