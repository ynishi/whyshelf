//! The answers of the command line, on the binary and a small project of files:
//! every answer is JSON with its version, a wrong argument included; and `check`
//! exits 1 when it finds something wrong.

mod common;

use std::fs;
use std::process::Command;

use common::{Project, READS, json};

#[test]
fn every_answer_is_json_with_its_version() {
    let project = Project::new("json");
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    for args in READS {
        let output = project.run(args);
        assert_eq!(json(&output)["version"], 1, "{args:?}");
    }
}

#[test]
fn a_wrong_argument_is_the_error_json_of_a_request() {
    let project = Project::new("args");
    for args in [&["bogus"][..], &["get"], &["stale", "--nope"], &[]] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let answer = json(&output);
        assert_eq!(answer["error"], "request", "{args:?}");
        assert_eq!(answer["version"], 1, "{args:?}");
        assert!(
            !answer["messages"][0].as_str().unwrap().is_empty(),
            "{args:?}"
        );
        assert!(
            !output.stderr.is_empty(),
            "the text goes to stderr: {args:?}"
        );
    }
    for args in [&["--help"][..], &["-h"], &["--version"], &["-V"]] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(!output.stdout.is_empty(), "text on stdout: {args:?}");
        assert!(output.stderr.is_empty(), "nothing on stderr: {args:?}");
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_err());
    }
    // There is no `help` command: it is a command that does not exist.
    let output = project.run(&["help"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["error"], "request");
    // A value missing for `--root` names it, and no hint is left in the message.
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .output()
        .unwrap();
    let message = json(&output)["messages"][0].as_str().unwrap().to_owned();
    assert!(message.contains("--root"), "{message}");
    assert!(!message.contains("For more information"), "{message}");
}

#[test]
fn no_argument_at_all_is_the_error_json_of_a_request() {
    let project = Project::new("bare");
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .current_dir(&project.0)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert_eq!(answer["messages"][0], "a command is needed (see --help)");
    assert!(!output.stderr.is_empty(), "the help goes to stderr");
}

#[test]
fn check_exits_1_when_it_finds_something_wrong() {
    let project = Project::new("check");
    assert_eq!(project.run(&["check"]).status.code(), Some(0));
    fs::write(
        project.0.join("notes/N-2.md"),
        "---\nkey: \"N-2\"\n---\nno id\n",
    )
    .unwrap();
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["clean"], false);
}
