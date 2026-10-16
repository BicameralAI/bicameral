# SMR Feedback Synthetic Benchmark Scorer

This scorer supports Bot #1177. It measures supplied assertion results against
independently reviewed synthetic gold. It does not extract assertions, execute
Alloy, admit Product truth, or make a release or external performance claim.

## Run

```sh
python3 scripts/run_smr_feedback_benchmark.py \
  --gold /approved/private/gold.json \
  --predictions /path/to/predictions.json \
  --fixture-pin protocol/smr-feedback-v0.1/fixtures/product-creation-workspace-routing.fixture-pin.json \
  --output /path/to/content-safe-report.json
```

The fixture pin is an independent reviewed identity input. Both gold and
predictions must carry its exact baseline fields; matching each other is not
enough. The gold document also contains `cases`. Each case
has `case_id`, `control_id`, the sanitized note, and canonical assertions with
their ordinal, byte range, note/span digests, verdict, coverage, optional gap
signal, and witness IDs.

The prediction document repeats the exact `baseline` and provides:

```json
{
  "baseline": {},
  "runs": [
    {
      "case_id": "SMRFB-DEV-001",
      "control_id": "reviewed-governed-domain-product-creation-workspace-routing-v1",
      "assertions": [
        {
          "assertion_id": "sha256:<64 lowercase hex>",
          "verdict": "entailed",
          "coverage": "unknown_scope",
          "coverage_limitations": ["partial_scope"],
          "witnesses": ["claim.alpha.c02.connection-resumes-draft"]
        }
      ],
      "operations": {
        "end_to_end_latency_ms": 1,
        "prompt_tokens": 0,
        "output_tokens": 0,
        "cost_usd": 0,
        "provider_failure": false,
        "timeout": false,
        "retry_count": 0
      }
    }
  ]
}
```

Use `unevaluated_reason` instead of `verdict` when semantic evaluation did not
complete. Missing and extra assertions remain visible. The report contains no
raw note text and keeps semantic, safety, coverage, grounding, and operations
metrics separate. Its `report_digest` is a reproducibility hash over sorted
JSON output; it is not a governed Product digest or an RFC 8785 claim.
