//! `fmt --drop` and `fmt --check --drop`, on the binary and a small project of files:
//! retiring a field from one book. `status --with` finds the documents that hold it,
//! the declaration is edited, and `fmt --check --drop` says which files lose which
//! names before `fmt --drop` takes them out; a name that does not fit the declaration
//! is a request error (exit 2) that writes nothing.
//!
//! What the filter and the form decide is tested beside their rules, in the domain.

mod common;

use std::fs;

use common::{Project, json};
use serde_json::{Value, json as value};

/// Two books that both declare `source`, an optional field.
const BOTH_DECLARE: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.field]]
name = "source"
type = "text"

[[book]]
name = "memo"
dir = "memos"
key = { prefix = "M" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.field]]
name = "source"
type = "text"
"#;

/// The same, once `source` is taken out of the declaration of `note`.
const ONLY_MEMO_DECLARES: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true

[[book]]
name = "memo"
dir = "memos"
key = { prefix = "M" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.field]]
name = "source"
type = "text"
"#;

const NOTE_WITH_SOURCE: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\nsource: \"x\"\ntitle: \"A note\"\n---\nThe body.\n";

const NOTE_WITHOUT: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n";

const MEMO_WITH_SOURCE: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000002\"\nkey: \"M-1\"\nsource: \"y\"\ntitle: \"A memo\"\n---\nThe memo.\n";

/// The project with both books declaring `source`, N-1 and M-1 each holding one.
fn project(name: &str) -> Project {
    let project = Project::with(name, BOTH_DECLARE, NOTE_WITH_SOURCE);
    fs::create_dir_all(project.0.join("memos")).unwrap();
    fs::write(project.0.join("memos/M-1.md"), MEMO_WITH_SOURCE).unwrap();
    project
}

fn declare(project: &Project, declaration: &str) {
    fs::write(project.0.join(".whyshelf/books.toml"), declaration).unwrap();
}

fn read(project: &Project, file: &str) -> String {
    fs::read_to_string(project.0.join(file)).unwrap()
}

fn keys(answer: &Value) -> Vec<String> {
    answer["docs"]
        .as_array()
        .expect("docs")
        .iter()
        .map(|doc| doc["doc"]["key"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_field_is_retired_by_finding_it_taking_its_declaration_out_and_dropping_it() {
    let project = project("fmt-drop");
    // Before: both documents hold the field, and fmt on its own keeps it.
    let held = json(&project.run(&["status", "--with", "source"]));
    assert_eq!(keys(&held), vec!["note:N-1", "memo:M-1"]);
    assert_eq!(held["filters"]["with"], value!(["source"]));
    assert_eq!(project.run(&["fmt"]).status.code(), Some(0));
    assert_eq!(read(&project, "notes/N-1.md"), NOTE_WITH_SOURCE);

    // The declaration of `note` no longer holds it: its document tells an unknown entry.
    declare(&project, ONLY_MEMO_DECLARES);
    assert_eq!(project.run(&["check"]).status.code(), Some(1));

    // Without `--drop`, fmt keeps the entry, as it always has.
    let plain = project.run(&["fmt", "--check"]);
    assert_eq!(plain.status.code(), Some(0));
    assert_eq!(json(&plain)["dropped"], value!([]));

    // The dry run says which file loses which name, and writes nothing.
    let before = project.files();
    let dry = project.run(&["fmt", "--check", "--drop", "source"]);
    assert_eq!(dry.status.code(), Some(1));
    let answer = json(&dry);
    assert_eq!(answer["not_in_form"], value!(["notes/N-1.md"]));
    assert_eq!(answer["rewritten"], value!([]));
    assert_eq!(answer["in_form"], 1);
    assert_eq!(
        answer["dropped"],
        value!([{"file": "notes/N-1.md", "names": ["source"]}])
    );
    assert_eq!(project.files(), before);

    // The write takes the entry out of the file whose book does not declare it, and
    // leaves the one whose book does.
    let output = project.run(&["fmt", "--drop", "source"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["rewritten"], value!(["notes/N-1.md"]));
    assert_eq!(answer["not_in_form"], Value::Null);
    assert_eq!(
        answer["dropped"],
        value!([{"file": "notes/N-1.md", "names": ["source"]}])
    );
    assert_eq!(read(&project, "notes/N-1.md"), NOTE_WITHOUT);
    assert_eq!(read(&project, "memos/M-1.md"), MEMO_WITH_SOURCE);

    // The unknown entry is gone from `check`, and every file is in the form.
    assert_eq!(project.run(&["check"]).status.code(), Some(0));
    let again = project.run(&["fmt", "--check", "--drop", "source"]);
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(json(&again)["dropped"], value!([]));
    // The documents that still hold a value are the ones of the book that declares it.
    let held = json(&project.run(&["status", "--with", "source"]));
    assert_eq!(keys(&held), vec!["memo:M-1"]);
}

#[test]
fn a_file_with_a_comment_keeps_its_entry_and_check_still_tells_it() {
    let project = project("fmt-drop-comment");
    declare(&project, ONLY_MEMO_DECLARES);
    let commented = NOTE_WITH_SOURCE.replace("title: \"A note\"", "title: \"A note\" # why");
    fs::write(project.0.join("notes/N-1.md"), &commented).unwrap();
    let output = project.run(&["fmt", "--drop", "source"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["rewritten"], value!([]));
    assert_eq!(answer["dropped"], value!([]));
    assert_eq!(answer["left_alone"][0]["reason"], "comment");
    assert_eq!(read(&project, "notes/N-1.md"), commented);
    assert_eq!(project.run(&["check"]).status.code(), Some(1));
}

#[test]
fn names_that_do_not_fit_the_declaration_are_exit_2_and_write_nothing() {
    let project = project("fmt-drop-refused");
    let before = project.files();
    // `source` is declared by both books, so nothing would be dropped.
    for check in [false, true] {
        let mut args = vec!["fmt", "--drop", "source"];
        if check {
            args.push("--check");
        }
        let output = project.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let answer = json(&output);
        assert_eq!(answer["error"], "request");
        assert_eq!(
            answer["problems"],
            value!([
                "every book declares `source`, so nothing would be dropped: take it out of the declaration of the books first"
            ])
        );
    }
    // The id and the key are never dropped; every problem is told, in the order of the
    // names.
    let output = project.run(&["fmt", "--drop", "key", "--drop", "id", "--drop", "source"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json(&output)["problems"],
        value!([
            "`id` cannot be dropped: every document has an `id` and a `key`",
            "`key` cannot be dropped: every document has an `id` and a `key`",
            "every book declares `source`, so nothing would be dropped: take it out of the declaration of the books first"
        ])
    );
    assert_eq!(project.files(), before);
}

#[test]
fn a_blank_name_is_an_argument_error_exit_2_and_writes_nothing() {
    let project = project("fmt-drop-blank");
    let before = project.files();
    for given in ["", "  "] {
        for check in [false, true] {
            let mut args = vec!["fmt", "--drop", given];
            if check {
                args.push("--check");
            }
            let output = project.run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            let answer = json(&output);
            assert_eq!(answer["error"], "request", "{args:?}");
            assert!(
                answer["messages"][0]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("--drop `{given}`: ")),
                "{answer}"
            );
        }
    }
    assert_eq!(project.files(), before);
}
