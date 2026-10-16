#!/usr/bin/env python3
"""Validate a frozen, exact, non-publishing release rehearsal contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


SHA_PATTERN = re.compile(r"^[a-f0-9]{40}$")
DIGEST_PATTERN = re.compile(r"^sha256:[a-f0-9]{64}$")
PRODUCT_COMPONENTS = ("bot", "cloud", "mcp", "integrations")
PUBLICATION_FIELDS = (
    "git_tag",
    "github_release",
    "production_r2_latest",
    "cloud_deploy",
)
CONTRACT_FIELDS = {
    "schema_version",
    "rehearsal_id",
    "release_plan_id",
    "release_plan_digest",
    "version",
    "component_commits",
    "evaluated_under",
    "required_closures",
    "artifact_destination",
    "publication",
    "created_by",
    "created_at",
}


def canonical_digest(payload: dict[str, Any]) -> str:
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _load_object(path: Path, label: str) -> tuple[dict[str, Any] | None, list[str]]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return None, [f"{label} is unreadable: {exc}"]
    if not isinstance(payload, dict):
        return None, [f"{label} must be a JSON object"]
    return payload, []


def validate(
    contract: dict[str, Any],
    release_plan: dict[str, Any],
    closure_policy: dict[str, Any],
) -> list[str]:
    errors: list[str] = []

    unknown_fields = sorted(set(contract) - CONTRACT_FIELDS)
    if unknown_fields:
        errors.append(
            "unknown rehearsal contract fields: " + ", ".join(unknown_fields)
        )
    if contract.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if release_plan.get("schema_version") != 1:
        errors.append("release plan schema_version must be 1")
    if release_plan.get("status") != "frozen":
        errors.append("release plan must be frozen before rehearsal")
    if release_plan.get("mode") != "enforced":
        errors.append("release plan must be enforced before rehearsal")
    if release_plan.get("repository") != "BicameralAI/bicameral-bot":
        errors.append("release plan repository must be BicameralAI/bicameral-bot")
    if closure_policy.get("repository") != release_plan.get("repository"):
        errors.append("closure policy repository does not match the release plan")

    policy_digest = canonical_digest(closure_policy)
    if release_plan.get("closure_policy_digest") != policy_digest:
        errors.append("release plan closure_policy_digest does not match the closure policy")

    if contract.get("release_plan_id") != release_plan.get("plan_id"):
        errors.append("release_plan_id does not match the release plan")
    expected_plan_digest = canonical_digest(release_plan)
    if contract.get("release_plan_digest") != expected_plan_digest:
        errors.append("release_plan_digest does not match the release plan")
    if contract.get("version") != release_plan.get("version"):
        errors.append("rehearsal version does not match the release plan")

    commits = contract.get("component_commits")
    if not isinstance(commits, dict) or set(commits) != set(PRODUCT_COMPONENTS):
        errors.append(
            "component_commits must contain exactly bot, cloud, mcp, integrations"
        )
    else:
        for component in PRODUCT_COMPONENTS:
            if not isinstance(commits[component], str) or not SHA_PATTERN.fullmatch(
                commits[component]
            ):
                errors.append(f"component_commits.{component} must be an exact commit SHA")

    evaluated_under = contract.get("evaluated_under")
    if not isinstance(evaluated_under, dict) or set(evaluated_under) != {
        "factory_commit",
        "closure_policy_digest",
    }:
        errors.append(
            "evaluated_under must contain exactly factory_commit and closure_policy_digest"
        )
    else:
        factory_commit = evaluated_under["factory_commit"]
        if not isinstance(factory_commit, str) or not SHA_PATTERN.fullmatch(factory_commit):
            errors.append("evaluated_under.factory_commit must be an exact commit SHA")
        if evaluated_under["closure_policy_digest"] != policy_digest:
            errors.append(
                "evaluated_under.closure_policy_digest does not match the closure policy"
            )

    closures = contract.get("required_closures")
    if (
        not isinstance(closures, list)
        or not closures
        or any(not isinstance(item, str) or not item for item in closures)
        or len(closures) != len(set(closures))
    ):
        errors.append("required_closures must be a non-empty unique string array")
    else:
        units = release_plan.get("release_units")
        policy_units = closure_policy.get("release_units")
        if not isinstance(units, list) or not isinstance(policy_units, dict):
            errors.append("release plan and closure policy must declare release_units")
        else:
            selected: set[str] = set()
            visiting: set[str] = set()

            def visit(unit: str) -> None:
                if unit in selected:
                    return
                definition = policy_units.get(unit)
                if not isinstance(definition, dict):
                    errors.append(f"release plan selects unknown release unit {unit}")
                    return
                if unit in visiting:
                    errors.append(f"closure policy release-unit dependency cycle at {unit}")
                    return
                visiting.add(unit)
                dependencies = definition.get("requires_release_units")
                if not isinstance(dependencies, list):
                    errors.append(f"release unit {unit} has invalid dependencies")
                else:
                    for dependency in dependencies:
                        if isinstance(dependency, str):
                            visit(dependency)
                        else:
                            errors.append(f"release unit {unit} has invalid dependency")
                visiting.remove(unit)
                selected.add(unit)

            for unit in units:
                if isinstance(unit, str):
                    visit(unit)
                else:
                    errors.append("release plan release_units must contain strings")

            expected: set[str] = set()
            for unit in selected:
                definition = policy_units.get(unit)
                if not isinstance(definition, dict):
                    continue
                unit_closures = definition.get("required_closures")
                if not isinstance(unit_closures, list):
                    errors.append(f"release unit {unit} has invalid required_closures")
                    continue
                expected.update(
                    closure for closure in unit_closures if isinstance(closure, str)
                )
            if set(closures) != expected:
                errors.append(
                    "required_closures do not match the exact release-unit projection"
                )

    destination = contract.get("artifact_destination")
    if not isinstance(destination, dict) or destination.get("kind") not in {
        "github_actions_artifact",
        "quarantined_object_prefix",
    }:
        errors.append("artifact_destination must be non-production rehearsal storage")

    publication = contract.get("publication")
    if not isinstance(publication, dict) or set(publication) != set(PUBLICATION_FIELDS):
        errors.append("publication must declare every prohibited production action")
    else:
        for field in PUBLICATION_FIELDS:
            if publication[field] is not False:
                errors.append(f"publication.{field} must be false for a rehearsal")

    for field in ("rehearsal_id", "created_by", "created_at"):
        if not isinstance(contract.get(field), str) or not contract[field]:
            errors.append(f"{field} must be a non-empty string")

    plan_digest = release_plan.get("closure_policy_digest")
    if not isinstance(plan_digest, str) or not DIGEST_PATTERN.fullmatch(plan_digest):
        errors.append("release plan closure_policy_digest must be a sha256 digest")

    return errors


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("contract", type=Path)
    validate_parser.add_argument("--release-plan", required=True, type=Path)
    validate_parser.add_argument("--closure-policy", required=True, type=Path)
    return parser


def main() -> int:
    args = _build_parser().parse_args()
    contract, errors = _load_object(args.contract, "rehearsal contract")
    release_plan, plan_errors = _load_object(args.release_plan, "release plan")
    closure_policy, policy_errors = _load_object(args.closure_policy, "closure policy")
    errors.extend(plan_errors)
    errors.extend(policy_errors)
    if contract is not None and release_plan is not None and closure_policy is not None:
        errors.extend(validate(contract, release_plan, closure_policy))
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    assert contract is not None
    print(
        json.dumps(
            {
                "state": "REHEARSAL_READY",
                "rehearsal_id": contract["rehearsal_id"],
                "version": contract["version"],
                "authorizes_publish": False,
                "contract_digest": canonical_digest(contract),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
