//! `status` with filters, on the binary and a small project of files: the three
//! filters narrow the answer and combine, the answer carries them as given, `health`
//! stays of the whole project, and a filter that is no request is answered with exit
//! 2 and every problem.
//!
//! Whether one document passes a filter is tested beside the rule, in the domain.

mod common;

use std::fs;

use common::{Project, json};
use serde_json::{Value, json as value};

/// Two books: a decision names the issues it answers (a list), has a level (an
/// enum) and a size (an integer); a note has a title only.
const DECLARATION: &str = r#"[[book]]
name = "decision"
dir = "decisions"
key = { prefix = "DEC" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.field]]
name = "issues"
type = "list"
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

/// A decision file: its number, the issues it names and the extra lines of its front
/// matter.
fn decision(n: u32, issues: &[&str], extra: &str) -> String {
    let issues: Vec<String> = issues.iter().map(|i| format!("\"{i}\"")).collect();
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000001{n:02}\"\nkey: \"DEC-{n}\"\ntitle: \"Decision {n}\"\nissues: [{}]\n{extra}---\nBody {n}.\n",
        issues.join(", ")
    )
}

/// The project: DEC-1 (#41, high, size 3), DEC-2 (#41 and #42, low), DEC-3 (#42), the
/// note N-1, and a file in `notes` that is no document.
fn project(name: &str) -> Project {
    let project = Project::with(name, DECLARATION, "");
    let dir = &project.0;
    fs::create_dir_all(dir.join("decisions")).unwrap();
    for (n, issues, extra) in [
        (1, &["#41"][..], "level: high\nsize: 3\n"),
        (2, &["#41", "#42"][..], "level: low\n"),
        (3, &["#42"][..], ""),
    ] {
        fs::write(
            dir.join(format!("decisions/DEC-{n}.md")),
            decision(n, issues, extra),
        )
        .unwrap();
    }
    fs::write(
        dir.join("notes/N-1.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n",
    )
    .unwrap();
    fs::write(dir.join("notes/broken.md"), "no front matter here\n").unwrap();
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
fn no_filter_is_every_document_and_the_filters_are_empty() {
    let project = project("status-none");
    let answer = answered(&project, &[]);
    assert_eq!(answer["version"], 1);
    assert_eq!(
        keys(&answer),
        // In the order of the identities, as `status` always answered.
        vec![
            "note:N-1",
            "decision:DEC-1",
            "decision:DEC-2",
            "decision:DEC-3"
        ]
    );
    assert_eq!(
        answer["filters"],
        value!({"books": [], "standings": [], "where": []})
    );
    // The same as naming every book: the rows are what `status` always answered.
    let both = answered(&project, &["--book", "decision", "--book", "note"]);
    assert_eq!(both["docs"], answer["docs"]);
    assert_eq!(both["health"], answer["health"]);
}

#[test]
fn a_book_and_a_field_answer_the_decisions_that_name_the_issue() {
    let project = project("status-issue");
    let answer = answered(&project, &["--book", "decision", "--where", "issues=#41"]);
    assert_eq!(keys(&answer), vec!["decision:DEC-1", "decision:DEC-2"]);
    assert_eq!(
        answer["filters"],
        value!({
            "books": ["decision"],
            "standings": [],
            "where": [{"field": "issues", "value": "#41"}],
        })
    );
    // The book alone, and the field alone (only a decision declares it).
    assert_eq!(
        keys(&answered(&project, &["--book", "note"])),
        vec!["note:N-1"]
    );
    assert_eq!(
        keys(&answered(&project, &["--where", "issues=#42"])),
        vec!["decision:DEC-2", "decision:DEC-3"]
    );
}

#[test]
fn the_three_filters_combine_and_a_repeated_kind_is_either() {
    let project = project("status-combine");
    assert_eq!(
        project.run(&["judge", "approve", "DEC-1"]).status.code(),
        Some(0)
    );
    let by = |args: &[&str]| keys(&answered(&project, args));
    assert_eq!(
        by(&[
            "--book",
            "decision",
            "--standing",
            "fresh",
            "--where",
            "issues=#41"
        ]),
        vec!["decision:DEC-1"]
    );
    assert_eq!(
        by(&[
            "--book",
            "decision",
            "--standing",
            "pending",
            "--where",
            "issues=#41"
        ]),
        vec!["decision:DEC-2"]
    );
    assert_eq!(
        by(&[
            "--standing",
            "fresh",
            "--standing",
            "pending",
            "--book",
            "note"
        ]),
        vec!["note:N-1"]
    );
    // A kind repeated is either; each `--where` is one more condition.
    assert_eq!(
        by(&[
            "--book",
            "note",
            "--book",
            "decision",
            "--standing",
            "fresh"
        ]),
        vec!["decision:DEC-1"]
    );
    assert_eq!(
        by(&["--where", "issues=#41", "--where", "issues=#42"]),
        vec!["decision:DEC-2"]
    );
    assert_eq!(
        by(&[
            "--where",
            "level=high",
            "--where",
            "size=3",
            "--where",
            "issues=#41"
        ]),
        vec!["decision:DEC-1"]
    );
    assert_eq!(
        by(&["--where", "level=high", "--where", "issues=#42"]),
        Vec::<String>::new()
    );
}

#[test]
fn the_answer_carries_the_filters_as_given_with_the_standings_in_a_fixed_order() {
    let project = project("status-echo");
    let answer = answered(
        &project,
        &[
            "--standing",
            "stale",
            "--standing",
            "pending",
            "--book",
            "note",
            "--book",
            "decision",
            "--where",
            "size=3",
            "--where",
            "issues=#41=x",
        ],
    );
    assert_eq!(
        answer["filters"],
        value!({
            "books": ["decision", "note"],
            "standings": ["pending", "stale"],
            "where": [
                {"field": "size", "value": "3"},
                // The value is what follows the first `=`, as typed.
                {"field": "issues", "value": "#41=x"},
            ],
        })
    );
}

#[test]
fn the_health_is_of_the_whole_project_whatever_the_rows() {
    let project = project("status-health");
    let whole = answered(&project, &[]);
    assert_eq!(
        whole["health"]["clean"], false,
        "the file that is no document"
    );
    let narrow = answered(&project, &["--book", "decision", "--where", "issues=#99"]);
    assert_eq!(keys(&narrow), Vec::<String>::new());
    assert_eq!(narrow["health"], whole["health"]);
}

#[test]
fn a_filter_that_is_no_request_is_exit_2_and_changes_nothing() {
    let project = project("status-refused");
    let before = project.files();
    for args in [
        &["--book", "nothing"][..],
        &["--where", "nothing=x"][..],
        &["--where", "level=urgent"][..],
        &["--where", "size=many"][..],
    ] {
        let (code, answer) = status(&project, args);
        assert_eq!(code, Some(2), "{args:?}: {answer}");
        assert_eq!(answer["error"], "request", "{args:?}");
        assert_eq!(
            answer["messages"][0], "the filter has 1 problem(s)",
            "{args:?}"
        );
    }
    // An unknown book is told in the words of every other command that names one.
    let (_, answer) = status(&project, &["--book", "nothing"]);
    assert_eq!(answer["problems"], value!(["there is no book `nothing`"]));
    let (_, answer) = status(&project, &["--where", "nothing=x"]);
    assert_eq!(
        answer["problems"],
        value!(["no book declares a field `nothing`"])
    );
    let (_, answer) = status(&project, &["--where", "level=urgent"]);
    assert_eq!(
        answer["problems"],
        value!(["`urgent` is no value of the field `level` in any book that declares it"])
    );
    assert_eq!(project.files(), before);
}

#[test]
fn every_problem_of_a_filter_is_listed() {
    let project = project("status-all-problems");
    let (code, answer) = status(
        &project,
        &[
            "--book",
            "nothing",
            "--where",
            "absent=1",
            "--where",
            "size=many",
            "--where",
            "issues=#1",
        ],
    );
    assert_eq!(code, Some(2), "{answer}");
    assert_eq!(
        answer["problems"],
        value!([
            "there is no book `nothing`",
            "no book declares a field `absent`",
            "`many` is no value of the field `size` in any book that declares it",
        ])
    );
}

#[test]
fn arguments_that_are_no_filter_are_a_usage_error() {
    let project = project("status-usage");
    for (args, text) in [
        (&["--standing", "soon"][..], "--standing `soon`"),
        (&["--where", "level"][..], "--where `level`"),
        (&["--where", "Level=high"][..], "--where `Level=high`"),
        (&["--book", "No Book"][..], "--book `No Book`"),
    ] {
        let (code, answer) = status(&project, args);
        assert_eq!(code, Some(2), "{args:?}: {answer}");
        assert_eq!(answer["error"], "request", "{args:?}");
        let message = answer["messages"][0].as_str().unwrap();
        assert!(message.starts_with(text), "{args:?}: {message}");
    }
    let (_, answer) = status(&project, &["--standing", "soon"]);
    let message = answer["messages"][0].as_str().unwrap();
    for word in [
        "pending",
        "fresh",
        "expired",
        "stale",
        "broken",
        "denied",
        "deprecated",
        "retired",
    ] {
        assert!(message.contains(word), "{word}: {message}");
    }
}

#[test]
fn the_schema_of_status_has_the_filters() {
    let project = project("status-schema");
    let output = project.run(&["schema"]);
    assert_eq!(output.status.code(), Some(0));
    let schema = json(&output);
    let text = schema.to_string();
    assert!(text.contains("\"filters\""), "{text}");
    assert!(text.contains("\"where\""), "{text}");
}
