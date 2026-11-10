//! The repository's own ledger, read by the binary as it is: every record it holds,
//! written by every version so far (before comparisons, before spaces), reads back
//! as a record, and none contradicts another. Read-only: `check` and `status` only.

use std::path::PathBuf;
use std::process::Command;

/// The root of the repository: two levels above this crate.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(root())
        .args(args)
        .output()
        .expect("the binary runs");
    serde_json::from_slice(&output.stdout).expect("the answer is JSON")
}

fn assert_ledger_reads(answer: &serde_json::Value, health: &serde_json::Value) {
    assert_eq!(
        health["unreadable_records"],
        serde_json::json!([]),
        "{answer}"
    );
    assert_eq!(health["ledger_problems"], serde_json::json!([]), "{answer}");
}

#[test]
fn the_repository_s_ledger_reads_with_no_unreadable_record_and_no_problem() {
    let check = run(&["check"]);
    assert_eq!(check["schema_problems"], serde_json::json!([]), "{check}");
    assert_ledger_reads(&check, &check["health"]);

    let status = run(&["status"]);
    assert_ledger_reads(&status, &status["health"]);
    assert!(
        status["docs"].as_array().is_some_and(|d| !d.is_empty()),
        "{status}"
    );
}
