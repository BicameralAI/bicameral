use std::fs;
use std::path::Path;
use std::process::Command;

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::fixtures;
use bicameral_api::managed_preflight::{
    ExactGoverningSpecBindingV1, ManagedAnalysisStatusV1, ManagedHostKind,
    ManagedPlanningBoundaryV1, ManagedPreflightLimitationCode, ManagedPreflightOutcome,
    ManagedPreflightResultV1, ManagedSemanticClassV1,
};
use bicameral_api::tool_request::{ToolCommand, ToolResponseStatus};
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git must run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output must be UTF-8")
        .trim()
        .to_string()
}

fn exact_spec_repo() -> (TempDir, ExactGoverningSpecBindingV1) {
    let repo = tempfile::tempdir().expect("temp repo");
    git(repo.path(), &["init"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/BicameralAI/example.git",
        ],
    );

    fs::create_dir_all(repo.path().join("docs/specs")).expect("spec directory");
    let spec_bytes = b"# Governing spec\n\nExact retained source.\n";
    fs::write(repo.path().join("docs/specs/governing.md"), spec_bytes).expect("spec bytes");
    git(repo.path(), &["add", "docs/specs/governing.md"]);
    git(repo.path(), &["commit", "-m", "add governing spec"]);

    let git_head = git(repo.path(), &["rev-parse", "HEAD"]);
    let blob_id = git(
        repo.path(),
        &["rev-parse", &format!("{git_head}:docs/specs/governing.md")],
    );
    let artifact_content_digest = format!("sha256:{:x}", Sha256::digest(spec_bytes));

    (
        repo,
        ExactGoverningSpecBindingV1 {
            repository_id: "BicameralAI/example".into(),
            git_head,
            artifact_path: "docs/specs/governing.md".into(),
            blob_id,
            artifact_content_digest,
        },
    )
}

fn managed_request(
    governing_spec: Option<ExactGoverningSpecBindingV1>,
) -> bicameral_api::tool_request::ToolRequest {
    let mut request = fixtures::preflight_run();
    let ToolCommand::PreflightRun(params) = &mut request.command else {
        panic!("fixture must be preflight.run");
    };
    params.managed_planning = Some(ManagedPlanningBoundaryV1 {
        product_id: "product-985".into(),
        host_kind: ManagedHostKind::Codex,
        host_session_id: "session-985".into(),
        host_turn_id: "turn-1".into(),
        plan_digest: format!("sha256:{:064x}", 1),
        governing_spec,
        claim_trace: None,
        analysis_budget_ms: None,
    });
    request
}

fn managed_result(
    response: &bicameral_api::tool_request::ToolResponse,
) -> ManagedPreflightResultV1 {
    serde_json::from_value(
        response.result.as_ref().expect("preflight result")["managed_preflight"].clone(),
    )
    .expect("typed managed preflight result")
}

#[test]
fn managed_preflight_resolves_and_preserves_exact_spec_binding() {
    let (repo, governing_spec) = exact_spec_repo();
    let request = managed_request(Some(governing_spec.clone()));

    let adapter = MemoryAdapter::new();
    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));

    assert_eq!(response.status, ToolResponseStatus::Ok);
    let managed = managed_result(&response);
    assert_eq!(managed.outcome, ManagedPreflightOutcome::BindingValidated);
    assert_eq!(
        managed.governing_spec.as_ref().map(|s| &s.repository_id),
        Some(&governing_spec.repository_id)
    );
    assert!(managed
        .governing_spec_binding_digest
        .as_deref()
        .is_some_and(|digest| digest.starts_with("sha256:")));
    assert!(managed.limitations.is_empty());
    // BindingValidated is not semantic success: no ClaimTrace ⇒ typed unavailable.
    let analysis = managed.analysis.expect("analysis attachment required");
    assert_eq!(
        analysis.status,
        ManagedAnalysisStatusV1::ProviderUnavailable
    );
    assert!(analysis.classes.contains(&ManagedSemanticClassV1::Unknown));
    assert_eq!(
        analysis.limitations[0].code,
        ManagedPreflightLimitationCode::ClaimTraceProviderUnavailable
    );
    assert!(adapter.replay().expect("event replay").is_empty());
}

#[test]
fn managed_preflight_missing_spec_is_typed_no_candidate() {
    let request = managed_request(None);
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, None);

    assert_eq!(response.status, ToolResponseStatus::Ok);
    let managed = managed_result(&response);
    assert_eq!(managed.outcome, ManagedPreflightOutcome::NoCandidate);
    assert_eq!(
        managed.limitations[0].code,
        ManagedPreflightLimitationCode::MissingGoverningSpec
    );
    assert!(managed.governing_spec_binding_digest.is_none());
    assert!(adapter.replay().expect("event replay").is_empty());
}

#[test]
fn managed_preflight_digest_mismatch_is_typed_no_candidate() {
    let (repo, mut governing_spec) = exact_spec_repo();
    governing_spec.artifact_content_digest = format!("sha256:{:064x}", 2);
    let request = managed_request(Some(governing_spec));
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));

    let managed = managed_result(&response);
    assert_eq!(managed.outcome, ManagedPreflightOutcome::NoCandidate);
    assert_eq!(
        managed.limitations[0].code,
        ManagedPreflightLimitationCode::ContentDigestMismatch
    );
    assert!(managed.governing_spec_binding_digest.is_none());
    assert!(adapter.replay().expect("event replay").is_empty());
}

#[test]
fn managed_preflight_rejects_non_normalized_spec_path() {
    let (repo, mut governing_spec) = exact_spec_repo();
    governing_spec.artifact_path = "../governing.md".into();
    let request = managed_request(Some(governing_spec));
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));

    let managed = managed_result(&response);
    assert_eq!(managed.outcome, ManagedPreflightOutcome::NoCandidate);
    assert_eq!(
        managed.limitations[0].code,
        ManagedPreflightLimitationCode::InvalidArtifactPath
    );
}

#[test]
fn legacy_preflight_omits_managed_result() {
    let request = fixtures::preflight_run();
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, None);

    assert_eq!(response.status, ToolResponseStatus::Ok);
    assert!(
        response.result.expect("preflight result")["managed_preflight"].is_null(),
        "manual and legacy preflight must retain the existing response shape"
    );
}

fn public_claim_trace_fixture() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../bicameral-smr/fixtures/claimtrace/valid-adjacent-states.json");
    let bytes = fs::read_to_string(&path).expect("public ClaimTrace fixture must exist for #985");
    serde_json::from_str(&bytes).expect("fixture is ClaimSemanticTraceV1 JSON")
}

fn managed_request_with_trace(
    governing_spec: Option<ExactGoverningSpecBindingV1>,
    claim_trace: Option<serde_json::Value>,
    analysis_budget_ms: Option<u64>,
) -> bicameral_api::tool_request::ToolRequest {
    let mut request = managed_request(governing_spec);
    let ToolCommand::PreflightRun(params) = &mut request.command else {
        panic!("fixture must be preflight.run");
    };
    if let Some(boundary) = params.managed_planning.as_mut() {
        boundary.claim_trace = claim_trace;
        boundary.analysis_budget_ms = analysis_budget_ms;
    }
    request
}

#[test]
fn managed_preflight_composes_fixture_claim_trace_through_kernel() {
    let (repo, governing_spec) = exact_spec_repo();
    let request = managed_request_with_trace(
        Some(governing_spec),
        Some(public_claim_trace_fixture()),
        None,
    );
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let managed = managed_result(&response);
    assert_eq!(managed.outcome, ManagedPreflightOutcome::BindingValidated);
    let analysis = managed.analysis.expect("analysis required");
    assert_eq!(analysis.status, ManagedAnalysisStatusV1::Completed);
    assert_eq!(
        analysis.claim_trace_id.as_deref(),
        Some("TRACE-checkout-retry")
    );
    assert!(analysis.classes.contains(&ManagedSemanticClassV1::Proposed));
    assert!(analysis
        .classes
        .contains(&ManagedSemanticClassV1::Contradiction));
    assert!(analysis.limitations.is_empty());
    // No Product events.
    assert!(adapter.replay().expect("event replay").is_empty());
}

#[test]
fn managed_preflight_invalid_claim_trace_fails_closed() {
    let (repo, governing_spec) = exact_spec_repo();
    let request = managed_request_with_trace(
        Some(governing_spec),
        Some(serde_json::json!({"incomplete": true})),
        None,
    );
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));
    let managed = managed_result(&response);
    assert_eq!(managed.outcome, ManagedPreflightOutcome::BindingValidated);
    let analysis = managed.analysis.expect("analysis required");
    assert_eq!(analysis.status, ManagedAnalysisStatusV1::InvalidTrace);
    assert!(analysis
        .classes
        .contains(&ManagedSemanticClassV1::NoCandidate));
    assert_eq!(
        analysis.limitations[0].code,
        ManagedPreflightLimitationCode::ClaimTraceInvalid
    );
    assert!(adapter.replay().expect("event replay").is_empty());
}

#[test]
fn managed_preflight_zero_budget_is_typed_timeout_not_reassurance() {
    let (repo, governing_spec) = exact_spec_repo();
    let request = managed_request_with_trace(
        Some(governing_spec),
        Some(public_claim_trace_fixture()),
        Some(0),
    );
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));
    let managed = managed_result(&response);
    let analysis = managed.analysis.expect("analysis required");
    assert_eq!(analysis.status, ManagedAnalysisStatusV1::TimedOut);
    assert!(analysis.classes.contains(&ManagedSemanticClassV1::Timeout));
    assert_eq!(
        analysis.limitations[0].code,
        ManagedPreflightLimitationCode::AnalysisTimedOut
    );
    assert!(
        analysis.limitations[0]
            .detail
            .contains("analysis_timed_out"),
        "timeout detail must use the contract phrase"
    );
    assert!(
        !analysis.limitations[0]
            .detail
            .to_lowercase()
            .contains("no conflict"),
        "timeout must not be laundered into no-conflict reassurance"
    );
    assert!(adapter.replay().expect("event replay").is_empty());
}

#[test]
fn managed_preflight_zero_budget_without_claim_trace_is_still_timeout() {
    // Topology force path: analysis_budget_ms: 0 must not be masked as
    // ProviderUnavailable when no ClaimTrace is supplied.
    let (repo, governing_spec) = exact_spec_repo();
    let request = managed_request_with_trace(Some(governing_spec), None, Some(0));
    let adapter = MemoryAdapter::new();

    let response = dispatch_with_workspace(&request, &adapter, Some(repo.path()));
    let managed = managed_result(&response);
    let analysis = managed.analysis.expect("analysis required");
    assert_eq!(analysis.status, ManagedAnalysisStatusV1::TimedOut);
    assert!(analysis.classes.contains(&ManagedSemanticClassV1::Timeout));
    assert_eq!(
        analysis.limitations[0].code,
        ManagedPreflightLimitationCode::AnalysisTimedOut
    );
    assert!(adapter.replay().expect("event replay").is_empty());
}
