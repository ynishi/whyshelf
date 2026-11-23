//! `reach`, on the binary and one small project of files: a requirement reaches the
//! decision that serves it and the verification that checks that, the files and tests
//! they stand on (what their links name now, and what an approval pinned), and the
//! files of the project all of it is in; a document whose links cannot be read and
//! that is not reached is counted, with a warning; a reference that names nothing is a
//! request error.
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

const TESTS: &str = "#[test]\nfn holds() {\n    assert!(true);\n}\n";

/// A document of `book` with identity `n`, its key, and the front matter `extra`.
fn doc(project: &Project, book: &str, n: u8, key: &str, extra: &str) {
    let text = format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000000{n:02}\"\nkey: \"{key}\"\ntitle: \"{key}\"\n{extra}---\nThe body.\n"
    );
    fs::write(project.0.join(format!("books/{book}/{key}.md")), text).unwrap();
}

/// The project: `REQ-1`, served by `DEC-1` (code `src/lib.rs`), checked by `V-1`
/// (the test `tests/a.rs::holds`); and `REQ-2`, served by `DEC-2`, apart.
fn project(name: &str) -> Project {
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
    fs::write(project.0.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
    fs::write(project.0.join("src/old.rs"), "pub fn y() {}\n").unwrap();
    fs::write(project.0.join("tests/a.rs"), TESTS).unwrap();
    doc(&project, "requirement", 1, "REQ-1", "");
    doc(&project, "requirement", 2, "REQ-2", "");
    doc(
        &project,
        "decision",
        3,
        "DEC-1",
        "serves: [\"REQ-1\"]\ncode: [\"src/lib.rs\"]\n",
    );
    doc(&project, "decision", 4, "DEC-2", "serves: [\"REQ-2\"]\n");
    doc(
        &project,
        "verification",
        5,
        "V-1",
        "checks: [\"DEC-1\"]\ntests: [\"tests/a.rs::holds\"]\n",
    );
    project
}

/// The keys of the documents of an answer's list.
fn keys(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|d| d["doc"]["key"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_requirement_reaches_its_decision_its_verification_and_what_they_stand_on() {
    let project = project("reach");
    let output = project.run(&["reach", "REQ-1"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "no warning");
    let answer = json(&output);
    let mut reached = keys(&answer["docs"]);
    reached.sort();
    assert_eq!(
        reached,
        ["decision:DEC-1", "requirement:REQ-1", "verification:V-1"],
        "{answer}"
    );
    let named: Vec<&Value> = answer["docs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["named"] == true)
        .collect();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0]["doc"]["key"], "requirement:REQ-1");
    assert_eq!(
        answer["artifacts"],
        json!([
            {
                "space": "file",
                "name": "src/lib.rs",
                "by": [{"id": "01a1c4f0-0000-7000-8000-000000000003", "key": "decision:DEC-1"}],
                "place": {"place": "only", "files": ["src/lib.rs"]},
            },
            {
                "space": "rust-test",
                "name": "tests/a.rs::holds",
                "by": [{"id": "01a1c4f0-0000-7000-8000-000000000005", "key": "verification:V-1"}],
                "place": {"place": "only", "files": ["tests/a.rs"]},
            },
        ]),
        "{answer}"
    );
    assert_eq!(
        answer["files"],
        json!([
            "books/decision/DEC-1.md",
            "books/requirement/REQ-1.md",
            "books/verification/V-1.md",
            "src/lib.rs",
            "tests/a.rs",
        ])
    );
    assert_eq!(answer["unplaced"], json!([]));
    assert_eq!(answer["gone"], json!([]));
    assert!(answer.get("untold").is_none(), "{answer}");

    // What an approval pinned is stood on too, after the link names another file.
    assert_eq!(
        project
            .run(&["judge", "approve", "REQ-1", "DEC-1", "V-1"])
            .status
            .code(),
        Some(0)
    );
    doc(
        &project,
        "decision",
        3,
        "DEC-1",
        "serves: [\"REQ-1\"]\ncode: [\"src/old.rs\"]\n",
    );
    let answer = json(&project.run(&["reach", "DEC-1"]));
    let names: Vec<&str> = answer["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["src/lib.rs", "src/old.rs", "tests/a.rs::holds"]);
}

#[test]
fn a_document_whose_links_cannot_be_read_is_counted_and_said_and_a_bad_reference_is_refused() {
    let project = project("reach-untold");
    // A decision with no title: it is invalid, and what it serves cannot be read.
    fs::write(
        project.0.join("books/decision/DEC-9.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000009\"\nkey: \"DEC-9\"\nserves: [\"REQ-1\"]\n---\nThe body.\n",
    )
    .unwrap();
    let output = project.run(&["reach", "REQ-1", "REQ-2"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["untold"], json!({"invalid": 1, "unreadable": 0}));
    assert_eq!(answer["docs"].as_array().unwrap().len(), 5, "{answer}");
    let warning = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        warning.trim_end(),
        "warning: 1 document whose links cannot be read may depend on the named, so what they reach may be more than this answer says"
    );

    let refused = project.run(&["reach", "REQ-1", "REQ-77"]);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(json(&refused)["error"], "request");
    assert_eq!(project.run(&["reach"]).status.code(), Some(2));
}
