#!/usr/bin/env python3
"""Cross-repo conformance validator for ProviderResourceDescriptor and ProviderItemEnvelope.

Validates that:
1. Integrations golden fixtures conform to bot canonical schemas.
2. Bot fixtures conform to bot canonical schemas (self-check).
3. Integrations pinned schemas match bot canonical schemas (drift detection).

Usage:
    # Check integrations fixtures against bot schemas:
    python scripts/validate_cross_repo_conformance.py \
        --bot-schemas protocol/schemas/v2 \
        --integrations-fixtures ../bicameral-integrations/protocol/provider_acquisition/fixtures

    # Check schema drift between repos:
    python scripts/validate_cross_repo_conformance.py \
        --bot-schemas protocol/schemas/v2 \
        --integrations-schemas ../bicameral-integrations/protocol/provider_acquisition/schemas \
        --check-drift

    # Full cross-repo validation:
    python scripts/validate_cross_repo_conformance.py \
        --bot-schemas protocol/schemas/v2 \
        --integrations-schemas ../bicameral-integrations/protocol/provider_acquisition/schemas \
        --integrations-fixtures ../bicameral-integrations/protocol/provider_acquisition/fixtures \
        --bot-fixtures protocol/schemas/v2/fixtures \
        --check-drift --check-conformance

Exit codes:
    0 — all checks pass
    1 — conformance or drift failure
    2 — missing paths or configuration error
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# Schema loading
# ---------------------------------------------------------------------------

DESCRIPTOR_SCHEMA_NAME = "provider-resource-descriptor.schema.json"
ITEM_SCHEMA_NAME = "provider-item-envelope.schema.json"


def load_schema(schema_dir: Path, name: str) -> dict | None:
    path = schema_dir / name
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


# ---------------------------------------------------------------------------
# Minimal JSON Schema draft-07 validator (stdlib-only)
# ---------------------------------------------------------------------------

try:
    from jsonschema import Draft7Validator  # type: ignore[import-untyped]

    _HAS_JSONSCHEMA = True
except ImportError:
    _HAS_JSONSCHEMA = False


def validate_instance(instance: dict, schema: dict) -> list[str]:
    """Validate a JSON instance against a draft-07 schema."""
    if _HAS_JSONSCHEMA:
        validator = Draft7Validator(schema)
        return [e.message for e in validator.iter_errors(instance)]
    # Fallback: required-field check only.
    errors: list[str] = []
    for field in schema.get("required", []):
        if field not in instance:
            errors.append(f"required field missing: '{field}'")
    return errors


# ---------------------------------------------------------------------------
# Fixture discovery
# ---------------------------------------------------------------------------


def discover_descriptor_fixtures(fixtures_dir: Path) -> list[Path]:
    """Find descriptor fixture files."""
    # Integrations layout: fixtures/descriptors/*.json
    desc_dir = fixtures_dir / "descriptors"
    if desc_dir.is_dir():
        return sorted(desc_dir.glob("*.json"))
    # Bot layout: fixtures/valid-provider-resource-descriptor-*.json
    return sorted(fixtures_dir.glob("valid-provider-resource-descriptor-*.json"))


def discover_item_fixtures(fixtures_dir: Path) -> list[Path]:
    """Find item-envelope fixture files."""
    # Integrations layout: fixtures/items/*.json
    items_dir = fixtures_dir / "items"
    if items_dir.is_dir():
        return sorted(items_dir.glob("*.json"))
    # Bot layout: fixtures/valid-provider-item-envelope-*.json
    return sorted(fixtures_dir.glob("valid-provider-item-envelope-*.json"))


# ---------------------------------------------------------------------------
# Conformance check
# ---------------------------------------------------------------------------


def check_conformance(
    schema: dict, fixtures: list[Path], schema_name: str
) -> list[str]:
    """Validate fixtures against a schema. Returns list of error strings."""
    errors: list[str] = []
    for fixture_path in fixtures:
        try:
            data = json.loads(fixture_path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as exc:
            errors.append(f"{fixture_path.name}: failed to load: {exc}")
            continue
        # Strip underscore-prefixed metadata keys (comments, boundary notes)
        instance = {k: v for k, v in data.items() if not k.startswith("_")}
        validation_errors = validate_instance(instance, schema)
        if validation_errors:
            for err in validation_errors:
                errors.append(f"{fixture_path.name} vs {schema_name}: {err}")
    return errors


# ---------------------------------------------------------------------------
# Drift check
# ---------------------------------------------------------------------------


def check_schema_drift(
    bot_schemas_dir: Path, integrations_schemas_dir: Path
) -> list[str]:
    """Compare schema content between bot and integrations repos."""
    errors: list[str] = []
    for name in [DESCRIPTOR_SCHEMA_NAME, ITEM_SCHEMA_NAME]:
        bot_path = bot_schemas_dir / name
        int_path = integrations_schemas_dir / name

        if not bot_path.exists():
            errors.append(f"Bot schema missing: {bot_path}")
            continue
        if not int_path.exists():
            errors.append(f"Integrations schema missing: {int_path}")
            continue

        # Compare by parsing and re-serializing to normalize formatting
        bot_schema = json.loads(bot_path.read_text(encoding="utf-8"))
        int_schema = json.loads(int_path.read_text(encoding="utf-8"))

        if bot_schema != int_schema:
            bot_hash = sha256_file(bot_path)
            int_hash = sha256_file(int_path)
            errors.append(
                f"DRIFT: {name}\n"
                f"  bot sha256:          {bot_hash}\n"
                f"  integrations sha256: {int_hash}\n"
                f"  Integrations pinned schema differs from bot canonical schema."
            )
    return errors


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Cross-repo conformance validator for provider acquisition schemas."
    )
    parser.add_argument(
        "--bot-schemas",
        type=Path,
        required=True,
        help="Path to bot schema directory (e.g. protocol/schemas/v2)",
    )
    parser.add_argument(
        "--bot-fixtures",
        type=Path,
        default=None,
        help="Path to bot fixtures directory (e.g. protocol/schemas/v2/fixtures)",
    )
    parser.add_argument(
        "--integrations-schemas",
        type=Path,
        default=None,
        help="Path to integrations schema directory",
    )
    parser.add_argument(
        "--integrations-fixtures",
        type=Path,
        default=None,
        help="Path to integrations fixtures directory",
    )
    parser.add_argument(
        "--check-drift",
        action="store_true",
        help="Check schema drift between bot and integrations",
    )
    parser.add_argument(
        "--check-conformance",
        action="store_true",
        default=True,
        help="Check fixture conformance against schemas (default: on)",
    )
    parser.add_argument(
        "--json-output",
        action="store_true",
        help="Output results as JSON for CI consumption",
    )

    args = parser.parse_args()
    all_errors: list[str] = []
    results: dict = {"drift": [], "conformance": [], "summary": {}}

    # Validate paths
    if not args.bot_schemas.is_dir():
        print(f"ERROR: Bot schemas directory not found: {args.bot_schemas}", file=sys.stderr)
        return 2

    # Load bot schemas
    descriptor_schema = load_schema(args.bot_schemas, DESCRIPTOR_SCHEMA_NAME)
    item_schema = load_schema(args.bot_schemas, ITEM_SCHEMA_NAME)

    if descriptor_schema is None:
        print(f"ERROR: {DESCRIPTOR_SCHEMA_NAME} not found in {args.bot_schemas}", file=sys.stderr)
        return 2
    if item_schema is None:
        print(f"ERROR: {ITEM_SCHEMA_NAME} not found in {args.bot_schemas}", file=sys.stderr)
        return 2

    print(f"Loaded bot schemas from: {args.bot_schemas}")
    print(f"  {DESCRIPTOR_SCHEMA_NAME}: {sha256_file(args.bot_schemas / DESCRIPTOR_SCHEMA_NAME)[:16]}...")
    print(f"  {ITEM_SCHEMA_NAME}: {sha256_file(args.bot_schemas / ITEM_SCHEMA_NAME)[:16]}...")
    print()

    # --- Drift check ---
    if args.check_drift and args.integrations_schemas:
        if not args.integrations_schemas.is_dir():
            print(f"ERROR: Integrations schemas directory not found: {args.integrations_schemas}", file=sys.stderr)
            return 2

        print("=== Schema drift check ===")
        drift_errors = check_schema_drift(args.bot_schemas, args.integrations_schemas)
        if drift_errors:
            for err in drift_errors:
                print(f"  FAIL: {err}")
            all_errors.extend(drift_errors)
            results["drift"] = drift_errors
        else:
            print("  OK: Schemas match between bot and integrations.")
        print()

    # --- Bot fixture self-check ---
    if args.check_conformance and args.bot_fixtures:
        if not args.bot_fixtures.is_dir():
            print(f"ERROR: Bot fixtures directory not found: {args.bot_fixtures}", file=sys.stderr)
            return 2

        print("=== Bot fixture self-check ===")
        desc_fixtures = discover_descriptor_fixtures(args.bot_fixtures)
        item_fixtures = discover_item_fixtures(args.bot_fixtures)

        print(f"  Descriptor fixtures: {len(desc_fixtures)}")
        print(f"  Item fixtures: {len(item_fixtures)}")

        errors = check_conformance(descriptor_schema, desc_fixtures, DESCRIPTOR_SCHEMA_NAME)
        errors.extend(check_conformance(item_schema, item_fixtures, ITEM_SCHEMA_NAME))

        if errors:
            for err in errors:
                print(f"  FAIL: {err}")
            all_errors.extend(errors)
        else:
            print("  OK: All bot fixtures conform to bot schemas.")
        results["conformance"].append({"source": "bot", "errors": errors})
        print()

    # --- Integrations fixture check ---
    if args.check_conformance and args.integrations_fixtures:
        if not args.integrations_fixtures.is_dir():
            print(f"ERROR: Integrations fixtures directory not found: {args.integrations_fixtures}", file=sys.stderr)
            return 2

        print("=== Integrations fixture conformance ===")
        desc_fixtures = discover_descriptor_fixtures(args.integrations_fixtures)
        item_fixtures = discover_item_fixtures(args.integrations_fixtures)

        print(f"  Descriptor fixtures: {len(desc_fixtures)}")
        print(f"  Item fixtures: {len(item_fixtures)}")

        errors = check_conformance(descriptor_schema, desc_fixtures, DESCRIPTOR_SCHEMA_NAME)
        errors.extend(check_conformance(item_schema, item_fixtures, ITEM_SCHEMA_NAME))

        if errors:
            for err in errors:
                print(f"  FAIL: {err}")
            all_errors.extend(errors)
        else:
            print("  OK: All integrations fixtures conform to bot schemas.")
        results["conformance"].append({"source": "integrations", "errors": errors})
        print()

    # --- Summary ---
    total_checked = 0
    if args.bot_fixtures and args.bot_fixtures.is_dir():
        total_checked += len(discover_descriptor_fixtures(args.bot_fixtures))
        total_checked += len(discover_item_fixtures(args.bot_fixtures))
    if args.integrations_fixtures and args.integrations_fixtures.is_dir():
        total_checked += len(discover_descriptor_fixtures(args.integrations_fixtures))
        total_checked += len(discover_item_fixtures(args.integrations_fixtures))

    results["summary"] = {
        "total_fixtures_checked": total_checked,
        "total_errors": len(all_errors),
        "drift_checked": args.check_drift and args.integrations_schemas is not None,
    }

    if args.json_output:
        print(json.dumps(results, indent=2))

    if all_errors:
        print(f"FAILED: {len(all_errors)} error(s) found.")
        return 1

    print("PASSED: All cross-repo conformance checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
