"""Regression lock for daemon-authored proof fields on preflight fallback.

PR #797 preserves packet-authored RecallPacket content, but its original
fallback path projected ``relevant_candidates`` through ``_render_review_item``.
That fixed allowlist silently discarded proof fields while returning a
syntactically valid candidate. The MCP Tool Surface must preserve daemon-owned
candidate structures and must not recompute or narrow their evidence contract.
"""

from __future__ import annotations

import json
from typing import Any

from responses import format_preflight_response


def test_fallback_preserves_complete_daemon_candidate_proof_structure() -> None:
    candidate: dict[str, Any] = {
        "candidate_id": "cand-fallback-797",
        "title": "Preserve exact proof-bearing candidate",
        "reason": "daemon-authored fallback candidate",
        "graph_snapshot_id": "snapshot-fallback-797",
        "validated_sha": "7977977977977977977977977977977977977977",
        "scope_fingerprint": "sha256:scope-fallback-797",
        "scope_sufficiency_proof": {
            "proof_id": "proof-fallback-797",
            "state": "sufficient",
        },
        "evidence_state": "verified",
        "readiness_warnings": ["daemon warning must survive rendering"],
        "opaque_future_proof_field": {
            "version": 2,
            "value": "must survive without an MCP allowlist update",
        },
    }
    packet_attention = {
        "kind": "decision_attention",
        "pressure": "packet-authored",
    }
    response: dict[str, Any] = {
        "request_id": "req-fallback-797",
        "status": "ok",
        "staged": {
            "capture": {"status": "active"},
            "projection": {"status": "active"},
            "lookup": {"status": "completed"},
            "enforcement": {"status": "not_configured"},
            "session_directive": {"mode": "continue"},
        },
        # The packet intentionally omits decision_candidates. The daemon's
        # compatibility/fallback collection carries the complete candidate.
        "relevant_candidates": [candidate],
        "alpha_recall": {
            "kind": "alpha_recall.product.packet",
            "product": {
                "decision_attention": {
                    "kind": "decision_attention",
                    "pressure": "wrapper-fallback-must-not-win",
                },
                "packet": {
                    "packet_id": "pkt-fallback-797",
                    "decision_attention": packet_attention,
                },
            },
        },
    }

    output = json.loads(format_preflight_response(response).text)

    assert output["alpha_recall"]["decision_candidates"] == [candidate]
    assert output["alpha_recall"]["decision_attention"] == packet_attention
    assert output["alpha_recall"]["operator_question"] == (
        "Is there anything here you ought to decide?"
    )
