"""Contract tests for PR-scoped Release Units."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("release_unit.py")
SPEC = importlib.util.spec_from_file_location("release_unit", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def proposal(required: bool = True, unit_id: str = "desktop-hosted-key-pairing") -> dict:
    topology = (
        {
            "required": True,
            "topology_type": "desktop_hosted_key_pairing_v1",
            "validation_checklist": [
                {"check_id": "pair", "label": "Pair the exact desktop and hosted revisions"},
                {"check_id": "reconnect", "label": "Reconnect after daemon restart"},
            ],
        }
        if required
        else {"required": False}
    )
    value = {
        "schema_version": 1,
        "release_unit_id": unit_id,
        "label": "Desktop to hosted key pairing",
        "repository": "BicameralAI/bicameral-bot",
        "scope_ref": "PR #999",
        "release_unit_type": "hosted_dashboard",
        "target_version": "0.1.12",
        "topology": topology,
        "proposed_by": "collaborator:kevin",
        "proposed_at": "2026-07-22T08:00:00Z",
    }
    value["proposal_digest"] = MODULE.canonical_digest(value)
    return value


def assignment(value: dict) -> dict:
    identity = {
        "proposal_digest": value["proposal_digest"],
        "repository": value["repository"],
        "pull_request": 999,
        "head_sha": "a" * 40,
        "base_sha": "b" * 40,
        "target_version": value["target_version"],
    }
    result = {
        "schema_version": 1,
        "assignment_id": MODULE.canonical_digest(identity),
        **identity,
        "release_unit_id": value["release_unit_id"],
        "release_unit_type": value["release_unit_type"],
        "topology": value["topology"],
        "decision": "assigned",
        "approver": "delivery-manager:jinhongkuan",
        "approved_at": "2026-07-22T08:10:00Z",
        "github_environment": "release-assignment",
        "github_deployment_id": 101,
        "github_workflow_run_id": 202,
    }
    result["receipt_digest"] = MODULE.canonical_digest(result)
    return result


def receipt(value: dict, approved: dict, simulated: bool = False) -> dict:
    state = "simulated" if simulated else "passed"
    result = {
        "schema_version": 1,
        "release_unit_id": value["release_unit_id"],
        "assignment_id": approved["assignment_id"],
        "topology_type": value["topology"]["topology_type"],
        "component_commits": {"bot": "a" * 40, "cloud": "c" * 40},
        "checklist_results": [
            {"check_id": check["check_id"], "state": state, "evidence": "https://example.test/run/1"}
            for check in value["topology"]["validation_checklist"]
        ],
        "outcome": "simulated" if simulated else "passed",
        "evidence_class": "simulated_reconstruction" if simulated else "observed",
        "run_url": "https://example.test/run/1",
        "observed_at": "2026-07-22T08:20:00Z",
        "authorizes_merge": not simulated,
    }
    result["receipt_digest"] = MODULE.canonical_digest(result)
    return result


class ReleaseUnitTests(unittest.TestCase):
    def test_optional_topology_becomes_eligible_after_assignment(self) -> None:
        value = proposal(required=False)
        result = MODULE.merge_eligibility(value, assignment(value), None)
        self.assertEqual("merge_eligible", result["state"])
        self.assertTrue(result["authorizes_merge"])

    def test_required_topology_waits_after_assignment(self) -> None:
        value = proposal()
        result = MODULE.merge_eligibility(value, assignment(value), None)
        self.assertEqual("awaiting_topology", result["state"])
        self.assertFalse(result["authorizes_merge"])

    def test_observed_exact_checklist_authorizes_merge(self) -> None:
        value = proposal()
        approved = assignment(value)
        result = MODULE.merge_eligibility(value, approved, receipt(value, approved))
        self.assertEqual("merge_eligible", result["state"])
        self.assertTrue(result["authorizes_merge"])

    def test_simulated_reconstruction_never_authorizes_merge(self) -> None:
        value = proposal()
        approved = assignment(value)
        result = MODULE.merge_eligibility(value, approved, receipt(value, approved, simulated=True))
        self.assertEqual("topology_not_authoritative", result["state"])
        self.assertFalse(result["authorizes_merge"])

    def test_push_makes_assignment_stale(self) -> None:
        value = proposal()
        approved = assignment(value)
        approved["head_sha"] = "f" * 40
        approved["receipt_digest"] = MODULE.canonical_digest(
            {key: item for key, item in approved.items() if key != "receipt_digest"}
        )
        result = MODULE.merge_eligibility(value, approved, None)
        self.assertEqual("stale_assignment", result["state"])

    def test_same_topology_type_isolated_by_release_unit_assignment(self) -> None:
        first = proposal(unit_id="pairing-first")
        second = proposal(unit_id="pairing-second")
        first_assignment = assignment(first)
        second_assignment = assignment(second)
        wrong_receipt = receipt(first, first_assignment)
        errors = MODULE.validate_topology_receipt(wrong_receipt, second, second_assignment)
        self.assertTrue(any("release_unit_id" in error or "assignment_id" in error for error in errors))

    def test_checklist_drift_fails_closed(self) -> None:
        value = proposal()
        approved = assignment(value)
        evidence = receipt(value, approved)
        evidence["checklist_results"].reverse()
        evidence["receipt_digest"] = MODULE.canonical_digest(
            {key: item for key, item in evidence.items() if key != "receipt_digest"}
        )
        errors = MODULE.validate_topology_receipt(evidence, value, approved)
        self.assertTrue(any("checklist_results" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
