"""Load-bearing checks for the spec-registry schema validator.

A validator that cannot fail proves nothing. These tests show the published
schemas reject the review cases, and that deleting the conflict-cell const
would let the forbidden payload through.
"""

from __future__ import annotations

import copy
import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_spec_registry_schemas",
    ROOT / "scripts/validate_spec_registry_schemas.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SpecRegistrySchemaTests(unittest.TestCase):
    def test_published_schemas_pass_the_review_plan(self) -> None:
        self.assertEqual(MODULE.run_checks(), [])

    def test_conflict_allow_true_is_enforced_by_the_schema(self) -> None:
        store = MODULE.schema_store()
        schema_id = f"{MODULE.ID_PREFIX}approve-policy.schema.json"
        published = store[schema_id]
        cells = MODULE.alpha_cells()
        cells[1]["allow"] = True
        payload = MODULE.policy(cells)
        self.assertTrue(MODULE.errors_for(payload, published, store))

        weakened = copy.deepcopy(published)
        contains = weakened["properties"]["approve_permission"]["allOf"]
        for clause in contains:
            properties = clause["contains"].get("properties", {})
            if properties.get("in_conflict", {}).get("const") is True:
                properties.pop("allow", None)
                clause["contains"]["required"] = ["role", "in_conflict"]
        for item_clause in weakened["properties"]["approve_permission"]["items"]["allOf"]:
            item_clause.pop("then", None)
        self.assertEqual(MODULE.errors_for(payload, weakened, store), [])


if __name__ == "__main__":
    unittest.main()
