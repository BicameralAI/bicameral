#!/usr/bin/env python3
"""Self-contained checks for commit-grounded attestation filename rules.

Run directly: ``python3 scripts/test_factory_attestation_naming.py``. Uses only
the stdlib so it works without a test harness or extra dependencies.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent


def _load(module_name: str, filename: str):
    spec = importlib.util.spec_from_file_location(module_name, SCRIPT_DIR / filename)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


validator = _load("validate_factory_attestation", "validate-factory-attestation.py")

COMMIT = "7d4a4207dcfe3dec9a5de87af7958f2053f4dd6d"


def check(name: str, expected: bool) -> None:
    actual = validator.attestation_filename_matches_commit(name, COMMIT)
    status = "ok" if actual == expected else "FAIL"
    print(f"[{status}] {name} -> {actual} (expected {expected})")
    if actual != expected:
        raise AssertionError(f"{name}: expected {expected}, got {actual}")


def main() -> int:
    # Legacy single-run form stays valid (existing records).
    check(f"{COMMIT}.json", True)
    # Per-run form with a branch-slug discriminator is valid.
    check(f"{COMMIT}.devin-1781821202-cross-repo-conformance-pin.json", True)
    check(f"{COMMIT}.pr-480.json", True)
    check(f"{COMMIT}.a1b2c3d.json", True)
    # Empty discriminator is not a per-run form.
    check(f"{COMMIT}..json", False)
    # Wrong commit prefix is rejected.
    check("0000000000000000000000000000000000000000.json", False)
    check(f"{COMMIT}-suffix.json", False)
    # Discriminator with a path separator or illegal char is rejected.
    check(f"{COMMIT}.bad/slug.json", False)
    check(f"{COMMIT}.bad slug.json", False)
    # Non-JSON is rejected.
    check(f"{COMMIT}.devin-x.txt", False)
    print("all attestation filename checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
