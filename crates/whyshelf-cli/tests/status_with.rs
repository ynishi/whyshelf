//! `status --with` (a field that holds a value, whichever), on the binary and a small
//! project of files: it narrows the answer and combines with the other conditions,
//! the answer carries it as given, and a `--with` no document could pass, or that is
//! no field, is answered with exit 2 and changes nothing.
//!
//! Whether one document passes the condition is tested beside the rule, in the domain.

mod common;

use std::fs;

use common::{Project, json};
use serde_json::{Value, json as value};

/// Two books: a decision has a level (an enum) and a size (an integer) beside its
/// title; a note has a title only.
const DECLARATION: &str = r#"[[book]]
name = "decision"
dir = "decisions"
key = { prefix = "DEC" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.field]]
name = "level"
type = { enum = ["low", "high"] }
[[book.field]]
name = "size"
type = "integer"

[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
"#;

/// A decision file: its number and the extra lines of its front matter.
fn decision(n: u32, extra: &str) -> String {
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000001{n:02}\"\nkey: \"DEC-{n}\"\ntitle: \"Decision {n}\"\n{extra}---\nBody {n}.\n"
    )
}

/// The project: DEC-1 (high, size 3), DEC-2 (low), DEC-3 (neither), DEC-4 (both
/// written with nothing in them), and the note N-1.
fn project(name: &str) -> Project {
    let project = Project::with(name, DECLARATION, "");
    let dir = &project.0;
    fs::create_dir_all(dir.join("decisions")).unwrap();
    for (n, extra) in [
        (1, "level: high\nsize: 3\n"),
        (2, "level: low\n"),
        (3, ""),
        (4, "size: \"\"\nlevel:\n"),
    ] {
        fs::write(
            dir.join(format!("decisions/DEC-{n}.md")),
            decision(n, extra),
        )
        .unwrap();
    }
    fs::write(
        dir.join("notes/N-1.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n",
    )
    .unwrap();
    project
}

/// The keys of the documents of an answer, in its order.
fn keys(answer: &Value) -> Vec<String> {
    answer["docs"]
        .as_array()
        .expect("docs")
        .iter()
        .map(|doc| doc["doc"]["key"].as_str().unwrap().to_owned())
        .collect()
}

fn status(project: &Project, args: &[&str]) -> (Option<i32>, Value) {
    let mut all = vec!["status"];
    all.extend_from_slice(args);
    let output = project.run(&all);
    (output.status.code(), json(&output))
}

fn answered(project: &Project, args: &[&str]) -> Value {
    let (code, answer) = status(project, args);
    assert_eq!(code, Some(0), "{args:?}: {answer}");
    answer
}

#[test]
fn with_answers_the_documents_that_hold_a_value_and_the_filters_say_it() {
    let project = project("status-with");
    let by = |args: &[&str]| keys(&answered(&project, args));
    // A field written with nothing in it holds no value, so it is not held either.
    assert_eq!(
        by(&["--with", "level"]),
        vec!["decision:DEC-1", "decision:DEC-2"]
    );
    assert_eq!(by(&["--with", "size"]), vec!["decision:DEC-1"]);
    // Every one has to hold, beside the others and `--where` on the same field.
    assert_eq!(
        by(&["--with", "size", "--with", "level"]),
        vec!["decision:DEC-1"]
    );
    assert_eq!(
        by(&["--with", "level", "--where", "level=high"]),
        vec!["decision:DEC-1"]
    );
    assert_eq!(
        by(&["--with", "level", "--without", "size"]),
        vec!["decision:DEC-2"]
    );
    // A book that does not declare the field passes nothing, and a field every
    // document holds is answered, not refused as the `--without` of it is.
    assert_eq!(
        by(&["--book", "note", "--with", "level"]),
        Vec::<String>::new()
    );
    assert_eq!(
        by(&["--with", "title"]),
        vec![
            "note:N-1",
            "decision:DEC-1",
            "decision:DEC-2",
            "decision:DEC-3",
            "decision:DEC-4"
        ]
    );
    // The answer carries them as given, apart from the others.
    let answer = answered(&project, &["--with", "size", "--with", "level"]);
    assert_eq!(
        answer["filters"],
        value!({
            "books": [],
            "standings": [],
            "readers": [],
            "where": [],
            "without": [],
            "with": ["size", "level"],
        })
    );
}

#[test]
fn a_with_no_document_could_pass_or_that_is_no_field_is_exit_2_and_changes_nothing() {
    let project = project("status-with-refused");
    let before = project.files();
    let (code, answer) = status(&project, &["--with", "level", "--without", "level"]);
    assert_eq!(code, Some(2), "{answer}");
    assert_eq!(answer["error"], "request");
    assert_eq!(
        answer["problems"],
        value!([
            "the field `level` is asked for with a value and with no value: no document holds both"
        ])
    );
    let (code, answer) = status(&project, &["--with", "nothing"]);
    assert_eq!(code, Some(2), "{answer}");
    assert_eq!(
        answer["problems"],
        value!(["no book declares a field `nothing`"])
    );
    let (code, answer) = status(&project, &["--with", "Level"]);
    assert_eq!(code, Some(2), "{answer}");
    assert!(
        answer["messages"][0]
            .as_str()
            .unwrap()
            .starts_with("--with `Level`"),
        "{answer}"
    );
    assert_eq!(project.files(), before);
}
