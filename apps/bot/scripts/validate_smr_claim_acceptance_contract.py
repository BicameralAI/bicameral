#!/usr/bin/env python3
"""Deterministic validator for the SMR claim-acceptance contract (issue #937).

Validates the fixtures under ``protocol/smr-claim-acceptance/fixtures/`` against
the hand-authored contract-spec JSON Schemas under
``protocol/smr-claim-acceptance/schemas/``, recomputes the golden digest
vectors, and enforces the cross-artifact invariants recorded in
``docs/specs/state-machine-resolver-v1-runtime-architecture.md`` and its
companion ``.contract.json``.

The invariants encoded here are the ones a prose contract cannot enforce:

  * digest profile v1 construction, encoding, key-order independence, NFC
    normalization, and domain separation (golden vectors);
  * ConfirmationChallenge v2 is a strict SUPERSET of the shipped v1 binding
    (amendment A2) and keeps the shipped 300-second TTL (amendment A1);
  * ``display_summary`` is bound only through ``display_summary_digest``
    (amendment A3);
  * ``base_event_position`` accompanies ``smr_base_snapshot_digest``
    (amendment A6);
  * rejection events are never claim-bearing;
  * replay performs zero provider calls;
  * the prohibited digest names never reappear;
  * the spec, the companion contract, and the schemas agree on conformance IDs,
    amendment IDs, and closed enums.

Dependency-free (stdlib only) so product repos need no extra tooling, matching
``scripts/validate_onboarding_contract_fixtures.py``.

Usage:

    python3 scripts/validate_smr_claim_acceptance_contract.py
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
import unicodedata
from datetime import datetime
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "protocol" / "smr-claim-acceptance"
SCHEMA_DIR = BASE / "schemas"
FIXTURE_DIR = BASE / "fixtures"
SPEC = ROOT / "docs" / "specs" / "state-machine-resolver-v1-runtime-architecture.md"
CONTRACT = ROOT / "docs" / "specs" / "state-machine-resolver-v1-runtime-architecture.contract.json"

SCHEMA_ID_PREFIX = "https://bicameral.dev/protocol/smr-claim-acceptance/v1/"

# Fixture name -> schema file that must accept it.
FIXTURE_SCHEMA = {
    "smr-claim-set-proposal.valid.json": "smr-claim-set-proposal.schema.json",
    "claim-set-candidate-promoted.valid.json": "claim-set-candidate-promoted.schema.json",
    "claim-set-candidate-rejected.valid.json": "claim-set-candidate-rejected.schema.json",
    "confirmation-binding.v2.valid.json": "confirmation-binding.v2.schema.json",
    "promotion-receipt.committed-once.json": "promotion-receipt.schema.json",
    "promotion-receipt.identity-mismatch.json": "promotion-receipt.schema.json",
    "test-manifest.valid.json": "test-manifest.schema.json",
}

# Shipped v1 ChallengeBinding fields
# (crates/bicameral-api/src/confirmation_challenge.rs:123). Amendment A2 keeps
# v2 a strict superset: dropping any of these would remove a shipped typed
# fail-closed control such as WorkspaceMismatch or PacketMismatch.
V1_BOUND_FIELDS = {
    "protocol_version",
    "workspace",
    "actor_id",
    "session_id",
    "packet_id",
    "candidate_id",
    "requested_outcome",
    "request_id",
}

# Shipped PromotionOutcome wire enum (crates/bicameral-api/src/recall.rs:703).
# Amendment 001 (A15) binds requested_outcome into ConfirmationChallengeV2:
# omitting it would let the reviewed disposition change without invalidating
# the binding, losing the shipped OutcomeMismatch control.
PROMOTION_OUTCOMES = [
    "new_constraint",
    "supersedes_existing_decision",
    "keeps_both_with_scope",
    "needs_signoff_review",
    "dismissed_as_noise",
]

# Amendment 001 action pairing: a rejection may only carry dismissed_as_noise,
# and a promotion may never carry it.
ACTION_PAIRING = {
    "promote_smr_claim_set": [
        "new_constraint",
        "supersedes_existing_decision",
        "keeps_both_with_scope",
        "needs_signoff_review",
    ],
    "reject_smr_claim_set": ["dismissed_as_noise"],
}

AMENDMENT_001 = (
    ROOT
    / "docs"
    / "specs"
    / "state-machine-resolver-v1-runtime-architecture.contract.amendment-001.json"
)
AMENDMENT_002 = (
    ROOT
    / "docs"
    / "specs"
    / "state-machine-resolver-v1-runtime-architecture.contract.amendment-002.json"
)
AMENDMENT_003 = (
    ROOT
    / "docs"
    / "specs"
    / "state-machine-resolver-v1-runtime-architecture.contract.amendment-003.json"
)
AMENDMENT_004 = (
    ROOT
    / "docs"
    / "specs"
    / "state-machine-resolver-v1-runtime-architecture.contract.amendment-004.json"
)
AMENDMENT_005 = (
    ROOT
    / "docs"
    / "specs"
    / "state-machine-resolver-v1-runtime-architecture.contract.amendment-005.json"
)

# Shipped TTL, crates/bicameral-api/src/confirmation_challenge.rs:43.
CONFIRMATION_TTL_SECONDS = 300

# Names that conflated SMR and CodeGraph identity in the earlier draft.
PROHIBITED_DIGEST_NAMES = ("canonical_snapshot_digest", "scope_digest")

# Never allowed to cross the contract boundary in a fixture.
FORBIDDEN_SUBSTRINGS = (
    "eyj",
    "service_role",
    "anon_key",
    "access_token",
    "refresh_token",
    "postgres://",
    "postgresql://",
    "/home/",
    "/users/",
    "/var/",
    "/tmp/",
    "c:\\",
)

REQUIRED_CONFORMANCE_IDS = {
    "CONF-SMR-PROPOSAL-SCHEMA-01",
    "CONF-SMR-DIGEST-PROFILE-01",
    "CONF-SMR-PROVIDER-BOUNDARY-01",
    "CONF-SMR-CONFIRMATION-V2-01",
    "CONF-SMR-CANONICAL-COMMIT-01",
    "CONF-SMR-REJECTION-REPLAY-01",
    "CONF-SMR-LEGACY-CANDIDATE-COMPAT-01",
    "CONF-SMR-CLAIM-AUTHORITY-01",
    "CONF-SMR-ACCEPTANCE-JOURNEY-01",
    "CONF-SMR-CHALLENGE-BINDING-SUPERSET-01",
    "CONF-SMR-BASE-POSITION-01",
    "CONF-SMR-OPERATION-IDENTITY-01",
    "CONF-SMR-EVENT-BOUNDS-01",
    "CONF-SMR-ROUTING-DERIVATION-01",
    "CONF-SMR-TERMINAL-DISPOSITION-01",
    "CONF-SMR-APPEND-AUTHORITY-01",
    "CONF-SMR-BASE-FRESHNESS-01",
    "CONF-SMR-PROJECTION-CATCHUP-01",
    "CONF-SMR-REPLAY-TOTALITY-01",
    "CONF-SMR-MODEL-ATOM-IDENTITY-01",
    "CONF-SMR-DECISION-AUTHORITY-01",
    "CONF-SMR-CODEGRAPH-RECALL-ONLY-01",
    "CONF-SMR-RELATION-SUPPORT-01",
}

REQUIRED_IDENTITY_VECTOR_IDS = {
    "V12-MODEL-ATOM-IDENTITY",
    "V13-EMPTY-SMR-BASE-SNAPSHOT",
}

# Obligations introduced by amendment 001 and carried in the amendment file.
AMENDMENT_001_CONFORMANCE_IDS = {
    "CONF-SMR-CHALLENGE-OUTCOME-SUPERSET-01",
    "CONF-SMR-CONFIRMATION-SCHEMA-REF-01",
    "CONF-SMR-CONFIRMATION-DIGEST-01",
}
AMENDMENT_002_CONFORMANCE_IDS = {
    "CONF-SMR-TERMINAL-DISPOSITION-01",
    "CONF-SMR-APPEND-AUTHORITY-01",
    "CONF-SMR-BASE-FRESHNESS-01",
    "CONF-SMR-PROJECTION-CATCHUP-01",
    "CONF-SMR-REPLAY-TOTALITY-01",
    "CONF-SMR-MODEL-ATOM-IDENTITY-01",
    "CONF-SMR-DECISION-AUTHORITY-01",
    "CONF-SMR-CODEGRAPH-RECALL-ONLY-01",
}
AMENDMENT_003_CONFORMANCE_IDS = {"CONF-SMR-RELATION-SUPPORT-01"}
AMENDMENT_004_CONFORMANCE_IDS = {"CONF-SMR-REPLAY-TOTALITY-01"}
AMENDMENT_005_CONFORMANCE_IDS = {
    "CONF-SMR-DECISION-AUTHORITY-01",
    "CONF-SMR-APPEND-AUTHORITY-01",
}

TEST_RUN_STATES = [
    "not_run",
    "blocked",
    "diagnostic_failed",
    "diagnostic_passed",
    "authoritative_failed",
    "authoritative_inconclusive",
    "authoritative_passed",
    "human_rejected",
    "human_accepted",
]


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


# --- minimal draft-07 subset validator ---------------------------------------


def validate(value: Any, schema: dict, store: dict, path: str, errors: list[str]) -> None:
    if "$ref" in schema:
        ref = schema["$ref"]
        if ref.startswith("#/definitions/"):
            target = store["__self__"].get("definitions", {}).get(ref.split("/")[-1])
            if target is None:
                errors.append(f"{path}: unresolvable $ref {ref}")
                return
            validate(value, target, store, path, errors)
            return
        errors.append(f"{path}: unsupported $ref {ref}")
        return

    if "const" in schema:
        if value != schema["const"]:
            errors.append(f"{path}: expected const {schema['const']!r}, got {value!r}")
        return

    if "enum" in schema:
        if value not in schema["enum"]:
            errors.append(f"{path}: {value!r} not in enum {schema['enum']}")
        return

    expected = schema.get("type")
    if expected == "object":
        if not isinstance(value, dict):
            errors.append(f"{path}: expected object, got {type(value).__name__}")
            return
        for req in schema.get("required", []):
            if req not in value:
                errors.append(f"{path}.{req}: required field missing")
        props = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            for key in value:
                if key not in props:
                    errors.append(f"{path}.{key}: additional property not permitted")
        for key, sub in props.items():
            if key in value:
                validate(value[key], sub, store, f"{path}.{key}", errors)
        return

    if expected == "array":
        if not isinstance(value, list):
            errors.append(f"{path}: expected array, got {type(value).__name__}")
            return
        if "minItems" in schema and len(value) < schema["minItems"]:
            errors.append(f"{path}: expected at least {schema['minItems']} items, got {len(value)}")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            errors.append(f"{path}: exceeds maxItems {schema['maxItems']} (got {len(value)})")
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for i, item in enumerate(value):
                validate(item, item_schema, store, f"{path}[{i}]", errors)
        return

    if expected == "string":
        if not isinstance(value, str):
            errors.append(f"{path}: expected string, got {type(value).__name__}")
            return
        if "minLength" in schema and len(value) < schema["minLength"]:
            errors.append(f"{path}: shorter than minLength {schema['minLength']}")
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            errors.append(f"{path}: longer than maxLength {schema['maxLength']}")
        pattern = schema.get("pattern")
        if pattern and not re.search(pattern, value):
            errors.append(f"{path}: {value!r} does not match pattern {pattern}")
        return

    if expected == "integer":
        # bool is a subclass of int; reject it explicitly.
        if not isinstance(value, int) or isinstance(value, bool):
            errors.append(f"{path}: expected integer, got {type(value).__name__}")
            return
        if "minimum" in schema and value < schema["minimum"]:
            errors.append(f"{path}: {value} below minimum {schema['minimum']}")
        if "maximum" in schema and value > schema["maximum"]:
            errors.append(f"{path}: {value} above maximum {schema['maximum']}")
        return

    if expected == "boolean":
        if not isinstance(value, bool):
            errors.append(f"{path}: expected boolean, got {type(value).__name__}")
        return

    if expected == "number":
        errors.append(f"{path}: floating-point values are forbidden by digest profile v1")
        return


# --- digest profile ----------------------------------------------------------


def jcs(value: Any) -> bytes:
    """RFC 8785 canonical bytes for the value space this profile permits."""
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def nfc(value: Any) -> Any:
    if isinstance(value, str):
        return unicodedata.normalize("NFC", value)
    if isinstance(value, list):
        return [nfc(v) for v in value]
    if isinstance(value, dict):
        return {nfc(k): nfc(v) for k, v in value.items()}
    return value


def smr_digest(artifact_name: str, value: Any) -> str:
    tag = f"bicameral/{artifact_name}/v1".encode("utf-8")
    return "sha256:" + hashlib.sha256(tag + b"\x00" + jcs(nfc(value))).hexdigest()


def check_digest_vectors(errors: list[str]) -> None:
    path = FIXTURE_DIR / "digest-vectors.json"
    if not path.exists():
        errors.append("digest-vectors.json: missing golden digest vectors")
        return
    data = load_json(path)
    by_id = {}
    for vec in data.get("vectors", []):
        vid = vec["vector_id"]
        by_id[vid] = vec
        normalized_value = nfc(vec["normalized_value"])
        canonical_bytes = jcs(normalized_value)
        preimage = (
            f"bicameral/{vec['artifact_name']}/v1".encode("utf-8")
            + b"\x00"
            + canonical_bytes
        )
        actual = "sha256:" + hashlib.sha256(preimage).hexdigest()
        if actual != vec["expected_digest"]:
            errors.append(
                f"digest-vectors.json:{vid}: recomputed {actual} != expected {vec['expected_digest']}"
            )
        if (
            "expected_jcs_utf8_hex" in vec
            and canonical_bytes.hex() != vec["expected_jcs_utf8_hex"]
        ):
            errors.append(
                f"digest-vectors.json:{vid}: JCS bytes do not match expected_jcs_utf8_hex"
            )
        if (
            "expected_preimage_hex" in vec
            and preimage.hex() != vec["expected_preimage_hex"]
        ):
            errors.append(
                f"digest-vectors.json:{vid}: digest preimage does not match expected_preimage_hex"
            )
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", vec["expected_digest"]):
            errors.append(f"digest-vectors.json:{vid}: malformed digest encoding")

    missing_identity_vectors = REQUIRED_IDENTITY_VECTOR_IDS - set(by_id)
    if missing_identity_vectors:
        errors.append(
            "digest-vectors.json: missing SMR identity vectors "
            f"{sorted(missing_identity_vectors)}"
        )

    for pair in data.get("equality_pairs", []):
        a, b = by_id.get(pair["a"]), by_id.get(pair["b"])
        if not a or not b:
            errors.append(f"digest-vectors.json: equality pair references unknown vector {pair}")
        elif a["expected_digest"] != b["expected_digest"]:
            errors.append(
                f"digest-vectors.json: {pair['a']} and {pair['b']} must be equal ({pair['reason']})"
            )
    for pair in data.get("inequality_pairs", []):
        a, b = by_id.get(pair["a"]), by_id.get(pair["b"])
        if not a or not b:
            errors.append(f"digest-vectors.json: inequality pair references unknown vector {pair}")
        elif a["expected_digest"] == b["expected_digest"]:
            errors.append(
                f"digest-vectors.json: {pair['a']} and {pair['b']} must differ ({pair['reason']})"
            )


# --- cross-artifact invariants ----------------------------------------------


def check_schema_ids(errors: list[str]) -> None:
    for schema_path in sorted(SCHEMA_DIR.glob("*.schema.json")):
        schema = load_json(schema_path)
        expected = f"{SCHEMA_ID_PREFIX}{schema_path.name}"
        if schema.get("$id") != expected:
            errors.append(
                f"{schema_path.name}: $id must be {expected!r}, got {schema.get('$id')!r}"
            )
        if schema.get("$schema") != "http://json-schema.org/draft-07/schema#":
            errors.append(f"{schema_path.name}: must declare draft-07")


def check_secret_boundary(name: str, raw: str, errors: list[str]) -> None:
    lower = raw.lower()
    for needle in FORBIDDEN_SUBSTRINGS:
        if needle in lower:
            errors.append(
                f"{name}: carries forbidden boundary material (matched {needle!r}); no "
                f"credential, token, or local path may cross the contract boundary"
            )


def check_prohibited_names(errors: list[str]) -> None:
    """canonical_snapshot_digest and scope_digest conflated SMR and CodeGraph identity."""
    targets = list(SCHEMA_DIR.glob("*.json")) + list(FIXTURE_DIR.glob("*.json")) + [CONTRACT]
    for path in targets:
        text = path.read_text(encoding="utf-8")
        for name in PROHIBITED_DIGEST_NAMES:
            # The contract file legitimately lists them as prohibited.
            if path == CONTRACT and '"prohibited_identity_names"' in text:
                continue
            if f'"{name}"' in text:
                errors.append(f"{path.name}: prohibited digest identity name {name!r}")


def check_confirmation_v2(errors: list[str]) -> None:
    schema = load_json(SCHEMA_DIR / "confirmation-binding.v2.schema.json")
    required = set(schema.get("required", []))

    missing = V1_BOUND_FIELDS - required
    if missing:
        errors.append(
            "confirmation-binding.v2.schema.json: amendment A2 requires a strict superset of the "
            f"shipped v1 binding; missing {sorted(missing)}"
        )

    props = schema.get("properties", {})
    if props.get("protocol_version", {}).get("const") != 2:
        errors.append("confirmation-binding.v2.schema.json: protocol_version must be const 2")

    # A3: display_summary is bound only through its digest.
    if "display_summary_digest" not in required:
        errors.append(
            "confirmation-binding.v2.schema.json: amendment A3 requires display_summary_digest"
        )
    binding_desc = props.get("confirmation_binding_digest", {}).get("description", "")
    if "except display_summary" not in binding_desc:
        errors.append(
            "confirmation-binding.v2.schema.json: the binding digest description must exclude "
            "display_summary itself (amendment A3)"
        )

    # A6: base_event_position travels with the base snapshot digest.
    identity_required = set(
        props.get("proposal_identity", {}).get("required", [])
    )
    if "smr_base_snapshot_digest" in identity_required and "base_event_position" not in identity_required:
        errors.append(
            "confirmation-binding.v2.schema.json: amendment A6 requires base_event_position "
            "alongside smr_base_snapshot_digest"
        )

    # A1: TTL is the shipped 300 seconds.
    fixture = load_json(FIXTURE_DIR / "confirmation-binding.v2.valid.json")
    issued = datetime.fromisoformat(fixture["issued_at"].replace("Z", "+00:00"))
    expires = datetime.fromisoformat(fixture["expires_at"].replace("Z", "+00:00"))
    delta = (expires - issued).total_seconds()
    if delta != CONFIRMATION_TTL_SECONDS:
        errors.append(
            f"confirmation-binding.v2.valid.json: TTL is {delta:.0f}s, expected "
            f"{CONFIRMATION_TTL_SECONDS}s to match the shipped "
            f"CONFIRMATION_CHALLENGE_TTL_SECONDS (amendment A1)"
        )
    for field in sorted(V1_BOUND_FIELDS - {"protocol_version"}):
        if field not in fixture:
            errors.append(f"confirmation-binding.v2.valid.json: missing retained v1 field {field!r}")

    normalized_binding = {
        key: value
        for key, value in fixture.items()
        if key not in {"display_summary", "confirmation_binding_digest"}
    }
    expected_binding_digest = smr_digest("confirmation-binding", normalized_binding)
    if fixture.get("confirmation_binding_digest") != expected_binding_digest:
        errors.append(
            "confirmation-binding.v2.valid.json: confirmation_binding_digest does not "
            "match the exact normalized binding"
        )


def check_amendment_001(errors: list[str]) -> None:
    """Amendment 001 (A15/A16): requested_outcome binding and schema reference."""
    if not AMENDMENT_001.exists():
        errors.append("amendment-001 companion contract is missing")
        return
    amendment = load_json(AMENDMENT_001)

    if amendment.get("normative") is not True:
        errors.append("amendment-001: must be normative")
    if amendment.get("authorizes_implementation") is not False:
        errors.append("amendment-001: authorizes no implementation")

    ids = {c["id"] for c in amendment.get("conformance_obligations", [])}
    missing = AMENDMENT_001_CONFORMANCE_IDS - ids
    if missing:
        errors.append(f"amendment-001: missing conformance obligations {sorted(missing)}")

    protocol = amendment.get("confirmation_protocol_v2", {})

    # A15: the wire enum must match the shipped PromotionOutcome exactly.
    if protocol.get("wire_enum") != PROMOTION_OUTCOMES:
        errors.append(
            "amendment-001: wire_enum must match the shipped PromotionOutcome exactly "
            f"({PROMOTION_OUTCOMES})"
        )

    pairing = protocol.get("action_pairing", {})
    if pairing != ACTION_PAIRING:
        errors.append("amendment-001: action_pairing must match the normative pairing")

    # The schema must actually require and constrain requested_outcome.
    schema = load_json(SCHEMA_DIR / "confirmation-binding.v2.schema.json")
    if "requested_outcome" not in set(schema.get("required", [])):
        errors.append(
            "confirmation-binding.v2.schema.json: amendment 001 (A15) requires requested_outcome; "
            "the shipped v1 binding returns OutcomeMismatch on it"
        )
    outcome_enum = schema.get("properties", {}).get("requested_outcome", {}).get("enum")
    if outcome_enum != PROMOTION_OUTCOMES:
        errors.append(
            "confirmation-binding.v2.schema.json: requested_outcome enum must be the shipped "
            f"PromotionOutcome wire enum {PROMOTION_OUTCOMES}, got {outcome_enum}"
        )

    # A16: the base contract must reference an existing schema artifact.
    contract = load_json(CONTRACT)
    schema_ref = contract.get("confirmation_protocol", {}).get("schema")
    if schema_ref and not (SCHEMA_DIR / schema_ref).is_file():
        errors.append(
            f"contract.json: confirmation_protocol.schema {schema_ref!r} does not resolve to an "
            "existing protocol artifact (amendment 001, A16)"
        )

    # The fixture must honour the action pairing.
    fixture = load_json(FIXTURE_DIR / "confirmation-binding.v2.valid.json")
    action = fixture.get("action")
    outcome = fixture.get("requested_outcome")
    permitted = ACTION_PAIRING.get(action)
    if permitted is None:
        errors.append(f"confirmation-binding.v2.valid.json: unknown action {action!r}")
    elif outcome not in permitted:
        errors.append(
            f"confirmation-binding.v2.valid.json: action {action!r} may not carry "
            f"requested_outcome {outcome!r}; permitted {permitted}"
        )

    # The binding digest must cover requested_outcome and exclude the summary text.
    digest_spec = protocol.get("binding_digest", {})
    includes = set(digest_spec.get("includes", []))
    excludes = set(digest_spec.get("excludes", []))
    if "requested_outcome" not in includes:
        errors.append("amendment-001: binding_digest must include requested_outcome")
    if "display_summary" not in excludes:
        errors.append("amendment-001: binding_digest must exclude the display_summary text")


def check_amendment_002(errors: list[str]) -> None:
    """Amendment 002 binds every relational-audit invariant."""
    if not AMENDMENT_002.exists():
        errors.append("amendment-002 companion contract missing")
        return

    amendment = load_json(AMENDMENT_002)
    if amendment.get("normative") is not True:
        errors.append("amendment-002: must be normative")
    if amendment.get("authorizes_implementation") is not False:
        errors.append("amendment-002: must not authorize implementation")
    if amendment.get("authorizes_release") is not False:
        errors.append("amendment-002: must not authorize release")

    amendment_ids = {item.get("id") for item in amendment.get("amendments", [])}
    if amendment_ids != {f"A{number}" for number in range(17, 25)}:
        errors.append("amendment-002: must contain exactly A17 through A24")

    conformance_ids = set(amendment.get("conformance_obligations", []))
    if conformance_ids != AMENDMENT_002_CONFORMANCE_IDS:
        errors.append(
            "amendment-002: conformance obligations must exactly match temporal audit"
        )

    journey = amendment.get("journey_boundary_review", {})
    if (
        journey.get("status") != "passed_for_specification_boundary"
        or journey.get("catalog_and_terminal_execution") != "delegated_to_issue_943"
        or journey.get("terminal_readiness_from_component_tests") is not False
    ):
        errors.append(
            "amendment-002: Journey Boundary Review must pass without claiming terminal evidence"
        )


def check_amendment_003(errors: list[str]) -> None:
    """Amendment 003 requires relation support lineage without fake epistemic labels."""
    if not AMENDMENT_003.exists():
        errors.append("amendment-003 companion contract missing")
        return

    amendment = load_json(AMENDMENT_003)
    if amendment.get("normative") is not True:
        errors.append("amendment-003: must be normative")
    if amendment.get("authorizes_implementation") is not False:
        errors.append("amendment-003: must not authorize implementation")
    if amendment.get("authorizes_release") is not False:
        errors.append("amendment-003: must not authorize release")

    amendments = amendment.get("amendments", [])
    if [item.get("id") for item in amendments] != ["A25"]:
        errors.append("amendment-003: must contain exactly A25")
        return

    relation_amendment = amendments[0]
    if relation_amendment.get("required_fields") != [
        "supporting_claim_ids",
        "supporting_source_anchor_ids",
    ]:
        errors.append("amendment-003: A25 must require both relation support fields")
    if relation_amendment.get("normalization") != (
        "deduplicated and sorted by stable semantic ID"
    ):
        errors.append("amendment-003: A25 must bind support-array normalization")
    if (
        relation_amendment.get("mismatch_result")
        != "proposal_identity_mismatch"
        or relation_amendment.get("appends_on_mismatch") is not False
    ):
        errors.append("amendment-003: relation support mismatch must append nothing")
    if set(relation_amendment.get("prohibited_v1_fields", [])) != {
        "source_explicit",
        "rule_derived",
        "model_inferred",
    }:
        errors.append("amendment-003: A25 must prohibit v1 derivation classifications")
    if set(amendment.get("conformance_obligations", [])) != AMENDMENT_003_CONFORMANCE_IDS:
        errors.append("amendment-003: conformance obligation must be relation support")


def check_amendment_004(errors: list[str]) -> None:
    """Amendment 004 makes claim-set terminals self-contained on replay."""
    if not AMENDMENT_004.exists():
        errors.append("amendment-004 companion contract missing")
        return

    amendment = load_json(AMENDMENT_004)
    if amendment.get("normative") is not True:
        errors.append("amendment-004: must be normative")
    if amendment.get("authorizes_implementation") is not False:
        errors.append("amendment-004: must not authorize implementation")
    if amendment.get("authorizes_release") is not False:
        errors.append("amendment-004: must not authorize release")

    amendments = amendment.get("amendments", [])
    if [item.get("id") for item in amendments] != ["A26"]:
        errors.append("amendment-004: must contain exactly A26")
        return

    replay_amendment = amendments[0]
    if set(replay_amendment.get("self_contained_terminal_events", [])) != {
        "claim_set_candidate_promoted",
        "claim_set_candidate_rejected",
    }:
        errors.append(
            "amendment-004: A26 must mark promotion and rejection as self-contained"
        )
    if (
        replay_amendment.get("forbidden_canonical_generation_event")
        != "claim_set_candidate_generated"
    ):
        errors.append(
            "amendment-004: A26 must forbid claim_set_candidate_generated"
        )
    if set(replay_amendment.get("required_predecessors_remain", [])) != {
        "approve_signoff",
        "reject_signoff",
        "supersede_decision",
    }:
        errors.append(
            "amendment-004: A26 must keep Decision signoff/supersession predecessors"
        )
    append_time = replay_amendment.get("append_time_candidate_generation", {})
    if (
        append_time.get("required_for") != "live_j2_compare_and_append"
        or append_time.get("not_required_for") != "replay"
    ):
        errors.append(
            "amendment-004: A26 must keep live J2 append-time candidate checks"
        )
    if replay_amendment.get("silent_ignore") is not False:
        errors.append("amendment-004: A26 must still forbid silent orphan ignore")
    if set(amendment.get("conformance_obligations", [])) != AMENDMENT_004_CONFORMANCE_IDS:
        errors.append(
            "amendment-004: conformance obligation must refine replay totality"
        )

    relation_schema = load_json(
        SCHEMA_DIR / "smr-claim-set-proposal.schema.json"
    )["definitions"]["AtomRelation"]
    required = set(relation_schema.get("required", []))
    for field in ("supporting_claim_ids", "supporting_source_anchor_ids"):
        if field not in required:
            errors.append(f"AtomRelation: amendment A25 requires {field}")
    if set(relation_schema.get("properties", {})) & {
        "source_explicit",
        "rule_derived",
        "model_inferred",
    }:
        errors.append("AtomRelation: provider-supplied derivation classification is prohibited")


def check_amendment_005(errors: list[str]) -> None:
    """Amendment 005: alpha confirm writes approved; every participant is Owner."""
    if not AMENDMENT_005.exists():
        errors.append("amendment-005 companion contract missing")
        return

    amendment = load_json(AMENDMENT_005)
    if amendment.get("normative") is not True:
        errors.append("amendment-005: must be normative")
    if amendment.get("authorizes_implementation") is not False:
        errors.append("amendment-005: must not authorize implementation")
    if amendment.get("authorizes_release") is not False:
        errors.append("amendment-005: must not authorize release")

    amendments = amendment.get("amendments", [])
    if [item.get("id") for item in amendments] != ["A27"]:
        errors.append("amendment-005: must contain exactly A27")
        return

    authority = amendments[0]
    if authority.get("resulting_signoff_state_on_claim_set_promote") != "approved":
        errors.append("amendment-005: A27 must write Decision(approved) on claim-set promote")
    if authority.get("observable_proposed_on_claim_set_path") is not False:
        errors.append("amendment-005: A27 must skip observable Decision(proposed) on this path")
    if authority.get("promotion_role") != "product_owner":
        errors.append("amendment-005: A27 promotion role must be product_owner")
    if authority.get("alpha_every_current_participant_is_product_owner") is not True:
        errors.append("amendment-005: A27 must make every current participant Product Owner")
    if authority.get("policy_owner_remains_creator_only") is not True:
        errors.append("amendment-005: A27 must keep Policy Owner creator-only")
    if set(amendment.get("conformance_obligations", [])) != AMENDMENT_005_CONFORMANCE_IDS:
        errors.append("amendment-005: conformance must refine decision and append authority")


def validate_relation_support(
    claim_set: dict[str, Any], context: str, errors: list[str]
) -> None:
    """Validate same-set Claim/SourceAnchor closure for every AtomRelation."""
    claims = {
        claim.get("claim_id"): claim
        for claim in claim_set.get("claims", [])
        if isinstance(claim, dict)
    }
    anchors = {
        anchor.get("anchor_id")
        for anchor in claim_set.get("source_anchors", [])
        if isinstance(anchor, dict)
    }

    for index, relation in enumerate(claim_set.get("atom_relations", [])):
        path = f"{context}.atom_relations[{index}]"
        if not isinstance(relation, dict):
            errors.append(f"{path}: relation must be an object")
            continue
        claim_ids = relation.get("supporting_claim_ids", [])
        anchor_ids = relation.get("supporting_source_anchor_ids", [])
        if not claim_ids:
            errors.append(f"{path}.supporting_claim_ids: at least one Claim is required")
        if not anchor_ids:
            errors.append(
                f"{path}.supporting_source_anchor_ids: at least one SourceAnchor is required"
            )

        if len(claim_ids) != len(set(claim_ids)) or claim_ids != sorted(claim_ids):
            errors.append(
                f"{path}.supporting_claim_ids: must be deduplicated and sorted by stable Claim id"
            )
        if len(anchor_ids) != len(set(anchor_ids)) or anchor_ids != sorted(anchor_ids):
            errors.append(
                f"{path}.supporting_source_anchor_ids: must be deduplicated and sorted by stable SourceAnchor id"
            )

        missing_claims = sorted(set(claim_ids) - set(claims))
        if missing_claims:
            errors.append(
                f"{path}.supporting_claim_ids: unknown same-set Claims {missing_claims}"
            )
        missing_anchors = sorted(set(anchor_ids) - anchors)
        if missing_anchors:
            errors.append(
                f"{path}.supporting_source_anchor_ids: unknown same-set SourceAnchors {missing_anchors}"
            )

        reachable_anchors = {
            anchor_id
            for claim_id in claim_ids
            for anchor_id in claims.get(claim_id, {}).get("source_anchor_ids", [])
        }
        unreachable = sorted(set(anchor_ids) - reachable_anchors)
        if unreachable:
            errors.append(
                f"{path}.supporting_source_anchor_ids: anchors are not reachable through supporting Claims {unreachable}"
            )


def check_relation_support(errors: list[str]) -> None:
    for fixture_name in (
        "smr-claim-set-proposal.valid.json",
        "claim-set-candidate-promoted.valid.json",
    ):
        fixture = load_json(FIXTURE_DIR / fixture_name)
        validate_relation_support(
            fixture["normalized_claim_set"], fixture_name, errors
        )
        expected_digest = smr_digest("claim-set", fixture["normalized_claim_set"])
        if fixture.get("digests", {}).get("claim_set_digest") != expected_digest:
            errors.append(
                f"{fixture_name}: claim_set_digest does not bind the exact relation support lineage"
            )


def check_promoted_event(errors: list[str]) -> None:
    fixture = load_json(FIXTURE_DIR / "claim-set-candidate-promoted.valid.json")
    if fixture["acceptance"]["confirmation_protocol_version"] != 2:
        errors.append("claim-set-candidate-promoted.valid.json: must bind confirmation protocol v2")
    if fixture["resulting_decision"]["signoff_state"] != "approved":
        errors.append(
            "claim-set-candidate-promoted.valid.json: alpha promotion creates Decision(approved); "
            "Decision(proposed) is out of this alpha cut on this path"
        )
    if "base_event_position" not in fixture:
        errors.append("claim-set-candidate-promoted.valid.json: amendment A6 requires base_event_position")
    if fixture["candidate"]["kind"] != "smr_claim_set_v1":
        errors.append("claim-set-candidate-promoted.valid.json: candidate kind must be smr_claim_set_v1")
    if not fixture["routing"].get("event_stream_id"):
        errors.append("claim-set-candidate-promoted.valid.json: daemon-derived event_stream_id required")


def check_rejected_event(errors: list[str]) -> None:
    schema = load_json(SCHEMA_DIR / "claim-set-candidate-rejected.schema.json")
    props = set(schema.get("properties", {}))
    leaked = props & {"normalized_claim_set", "resulting_decision", "acceptance"}
    if leaked:
        errors.append(
            f"claim-set-candidate-rejected.schema.json: rejection must not be claim-bearing; "
            f"found {sorted(leaked)}"
        )
    fixture = load_json(FIXTURE_DIR / "claim-set-candidate-rejected.valid.json")
    if "rejection_fingerprint" not in fixture:
        errors.append("claim-set-candidate-rejected.valid.json: rejection_fingerprint required for replay tombstones")
    if fixture["rejection_fingerprint"] == fixture["claim_set_digest"]:
        errors.append(
            "claim-set-candidate-rejected.valid.json: rejection_fingerprint must be derived "
            "separately from claim_set_digest"
        )


def check_receipts(errors: list[str]) -> None:
    for name in ("promotion-receipt.committed-once.json", "promotion-receipt.identity-mismatch.json"):
        fixture = load_json(FIXTURE_DIR / name)
        calls = fixture.get("smr", {}).get("provider_calls_during_replay")
        if calls != 0:
            errors.append(f"{name}: replay must perform zero provider calls, got {calls!r}")
        if fixture.get("outcome") not in TEST_RUN_STATES:
            errors.append(f"{name}: outcome must be one of the nine typed run states")
        if fixture.get("outcome") == "human_accepted" and not fixture.get("accepted_by"):
            errors.append(f"{name}: human_accepted requires a named accepted_by")
        if fixture["typed_result"] == "proposal_identity_mismatch":
            if fixture["canonical"].get("appended") is not False:
                errors.append(f"{name}: an identity mismatch must append nothing")
            if not fixture.get("mismatched_field"):
                errors.append(f"{name}: an identity mismatch must name the mismatched field")


def check_test_manifest(errors: list[str]) -> None:
    fixture = load_json(FIXTURE_DIR / "test-manifest.valid.json")
    groups = {g["group_id"] for g in fixture["groups"]}
    expected = {"T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8"}
    if groups != expected:
        errors.append(f"test-manifest.valid.json: expected groups {sorted(expected)}, got {sorted(groups)}")
    for grp in fixture["groups"]:
        if not grp.get("negative_controls"):
            errors.append(f"test-manifest.valid.json:{grp['group_id']}: negative controls are mandatory")
        for ctl in grp.get("negative_controls", []):
            if ctl.get("must_fail_closed") is not True:
                errors.append(
                    f"test-manifest.valid.json:{grp['group_id']}:{ctl['control_id']}: "
                    "negative controls must fail closed"
                )
        if grp["group_id"] == "T8":
            if grp["environment"]["topology"] != "released_host_terminal":
                errors.append("test-manifest.valid.json:T8: must run the released-host terminal topology")
            if not grp.get("human_acceptance_required"):
                errors.append("test-manifest.valid.json:T8: terminal evidence requires named human acceptance")


def check_temporal_governance(contract: dict[str, Any], errors: list[str]) -> None:
    """Reject the bounded temporal counterexamples found in the SMR audit."""
    invariants = contract.get("temporal_invariants", {})

    terminal = invariants.get("candidate_terminal_disposition")
    if not isinstance(terminal, dict):
        errors.append(
            "contract.json: temporal_invariants.candidate_terminal_disposition required"
        )
    else:
        if terminal.get("key") != [
            "product_id",
            "candidate_id",
            "candidate_generation",
        ]:
            errors.append(
                "contract.json: candidate_terminal_disposition must key Product/candidate/generation"
            )
        if terminal.get("cardinality") != "at_most_one":
            errors.append(
                "contract.json: candidate_terminal_disposition cardinality must be at_most_one"
            )
        if terminal.get("enforced_by") != "canonical_stream_compare_and_append":
            errors.append(
                "contract.json: candidate_terminal_disposition must be enforced by canonical compare-and-append"
            )

    authority = invariants.get("append_time_authorization")
    if not isinstance(authority, dict):
        errors.append(
            "contract.json: temporal_invariants.append_time_authorization required"
        )
    elif (
        authority.get("promotion_role") != "product_owner"
        or authority.get("signoff_role") != "product_owner"
        or authority.get("alpha_every_current_participant_is_product_owner") is not True
        or authority.get("challenge_is_authority") is not False
        or "compare_and_append" not in authority.get("checked_at", "")
    ):
        errors.append(
            "contract.json: append_time_authorization must recheck Product Owner role atomically; challenge is not authority"
        )

    base = invariants.get("base_snapshot_freshness")
    if not isinstance(base, dict):
        errors.append(
            "contract.json: temporal_invariants.base_snapshot_freshness required"
        )
    elif (
        base.get("mismatch_result") != "base_snapshot_drift"
        or base.get("appends_on_mismatch") is not False
        or "compare_and_append" not in base.get("checked_at", "")
    ):
        errors.append(
            "contract.json: base_snapshot_freshness must fail atomically without append"
        )

    projection = invariants.get("projection_read_freshness")
    if not isinstance(projection, dict):
        errors.append(
            "contract.json: temporal_invariants.projection_read_freshness required"
        )
    elif (
        projection.get("catch_up_before_read") is not True
        or projection.get("catch_up_failure_result") != "model_unavailable"
        or projection.get("stale_or_empty_fallback") is not False
        or "projection_cursor == canonical_event_log.head_position"
        not in projection.get("success_precondition", "")
    ):
        errors.append(
            "contract.json: projection_read_freshness must catch up to canonical head or fail visibly"
        )

    replay = invariants.get("replay_totality")
    required_predecessors = (
        replay.get("required_predecessors", {}) if isinstance(replay, dict) else {}
    )
    self_contained = (
        set(replay.get("self_contained_terminal_events", []))
        if isinstance(replay, dict)
        else set()
    )
    append_time = (
        replay.get("append_time_candidate_generation", {})
        if isinstance(replay, dict)
        else {}
    )
    if not isinstance(replay, dict):
        errors.append("contract.json: temporal_invariants.replay_totality required")
    elif (
        replay.get("missing_predecessor_result") != "invalid_canonical_history"
        or replay.get("silent_ignore") is not False
        or replay.get("candidate_generation_predecessor_required") is not False
        or replay.get("forbidden_canonical_generation_event")
        != "claim_set_candidate_generated"
        or replay.get("second_terminal_replay_result") != "invalid_canonical_history"
        or set(required_predecessors)
        != {
            "approve_signoff",
            "reject_signoff",
            "supersede_decision",
        }
        or self_contained
        != {
            "claim_set_candidate_promoted",
            "claim_set_candidate_rejected",
        }
        or append_time.get("required_for") != "live_j2_compare_and_append"
        or append_time.get("not_required_for") != "replay"
    ):
        errors.append(
            "contract.json: replay_totality must treat claim-set terminals as self-contained and fail closed only on missing Decision predecessors or a second terminal"
        )

    atom_identity = invariants.get("model_atom_identity")
    if not isinstance(atom_identity, dict):
        errors.append(
            "contract.json: temporal_invariants.model_atom_identity required"
        )
    elif (
        atom_identity.get("identity_inputs")
        != ["atom_kind", "normalized semantic_key", "normalized semantic scope"]
        or atom_identity.get("same_semantics_same_identity") is not True
        or atom_identity.get("extractor_supplied_identity_authoritative") is not False
    ):
        errors.append(
            "contract.json: model_atom_identity must derive one daemon-verified identity for normalized semantics"
        )

    decision = contract.get("state_records", {}).get("decision_signoff_lifecycle")
    expected_effect = {
        "proposed": "out_of_alpha_claim_set_path",
        "approved": "canonical_recallable_authoritative",
        "rejected": "immutable_history_only",
        "superseded": "immutable_history_only",
    }
    if not isinstance(decision, dict):
        errors.append("contract.json: decision_signoff_lifecycle required")
    elif (
        decision.get("authority") != "product_owner"
        or decision.get("initial") != "approved"
        or decision.get("alpha_every_current_participant_is_product_owner") is not True
        or decision.get("durable_states")
        != ["proposed", "approved", "rejected", "superseded"]
        or decision.get("claim_effect_by_state") != expected_effect
    ):
        errors.append(
            "contract.json: decision_signoff_lifecycle must write approved on claim-set promote and treat every current participant as Product Owner"
        )

    codegraph = next(
        (
            scenario
            for scenario in contract.get("scenarios", [])
            if scenario.get("id") == "SC-CODEGRAPH-OPTIONAL-RECALL"
        ),
        None,
    )
    required_codegraph_outcomes = {
        "core SMR remains available from the controlled spec corpus",
        "CodeGraph contributes no implementation-compliance claim",
        "CodeGraph absence never blocks SMR closure",
        "when present, CodeGraph may expand recall candidates only",
    }
    if (
        not isinstance(codegraph, dict)
        or set(codegraph.get("then", [])) != required_codegraph_outcomes
    ):
        errors.append(
            "contract.json: CodeGraph must remain optional recall expansion only, never SMR closure or implementation compliance"
        )


def check_contract_agreement(errors: list[str]) -> None:
    if not CONTRACT.exists():
        errors.append("companion contract JSON is missing")
        return
    contract = load_json(CONTRACT)
    check_temporal_governance(contract, errors)
    ids = {c["id"] for c in contract.get("conformance_obligations", [])}
    missing = REQUIRED_CONFORMANCE_IDS - ids
    if missing:
        errors.append(f"contract.json: missing conformance obligations {sorted(missing)}")

    if contract.get("disposition") != "AMEND":
        errors.append("contract.json: disposition must be AMEND")
    if contract.get("authorizes_implementation") is not False:
        errors.append("contract.json: this lane authorizes no implementation")
    if contract.get("authorizes_release") is not False:
        errors.append("contract.json: this lane authorizes no release")

    ttl = contract.get("confirmation_protocol", {}).get("ttl_seconds")
    if ttl != CONFIRMATION_TTL_SECONDS:
        errors.append(f"contract.json: confirmation TTL must be {CONFIRMATION_TTL_SECONDS}s, got {ttl!r}")

    retained = set(contract.get("confirmation_protocol", {}).get("retained_v1_bound_fields", []))
    if not V1_BOUND_FIELDS <= retained:
        errors.append(
            f"contract.json: retained_v1_bound_fields must cover {sorted(V1_BOUND_FIELDS)}"
        )

    states = contract.get("state_records", {}).get("candidate_lifecycle", {}).get("durable_states")
    if states != ["proposed", "promoted", "rejected"]:
        errors.append("contract.json: durable candidate states must be exactly proposed, promoted, rejected")

    if contract.get("test_run_states") != TEST_RUN_STATES:
        errors.append("contract.json: the nine typed run states must be preserved in order")

    support = contract.get("relation_support_provenance", {})
    if (
        support.get("required_fields")
        != ["supporting_claim_ids", "supporting_source_anchor_ids"]
        or support.get("minimum_items_each") != 1
        or support.get("normalization")
        != "deduplicated and sorted by stable semantic ID"
        or support.get("same_claim_set_referential_integrity") is not True
        or support.get("source_anchor_must_be_reachable_through_supporting_claim")
        is not True
        or support.get("mismatch_result") != "proposal_identity_mismatch"
        or support.get("appends_on_mismatch") is not False
        or support.get("records") != "proposal_input_lineage"
        or set(support.get("does_not_prove", []))
        != {"semantic_entailment", "explicitness", "correctness", "authority"}
        or set(support.get("prohibited_v1_fields", []))
        != {"source_explicit", "rule_derived", "model_inferred"}
    ):
        errors.append(
            "contract.json: relation support must bind normalized same-set review lineage without strengthening authority"
        )

    for attr, spec in contract.get("state_records", {}).get("orthogonal_attributes", {}).items():
        if spec.get("closed") is not True:
            errors.append(f"contract.json: orthogonal attribute {attr} must declare a closed enum (amendment A9)")
        if not spec.get("reason_enum"):
            errors.append(f"contract.json: orthogonal attribute {attr} must enumerate its reasons (amendment A9)")

    # Every amendment claimed in the contract must be described in the spec.
    if SPEC.exists():
        spec_text = SPEC.read_text(encoding="utf-8")
        for amendment in contract.get("amendments", []):
            aid = amendment["id"]
            if not re.search(rf"\|\s*{aid}\s*\|", spec_text):
                errors.append(f"spec: amendment {aid} is in contract.json but not in the amendment table")
    else:
        errors.append("runtime specification markdown is missing")


def check_spec_links(errors: list[str]) -> None:
    """The old docs/reviews/ path is governance-prohibited and must not reappear."""
    for path in [SPEC, CONTRACT, ROOT / "docs" / "domain" / "smr-candidate-promotion" / "README.md"]:
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8")
        if "docs/reviews/issue-937" in text:
            errors.append(
                f"{path.name}: references the governance-prohibited docs/reviews/issue-937 path"
            )


def main() -> int:
    errors: list[str] = []

    if not SCHEMA_DIR.is_dir():
        print(f"FAIL: missing schema directory {SCHEMA_DIR}", file=sys.stderr)
        return 1

    check_schema_ids(errors)

    for fixture_name, schema_name in sorted(FIXTURE_SCHEMA.items()):
        fixture_path = FIXTURE_DIR / fixture_name
        schema_path = SCHEMA_DIR / schema_name
        if not fixture_path.exists():
            errors.append(f"{fixture_name}: fixture missing")
            continue
        if not schema_path.exists():
            errors.append(f"{schema_name}: schema missing")
            continue
        schema = load_json(schema_path)
        raw = fixture_path.read_text(encoding="utf-8")
        check_secret_boundary(fixture_name, raw, errors)
        validate(json.loads(raw), schema, {"__self__": schema}, fixture_name, errors)

    check_digest_vectors(errors)
    check_prohibited_names(errors)
    check_confirmation_v2(errors)
    check_amendment_001(errors)
    check_amendment_002(errors)
    check_amendment_003(errors)
    check_amendment_004(errors)
    check_amendment_005(errors)
    check_relation_support(errors)
    check_promoted_event(errors)
    check_rejected_event(errors)
    check_receipts(errors)
    check_test_manifest(errors)
    check_contract_agreement(errors)
    check_spec_links(errors)

    if errors:
        print("SMR claim-acceptance contract validation FAILED:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    checked = len(FIXTURE_SCHEMA)
    schemas = len(list(SCHEMA_DIR.glob("*.schema.json")))
    print(
        f"SMR claim-acceptance contract OK: {schemas} schemas, {checked} fixtures, "
        f"golden digest vectors reproduced, amendments A1-A27 applicable invariants enforced."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
