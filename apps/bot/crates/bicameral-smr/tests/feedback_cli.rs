use std::{fs, process::Command};

fn unavailable_run() -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_smr-feedback-experiment"))
        .args([
            "--input",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/feedback/c04-request.json"
            ),
            "--java-executable",
            "/not-used",
            "--alloy-jar",
            "/not-used",
        ])
        .output()
        .expect("experiment CLI runs")
}

#[test]
fn missing_analyzer_emits_content_safe_typed_unavailable_receipt() {
    let output = unavailable_run();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, unavailable_run().stdout);
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        receipt["contract_id"],
        "bicameral.smr-feedback.experiment-cli.v1"
    );
    assert_eq!(receipt["authority"], "non_authoritative_experiment");
    assert_eq!(receipt["baseline"]["product_authority"], false);
    assert_eq!(receipt["attempt"]["status"], "unavailable");
    assert_eq!(
        receipt["attempt"]["unevaluated"]["reason"],
        "analyzer_unavailable"
    );
    let rendered = String::from_utf8(output.stdout).unwrap();
    assert!(!rendered.contains("/not-used"));
}

#[test]
fn malformed_input_stops_before_semantic_execution() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("malformed.json");
    fs::write(&input, br#"{"unexpected":true}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_smr-feedback-experiment"))
        .args(["--input", input.to_str().unwrap()])
        .output()
        .expect("experiment CLI runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("malformed input JSON"));
}

#[test]
fn external_board_requires_and_preserves_the_exact_fixture_pin() {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../protocol/smr-feedback-v0.1/fixtures/"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_smr-feedback-experiment"))
        .args([
            "--input",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/feedback/c04-request.json"
            ),
            "--board",
            &format!("{root}product-creation-workspace-routing.governed-domain.fixture.json"),
            "--fixture-pin",
            &format!("{root}product-creation-workspace-routing.fixture-pin.json"),
            "--java-executable",
            "/not-used",
            "--alloy-jar",
            "/not-used",
        ])
        .output()
        .expect("experiment CLI runs");
    assert_eq!(output.status.code(), Some(2));
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        receipt["baseline"]["fixture_file_sha256"],
        "bad78bae59137eb60c91f9863d5954f7ac677edcd31e97fec0630a0870e64a64"
    );
    assert_eq!(receipt["attempt"]["status"], "unavailable");
}

#[test]
fn non_regular_board_is_rejected_before_semantic_execution() {
    let directory = tempfile::tempdir().unwrap();
    let pin = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../protocol/smr-feedback-v0.1/fixtures/product-creation-workspace-routing.fixture-pin.json"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_smr-feedback-experiment"))
        .args([
            "--input",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/feedback/c04-request.json"
            ),
            "--board",
            directory.path().to_str().unwrap(),
            "--fixture-pin",
            pin,
        ])
        .output()
        .expect("experiment CLI runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("board must be a regular file"));
}
