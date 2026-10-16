"""Contract tests for CONF-MCP-PLAN-HOOK-01 (mcp#832).

These tests exercise the packaged installer and runner against recorded
official host-event JSON fixtures. They are not User Journey acceptance and
do not close BicameralAI/bicameral-mcp#786.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import pytest
from test_prework_adapters import HOSTS, _FakeDaemon, _home_for, _install, _startup_event

from preflight_adapters.context import FORBIDDEN_EVENT_FIELDS, assert_no_forbidden_fields
from preflight_adapters.planning import compute_plan_digest
from preflight_adapters.runner import PreworkOutcome, run_prework
from preflight_adapters.state import AdapterState

PLAN_TEXT = "## Implement Planning-complete hook\n1. Hash exact Plan bytes.\n"
PLAN_DIGEST = compute_plan_digest(PLAN_TEXT.encode("utf-8"))
SPEC_BIND = {
    "repository_id": "BicameralAI/bicameral-bot",
    "git_head": "51ef99fc5584f7a2b042d0d58c811a52631b7e72",
    "artifact_path": "docs/specs/managed-mcp-drift-aware-continuation-runtime-architecture.md",
    "blob_id": "blob-spec-bind",
    "artifact_content_digest": "sha256:" + "ab" * 32,
}


def _claude_exit_plan_event(**overrides: Any) -> dict[str, Any]:
    event = {
        "hook_event_name": "PostToolUse",
        "session_id": "sess-plan",
        "cwd": "/workspace",
        "tool_name": "ExitPlanMode",
        "tool_use_id": "toolu_plan_1",
        "turn_id": "turn-plan-1",
        "product_id": "prd_planning",
        "tool_response": {"plan": PLAN_TEXT, "filePath": "/tmp/plan.md"},
        "transcript_path": "/should/not/be/read/transcript.jsonl",
        "messages": [{"role": "assistant", "content": "scraped chatter"}],
    }
    event.update(overrides)
    return event


def _codex_plan_turn_event(**overrides: Any) -> dict[str, Any]:
    event = {
        "hook_event_name": "Stop",
        "session_id": "sess-plan",
        "cwd": "/workspace",
        "turn_id": "turn-plan-1",
        "permission_mode": "plan",
        "product_id": "prd_planning",
        "last_assistant_message": PLAN_TEXT,
        "transcript_path": "/should/not/be/read/transcript.jsonl",
        "messages": [{"role": "assistant", "content": "scraped chatter"}],
    }
    event.update(overrides)
    return event


def _planning_event(host: str, **overrides: Any) -> dict[str, Any]:
    if host == "claude":
        return _claude_exit_plan_event(**overrides)
    return _codex_plan_turn_event(**overrides)


def _managed_commands(data: dict[str, Any], event_name: str) -> list[str]:
    commands: list[str] = []
    for group in data.get("hooks", {}).get(event_name, []):
        for handler in group.get("hooks", []):
            command = handler.get("command")
            if isinstance(command, str) and "prework-run" in command:
                commands.append(command)
    return commands


@pytest.mark.parametrize("host", HOSTS)
def test_install_writes_second_planning_hook_distinct_from_session_start(
    host: str, tmp_path: Path
) -> None:
    adapter, result = _install(host, tmp_path)
    assert result.ok is True
    data = json.loads(adapter.config_path().read_text())
    session_cmds = _managed_commands(data, "SessionStart")
    planning_event = "PostToolUse" if host == "claude" else "Stop"
    planning_cmds = _managed_commands(data, planning_event)
    assert len(session_cmds) == 1
    assert len(planning_cmds) == 1
    if host == "claude":
        groups = data["hooks"]["PostToolUse"]
        managed = [g for g in groups if g.get("matcher") == "ExitPlanMode"]
        assert len(managed) == 1
    assert adapter.status().hook_present is True


@pytest.mark.parametrize("host", HOSTS)
def test_update_disable_uninstall_preserve_user_hooks_for_both_entries(
    host: str, tmp_path: Path
) -> None:
    home = _home_for(host, tmp_path)
    home.mkdir(parents=True)
    planning_event = "PostToolUse" if host == "claude" else "Stop"
    existing = {
        "hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": "echo keep-start"}]}],
            planning_event: [
                {
                    **({"matcher": "Edit|Write"} if host == "claude" else {}),
                    "hooks": [{"type": "command", "command": "echo keep-plan"}],
                }
            ],
        }
    }
    (home / ("settings.json" if host == "claude" else "hooks.json")).write_text(
        json.dumps(existing)
    )
    adapter, _ = _install(host, tmp_path)
    adapter.update()
    data = json.loads(adapter.config_path().read_text())
    start_cmds = [h["command"] for g in data["hooks"]["SessionStart"] for h in g["hooks"]]
    plan_cmds = [h["command"] for g in data["hooks"][planning_event] for h in g["hooks"]]
    assert "echo keep-start" in start_cmds
    assert "echo keep-plan" in plan_cmds
    assert len(_managed_commands(data, "SessionStart")) == 1
    assert len(_managed_commands(data, planning_event)) == 1

    adapter.disable()
    data = json.loads(adapter.config_path().read_text())
    start_cmds = [h["command"] for g in data["hooks"]["SessionStart"] for h in g["hooks"]]
    plan_cmds = [h["command"] for g in data["hooks"][planning_event] for h in g["hooks"]]
    assert start_cmds == ["echo keep-start"]
    assert plan_cmds == ["echo keep-plan"]

    adapter.uninstall()
    data = json.loads(adapter.config_path().read_text())
    start_cmds = [h["command"] for g in data["hooks"]["SessionStart"] for h in g["hooks"]]
    plan_cmds = [h["command"] for g in data["hooks"][planning_event] for h in g["hooks"]]
    assert start_cmds == ["echo keep-start"]
    assert plan_cmds == ["echo keep-plan"]
    assert adapter.status().state == AdapterState.NOT_INSTALLED


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_planning_complete_submits_one_managed_planning_preflight(
    host: str, tmp_path: Path
) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    event = _planning_event(host, governing_spec=SPEC_BIND)

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.INVOKED
    assert result.preflight_invoked is True
    assert len(daemon.requests) == 1
    request = daemon.requests[0]
    params = request["command"]["params"]
    assert "checkpoint_hint" not in params
    mp = params["managed_planning"]
    assert mp["plan_digest"] == PLAN_DIGEST
    assert mp["host_session_id"] == "sess-plan"
    assert mp["host_turn_id"] == "turn-plan-1"
    assert mp["host_kind"] == ("claude_code" if host == "claude" else "codex")
    assert "governing_spec" not in mp
    assert PLAN_TEXT not in json.dumps(request)
    assert request["authority"]["audit_metadata"]["checkpoint"] == "planning_complete"
    assert not adapter._store().activation_receipt_dir.exists()


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_session_start_does_not_submit_managed_planning(host: str, tmp_path: Path) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon(resolved_product_id="prd_session_start")
    event = _startup_event(tmp_path)
    event["tool_response"] = {"plan": PLAN_TEXT}
    event["last_assistant_message"] = PLAN_TEXT
    event["governing_spec"] = SPEC_BIND

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.INVOKED
    assert len(daemon.requests) == 2
    assert daemon.requests[0]["command"]["name"] == "workspace.resolve"
    assert daemon.requests[1]["command"]["name"] == "preflight.run"
    params = daemon.requests[1]["command"]["params"]
    assert "managed_planning" not in params
    assert params["checkpoint_hint"] == "pre_work"
    assert set(params) <= {"files", "symbols", "diff_context", "branch", "checkpoint_hint"}


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_planning_without_plan_bytes_does_not_scrape_transcript(
    host: str, tmp_path: Path
) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    if host == "claude":
        event = _claude_exit_plan_event(tool_response={"filePath": "/tmp/plan.md"})
    else:
        event = _codex_plan_turn_event()
        event.pop("last_assistant_message")
        event["last-assistant-message"] = ""
    event["conversation"] = "full transcript dump"
    event["transcript"] = PLAN_TEXT
    assert set(assert_no_forbidden_fields(event)) >= {
        "transcript_path",
        "messages",
        "conversation",
        "transcript",
    }

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.SKIPPED_NO_PLAN_BYTES
    assert result.preflight_invoked is False
    assert daemon.requests == []
    assert "scrape" in result.message.lower()


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_duplicate_planning_event_is_exactly_once(host: str, tmp_path: Path) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    event = _planning_event(host)

    first = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)
    second = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert first.outcome == PreworkOutcome.INVOKED
    assert second.outcome == PreworkOutcome.SKIPPED_ALREADY_FIRED
    assert len(daemon.requests) == 1


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_same_session_different_plan_digest_is_not_deduped(host: str, tmp_path: Path) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    first = _planning_event(host)
    second_plan = PLAN_TEXT + "\n2. Second revision.\n"
    if host == "claude":
        second = _claude_exit_plan_event(tool_response={"plan": second_plan})
    else:
        second = _codex_plan_turn_event(last_assistant_message=second_plan)

    await run_prework(host, first, home=adapter.home(), client_factory=lambda: daemon)
    await run_prework(host, second, home=adapter.home(), client_factory=lambda: daemon)

    assert len(daemon.requests) == 2
    digests = {
        req["command"]["params"]["managed_planning"]["plan_digest"] for req in daemon.requests
    }
    assert digests == {
        PLAN_DIGEST,
        "sha256:" + hashlib.sha256(second_plan.encode("utf-8")).hexdigest(),
    }


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_plan_bytes_never_sent_as_product_truth(host: str, tmp_path: Path) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    plan = "Implement docs/specs/governing.md at HEAD abc123. This Plan is not the governing spec."
    if host == "claude":
        event = _claude_exit_plan_event(tool_response={"plan": plan})
    else:
        event = _codex_plan_turn_event(last_assistant_message=plan)
    event["branch"] = "main"
    event["cwd"] = str(tmp_path)

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.INVOKED
    mp = daemon.requests[0]["command"]["params"]["managed_planning"]
    assert "governing_spec" not in mp
    assert mp["plan_digest"] == compute_plan_digest(plan.encode("utf-8"))
    serialized = json.dumps(daemon.requests[0])
    assert plan not in serialized
    assert "docs/specs/governing.md" not in serialized


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_non_planning_tool_event_does_not_submit_managed_planning(
    host: str, tmp_path: Path
) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    if host == "claude":
        event = _claude_exit_plan_event(tool_name="Write")
    else:
        event = _codex_plan_turn_event(permission_mode="default")

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.SKIPPED_NOT_PREWORK
    assert daemon.requests == []


@pytest.mark.asyncio
async def test_codex_accepts_hyphenated_last_assistant_message(tmp_path: Path) -> None:
    adapter, _ = _install("codex", tmp_path)
    daemon = _FakeDaemon()
    event = _codex_plan_turn_event()
    event.pop("last_assistant_message")
    event["last-assistant-message"] = PLAN_TEXT

    result = await run_prework("codex", event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.INVOKED
    assert daemon.requests[0]["command"]["params"]["managed_planning"]["plan_digest"] == PLAN_DIGEST


@pytest.mark.asyncio
async def test_legacy_governing_spec_inputs_are_omitted(tmp_path: Path) -> None:
    adapter, _ = _install("claude", tmp_path)
    daemon = _FakeDaemon()
    other = dict(SPEC_BIND)
    other["artifact_path"] = "docs/other.md"
    event = _claude_exit_plan_event(
        governing_spec=SPEC_BIND,
        managed_planning={"governing_spec": other},
    )

    result = await run_prework("claude", event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.INVOKED
    mp = daemon.requests[0]["command"]["params"]["managed_planning"]
    assert "governing_spec" not in mp


def test_forbidden_fields_still_denylist_transcript() -> None:
    assert "transcript" in FORBIDDEN_EVENT_FIELDS
    assert "messages" in FORBIDDEN_EVENT_FIELDS
    assert "conversation" in FORBIDDEN_EVENT_FIELDS
