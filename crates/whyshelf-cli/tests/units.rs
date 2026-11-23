//! `units`, on the binary and one small project of files: two chains that share
//! nothing are two units, each upstream first, pending or stale; two chains whose
//! artifacts live in one file (a file, or two tests of one file) are one unit; and what
//! could join units unseen (a document divided whose links cannot be read, a file whose
//! identity cannot be read) is counted in the answer and said on stderr.
//!
//! The project is written once per test from one fixture, with no program but the
//! binary run.

mod common;

use std::fs;

use common::{Project, json};
use serde_json::{Value, json};

const BOOKS: &str = r#"[defaults]
artifact = "file"

[[book]]
name = "requirement"
dir = "books/requirement"
key = { prefix = "REQ" }
[[book.field]]
name = "title"
type = "line"
required = true

[[book]]
name = "decision"
dir = "books/decision"
key = { prefix = "DEC" }
[[book.field]]
name = "title"
type = "line"
required = true
[[book.link]]
name = "serves"
target = "requirement"
means = "depends"
[[book.link]]
name = "code"
target = "artifact"
preset = "code"

[[book]]
name = "verification"
dir = "books/verification"
key = { prefix = "V" }
[[book.field]]
name = "title"
type = "line"
required = true
[[book.link]]
name = "checks"
target = "decision"
means = "depends"
[[book.link]]
name = "tests"
target = "artifact"
preset = "test"
"#;

const TESTS: &str =
    "#[test]\nfn one() {\n    assert!(true);\n}\n\n#[test]\nfn two() {\n    assert!(true);\n}\n";

/// A document of `book` with identity `n`, its key, the title `title`, and the front
/// matter `extra`.
fn doc(project: &Project, book: &str, n: u8, key: &str, title: &str, extra: &str) {
    let text = format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000000{n:02}\"\nkey: \"{key}\"\ntitle: \"{title}\"\n{extra}---\nThe body.\n"
    );
    fs::write(project.0.join(format!("books/{book}/{key}.md")), text).unwrap();
}

/// The two chains: `REQ-1` ← `DEC-1` (code `src/a.rs`) ← `V-1` (the test
/// `tests/a.rs::one`), and `REQ-2` ← `DEC-2` (code `code`) ← `V-2` (the test `test`).
fn chains(project: &Project, code: &str, test: &str) {
    doc(project, "requirement", 1, "REQ-1", "REQ-1", "");
    doc(project, "requirement", 2, "REQ-2", "REQ-2", "");
    doc(
        project,
        "decision",
        3,
        "DEC-1",
        "DEC-1",
        "serves: [\"REQ-1\"]\ncode: [\"src/a.rs\"]\n",
    );
    doc(
        project,
        "decision",
        4,
        "DEC-2",
        "DEC-2",
        &format!("serves: [\"REQ-2\"]\ncode: [\"{code}\"]\n"),
    );
    doc(
        project,
        "verification",
        5,
        "V-1",
        "V-1",
        "checks: [\"DEC-1\"]\ntests: [\"tests/a.rs::one\"]\n",
    );
    doc(
        project,
        "verification",
        6,
        "V-2",
        "V-2",
        &format!("checks: [\"DEC-2\"]\ntests: [\"{test}\"]\n"),
    );
}

/// The project of the two chains, as [`chains`] writes them.
fn project(name: &str, code: &str, test: &str) -> Project {
    let project = Project::with(name, BOOKS, "");
    fs::remove_dir_all(project.0.join("notes")).unwrap();
    for dir in [
        "books/requirement",
        "books/decision",
        "books/verification",
        "src",
        "tests",
    ] {
        fs::create_dir_all(project.0.join(dir)).unwrap();
    }
    fs::write(project.0.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(project.0.join("src/b.rs"), "pub fn b() {}\n").unwrap();
    fs::write(project.0.join("tests/a.rs"), TESTS).unwrap();
    fs::write(project.0.join("tests/b.rs"), TESTS).unwrap();
    chains(&project, code, test);
    project
}

/// All six approved, then both requirements edited: every approval is stale.
fn approved_then_edited(project: &Project) {
    let approve = project.run(&["judge", "approve", "--pending", "--reason", "read"]);
    assert_eq!(approve.status.code(), Some(0), "{}", json(&approve));
    doc(project, "requirement", 1, "REQ-1", "REQ-1 edited", "");
    doc(project, "requirement", 2, "REQ-2", "REQ-2 edited", "");
}

/// The keys of each unit's documents, in their order.
fn keys(answer: &Value) -> Vec<Vec<String>> {
    answer["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| {
            unit["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["doc"]["key"].as_str().unwrap().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn two_chains_that_share_nothing_are_two_units_each_upstream_first() {
    let project = project("units", "src/b.rs", "tests/b.rs::two");
    let chain = |n: u8| {
        vec![
            format!("requirement:REQ-{n}"),
            format!("decision:DEC-{n}"),
            format!("verification:V-{n}"),
        ]
    };
    // Pending: nothing no longer holds, and with --pending the chains are divided.
    let output = project.run(&["units"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["units"], json!([]));
    let answer = json(&project.run(&["units", "--pending"]));
    assert_eq!(keys(&answer), [chain(1), chain(2)], "{answer}");
    assert_eq!(answer["units"][0]["documents"][0]["standing"], "pending");

    approved_then_edited(&project);
    let output = project.run(&["units"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "no warning");
    let answer = json(&output);
    assert!(answer.get("untold").is_none(), "{answer}");
    assert_eq!(keys(&answer), [chain(1), chain(2)], "{answer}");
    let first = &answer["units"][0];
    assert_eq!(first["documents"][1]["standing"], "stale");
    assert_eq!(first["files"], json!(["src/a.rs", "tests/a.rs"]));
    assert_eq!(
        first["artifacts"][1],
        json!({
            "space": "rust-test",
            "name": "tests/a.rs::one",
            "documents": [{"id": "01a1c4f0-0000-7000-8000-000000000005", "key": "verification:V-1"}],
            "place": {"place": "only", "files": ["tests/a.rs"]},
        })
    );
    assert_eq!(first["waits_on"], json!([]));
    assert_eq!(
        answer["units"][1]["files"],
        json!(["src/b.rs", "tests/b.rs"])
    );
    assert_eq!(answer["unplaced"], json!([]));
}

#[test]
fn two_chains_whose_artifacts_live_in_one_file_are_one_unit() {
    // DEC-2's code is DEC-1's file.
    let one = project("units-file", "src/a.rs", "tests/b.rs::two");
    approved_then_edited(&one);
    let answer = json(&one.run(&["units"]));
    assert_eq!(keys(&answer).len(), 1, "{answer}");
    assert_eq!(
        answer["units"][0]["files"],
        json!(["src/a.rs", "tests/a.rs", "tests/b.rs"])
    );

    // V-2's test is another test of V-1's file.
    let other = project("units-test", "src/b.rs", "tests/a.rs::two");
    approved_then_edited(&other);
    let answer = json(&other.run(&["units"]));
    assert_eq!(keys(&answer).len(), 1, "{answer}");
    let tests: Vec<&str> = answer["units"][0]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["space"] == "rust-test")
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(tests, ["tests/a.rs::one", "tests/a.rs::two"]);
}

#[test]
fn what_could_join_units_unseen_is_counted_and_said() {
    let project = project("units-untold", "src/b.rs", "tests/b.rs::two");
    let approve = project.run(&["judge", "approve", "--pending", "--reason", "read"]);
    assert_eq!(approve.status.code(), Some(0), "{}", json(&approve));
    // DEC-1, approved, loses its title (its links cannot be read now), and a file of
    // the requirements has a front matter that is no mapping (no identity is read).
    fs::write(
        project.0.join("books/decision/DEC-1.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000003\"\nkey: \"DEC-1\"\nserves: [\"REQ-1\"]\n---\nThe body.\n",
    )
    .unwrap();
    fs::write(
        project.0.join("books/requirement/REQ-9.md"),
        "---\n- a list\n---\nThe body.\n",
    )
    .unwrap();
    let output = project.run(&["units"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["untold"], json!({"invalid": 1, "unreadable": 1}));
    assert_eq!(
        keys_or_ids(&answer),
        [vec![
            "01a1c4f0-0000-7000-8000-000000000003",
            "verification:V-1"
        ]],
        "{answer}"
    );
    let warning = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        warning.trim_end(),
        "warning: 1 document whose links cannot be read and 1 file whose identity cannot be read may join units told apart"
    );
}

/// The key of each unit's documents, or its identity when it has no key.
fn keys_or_ids(answer: &Value) -> Vec<Vec<String>> {
    answer["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| {
            unit["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| match d["doc"]["key"].as_str() {
                    Some(key) => key.to_owned(),
                    None => d["doc"]["id"].as_str().unwrap().to_owned(),
                })
                .collect()
        })
        .collect()
}
