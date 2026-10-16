#!/usr/bin/env python3
"""Schema checks for the hand-authored spec-registry payloads.

The payloads under protocol/spec-registry/ are forward-defining. schema_gen
does not emit them. This validator is the test plan for that contract: draft-07
shape, $ref resolution, signoff actor rules, all-or-none extract metadata, and
fail-closed approve-permission conflict cells.

Dependency-free (stdlib only), matching scripts/validate_smr_claim_acceptance_contract.py.

    python3 scripts/validate_spec_registry_schemas.py
"""

from __future__ import annotations

import itertools
import json
import re
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_DIR = ROOT / "protocol" / "spec-registry"
SCHEMA_GEN = ROOT / "crates" / "bicameral-gateway" / "tests" / "schema_gen.rs"
ID_PREFIX = "https://bicameral.dev/protocol/spec-registry/v1/"
DRAFT_07 = "http://json-schema.org/draft-07/schema#"
DATE_TIME = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$"
)

META_FIELDS = (
    "source_digest",
    "route",
    "model_id",
    "prompt_id",
    "prompt_digest",
)
META_VALUES = {
    "source_digest": "abc",
    "route": "system_one",
    "model_id": "model",
    "prompt_id": "prompt",
    "prompt_digest": "digest",
}


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def schema_store() -> dict[str, dict]:
    store: dict[str, dict] = {}
    for path in sorted(SCHEMA_DIR.glob("*.schema.json")):
        schema = load_json(path)
        schema_id = schema.get("$id")
        if not isinstance(schema_id, str):
            continue
        store[schema_id] = schema
    return store


def errors_for(value: Any, schema: Any, store: dict[str, dict], path: str = "$") -> list[str]:
    """Return draft-07 errors for the keywords the spec-registry schemas use."""
    if not isinstance(schema, dict):
        return [f"{path}: schema is not an object"]
    if "$ref" in schema:
        ref = schema["$ref"]
        target = store.get(ref)
        if target is None:
            return [f"{path}: unresolvable $ref {ref}"]
        return errors_for(value, target, store, path)

    errors: list[str] = []
    if "const" in schema and value != schema["const"]:
        errors.append(f"{path}: expected {schema['const']!r}, got {value!r}")
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: {value!r} is not one of {schema['enum']!r}")

    expected = schema.get("type")
    if expected is not None:
        type_errors = _type_errors(value, schema, store, path)
        if type_errors:
            return errors + type_errors
    elif any(key in schema for key in ("properties", "required", "additionalProperties")):
        if not isinstance(value, dict):
            return errors + [f"{path}: expected object, got {type(value).__name__}"]
        errors.extend(_object_keywords(value, schema, store, path))
    elif any(key in schema for key in ("minItems", "maxItems", "items", "contains")):
        if not isinstance(value, list):
            return errors + [f"{path}: expected array, got {type(value).__name__}"]
        errors.extend(_array_keywords(value, schema, store, path))

    errors.extend(_combinators(value, schema, store, path))
    return errors


def _type_errors(value: Any, schema: dict, store: dict[str, dict], path: str) -> list[str]:
    expected = schema["type"]
    if expected == "object":
        if not isinstance(value, dict):
            return [f"{path}: expected object, got {type(value).__name__}"]
        return _object_keywords(value, schema, store, path)
    if expected == "array":
        if not isinstance(value, list):
            return [f"{path}: expected array, got {type(value).__name__}"]
        return _array_keywords(value, schema, store, path)
    if expected == "string":
        if not isinstance(value, str):
            return [f"{path}: expected string, got {type(value).__name__}"]
        errors: list[str] = []
        if "minLength" in schema and len(value) < schema["minLength"]:
            errors.append(f"{path}: shorter than minLength {schema['minLength']}")
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            errors.append(f"{path}: longer than maxLength {schema['maxLength']}")
        pattern = schema.get("pattern")
        if pattern and re.search(pattern, value) is None:
            errors.append(f"{path}: does not match {pattern}")
        if schema.get("format") == "date-time" and DATE_TIME.fullmatch(value) is None:
            errors.append(f"{path}: is not a date-time")
        return errors
    if expected == "integer":
        if isinstance(value, bool) or not isinstance(value, int):
            return [f"{path}: expected integer, got {type(value).__name__}"]
        errors = []
        if "minimum" in schema and value < schema["minimum"]:
            errors.append(f"{path}: below minimum {schema['minimum']}")
        if "maximum" in schema and value > schema["maximum"]:
            errors.append(f"{path}: above maximum {schema['maximum']}")
        return errors
    if expected == "boolean":
        if not isinstance(value, bool):
            return [f"{path}: expected boolean, got {type(value).__name__}"]
        return []
    if expected == "number":
        return [f"{path}: number is outside the spec-registry keyword set"]
    return [f"{path}: unsupported type {expected!r}"]


def _object_keywords(value: dict, schema: dict, store: dict[str, dict], path: str) -> list[str]:
    errors: list[str] = []
    for field in schema.get("required", []):
        if field not in value:
            errors.append(f"{path}.{field}: required")
    props = schema.get("properties", {})
    if schema.get("additionalProperties") is False:
        for key in value:
            if key not in props:
                errors.append(f"{path}.{key}: additional property")
    for key, sub in props.items():
        if key in value:
            errors.extend(errors_for(value[key], sub, store, f"{path}.{key}"))
    return errors


def _array_keywords(value: list, schema: dict, store: dict[str, dict], path: str) -> list[str]:
    errors: list[str] = []
    if "minItems" in schema and len(value) < schema["minItems"]:
        errors.append(f"{path}: fewer than minItems {schema['minItems']}")
    if "maxItems" in schema and len(value) > schema["maxItems"]:
        errors.append(f"{path}: more than maxItems {schema['maxItems']}")
    item_schema = schema.get("items")
    if isinstance(item_schema, dict):
        for index, item in enumerate(value):
            errors.extend(errors_for(item, item_schema, store, f"{path}[{index}]"))
    contains = schema.get("contains")
    if isinstance(contains, dict):
        matched = sum(1 for item in value if not errors_for(item, contains, store, path))
        minimum = schema.get("minContains", 1)
        if matched < minimum:
            errors.append(f"{path}: contains matched {matched}, need {minimum}")
    return errors


def _combinators(value: Any, schema: dict, store: dict[str, dict], path: str) -> list[str]:
    errors: list[str] = []
    for index, sub in enumerate(schema.get("allOf", [])):
        errors.extend(errors_for(value, sub, store, f"{path}/allOf[{index}]"))
    any_of = schema.get("anyOf")
    if isinstance(any_of, list) and not any(not errors_for(value, sub, store, path) for sub in any_of):
        errors.append(f"{path}: anyOf failed")
    if "not" in schema and not errors_for(value, schema["not"], store, path):
        errors.append(f"{path}: not failed")
    if "if" in schema:
        if not errors_for(value, schema["if"], store, path):
            if "then" in schema:
                errors.extend(errors_for(value, schema["then"], store, f"{path}/then"))
        elif "else" in schema:
            errors.extend(errors_for(value, schema["else"], store, f"{path}/else"))
    return errors


def _accepts(value: Any, schema: dict, store: dict[str, dict]) -> bool:
    return not errors_for(value, schema, store)


def empty_domain() -> dict:
    return {
        "product_id": "product",
        "snapshot_addr": "snap",
        "claims": [],
        "atoms": [],
        "claim_assertions": [],
        "relations": [],
    }


def claim() -> dict:
    return {"claim_id": "c", "revision": 0, "context_statement": "s"}


def permission_cell(role: str, in_conflict: bool, allow: bool) -> dict:
    return {"role": role, "in_conflict": in_conflict, "allow": allow}


def policy(cells: list[dict]) -> dict:
    return {"auto_approve": True, "approve_permission": cells}


def alpha_cells(owner_allow: bool = True, member_allow: bool = True) -> list[dict]:
    return [
        permission_cell("owner", False, owner_allow),
        permission_cell("owner", True, False),
        permission_cell("member", False, member_allow),
        permission_cell("member", True, False),
    ]


def manual_signoff() -> dict:
    return {
        "snapshot_addr": "snap",
        "domain_digest": "digest",
        "daemon_id": "daemon",
        "actor": "manual",
        "role": "owner",
        "person_id": "person",
        "check_records": [
            {
                "domain_digest": "a",
                "other_domain_digest": "b",
                "conflict": False,
                "daemon_id": "daemon",
            }
        ],
    }


def policy_signoff() -> dict:
    payload = manual_signoff()
    payload["actor"] = "policy"
    payload["check_records"] = []
    del payload["role"]
    del payload["person_id"]
    return payload


def check_schema_identity(store: dict[str, dict], errors: list[str]) -> None:
    paths = sorted(SCHEMA_DIR.glob("*.schema.json"))
    if len(paths) != 13:
        errors.append(f"expected 13 spec-registry schemas, found {len(paths)}")
    for path in paths:
        schema = load_json(path)
        if schema.get("$schema") != DRAFT_07:
            errors.append(f"{path.name}: $schema is not draft-07")
        expected = f"{ID_PREFIX}{path.name}"
        if schema.get("$id") != expected:
            errors.append(f"{path.name}: $id must be {expected}")
        if expected not in store:
            errors.append(f"{path.name}: $id missing from the resolver store")
    for schema_id, schema in store.items():
        _unresolved_refs(schema, store, schema_id, errors)


def _unresolved_refs(schema: Any, store: dict[str, dict], where: str, errors: list[str]) -> None:
    if isinstance(schema, dict):
        ref = schema.get("$ref")
        if isinstance(ref, str) and ref not in store:
            errors.append(f"{where}: unresolvable $ref {ref}")
        for value in schema.values():
            _unresolved_refs(value, store, where, errors)
    elif isinstance(schema, list):
        for value in schema:
            _unresolved_refs(value, store, where, errors)


def check_schema_gen_isolation(errors: list[str]) -> None:
    text = SCHEMA_GEN.read_text(encoding="utf-8")
    if "spec-registry" in text:
        errors.append("schema_gen.rs names protocol/spec-registry")
    if 'join("../../protocol/schemas/v1")' not in text.replace(" ", ""):
        errors.append("schema_gen.rs no longer writes only protocol/schemas/v1")
    if 'join("../../protocol/schemas/v2")' not in text.replace(" ", ""):
        errors.append("schema_gen.rs no longer writes only protocol/schemas/v2")


def check_instances(store: dict[str, dict], errors: list[str]) -> None:
    def require_invalid(name: str, value: Any, schema_name: str) -> None:
        schema = store[f"{ID_PREFIX}{schema_name}"]
        if _accepts(value, schema, store):
            errors.append(f"{name}: accepted")

    def require_valid(name: str, value: Any, schema_name: str) -> None:
        schema = store[f"{ID_PREFIX}{schema_name}"]
        found = errors_for(value, schema, store)
        if found:
            errors.append(f"{name}: rejected ({found[0]})")

    domain = f"{ID_PREFIX}governed-domain.schema.json"
    signoff = "signoff.schema.json"
    approve = "approve-policy.schema.json"

    require_valid("empty capture", empty_domain(), "governed-domain.schema.json")
    completed = empty_domain()
    completed.update(META_VALUES)
    require_valid("empty completed extract", completed, "governed-domain.schema.json")
    failed = empty_domain()
    failed["extract_error"] = {"reason": "timeout", "failed_at": "2026-09-30T00:00:00Z"}
    require_valid("finished failure", failed, "governed-domain.schema.json")

    route_only = empty_domain()
    route_only["route"] = "system_one"
    require_invalid("route alone", route_only, "governed-domain.schema.json")

    for size in range(1, len(META_FIELDS)):
        for chosen in itertools.combinations(META_FIELDS, size):
            partial = empty_domain()
            for field in chosen:
                partial[field] = META_VALUES[field]
            if _accepts(partial, store[domain], store):
                errors.append(f"partial metadata {','.join(chosen)}: accepted")

    content_cases = {
        "claims": [claim()],
        "atoms": [{"atom_id": "a", "atom_kind": "k", "semantic_key": "s"}],
        "claim_assertions": [
            {
                "assertion_id": "n",
                "claim_id": "c",
                "atom_id": "a",
                "polarity": "supports",
            }
        ],
        "relations": [
            {
                "relation_id": "r",
                "relation_kind": "k",
                "from_atom_id": "a",
                "to_atom_id": "b",
            }
        ],
    }
    for field, items in content_cases.items():
        payload = empty_domain()
        payload[field] = items
        require_invalid(f"content without provenance:{field}", payload, "governed-domain.schema.json")

    mixed = empty_domain()
    mixed.update(META_VALUES)
    mixed["extract_error"] = {"reason": "timeout", "failed_at": "2026-09-30T00:00:00Z"}
    require_invalid("completion with extract_error", mixed, "governed-domain.schema.json")
    claimed_failure = empty_domain()
    claimed_failure["extract_error"] = {
        "reason": "timeout",
        "failed_at": "2026-09-30T00:00:00Z",
    }
    claimed_failure["claims"] = [claim()]
    require_invalid("failure with claims", claimed_failure, "governed-domain.schema.json")

    require_valid("manual signoff", manual_signoff(), signoff)
    require_valid("policy signoff", policy_signoff(), signoff)
    missing_person = manual_signoff()
    del missing_person["person_id"]
    require_invalid("manual signoff without person_id", missing_person, signoff)
    with_role = policy_signoff()
    with_role["role"] = "owner"
    require_invalid("policy signoff with role", with_role, signoff)
    with_person = policy_signoff()
    with_person["person_id"] = "person"
    require_invalid("policy signoff with person_id", with_person, signoff)

    require_valid("alpha policy", policy(alpha_cells()), approve)
    require_valid(
        "non-conflict allow false",
        policy(alpha_cells(owner_allow=False, member_allow=False)),
        approve,
    )
    for role in ("owner", "member"):
        cells = alpha_cells()
        for cell in cells:
            if cell["role"] == role and cell["in_conflict"] is True:
                cell["allow"] = True
        require_invalid(f"{role} conflict allow true", policy(cells), approve)

    keys = [("owner", False), ("owner", True), ("member", False), ("member", True)]
    for duplicated in keys:
        for dropped in keys:
            if dropped == duplicated:
                continue
            cells = []
            for role, in_conflict in keys:
                if (role, in_conflict) == dropped:
                    role, in_conflict = duplicated
                allow = False if in_conflict else True
                cells.append(permission_cell(role, in_conflict, allow))
            require_invalid(
                f"duplicate {duplicated} drops {dropped}",
                policy(cells),
                approve,
            )

    require_valid(
        "spec source",
        {"uri": "file:///spec.md", "source_type": "markdown", "source_kind": "file"},
        "spec-source.schema.json",
    )
    require_valid(
        "source snapshot",
        {
            "snapshot_addr": "snap",
            "snapshot_ref": "ref",
            "captured_at": "2026-09-30T00:00:00Z",
            "source_uri": "file:///spec.md",
        },
        "source-snapshot.schema.json",
    )
    actor = {"source_uri": "file:///spec.md", "snapshot_addr": "snap", "person_id": "person", "daemon_id": "daemon"}
    require_valid("track", actor, "track-source.schema.json")
    require_valid("untrack", actor, "untrack-source.schema.json")
    require_valid("reject", actor, "reject-snapshot.schema.json")
    supersede = dict(actor)
    supersede["successor_snapshot_addr"] = "next"
    require_valid("supersede", supersede, "supersede-snapshot.schema.json")
    missing_actor = dict(actor)
    del missing_actor["person_id"]
    require_invalid("track without person_id", missing_actor, "track-source.schema.json")
    require_valid(
        "resolve_conflict",
        {
            "person_id": "owner",
            "daemon_id": "daemon",
            "members": [
                {"source_uri": "a", "snapshot_addr": "s1", "state": "approved"},
                {"source_uri": "b", "snapshot_addr": "s2", "state": "rejected"},
            ],
        },
        "resolve-conflict.schema.json",
    )
    require_invalid(
        "resolve_conflict without person_id",
        {
            "daemon_id": "daemon",
            "members": [
                {"source_uri": "a", "snapshot_addr": "s1", "state": "approved"},
                {"source_uri": "b", "snapshot_addr": "s2", "state": "rejected"},
            ],
        },
        "resolve-conflict.schema.json",
    )
    require_valid(
        "prompt version",
        {
            "prompt_id": "prompt",
            "route": "system_one",
            "bytes": "extract",
            "sha256": "sha256:" + ("ab" * 32),
        },
        "prompt-version.schema.json",
    )
    require_valid(
        "prompt assignment",
        {"route": "openai_compatible", "prompt_id": "prompt"},
        "prompt-assignment.schema.json",
    )


def run_checks() -> list[str]:
    errors: list[str] = []
    store = schema_store()
    check_schema_identity(store, errors)
    check_schema_gen_isolation(errors)
    check_instances(store, errors)
    return errors


def main() -> int:
    errors = run_checks()
    if errors:
        print(f"spec-registry schema check failed ({len(errors)})")
        for error in errors:
            print(f"- {error}")
        return 1
    print("spec-registry schema check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
