#!/usr/bin/env python3
"""Fail-closed policy for release manifests, descriptors, receipts, and served identity."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


SHA = re.compile(r"^[a-f0-9]{40}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")


def digest(value: object) -> str:
    return "sha256:" + hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def _error(errors: list[str], location: str, message: str) -> None:
    errors.append(f"{location}: {message}")


def _valid_digest_map(value: object, location: str, errors: list[str]) -> bool:
    if not isinstance(value, dict) or not value:
        _error(errors, location, "must be a non-empty object")
        return False
    valid = True
    for name, item in value.items():
        if not isinstance(name, str) or not name:
            _error(errors, location, "keys must be non-empty strings")
            valid = False
        if not isinstance(item, str) or not DIGEST.fullmatch(item):
            _error(errors, f"{location}.{name}", "must be sha256:<64 lowercase hex>")
            valid = False
    return valid


def validate_component(name: str, component: object) -> list[str]:
    errors: list[str] = []
    location = f"components.{name}"
    if not isinstance(component, dict):
        return [f"{location}: must be an object"]
    required = {"commit", "artifacts", "interfaces", "descriptor_digest"}
    if set(component) != required:
        _error(errors, location, f"keys must be exactly {sorted(required)}")
        return errors
    if not isinstance(component["commit"], str) or not SHA.fullmatch(component["commit"]):
        _error(errors, f"{location}.commit", "must be a full lowercase git SHA")
    _valid_digest_map(component["artifacts"], f"{location}.artifacts", errors)
    _valid_digest_map(component["interfaces"], f"{location}.interfaces", errors)
    if not isinstance(component["descriptor_digest"], str) or not DIGEST.fullmatch(component["descriptor_digest"]):
        _error(errors, f"{location}.descriptor_digest", "must be sha256:<64 lowercase hex>")
    return errors


def validate_manifest(manifest: object) -> list[str]:
    errors: list[str] = []
    if not isinstance(manifest, dict):
        return ["manifest: expected object"]
    required = {"schema_version", "release_id", "components", "closures", "required_journeys"}
    if set(manifest) != required:
        _error(errors, "manifest", f"keys must be exactly {sorted(required)}")
    if manifest.get("schema_version") != 1:
        _error(errors, "schema_version", "must be 1")
    if not isinstance(manifest.get("release_id"), str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{2,127}", manifest["release_id"]):
        _error(errors, "release_id", "must be a non-empty immutable id")
    components = manifest.get("components")
    if not isinstance(components, dict) or not components:
        _error(errors, "components", "must be a non-empty object")
        components = {}
    for name, component in components.items():
        errors.extend(validate_component(str(name), component))
    closures = manifest.get("closures")
    if not isinstance(closures, dict) or not closures:
        _error(errors, "closures", "must be a non-empty object")
        closures = {}
    for journey, closure in closures.items():
        location = f"closures.{journey}"
        if not isinstance(closure, dict):
            _error(errors, location, "must be an object")
            continue
        expected_keys = {"components", "profile_version", "closure_digest"}
        if set(closure) != expected_keys:
            _error(errors, location, f"keys must be exactly {sorted(expected_keys)}")
            continue
        names = closure["components"]
        if not isinstance(names, list) or not names or len(names) != len(set(names)):
            _error(errors, f"{location}.components", "must be a unique non-empty list")
            continue
        if any(name not in components for name in names):
            _error(errors, f"{location}.components", "must name declared components")
            continue
        profile = closure["profile_version"]
        if not isinstance(profile, str) or not profile:
            _error(errors, f"{location}.profile_version", "must be non-empty")
        expected_digest = digest(
            {
                "components": {name: components[name] for name in sorted(names)},
                "profile_version": profile,
            }
        )
        if closure["closure_digest"] != expected_digest:
            _error(errors, f"{location}.closure_digest", "does not bind the complete closure")
    required_journeys = manifest.get("required_journeys")
    if not isinstance(required_journeys, list) or not required_journeys or len(required_journeys) != len(set(required_journeys)):
        _error(errors, "required_journeys", "must be a unique non-empty list")
    elif any(journey not in closures for journey in required_journeys):
        _error(errors, "required_journeys", "must name declared closures")
    return errors


def validate_descriptor(name: str, component: object, descriptor: object) -> list[str]:
    if not isinstance(descriptor, dict):
        return [f"descriptor.{name}: expected object"]
    errors: list[str] = []
    if descriptor.get("schema_version") != 1 or descriptor.get("component") != name:
        _error(errors, f"descriptor.{name}", "schema/component mismatch")
    if isinstance(component, dict):
        for field in ("commit", "artifacts", "interfaces", "descriptor_digest"):
            if descriptor.get(field) != component.get(field):
                _error(errors, f"descriptor.{name}.{field}", "does not match manifest")
    unsigned = {key: value for key, value in descriptor.items() if key != "descriptor_digest"}
    if descriptor.get("descriptor_digest") != digest(unsigned):
        _error(errors, f"descriptor.{name}.descriptor_digest", "does not bind descriptor bytes")
    return errors


def validate_receipt(manifest: object, receipt: object) -> list[str]:
    errors = validate_manifest(manifest)
    if errors:
        return errors
    if not isinstance(receipt, dict):
        return ["receipt: expected object"]
    assert isinstance(manifest, dict)
    required = {"schema_version", "release_id", "journey_id", "closure_digest", "profile_version", "outcome"}
    missing = required - set(receipt)
    if missing:
        _error(errors, "receipt", f"missing {sorted(missing)}")
        return errors
    if receipt["schema_version"] != 1:
        _error(errors, "receipt.schema_version", "must be 1")
    if receipt["outcome"] != "passed":
        _error(errors, "receipt.outcome", "must be passed")
    if receipt["release_id"] != manifest["release_id"]:
        _error(errors, "receipt.release_id", "does not match manifest")
    journey = receipt["journey_id"]
    if journey not in manifest["required_journeys"]:
        _error(errors, "receipt.journey_id", "is not required by the manifest")
        return errors
    closure = manifest["closures"][journey]
    if receipt["closure_digest"] != closure["closure_digest"]:
        _error(errors, "receipt.closure_digest", "does not match manifest closure")
    if receipt["profile_version"] != closure["profile_version"]:
        _error(errors, "receipt.profile_version", "does not match manifest profile")
    return errors


def validate_receipt_set(manifest: object, receipts: list[object]) -> list[str]:
    errors = validate_manifest(manifest)
    if errors:
        return errors
    assert isinstance(manifest, dict)
    by_journey: dict[str, list[object]] = {}
    for receipt in receipts:
        if isinstance(receipt, dict) and isinstance(receipt.get("journey_id"), str):
            by_journey.setdefault(receipt["journey_id"], []).append(receipt)
        errors.extend(validate_receipt(manifest, receipt))
    for journey in manifest["required_journeys"]:
        if len(by_journey.get(journey, [])) != 1:
            _error(errors, f"receipts.{journey}", "requires exactly one matching receipt")
    return errors


def validate_served_identity(manifest: object, identity: object) -> list[str]:
    errors = validate_manifest(manifest)
    if errors:
        return errors
    if not isinstance(identity, dict):
        return ["served_identity: expected object"]
    assert isinstance(manifest, dict)
    required = {"schema_version", "release_id", "components"}
    if set(identity) != required:
        _error(errors, "served_identity", f"keys must be exactly {sorted(required)}")
    if identity.get("schema_version") != 1:
        _error(errors, "served_identity.schema_version", "must be 1")
    if identity.get("release_id") != manifest["release_id"]:
        _error(errors, "served_identity.release_id", "does not match manifest")
    served = identity.get("components")
    if not isinstance(served, dict):
        return errors + ["served_identity.components: must be an object"]
    if set(served) != set(manifest["components"]):
        _error(
            errors,
            "served_identity.components",
            "component names must exactly match manifest",
        )
    for name, component in manifest["components"].items():
        actual = served.get(name)
        if not isinstance(actual, dict):
            _error(errors, f"served_identity.components.{name}", "is missing")
            continue
        if set(actual) != {"commit", "artifacts"}:
            _error(
                errors,
                f"served_identity.components.{name}",
                "keys must be exactly ['artifacts', 'commit']",
            )
        for field in ("commit", "artifacts"):
            if actual.get(field) != component[field]:
                _error(errors, f"served_identity.components.{name}.{field}", "does not match manifest")
    return errors


def _load(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--receipt", type=Path, action="append", default=[])
    parser.add_argument("--descriptor", action="append", default=[], metavar="COMPONENT=PATH")
    parser.add_argument("--served-identity", type=Path)
    args = parser.parse_args()
    manifest = _load(args.manifest)
    errors = validate_manifest(manifest)
    if isinstance(manifest, dict):
        for item in args.descriptor:
            if "=" not in item:
                errors.append("descriptor argument must be COMPONENT=PATH")
                continue
            name, filename = item.split("=", 1)
            errors.extend(validate_descriptor(name, manifest.get("components", {}).get(name), _load(Path(filename))))
    if args.receipt:
        errors.extend(validate_receipt_set(manifest, [_load(path) for path in args.receipt]))
    if args.served_identity:
        errors.extend(validate_served_identity(manifest, _load(args.served_identity)))
    if errors:
        print("Release tuple rejected:", file=sys.stderr)
        print("\n".join(f"- {error}" for error in dict.fromkeys(errors)), file=sys.stderr)
        return 1
    print("OK: release tuple is revision-pinned, receipt-complete, and identity-bound")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
