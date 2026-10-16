"""Negative-control tests for the SMR claim-acceptance contract validator.

A validator that cannot fail proves nothing. Each test mutates one contract
invariant and asserts the validator reports it, so the guard is demonstrably
load-bearing rather than decorative.
"""

from __future__ import annotations

import copy
import importlib.util
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_smr_claim_acceptance_contract",
    ROOT / "scripts/validate_smr_claim_acceptance_contract.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def load(name: str) -> dict:
    return MODULE.load_json(MODULE.FIXTURE_DIR / name)


def schema(name: str) -> dict:
    return MODULE.load_json(MODULE.SCHEMA_DIR / name)


def run(value: dict, schema_doc: dict) -> list[str]:
    errors: list[str] = []
    MODULE.validate(value, schema_doc, {"__self__": schema_doc}, "fixture", errors)
    return errors


class RepositoryStateTest(unittest.TestCase):
    def test_repository_contract_validates_clean(self) -> None:
        self.assertEqual(MODULE.main(), 0)


class DigestProfileTest(unittest.TestCase):
    def test_golden_vectors_reproduce(self) -> None:
        errors: list[str] = []
        MODULE.check_digest_vectors(errors)
        self.assertEqual(errors, [])

    def test_domain_tag_separates_identical_values(self) -> None:
        self.assertNotEqual(
            MODULE.smr_digest("claim-set", {}),
            MODULE.smr_digest("claim-scope", {}),
        )

    def test_key_order_does_not_change_the_digest(self) -> None:
        self.assertEqual(
            MODULE.smr_digest("claim-set", {"a": 1, "b": 2}),
            MODULE.smr_digest("claim-set", {"b": 2, "a": 1}),
        )

    def test_nfc_normalization_is_applied(self) -> None:
        decomposed = "Idée"
        precomposed = "Idée"
        self.assertNotEqual(decomposed, precomposed)
        self.assertEqual(
            MODULE.smr_digest("claim-set", {"k": decomposed}),
            MODULE.smr_digest("claim-set", {"k": precomposed}),
        )

    def test_digest_encoding_is_prefixed_lowercase_hex(self) -> None:
        value = MODULE.smr_digest("claim-set", {})
        self.assertTrue(value.startswith("sha256:"))
        self.assertEqual(len(value), len("sha256:") + 64)
        self.assertEqual(value, value.lower())

    def test_model_atom_vector_rejects_changed_jcs_bytes(self) -> None:
        vectors = load("digest-vectors.json")
        vector = next(
            v
            for v in vectors["vectors"]
            if v["vector_id"] == "V12-MODEL-ATOM-IDENTITY"
        )
        vector["expected_jcs_utf8_hex"] = "00"
        errors: list[str] = []
        with mock.patch.object(MODULE, "load_json", return_value=vectors):
            MODULE.check_digest_vectors(errors)
        self.assertTrue(any("JCS bytes" in error for error in errors))

    def test_empty_projection_vector_rejects_changed_preimage(self) -> None:
        vectors = load("digest-vectors.json")
        vector = next(
            v for v in vectors["vectors"] if v["vector_id"] == "V13-EMPTY-SMR-BASE-SNAPSHOT"
        )
        vector["expected_preimage_hex"] = "00"
        errors: list[str] = []
        with mock.patch.object(MODULE, "load_json", return_value=vectors):
            MODULE.check_digest_vectors(errors)
        self.assertTrue(any("digest preimage" in error for error in errors))


class SchemaRejectionTest(unittest.TestCase):
    def test_malformed_digest_is_rejected(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["digests"]["claim_set_digest"] = "sha256:NOTHEX"
        self.assertTrue(any("does not match pattern" in e for e in run(fixture, doc)))

    def test_unprefixed_digest_is_rejected(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["digests"]["claim_set_digest"] = "a" * 64
        self.assertTrue(run(fixture, doc))

    def test_additional_property_fails_closed(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["extractor_supplied_authority"] = "granted"
        self.assertTrue(
            any("additional property not permitted" in e for e in run(fixture, doc))
        )

    def test_unknown_event_schema_version_is_rejected(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["event_schema_version"] = 2
        self.assertTrue(any("expected const 1" in e for e in run(fixture, doc)))

    def test_missing_base_event_position_is_rejected(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        del fixture["base_event_position"]
        self.assertTrue(
            any("base_event_position: required field missing" in e for e in run(fixture, doc))
        )

    def test_open_atom_kind_is_rejected(self) -> None:
        doc = schema("smr-claim-set-proposal.schema.json")
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        fixture["normalized_claim_set"]["model_atoms"][0]["atom_kind"] = "freeform"
        self.assertTrue(any("not in enum" in e for e in run(fixture, doc)))

    def test_float_value_is_rejected(self) -> None:
        doc = schema("smr-claim-set-proposal.schema.json")
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        fixture["base_event_position"] = 14.87
        self.assertTrue(any("expected integer" in e for e in run(fixture, doc)))

    def test_absolute_artifact_path_is_rejected(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["artifact_binding"]["artifact_path"] = "/etc/spec.md"
        self.assertTrue(run(fixture, doc))

    def test_signoff_state_cannot_be_proposed_at_promotion(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["resulting_decision"]["signoff_state"] = "proposed"
        self.assertTrue(any("expected const 'approved'" in e for e in run(fixture, doc)))

    def test_amendment_005_is_clean(self) -> None:
        errors: list[str] = []
        MODULE.check_amendment_005(errors)
        self.assertEqual(errors, [])

    def test_v1_confirmation_version_rejected_for_smr_event(self) -> None:
        doc = schema("claim-set-candidate-promoted.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-promoted.valid.json"))
        fixture["acceptance"]["confirmation_protocol_version"] = 1
        self.assertTrue(any("expected const 2" in e for e in run(fixture, doc)))

    def test_nonzero_replay_provider_calls_rejected(self) -> None:
        doc = schema("promotion-receipt.schema.json")
        fixture = copy.deepcopy(load("promotion-receipt.committed-once.json"))
        fixture["smr"]["provider_calls_during_replay"] = 1
        self.assertTrue(any("above maximum 0" in e for e in run(fixture, doc)))

    def test_open_rejection_reason_is_rejected(self) -> None:
        doc = schema("claim-set-candidate-rejected.schema.json")
        fixture = copy.deepcopy(load("claim-set-candidate-rejected.valid.json"))
        fixture["reason_code"] = "because the reviewer said so"
        self.assertTrue(any("not in enum" in e for e in run(fixture, doc)))

    def test_claim_bound_is_enforced(self) -> None:
        doc = schema("smr-claim-set-proposal.schema.json")
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        template = fixture["normalized_claim_set"]["claims"][0]
        fixture["normalized_claim_set"]["claims"] = [template] * 513
        self.assertTrue(any("exceeds maxItems 512" in e for e in run(fixture, doc)))

    def test_missing_relation_support_is_rejected(self) -> None:
        doc = schema("smr-claim-set-proposal.schema.json")
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        del relation["supporting_claim_ids"]
        self.assertTrue(
            any("supporting_claim_ids: required field missing" in e for e in run(fixture, doc))
        )

    def test_empty_relation_source_support_is_rejected(self) -> None:
        doc = schema("smr-claim-set-proposal.schema.json")
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        relation["supporting_source_anchor_ids"] = []
        self.assertTrue(any("expected at least 1 items" in e for e in run(fixture, doc)))

    def test_provider_derivation_label_is_rejected(self) -> None:
        doc = schema("smr-claim-set-proposal.schema.json")
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        relation["source_explicit"] = True
        self.assertTrue(
            any("additional property not permitted" in e for e in run(fixture, doc))
        )


class ConfirmationBindingTest(unittest.TestCase):
    def test_v2_binding_is_a_superset_of_v1(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        required = set(doc["required"])
        self.assertTrue(
            MODULE.V1_BOUND_FIELDS <= required,
            f"v2 dropped shipped v1 bound fields: {sorted(MODULE.V1_BOUND_FIELDS - required)}",
        )

    def test_dropping_workspace_binding_is_caught(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        fixture = copy.deepcopy(load("confirmation-binding.v2.valid.json"))
        del fixture["workspace"]
        self.assertTrue(
            any("workspace: required field missing" in e for e in run(fixture, doc))
        )

    def test_dropping_packet_binding_is_caught(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        fixture = copy.deepcopy(load("confirmation-binding.v2.valid.json"))
        del fixture["packet_id"]
        self.assertTrue(
            any("packet_id: required field missing" in e for e in run(fixture, doc))
        )

    def test_ttl_matches_the_shipped_constant(self) -> None:
        self.assertEqual(MODULE.CONFIRMATION_TTL_SECONDS, 300)
        errors: list[str] = []
        MODULE.check_confirmation_v2(errors)
        self.assertEqual(errors, [])


class Amendment001Test(unittest.TestCase):
    """A15/A16: requested_outcome binding and a resolvable schema reference.

    The first draft of this contract omitted requested_outcome from the v1
    superset, which would have let the reviewed disposition change without
    invalidating the binding and silently dropped the shipped OutcomeMismatch
    control. These tests keep that hole closed.
    """

    def test_amendment_is_clean(self) -> None:
        errors: list[str] = []
        MODULE.check_amendment_001(errors)
        self.assertEqual(errors, [])

    def test_requested_outcome_is_in_the_v1_superset(self) -> None:
        self.assertIn("requested_outcome", MODULE.V1_BOUND_FIELDS)

    def test_requested_outcome_is_required_by_the_schema(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        self.assertIn("requested_outcome", doc["required"])

    def test_dropping_requested_outcome_is_caught(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        fixture = copy.deepcopy(load("confirmation-binding.v2.valid.json"))
        del fixture["requested_outcome"]
        self.assertTrue(
            any("requested_outcome: required field missing" in e for e in run(fixture, doc))
        )

    def test_outcome_enum_matches_shipped_promotion_outcome(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        self.assertEqual(
            doc["properties"]["requested_outcome"]["enum"], MODULE.PROMOTION_OUTCOMES
        )

    def test_unknown_outcome_is_rejected(self) -> None:
        doc = schema("confirmation-binding.v2.schema.json")
        fixture = copy.deepcopy(load("confirmation-binding.v2.valid.json"))
        fixture["requested_outcome"] = "approved_by_provider"
        self.assertTrue(any("not in enum" in e for e in run(fixture, doc)))

    def test_rejection_may_only_carry_dismissed_as_noise(self) -> None:
        self.assertEqual(
            MODULE.ACTION_PAIRING["reject_smr_claim_set"], ["dismissed_as_noise"]
        )
        self.assertNotIn(
            "dismissed_as_noise", MODULE.ACTION_PAIRING["promote_smr_claim_set"]
        )

    def test_schema_reference_resolves_to_a_real_artifact(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        ref = contract["confirmation_protocol"]["schema"]
        self.assertTrue((MODULE.SCHEMA_DIR / ref).is_file(), f"{ref} does not exist")

    def test_binding_digest_covers_outcome_and_excludes_summary_text(self) -> None:
        amendment = MODULE.load_json(MODULE.AMENDMENT_001)
        digest = amendment["confirmation_protocol_v2"]["binding_digest"]
        self.assertIn("requested_outcome", digest["includes"])
        self.assertIn("display_summary", digest["excludes"])


class ProhibitedNameTest(unittest.TestCase):
    def test_conflated_digest_names_are_absent(self) -> None:
        errors: list[str] = []
        MODULE.check_prohibited_names(errors)
        self.assertEqual(errors, [])

    def test_prohibited_names_list_is_enforced(self) -> None:
        self.assertIn("canonical_snapshot_digest", MODULE.PROHIBITED_DIGEST_NAMES)
        self.assertIn("scope_digest", MODULE.PROHIBITED_DIGEST_NAMES)


class ContractAgreementTest(unittest.TestCase):
    def test_contract_and_spec_agree(self) -> None:
        errors: list[str] = []
        MODULE.check_contract_agreement(errors)
        self.assertEqual(errors, [])

    def test_governance_prohibited_path_is_absent(self) -> None:
        errors: list[str] = []
        MODULE.check_spec_links(errors)
        self.assertEqual(errors, [])

    def test_lane_authorizes_no_implementation_or_release(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        self.assertFalse(contract["authorizes_implementation"])
        self.assertFalse(contract["authorizes_release"])

    def test_nine_typed_run_states_are_preserved(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        self.assertEqual(contract["test_run_states"], MODULE.TEST_RUN_STATES)
        self.assertEqual(len(MODULE.TEST_RUN_STATES), 9)

    def test_only_two_durable_transitions_are_persisted(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        self.assertEqual(
            contract["state_records"]["candidate_lifecycle"]["durable_states"],
            ["proposed", "promoted", "rejected"],
        )
        for term in ("stale", "invalidated", "suppressed", "needs_reanalysis"):
            self.assertIn(term, contract["state_records"]["not_lifecycle_states"])

    def test_parallel_approval_command_stays_forbidden(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        forbidden = " ".join(
            f["description"] for f in contract["forbidden_transitions"]
        )
        self.assertIn("smr.accept_claim_set", forbidden)

    def test_relation_support_authority_ceiling_is_load_bearing(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["relation_support_provenance"]["records"] = "semantic_entailment"
        errors: list[str] = []
        original_load_json = MODULE.load_json
        try:
            MODULE.load_json = (
                lambda path: contract
                if path == MODULE.CONTRACT
                else original_load_json(path)
            )
            MODULE.check_contract_agreement(errors)
        finally:
            MODULE.load_json = original_load_json
        self.assertTrue(any("relation support" in error for error in errors))


class TemporalGovernanceInvariantTest(unittest.TestCase):
    """Bounded counterexamples must be rejected by the machine contract."""

    def test_amendment_002_is_clean(self) -> None:
        errors: list[str] = []
        MODULE.check_amendment_002(errors)
        self.assertEqual(errors, [])

    def test_candidate_generation_has_one_atomic_terminal_disposition(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"].pop("candidate_terminal_disposition")
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(
            any("candidate_terminal_disposition" in error for error in errors)
        )

    def test_role_authority_is_rechecked_at_append(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["append_time_authorization"][
            "challenge_is_authority"
        ] = True
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("append_time_authorization" in error for error in errors))

    def test_stale_base_cannot_append(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["base_snapshot_freshness"][
            "appends_on_mismatch"
        ] = True
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("base_snapshot_freshness" in error for error in errors))

    def test_successful_read_cannot_use_stale_projection(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["projection_read_freshness"][
            "stale_or_empty_fallback"
        ] = True
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("projection_read_freshness" in error for error in errors))

    def test_orphan_replay_fails_closed(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["replay_totality"]["silent_ignore"] = True
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("replay_totality" in error for error in errors))

    def test_claim_set_terminals_must_not_require_generated_predecessor(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["replay_totality"][
            "candidate_generation_predecessor_required"
        ] = True
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("replay_totality" in error for error in errors))

    def test_decision_signoff_still_requires_decision_predecessor(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["replay_totality"][
            "required_predecessors"
        ].pop("approve_signoff")
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("replay_totality" in error for error in errors))

    def test_amendment_004_is_clean(self) -> None:
        errors: list[str] = []
        MODULE.check_amendment_004(errors)
        self.assertEqual(errors, [])

    def test_semantic_atom_alias_cannot_hide_overlap(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["temporal_invariants"]["model_atom_identity"][
            "extractor_supplied_identity_authoritative"
        ] = True
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("model_atom_identity" in error for error in errors))

    def test_owner_signoff_controls_claim_influence(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        contract["state_records"]["decision_signoff_lifecycle"]["authority"] = (
            "product_member"
        )
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(
            any("decision_signoff_lifecycle" in error for error in errors)
        )

    def test_codegraph_is_optional_recall_expansion_only(self) -> None:
        contract = MODULE.load_json(MODULE.CONTRACT)
        scenario = next(
            item
            for item in contract["scenarios"]
            if item["id"] == "SC-CODEGRAPH-OPTIONAL-RECALL"
        )
        scenario["then"] = ["CodeGraph implementation evidence required"]
        errors: list[str] = []
        MODULE.check_temporal_governance(contract, errors)
        self.assertTrue(any("CodeGraph" in error for error in errors))


class RelationSupportProvenanceTest(unittest.TestCase):
    """A25 records input lineage without laundering semantic classification."""

    def test_amendment_is_clean(self) -> None:
        errors: list[str] = []
        MODULE.check_amendment_003(errors)
        self.assertEqual(errors, [])

    def test_valid_relation_support_is_referentially_closed(self) -> None:
        fixture = load("smr-claim-set-proposal.valid.json")
        errors: list[str] = []
        MODULE.validate_relation_support(
            fixture["normalized_claim_set"], "fixture", errors
        )
        self.assertEqual(errors, [])

    def test_dangling_supporting_claim_fails_closed(self) -> None:
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        relation["supporting_claim_ids"] = ["claim.missing"]
        errors: list[str] = []
        MODULE.validate_relation_support(
            fixture["normalized_claim_set"], "fixture", errors
        )
        self.assertTrue(any("unknown same-set Claims" in error for error in errors))

    def test_dangling_supporting_anchor_fails_closed(self) -> None:
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        relation["supporting_source_anchor_ids"] = ["anchor.missing"]
        errors: list[str] = []
        MODULE.validate_relation_support(
            fixture["normalized_claim_set"], "fixture", errors
        )
        self.assertTrue(any("unknown same-set SourceAnchors" in error for error in errors))

    def test_anchor_not_reachable_through_supporting_claim_fails_closed(self) -> None:
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        fixture["normalized_claim_set"]["source_anchors"].append(
            {
                "anchor_id": "anchor.unrelated",
                "artifact_binding_ref": "sha256:" + "a" * 64,
                "byte_start": 0,
                "byte_end": 1,
                "line_start": 1,
                "line_end": 1,
            }
        )
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        relation["supporting_source_anchor_ids"] = ["anchor.unrelated"]
        errors: list[str] = []
        MODULE.validate_relation_support(
            fixture["normalized_claim_set"], "fixture", errors
        )
        self.assertTrue(any("not reachable" in error for error in errors))

    def test_duplicate_relation_support_fails_closed(self) -> None:
        fixture = copy.deepcopy(load("smr-claim-set-proposal.valid.json"))
        relation = fixture["normalized_claim_set"]["atom_relations"][0]
        claim_id = relation["supporting_claim_ids"][0]
        relation["supporting_claim_ids"] = [claim_id, claim_id]
        errors: list[str] = []
        MODULE.validate_relation_support(
            fixture["normalized_claim_set"], "fixture", errors
        )
        self.assertTrue(any("deduplicated and sorted" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
