"""UX presentation-state matrix for MCP #785 (bic:spec-governance-gate)."""

from __future__ import annotations

from candidate_selection_surface import (
    PRESENTATION_STATE_MATRIX,
    UX_PAGE_CANDIDATE_SET,
    UX_PAGE_COMMIT,
    UX_PAGE_PREPARE,
    UX_SURFACE_NAME,
    render_candidate_set,
    render_commit_response,
    render_failure,
    render_prepare_response,
    render_session_response,
)

CANDIDATE_A = "00000000-0000-0000-0000-00000000000a"
CANDIDATE_B = "00000000-0000-0000-0000-00000000000b"
SET_ID = "00000000-0000-0000-0000-000000000100"
DIGEST = "sha256:" + "a" * 64


def _candidate_set() -> dict:
    return {
        "contract_version": 1,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 1,
        "candidate_set_digest": DIGEST,
        "binding": {"product_id": "p1", "host_session_id": "h1"},
        "session_lease_id": "lease-1",
        "candidates": [
            {
                "candidate_id": CANDIDATE_A,
                "proposed_decision": "Use retries.",
                "source_spans": [
                    {
                        "artifact_path": "docs/spec.md",
                        "start_byte": 0,
                        "end_byte": 8,
                    }
                ],
                "disposition": "pending",
                "dependency_candidate_ids": [CANDIDATE_B],
            }
        ],
    }


def test_presentation_matrix_covers_required_display_states():
    required = {
        "candidate_set_review",
        "confirmation_required_promote",
        "confirmation_required_promote_and_approve",
        "committed",
        "session_heartbeat_ok",
        "session_ended",
        "error_dependency_closure_required",
        "error_candidate_set_stale",
        "error_preview_stale",
        "error_session_lease_expired",
        "error_human_confirmation_required",
        "error_unsupported_contract_version",
        "error_generic",
    }
    assert required <= set(PRESENTATION_STATE_MATRIX)
    for state, row in PRESENTATION_STATE_MATRIX.items():
        assert len(row["title"]) >= 4, state
        assert len(row["visible_result"]) >= 20, state
        assert len(row["next_user_action"]) >= 10, state


def test_candidate_set_review_ux_block():
    rendered = render_candidate_set(_candidate_set(), exact_spec_text="exact-bytes")
    ux = rendered["ux"]
    assert ux["surface"] == UX_SURFACE_NAME
    assert ux["page_or_route"] == UX_PAGE_CANDIDATE_SET
    assert ux["presentation_state"] == "candidate_set_review"
    assert ux["agent_may_confirm"] is False
    assert ux["color_is_authority"] is False
    assert ux["selection_policy"] == "auto_promote_unless_rejected_or_contradiction"
    assert "accept_defaults" in ux["primary_actions"]
    assert "not_track" in ux["primary_actions"]
    assert rendered["presentation"]["candidates"][0]["selected"] is False


def test_prepare_ux_promote_vs_approve_policy():
    challenge = {"token": "opaque", "nonce": "n1"}
    base_preview = {
        "contract_version": 1,
        "selection_operation_id": "00000000-0000-0000-0000-000000000200",
        "selection_digest": DIGEST,
        "confirmation_challenge": challenge,
        "pending_count": 1,
        "promote_effects": [{"candidate_id": CANDIDATE_A}],
        "reject_effects": [],
    }
    promote = render_prepare_response(
        {
            "candidate_selection_preview": {
                **base_preview,
                "resulting_signoff": "proposed",
            }
        }
    )
    assert promote["ux"]["presentation_state"] == "confirmation_required_promote"
    assert promote["ux"]["page_or_route"] == UX_PAGE_PREPARE
    assert promote["human_action"]["label"] == "Promote 1 selected candidate"
    assert "Accept defaults" not in promote["human_action"]["label"]
    assert promote["ux"]["agent_may_confirm"] is False
    assert promote["confirmation_summary"]["challenge_in_human_summary"] is False
    assert "opaque" not in promote["human_action"]["summary"].lower()
    assert "secret" not in promote["human_action"]["summary"].lower()

    approve = render_prepare_response(
        {
            "candidate_selection_preview": {
                **base_preview,
                "resulting_signoff": "approved_by_owner_policy",
            }
        }
    )
    assert approve["ux"]["presentation_state"] == "confirmation_required_promote_and_approve"
    assert (
        approve["human_action"]["label"]
        == "Promote and approve 1 selected candidate as Product Owner"
    )


def test_commit_and_session_ux_blocks():
    committed = render_commit_response(
        {
            "candidate_selection_receipt": {
                "contract_version": 1,
                "promoted_candidate_ids": [CANDIDATE_A],
                "rejected_candidate_ids": [],
                "replay_verified": True,
                "session_reconciled": True,
            }
        }
    )
    assert committed["ux"]["presentation_state"] == "committed"
    assert committed["ux"]["page_or_route"] == UX_PAGE_COMMIT
    assert committed["ux"]["is_product_acceptance"] is False

    heartbeat = render_session_response(
        {
            "result": {
                "contract_version": 1,
                "session_lease_id": "lease-1",
                "lease_expires_at": "2026-08-04T00:00:00Z",
            }
        },
        operation="heartbeat",
    )
    assert heartbeat["ux"]["presentation_state"] == "session_heartbeat_ok"

    ended = render_session_response(
        {
            "result": {
                "contract_version": 1,
                "session_lease_id": "lease-1",
                "erased": True,
            }
        },
        operation="end",
    )
    assert ended["ux"]["presentation_state"] == "session_ended"
    assert ended["erasure_is_rejection"] is False


def test_failure_ux_maps_known_codes():
    rendered = render_failure(
        {
            "failure": {
                "code": "dependency_closure_required",
                "missing_candidate_ids": [CANDIDATE_B],
            }
        },
        expected_codes=frozenset({"dependency_closure_required"}),
    )
    assert rendered["ux"]["presentation_state"] == "error_dependency_closure_required"
    assert rendered["recovery"]["action"] == "revise_selection"
    # Meaningful negative: recovery must not claim silent dependency expansion.
    visible = rendered["ux"]["visible_result"].lower()
    assert "silently" not in visible
    assert "expand" not in visible or "not expanded" in visible or "was not expanded" in visible
