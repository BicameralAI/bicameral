//! Proves the composition-stack fixture location feeds the real kernel API.

use std::fs;
use std::path::PathBuf;

use bicameral_smr::{
    validate_claim_trace, ClaimSemanticTraceV1, SourceCatalog, SourceSpan, SourceUnit, TraceLimits,
};

fn public_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/claimtrace")
        .join(name)
}

fn source_catalog() -> SourceCatalog {
    SourceCatalog::new([
        SourceUnit {
            source_unit_id: "UNIT-1".into(),
            span: SourceSpan {
                byte_start: 0,
                byte_end: 18,
                line_start: 1,
                line_end: 1,
            },
            content_digest: "sha256:unit-1".into(),
        },
        SourceUnit {
            source_unit_id: "UNIT-2".into(),
            span: SourceSpan {
                byte_start: 19,
                byte_end: 47,
                line_start: 2,
                line_end: 2,
            },
            content_digest: "sha256:unit-2".into(),
        },
    ])
    .expect("catalog is valid")
}

#[test]
fn public_claimtrace_fixture_validates_through_real_kernel_api() {
    let path = public_fixture("valid-adjacent-states.json");
    let bytes = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "public fixture must exist for #985 stacking at {}: {e}",
            path.display()
        )
    });
    let trace: ClaimSemanticTraceV1 =
        serde_json::from_str(&bytes).expect("public fixture is ClaimSemanticTraceV1 JSON");

    let validated = validate_claim_trace(trace, &source_catalog(), TraceLimits::default())
        .expect("public happy-path fixture must validate through real API");

    assert_eq!(validated.trace.trace_id, "TRACE-checkout-retry");
    assert!(
        !validated.trace.occurrences.is_empty(),
        "fixture must carry occurrences for composition stacks"
    );
}

#[test]
fn public_invalid_fixture_fails_closed_without_silent_repair() {
    let path = public_fixture("invalid-incomplete.json");
    let bytes = fs::read_to_string(&path).expect("invalid fixture must exist");
    let parse = serde_json::from_str::<ClaimSemanticTraceV1>(&bytes);
    assert!(
        parse.is_err(),
        "incomplete public fixture must not parse into a valid ClaimSemanticTraceV1 (fail closed)"
    );
}
