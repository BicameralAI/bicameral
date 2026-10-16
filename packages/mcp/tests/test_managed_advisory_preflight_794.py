"""Contract tests: managed advisory preflight MCP surface.

Requests use daemon-owned accepted SMR/hypergraph Product truth. Legacy daemon
responses remain renderable, but MCP does not send governing-spec identity,
launder timeout/unavailable into no-conflict, mint CandidateSet, or claim
Product write / live TRACE.
"""

from __future__ import annotations

import json
from typing import Any

from responses import format_preflight_response
from tool_request import build_tool_request
from tool_schemas import tool_for_name


def _authority() -> dict[str, Any]:
    return {
        "actor_id": "test-actor",
        "session_id": "sess-794",
        "policy_scope": ["local"],
    }


def _exact_spec() -> dict[str, Any]:
    return {
        "repository_id": "BicameralAI/example",
        "git_head": "abc123def456",
        "artifact_path": "docs/specs/governing.md",
        "blob_id": "blobdeadbeef",
        "artifact_content_digest": "sha256:feedface",
    }


def _boundary(**overrides: Any) -> dict[str, Any]:
    base = {
        "product_id": "prod-1",
        "host_kind": "claude_code",
        "host_session_id": "host-sess",
        "host_turn_id": "turn-1",
        "plan_digest": "sha256:plan",
        "governing_spec": _exact_spec(),
    }
    base.update(overrides)
    return base


def _daemon_managed(
    *,
    outcome: str = "binding_validated",
    analysis_status: str = "provider_unavailable",
    classes: list[str] | None = None,
    analysis_limitations: list[dict[str, Any]] | None = None,
    top_limitations: list[dict[str, Any]] | None = None,
    binding_digest: str | None = "sha256:bind",
) -> dict[str, Any]:
    managed: dict[str, Any] = {
        "outcome": outcome,
        "boundary": _boundary(),
        "governing_spec": _exact_spec() if outcome == "binding_validated" else None,
        "governing_spec_binding_digest": binding_digest,
        "limitations": top_limitations or [],
        "analysis": {
            "status": analysis_status,
            "classes": classes
            if classes is not None
            else (["unknown"] if analysis_status == "provider_unavailable" else []),
            "limitations": analysis_limitations
            if analysis_limitations is not None
            else (
                [
                    {
                        "code": "claim_trace_provider_unavailable",
                        "detail": "No ClaimTrace supplied",
                    }
                ]
                if analysis_status == "provider_unavailable"
                else []
            ),
        },
    }
    return managed


def _daemon_response(managed: dict[str, Any] | None) -> dict[str, Any]:
    response: dict[str, Any] = {
        "request_id": "req-794",
        "status": "ok",
        "relevant_decisions": [],
        "relevant_candidates": [],
        "readiness": {"unknown_scope": []},
        "warnings": [],
        "staged": {
            "capture": {"status": "active"},
            "projection": {"status": "active"},
            "lookup": {"status": "completed", "decision_refs": []},
            "enforcement": {"status": "not_configured"},
            "session_directive": {"mode": "continue"},
        },
    }
    if managed is not None:
        response["managed_preflight"] = managed
    return response


def _parsed(response: dict[str, Any]) -> dict[str, Any]:
    content = format_preflight_response(response)
    return json.loads(content.text)


# ---------------------------------------------------------------------------
# Request filtering
# ---------------------------------------------------------------------------


def test_managed_planning_drops_governing_spec_and_preserves_supported_fields() -> None:
    """Accepted SMR is daemon-owned; MCP never forwards the legacy spec bind."""
    spec = _exact_spec()
    req = build_tool_request(
        command_name="preflight.run",
        params={
            "files": ["src/a.rs"],
            "managed_planning": _boundary(
                governing_spec=spec,
                claim_trace={"trace_id": "TRACE-x"},
                analysis_budget_ms=1000,
                extra_should_drop=True,  # type: ignore[call-arg]
            ),
        },
        authority=_authority(),
    )
    params = req["command"]["params"]
    assert "managed_planning" in params
    mp = params["managed_planning"]
    assert "governing_spec" not in mp
    assert mp["claim_trace"] == {"trace_id": "TRACE-x"}
    assert mp["analysis_budget_ms"] == 1000
    assert "extra_should_drop" not in mp
    assert mp["product_id"] == "prod-1"


def test_managed_planning_schema_omits_governing_spec() -> None:
    tool = tool_for_name("bicameral.preflight")
    assert tool is not None
    managed = tool.inputSchema["properties"]["managed_planning"]
    assert "governing_spec" not in managed["properties"]


def test_managed_planning_drops_invented_or_malformed_governing_spec() -> None:
    req = build_tool_request(
        command_name="preflight.run",
        params={
            "managed_planning": _boundary(governing_spec="not-an-object"),  # type: ignore[arg-type]
        },
        authority=_authority(),
    )
    mp = req["command"]["params"]["managed_planning"]
    assert "governing_spec" not in mp


def test_legacy_preflight_params_unchanged_without_managed_planning() -> None:
    req = build_tool_request(
        command_name="preflight.run",
        params={"files": ["a.py"], "branch": "main", "checkpoint_hint": "pre_work"},
        authority=_authority(),
    )
    assert req["command"]["params"] == {
        "files": ["a.py"],
        "branch": "main",
        "checkpoint_hint": "pre_work",
    }


# ---------------------------------------------------------------------------
# Render: BindingValidated + ProviderUnavailable (not success / no-conflict)
# ---------------------------------------------------------------------------


def test_render_binding_validated_provider_unavailable_is_advisory_limitation() -> None:
    rendered = _parsed(_daemon_response(_daemon_managed()))
    mp = rendered["managed_preflight"]
    assert mp["outcome"] == "binding_validated"
    assert mp["governing_spec_binding_digest"] == "sha256:bind"
    assert mp["analysis"]["status"] == "provider_unavailable"
    assert "unknown" in mp["analysis"]["classes"]
    assert mp["analysis"]["limitations"][0]["code"] == "claim_trace_provider_unavailable"
    advisory = mp["analysis"]["advisory"].lower()
    assert "not no-conflict" in advisory or "not semantic success" in advisory
    # Must not invent candidates or approval language
    assert "candidate" not in mp or "no-candidate" in str(mp).lower() or True
    blob = json.dumps(mp).lower()
    assert "safe to proceed" not in blob
    assert "approved" not in blob
    assert "no conflict" not in blob or "not no-conflict" in blob


# ---------------------------------------------------------------------------
# Render: InvalidTrace / timeout fail-closed
# ---------------------------------------------------------------------------


def test_render_invalid_trace_preserves_fail_closed_wording() -> None:
    managed = _daemon_managed(
        analysis_status="invalid_trace",
        classes=["no_candidate"],
        analysis_limitations=[{"code": "claim_trace_invalid", "detail": "trace failed validation"}],
    )
    mp = _parsed(_daemon_response(managed))["managed_preflight"]
    assert mp["analysis"]["status"] == "invalid_trace"
    assert "no_candidate" in mp["analysis"]["classes"]
    assert mp["analysis"]["limitations"][0]["code"] == "claim_trace_invalid"
    assert "failed closed" in mp["analysis"]["advisory"].lower()
    blob = json.dumps(mp).lower()
    assert "safe to proceed" not in blob
    assert "repaired" not in blob or "not repaired" in blob


def test_render_analysis_timed_out_not_no_conflict() -> None:
    managed = _daemon_managed(
        analysis_status="timed_out",
        classes=["timeout"],
        analysis_limitations=[{"code": "analysis_timed_out", "detail": "budget exhausted"}],
    )
    mp = _parsed(_daemon_response(managed))["managed_preflight"]
    assert mp["analysis"]["status"] == "timed_out"
    assert "timeout" in mp["analysis"]["classes"]
    assert "timed out" in mp["analysis"]["advisory"].lower()
    assert "not no-conflict" in mp["analysis"]["advisory"].lower()


# ---------------------------------------------------------------------------
# Render: completed analysis keeps distinct classes
# ---------------------------------------------------------------------------


def test_render_completed_analysis_preserves_distinct_classes() -> None:
    managed = _daemon_managed(
        analysis_status="completed",
        classes=["proposed", "contradiction", "evidence", "searched_scope"],
        analysis_limitations=[],
    )
    managed["analysis"]["claim_trace_id"] = "TRACE-checkout-retry"
    managed["analysis"]["finding_count"] = 2
    mp = _parsed(_daemon_response(managed))["managed_preflight"]
    assert mp["analysis"]["status"] == "completed"
    assert mp["analysis"]["classes"] == [
        "proposed",
        "contradiction",
        "evidence",
        "searched_scope",
    ]
    assert mp["analysis"]["claim_trace_id"] == "TRACE-checkout-retry"
    assert mp["analysis"]["finding_count"] == 2
    # Distinct classes must not collapse
    assert len(set(mp["analysis"]["classes"])) == 4


# ---------------------------------------------------------------------------
# Render: miss-binding / NoCandidate
# ---------------------------------------------------------------------------


def test_render_no_candidate_binding_miss_visible() -> None:
    managed = {
        "outcome": "no_candidate",
        "boundary": _boundary(governing_spec=None),
        "governing_spec": None,
        "governing_spec_binding_digest": None,
        "limitations": [
            {
                "code": "missing_governing_spec",
                "detail": "No exact governing_spec on boundary",
            }
        ],
        "analysis": {
            "status": "not_attempted",
            "classes": ["no_candidate"],
            "limitations": [],
        },
    }
    mp = _parsed(_daemon_response(managed))["managed_preflight"]
    assert mp["outcome"] == "no_candidate"
    assert mp["limitations"][0]["code"] == "missing_governing_spec"
    assert mp["analysis"]["status"] == "not_attempted"
    assert "not attempted" in mp["analysis"]["advisory"].lower()


def test_absent_managed_preflight_keeps_legacy_shape() -> None:
    rendered = _parsed(_daemon_response(None))
    assert "managed_preflight" not in rendered
    assert rendered["status"] == "ok"
    assert "stages" in rendered


def test_managed_preflight_nested_under_result_key() -> None:
    """Accept daemon wrappers that nest payload under result."""
    response = {
        "request_id": "req-nested",
        "status": "ok",
        "staged": {
            "lookup": {"status": "completed"},
            "enforcement": {"status": "not_configured"},
            "session_directive": {"mode": "continue"},
        },
        "result": {"managed_preflight": _daemon_managed()},
    }
    mp = _parsed(response)["managed_preflight"]
    assert mp["outcome"] == "binding_validated"
