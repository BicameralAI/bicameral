#!/usr/bin/env python3
"""Score identity-bound SMR feedback results without retaining raw notes."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
from pathlib import Path
from typing import Any


VERDICTS = ("entailed", "refuted", "underdetermined", "conflict")
ASSERTION_TAG = b"smr-feedback-assertion-v1\x00"
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
FILE_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")


class BenchmarkError(ValueError):
    pass


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def stable_json_bytes(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, allow_nan=False, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")


def assertion_identity(
    note: str, ordinal: int, start_byte: int, end_byte: int
) -> dict[str, Any]:
    note_bytes = note.encode("utf-8")
    if (
        not all(
            isinstance(value, int) and not isinstance(value, bool)
            for value in (ordinal, start_byte, end_byte)
        )
        or ordinal < 0
        or start_byte < 0
        or end_byte <= start_byte
        or end_byte > len(note_bytes)
    ):
        raise BenchmarkError("assertion span or ordinal is out of range")
    span = note_bytes[start_byte:end_byte]
    try:
        span.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise BenchmarkError("assertion span splits a UTF-8 code point") from exc

    note_digest = digest(note_bytes)
    span_digest = digest(span)
    preimage = ASSERTION_TAG + b"\x00".join(
        (
            note_digest.encode(),
            str(ordinal).encode(),
            str(start_byte).encode(),
            str(end_byte).encode(),
            span_digest.encode(),
        )
    )
    return {
        "assertion_id": digest(preimage),
        "note_digest": note_digest,
        "start_byte": start_byte,
        "end_byte": end_byte,
        "span_digest": span_digest,
    }


def _load(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise BenchmarkError(f"cannot load {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise BenchmarkError(f"{path} must contain a JSON object")
    return value


def baseline_from_pin(pin: dict[str, Any]) -> dict[str, Any]:
    required = {
        "fixture_id",
        "fixture_file_sha256",
        "fixture_head_commit",
        "merged_commit",
        "claim_set_digest",
        "analysis_key",
        "scope_id",
        "scope_completeness",
    }
    if set(pin) != required:
        raise BenchmarkError("fixture pin fields do not match v0.1")
    if not isinstance(pin["fixture_id"], str) or not pin["fixture_id"]:
        raise BenchmarkError("fixture pin has an invalid fixture_id")
    if not FILE_SHA256_RE.fullmatch(pin["fixture_file_sha256"]):
        raise BenchmarkError("fixture pin has an invalid file digest")
    if not COMMIT_RE.fullmatch(pin["fixture_head_commit"]) or not COMMIT_RE.fullmatch(
        pin["merged_commit"]
    ):
        raise BenchmarkError("fixture pin has an invalid review commit")
    if not SHA256_RE.fullmatch(pin["claim_set_digest"]):
        raise BenchmarkError("fixture pin has an invalid Claim-set digest")
    for field in ("analysis_key", "scope_id"):
        if not isinstance(pin[field], str) or not pin[field]:
            raise BenchmarkError(f"fixture pin has an invalid {field}")
    if pin["scope_completeness"] not in ("partial", "complete"):
        raise BenchmarkError("fixture pin has invalid scope completeness")
    return {
        key: pin[key]
        for key in (
            "fixture_file_sha256",
            "fixture_head_commit",
            "merged_commit",
            "claim_set_digest",
            "scope_id",
            "scope_completeness",
        )
    } | {"product_authority": False}


def _validate_result_fields(row: dict[str, Any], context: str) -> None:
    coverage = row.get("coverage")
    limitations = row.get("coverage_limitations", [])
    signal = row.get("discovery_signal")
    witnesses = row.get("witnesses", [])
    if coverage not in ("complete_named_baseline", "unknown_scope"):
        raise BenchmarkError(f"invalid coverage in {context}")
    if (
        not isinstance(limitations, list)
        or any(not isinstance(value, str) or not value for value in limitations)
        or len(limitations) != len(set(limitations))
    ):
        raise BenchmarkError(f"invalid coverage limitations in {context}")
    if coverage == "unknown_scope" and not limitations:
        raise BenchmarkError(f"unknown scope requires a coverage limitation in {context}")
    if coverage == "complete_named_baseline" and limitations:
        raise BenchmarkError(f"complete coverage cannot have limitations in {context}")
    if signal not in (None, "candidate_product_gap"):
        raise BenchmarkError(f"invalid discovery signal in {context}")
    if signal == "candidate_product_gap" and coverage != "complete_named_baseline":
        raise BenchmarkError(
            f"candidate product gap requires complete named coverage in {context}"
        )
    if signal == "candidate_product_gap" and row.get("verdict") != "underdetermined":
        raise BenchmarkError(
            f"candidate product gap requires an underdetermined verdict in {context}"
        )
    if (
        not isinstance(witnesses, list)
        or any(not isinstance(value, str) or not value for value in witnesses)
        or len(witnesses) != len(set(witnesses))
    ):
        raise BenchmarkError(f"invalid witnesses in {context}")


def _flatten_gold(
    gold: dict[str, Any],
) -> tuple[dict[tuple[str, str], dict[str, Any]], dict[str, str]]:
    rows: dict[tuple[str, str], dict[str, Any]] = {}
    case_controls: dict[str, str] = {}
    cases = gold.get("cases", [])
    if not isinstance(cases, list):
        raise BenchmarkError("gold cases must be an array")
    for case in cases:
        if not isinstance(case, dict):
            raise BenchmarkError("gold cases must contain objects")
        case_id = case.get("case_id")
        control_id = case.get("control_id")
        note = case.get("note")
        if (
            not isinstance(case_id, str)
            or not isinstance(control_id, str)
            or not isinstance(note, str)
            or case_id in case_controls
        ):
            raise BenchmarkError("gold contains an invalid or duplicate case_id")
        if len(note.encode("utf-8")) > 32768:
            raise BenchmarkError(f"gold note exceeds the 32 KiB limit in {case_id}")
        case_controls[case_id] = control_id
        assertions = case.get("assertions", [])
        if not isinstance(assertions, list):
            raise BenchmarkError(f"gold assertions must be an array in {case_id}")
        for row in assertions:
            if not isinstance(row, dict):
                raise BenchmarkError(f"gold assertions must contain objects in {case_id}")
            identity = assertion_identity(
                note, row.get("ordinal"), row.get("start_byte"), row.get("end_byte")
            )
            assertion_id = row.get("assertion_id")
            if not isinstance(assertion_id, str) or not SHA256_RE.fullmatch(assertion_id):
                raise BenchmarkError(f"invalid gold assertion_id in {case_id}")
            if assertion_id != identity["assertion_id"]:
                raise BenchmarkError(f"gold assertion identity mismatch in {case_id}")
            if row.get("note_digest") != identity["note_digest"]:
                raise BenchmarkError(f"gold note digest mismatch in {case_id}")
            if row.get("span_digest") != identity["span_digest"]:
                raise BenchmarkError(f"gold span digest mismatch in {case_id}")
            key = (case_id, assertion_id)
            if key in rows:
                raise BenchmarkError(f"duplicate gold assertion_id in {case_id}: {assertion_id}")
            if row.get("verdict") not in VERDICTS:
                raise BenchmarkError(f"invalid gold verdict in {case_id}")
            _validate_result_fields(row, f"gold {case_id}")
            rows[key] = {**row, "case_id": case_id}
    if not rows:
        raise BenchmarkError("gold contains no assertions")
    return rows, case_controls


def _flatten_predictions(
    predictions: dict[str, Any], case_controls: dict[str, str]
) -> tuple[dict[tuple[str, str], dict[str, Any]], list[dict[str, Any]]]:
    rows: dict[tuple[str, str], dict[str, Any]] = {}
    operations: list[dict[str, Any]] = []
    seen_cases: set[str] = set()
    runs = predictions.get("runs", [])
    if not isinstance(runs, list):
        raise BenchmarkError("prediction runs must be an array")
    for run in runs:
        if not isinstance(run, dict):
            raise BenchmarkError("prediction runs must contain objects")
        case_id = run.get("case_id")
        if case_id not in case_controls or case_id in seen_cases:
            raise BenchmarkError("predictions contain an unknown or duplicate case_id")
        if run.get("control_id") != case_controls[case_id]:
            raise BenchmarkError(f"prediction control identity mismatch in {case_id}")
        seen_cases.add(case_id)
        operation = run.get("operations", {})
        if not isinstance(operation, dict):
            raise BenchmarkError(f"operations must be an object in {case_id}")
        for key in (
            "end_to_end_latency_ms",
            "prompt_tokens",
            "output_tokens",
            "cost_usd",
            "retry_count",
        ):
            value = operation.get(key, 0)
            if (
                not isinstance(value, (int, float))
                or isinstance(value, bool)
                or value < 0
            ):
                raise BenchmarkError(f"invalid non-negative operation value {key} in {case_id}")
        for key in ("provider_failure", "timeout"):
            if key in operation and not isinstance(operation[key], bool):
                raise BenchmarkError(f"operation value {key} must be boolean in {case_id}")
        operations.append(operation)
        assertions = run.get("assertions", [])
        if not isinstance(assertions, list):
            raise BenchmarkError(f"prediction assertions must be an array in {case_id}")
        for row in assertions:
            if not isinstance(row, dict):
                raise BenchmarkError(
                    f"prediction assertions must contain objects in {case_id}"
                )
            assertion_id = row.get("assertion_id")
            if (
                not isinstance(assertion_id, str)
                or not SHA256_RE.fullmatch(assertion_id)
                or (case_id, assertion_id) in rows
            ):
                raise BenchmarkError("predictions contain an invalid or duplicate assertion_id")
            verdict = row.get("verdict")
            unevaluated = row.get("unevaluated_reason")
            has_reason = isinstance(unevaluated, str) and bool(unevaluated)
            if (verdict in VERDICTS) == has_reason:
                raise BenchmarkError(
                    f"{assertion_id} must have exactly one verdict or unevaluated reason"
                )
            if verdict is not None and verdict not in VERDICTS:
                raise BenchmarkError(f"invalid predicted verdict: {verdict}")
            _validate_result_fields(row, f"prediction {case_id}:{assertion_id}")
            rows[(case_id, assertion_id)] = {**row, "case_id": case_id}
    return rows, operations


def _ratio(numerator: int, denominator: int) -> dict[str, Any]:
    return {
        "numerator": numerator,
        "denominator": denominator,
        "value": None if denominator == 0 else numerator / denominator,
    }


def _percentile(values: list[float], percentile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(percentile * len(ordered)) - 1)]


def score(
    gold: dict[str, Any], predictions: dict[str, Any], fixture_pin: dict[str, Any]
) -> dict[str, Any]:
    expected_baseline = baseline_from_pin(fixture_pin)
    if gold.get("baseline") != expected_baseline:
        raise BenchmarkError("gold is not bound to the exact reviewed fixture pin")
    if gold.get("baseline") != predictions.get("baseline"):
        raise BenchmarkError("prediction baseline identity does not exactly match gold")

    gold_rows, case_controls = _flatten_gold(gold)
    predicted_rows, operations = _flatten_predictions(predictions, case_controls)
    missing = sorted(set(gold_rows) - set(predicted_rows))
    extra = sorted(set(predicted_rows) - set(gold_rows))

    confusion = {actual: {predicted: 0 for predicted in VERDICTS} for actual in VERDICTS}
    unevaluated = 0
    exact = 0
    false_safe: list[str] = []
    structural_errors: list[dict[str, str]] = []
    unknown_tp = unknown_fp = unknown_fn = 0
    gap_tp = gap_fp = gap_fn = 0
    witness_tp = witness_fp = witness_fn = 0

    for key, expected in gold_rows.items():
        case_id, assertion_id = key
        actual_verdict = expected["verdict"]
        predicted = predicted_rows.get(key)
        expected_unknown = expected["coverage"] == "unknown_scope"
        expected_gap = expected.get("discovery_signal") == "candidate_product_gap"
        expected_witnesses = set(expected.get("witnesses", []))
        if predicted is None:
            unknown_fn += expected["coverage"] == "unknown_scope"
            gap_fn += expected.get("discovery_signal") == "candidate_product_gap"
            witness_fn += len(expected_witnesses)
            continue

        predicted_unknown = predicted["coverage"] == "unknown_scope"
        predicted_gap = predicted.get("discovery_signal") == "candidate_product_gap"
        predicted_witnesses = set(predicted.get("witnesses", []))
        unknown_tp += expected_unknown and predicted_unknown
        unknown_fp += not expected_unknown and predicted_unknown
        unknown_fn += expected_unknown and not predicted_unknown
        gap_tp += expected_gap and predicted_gap
        gap_fp += not expected_gap and predicted_gap
        gap_fn += expected_gap and not predicted_gap
        witness_tp += len(expected_witnesses & predicted_witnesses)
        witness_fp += len(predicted_witnesses - expected_witnesses)
        witness_fn += len(expected_witnesses - predicted_witnesses)
        if predicted.get("verdict") is None:
            unevaluated += 1
            continue
        predicted_verdict = predicted["verdict"]
        confusion[actual_verdict][predicted_verdict] += 1
        exact += actual_verdict == predicted_verdict
        if actual_verdict in ("refuted", "conflict") and predicted_verdict == "entailed":
            false_safe.append(f"{case_id}:{assertion_id}")

    for key in extra:
        unknown_fp += predicted_rows[key]["coverage"] == "unknown_scope"
        gap_fp += predicted_rows[key].get("discovery_signal") == "candidate_product_gap"
        witness_fp += len(set(predicted_rows[key].get("witnesses", [])))

    semantic: dict[str, Any] = {}
    for verdict in VERDICTS:
        tp = confusion[verdict][verdict]
        fp = sum(confusion[actual][verdict] for actual in VERDICTS if actual != verdict)
        fn = sum(confusion[verdict][predicted] for predicted in VERDICTS if predicted != verdict)
        fn += sum(1 for key in missing if gold_rows[key]["verdict"] == verdict)
        fn += sum(
            1
            for key, predicted in predicted_rows.items()
            if key in gold_rows
            and predicted.get("verdict") is None
            and gold_rows[key]["verdict"] == verdict
        )
        fp += sum(1 for key in extra if predicted_rows[key].get("verdict") == verdict)
        semantic[verdict] = {
            "precision": _ratio(tp, tp + fp),
            "recall": _ratio(tp, tp + fn),
            "f1": _ratio(2 * tp, 2 * tp + fp + fn),
        }

    latencies = [
        op["end_to_end_latency_ms"]
        for op in operations
        if isinstance(op.get("end_to_end_latency_ms"), (int, float))
    ]
    report = {
        "report_version": "smr-feedback-benchmark-report/v1",
        "baseline": gold["baseline"],
        "mechanical": {
            "gold_assertion_count": len(gold_rows),
            "predicted_assertion_count": len(predicted_rows),
            "missing_assertions": [
                {"case_id": case_id, "assertion_id": assertion_id}
                for case_id, assertion_id in missing
            ],
            "extra_assertions": [
                {"case_id": case_id, "assertion_id": assertion_id}
                for case_id, assertion_id in extra
            ],
            "unevaluated_count": unevaluated,
            "structural_errors": structural_errors,
        },
        "semantic": {
            "per_verdict": semantic,
            "exact_match": _ratio(exact, len(gold_rows)),
            "confusion": confusion,
        },
        "safety": {
            "critical_false_safe_count": len(false_safe),
            "critical_false_safe_assertion_ids": false_safe,
        },
        "coverage": {
            "unknown_scope_precision": _ratio(unknown_tp, unknown_tp + unknown_fp),
            "unknown_scope_recall": _ratio(unknown_tp, unknown_tp + unknown_fn),
            "candidate_gap_precision": _ratio(gap_tp, gap_tp + gap_fp),
            "candidate_gap_recall": _ratio(gap_tp, gap_tp + gap_fn),
        },
        "grounding": {
            "witness_precision": _ratio(witness_tp, witness_tp + witness_fp),
            "witness_recall": _ratio(witness_tp, witness_tp + witness_fn),
        },
        "operations": {
            "run_count": len(operations),
            "latency_observation_count": len(latencies),
            "end_to_end_latency_ms_p50": _percentile(latencies, 0.50),
            "end_to_end_latency_ms_p95": _percentile(latencies, 0.95),
            "prompt_tokens": sum(op.get("prompt_tokens", 0) for op in operations),
            "output_tokens": sum(op.get("output_tokens", 0) for op in operations),
            "cost_usd": sum(op.get("cost_usd", 0) for op in operations),
            "provider_failure_count": sum(bool(op.get("provider_failure")) for op in operations),
            "timeout_count": sum(bool(op.get("timeout")) for op in operations),
            "retry_count": sum(op.get("retry_count", 0) for op in operations),
        },
        "limitations": [
            "synthetic_corpus_only",
            "experiment_board_not_product_truth",
            "not_release_or_external_claim_evidence",
        ],
    }
    report["report_digest"] = digest(stable_json_bytes(report))
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gold", type=Path, required=True)
    parser.add_argument("--predictions", type=Path, required=True)
    parser.add_argument("--fixture-pin", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        report = score(
            _load(args.gold),
            _load(args.predictions),
            _load(args.fixture_pin),
        )
    except BenchmarkError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    rendered = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
