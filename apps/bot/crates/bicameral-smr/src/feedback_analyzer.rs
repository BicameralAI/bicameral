//! Pinned Alloy execution and verdict mapping for an exact feedback snapshot pair.

use crate::feedback_alloy::{
    compute_feedback_snapshot_digest, FEEDBACK_ALLOY_CHECKS, FEEDBACK_ALLOY_META_MODEL,
};
use crate::{
    map_feedback_verdict, ArtifactAuthorityV1, BoundedCheckOutcome, FeedbackAlloySnapshotPairV1,
    FeedbackAlloySnapshotV1, FeedbackCheckRoleV1, FeedbackVerdict,
    FEEDBACK_ALLOY_SERIALIZER_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

const FEEDBACK_ALLOY_COMMAND: &str = "FeedbackCompatibility";
pub const FEEDBACK_ALLOY_DISTRIBUTION_SHA256: &str =
    "6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d";
const FEEDBACK_ALLOY_VERSION_PREFIX: &str = "6.2.0";
const FEEDBACK_JAVA_MAJOR: u16 = 17;
const FEEDBACK_ALLOY_SOLVER: &str = "SAT4J";
const FEEDBACK_ALLOY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackAlloyAnalyzerConfig {
    pub java_executable: PathBuf,
    pub distribution_jar: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackAnalyzerReceiptV1 {
    pub command_name: String,
    pub alloy_version: String,
    pub java_version: String,
    pub solver: String,
    pub distribution_sha256: String,
    pub timeout_millis: u128,
    pub exit_code: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackBoundedCheckV1 {
    pub check_id: String,
    pub role: FeedbackCheckRoleV1,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    pub outcome: BoundedCheckOutcome,
    pub analyzer_receipt: FeedbackAnalyzerReceiptV1,
    pub result_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackBoundedEvaluationV1 {
    pub evaluation_id: String,
    pub authority: ArtifactAuthorityV1,
    pub feedback_assertion_id: String,
    pub baseline_fixture_file_sha256: String,
    pub baseline_head_commit: String,
    pub baseline_merged_commit: String,
    pub baseline_claim_set_digest: String,
    pub scope_id: String,
    pub scope_completeness: String,
    pub coverage_limitations: Vec<String>,
    pub accepted_claim_refs: Vec<String>,
    pub accepted_atom_refs: Vec<String>,
    pub accepted_relation_refs: Vec<String>,
    pub source_anchor_refs: Vec<String>,
    pub assertion_check: FeedbackBoundedCheckV1,
    pub negated_assertion_check: FeedbackBoundedCheckV1,
    pub verdict: FeedbackVerdict,
    pub result_digest: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FeedbackAlloyAnalysisError {
    #[error("feedback snapshot pair mismatch on {field}")]
    PairMismatch { field: &'static str },
    #[error("{role:?} snapshot digest mismatch: expected {expected}, observed {observed}")]
    SnapshotDigestMismatch {
        role: FeedbackCheckRoleV1,
        expected: String,
        observed: String,
    },
    #[error("feedback Alloy artifact {artifact} digest mismatch")]
    StableArtifactDigestMismatch { artifact: &'static str },
    #[error("feedback Alloy analyzer I/O failed: {0}")]
    Io(String),
    #[error("Alloy distribution digest mismatch: expected {expected}, observed {observed}")]
    DistributionDigestMismatch { expected: String, observed: String },
    #[error("Alloy version mismatch: expected prefix {expected}, observed {observed}")]
    AlloyVersionMismatch { expected: String, observed: String },
    #[error("Java version mismatch: expected major {expected}, observed {observed}")]
    JavaVersionMismatch { expected: u16, observed: String },
    #[error("{role:?} Alloy check timed out after {timeout_millis}ms")]
    TimedOut {
        role: FeedbackCheckRoleV1,
        timeout_millis: u128,
    },
    #[error("{role:?} Alloy check failed with exit code {exit_code}: {stderr}")]
    AnalyzerFailed {
        role: FeedbackCheckRoleV1,
        exit_code: i32,
        stderr: String,
    },
}

pub fn analyze_feedback_snapshot_pair(
    pair: &FeedbackAlloySnapshotPairV1,
    config: &FeedbackAlloyAnalyzerConfig,
) -> Result<FeedbackBoundedEvaluationV1, FeedbackAlloyAnalysisError> {
    validate_pair(pair)?;
    validate_snapshot(&pair.assertion)?;
    validate_snapshot(&pair.negated_assertion)?;

    let environment = validate_environment(config)?;
    let assertion_check = execute_check(&pair.assertion, config, &environment)?;
    let negated_assertion_check = execute_check(&pair.negated_assertion, config, &environment)?;
    let verdict = map_feedback_verdict(assertion_check.outcome, negated_assertion_check.outcome)
        .expect("successful Alloy runs always produce conclusive outcomes");

    let mut evaluation = FeedbackBoundedEvaluationV1 {
        evaluation_id: String::new(),
        authority: ArtifactAuthorityV1::BoundedEvidence,
        feedback_assertion_id: pair.assertion.feedback_assertion_id.clone(),
        baseline_fixture_file_sha256: pair.assertion.baseline_fixture_file_sha256.clone(),
        baseline_head_commit: pair.assertion.baseline_head_commit.clone(),
        baseline_merged_commit: pair.assertion.baseline_merged_commit.clone(),
        baseline_claim_set_digest: pair.assertion.baseline_claim_set_digest.clone(),
        scope_id: pair.assertion.scope_id.clone(),
        scope_completeness: pair.assertion.scope_completeness.clone(),
        coverage_limitations: pair.assertion.coverage_limitations.clone(),
        accepted_claim_refs: pair.assertion.accepted_claim_refs.clone(),
        accepted_atom_refs: pair.assertion.accepted_atom_refs.clone(),
        accepted_relation_refs: pair.assertion.accepted_relation_refs.clone(),
        source_anchor_refs: pair.assertion.source_anchor_refs.clone(),
        assertion_check,
        negated_assertion_check,
        verdict,
        result_digest: String::new(),
    };
    evaluation.result_digest = digest_without_derived_ids(&evaluation);
    evaluation.evaluation_id = format!(
        "SMR-FEEDBACK-EVALUATION-{}",
        evaluation.result_digest.trim_start_matches("sha256:")
    );
    Ok(evaluation)
}

fn validate_pair(pair: &FeedbackAlloySnapshotPairV1) -> Result<(), FeedbackAlloyAnalysisError> {
    for (field, matches) in [
        (
            "check_roles",
            pair.assertion.check_role == FeedbackCheckRoleV1::Assertion
                && pair.negated_assertion.check_role == FeedbackCheckRoleV1::NegatedAssertion,
        ),
        (
            "feedback_assertion_id",
            pair.assertion.feedback_assertion_id == pair.negated_assertion.feedback_assertion_id,
        ),
        (
            "baseline_fixture_file_sha256",
            pair.assertion.baseline_fixture_file_sha256
                == pair.negated_assertion.baseline_fixture_file_sha256,
        ),
        (
            "baseline_head_commit",
            pair.assertion.baseline_head_commit == pair.negated_assertion.baseline_head_commit,
        ),
        (
            "baseline_merged_commit",
            pair.assertion.baseline_merged_commit == pair.negated_assertion.baseline_merged_commit,
        ),
        (
            "baseline_claim_set_digest",
            pair.assertion.baseline_claim_set_digest
                == pair.negated_assertion.baseline_claim_set_digest,
        ),
        (
            "scope",
            pair.assertion.scope_id == pair.negated_assertion.scope_id
                && pair.assertion.scope_completeness == pair.negated_assertion.scope_completeness
                && pair.assertion.coverage_limitations
                    == pair.negated_assertion.coverage_limitations,
        ),
        (
            "artifact_pins",
            pair.assertion.authority == ArtifactAuthorityV1::NonCanonicalProjection
                && pair.negated_assertion.authority == ArtifactAuthorityV1::NonCanonicalProjection
                && pair.assertion.serializer_version == FEEDBACK_ALLOY_SERIALIZER_VERSION
                && pair.negated_assertion.serializer_version == FEEDBACK_ALLOY_SERIALIZER_VERSION
                && pair.assertion.meta_model_digest == pair.negated_assertion.meta_model_digest
                && pair.assertion.checks_digest == pair.negated_assertion.checks_digest,
        ),
        (
            "proposition_atom_id",
            pair.assertion.proposition_atom_id == pair.negated_assertion.proposition_atom_id,
        ),
        (
            "proposition_values",
            pair.assertion.proposition_value != pair.negated_assertion.proposition_value,
        ),
        (
            "witness_refs",
            pair.assertion.accepted_claim_refs == pair.negated_assertion.accepted_claim_refs
                && pair.assertion.accepted_atom_refs == pair.negated_assertion.accepted_atom_refs
                && pair.assertion.accepted_relation_refs
                    == pair.negated_assertion.accepted_relation_refs
                && pair.assertion.source_anchor_refs == pair.negated_assertion.source_anchor_refs,
        ),
        (
            "exact_cardinalities",
            pair.assertion.exact_cardinalities == pair.negated_assertion.exact_cardinalities,
        ),
    ] {
        if !matches {
            return Err(FeedbackAlloyAnalysisError::PairMismatch { field });
        }
    }
    Ok(())
}

fn validate_snapshot(snapshot: &FeedbackAlloySnapshotV1) -> Result<(), FeedbackAlloyAnalysisError> {
    let observed = compute_feedback_snapshot_digest(snapshot);
    if observed != snapshot.result_digest {
        return Err(FeedbackAlloyAnalysisError::SnapshotDigestMismatch {
            role: snapshot.check_role,
            expected: snapshot.result_digest.clone(),
            observed,
        });
    }
    for (artifact, expected, contents) in [
        (
            "smr_feedback_meta_model.als",
            snapshot.meta_model_digest.as_str(),
            FEEDBACK_ALLOY_META_MODEL,
        ),
        (
            "smr_feedback_checks.als",
            snapshot.checks_digest.as_str(),
            FEEDBACK_ALLOY_CHECKS,
        ),
    ] {
        if sha256_prefixed(contents.as_bytes()) != expected {
            return Err(FeedbackAlloyAnalysisError::StableArtifactDigestMismatch { artifact });
        }
    }
    Ok(())
}

struct ValidatedEnvironment {
    distribution_sha256: String,
    alloy_version: String,
    java_version: String,
}

fn validate_environment(
    config: &FeedbackAlloyAnalyzerConfig,
) -> Result<ValidatedEnvironment, FeedbackAlloyAnalysisError> {
    let distribution_sha256 =
        sha256_unprefixed(&fs::read(&config.distribution_jar).map_err(io_error)?);
    if distribution_sha256 != FEEDBACK_ALLOY_DISTRIBUTION_SHA256 {
        return Err(FeedbackAlloyAnalysisError::DistributionDigestMismatch {
            expected: FEEDBACK_ALLOY_DISTRIBUTION_SHA256.into(),
            observed: distribution_sha256,
        });
    }

    let java_output = isolated_analyzer_command(&config.java_executable)
        .arg("-version")
        .output()
        .map_err(io_error)?;
    if !java_output.status.success() {
        return Err(FeedbackAlloyAnalysisError::AnalyzerFailed {
            role: FeedbackCheckRoleV1::Assertion,
            exit_code: java_output.status.code().unwrap_or(-1),
            stderr: sanitized(&java_output.stderr),
        });
    }
    let java_version = first_nonempty_line(&java_output.stderr, &java_output.stdout);
    if java_major(&java_version) != Some(FEEDBACK_JAVA_MAJOR) {
        return Err(FeedbackAlloyAnalysisError::JavaVersionMismatch {
            expected: FEEDBACK_JAVA_MAJOR,
            observed: java_version,
        });
    }

    let alloy_output = isolated_analyzer_command(&config.java_executable)
        .args(["-Xmx512m", "-jar"])
        .arg(&config.distribution_jar)
        .args(["version", "--full"])
        .output()
        .map_err(io_error)?;
    if !alloy_output.status.success() {
        return Err(FeedbackAlloyAnalysisError::AnalyzerFailed {
            role: FeedbackCheckRoleV1::Assertion,
            exit_code: alloy_output.status.code().unwrap_or(-1),
            stderr: sanitized(&alloy_output.stderr),
        });
    }
    let alloy_version = String::from_utf8_lossy(&alloy_output.stdout)
        .trim()
        .to_string();
    if !alloy_version.starts_with(FEEDBACK_ALLOY_VERSION_PREFIX) {
        return Err(FeedbackAlloyAnalysisError::AlloyVersionMismatch {
            expected: FEEDBACK_ALLOY_VERSION_PREFIX.into(),
            observed: alloy_version,
        });
    }
    Ok(ValidatedEnvironment {
        distribution_sha256: FEEDBACK_ALLOY_DISTRIBUTION_SHA256.into(),
        alloy_version,
        java_version,
    })
}

fn execute_check(
    snapshot: &FeedbackAlloySnapshotV1,
    config: &FeedbackAlloyAnalyzerConfig,
    environment: &ValidatedEnvironment,
) -> Result<FeedbackBoundedCheckV1, FeedbackAlloyAnalysisError> {
    let directory = tempfile::Builder::new()
        .prefix("bicameral-smr-feedback-alloy-")
        .tempdir()
        .map_err(io_error)?;
    let module_path = directory.path().join("snapshot.als");
    let stdout_path = directory.path().join("stdout.json");
    let stderr_path = directory.path().join("stderr.log");
    fs::write(&module_path, &snapshot.alloy_module).map_err(io_error)?;
    fs::write(
        directory.path().join("smr_feedback_meta_model.als"),
        FEEDBACK_ALLOY_META_MODEL,
    )
    .map_err(io_error)?;
    fs::write(
        directory.path().join("smr_feedback_checks.als"),
        FEEDBACK_ALLOY_CHECKS,
    )
    .map_err(io_error)?;
    let stdout_file = File::create(&stdout_path).map_err(io_error)?;
    let stderr_file = File::create(&stderr_path).map_err(io_error)?;
    let mut child = isolated_analyzer_command(&config.java_executable)
        .arg("-Xmx512m")
        .arg(format!("-Djava.io.tmpdir={}", directory.path().display()))
        .arg(format!("-Duser.home={}", directory.path().display()))
        .arg("-jar")
        .arg(&config.distribution_jar)
        .args([
            "exec",
            "-q",
            "-c",
            FEEDBACK_ALLOY_COMMAND,
            "-t",
            "json",
            "-o",
            "-",
            "-s",
        ])
        .arg(FEEDBACK_ALLOY_SOLVER)
        .arg(&module_path)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(io_error)?;

    let started = Instant::now();
    let exit_status = loop {
        if let Some(status) = child.try_wait().map_err(io_error)? {
            break status;
        }
        if started.elapsed() >= FEEDBACK_ALLOY_TIMEOUT {
            child.kill().map_err(io_error)?;
            child.wait().map_err(io_error)?;
            return Err(FeedbackAlloyAnalysisError::TimedOut {
                role: snapshot.check_role,
                timeout_millis: FEEDBACK_ALLOY_TIMEOUT.as_millis(),
            });
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = fs::read(stdout_path).map_err(io_error)?;
    let stderr = fs::read(stderr_path).map_err(io_error)?;
    if !exit_status.success() {
        return Err(FeedbackAlloyAnalysisError::AnalyzerFailed {
            role: snapshot.check_role,
            exit_code: exit_status.code().unwrap_or(-1),
            stderr: sanitized(&stderr),
        });
    }

    let outcome = if stdout.iter().any(|byte| !byte.is_ascii_whitespace()) {
        BoundedCheckOutcome::Possible
    } else {
        BoundedCheckOutcome::Impossible
    };
    let receipt = FeedbackAnalyzerReceiptV1 {
        command_name: FEEDBACK_ALLOY_COMMAND.into(),
        alloy_version: environment.alloy_version.clone(),
        java_version: environment.java_version.clone(),
        solver: FEEDBACK_ALLOY_SOLVER.into(),
        distribution_sha256: environment.distribution_sha256.clone(),
        timeout_millis: FEEDBACK_ALLOY_TIMEOUT.as_millis(),
        exit_code: exit_status.code().unwrap_or(0),
        stdout_sha256: sha256_unprefixed(&stdout),
        stderr_sha256: sha256_unprefixed(&stderr),
    };
    let result_digest = sha256_prefixed(
        &serde_json::to_vec(&(
            snapshot.result_digest.as_str(),
            snapshot.check_role,
            outcome,
            &receipt,
        ))
        .expect("feedback check serialization is infallible"),
    );
    Ok(FeedbackBoundedCheckV1 {
        check_id: format!(
            "SMR-FEEDBACK-CHECK-{}",
            result_digest.trim_start_matches("sha256:")
        ),
        role: snapshot.check_role,
        snapshot_id: snapshot.snapshot_id.clone(),
        snapshot_digest: snapshot.result_digest.clone(),
        outcome,
        analyzer_receipt: receipt,
        result_digest,
    })
}

fn digest_without_derived_ids(evaluation: &FeedbackBoundedEvaluationV1) -> String {
    let mut input = evaluation.clone();
    input.evaluation_id.clear();
    input.result_digest.clear();
    sha256_prefixed(
        &serde_json::to_vec(&input).expect("feedback evaluation serialization is infallible"),
    )
}

fn java_major(version: &str) -> Option<u16> {
    version.split('"').nth(1)?.split('.').next()?.parse().ok()
}

fn first_nonempty_line(primary: &[u8], fallback: &[u8]) -> String {
    [primary, fallback]
        .into_iter()
        .flat_map(|bytes| {
            String::from_utf8_lossy(bytes)
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
}

fn sha256_unprefixed(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_unprefixed(bytes))
}

fn sanitized(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(2_000).collect()
}

fn isolated_analyzer_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env_clear().env("LANG", "C").env("LC_ALL", "C");
    command
}

fn io_error(error: std::io::Error) -> FeedbackAlloyAnalysisError {
    FeedbackAlloyAnalysisError::Io(error.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn analyzer_process_receives_only_the_fixed_locale_environment() {
        let output = isolated_analyzer_command(Path::new("/usr/bin/env"))
            .output()
            .expect("env command runs");
        assert!(output.status.success());
        let environment = String::from_utf8(output.stdout).expect("environment is UTF-8");
        assert!(environment.lines().any(|line| line == "LANG=C"));
        assert!(environment.lines().any(|line| line == "LC_ALL=C"));
        assert!(!environment.lines().any(|line| {
            line.starts_with("HOME=")
                || line.starts_with("PATH=")
                || line.starts_with("BICAMERAL_")
                || line.starts_with("GOOGLE_")
                || line.starts_with("SLACK_")
        }));
    }
}
