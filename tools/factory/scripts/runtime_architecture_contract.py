#!/usr/bin/env python3
"""Validate and project canonical runtime architecture contracts.

The implementation is stdlib-only so Product repositories can run the same
fail-closed referential-integrity checks without installing a renderer or a
JSON-Schema package. The JSON Schema remains the exact authoring contract; the
constants below are checked against it by the contract test to prevent drift.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SCHEMA = ROOT / "schemas/runtime-architecture-contract.schema.json"

TOP_LEVEL_FIELDS = {
    "$schema",
    "schema_version",
    "spec_id",
    "title",
    "components",
    "relationships",
    "states",
    "transitions",
    "scenarios",
    "conformance",
}

COLLECTION_DEFS = {
    "components": "runtime_component",
    "relationships": "runtime_relationship",
    "states": "runtime_state",
    "transitions": "runtime_transition",
    "scenarios": "runtime_scenario",
    "conformance": "runtime_conformance",
}

REQUIRED_FIELDS = {
    "components": {
        "id",
        "c4_kind",
        "name",
        "responsibility",
        "process_boundary",
        "repository_runtime",
        "deployment_location",
        "authority_role",
        "inputs",
        "outputs",
        "state_ids",
        "multiplicity",
        "prohibited_authority",
    },
    "relationships": {
        "id",
        "source_id",
        "destination_id",
        "interaction",
        "medium",
        "handoff",
        "state_ids",
        "payloads",
        "authority_boundaries",
    },
    "states": {
        "id",
        "artifact",
        "classification",
        "authoritative_writer_ids",
        "persistence_owner_ids",
        "persistence_location",
        "reader_ids",
        "routing_key",
        "mutation_path",
        "lifecycle",
        "prohibited_fallbacks",
    },
    "transitions": {
        "id",
        "trigger",
        "actor_ids",
        "source_state_id",
        "destination_state_id",
        "relationship_ids",
        "persistence_effect",
    },
    "scenarios": {
        "id",
        "name",
        "trigger",
        "participant_ids",
        "relationship_ids",
        "state_read_ids",
        "state_write_ids",
        "transition_ids",
        "authority_boundaries",
        "expected_result",
        "failure_behavior",
    },
    "conformance": {
        "id",
        "status",
        "source_ids",
        "owner_module_or_seam",
        "required_behavior",
        "prohibited_fallback",
        "validation_obligation",
        "implementation_evidence",
    },
}

ID_PATTERNS = {
    "components": re.compile(r"^ELM-[A-Z0-9-]+$"),
    "relationships": re.compile(r"^REL-[A-Z0-9-]+$"),
    "states": re.compile(r"^STATE-[A-Z0-9-]+$"),
    "transitions": re.compile(r"^TRANS-[A-Z0-9-]+$"),
    "scenarios": re.compile(r"^SCN-[A-Z0-9-]+$"),
    "conformance": re.compile(r"^CONF-[A-Z0-9-]+$"),
}

STRING_FIELDS = {
    "components": {
        "name",
        "responsibility",
        "process_boundary",
        "repository_runtime",
        "deployment_location",
        "authority_role",
        "multiplicity",
    },
    "relationships": {"interaction", "medium", "handoff"},
    "states": {
        "artifact",
        "persistence_location",
        "routing_key",
        "mutation_path",
        "lifecycle",
    },
    "transitions": {"trigger", "persistence_effect"},
    "scenarios": {"name", "trigger", "expected_result", "failure_behavior"},
    "conformance": {
        "owner_module_or_seam",
        "required_behavior",
        "prohibited_fallback",
    },
}

ARRAY_FIELDS = {
    "components": {"inputs", "outputs", "state_ids", "prohibited_authority"},
    "relationships": {"state_ids", "payloads", "authority_boundaries"},
    "states": {
        "authoritative_writer_ids",
        "persistence_owner_ids",
        "reader_ids",
        "prohibited_fallbacks",
    },
    "transitions": {"actor_ids", "relationship_ids"},
    "scenarios": {
        "participant_ids",
        "relationship_ids",
        "state_read_ids",
        "state_write_ids",
        "transition_ids",
        "authority_boundaries",
    },
    "conformance": {"source_ids", "implementation_evidence"},
}

ALLOWED_C4_KINDS = {"person", "software_system", "container", "component", "datastore"}
ALLOWED_STATE_CLASSIFICATIONS = {
    "canonical_state",
    "accepted_event",
    "evidence",
    "candidate_or_proposal",
    "derived_projection_or_read_model",
    "cache",
    "local_scratch",
    "audit_receipt_or_log",
}
ALLOWED_CONFORMANCE_STATUS = {"proven", "partial", "missing", "unresolved"}
ALLOWED_VALIDATION_KINDS = {
    "unit",
    "integration",
    "contract",
    "topology",
    "restart_replay",
    "manual",
}


class ContractValidationError(ValueError):
    """Raised when projection is attempted from an invalid contract."""


def load_json(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise ContractValidationError(
            f"missing runtime architecture contract: {path}"
        ) from exc
    except json.JSONDecodeError as exc:
        raise ContractValidationError(f"invalid JSON in {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise ContractValidationError(
            "runtime architecture contract must be a JSON object"
        )
    return data


def _non_empty_string(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def _validate_string_array(value: Any, path: str, errors: list[str]) -> list[str]:
    if not isinstance(value, list):
        errors.append(f"{path} must be an array")
        return []
    if any(not _non_empty_string(item) for item in value):
        errors.append(f"{path} must contain non-empty strings")
    if len(value) != len(set(item for item in value if isinstance(item, str))):
        errors.append(f"{path} must not contain duplicate values")
    return [item for item in value if isinstance(item, str)]


def validate_schema_alignment(schema: dict[str, Any]) -> list[str]:
    """Prove the checked field names match the canonical JSON Schema."""

    errors: list[str] = []
    if set(schema.get("required", [])) != TOP_LEVEL_FIELDS:
        errors.append(
            "schema top-level required fields do not match validator contract"
        )
    if set(schema.get("properties", {})) != TOP_LEVEL_FIELDS:
        errors.append("schema top-level properties do not match validator contract")

    definitions = schema.get("$defs", {})
    for collection, definition_name in COLLECTION_DEFS.items():
        definition = definitions.get(definition_name, {})
        if set(definition.get("required", [])) != REQUIRED_FIELDS[collection]:
            errors.append(
                f"schema {definition_name}.required does not match validator contract"
            )
        if set(definition.get("properties", {})) != REQUIRED_FIELDS[collection]:
            errors.append(
                f"schema {definition_name}.properties does not match validator contract"
            )

    enum_contracts = {
        "runtime_component.c4_kind": (
            definitions.get("runtime_component", {})
            .get("properties", {})
            .get("c4_kind", {})
            .get("enum", []),
            ALLOWED_C4_KINDS,
        ),
        "runtime_state.classification": (
            definitions.get("runtime_state", {})
            .get("properties", {})
            .get("classification", {})
            .get("enum", []),
            ALLOWED_STATE_CLASSIFICATIONS,
        ),
        "runtime_conformance.status": (
            definitions.get("runtime_conformance", {})
            .get("properties", {})
            .get("status", {})
            .get("enum", []),
            ALLOWED_CONFORMANCE_STATUS,
        ),
        "runtime_conformance.validation_obligation.kind": (
            definitions.get("runtime_conformance", {})
            .get("properties", {})
            .get("validation_obligation", {})
            .get("properties", {})
            .get("kind", {})
            .get("enum", []),
            ALLOWED_VALIDATION_KINDS,
        ),
    }
    for name, (schema_values, validator_values) in enum_contracts.items():
        if set(schema_values) != validator_values:
            errors.append(f"schema {name} enum does not match validator contract")

    for collection, pattern in ID_PATTERNS.items():
        definition = definitions.get(COLLECTION_DEFS[collection], {})
        id_schema = definition.get("properties", {}).get("id", {})
        schema_pattern = id_schema.get("pattern")
        if schema_pattern is None and "$ref" in id_schema:
            referenced_name = id_schema["$ref"].rsplit("/", 1)[-1]
            schema_pattern = definitions.get(referenced_name, {}).get("pattern")
        if schema_pattern != pattern.pattern:
            errors.append(
                f"schema {COLLECTION_DEFS[collection]}.id pattern does not match validator"
            )
    return errors


def validate_contract(data: dict[str, Any]) -> list[str]:
    errors: list[str] = []

    missing_top = TOP_LEVEL_FIELDS - set(data)
    extra_top = set(data) - TOP_LEVEL_FIELDS
    if missing_top:
        errors.append(
            f"contract missing top-level field(s): {', '.join(sorted(missing_top))}"
        )
    if extra_top:
        errors.append(
            f"contract has undeclared top-level field(s): {', '.join(sorted(extra_top))}"
        )
    if type(data.get("schema_version")) is not int or data.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if not _non_empty_string(data.get("$schema")):
        errors.append("$schema must be a non-empty schema reference")
    spec_id = data.get("spec_id")
    if not isinstance(spec_id, str) or not re.fullmatch(
        r"RUNTIME-SPEC-[A-Z0-9-]+", spec_id
    ):
        errors.append("spec_id must match RUNTIME-SPEC-[A-Z0-9-]+")
    if not _non_empty_string(data.get("title")):
        errors.append("title must be a non-empty string")

    records: dict[str, list[dict[str, Any]]] = {}
    declared_ids: dict[str, str] = {}
    ids_by_collection: dict[str, set[str]] = {name: set() for name in COLLECTION_DEFS}

    for collection in COLLECTION_DEFS:
        value = data.get(collection)
        if not isinstance(value, list):
            errors.append(f"{collection} must be an array")
            records[collection] = []
            continue
        if collection == "components" and not value:
            errors.append("components must contain at least one record")
        records[collection] = []
        for index, item in enumerate(value):
            path = f"{collection}[{index}]"
            if not isinstance(item, dict):
                errors.append(f"{path} must be an object")
                continue
            records[collection].append(item)
            missing = REQUIRED_FIELDS[collection] - set(item)
            extra = set(item) - REQUIRED_FIELDS[collection]
            if missing:
                errors.append(f"{path} missing field(s): {', '.join(sorted(missing))}")
            if extra:
                errors.append(
                    f"{path} has undeclared field(s): {', '.join(sorted(extra))}"
                )

            record_id = item.get("id")
            if not isinstance(record_id, str) or not ID_PATTERNS[collection].fullmatch(
                record_id
            ):
                errors.append(f"{path}.id has invalid {collection} id")
            elif record_id in declared_ids:
                errors.append(
                    f"duplicate id {record_id} in {path}; first declared in {declared_ids[record_id]}"
                )
            else:
                declared_ids[record_id] = path
                ids_by_collection[collection].add(record_id)

            for field in STRING_FIELDS[collection]:
                if field in item and not _non_empty_string(item[field]):
                    errors.append(f"{path}.{field} must be a non-empty string")
            for field in ARRAY_FIELDS[collection]:
                if field in item:
                    _validate_string_array(item[field], f"{path}.{field}", errors)

    if any(
        not isinstance(item.get("c4_kind"), str)
        or item.get("c4_kind") not in ALLOWED_C4_KINDS
        for item in records["components"]
    ):
        errors.append("components[].c4_kind contains an unsupported value")
    if any(
        not isinstance(item.get("classification"), str)
        or item.get("classification") not in ALLOWED_STATE_CLASSIFICATIONS
        for item in records["states"]
    ):
        errors.append("states[].classification contains an unsupported value")
    if any(
        not isinstance(item.get("status"), str)
        or item.get("status") not in ALLOWED_CONFORMANCE_STATUS
        for item in records["conformance"]
    ):
        errors.append("conformance[].status contains an unsupported value")

    element_ids = ids_by_collection["components"]
    relationship_ids = ids_by_collection["relationships"]
    state_ids = ids_by_collection["states"]
    transition_ids = ids_by_collection["transitions"]
    source_ids = (
        element_ids
        | relationship_ids
        | state_ids
        | transition_ids
        | ids_by_collection["scenarios"]
    )

    def require_refs(values: Any, allowed: set[str], path: str) -> None:
        for value in _validate_string_array(values, path, errors):
            if value not in allowed:
                errors.append(f"{path} references undeclared id {value}")

    for index, item in enumerate(records["components"]):
        require_refs(item.get("state_ids"), state_ids, f"components[{index}].state_ids")

    for index, item in enumerate(records["relationships"]):
        for field in ("source_id", "destination_id"):
            value = item.get(field)
            if not isinstance(value, str) or value not in element_ids:
                errors.append(
                    f"relationships[{index}].{field} references undeclared ELM id {value}"
                )
        require_refs(
            item.get("state_ids"), state_ids, f"relationships[{index}].state_ids"
        )

    for index, item in enumerate(records["states"]):
        for field in (
            "authoritative_writer_ids",
            "persistence_owner_ids",
            "reader_ids",
        ):
            values = item.get(field)
            require_refs(values, element_ids, f"states[{index}].{field}")
            if field != "reader_ids" and isinstance(values, list) and not values:
                errors.append(
                    f"states[{index}].{field} must contain at least one ELM id"
                )

    for index, item in enumerate(records["transitions"]):
        require_refs(
            item.get("actor_ids"), element_ids, f"transitions[{index}].actor_ids"
        )
        if isinstance(item.get("actor_ids"), list) and not item["actor_ids"]:
            errors.append(
                f"transitions[{index}].actor_ids must contain at least one ELM id"
            )
        require_refs(
            item.get("relationship_ids"),
            relationship_ids,
            f"transitions[{index}].relationship_ids",
        )
        for field in ("source_state_id", "destination_state_id"):
            value = item.get(field)
            if value is not None and (
                not isinstance(value, str) or value not in state_ids
            ):
                errors.append(
                    f"transitions[{index}].{field} references undeclared STATE id {value}"
                )
        if (
            item.get("source_state_id") is None
            and item.get("destination_state_id") is None
        ):
            errors.append(
                f"transitions[{index}] must reference a source or destination STATE id"
            )

    for index, item in enumerate(records["scenarios"]):
        require_refs(
            item.get("participant_ids"),
            element_ids,
            f"scenarios[{index}].participant_ids",
        )
        require_refs(
            item.get("relationship_ids"),
            relationship_ids,
            f"scenarios[{index}].relationship_ids",
        )
        require_refs(
            item.get("state_read_ids"), state_ids, f"scenarios[{index}].state_read_ids"
        )
        require_refs(
            item.get("state_write_ids"),
            state_ids,
            f"scenarios[{index}].state_write_ids",
        )
        require_refs(
            item.get("transition_ids"),
            transition_ids,
            f"scenarios[{index}].transition_ids",
        )
        if (
            isinstance(item.get("participant_ids"), list)
            and not item["participant_ids"]
        ):
            errors.append(
                f"scenarios[{index}].participant_ids must contain at least one ELM id"
            )

    for index, item in enumerate(records["conformance"]):
        require_refs(
            item.get("source_ids"), source_ids, f"conformance[{index}].source_ids"
        )
        if isinstance(item.get("source_ids"), list) and not item["source_ids"]:
            errors.append(
                f"conformance[{index}].source_ids must contain at least one declared id"
            )
        obligation = item.get("validation_obligation")
        if not isinstance(obligation, dict):
            errors.append(
                f"conformance[{index}].validation_obligation must be an object"
            )
        else:
            if set(obligation) != {"kind", "description"}:
                errors.append(
                    f"conformance[{index}].validation_obligation must contain exactly kind and description"
                )
            if obligation.get("kind") not in ALLOWED_VALIDATION_KINDS:
                errors.append(
                    f"conformance[{index}].validation_obligation.kind is unsupported"
                )
            if not _non_empty_string(obligation.get("description")):
                errors.append(
                    f"conformance[{index}].validation_obligation.description must be non-empty"
                )

    return errors


def project_contract(data: dict[str, Any]) -> dict[str, Any]:
    """Produce a renderer-neutral projection without inventing graph ids."""

    errors = validate_contract(data)
    if errors:
        raise ContractValidationError("; ".join(errors))

    relationships = {item["id"]: item for item in data["relationships"]}
    state_ids = {item["id"] for item in data["states"]}

    c4_nodes = [
        {
            "id": item["id"],
            "source_contract_id": item["id"],
            "kind": item["c4_kind"],
            "label": item["name"],
        }
        for item in data["components"]
    ]
    c4_edges = [
        {
            "id": item["id"],
            "source_contract_id": item["id"],
            "source_id": item["source_id"],
            "destination_id": item["destination_id"],
            "label": item["interaction"],
        }
        for item in data["relationships"]
    ]
    state_nodes = [
        {
            "id": item["id"],
            "source_contract_id": item["id"],
            "label": item["artifact"],
            "classification": item["classification"],
            "authoritative_writer_ids": item["authoritative_writer_ids"],
            "persistence_owner_ids": item["persistence_owner_ids"],
            "routing_key": item["routing_key"],
        }
        for item in data["states"]
    ]
    state_edges = [
        {
            "id": item["id"],
            "source_contract_id": item["id"],
            "source_id": item["source_state_id"],
            "destination_id": item["destination_state_id"],
            "label": item["trigger"],
        }
        for item in data["transitions"]
        if item["source_state_id"] in state_ids
        and item["destination_state_id"] in state_ids
    ]
    scenario_projections = []
    for scenario in data["scenarios"]:
        scenario_projections.append(
            {
                "id": scenario["id"],
                "source_contract_id": scenario["id"],
                "edges": [
                    {
                        "id": relationship_id,
                        "source_contract_id": relationship_id,
                        "source_id": relationships[relationship_id]["source_id"],
                        "destination_id": relationships[relationship_id][
                            "destination_id"
                        ],
                    }
                    for relationship_id in scenario["relationship_ids"]
                ],
            }
        )

    return {
        "schema_version": 1,
        "source_spec_id": data["spec_id"],
        "c4": {"nodes": c4_nodes, "edges": c4_edges},
        "state": {"nodes": state_nodes, "edges": state_edges},
        "scenarios": scenario_projections,
    }


def _print_errors(path: Path, errors: list[str]) -> None:
    for error in errors:
        print(f"{path}: {error}", file=sys.stderr)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser(
        "validate", help="Validate one or more contracts"
    )
    validate_parser.add_argument("paths", nargs="+", type=Path)
    validate_parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    project_parser = subparsers.add_parser("project", help="Emit renderer-neutral JSON")
    project_parser.add_argument("path", type=Path)
    args = parser.parse_args(argv)

    try:
        schema = load_json(
            args.schema if args.command == "validate" else DEFAULT_SCHEMA
        )
    except ContractValidationError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    schema_errors = validate_schema_alignment(schema)
    if schema_errors:
        _print_errors(DEFAULT_SCHEMA, schema_errors)
        return 1

    if args.command == "validate":
        failed = False
        for path in args.paths:
            try:
                contract = load_json(path)
            except ContractValidationError as exc:
                print(str(exc), file=sys.stderr)
                failed = True
                continue
            errors = validate_contract(contract)
            if errors:
                _print_errors(path, errors)
                failed = True
            else:
                print(f"Runtime architecture contract OK: {path}")
        return 1 if failed else 0

    try:
        projection = project_contract(load_json(args.path))
    except ContractValidationError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    print(json.dumps(projection, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
