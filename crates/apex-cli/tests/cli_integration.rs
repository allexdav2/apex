use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn cli_help() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Autonomous Path EXploration"));
}

#[test]
fn cli_no_args_shows_usage() {
    Command::cargo_bin("apex")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn cli_doctor_runs() {
    // doctor exits 0 when all required tools are present, 1 when some are missing.
    // We only assert it produces output (not a crash/panic).
    Command::cargo_bin("apex")
        .unwrap()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("APEX prerequisite check"));
}

#[test]
fn cli_run_missing_target() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["run", "--lang", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--target"));
}

#[test]
fn cli_ratchet_missing_args() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["ratchet"])
        .assert()
        .failure();
}

#[test]
fn cli_unknown_subcommand() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("nonexistent")
        .assert()
        .failure();
}

#[test]
fn cli_audit_missing_target() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["audit", "--lang", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--target"));
}

#[test]
fn cli_index_missing_args() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["index"])
        .assert()
        .failure();
}

#[test]
fn cli_run_help() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--target"));
}

#[test]
fn cli_deploy_score_help() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["deploy-score", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--target"));
}
