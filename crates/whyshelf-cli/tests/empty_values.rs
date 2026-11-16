//! What a value written with nothing in it, an item that says nothing and a line break
//! in a text come to, on the binary and a small project of files:
//!
//! - an optional field written `""`, with no value (`source:`) or as `[]` is read as
//!   left out, so the document is valid and the answer shows no value for it;
//! - an item of a list that says nothing makes the document invalid, and `check` names
//!   which item;
//! - a title of the project `init` makes is a `line`, so a line break in it makes the
//!   document invalid, and `new` refuses it;
//! - `status --where field=` (no value) is a request that cannot be answered: exit 2;
//! - `fmt` writes a no value as `null`, and the approval of the document still holds.

mod common;

use std::fs;

use common::{Project, json};

/// A note with an optional text, an optional list and an optional integer.
const DECLARATION: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "line"
required = true
[[book.field]]
name = "source"
type = "text"
[[book.field]]
name = "tags"
type = "list"
[[book.field]]
name = "count"
type = "integer"
"#;

/// A note numbered `n` with this title and these extra lines of front matter.
fn note_titled(n: u32, title: &str, extra: &str) -> String {
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000001{n:02}\"\nkey: \"N-{n}\"\ntitle: \"{title}\"\n{extra}---\nThe body.\n"
    )
}

/// A project of these notes, `(number, extra front matter)`, each titled `Note <n>`.
fn project(name: &str, notes: &[(u32, &str)]) -> Project {
    let project = Project::with(name, DECLARATION, "");
    fs::remove_file(project.0.join("notes/N-1.md")).unwrap();
    for (n, extra) in notes {
        let text = note_titled(*n, &format!("Note {n}"), extra);
        fs::write(project.0.join(format!("notes/N-{n}.md")), text).unwrap();
    }
    project
}

fn problems(answer: &serde_json::Value, file: &str) -> Vec<String> {
    answer["health"]["invalid"]
        .as_array()
        .expect("a list of invalid files")
        .iter()
        .find(|entry| entry["file"] == file)
        .unwrap_or_else(|| panic!("{file} is invalid: {answer}"))["problems"]
        .as_array()
        .expect("a list of problems")
        .iter()
        .map(|problem| problem.as_str().expect("a problem is a text").to_owned())
        .collect()
}

#[test]
fn an_optional_field_written_empty_or_with_no_value_is_valid_and_shows_no_value() {
    let project = project(
        "empty-optional",
        &[
            (1, "source: \"\"\n"),
            (2, "source:\n"),
            (3, "source: \"   \"\ntags: []\n"),
            (4, "count: \"\"\ntags: ~\n"),
            (5, "source: \"kept\"\n"),
        ],
    );
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", json(&check));
    assert_eq!(json(&check)["health"]["invalid"], serde_json::json!([]));
    let fields = |key: &str| {
        let output = project.run(&["get", key]);
        assert_eq!(output.status.code(), Some(0), "{key}");
        json(&output)["content"]["fields"].clone()
    };
    for key in ["N-1", "N-2", "N-3", "N-4"] {
        assert_eq!(
            fields(key),
            serde_json::json!({"title": format!("Note {}", &key[2..])}),
            "{key}"
        );
    }
    assert_eq!(fields("N-5")["source"], "kept");
}

#[test]
fn a_document_that_writes_nothing_has_the_hash_of_one_that_leaves_it_out() {
    let project = Project::with("empty-hash", DECLARATION, "");
    let writings = [
        (1, "source: \"\"\ntags: []\ncount: \"\"\n"),
        (2, ""),
        (3, "source:\ntags: ~\n"),
        (4, "source: \"  \"\n"),
    ];
    for (n, extra) in writings {
        fs::write(
            project.0.join(format!("notes/N-{n}.md")),
            note_titled(n, "Same title", extra),
        )
        .unwrap();
    }
    let hash = |key: &str| {
        let output = project.run(&["get", key]);
        assert_eq!(output.status.code(), Some(0), "{key}");
        json(&output)["row"]["presence"]["hash"].clone()
    };
    for key in ["N-1", "N-3", "N-4"] {
        assert_eq!(hash(key), hash("N-2"), "{key}");
    }
}

#[test]
fn an_item_of_a_list_that_says_nothing_makes_the_document_invalid_and_check_names_it() {
    let project = project(
        "empty-item",
        &[
            (1, "tags: [\"a\", \"\"]\n"),
            (2, "tags: [\"a\", \"  \", \"b\"]\n"),
            (3, "tags: [\"a\", \"b\"]\n"),
        ],
    );
    let check = project.run(&["check"]);
    let answer = json(&check);
    assert_ne!(check.status.code(), Some(0), "{answer}");
    assert_eq!(
        problems(&answer, "notes/N-1.md"),
        ["the item 2 of `tags` is empty or blank"]
    );
    assert_eq!(
        problems(&answer, "notes/N-2.md"),
        ["the item 2 of `tags` is empty or blank"]
    );
    let invalid = answer["health"]["invalid"].as_array().unwrap();
    assert_eq!(invalid.len(), 2, "{answer}");
}

#[test]
fn new_refuses_an_item_that_says_nothing_and_writes_nothing() {
    let project = project("empty-new-item", &[(1, "")]);
    let before = project.files();
    let output = project.run(&["new", "note", "--title", "t", "tags+="]);
    assert_eq!(output.status.code(), Some(2), "{}", json(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("the item 1 of `tags` is empty or blank"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(project.files(), before);
}

#[test]
fn new_does_not_write_a_field_given_with_nothing() {
    let project = project("empty-new-field", &[(1, "")]);
    let output = project.run(&["new", "note", "--title", "t", "source=", "tags="]);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    let written = fs::read_to_string(project.0.join("notes/N-2.md")).unwrap();
    assert!(!written.contains("source"), "{written}");
    assert!(!written.contains("tags"), "{written}");
    assert!(written.contains("title: \"t\""), "{written}");
}

#[test]
fn new_from_a_line_with_a_null_entry_writes_it_left_out() {
    let project = project("empty-new-from-null", &[(1, "")]);
    let line = r#"{"book":"note","entries":{"title":"t","source":null,"tags":null}}"#;
    let output = project.run_with_stdin(&["new", "--from", "-"], &format!("{line}\n"));
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    let written = fs::read_to_string(project.0.join("notes/N-2.md")).unwrap();
    assert!(!written.contains("source"), "{written}");
    assert!(!written.contains("tags"), "{written}");
    assert!(written.contains("title: \"t\""), "{written}");
}

#[test]
fn a_title_of_a_project_init_made_is_one_line() {
    let project = Project::empty("empty-line");
    let init = project.run(&["init"]);
    assert_eq!(init.status.code(), Some(0));
    // A title on one line is a note.
    let one = project.run(&["new", "note", "--title", "one line"]);
    assert_eq!(one.status.code(), Some(0), "{}", json(&one));
    // `new` does not write a title with a line break.
    let before = project.files();
    let two = project.run(&["new", "note", "--title", "two\nlines"]);
    assert_eq!(two.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&two.stderr).contains("a text of more than one line, not `line`"),
        "{}",
        String::from_utf8_lossy(&two.stderr)
    );
    assert_eq!(project.files(), before);
    // A file written by hand with one is invalid, whether it was written with `|` or not.
    let by_hand = |n: u32, title: &str| {
        format!(
            "---\nid: \"01a1c4f0-0000-7000-8000-0000000001{n:02}\"\nkey: \"N-{n}\"\ntitle: {title}\n---\n"
        )
    };
    fs::write(project.0.join("books/note/N-7.md"), by_hand(7, "\"a\\nb\"")).unwrap();
    fs::write(project.0.join("books/note/N-8.md"), by_hand(8, "|\n  a\n")).unwrap();
    fs::write(project.0.join("books/note/N-9.md"), by_hand(9, "|-\n  a\n")).unwrap();
    let answer = json(&project.run(&["check"]));
    let one_line = "the field `title` is a text of more than one line, not `line`";
    for file in ["books/note/N-7.md", "books/note/N-8.md"] {
        let told = problems(&answer, file);
        assert_eq!(told.len(), 1, "{file}: {told:?}");
        assert!(told[0].starts_with(one_line), "{file}: {told:?}");
    }
    let invalid = answer["health"]["invalid"].as_array().unwrap();
    assert!(
        invalid
            .iter()
            .all(|entry| entry["file"] != "books/note/N-9.md"),
        "`|-` keeps no line break at its end: {answer}"
    );
}

#[test]
fn status_where_with_no_value_is_a_request_that_cannot_be_answered() {
    let project = project(
        "empty-where",
        &[(1, "source: \"x\"\n"), (2, "source: \"\"\n")],
    );
    for value in ["", "  "] {
        let output = project.run(&["status", "--where", &format!("source={value}")]);
        assert_eq!(output.status.code(), Some(2), "{value:?}");
        let answer = json(&output);
        let told = answer.to_string();
        assert!(
            told.contains("the value of the condition on `source` is empty"),
            "{value:?}: {told}"
        );
    }
    // A value is still asked for as before.
    let output = project.run(&["status", "--where", "source=x"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn fmt_writes_a_no_value_as_null_and_the_approval_still_holds() {
    let project = project("empty-fmt", &[(1, "source:\n")]);
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    let output = project.run(&["fmt"]);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    assert_eq!(
        json(&output)["rewritten"],
        serde_json::json!(["notes/N-1.md"])
    );
    let written = fs::read_to_string(project.0.join("notes/N-1.md")).unwrap();
    assert!(written.contains("\nsource: null\n"), "{written}");
    // The same document: its hash did not change, so the approval holds.
    let stale = project.run(&["stale", "--exit-code"]);
    assert_eq!(stale.status.code(), Some(0), "{}", json(&stale));
    assert_eq!(json(&stale)["stale"], serde_json::json!([]));
}

#[test]
fn fmt_writes_a_key_with_no_value_as_null_and_reads_it_back_as_the_same_thing() {
    let project = Project::with(
        "empty-fmt-key",
        DECLARATION,
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000101\"\nkey:\ntitle: \"t\"\n---\nBody.\n",
    );
    let before = problems(&json(&project.run(&["check"])), "notes/N-1.md");
    assert_eq!(before, ["`key:` is written empty or blank"]);
    let output = project.run(&["fmt"]);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    assert_eq!(
        json(&output)["rewritten"],
        serde_json::json!(["notes/N-1.md"])
    );
    assert_eq!(
        fs::read_to_string(project.0.join("notes/N-1.md")).unwrap(),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000101\"\nkey: null\ntitle: \"t\"\n---\nBody.\n"
    );
    // The file says the same thing as it did, and is in the form now.
    let after = problems(&json(&project.run(&["check"])), "notes/N-1.md");
    assert_eq!(after, before);
    assert_eq!(project.run(&["fmt", "--check"]).status.code(), Some(0));
}

#[test]
fn an_id_or_a_key_written_with_no_value_is_told_as_empty() {
    let project = Project::with(
        "empty-identity",
        DECLARATION,
        "---\nid:\nkey: \"N-1\"\ntitle: \"t\"\n---\n",
    );
    fs::write(
        project.0.join("notes/N-2.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000102\"\nkey: \"\"\ntitle: \"t\"\n---\n",
    )
    .unwrap();
    let answer = json(&project.run(&["check"]));
    assert_eq!(
        problems(&answer, "notes/N-1.md"),
        ["`id:` is written empty or blank"]
    );
    assert_eq!(
        problems(&answer, "notes/N-2.md"),
        ["`key:` is written empty or blank"]
    );
}
