#!/usr/bin/env python3
"""Recompute the issue #937 ConfirmationChallengeV2 binding digest.

This is intentionally narrow. The primary contract validator owns the schema,
state machine, amendment, and compatibility checks. This check proves the
fixture's confirmation_binding_digest actually covers requested_outcome while
excluding display_summary and the digest field itself.
"""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path
from typing import Any

import validate_smr_claim_acceptance_contract as contract

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = (
    ROOT
    / "protocol"
    / "smr-claim-acceptance"
    / "fixtures"
    / "confirmation-binding.v2.valid.json"
)


def load(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def recompute(document: dict[str, Any]) -> str:
    normalized = {
        key: value
        for key, value in document.items()
        if key not in {"display_summary", "confirmation_binding_digest"}
    }
    return contract.smr_digest("confirmation-binding", normalized)


def main() -> int:
    fixture = load(FIXTURE)
    errors: list[str] = []

    expected = recompute(fixture)
    actual = fixture.get("confirmation_binding_digest")
    if actual != expected:
        errors.append(
            "confirmation_binding_digest does not reproduce: "
            f"expected {expected}, got {actual!r}"
        )

    mutated_outcome = copy.deepcopy(fixture)
    mutated_outcome["requested_outcome"] = "needs_signoff_review"
    if recompute(mutated_outcome) == expected:
        errors.append("requested_outcome does not participate in confirmation_binding_digest")

    mutated_summary = copy.deepcopy(fixture)
    mutated_summary["display_summary"] = "Cosmetic wording changed; machine effect did not."
    if recompute(mutated_summary) != expected:
        errors.append("display_summary text must be excluded from confirmation_binding_digest")

    mutated_summary_digest = copy.deepcopy(fixture)
    mutated_summary_digest["display_summary_digest"] = "sha256:" + "0" * 64
    if recompute(mutated_summary_digest) == expected:
        errors.append("display_summary_digest must participate in confirmation_binding_digest")

    if errors:
        print("SMR confirmation binding digest validation FAILED:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(
        "SMR confirmation binding digest OK: fixture reproduced; requested_outcome and "
        "display_summary_digest are bound; display_summary text is excluded."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
