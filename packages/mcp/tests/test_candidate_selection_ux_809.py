"""Permanent UX contract tests for MCP #809 (CandidateSet confirmation amend)."""

from __future__ import annotations

from candidate_selection_surface import (
    SESSION_ONLY_REJECT_DETAIL,
    compute_default_selection,
    compute_not_track_selection,
    render_candidate_set,
    render_prepare_response,
)

CANDIDATE_A = "00000000-0000-0000-0000-00000000000a"
CANDIDATE_B = "00000000-0000-0000-0000-00000000000b"
CANDIDATE_C = "00000000-0000-0000-0000-00000000000c"
SET_ID = "00000000-0000-0000-0000-000000000100"
DIGEST = "sha256:" + "b" * 64
CHALLENGE_SECRET = "opaque-challenge-must-not-render"


def _candidates() -> list[dict]:
    return [
        {
            "candidate_id": CANDIDATE_A,
            "proposed_decision": "Use bounded retries.",
            "source_spans": [{"artifact_path": "docs/spec.md", "start_byte": 0, "end_byte": 12}],
            "disposition": "pending",
            "dependency_candidate_ids": [],
            "contradiction_refs": [],
        },
        {
            "candidate_id": CANDIDATE_B,
            "proposed_decision": "Cap attempts at eight.",
            "source_spans": [{"artifact_path": "docs/ops.md", "start_byte": 4, "end_byte": 20}],
            "disposition": "pending",
            "dependency_candidate_ids": [CANDIDATE_A],
            "contradiction_refs": [],
        },
        {
            "candidate_id": CANDIDATE_C,
            "proposed_decision": "Contradictory timeout claim.",
            "source_spans": [{"artifact_path": "docs/ops.md", "start_byte": 30, "end_byte": 40}],
            "disposition": "pending",
            "dependency_candidate_ids": [],
            "contradiction_refs": ["claim:conflict-1"],
        },
    ]


def _candidate_set() -> dict:
    return {
        "contract_version": 1,
        "candidate_set_id": SET_ID,
        "candidate_set_generation": 1,
        "candidate_set_digest": DIGEST,
        "binding": {"product_id": "product-alpha", "host_session_id": "host-1"},
        "session_lease_id": "lease-1",
        "candidates": _candidates(),
    }


def test_accept_defaults_and_not_track_are_count_bearing_not_ambiguous():
    resolved = compute_default_selection(_candidates())
    accept = resolved["options"]["accept_defaults"]
    not_track = resolved["options"]["not_track"]

    assert accept["label"] == "Promote all 2 non-contradictory candidates"
    assert not_track["label"] == "Reject all 3 candidates for this session"
    assert accept["label"] != "Accept defaults"
    assert not_track["label"] != "Don't track"
    assert "Accept defaults" not in accept["label"]
    assert "Don't track" not in not_track["label"]

    assert resolved["promote_count"] == 2
    assert resolved["reject_count"] == 0
    assert resolved["held_contradiction_count"] == 1
    assert resolved["remaining_pending_count"] == 1
    assert accept["promote_count"] == 2
    assert accept["held_contradiction_count"] == 1
    assert not_track["session_only"] is True
    assert not_track["creates_canonical_event"] is False
    assert not_track["creates_durable_suppression"] is False
    assert SESSION_ONLY_REJECT_DETAIL in not_track["detail"]


def test_session_only_reject_all_semantics_on_not_track_selection():
    resolved = compute_not_track_selection(_candidates())
    assert resolved["promote_count"] == 0
    assert resolved["reject_count"] == 3
    assert resolved["session_only"] is True
    assert resolved["creates_canonical_event"] is False
    assert resolved["creates_durable_suppression"] is False
    assert "session" in resolved["label"].lower()
    assert "durable" in resolved["detail"].lower() or "suppression" in resolved["detail"].lower()


def test_candidate_rows_lead_with_decision_text_before_uuid():
    rendered = render_candidate_set(_candidate_set())
    row = rendered["presentation"]["candidates"][0]
    assert row["label"].startswith("Use bounded retries.")
    assert row["label"].index("Use bounded retries.") < row["label"].find(CANDIDATE_A) or (
        CANDIDATE_A not in row["label"]
    )
    # Stable ID remains authoritative transport field.
    assert row["candidate_id"] == CANDIDATE_A
    assert "docs/spec.md" in row["label"] or "docs/spec.md" in " ".join(row["source_context"])

    annotation = rendered["presentation"]["annotations"][0]
    assert annotation["label"].startswith("Use bounded retries.")
    assert annotation["candidate_id"] == CANDIDATE_A
    assert CANDIDATE_A not in annotation["label"].split("—")[0]

    options = rendered["presentation"]["selection_options"]
    assert options["accept_defaults"]["label"].startswith("Promote all")
    assert options["not_track"]["label"].startswith("Reject all")
    assert "Accept defaults" not in options["accept_defaults"]["label"]
    assert "Don't track" not in options["not_track"]["label"]

    assert rendered["ux"]["primary_action_labels"]["accept_defaults"].startswith("Promote all")
    assert rendered["presentation"]["product_id"] == "product-alpha"


def test_confirmation_summary_count_signoff_product_actor_canonical_no_challenge():
    preview = {
        "contract_version": 1,
        "selection_operation_id": "00000000-0000-0000-0000-000000000200",
        "selection_digest": DIGEST,
        "product_id": "product-alpha",
        "actor_id": "owner@example.com",
        "pending_candidate_count": 1,
        "resulting_signoff": "proposed",
        "promote_effects": [
            {
                "candidate_id": CANDIDATE_A,
                "proposed_decision": "Use bounded retries.",
                "source_spans": [
                    {"artifact_path": "docs/spec.md", "start_byte": 0, "end_byte": 12}
                ],
            },
            {
                "candidate_id": CANDIDATE_B,
                "proposed_decision": "Cap attempts at eight.",
            },
        ],
        "reject_effects": [
            {
                "candidate_id": CANDIDATE_C,
                "proposed_decision": "Contradictory timeout claim.",
            }
        ],
        "confirmation_challenge": {
            "challenge_id": "ch-1",
            "secret": CHALLENGE_SECRET,
            "expires_at": "2026-08-07T00:00:00Z",
        },
    }
    rendered = render_prepare_response({"candidate_selection_preview": preview})

    assert rendered["human_action"]["label"] == "Promote 2 selected candidates"
    summary = rendered["confirmation_summary"]
    assert summary["promote_count"] == 2
    assert summary["reject_count"] == 1
    assert summary["remaining_pending_count"] == 1
    assert summary["product_id"] == "product-alpha"
    assert summary["actor_id"] == "owner@example.com"
    assert summary["signoff_wording"] == "proposed"
    assert summary["resulting_signoff"] == "proposed"
    assert summary["will_append_canonical_events"] is True
    assert "canonical" in summary["canonical_append_statement"].lower()
    assert summary["challenge_in_human_summary"] is False
    assert CHALLENGE_SECRET not in rendered["human_action"]["summary"]
    assert CHALLENGE_SECRET not in str(summary)
    assert "secret" not in rendered["human_action"]["summary"].lower()

    # Decision text precedes UUID in promote item human labels.
    promote0 = summary["promote_items"][0]
    assert promote0["label"].startswith("Use bounded retries.")
    assert promote0["candidate_id"] == CANDIDATE_A

    # Session-only reject semantics present when rejects exist.
    assert summary["session_only_reject_semantics"] is not None
    assert "session" in summary["session_only_reject_semantics"].lower()

    # Transport still carries opaque challenge separately.
    assert rendered["confirmation_transport"]["challenge"]["secret"] == CHALLENGE_SECRET
    assert rendered["confirmation_transport"]["opaque"] is True
    assert rendered["confirmation_transport"]["persist"] is False
    assert rendered["confirmation_transport"]["log"] is False


def test_owner_policy_approve_wording_is_unmistakable():
    preview = {
        "contract_version": 1,
        "selection_operation_id": "00000000-0000-0000-0000-000000000201",
        "selection_digest": DIGEST,
        "product_id": "product-alpha",
        "actor_id": "owner@example.com",
        "pending_candidate_count": 0,
        "resulting_signoff": "approved_by_owner_policy",
        "promote_effects": [
            {
                "candidate_id": CANDIDATE_A,
                "proposed_decision": "Use bounded retries.",
            }
        ],
        "reject_effects": [],
        "confirmation_challenge": {
            "challenge_id": "ch-2",
            "secret": CHALLENGE_SECRET,
        },
    }
    rendered = render_prepare_response({"candidate_selection_preview": preview})
    assert (
        rendered["human_action"]["label"]
        == "Promote and approve 1 selected candidate as Product Owner"
    )
    assert rendered["confirmation_summary"]["signoff_wording"] == "owner-policy-approved"
    assert "Product Owner" in rendered["human_action"]["label"]
    assert rendered["ux"]["signoff_wording"] == "owner-policy-approved"


def test_no_vacuous_or_true_in_legacy_ux_suite():
    """Guard: #785 UX suite must not reintroduce vacuous ``assert x or True``."""

    import pathlib
    import re

    path = pathlib.Path(__file__).with_name("test_candidate_selection_ux_785.py")
    text = path.read_text(encoding="utf-8")
    # Match only vacuous assert patterns, not prose.
    assert re.search(r"assert\s+.+\s+or\s+True\b", text) is None
