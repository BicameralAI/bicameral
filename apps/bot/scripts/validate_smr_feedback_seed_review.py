#!/usr/bin/env python3
"""Validate the non-authoritative #1221 GovernedDomain fixture."""

from __future__ import annotations

import copy
import importlib.util
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = (
    ROOT
    / "protocol"
    / "smr-feedback-v0.1"
    / "fixtures"
    / "product-creation-workspace-routing.governed-domain.fixture.json"
)
CONTRACT_VALIDATOR = ROOT / "scripts" / "validate_smr_claim_acceptance_contract.py"
PROPOSAL_SCHEMA = (
    ROOT
    / "protocol"
    / "smr-claim-acceptance"
    / "schemas"
    / "smr-claim-set-proposal.schema.json"
)
DIGEST_VECTORS = (
    ROOT / "protocol" / "smr-claim-acceptance" / "fixtures" / "digest-vectors.json"
)

EXPECTED_ANALYSIS_KEY = "fixture.smr-feedback.product-creation-workspace-routing-v1"
EXPECTED_HOLDS = {
    "runtime GovernedDomain schema and validation",
    "governed_domain_admitted event type and replay consumer",
    "owner review of the exact domain-shaped fixture and resulting digests",
}
FORBIDDEN_AUTHORITY_KEYS = {
    "accepted_baseline",
    "accepted_event_id",
    "canonical_events",
    "event_stream_id",
    "graph",
    "graph_json",
    "projection_digest",
}
FORBIDDEN_LEGACY_KEYS = {
    "candidate_drafts",
    "decision_candidates",
    "claim_set_candidate_promoted",
    "governed_domain_admitted",
}

EXPECTED_STATEMENTS = [
    "Canonical creation requires explicit supported provider/resource selection, operator-level connection, descriptor validation, and adapter resolution.",
    "Missing provider access uses the shared Connection service and resumes the same retained draft; failure leaves a retryable draft and creates no Product.",
    "The selected event-store adapter owns canonical Product events. Cloud publication is a separate non-canonical projection.",
    "Cloud publication is forbidden until the provider confirms the exact first event through a bounded commit proof; the proof is a witness, not lifecycle state.",
    "Connection, canonical creation, provider confirmation, Cloud publication, local registration, and replay are independent observable commit points.",
    "Pre-registration incompleteness is `needs_setup`; post-registration access/replay failure is `repair_required`; daemon loss routes to pairing/reconnect, not an `offline` Product row.",
    "Partial-failure recovery is idempotent and cannot duplicate Product, first event, proof, registration, or Cloud row.",
    "A runnable Product requires explicit hosted-assisted or local-only registration; cwd/upward `.bicameral/` discovery is forbidden.",
    "`LocalWorkspaceBinding` is Product-keyed local code access, not Product identity, source binding, BindingEvidence, canonical state, or approval.",
    "A missing or invalid local path makes only the binding `repair_required`; Product registration remains valid.",
    "On genuine SessionStart, the host workspace path is only a routing hint. The daemon may return `product_id` only from exactly one valid existing binding. An `unbound`, `repair_required`, `ambiguous`, or `product_context_required` outcome fails closed with a typed no-run result; the host never infers or chooses a Product.",
    "SessionStart activation and Product/workspace readiness remain separate. Lookup and hook receipts create or approve no Product, binding, Decision, or release state.",
]


def _contract_module() -> Any:
    spec = importlib.util.spec_from_file_location("smr_contract_validator", CONTRACT_VALIDATOR)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _unique_ids(items: list[dict[str, Any]], field: str, errors: list[str]) -> set[str]:
    values = [item[field] for item in items]
    if values != sorted(values):
        errors.append(f"normalized_claim_set.{field}: collection must be stable-ID sorted")
    if len(values) != len(set(values)):
        errors.append(f"normalized_claim_set.{field}: duplicate identities")
    return set(values)


def _all_keys(value: Any) -> set[str]:
    if isinstance(value, dict):
        return set(value).union(*(_all_keys(item) for item in value.values()), set())
    if isinstance(value, list):
        return set().union(*(_all_keys(item) for item in value), set())
    return set()


def replay_throwaway_fixture(package: dict[str, Any]) -> dict[str, Any]:
    """Return an isolated board projection without writing Product state."""
    domain = package["governed_domain"]
    replay = package["replay"]
    if domain["is_source"] is not False:
        raise ValueError("only an is_source=false domain can seed this fixture board")
    if replay != {
        "mode": "throwaway_fixture",
        "base_event_position": 0,
        "writes_product_state": False,
        "canonical_event_kind": None,
    }:
        raise ValueError("fixture replay must remain read-only and non-canonical")
    return copy.deepcopy(domain["normalized_claim_set"])


def validate_review_package(package: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    contract = _contract_module()

    if package.get("fixture_version") != 1:
        errors.append("fixture_version must be 1")
    if package.get("status") != "owner_review_required":
        errors.append("fixture must remain owner_review_required before benchmark use")
    if package.get("fixture_role") != "non_authoritative_throwaway_replay":
        errors.append("fixture role must remain non-authoritative throwaway replay")

    keys = _all_keys(package)
    forbidden = FORBIDDEN_AUTHORITY_KEYS & keys
    if forbidden:
        errors.append(f"fixture overclaims unavailable canonical artifacts: {sorted(forbidden)}")
    legacy = FORBIDDEN_LEGACY_KEYS & keys
    if legacy:
        errors.append(f"fixture uses a superseded candidate or event envelope: {sorted(legacy)}")

    domain = package.get("governed_domain")
    if not isinstance(domain, dict):
        return errors + ["governed_domain is required"]
    if domain.get("analysis_key") != EXPECTED_ANALYSIS_KEY:
        errors.append("analysis_key must keep its explicit fixture identity")
    if domain.get("is_source") is not False:
        errors.append("board seed must be one is_source=false GovernedDomain")
    if domain.get("artifact_role") != "provenance_only_not_product_truth":
        errors.append("markdown may be provenance only; it must not become the board")

    claim_set = domain.get("normalized_claim_set")
    if not isinstance(claim_set, dict):
        return errors + ["governed_domain.normalized_claim_set is required"]
    schema = contract.load_json(PROPOSAL_SCHEMA)
    contract.validate(
        claim_set,
        {"$ref": "#/definitions/NormalizedClaimSetV1"},
        {"__self__": schema},
        "normalized_claim_set",
        errors,
    )

    identity = package.get("identity", {})
    expected_digest = contract.smr_digest("claim-set", claim_set)
    if identity.get("claim_set_digest") != expected_digest:
        errors.append(f"claim_set_digest must reproduce as {expected_digest}")
    artifact = domain.get("artifact", {})
    expected_binding = contract.smr_digest("artifact-binding", artifact)
    if domain.get("artifact_binding_digest") != expected_binding:
        errors.append(f"artifact_binding_digest must reproduce as {expected_binding}")

    vectors = contract.load_json(DIGEST_VECTORS)["vectors"]
    empty_base_digest = next(
        vector["expected_digest"]
        for vector in vectors
        if vector["vector_id"] == "V13-EMPTY-SMR-BASE-SNAPSHOT"
    )
    if identity.get("smr_base_snapshot_digest") != empty_base_digest:
        errors.append("smr_base_snapshot_digest must use the approved V13 vector")
    if identity.get("model_atom_profile") != "digest-profile-v1/model-atom":
        errors.append("ModelAtom identity must use the approved digest profile")

    claims = claim_set.get("claims", [])
    if [claim.get("context_statement") for claim in claims] != EXPECTED_STATEMENTS:
        errors.append("claims must preserve the twelve owner-approved statements in order")
    if package.get("scope", {}).get("completeness") != "partial":
        errors.append("the reviewed scope must remain partial")
    closures = claim_set.get("closure_assertions", [])
    if len(closures) != 1 or closures[0].get("closure_kind") != "partial":
        errors.append("the normalized Claim set must carry one partial closure assertion")

    claim_ids = _unique_ids(claims, "claim_id", errors)
    atoms = claim_set.get("model_atoms", [])
    atom_ids = _unique_ids(atoms, "atom_id", errors)
    for atom in atoms:
        expected_atom_id = contract.smr_digest(
            "model-atom",
            {
                "atom_kind": atom["atom_kind"],
                "scope_id": atom["scope_id"],
                "semantic_key": atom["semantic_key"],
            },
        )
        if atom["atom_id"] != expected_atom_id:
            errors.append(f"{atom['atom_id']}: ModelAtom identity must be {expected_atom_id}")
    assertions = claim_set.get("claim_assertions", [])
    assertion_ids = _unique_ids(assertions, "assertion_id", errors)
    relations = claim_set.get("atom_relations", [])
    _unique_ids(relations, "relation_id", errors)
    anchors = claim_set.get("source_anchors", [])
    anchor_ids = _unique_ids(anchors, "anchor_id", errors)

    for claim in claims:
        if not set(claim["assertion_ids"]) <= assertion_ids:
            errors.append(f"{claim['claim_id']}: unresolved assertion reference")
        if not set(claim["source_anchor_ids"]) <= anchor_ids:
            errors.append(f"{claim['claim_id']}: unresolved source anchor")
    for assertion in assertions:
        if assertion["claim_id"] not in claim_ids or assertion["atom_id"] not in atom_ids:
            errors.append(f"{assertion['assertion_id']}: unresolved Claim or ModelAtom")
    claim_anchors = {claim["claim_id"]: set(claim["source_anchor_ids"]) for claim in claims}
    for relation in relations:
        if relation["from_atom_id"] not in atom_ids or relation["to_atom_id"] not in atom_ids:
            errors.append(f"{relation['relation_id']}: unresolved relation endpoint")
        supports = set(relation["supporting_claim_ids"])
        relation_anchors = set(relation["supporting_source_anchor_ids"])
        if not supports <= claim_ids or not relation_anchors <= anchor_ids:
            errors.append(f"{relation['relation_id']}: unresolved support identity")
        reachable = set().union(*(claim_anchors.get(claim_id, set()) for claim_id in supports))
        if not relation_anchors <= reachable:
            errors.append(f"{relation['relation_id']}: source support is not reachable through its Claims")
    binding_digest = domain.get("artifact_binding_digest")
    if any(anchor.get("artifact_binding_ref") != binding_digest for anchor in anchors):
        errors.append("every source anchor must bind the exact provenance artifact")

    if set(package.get("unresolved_before_canonical_admission", [])) != EXPECTED_HOLDS:
        errors.append("canonical-admission holds changed without resolving the underlying contracts")

    try:
        replayed = replay_throwaway_fixture(package)
    except (KeyError, TypeError, ValueError) as exc:
        errors.append(str(exc))
    else:
        if contract.smr_digest("claim-set", replayed) != expected_digest:
            errors.append("throwaway replay changed the reviewed Claim-set projection")
    return errors


def main() -> int:
    module = _contract_module()
    errors = validate_review_package(module.load_json(FIXTURE))
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print("OK: #1221 GovernedDomain fixture replays in memory and remains non-authoritative")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
