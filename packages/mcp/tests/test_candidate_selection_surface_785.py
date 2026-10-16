"""Contract tests for MCP #785 CandidateSet rendering and selection mediation."""

from __future__ import annotations

from copy import deepcopy

import pytest

from candidate_selection_surface import (
    COMMIT_TOOL,
    END_TOOL,
    HEARTBEAT_TOOL,
    PREPARE_TOOL,
    SurfaceContractError,
    build_commit_params,
    build_prepare_params,
    build_session_params,
    extract_candidate_set_surface,
    render_candidate_set,
    render_commit_response,
    render_prepare_response,
    render_session_response,
    shape_command_params,
    tool_definitions,
)

CANDIDATE_A = "00000000-0000-0000-0000-00000000000a"
CANDIDATE_B = "00000000-0000-0000-0000-00000000000b"
SET_ID = "00000000-0000-0000-0000-000000000100"
OPERATION_ID = "00000000-0000-0000-0000-000000000200"
CHALLENGE_ID = "00000000-0000-0000-0000-000000000300"
RESTART_EPOCH = "00000000-0000-0000-0000-000000000400"
DIGEST_A = "sha256:" + "a" * 64
DIGEST_B = "sha256:" + "b" * 64
DIGEST_C = "sha256:" + "c" * 64


def _candidate_set() -> dict:
    return {
        "contract_version": 1,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 7,
        "candidate_set_digest": DIGEST_A,
        "binding": {
            "product_id": "product-1",
            "host_session_id": "host-session-1",
            "governing_spec_binding_digest": DIGEST_B,
            "plan_digest": DIGEST_C,
            "accepted_model_base_position": 42,
            "accepted_model_base_digest": DIGEST_A,
        },
        "session_lease_id": "lease-1",
        "created_at": "2026-08-03T16:00:00Z",
        "lease_expires_at": "2026-08-03T16:05:00Z",
        "coverage_limitations": ["unsupported relation family remains unknown"],
        "future_proof_bundle": {
            "provider_receipt": "opaque-provider-receipt",
            "nested": {"must_survive": True},
        },
        "candidates": [
            {
                "candidate_id": CANDIDATE_A,
                "candidate_generation": 1,
                "proposed_decision": "Use deterministic retries.",
                "source_spans": [
                    {
                        "artifact_path": "docs/spec.md",
                        "start_byte": 10,
                        "end_byte": 40,
                        "future_span_proof": "keep-me",
                    }
                ],
                "contradiction_refs": ["claim:contradiction-1"],
                "dependency_candidate_ids": [CANDIDATE_B],
                "claim_trace_digest": DIGEST_B,
                "proposal_digest": DIGEST_C,
                "disposition": "pending",
                "future_candidate_proof": {"verifier": "independent"},
            },
            {
                "candidate_id": CANDIDATE_B,
                "candidate_generation": 1,
                "proposed_decision": "Cap retry attempts at eight.",
                "source_spans": [
                    {
                        "artifact_path": "docs/spec.md",
                        "start_byte": 41,
                        "end_byte": 72,
                    }
                ],
                "contradiction_refs": [],
                "dependency_candidate_ids": [],
                "claim_trace_digest": DIGEST_B,
                "proposal_digest": DIGEST_A,
                "disposition": "pending",
            },
        ],
    }


def _prepare_args() -> dict:
    return {
        "contract_version": 1,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 7,
        "candidate_set_digest": DIGEST_A,
        "product_id": "product-1",
        "host_session_id": "host-session-1",
        "session_lease_id": "lease-1",
        "promote_candidate_ids": [CANDIDATE_A, CANDIDATE_B],
        "reject_candidate_ids": [],
        "actor_id": "human-owner-1",
    }


def _preview() -> dict:
    return {
        "contract_version": 1,
        "selection_operation_id": OPERATION_ID,
        "selection_digest": DIGEST_A,
        "display_digest": DIGEST_B,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 7,
        "candidate_set_digest": DIGEST_C,
        "product_id": "product-1",
        "host_session_id": "host-session-1",
        "actor_id": "human-owner-1",
        "governing_spec_binding_digest": DIGEST_B,
        "accepted_model_base_position": 42,
        "accepted_model_base_digest": DIGEST_A,
        "policy_revision": "policy-7",
        "promote_effects": [
            {
                "candidate_id": CANDIDATE_A,
                "decision_id": "00000000-0000-0000-0000-000000000500",
                "proposed_decision": "Use deterministic retries.",
                "proposal_digest": DIGEST_C,
                "resulting_signoff": "approved_by_owner_policy",
                "future_effect_proof": {"source": "daemon"},
            }
        ],
        "reject_effects": [
            {
                "candidate_id": CANDIDATE_B,
                "resulting_disposition": "rejected",
            }
        ],
        "pending_candidate_count": 0,
        "resulting_signoff": "approved_by_owner_policy",
        "confirmation_challenge": {
            "challenge_id": CHALLENGE_ID,
            "restart_epoch": RESTART_EPOCH,
            "secret": "opaque-never-duplicate",
            "expires_at": "2026-08-03T16:05:00Z",
            "future_token_field": "preserve-me",
        },
        "future_preview_proof": {"policy_receipt": "opaque"},
    }


def test_tool_definitions_are_versioned_and_do_not_offer_positional_selection():
    definitions = {item["name"]: item for item in tool_definitions()}
    assert set(definitions) == {PREPARE_TOOL, COMMIT_TOOL, HEARTBEAT_TOOL, END_TOOL}
    prepare = definitions[PREPARE_TOOL]["inputSchema"]
    assert prepare["properties"]["contract_version"]["const"] == 1
    assert "promote_candidate_ids" in prepare["properties"]
    assert "candidate_positions" not in prepare["properties"]
    assert definitions[COMMIT_TOOL]["inputSchema"]["properties"]["human_confirmed"]["const"]


def test_prepare_builder_generates_operation_id_but_never_selection_authority():
    params = build_prepare_params(
        _prepare_args(),
        operation_id_factory=lambda: OPERATION_ID,
    )
    assert params["selection_operation_id"] == OPERATION_ID
    assert params["promote_candidate_ids"] == [CANDIDATE_A, CANDIDATE_B]
    assert "candidate_positions" not in params
    assert "auto_approve" not in params
    assert "human_confirmed" not in params


def test_prepare_builder_rejects_display_position_as_authority():
    arguments = {**_prepare_args(), "candidate_positions": [1, 2]}
    with pytest.raises(SurfaceContractError) as exc:
        build_prepare_params(arguments)
    assert exc.value.code == "positional_authority_forbidden"


def test_prepare_builder_rejects_empty_and_overlapping_subsets():
    empty = {**_prepare_args(), "promote_candidate_ids": []}
    with pytest.raises(SurfaceContractError) as exc:
        build_prepare_params(empty)
    assert exc.value.code == "empty_selection"

    overlap = {**_prepare_args(), "reject_candidate_ids": [CANDIDATE_A]}
    with pytest.raises(SurfaceContractError) as exc:
        build_prepare_params(overlap)
    assert exc.value.code == "overlapping_dispositions"


def test_commit_builder_requires_explicit_human_confirmation_and_opaque_token():
    base = {
        "contract_version": 1,
        "selection_operation_id": OPERATION_ID,
        "selection_digest": DIGEST_A,
        "actor_id": "human-owner-1",
        "confirmation_token": {
            "challenge_id": CHALLENGE_ID,
            "restart_epoch": RESTART_EPOCH,
            "secret": "opaque",
        },
    }
    with pytest.raises(SurfaceContractError) as exc:
        build_commit_params(base)
    assert exc.value.code == "human_confirmation_required"

    params = build_commit_params({**base, "human_confirmed": True})
    assert params["human_confirmed"] is True
    assert params["confirmation_token"] == base["confirmation_token"]
    assert params["confirmation_token"] is not base["confirmation_token"]


def test_session_builder_requires_exact_explicit_identity():
    params = build_session_params(
        {
            "access": {
                "contract_version": 1,
                "product_id": "product-1",
                "host_session_id": "host-session-1",
                "session_lease_id": "lease-1",
                "candidate_set_generation": 7,
            }
        }
    )
    assert params["access"]["candidate_set_generation"] == 7
    assert set(params["access"]) == {
        "contract_version",
        "product_id",
        "host_session_id",
        "session_lease_id",
        "candidate_set_generation",
    }


def test_shape_command_params_routes_exact_contracts():
    prepare = shape_command_params(
        PREPARE_TOOL,
        _prepare_args(),
        operation_id_factory=lambda: OPERATION_ID,
    )
    assert prepare["selection_operation_id"] == OPERATION_ID

    heartbeat = shape_command_params(
        HEARTBEAT_TOOL,
        {
            "access": {
                "contract_version": 1,
                "product_id": "product-1",
                "host_session_id": "host-session-1",
                "session_lease_id": "lease-1",
                "candidate_set_generation": 7,
            }
        },
    )
    assert heartbeat["access"]["session_lease_id"] == "lease-1"


def test_candidate_set_renderer_preserves_exact_spec_and_every_daemon_field():
    candidate_set = _candidate_set()
    exact_spec = "Requirement A\n  -> exact bytes, spacing, and punctuation.\n"
    rendered = render_candidate_set(
        candidate_set,
        exact_spec_text=exact_spec,
        annotation_colors={CANDIDATE_A: "#3366ff"},
    )

    assert rendered["daemon_candidate_set"] == candidate_set
    assert rendered["daemon_candidate_set"] is not candidate_set
    assert rendered["daemon_candidate_set"]["future_proof_bundle"]["nested"]["must_survive"]
    assert rendered["daemon_candidate_set"]["candidates"][0]["future_candidate_proof"] == {
        "verifier": "independent"
    }
    assert (
        rendered["daemon_candidate_set"]["candidates"][0]["source_spans"][0]["future_span_proof"]
        == "keep-me"
    )
    assert rendered["exact_spec"]["text"] == exact_spec
    assert rendered["exact_spec"]["rewritten"] is False
    assert rendered["exact_spec"]["utf8_byte_length"] == len(exact_spec.encode("utf-8"))


def test_candidate_set_renderer_never_marks_initial_candidates_selected_or_approved():
    rendered = render_candidate_set(_candidate_set())
    for candidate in rendered["presentation"]["candidates"]:
        assert candidate["selected"] is False
        assert candidate["accepted"] is False
        assert candidate["approved"] is False
    assert rendered["authority"]["selection_status"] == "defaults_proposed_not_committed"
    assert rendered["authority"]["approval_status"] == "not_approved"


def test_candidate_set_annotations_are_accessible_without_color():
    rendered = render_candidate_set(_candidate_set())
    annotations = rendered["presentation"]["annotations"]
    assert annotations
    # Decision text leads; stable ID remains a field, not the lead label (#809).
    assert annotations[0]["label"].startswith("Use deterministic retries.")
    assert "docs/spec.md" in annotations[0]["label"]
    assert "bytes 10 through 40" in annotations[0]["label"]
    assert annotations[0]["candidate_id"] == CANDIDATE_A
    assert "color" not in annotations[0]
    semantics = rendered["presentation"]["annotation_semantics"]
    assert semantics["color_is_authority"] is False
    assert semantics["stable_candidate_id_is_authority"] is True
    assert semantics["display_position_is_authority"] is False


def test_preflight_extractor_finds_nested_candidate_set_and_exact_text():
    candidate_set = _candidate_set()
    response = {
        "status": "ok",
        "result": {
            "managed_preflight": {
                "outcome": "analysis_available",
                "exact_spec_text": "exact spec bytes",
                "candidate_set": candidate_set,
            }
        },
    }
    rendered = extract_candidate_set_surface(response)
    assert rendered is not None
    assert rendered["daemon_candidate_set"] == candidate_set
    assert rendered["exact_spec"]["text"] == "exact spec bytes"


def test_prepare_renderer_preserves_exact_preview_and_opaque_future_fields():
    preview = _preview()
    response = {"status": "ok", "result": {"candidate_selection_preview": preview}}
    rendered = render_prepare_response(response)

    assert rendered["status"] == "confirmation_required"
    assert rendered["preview"] == preview
    assert rendered["preview"]["future_preview_proof"] == {"policy_receipt": "opaque"}
    assert rendered["preview"]["promote_effects"][0]["future_effect_proof"] == {"source": "daemon"}
    assert (
        rendered["human_action"]["label"]
        == "Promote and approve 1 selected candidate as Product Owner"
    )
    assert rendered["confirmation_summary"]["signoff_wording"] == "owner-policy-approved"
    assert rendered["confirmation_summary"]["challenge_in_human_summary"] is False
    assert "opaque-never-duplicate" not in rendered["human_action"]["summary"]
    assert "secret" not in rendered["human_action"]["summary"].lower()
    assert rendered["human_action"]["agent_may_confirm"] is False
    assert rendered["canonical_write"] == "none"


def test_prepare_renderer_transports_challenge_once_without_deriving_secret_summary():
    preview = _preview()
    response = {"candidate_selection_preview": preview}
    rendered = render_prepare_response(response)
    assert rendered["confirmation_transport"]["challenge"] == preview["confirmation_challenge"]
    assert rendered["confirmation_transport"]["opaque"] is True
    assert rendered["confirmation_transport"]["persist"] is False
    assert rendered["confirmation_transport"]["log"] is False
    assert "secret" not in rendered["human_action"]


def test_prepare_renderer_preserves_dependency_failure_and_exact_candidate_ids():
    response = {
        "status": "error",
        "failure": {
            "code": "dependency_closure_required",
            "detail": "Selection omits required dependencies.",
            "candidate_ids": [CANDIDATE_B],
            "future_failure_context": {"closure_digest": DIGEST_C},
        },
    }
    rendered = render_prepare_response(response)
    assert rendered["error_code"] == "dependency_closure_required"
    assert rendered["failure"]["candidate_ids"] == [CANDIDATE_B]
    assert rendered["failure"]["future_failure_context"] == {"closure_digest": DIGEST_C}
    assert rendered["recovery"]["action"] == "revise_selection"
    assert rendered["canonical_write"] == "none_or_daemon_reported_only"


def test_commit_renderer_preserves_receipt_and_replay_reconciliation_facts():
    receipt = {
        "contract_version": 1,
        "selection_operation_id": OPERATION_ID,
        "selection_digest": DIGEST_A,
        "product_id": "product-1",
        "actor_id": "human-owner-1",
        "promoted_candidate_ids": [CANDIDATE_A],
        "rejected_candidate_ids": [CANDIDATE_B],
        "decision_ids": ["00000000-0000-0000-0000-000000000500"],
        "canonical_event_ids": ["00000000-0000-0000-0000-000000000600"],
        "resulting_signoff": "approved_by_owner_policy",
        "pending_candidate_count": 0,
        "replay_verified": True,
        "session_reconciled": True,
        "future_receipt_proof": {"event_store": "git"},
    }
    rendered = render_commit_response({"status": "ok", "candidate_selection_receipt": receipt})
    assert rendered["status"] == "committed"
    assert rendered["receipt"] == receipt
    assert rendered["receipt"]["future_receipt_proof"] == {"event_store": "git"}
    assert rendered["replay_verified"] is True
    assert rendered["session_reconciled"] is True
    assert rendered["canonical_write"] == "daemon_reported"


def test_session_end_renderer_states_erasure_is_not_rejection():
    result = {
        "contract_version": 1,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 7,
        "erased_candidate_count": 2,
        "erased_pending_count": 1,
        "erased_rejected_count": 1,
        "erased_promoted_disposition_count": 0,
    }
    rendered = render_session_response(
        {"status": "ok", "candidate_session_end_result": result},
        operation="end",
    )
    assert rendered["canonical_write"] == "none"
    assert rendered["erasure_is_rejection"] is False
    assert rendered["result"] == result


def test_unsupported_versions_fail_closed_in_builders_and_renderers():
    arguments = {**_prepare_args(), "contract_version": 2}
    with pytest.raises(SurfaceContractError) as exc:
        build_prepare_params(arguments)
    assert exc.value.code == "unsupported_contract_version"

    candidate_set = deepcopy(_candidate_set())
    candidate_set["contract_version"] = 2
    with pytest.raises(SurfaceContractError) as exc:
        render_candidate_set(candidate_set)
    assert exc.value.code == "unsupported_contract_version"
