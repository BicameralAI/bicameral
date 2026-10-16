use crate::alloy::{compute_snapshot_digest, ALLOY_CHECKS, ALLOY_META_MODEL};
use crate::{
    AlloyCommand, AlloyCommandKind, ArtifactAuthorityV1, CandidatePromotionAlloySnapshotV1,
    SourceSpan,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlloyAnalyzerConfig {
    pub command: AlloyCommand,
    pub executable: PathBuf,
    pub distribution_jar: PathBuf,
    pub expected_distribution_sha256: String,
    pub expected_version_prefix: String,
    pub solver: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlloyAnalysisStatus {
    InstanceFound,
    NoInstanceWithinScope,
    CounterexampleFound,
    NoCounterexampleWithinScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlloyAnalyzerReceiptV1 {
    pub command_name: String,
    pub command_kind: AlloyCommandKind,
    pub version: String,
    pub solver: String,
    pub distribution_sha256: String,
    pub timeout_millis: u128,
    pub exit_code: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlloyFindingV1 {
    pub finding_id: String,
    pub command: AlloyCommand,
    pub semantic_ids: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlloyAnalysisResultV1 {
    pub analysis_id: String,
    pub authority: ArtifactAuthorityV1,
    pub snapshot_digest: String,
    pub command: AlloyCommand,
    pub status: AlloyAnalysisStatus,
    pub findings: Vec<AlloyFindingV1>,
    pub analyzer_receipt: AlloyAnalyzerReceiptV1,
    pub result_digest: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AlloyAnalysisError {
    #[error("Alloy analyzer I/O failed: {0}")]
    Io(String),
    #[error("Alloy snapshot digest mismatch: expected {expected}, observed {observed}")]
    SnapshotDigestMismatch { expected: String, observed: String },
    #[error(
        "stable Alloy artifact {artifact} digest mismatch: expected {expected}, observed {observed}"
    )]
    StableArtifactDigestMismatch {
        artifact: &'static str,
        expected: String,
        observed: String,
    },
    #[error("Alloy distribution digest mismatch: expected {expected}, observed {observed}")]
    DistributionDigestMismatch { expected: String, observed: String },
    #[error("Alloy version mismatch: expected prefix {expected}, observed {observed}")]
    VersionMismatch { expected: String, observed: String },
    #[error("Alloy analysis timed out after {timeout_millis}ms")]
    TimedOut { timeout_millis: u128 },
    #[error("Alloy analyzer failed with exit code {exit_code}: {stderr}")]
    AnalyzerFailed { exit_code: i32, stderr: String },
}

pub fn analyze_alloy_snapshot(
    snapshot: &CandidatePromotionAlloySnapshotV1,
    config: &AlloyAnalyzerConfig,
) -> Result<AlloyAnalysisResultV1, AlloyAnalysisError> {
    let observed_snapshot_digest = compute_snapshot_digest(snapshot);
    if observed_snapshot_digest != snapshot.result_digest {
        return Err(AlloyAnalysisError::SnapshotDigestMismatch {
            expected: snapshot.result_digest.clone(),
            observed: observed_snapshot_digest,
        });
    }
    verify_stable_artifact_digest(
        "smr_meta_model.als",
        &snapshot.meta_model_digest,
        ALLOY_META_MODEL,
    )?;
    verify_stable_artifact_digest("smr_checks.als", &snapshot.checks_digest, ALLOY_CHECKS)?;

    let distribution_sha256 = sha256_file(&config.distribution_jar)?;
    if distribution_sha256 != config.expected_distribution_sha256 {
        return Err(AlloyAnalysisError::DistributionDigestMismatch {
            expected: config.expected_distribution_sha256.clone(),
            observed: distribution_sha256,
        });
    }

    let version_output = Command::new(&config.executable)
        .args(["version", "--full"])
        .output()
        .map_err(io_error)?;
    if !version_output.status.success() {
        return Err(AlloyAnalysisError::AnalyzerFailed {
            exit_code: version_output.status.code().unwrap_or(-1),
            stderr: sanitized_stderr(&version_output.stderr),
        });
    }
    let version = String::from_utf8_lossy(&version_output.stdout)
        .trim()
        .to_string();
    if !version.starts_with(&config.expected_version_prefix) {
        return Err(AlloyAnalysisError::VersionMismatch {
            expected: config.expected_version_prefix.clone(),
            observed: version,
        });
    }

    let run_directory = tempfile::Builder::new()
        .prefix("bicameral-smr-alloy-")
        .tempdir()
        .map_err(io_error)?;
    let module_path = run_directory.path().join("snapshot.als");
    let meta_model_path = run_directory.path().join("smr_meta_model.als");
    let checks_path = run_directory.path().join("smr_checks.als");
    let stdout_path = run_directory.path().join("stdout.json");
    let stderr_path = run_directory.path().join("stderr.log");
    fs::write(&module_path, &snapshot.alloy_module).map_err(io_error)?;
    fs::write(&meta_model_path, ALLOY_META_MODEL).map_err(io_error)?;
    fs::write(&checks_path, ALLOY_CHECKS).map_err(io_error)?;
    let stdout_file = File::create(&stdout_path).map_err(io_error)?;
    let stderr_file = File::create(&stderr_path).map_err(io_error)?;

    let mut child = Command::new(&config.executable)
        .arg("exec")
        .args([
            "-q",
            "-c",
            config.command.alloy_name(),
            "-t",
            "json",
            "-o",
            "-",
            "-s",
        ])
        .arg(&config.solver)
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
        if started.elapsed() >= config.timeout {
            child.kill().map_err(io_error)?;
            child.wait().map_err(io_error)?;
            return Err(AlloyAnalysisError::TimedOut {
                timeout_millis: config.timeout.as_millis(),
            });
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = fs::read(&stdout_path).map_err(io_error)?;
    let stderr = fs::read(&stderr_path).map_err(io_error)?;
    if !exit_status.success() {
        return Err(AlloyAnalysisError::AnalyzerFailed {
            exit_code: exit_status.code().unwrap_or(-1),
            stderr: sanitized_stderr(&stderr),
        });
    }

    let has_solution = stdout.iter().any(|byte| !byte.is_ascii_whitespace());
    let status = match (config.command.kind(), has_solution) {
        (AlloyCommandKind::Run, true) => AlloyAnalysisStatus::InstanceFound,
        (AlloyCommandKind::Run, false) => AlloyAnalysisStatus::NoInstanceWithinScope,
        (AlloyCommandKind::Check, true) => AlloyAnalysisStatus::CounterexampleFound,
        (AlloyCommandKind::Check, false) => AlloyAnalysisStatus::NoCounterexampleWithinScope,
    };
    let findings = if status == AlloyAnalysisStatus::CounterexampleFound {
        snapshot
            .check_evidence_map
            .iter()
            .filter(|entry| entry.command == config.command)
            .map(|entry| AlloyFindingV1 {
                finding_id: finding_id(config.command, &entry.semantic_ids),
                command: config.command,
                semantic_ids: entry.semantic_ids.clone(),
                source_spans: entry.source_spans.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let analyzer_receipt = AlloyAnalyzerReceiptV1 {
        command_name: config.command.alloy_name().into(),
        command_kind: config.command.kind(),
        version,
        solver: config.solver.clone(),
        distribution_sha256: config.expected_distribution_sha256.clone(),
        timeout_millis: config.timeout.as_millis(),
        exit_code: exit_status.code().unwrap_or(0),
        stdout_sha256: sha256_bytes(&stdout),
        stderr_sha256: sha256_bytes(&stderr),
    };
    let result_digest = digest_result(
        snapshot,
        config.command,
        status,
        &findings,
        &analyzer_receipt,
    );
    let analysis_id = format!(
        "ALLOY-ANALYSIS-{}",
        result_digest.trim_start_matches("sha256:")
    );

    Ok(AlloyAnalysisResultV1 {
        analysis_id,
        authority: ArtifactAuthorityV1::BoundedEvidence,
        snapshot_digest: snapshot.result_digest.clone(),
        command: config.command,
        status,
        findings,
        analyzer_receipt,
        result_digest,
    })
}

fn digest_result(
    snapshot: &CandidatePromotionAlloySnapshotV1,
    command: AlloyCommand,
    status: AlloyAnalysisStatus,
    findings: &[AlloyFindingV1],
    receipt: &AlloyAnalyzerReceiptV1,
) -> String {
    let bytes = serde_json::to_vec(&(
        snapshot.result_digest.as_str(),
        command,
        status,
        findings,
        receipt,
    ))
    .expect("analysis result serialization is infallible");
    sha256_bytes(&bytes)
}

fn finding_id(command: AlloyCommand, semantic_ids: &[String]) -> String {
    let bytes = serde_json::to_vec(&(command, semantic_ids))
        .expect("finding identity serialization is infallible");
    format!("ALLOY-FINDING-{}", sha256_bytes(&bytes))
}

fn sha256_file(path: &std::path::Path) -> Result<String, AlloyAnalysisError> {
    let bytes = fs::read(path).map_err(io_error)?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn verify_stable_artifact_digest(
    artifact: &'static str,
    expected: &str,
    contents: &str,
) -> Result<(), AlloyAnalysisError> {
    let observed = format!("sha256:{}", sha256_bytes(contents.as_bytes()));
    if observed == expected {
        Ok(())
    } else {
        Err(AlloyAnalysisError::StableArtifactDigestMismatch {
            artifact,
            expected: expected.into(),
            observed,
        })
    }
}

fn sanitized_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(2_000).collect()
}

fn io_error(error: std::io::Error) -> AlloyAnalysisError {
    AlloyAnalysisError::Io(error.to_string())
}
