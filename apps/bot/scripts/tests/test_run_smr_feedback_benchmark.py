import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "run_smr_feedback_benchmark", ROOT / "scripts" / "run_smr_feedback_benchmark.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class SmrFeedbackBenchmarkTest(unittest.TestCase):
    def setUp(self) -> None:
        note = "Alpha is required. Beta is forbidden."
        alpha_start = note.index("Alpha")
        alpha_end = alpha_start + len("Alpha is required.")
        beta_start = note.index("Beta")
        beta_end = beta_start + len("Beta is forbidden.")
        alpha = MODULE.assertion_identity(note, 0, alpha_start, alpha_end)
        beta = MODULE.assertion_identity(note, 1, beta_start, beta_end)
        self.pin = {
            "fixture_id": "product-creation-workspace-routing-v1",
            "fixture_file_sha256": "bad78bae59137eb60c91f9863d5954f7ac677edcd31e97fec0630a0870e64a64",
            "fixture_head_commit": "bcbf14b638c44d460beadfd1f01fbbf33820fb7b",
            "merged_commit": "d2b83826579c648da9b5cee6b9a92088ef3768a5",
            "claim_set_digest": "sha256:350fa9f73cdf6b80e42822371973b27c1eb6611b52416cf2a92e7ac3bf7c98ae",
            "analysis_key": "fixture.smr-feedback.product-creation-workspace-routing-v1",
            "scope_id": "scope.alpha.product-creation-workspace-routing",
            "scope_completeness": "partial",
        }
        self.gold = {
            "baseline": MODULE.baseline_from_pin(self.pin),
            "cases": [
                {
                    "case_id": "case-1",
                    "control_id": "reviewed-board",
                    "note": note,
                    "assertions": [
                        {
                            **alpha,
                            "ordinal": 0,
                            "verdict": "entailed",
                            "coverage": "unknown_scope",
                            "coverage_limitations": ["partial_scope"],
                            "witnesses": ["claim-1"],
                        },
                        {
                            **beta,
                            "ordinal": 1,
                            "verdict": "refuted",
                            "coverage": "complete_named_baseline",
                            "coverage_limitations": [],
                            "discovery_signal": None,
                            "witnesses": ["claim-2"],
                        },
                    ],
                }
            ],
        }
        self.predictions = {
            "baseline": self.gold["baseline"],
            "runs": [
                {
                    "case_id": "case-1",
                    "control_id": "reviewed-board",
                    "assertions": [
                        {
                            "assertion_id": alpha["assertion_id"],
                            "verdict": "entailed",
                            "coverage": "unknown_scope",
                            "coverage_limitations": ["partial_scope"],
                            "witnesses": ["claim-1"],
                        },
                        {
                            "assertion_id": beta["assertion_id"],
                            "verdict": "entailed",
                            "coverage": "complete_named_baseline",
                            "coverage_limitations": [],
                            "witnesses": [],
                        },
                    ],
                    "operations": {
                        "end_to_end_latency_ms": 12,
                        "prompt_tokens": 10,
                        "output_tokens": 4,
                        "cost_usd": 0.01,
                    },
                }
            ],
        }

    def test_assertion_identity_is_stable_and_sensitive(self) -> None:
        value = MODULE.assertion_identity("Alpha is required.", 0, 0, 18)
        self.assertEqual(
            value["assertion_id"],
            "sha256:201828e374f4a424368340e48b150cc978b4fa90fc639e32dbc29ad97afabac5",
        )
        changed = MODULE.assertion_identity("Alpha is required!", 0, 0, 18)
        self.assertNotEqual(value["assertion_id"], changed["assertion_id"])

    def test_score_keeps_false_safe_and_layered_metrics_visible(self) -> None:
        report = MODULE.score(self.gold, self.predictions, self.pin)
        self.assertEqual(report["safety"]["critical_false_safe_count"], 1)
        self.assertEqual(
            report["semantic"]["per_verdict"]["entailed"]["precision"]["denominator"], 2
        )
        self.assertEqual(report["grounding"]["witness_recall"]["numerator"], 1)
        self.assertEqual(report["operations"]["end_to_end_latency_ms_p95"], 12)

    def test_missing_prediction_stays_in_recall_denominator(self) -> None:
        self.predictions["runs"][0]["assertions"].pop()
        report = MODULE.score(self.gold, self.predictions, self.pin)
        self.assertEqual(len(report["mechanical"]["missing_assertions"]), 1)
        self.assertEqual(
            report["semantic"]["per_verdict"]["refuted"]["recall"]["denominator"],
            1,
        )

    def test_same_assertion_identity_is_allowed_under_distinct_case_controls(self) -> None:
        second_case = copy.deepcopy(self.gold["cases"][0])
        second_case["case_id"] = "case-2"
        second_case["control_id"] = "synthetic-control"
        second_run = copy.deepcopy(self.predictions["runs"][0])
        second_run["case_id"] = "case-2"
        second_run["control_id"] = "synthetic-control"
        self.gold["cases"].append(second_case)
        self.predictions["runs"].append(second_run)
        report = MODULE.score(self.gold, self.predictions, self.pin)
        self.assertEqual(report["mechanical"]["gold_assertion_count"], 4)

    def test_partial_scope_gap_is_rejected_before_scoring(self) -> None:
        first = self.predictions["runs"][0]["assertions"][0]
        first["verdict"] = "underdetermined"
        first["discovery_signal"] = "candidate_product_gap"
        with self.assertRaisesRegex(
            MODULE.BenchmarkError, "candidate product gap requires complete named coverage"
        ):
            MODULE.score(self.gold, self.predictions, self.pin)

    def test_null_verdict_with_reason_counts_as_unevaluated(self) -> None:
        first = self.predictions["runs"][0]["assertions"][0]
        first["verdict"] = None
        first["unevaluated_reason"] = "mapping_unavailable"
        report = MODULE.score(self.gold, self.predictions, self.pin)
        self.assertEqual(report["mechanical"]["unevaluated_count"], 1)
        self.assertEqual(
            report["semantic"]["per_verdict"]["entailed"]["recall"],
            {"numerator": 0, "denominator": 1, "value": 0.0},
        )

    def test_extra_prediction_stays_in_precision_denominator(self) -> None:
        extra_id = "sha256:" + "f" * 64
        self.predictions["runs"][0]["assertions"].append(
            {
                "assertion_id": extra_id,
                "verdict": "entailed",
                "coverage": "complete_named_baseline",
                "coverage_limitations": [],
                "witnesses": [],
            }
        )
        report = MODULE.score(self.gold, self.predictions, self.pin)
        self.assertEqual(
            report["mechanical"]["extra_assertions"],
            [{"case_id": "case-1", "assertion_id": extra_id}],
        )
        self.assertEqual(
            report["semantic"]["per_verdict"]["entailed"]["precision"][
                "denominator"
            ],
            3,
        )

    def test_unknown_scope_without_a_limitation_fails_closed(self) -> None:
        del self.predictions["runs"][0]["assertions"][0]["coverage_limitations"]
        with self.assertRaisesRegex(MODULE.BenchmarkError, "requires a coverage limitation"):
            MODULE.score(self.gold, self.predictions, self.pin)

    def test_baseline_or_assertion_identity_mismatch_fails_closed(self) -> None:
        self.predictions["baseline"] = {"fixture_sha256": "changed"}
        with self.assertRaisesRegex(MODULE.BenchmarkError, "baseline identity"):
            MODULE.score(self.gold, self.predictions, self.pin)
        self.predictions["baseline"] = self.gold["baseline"]
        self.gold["cases"][0]["assertions"][0]["assertion_id"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(MODULE.BenchmarkError, "assertion identity"):
            MODULE.score(self.gold, self.predictions, self.pin)

    def test_second_reviewed_pin_needs_no_scorer_constant(self) -> None:
        second_pin = copy.deepcopy(self.pin)
        second_pin.update(
            {
                "fixture_id": "second-synthetic-domain-v1",
                "fixture_file_sha256": "1" * 64,
                "fixture_head_commit": "2" * 40,
                "merged_commit": "3" * 40,
                "claim_set_digest": "sha256:" + "4" * 64,
                "analysis_key": "fixture.smr-feedback.second-synthetic-domain-v1",
                "scope_id": "scope.synthetic.second-domain",
            }
        )
        self.gold["baseline"] = MODULE.baseline_from_pin(second_pin)
        self.predictions["baseline"] = self.gold["baseline"]
        report = MODULE.score(self.gold, self.predictions, second_pin)
        self.assertEqual(report["baseline"]["fixture_file_sha256"], "1" * 64)

    def test_cli_emits_deterministic_content_safe_report(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            gold_path = tmp_path / "gold.json"
            predictions_path = tmp_path / "predictions.json"
            pin_path = tmp_path / "fixture-pin.json"
            output_path = tmp_path / "report.json"
            gold_path.write_text(json.dumps(self.gold), encoding="utf-8")
            predictions_path.write_text(json.dumps(self.predictions), encoding="utf-8")
            pin_path.write_text(json.dumps(self.pin), encoding="utf-8")
            arguments = [
                "--gold",
                str(gold_path),
                "--predictions",
                str(predictions_path),
                "--fixture-pin",
                str(pin_path),
                "--output",
                str(output_path),
            ]
            self.assertEqual(MODULE.main(arguments), 0)
            report = json.loads(output_path.read_text(encoding="utf-8"))
            self.assertNotIn("Alpha is required", output_path.read_text(encoding="utf-8"))
            first_digest = report["report_digest"]
            self.assertEqual(MODULE.main(arguments), 0)
            self.assertEqual(
                json.loads(output_path.read_text())["report_digest"], first_digest
            )

if __name__ == "__main__":
    unittest.main()
