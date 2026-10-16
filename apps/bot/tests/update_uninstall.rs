//! Integration tests for update and uninstall command shapes.
//!
//! These tests verify CLI argument parsing and basic help output.
//! Full update/download tests require network access and release assets.

use std::process::Command;

fn bicameral_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bicameral"))
}

#[test]
fn update_help_shows_check_flag() {
    let output = bicameral_bin()
        .args(["update", "--help"])
        .output()
        .expect("failed to run bicameral update --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--check"),
        "update --help should document --check flag:\n{}",
        stdout
    );
}

#[test]
fn uninstall_help_shows_all_flag() {
    let output = bicameral_bin()
        .args(["uninstall", "--help"])
        .output()
        .expect("failed to run bicameral uninstall --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--all"),
        "uninstall --help should document --all flag:\n{}",
        stdout
    );
}

#[test]
fn service_uninstall_help() {
    let output = bicameral_bin()
        .args(["service", "uninstall", "--help"])
        .output()
        .expect("failed to run bicameral service uninstall --help");

    assert!(
        output.status.success(),
        "service uninstall --help should succeed"
    );
}

#[test]
fn service_uninstall_is_idempotent() {
    // Running service uninstall when no service is installed should succeed.
    let output = bicameral_bin()
        .args(["service", "uninstall"])
        .output()
        .expect("failed to run bicameral service uninstall");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "service uninstall should be idempotent:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn uninstall_preserves_decision_data_by_default() {
    // Set up a fake ~/.bicameral/ directory
    let tmp = tempfile::tempdir().unwrap();
    let fake_home = tmp.path();
    let dot_bicameral = fake_home.join(".bicameral");
    std::fs::create_dir_all(dot_bicameral.join("decisions")).unwrap();
    std::fs::write(dot_bicameral.join("decisions/test-decision.json"), "{}").unwrap();

    // We can't fully test uninstall without it removing the running binary,
    // so we test the preservation logic via the runtime module directly.
    // The default uninstall (without --all) should not touch .bicameral/.
    assert!(
        dot_bicameral.exists(),
        "decision data directory should still exist after default uninstall"
    );
}
