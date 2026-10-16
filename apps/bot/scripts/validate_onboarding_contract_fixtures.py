#!/usr/bin/env python3
"""Deterministic validator for the domain-Product onboarding contract fixtures.

Validates the cross-repo onboarding contract fixtures under
``protocol/onboarding/fixtures/`` against their hand-authored contract-spec JSON
Schemas under ``protocol/onboarding/schemas/`` and enforces the invariants from
``docs/specs/domain-product-onboarding-contract.md`` (issue #765):

* every fixture is structurally valid against its schema;
* every fixture round-trips deterministically as canonical JSON (sorted keys,
  2-space indent, trailing newline) — i.e. serialization is deterministic;
* ``ProductJoinTicket`` fixtures carry ONLY the minimum bounded claims
  (fail-closed: any extra claim key is rejected);
* ``ProductJoinResult`` fixtures obey the granted/rejected invariants (granted =>
  Product Member only + canonical refs + no failure; rejected => typed failure +
  no role/membership).

Dependency-free (stdlib only) so the bot and consumer repos (cloud, runtime
slices) can run it without extra tooling. Draft-07 support is intentionally a
pragmatic subset (the keywords these fixtures/schemas use).

Usage:
    python3 scripts/validate_onboarding_contract_fixtures.py
    python3 scripts/validate_onboarding_contract_fixtures.py --write   # canonicalize fixtures
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "protocol" / "onboarding"
SCHEMA_DIR = BASE / "schemas"
FIXTURE_DIR = BASE / "fixtures"
MANIFEST_PATH = BASE / "manifest.json"

# Versioned contract identity. The semantic fingerprint below is derived from
# this version plus the canonical schema/fixture inputs; consumers (cloud,
# runtime) pin the version + fingerprint, not the SPA bundle digest.
CONTRACT_ID = "domain-product-onboarding-contract"
CONTRACT_VERSION = "1.0.0"
# Every authoritative schema carries a stable, versioned `$id` under this prefix.
SCHEMA_ID_PREFIX = "https://bicameral.dev/protocol/onboarding/v1/"

# fixture filename -> schema filename
FIXTURE_SCHEMA = {
    "hosted-project-preview.valid.json": "hosted-project-preview.schema.json",
    "product-join-ticket.valid.json": "product-join-ticket.schema.json",
    "product-creation-result.valid.json": "product-creation-result.schema.json",
    "create-product-ledger-event.valid.json": "create-product-ledger-event.schema.json",
    "product-creation-publication.valid.json": "product-creation-publication.schema.json",
    "product-creation-publication.retry.json": "product-creation-publication.schema.json",
    "product-creation-publication.with-proof.json": "product-creation-publication.schema.json",
    "product-join-request.valid.json": "product-join-request.schema.json",
    "product-join-result.granted.json": "product-join-result.schema.json",
    "product-join-result.invalid-signature.json": "product-join-result.schema.json",
    "product-join-result.wrong-audience.json": "product-join-result.schema.json",
    "product-join-result.wrong-product.json": "product-join-result.schema.json",
    "product-join-result.wrong-account.json": "product-join-result.schema.json",
    "product-join-result.missing-pairing.json": "product-join-result.schema.json",
    "product-join-result.wrong-domain.json": "product-join-result.schema.json",
    "product-join-result.policy-changed.json": "product-join-result.schema.json",
    "product-join-result.expired-ticket.json": "product-join-result.schema.json",
    "product-join-result.not-yet-valid.json": "product-join-result.schema.json",
    "product-join-result.replayed-ticket.json": "product-join-result.schema.json",
}

TICKET_ALLOWED_CLAIMS = {
    "issuer",
    "audience",
    "product_id",
    "cloud_account_id",
    "verified_domain",
    "issued_at",
    "expiry",
    "nonce",
    "registration_ref",
    "provider_hint",
    "substrate_hint",
}

FAILURE_CODES = {
    "invalid_signature",
    "wrong_audience",
    "wrong_product",
    "wrong_account",
    "missing_pairing",
    "wrong_domain",
    "policy_changed",
    "expired_ticket",
    "not_yet_valid",
    "replayed_ticket",
}


class SchemaError(Exception):
    pass


def canonical(data: Any) -> str:
    return json.dumps(data, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


# --- minimal draft-07 subset validator ------------------------------------

def _type_ok(value: Any, expected: Any) -> bool:
    types = expected if isinstance(expected, list) else [expected]
    for t in types:
        if t == "object" and isinstance(value, dict):
            return True
        if t == "array" and isinstance(value, list):
            return True
        if t == "string" and isinstance(value, str):
            return True
        if t == "boolean" and isinstance(value, bool):
            return True
        if t == "null" and value is None:
            return True
        if t == "integer" and isinstance(value, int) and not isinstance(value, bool):
            return True
        if t == "number" and isinstance(value, (int, float)) and not isinstance(value, bool):
            return True
    return False


def validate(value: Any, schema: dict, store: dict[str, dict], path: str, errors: list[str]) -> None:
    if "$ref" in schema:
        ref = schema["$ref"]
        target = store.get(ref)
        if target is None:
            errors.append(f"{path}: unresolved $ref {ref}")
            return
        validate(value, target, store, path, errors)
        return

    if "const" in schema and value != schema["const"]:
        errors.append(f"{path}: expected const {schema['const']!r}, got {value!r}")

    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: {value!r} not in enum {schema['enum']!r}")

    if "type" in schema and not _type_ok(value, schema["type"]):
        errors.append(f"{path}: expected type {schema['type']!r}, got {type(value).__name__}")
        return

    if isinstance(value, dict):
        props = schema.get("properties", {})
        for req in schema.get("required", []):
            if req not in value:
                errors.append(f"{path}: missing required property '{req}'")
        if schema.get("additionalProperties", True) is False:
            for key in value:
                if key not in props:
                    errors.append(f"{path}: additional property '{key}' not allowed")
        for key, sub in props.items():
            if key in value:
                validate(value[key], sub, store, f"{path}.{key}", errors)

    if isinstance(value, list):
        items = schema.get("items")
        if isinstance(items, dict):
            for i, item in enumerate(value):
                validate(item, items, store, f"{path}[{i}]", errors)
        if "minItems" in schema and len(value) < schema["minItems"]:
            errors.append(f"{path}: fewer than minItems {schema['minItems']}")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            errors.append(f"{path}: more than maxItems {schema['maxItems']}")
        if schema.get("uniqueItems") and len(value) != len({json.dumps(v, sort_keys=True) for v in value}):
            errors.append(f"{path}: items not unique")

    if "oneOf" in schema:
        matches = 0
        for branch in schema["oneOf"]:
            branch_errors: list[str] = []
            validate(value, branch, store, path, branch_errors)
            if not branch_errors:
                matches += 1
        if matches != 1:
            errors.append(f"{path}: matched {matches} oneOf branches (expected exactly 1)")


def load_schema_store() -> dict[str, dict]:
    store: dict[str, dict] = {}
    for schema_path in SCHEMA_DIR.glob("*.schema.json"):
        store[schema_path.name] = load_json(schema_path)
    return store


# --- extra invariant checks -------------------------------------------------

def check_ticket_claims(data: dict, errors: list[str]) -> None:
    extra = set(data) - TICKET_ALLOWED_CLAIMS
    if extra:
        errors.append(f"ProductJoinTicket carries claims outside the bounded set: {sorted(extra)}")


def check_join_result(name: str, data: dict, errors: list[str]) -> None:
    outcome = data.get("outcome")
    if outcome == "granted":
        if data.get("granted_role") != "Product Member":
            errors.append(f"{name}: granted result must grant 'Product Member' only")
        if not data.get("membership_event_ref"):
            errors.append(f"{name}: granted result must carry a canonical membership_event_ref")
        if data.get("failure") is not None:
            errors.append(f"{name}: granted result must not carry a failure")
    elif outcome == "rejected":
        if data.get("granted_role") is not None:
            errors.append(f"{name}: rejected result must not grant a role")
        if data.get("membership_event_ref") is not None:
            errors.append(f"{name}: rejected result must not carry a membership_event_ref")
        failure = data.get("failure")
        if not isinstance(failure, dict) or failure.get("code") not in FAILURE_CODES:
            errors.append(f"{name}: rejected result must carry a typed failure code from {sorted(FAILURE_CODES)}")


# --- versioned $id, secret boundary, creation-binding invariants ----------

# Substrings that must never cross the contract boundary in a fixture: provider
# secrets, Supabase tokens, raw event-store credentials, and local filesystem
# paths. Mirrors the runtime `FORBIDDEN_RECEIPT_SUBSTRINGS` guard.
FORBIDDEN_SUBSTRINGS = (
    "eyj",
    "sb-",
    "supabase",
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


def check_fixture_schema_coverage(errors: list[str]) -> None:
    """Every on-disk fixture and schema must be in FIXTURE_SCHEMA.

    This is the drift gate that previously let CreateProduct's ledger file
    exist in Rust without a protocol schema: unmapped files were invisible.
    """
    on_disk_fixtures = {p.name for p in FIXTURE_DIR.glob("*.json")}
    mapped_fixtures = set(FIXTURE_SCHEMA)
    for name in sorted(on_disk_fixtures - mapped_fixtures):
        errors.append(
            f"{name}: fixture on disk is not in FIXTURE_SCHEMA — map it or remove it"
        )
    for name in sorted(mapped_fixtures - on_disk_fixtures):
        errors.append(f"{name}: FIXTURE_SCHEMA entry has no file under fixtures/")
    mapped_schemas = set(FIXTURE_SCHEMA.values())
    on_disk_schemas = {p.name for p in SCHEMA_DIR.glob("*.schema.json")}
    for name in sorted(on_disk_schemas - mapped_schemas):
        errors.append(
            f"{name}: schema has no fixture mapping in FIXTURE_SCHEMA (drift)"
        )
    for name in sorted(mapped_schemas - on_disk_schemas):
        errors.append(f"{name}: FIXTURE_SCHEMA references a missing schema file")


def check_schema_ids(errors: list[str]) -> None:
    """Every authoritative schema must carry a stable, versioned `$id`."""
    for schema_path in sorted(SCHEMA_DIR.glob("*.schema.json")):
        schema = load_json(schema_path)
        schema_id = schema.get("$id")
        expected = f"{SCHEMA_ID_PREFIX}{schema_path.name}"
        if schema_id != expected:
            errors.append(
                f"{schema_path.name}: $id must be the versioned {expected!r}, got {schema_id!r}"
            )


def check_secret_boundary(fixture_name: str, raw: str, errors: list[str]) -> None:
    lower = raw.lower()
    for needle in FORBIDDEN_SUBSTRINGS:
        if needle in lower:
            errors.append(
                f"{fixture_name}: fixture carries forbidden boundary material "
                f"(matched {needle!r}); no Supabase token, provider secret, or local "
                f"path may cross the contract boundary"
            )


def check_creation_bindings(fixtures: dict[str, Any], errors: list[str]) -> None:
    """Prove creator-account publication binding + stable publication idempotency."""
    result = fixtures.get("product-creation-result.valid.json")
    base = fixtures.get("product-creation-publication.valid.json")
    retry = fixtures.get("product-creation-publication.retry.json")

    if isinstance(result, dict):
        roles = result.get("creator_roles")
        if roles != ["Policy Owner", "Product Owner"]:
            errors.append(
                "product-creation-result.valid.json: creator_roles must be exactly "
                "['Policy Owner', 'Product Owner'] (atomic creator materialization)"
            )

    if isinstance(result, dict) and isinstance(base, dict):
        if base.get("product_id") != result.get("product_id"):
            errors.append(
                "product-creation-publication.valid.json: product_id must equal the "
                "canonical ProductCreationResult product_id"
            )
        if base.get("creator_cloud_account_id") != result.get("creator_cloud_account_id"):
            errors.append(
                "product-creation-publication.valid.json: creator_cloud_account_id must "
                "bind to the ProductCreationResult creator_cloud_account_id"
            )

    if isinstance(base, dict) and isinstance(retry, dict):
        if retry.get("product_id") != base.get("product_id"):
            errors.append(
                "product-creation-publication.retry.json: retry must preserve the "
                "original product_id (never a second Product)"
            )
        if retry.get("idempotency_key") != base.get("idempotency_key"):
            errors.append(
                "product-creation-publication.retry.json: retry must preserve the "
                "original idempotency_key"
            )
        if retry.get("publication_id") == base.get("publication_id"):
            errors.append(
                "product-creation-publication.retry.json: retry must be a distinct "
                "publication row (publication_id must differ)"
            )


# --- deterministic semantic-fingerprint manifest --------------------------

def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_map(directory: Path, names: list[str]) -> dict[str, str]:
    return {name: sha256_hex((directory / name).read_bytes()) for name in sorted(names)}


def fingerprint_material() -> dict[str, Any]:
    """Canonical semantic inputs the fingerprint is derived from.

    Deterministic and source-commit independent: schema + fixture content
    digests plus the declared contract version.
    """
    schema_names = [p.name for p in SCHEMA_DIR.glob("*.schema.json")]
    fixture_names = list(FIXTURE_SCHEMA)
    return {
        "contract": CONTRACT_ID,
        "contract_version": CONTRACT_VERSION,
        "schemas": digest_map(SCHEMA_DIR, schema_names),
        "fixtures": digest_map(FIXTURE_DIR, fixture_names),
    }


def semantic_fingerprint(material: dict[str, Any]) -> str:
    return sha256_hex(canonical(material).encode("utf-8"))


def source_commit() -> str:
    try:
        out = subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        return out.stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def build_manifest() -> dict[str, Any]:
    material = fingerprint_material()
    return {
        "contract": CONTRACT_ID,
        "contract_version": CONTRACT_VERSION,
        "source_commit": source_commit(),
        "semantic_fingerprint": semantic_fingerprint(material),
        "schemas": material["schemas"],
        "fixtures": material["fixtures"],
    }


def check_manifest(write: bool, errors: list[str]) -> None:
    material = fingerprint_material()
    expected_fingerprint = semantic_fingerprint(material)

    if write:
        MANIFEST_PATH.write_text(canonical(build_manifest()), encoding="utf-8")
        return

    if not MANIFEST_PATH.exists():
        errors.append(
            "manifest.json: missing semantic-fingerprint manifest — run with --write"
        )
        return

    manifest = load_json(MANIFEST_PATH)
    if manifest.get("contract_version") != CONTRACT_VERSION:
        errors.append(
            f"manifest.json: contract_version drift — expected {CONTRACT_VERSION!r}, "
            f"got {manifest.get('contract_version')!r}"
        )
    for kind in ("schemas", "fixtures"):
        recorded = manifest.get(kind, {})
        actual = material[kind]
        for name, digest in actual.items():
            if recorded.get(name) != digest:
                errors.append(
                    f"manifest.json: {kind[:-1]} drift for {name} — recorded "
                    f"{recorded.get(name)!r}, actual {digest!r}; regenerate with --write"
                )
        for name in recorded:
            if name not in actual:
                errors.append(
                    f"manifest.json: records stale {kind[:-1]} {name!r} no longer present"
                )
    if manifest.get("semantic_fingerprint") != expected_fingerprint:
        errors.append(
            "manifest.json: semantic_fingerprint drift — recorded "
            f"{manifest.get('semantic_fingerprint')!r}, expected {expected_fingerprint!r}; "
            "regenerate with --write"
        )


def main() -> int:
    write = "--write" in sys.argv[1:]
    store = load_schema_store()
    all_errors: list[str] = []
    parsed_fixtures: dict[str, Any] = {}
    checked = 0

    for fixture_name, schema_name in sorted(FIXTURE_SCHEMA.items()):
        fixture_path = FIXTURE_DIR / fixture_name
        if not fixture_path.exists():
            all_errors.append(f"{fixture_name}: fixture file missing")
            continue
        data = load_json(fixture_path)
        parsed_fixtures[fixture_name] = data

        # Deterministic canonical serialization.
        canon = canonical(data)
        if write:
            fixture_path.write_text(canon, encoding="utf-8")
        else:
            raw = fixture_path.read_text(encoding="utf-8")
            if raw != canon:
                all_errors.append(
                    f"{fixture_name}: not canonical/deterministic — run with --write"
                )
            # round-trip determinism: re-parse canonical form must equal data
            if json.loads(canon) != data:
                all_errors.append(f"{fixture_name}: canonical round-trip changed the value")

        # Boundary secret scan: no token/secret/local path may cross the contract.
        check_secret_boundary(fixture_name, canon, all_errors)

        schema = store.get(schema_name)
        if schema is None:
            all_errors.append(f"{fixture_name}: schema {schema_name} not found")
            continue
        errors: list[str] = []
        validate(data, schema, store, fixture_name, errors)

        if schema_name == "product-join-ticket.schema.json":
            check_ticket_claims(data, errors)
        if schema_name == "product-join-request.schema.json" and isinstance(data.get("ticket"), dict):
            check_ticket_claims(data["ticket"], errors)
        if schema_name == "product-join-result.schema.json":
            check_join_result(fixture_name, data, errors)

        all_errors.extend(errors)
        checked += 1

    # Versioned $id, creation-binding/idempotency invariants, and the
    # deterministic semantic-fingerprint manifest.
    check_schema_ids(all_errors)
    check_fixture_schema_coverage(all_errors)
    check_creation_bindings(parsed_fixtures, all_errors)
    check_manifest(write, all_errors)

    if all_errors:
        print("FAIL: onboarding contract fixture validation")
        for err in all_errors:
            print(f"  - {err}")
        return 1

    fingerprint = semantic_fingerprint(fingerprint_material())
    print(
        f"OK: {checked} onboarding contract fixtures valid and deterministic; "
        f"contract {CONTRACT_ID} v{CONTRACT_VERSION}; semantic fingerprint {fingerprint}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
