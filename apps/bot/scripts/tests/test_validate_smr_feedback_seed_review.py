"""Controls for the non-authoritative #1221 GovernedDomain fixture."""

from __future__ import annotations

import copy
import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_smr_feedback_seed_review",
    ROOT / "scripts" / "validate_smr_feedback_seed_review.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
CONTRACT = MODULE._contract_module()


def load() -> dict:
    return CONTRACT.load_json(MODULE.FIXTURE)


class SeedReviewTest(unittest.TestCase):
    def test_repository_review_package_validates(self) -> None:
        self.assertEqual(MODULE.validate_review_package(load()), [])

    def test_markdown_cannot_be_promoted_to_board_authority(self) -> None:
        package = copy.deepcopy(load())
        package["governed_domain"]["artifact_role"] = "product_truth"
        errors = MODULE.validate_review_package(package)
        self.assertTrue(any("markdown may be provenance only" in error for error in errors))

    def test_graph_json_cannot_be_added_as_board(self) -> None:
        package = copy.deepcopy(load())
        package["graph_json"] = {"claims": []}
        errors = MODULE.validate_review_package(package)
        self.assertTrue(any("overclaims unavailable canonical artifacts" in error for error in errors))

    def test_claim_mutation_breaks_digest_and_approved_content(self) -> None:
        package = copy.deepcopy(load())
        package["governed_domain"]["normalized_claim_set"]["claims"][0][
            "context_statement"
        ] = "Mutated"
        errors = MODULE.validate_review_package(package)
        self.assertTrue(any("twelve owner-approved statements" in error for error in errors))
        self.assertTrue(any("claim_set_digest" in error for error in errors))

    def test_source_domain_cannot_seed_the_accepted_board(self) -> None:
        package = copy.deepcopy(load())
        package["governed_domain"]["is_source"] = True
        errors = MODULE.validate_review_package(package)
        self.assertTrue(any("is_source=false" in error for error in errors))

    def test_legacy_candidate_envelope_is_rejected(self) -> None:
        package = copy.deepcopy(load())
        package["governed_domain"]["decision_candidates"] = []
        errors = MODULE.validate_review_package(package)
        self.assertTrue(any("superseded candidate or event envelope" in error for error in errors))

    def test_model_atom_identity_is_recomputed(self) -> None:
        package = copy.deepcopy(load())
        package["governed_domain"]["normalized_claim_set"]["model_atoms"][0][
            "semantic_key"
        ] = "mutated"
        errors = MODULE.validate_review_package(package)
        self.assertTrue(any("ModelAtom identity must be" in error for error in errors))

    def test_throwaway_replay_is_isolated_and_read_only(self) -> None:
        package = load()
        replayed = MODULE.replay_throwaway_fixture(package)
        replayed["claims"][0]["context_statement"] = "Mutated replay"
        original = package["governed_domain"]["normalized_claim_set"]["claims"][0]
        self.assertNotEqual(replayed["claims"][0], original)
        self.assertFalse(package["replay"]["writes_product_state"])
        self.assertIsNone(package["replay"]["canonical_event_kind"])


if __name__ == "__main__":
    unittest.main()
