#!/usr/bin/env python3
"""Deterministic verification of the pinned Bot onboarding contract (cloud#136).

Recomputes the canonical semantic fingerprint from the vendored schema/fixture
inputs and fails closed if the pinned bot#787 metadata is absent, stale, or
mismatched. Dependency-free (stdlib only) so it runs in any Cloud CI step.

Usage:
    python3 scripts/verify_onboarding_contract.py
"""

from __future__ import annotations

import sys
from pathlib import Path

# Allow running as a bare script without installing the package.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bicameral_cloud.contracts.onboarding import (  # noqa: E402
    EXPECTED_VENDOR_COMMIT,
    OnboardingContractError,
    recompute_semantic_fingerprint,
    verify_onboarding_contract,
)


def main() -> int:
    try:
        manifest = verify_onboarding_contract()
    except OnboardingContractError as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 1

    fingerprint = recompute_semantic_fingerprint()
    print("Onboarding contract pin verified (fail-closed):")
    print(f"  contract:            {manifest['contract']}")
    print(f"  contract_version:    {manifest['contract_version']}")
    print(f"  source_commit:       {manifest['source_commit']}")
    print(f"  vendored_from:       {EXPECTED_VENDOR_COMMIT}")
    print(f"  semantic_fingerprint:{fingerprint}")
    print(f"  schemas:             {len(manifest['schemas'])}")
    print(f"  fixtures:            {len(manifest['fixtures'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
