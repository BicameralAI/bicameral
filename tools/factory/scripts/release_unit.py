#!/usr/bin/env python3
"""Validate PR-scoped release units and compute merge eligibility."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any


SHA = re.compile(r"^[a-f0-9]{40}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")
SEMVER = re.compile(r"^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:[-+][0-9A-Za-z.-]+)?$")
PROPOSAL_KEYS = {
    "schema_version", "release_unit_id", "label", "repository", "scope_ref",
    "release_unit_type", "target_version", "topology", "proposed_by",
    "proposed_at", "proposal_digest",
}
ASSIGNMENT_KEYS = {
    "schema_version", "assignment_id", "proposal_digest", "release_unit_id",
    "repository", "pull_request", "head_sha", "base_sha", "target_version",
    "release_unit_type", "topology", "decision", "approver", "approved_at",
    "github_environment", "github_deployment_id", "github_workflow_run_id",
    "receipt_digest",
}
RECEIPT_KEYS = {
    "schema_version", "release_unit_id", "assignment_id", "topology_type",
    "component_commits", "checklist_results", "outcome", "evidence_class",
    "run_url", "observed_at", "authorizes_merge", "receipt_digest",
}


def canonical_digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _timestamp(value: object) -> bool:
    if not isinstance(value, str):
        return False
    try:
        datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return False
    return value.endswith("Z") or "+" in value[10:]


def _closed(value: object, keys: set[str], location: str) -> list[str]:
    if not isinstance(value, dict):
        return [f"{location}: must be an object"]
    return [] if set(value) == keys else [f"{location}: keys must be exactly {sorted(keys)}"]


def validate_topology_requirement(value: object) -> list[str]:
    if not isinstance(value, dict):
        return ["proposal.topology: must be an object"]
    if value.get("required") is False:
        return [] if set(value) == {"required"} else ["proposal.topology: optional topology may only contain required=false"]
    if value.get("required") is not True:
        return ["proposal.topology.required: must be boolean"]
    if set(value) != {"required", "topology_type", "validation_checklist"}:
        return ["proposal.topology: required topology has invalid fields"]
    errors: list[str] = []
    if not isinstance(value.get("topology_type"), str) or not value["topology_type"]:
        errors.append("proposal.topology.topology_type: must be non-empty")
    checks = value.get("validation_checklist")
    if not isinstance(checks, list) or not checks:
        errors.append("proposal.topology.validation_checklist: must be non-empty")
        return errors
    ids: list[str] = []
    for index, check in enumerate(checks):
        if not isinstance(check, dict) or set(check) != {"check_id", "label"}:
            errors.append(f"proposal.topology.validation_checklist[{index}]: invalid fields")
            continue
        if not isinstance(check["check_id"], str) or not check["check_id"]:
            errors.append(f"proposal.topology.validation_checklist[{index}].check_id: must be non-empty")
        else:
            ids.append(check["check_id"])
        if not isinstance(check["label"], str) or len(check["label"]) < 3:
            errors.append(f"proposal.topology.validation_checklist[{index}].label: must be descriptive")
    if len(ids) != len(set(ids)):
        errors.append("proposal.topology.validation_checklist: check_id values must be unique")
    return errors


def validate_proposal(proposal: object) -> list[str]:
    errors = _closed(proposal, PROPOSAL_KEYS, "proposal")
    if errors:
        return errors
    assert isinstance(proposal, dict)
    if proposal["schema_version"] != 1:
        errors.append("proposal.schema_version: must be 1")
    for field in ("release_unit_id", "label", "repository", "scope_ref", "release_unit_type", "proposed_by"):
        if not isinstance(proposal[field], str) or not proposal[field]:
            errors.append(f"proposal.{field}: must be non-empty")
    if not isinstance(proposal["target_version"], str) or not SEMVER.fullmatch(proposal["target_version"]):
        errors.append("proposal.target_version: must be semantic version")
    if not _timestamp(proposal["proposed_at"]):
        errors.append("proposal.proposed_at: must be an offset-aware date-time")
    errors.extend(validate_topology_requirement(proposal["topology"]))
    unsigned = {key: value for key, value in proposal.items() if key != "proposal_digest"}
    if proposal["proposal_digest"] != canonical_digest(unsigned):
        errors.append("proposal.proposal_digest: does not bind the complete proposal")
    return errors


def validate_assignment(assignment: object, proposal: object) -> list[str]:
    errors = validate_proposal(proposal)
    errors.extend(_closed(assignment, ASSIGNMENT_KEYS, "assignment"))
    if errors:
        return errors
    assert isinstance(assignment, dict) and isinstance(proposal, dict)
    if assignment["schema_version"] != 1:
        errors.append("assignment.schema_version: must be 1")
    for field in ("proposal_digest", "release_unit_id", "repository", "target_version", "release_unit_type", "topology"):
        proposal_field = "proposal_digest" if field == "proposal_digest" else field
        if assignment[field] != proposal[proposal_field]:
            errors.append(f"assignment.{field}: does not match proposal")
    if assignment["decision"] != "assigned":
        errors.append("assignment.decision: must be assigned")
    if assignment["github_environment"] != "release-assignment":
        errors.append("assignment.github_environment: must be release-assignment")
    for field in ("pull_request", "github_deployment_id", "github_workflow_run_id"):
        if not isinstance(assignment[field], int) or isinstance(assignment[field], bool) or assignment[field] < 1:
            errors.append(f"assignment.{field}: must be a positive integer")
    for field in ("head_sha", "base_sha"):
        if not isinstance(assignment[field], str) or not SHA.fullmatch(assignment[field]):
            errors.append(f"assignment.{field}: must be a full lowercase SHA")
    if not isinstance(assignment["approver"], str) or not assignment["approver"]:
        errors.append("assignment.approver: must be non-empty")
    if not _timestamp(assignment["approved_at"]):
        errors.append("assignment.approved_at: must be an offset-aware date-time")
    identity = {
        key: assignment[key]
        for key in ("proposal_digest", "repository", "pull_request", "head_sha", "base_sha", "target_version")
    }
    if assignment["assignment_id"] != canonical_digest(identity):
        errors.append("assignment.assignment_id: does not bind exact assignment tuple")
    unsigned = {key: value for key, value in assignment.items() if key != "receipt_digest"}
    if assignment["receipt_digest"] != canonical_digest(unsigned):
        errors.append("assignment.receipt_digest: does not bind complete assignment")
    return errors


def validate_topology_receipt(receipt: object, proposal: object, assignment: object) -> list[str]:
    errors = validate_assignment(assignment, proposal)
    errors.extend(_closed(receipt, RECEIPT_KEYS, "receipt"))
    if errors:
        return errors
    assert isinstance(receipt, dict) and isinstance(proposal, dict) and isinstance(assignment, dict)
    requirement = proposal["topology"]
    if not isinstance(requirement, dict) or requirement.get("required") is not True:
        errors.append("receipt: topology receipt supplied for a unit without required topology")
        return errors
    if receipt["schema_version"] != 1:
        errors.append("receipt.schema_version: must be 1")
    if receipt["release_unit_id"] != proposal["release_unit_id"]:
        errors.append("receipt.release_unit_id: does not match proposal")
    if receipt["assignment_id"] != assignment["assignment_id"]:
        errors.append("receipt.assignment_id: does not match assignment")
    if receipt["topology_type"] != requirement["topology_type"]:
        errors.append("receipt.topology_type: does not match proposal")
    commits = receipt["component_commits"]
    if not isinstance(commits, dict) or not commits or any(not isinstance(v, str) or not SHA.fullmatch(v) for v in commits.values()):
        errors.append("receipt.component_commits: must bind one or more full lowercase SHAs")
    expected_ids = [check["check_id"] for check in requirement["validation_checklist"]]
    results = receipt["checklist_results"]
    if not isinstance(results, list):
        errors.append("receipt.checklist_results: must be a list")
        results = []
    actual_ids = [item.get("check_id") for item in results if isinstance(item, dict)]
    if actual_ids != expected_ids:
        errors.append("receipt.checklist_results: must exactly match proposed checklist order")
    for index, item in enumerate(results):
        if not isinstance(item, dict) or set(item) != {"check_id", "state", "evidence"}:
            errors.append(f"receipt.checklist_results[{index}]: invalid fields")
    evidence_class = receipt["evidence_class"]
    outcome = receipt["outcome"]
    authoritative = (
        evidence_class == "observed"
        and outcome == "passed"
        and bool(results)
        and all(isinstance(item, dict) and item.get("state") == "passed" for item in results)
    )
    if evidence_class not in {"observed", "simulated_reconstruction"}:
        errors.append("receipt.evidence_class: unsupported value")
    if evidence_class == "simulated_reconstruction" and outcome != "simulated":
        errors.append("receipt.outcome: simulated reconstruction must use simulated")
    if receipt["authorizes_merge"] is not authoritative:
        errors.append("receipt.authorizes_merge: must equal the observed passed result")
    if not _timestamp(receipt["observed_at"]):
        errors.append("receipt.observed_at: must be an offset-aware date-time")
    unsigned = {key: value for key, value in receipt.items() if key != "receipt_digest"}
    if receipt["receipt_digest"] != canonical_digest(unsigned):
        errors.append("receipt.receipt_digest: does not bind complete receipt")
    return errors


def merge_eligibility(proposal: object, assignment: object | None, receipt: object | None) -> dict[str, object]:
    proposal_errors = validate_proposal(proposal)
    if proposal_errors:
        return {"state": "invalid_proposal", "authorizes_merge": False, "errors": proposal_errors}
    if assignment is None:
        return {"state": "awaiting_assignment", "authorizes_merge": False, "errors": []}
    assignment_errors = validate_assignment(assignment, proposal)
    if assignment_errors:
        return {"state": "stale_assignment", "authorizes_merge": False, "errors": assignment_errors}
    assert isinstance(proposal, dict)
    topology = proposal["topology"]
    if isinstance(topology, dict) and topology.get("required") is False:
        return {"state": "merge_eligible", "authorizes_merge": True, "errors": []}
    if receipt is None:
        return {"state": "awaiting_topology", "authorizes_merge": False, "errors": []}
    receipt_errors = validate_topology_receipt(receipt, proposal, assignment)
    if receipt_errors:
        return {"state": "topology_not_authoritative", "authorizes_merge": False, "errors": receipt_errors}
    assert isinstance(receipt, dict)
    return {
        "state": "merge_eligible" if receipt["authorizes_merge"] else "topology_not_authoritative",
        "authorizes_merge": bool(receipt["authorizes_merge"]),
        "errors": [],
    }


def _load(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    validate = sub.add_parser("validate-proposal")
    validate.add_argument("proposal", type=Path)
    eligibility = sub.add_parser("merge-eligibility")
    eligibility.add_argument("--proposal", required=True, type=Path)
    eligibility.add_argument("--assignment", type=Path)
    eligibility.add_argument("--topology-receipt", type=Path)
    args = parser.parse_args()
    if args.command == "validate-proposal":
        errors = validate_proposal(_load(args.proposal))
        result = {"valid": not errors, "errors": errors}
    else:
        result = merge_eligibility(
            _load(args.proposal),
            _load(args.assignment) if args.assignment else None,
            _load(args.topology_receipt) if args.topology_receipt else None,
        )
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if not result.get("errors") and (args.command == "validate-proposal" or result.get("authorizes_merge")) else 1


if __name__ == "__main__":
    raise SystemExit(main())
