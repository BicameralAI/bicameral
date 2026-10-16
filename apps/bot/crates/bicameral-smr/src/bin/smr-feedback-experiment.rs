use std::{
    env,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

use bicameral_smr::{
    evaluate_feedback_proposition, load_bundled_reviewed_experiment_board, load_experiment_board,
    ExperimentBoardPinV1, FeedbackAlloyAnalyzerConfig, FeedbackAssertionWitnessRequestV1,
    FeedbackBooleanPropositionV1, FeedbackEvaluationAttemptV1,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const INPUT_MAX_BYTES: usize = 32_768;
const BOARD_MAX_BYTES: usize = 1_048_576;
const PIN_MAX_BYTES: usize = 16_384;
const CONTRACT_ID: &str = "bicameral.smr-feedback.experiment-cli.v1";

#[derive(Parser)]
#[command(name = "smr-feedback-experiment")]
#[command(about = "Run one non-authoritative structured SMR feedback evaluation")]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, requires = "fixture_pin")]
    board: Option<PathBuf>,
    #[arg(long, requires = "board")]
    fixture_pin: Option<PathBuf>,
    #[arg(long)]
    java_executable: Option<PathBuf>,
    #[arg(long)]
    alloy_jar: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputV1 {
    assertion_id: String,
    accepted_claim_refs: Vec<String>,
    accepted_atom_refs: Vec<String>,
    #[serde(default)]
    accepted_relation_refs: Vec<String>,
    source_anchor_refs: Vec<String>,
    accepted_assertion_ref: String,
    asserted_value: bool,
}

#[derive(Serialize)]
struct BaselineReceipt<'a> {
    fixture_file_sha256: &'a str,
    fixture_head_commit: &'a str,
    merged_commit: &'a str,
    claim_set_digest: &'a str,
    scope_id: &'a str,
    scope_completeness: &'a str,
    coverage_limitations: &'a [String],
    product_authority: bool,
}

#[derive(Serialize)]
struct ReceiptPayload<'a> {
    contract_id: &'static str,
    authority: &'static str,
    input_digest: String,
    baseline: BaselineReceipt<'a>,
    attempt: &'a FeedbackEvaluationAttemptV1,
}

#[derive(Serialize)]
struct Receipt<'a> {
    #[serde(flatten)]
    payload: ReceiptPayload<'a>,
    result_digest: String,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(error) => {
            eprintln!("smr-feedback-experiment: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Args) -> Result<bool, String> {
    let input_bytes = read_bounded_regular_file(&args.input, INPUT_MAX_BYTES, "input")?;
    let input: InputV1 = serde_json::from_slice(&input_bytes)
        .map_err(|error| format!("malformed input JSON: {error}"))?;
    let board = match (&args.board, &args.fixture_pin) {
        (Some(board_path), Some(pin_path)) => {
            let board_bytes = read_bounded_regular_file(board_path, BOARD_MAX_BYTES, "board")?;
            let pin_bytes = read_bounded_regular_file(pin_path, PIN_MAX_BYTES, "fixture pin")?;
            let pin: ExperimentBoardPinV1 = serde_json::from_slice(&pin_bytes)
                .map_err(|error| format!("malformed fixture pin JSON: {error}"))?;
            load_experiment_board(&board_bytes, &pin)
        }
        (None, None) => load_bundled_reviewed_experiment_board(),
        _ => return Err("--board and --fixture-pin must be supplied together".into()),
    }
    .map_err(|error| format!("reviewed experiment board unavailable: {error}"))?;
    let analyzer = FeedbackAlloyAnalyzerConfig {
        java_executable: configured_path(
            args.java_executable,
            "BICAMERAL_JAVA17_EXECUTABLE",
            "java",
        ),
        distribution_jar: configured_path(
            args.alloy_jar,
            "BICAMERAL_ALLOY_DISTRIBUTION_JAR",
            "__bicameral_alloy_6_2_0_not_configured__",
        ),
    };
    let request = FeedbackAssertionWitnessRequestV1 {
        assertion_id: input.assertion_id,
        accepted_claim_refs: input.accepted_claim_refs,
        accepted_atom_refs: input.accepted_atom_refs,
        accepted_relation_refs: input.accepted_relation_refs,
        source_anchor_refs: input.source_anchor_refs,
    };
    let proposition = FeedbackBooleanPropositionV1 {
        accepted_assertion_ref: input.accepted_assertion_ref,
        asserted_value: input.asserted_value,
    };
    let attempt = evaluate_feedback_proposition(&board, request, proposition, &analyzer);
    let bounded_result = matches!(attempt, FeedbackEvaluationAttemptV1::BoundedResult { .. });
    let payload = ReceiptPayload {
        contract_id: CONTRACT_ID,
        authority: "non_authoritative_experiment",
        input_digest: sha256(&input_bytes),
        baseline: BaselineReceipt {
            fixture_file_sha256: board.fixture_file_sha256(),
            fixture_head_commit: board.fixture_head_commit(),
            merged_commit: board.merged_commit(),
            claim_set_digest: board.claim_set_digest(),
            scope_id: board.scope_id(),
            scope_completeness: board.scope_completeness(),
            coverage_limitations: board.scope_exclusions(),
            product_authority: board.has_product_authority(),
        },
        attempt: &attempt,
    };
    let result_digest = sha256(
        &serde_json::to_vec(&payload).map_err(|error| format!("cannot encode receipt: {error}"))?,
    );
    let receipt = Receipt {
        payload,
        result_digest,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("cannot encode receipt: {error}"))?
    );
    Ok(bounded_result)
}

fn read_bounded_regular_file(
    path: &Path,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("cannot read {label} file"))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{label} must be a regular file"));
    }
    if metadata.len() > max_bytes as u64 {
        return Err(format!("{label} exceeds {max_bytes} bytes"));
    }
    let file = File::open(path).map_err(|_| format!("cannot read {label} file"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("cannot read {label} file"))?;
    if bytes.len() > max_bytes {
        return Err(format!("{label} exceeds {max_bytes} bytes"));
    }
    Ok(bytes)
}

fn configured_path(flag: Option<PathBuf>, variable: &str, fallback: &str) -> PathBuf {
    flag.or_else(|| env::var_os(variable).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(fallback))
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
