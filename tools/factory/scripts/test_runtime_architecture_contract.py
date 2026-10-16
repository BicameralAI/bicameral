"""Executable contract tests for canonical runtime architecture specs."""

from __future__ import annotations

import copy
import unittest
from pathlib import Path

from scripts.runtime_architecture_contract import (
    ContractValidationError,
    load_json,
    project_contract,
    validate_contract,
    validate_schema_alignment,
)


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "schemas/runtime-architecture-contract.schema.json"
FIXTURE = (
    ROOT
    / "fixtures/runtime-architecture-contract/accepted-explicit-product-registration.json"
)


class RuntimeArchitectureContractTest(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = load_json(FIXTURE)

    def assert_invalid(self, contract: dict, fragment: str) -> None:
        errors = validate_contract(contract)
        self.assertTrue(
            any(fragment in error for error in errors),
            f"expected {fragment!r} in validation errors: {errors}",
        )

    def test_schema_and_validator_require_the_same_exact_fields(self) -> None:
        self.assertEqual([], validate_schema_alignment(load_json(SCHEMA)))

    def test_representative_accepted_spec_is_valid(self) -> None:
        self.assertEqual([], validate_contract(self.contract))

    def test_projection_uses_only_declared_ids(self) -> None:
        projection = project_contract(self.contract)
        declared = {
            row["id"]
            for collection in (
                "components",
                "relationships",
                "states",
                "transitions",
                "scenarios",
                "conformance",
            )
            for row in self.contract[collection]
        }
        element_ids = {row["id"] for row in self.contract["components"]}
        state_ids = {row["id"] for row in self.contract["states"]}

        produced = [
            *projection["c4"]["nodes"],
            *projection["c4"]["edges"],
            *projection["state"]["nodes"],
            *projection["state"]["edges"],
            *projection["scenarios"],
        ]
        for item in produced:
            with self.subTest(item=item["id"]):
                self.assertIn(item["id"], declared)
                self.assertEqual(item["id"], item["source_contract_id"])

        for edge in projection["c4"]["edges"]:
            self.assertIn(edge["source_id"], element_ids)
            self.assertIn(edge["destination_id"], element_ids)
        for edge in projection["state"]["edges"]:
            self.assertIn(edge["source_id"], state_ids)
            self.assertIn(edge["destination_id"], state_ids)
        for scenario in projection["scenarios"]:
            for edge in scenario["edges"]:
                self.assertIn(edge["id"], declared)
                self.assertEqual(edge["id"], edge["source_contract_id"])
                self.assertIn(edge["source_id"], element_ids)
                self.assertIn(edge["destination_id"], element_ids)

    def test_duplicate_ids_fail_closed(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["relationships"][1]["id"] = contract["relationships"][0]["id"]
        self.assert_invalid(contract, "duplicate id REL-DAEMON-EVENT-STORE")

    def test_undeclared_relationship_endpoint_fails_closed(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["relationships"][0]["destination_id"] = "ELM-UNDECLARED"
        self.assert_invalid(contract, "references undeclared ELM id ELM-UNDECLARED")

    def test_malformed_reference_fails_closed_without_validator_crash(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["relationships"][0]["source_id"] = ["ELM-DAEMON"]
        self.assert_invalid(contract, "references undeclared ELM id")

    def test_undeclared_state_reference_fails_closed(self) -> None:
        mutations = (
            (
                "component state",
                lambda data: data["components"][0].update(
                    state_ids=["STATE-UNDECLARED"]
                ),
            ),
            (
                "relationship state",
                lambda data: data["relationships"][0].update(
                    state_ids=["STATE-UNDECLARED"]
                ),
            ),
            (
                "transition state",
                lambda data: data["transitions"][0].update(
                    source_state_id="STATE-UNDECLARED"
                ),
            ),
            (
                "scenario state",
                lambda data: data["scenarios"][0].update(
                    state_read_ids=["STATE-UNDECLARED"]
                ),
            ),
        )
        for name, mutate in mutations:
            with self.subTest(name=name):
                contract = copy.deepcopy(self.contract)
                mutate(contract)
                self.assert_invalid(contract, "references undeclared")

    def test_undeclared_state_owner_fails_closed(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["states"][0]["persistence_owner_ids"] = ["ELM-UNDECLARED"]
        self.assert_invalid(contract, "references undeclared id ELM-UNDECLARED")

    def test_undeclared_transition_reference_fails_closed(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["scenarios"][0]["transition_ids"] = ["TRANS-UNDECLARED"]
        self.assert_invalid(contract, "references undeclared id TRANS-UNDECLARED")

    def test_undeclared_conformance_source_fails_closed(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["conformance"][0]["source_ids"] = ["SCN-UNDECLARED"]
        self.assert_invalid(contract, "references undeclared id SCN-UNDECLARED")

    def test_projection_refuses_invalid_contract(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["relationships"][0]["source_id"] = "ELM-UNDECLARED"
        with self.assertRaisesRegex(ContractValidationError, "ELM-UNDECLARED"):
            project_contract(contract)


if __name__ == "__main__":
    unittest.main()
