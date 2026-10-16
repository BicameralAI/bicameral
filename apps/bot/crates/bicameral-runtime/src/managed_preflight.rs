//! Exact governing-spec resolution and advisory ClaimTrace→kernel composition
//! for managed post-Planning preflight (#985).
//!
//! BindingValidated is not semantic success. Production never loads ClaimTrace
//! fixtures from disk/env; only caller-injected JSON is accepted as untrusted
//! input. No canonical Product events are written on this path.

use std::path::{Component, Path};
use std::process::Command;
use std::time::{Duration, Instant};

use bicameral_api::managed_preflight::{
    ExactGoverningSpecBindingV1, ManagedAnalysisStatusV1, ManagedPlanningBoundaryV1,
    ManagedPreflightLimitationCode, ManagedPreflightLimitationV1, ManagedPreflightOutcome,
    ManagedPreflightResultV1, ManagedSemanticAnalysisV1, ManagedSemanticClassV1,
};
use bicameral_smr::{
    lower_claim_trace, serialize_alloy_snapshot, validate_claim_trace, AlloyCapacity,
    ClaimSemanticTraceV1, CoverageStatus, SourceCatalog, SourceSpan, SourceUnit, TraceLimits,
};
use sha2::{Digest, Sha256};

/// Default advisory analysis budget (60 seconds).
pub const DEFAULT_ANALYSIS_BUDGET_MS: u64 = 60_000;

pub fn resolve_managed_planning_boundary(
    boundary: &ManagedPlanningBoundaryV1,
    workspace_root: Option<&Path>,
) -> ManagedPreflightResultV1 {
    if boundary.product_id.trim().is_empty()
        || boundary.host_session_id.trim().is_empty()
        || boundary.host_turn_id.trim().is_empty()
        || !is_sha256_digest(&boundary.plan_digest)
    {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::InvalidBoundary,
            "managed Planning identity or plan digest is invalid",
        );
    }

    let Some(binding) = boundary.governing_spec.as_ref() else {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::MissingGoverningSpec,
            "managed preflight requires one exact governing specification binding",
        );
    };
    let Some(workspace_root) = workspace_root else {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::WorkspaceUnavailable,
            "the daemon has no workspace from which to resolve retained spec bytes",
        );
    };

    if !is_lower_hex(&binding.git_head, 40) {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::InvalidGitHead,
            "governing spec git_head must be a 40-character lowercase commit SHA",
        );
    }
    if !is_normalized_relative_path(&binding.artifact_path) {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::InvalidArtifactPath,
            "governing spec artifact_path must be a normalized repo-relative path",
        );
    }
    if !(is_lower_hex(&binding.blob_id, 40) || is_lower_hex(&binding.blob_id, 64)) {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::InvalidBlobId,
            "governing spec blob_id must be a lowercase Git object ID",
        );
    }
    if !is_sha256_digest(&binding.artifact_content_digest) {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::InvalidContentDigest,
            "governing spec artifact_content_digest must be sha256 plus 64 lowercase hex characters",
        );
    }

    let remote = match git_output(workspace_root, &["remote", "get-url", "origin"]) {
        Ok(remote) => remote,
        Err(_) => {
            return no_candidate(
                boundary,
                ManagedPreflightLimitationCode::RepositoryMismatch,
                "the workspace origin cannot verify the governing spec repository_id",
            );
        }
    };
    if normalize_repo_id(&remote) != binding.repository_id {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::RepositoryMismatch,
            "the governing spec repository_id does not match the workspace origin",
        );
    }

    if git_status(
        workspace_root,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", binding.git_head),
        ],
    )
    .is_err()
    {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::RevisionUnavailable,
            "the governing spec git_head is not available in the workspace object database",
        );
    }

    let object_spec = format!("{}:{}", binding.git_head, binding.artifact_path);
    let actual_blob = match git_output(workspace_root, &["rev-parse", &object_spec]) {
        Ok(blob) => blob,
        Err(_) => {
            return no_candidate(
                boundary,
                ManagedPreflightLimitationCode::ArtifactUnavailable,
                "the governing spec path does not exist at the supplied git_head",
            );
        }
    };
    if actual_blob != binding.blob_id {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::BlobMismatch,
            "the governing spec blob_id does not match the retained Git object",
        );
    }

    let spec_bytes = match git_bytes(workspace_root, &["show", &object_spec]) {
        Ok(bytes) => bytes,
        Err(_) => {
            return no_candidate(
                boundary,
                ManagedPreflightLimitationCode::ArtifactUnavailable,
                "the governing spec bytes cannot be read from the retained Git object",
            );
        }
    };
    let actual_content_digest = format!("sha256:{:x}", Sha256::digest(&spec_bytes));
    if actual_content_digest != binding.artifact_content_digest {
        return no_candidate(
            boundary,
            ManagedPreflightLimitationCode::ContentDigestMismatch,
            "the governing spec content digest does not match the retained Git bytes",
        );
    }

    let mut result = ManagedPreflightResultV1 {
        outcome: ManagedPreflightOutcome::BindingValidated,
        boundary: boundary.clone(),
        governing_spec: Some(binding.clone()),
        governing_spec_binding_digest: Some(binding_digest(binding)),
        limitations: Vec::new(),
        analysis: None,
    };
    result.analysis = Some(compose_semantic_analysis(boundary));
    result
}

/// Compose advisory analysis after exact-spec binding.
///
/// - No ClaimTrace → ProviderUnavailable (typed; not success).
/// - Invalid ClaimTrace → InvalidTrace fail-closed (not repaired into no-finding).
/// - Valid ClaimTrace → deterministic validate + lower (+ optional Alloy snapshot).
/// - Budget elapsed → TimedOut with honest timeout class (not "no conflict").
pub fn compose_semantic_analysis(
    boundary: &ManagedPlanningBoundaryV1,
) -> ManagedSemanticAnalysisV1 {
    let budget = Duration::from_millis(
        boundary
            .analysis_budget_ms
            .unwrap_or(DEFAULT_ANALYSIS_BUDGET_MS),
    );
    let started = Instant::now();

    // Fail closed on a zero or already-elapsed budget *before* ClaimTrace work so
    // topology can force `advisory_timeout_honest` with `analysis_budget_ms: 0`
    // without needing a deep mock or depending on parse/validation cost.
    if budget.is_zero() || started.elapsed() >= budget {
        return timed_out_analysis();
    }

    let Some(raw_trace) = boundary.claim_trace.as_ref() else {
        return ManagedSemanticAnalysisV1 {
            status: ManagedAnalysisStatusV1::ProviderUnavailable,
            classes: vec![ManagedSemanticClassV1::Unknown, ManagedSemanticClassV1::NoCandidate],
            claim_trace_id: None,
            lowering_status: None,
            finding_count: None,
            limitations: vec![ManagedPreflightLimitationV1 {
                code: ManagedPreflightLimitationCode::ClaimTraceProviderUnavailable,
                detail: "no ClaimTrace was supplied; live extraction is unconfigured and fixtures are not selected in production"
                    .into(),
            }],
        };
    };

    if started.elapsed() >= budget {
        return timed_out_analysis();
    }

    let trace: ClaimSemanticTraceV1 = match serde_json::from_value(raw_trace.clone()) {
        Ok(trace) => trace,
        Err(err) => {
            return ManagedSemanticAnalysisV1 {
                status: ManagedAnalysisStatusV1::InvalidTrace,
                classes: vec![ManagedSemanticClassV1::NoCandidate],
                claim_trace_id: None,
                lowering_status: None,
                finding_count: None,
                limitations: vec![ManagedPreflightLimitationV1 {
                    code: ManagedPreflightLimitationCode::ClaimTraceInvalid,
                    detail: format!("ClaimTrace JSON failed closed under schema parse: {err}"),
                }],
            };
        }
    };

    let catalog = catalog_from_trace_source_refs(&trace);
    let validated = match validate_claim_trace(trace, &catalog, TraceLimits::default()) {
        Ok(validated) => validated,
        Err(err) => {
            return ManagedSemanticAnalysisV1 {
                status: ManagedAnalysisStatusV1::InvalidTrace,
                classes: vec![ManagedSemanticClassV1::NoCandidate],
                claim_trace_id: None,
                lowering_status: None,
                finding_count: None,
                limitations: vec![ManagedPreflightLimitationV1 {
                    code: ManagedPreflightLimitationCode::ClaimTraceInvalid,
                    detail: format!("ClaimTrace validation failed closed: {err}"),
                }],
            };
        }
    };

    if started.elapsed() >= budget {
        return timed_out_analysis();
    }

    let lowered = lower_claim_trace(&validated);
    let mut classes = vec![
        ManagedSemanticClassV1::Proposed,
        ManagedSemanticClassV1::Evidence,
        ManagedSemanticClassV1::SearchedScope,
    ];
    if !validated.trace.semantic_relations.is_empty() {
        // Distinct/identity relations are advisory contradiction signals, not Product authority.
        classes.push(ManagedSemanticClassV1::Contradiction);
    }
    if matches!(
        validated.trace.coverage.status,
        CoverageStatus::Partial | CoverageStatus::Unknown
    ) {
        classes.push(ManagedSemanticClassV1::Unknown);
    }

    // Alloy snapshot is advisory evidence. Live solver is optional; serialization
    // proves the runtime-derived snapshot path without claiming global safety.
    let finding_count = match serialize_alloy_snapshot(&lowered, AlloyCapacity::alpha_default()) {
        Ok(_snapshot) => Some(0),
        Err(_) => None,
    };

    if started.elapsed() >= budget {
        return timed_out_analysis();
    }

    let lowering_status = if lowered.loss_manifest.is_empty() {
        "lowered".to_string()
    } else {
        format!("loss_manifest:{}", lowered.loss_manifest.len())
    };

    ManagedSemanticAnalysisV1 {
        status: ManagedAnalysisStatusV1::Completed,
        classes,
        claim_trace_id: Some(validated.trace.trace_id.clone()),
        lowering_status: Some(lowering_status),
        finding_count,
        limitations: Vec::new(),
    }
}

fn timed_out_analysis() -> ManagedSemanticAnalysisV1 {
    ManagedSemanticAnalysisV1 {
        status: ManagedAnalysisStatusV1::TimedOut,
        classes: vec![ManagedSemanticClassV1::Timeout, ManagedSemanticClassV1::Unknown],
        claim_trace_id: None,
        lowering_status: None,
        finding_count: None,
        limitations: vec![ManagedPreflightLimitationV1 {
            code: ManagedPreflightLimitationCode::AnalysisTimedOut,
            detail: "analysis_timed_out: advisory budget elapsed; known scope is incomplete and this is not a no-conflict result"
                .into(),
        }],
    }
}

/// Build a minimal SourceCatalog from ClaimTrace source_unit_refs.
///
/// Catalog construction is structural only — it does not invent Product truth.
/// Missing content digests use placeholder digests so validation can run on
/// caller-supplied traces that only declare unit IDs (fixture / provider port).
fn catalog_from_trace_source_refs(trace: &ClaimSemanticTraceV1) -> SourceCatalog {
    use std::collections::BTreeSet;

    let mut ids = BTreeSet::new();
    for occurrence in &trace.occurrences {
        ids.extend(occurrence.source_unit_refs.iter().cloned());
    }
    for assertion in &trace.assertions {
        ids.extend(assertion.source_unit_refs.iter().cloned());
    }
    for relation in &trace.semantic_relations {
        ids.extend(relation.source_unit_refs.iter().cloned());
    }

    let units: Vec<SourceUnit> = ids
        .into_iter()
        .enumerate()
        .map(|(idx, source_unit_id)| {
            let idx_u64 = idx as u64;
            SourceUnit {
                source_unit_id,
                span: SourceSpan {
                    byte_start: idx_u64 * 10,
                    byte_end: idx_u64 * 10 + 9,
                    line_start: (idx + 1) as u32,
                    line_end: (idx + 1) as u32,
                },
                content_digest: format!("sha256:unit-placeholder-{idx}"),
            }
        })
        .collect();

    SourceCatalog::new(units).unwrap_or_else(|_| {
        // Empty catalog: validation will fail closed with EmptyCollection.
        SourceCatalog::new([SourceUnit {
            source_unit_id: "UNIT-EMPTY".into(),
            span: SourceSpan {
                byte_start: 0,
                byte_end: 1,
                line_start: 1,
                line_end: 1,
            },
            content_digest: "sha256:empty".into(),
        }])
        .expect("singleton catalog is valid")
    })
}

fn no_candidate(
    boundary: &ManagedPlanningBoundaryV1,
    code: ManagedPreflightLimitationCode,
    detail: &str,
) -> ManagedPreflightResultV1 {
    ManagedPreflightResultV1 {
        outcome: ManagedPreflightOutcome::NoCandidate,
        boundary: boundary.clone(),
        governing_spec: boundary.governing_spec.clone(),
        governing_spec_binding_digest: None,
        limitations: vec![ManagedPreflightLimitationV1 {
            code,
            detail: detail.to_string(),
        }],
        analysis: Some(ManagedSemanticAnalysisV1 {
            status: ManagedAnalysisStatusV1::NotAttempted,
            classes: vec![ManagedSemanticClassV1::NoCandidate],
            claim_trace_id: None,
            lowering_status: None,
            finding_count: None,
            limitations: Vec::new(),
        }),
    }
}

fn binding_digest(binding: &ExactGoverningSpecBindingV1) -> String {
    let canonical = serde_json::to_vec(binding)
        .expect("ExactGoverningSpecBindingV1 serialization is infallible");
    format!("sha256:{:x}", Sha256::digest(canonical))
}

fn is_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| is_lower_hex(digest, 64))
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_normalized_relative_path(value: &str) -> bool {
    if value.is_empty() || value.contains('\\') || value.contains("//") || value.ends_with('/') {
        return false;
    }
    Path::new(value)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn normalize_repo_id(remote: &str) -> String {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .unwrap_or(remote)
        .to_string()
}

fn git_status(workspace_root: &Path, args: &[&str]) -> Result<(), ()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .status()
        .map_err(|_| ())?;
    status.success().then_some(()).ok_or(())
}

fn git_output(workspace_root: &Path, args: &[&str]) -> Result<String, ()> {
    let output = git_bytes(workspace_root, args)?;
    String::from_utf8(output)
        .map(|value| value.trim().to_string())
        .map_err(|_| ())
}

fn git_bytes(workspace_root: &Path, args: &[&str]) -> Result<Vec<u8>, ()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .map_err(|_| ())?;
    output.status.success().then_some(output.stdout).ok_or(())
}
