"""Default auto-promote / not-track selection policy for Managed MCP UX."""

from __future__ import annotations

import pytest

from candidate_selection_surface import (
    SELECTION_MODE_ACCEPT_DEFAULTS,
    SELECTION_MODE_NOT_TRACK,
    SurfaceContractError,
    build_prepare_params,
    compute_default_selection,
    compute_not_track_selection,
    render_candidate_set,
)

A = "11111111-1111-1111-1111-111111111111"
B = "22222222-2222-2222-2222-222222222222"
C = "33333333-3333-3333-3333-333333333333"
SET_ID = "00000000-0000-0000-0000-000000000101"
DIGEST = "sha256:" + "a" * 64


def _cands():
    return [
        {
            "candidate_id": A,
            "proposed_decision": "Bounded retries",
            "contradiction_refs": [],
            "source_spans": [{"artifact_path": "docs/a.md", "start_byte": 0, "end_byte": 1}],
            "dependency_candidate_ids": [],
            "disposition": "pending",
        },
        {
            "candidate_id": B,
            "proposed_decision": "Timeouts as limitations",
            "contradiction_refs": [],
            "source_spans": [{"artifact_path": "docs/a.md", "start_byte": 2, "end_byte": 3}],
            "dependency_candidate_ids": [A],
            "disposition": "pending",
        },
        {
            "candidate_id": C,
            "proposed_decision": "Contradictory claim",
            "contradiction_refs": ["claim:conflict-1"],
            "source_spans": [{"artifact_path": "docs/a.md", "start_byte": 4, "end_byte": 5}],
            "dependency_candidate_ids": [],
            "disposition": "pending",
        },
    ]


def _set():
    return {
        "contract_version": 1,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 1,
        "candidate_set_digest": DIGEST,
        "binding": {"product_id": "p1", "host_session_id": "h1"},
        "session_lease_id": "lease-1",
        "candidates": _cands(),
    }


def _base_ids(**extra):
    return {
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 1,
        "candidate_set_digest": DIGEST,
        "product_id": "p1",
        "host_session_id": "h1",
        "session_lease_id": "lease-1",
        "actor_id": "human@example.com",
        "selection_operation_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        **extra,
    }


def test_default_promotes_non_contradiction_only():
    resolved = compute_default_selection(_cands())
    assert set(resolved["promote_candidate_ids"]) == {A, B}
    assert resolved["reject_candidate_ids"] == []
    assert resolved["held_for_contradiction_ids"] == [C]


def test_default_honors_explicit_reject():
    resolved = compute_default_selection(_cands(), explicitly_rejected_ids=[B])
    assert set(resolved["promote_candidate_ids"]) == {A}
    assert resolved["reject_candidate_ids"] == [B]
    assert resolved["held_for_contradiction_ids"] == [C]


def test_not_track_rejects_all():
    resolved = compute_not_track_selection(_cands())
    assert resolved["promote_candidate_ids"] == []
    assert set(resolved["reject_candidate_ids"]) == {A, B, C}


def test_render_includes_default_selection_and_options():
    rendered = render_candidate_set(_set())
    ds = rendered["presentation"]["default_selection"]
    assert set(ds["promote_candidate_ids"]) == {A, B}
    assert ds["held_for_contradiction_ids"] == [C]
    assert "accept_defaults" in rendered["presentation"]["selection_options"]
    assert "not_track" in rendered["presentation"]["selection_options"]
    by_id = {c["candidate_id"]: c for c in rendered["presentation"]["candidates"]}
    assert by_id[A]["default_disposition"] == "auto_promote"
    assert by_id[C]["default_disposition"] == "hold_contradiction"
    assert rendered["ux"]["auto_promote_count"] == 2
    assert rendered["ux"]["held_contradiction_count"] == 1


def test_prepare_accept_defaults():
    params = build_prepare_params(
        _base_ids(
            selection_mode=SELECTION_MODE_ACCEPT_DEFAULTS,
            candidates=_cands(),
            reject_candidate_ids=[B],
        )
    )
    assert set(params["promote_candidate_ids"]) == {A}
    assert params["reject_candidate_ids"] == [B]
    assert C not in params["promote_candidate_ids"]


def test_prepare_accept_defaults_also_promote_contradiction():
    params = build_prepare_params(
        _base_ids(
            selection_mode=SELECTION_MODE_ACCEPT_DEFAULTS,
            candidates=_cands(),
            also_promote_candidate_ids=[C],
        )
    )
    assert set(params["promote_candidate_ids"]) == {A, B, C}


def test_prepare_not_track():
    params = build_prepare_params(
        _base_ids(selection_mode=SELECTION_MODE_NOT_TRACK, candidates=_cands())
    )
    assert params["promote_candidate_ids"] == []
    assert set(params["reject_candidate_ids"]) == {A, B, C}


def test_prepare_accept_defaults_requires_candidates():
    with pytest.raises(SurfaceContractError) as exc:
        build_prepare_params(_base_ids(selection_mode=SELECTION_MODE_ACCEPT_DEFAULTS))
    assert exc.value.code == "candidates_required_for_selection_mode"
