//! `new --from`: the new documents of a file of JSON lines (or of the standard
//! input), judged all together and written all, or none.

mod common;

use std::fs;

use common::{BOOKS, NOTE, Project, json};
use serde_json::{Value, json};

/// The notes book with a list field and a link to notes.
fn books() -> String {
    format!(
        "{BOOKS}[[book.field]]\nname = \"tags\"\ntype = \"list\"\n\
         [[book.link]]\nname = \"base\"\ntarget = \"note\"\nmeans = \"depends\"\n"
    )
}

fn project(name: &str) -> Project {
    Project::with(name, &books(), NOTE)
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The lines as a file: one JSON object each.
fn lines(rows: &[Value]) -> String {
    rows.iter().map(|row| format!("{row}\n")).collect()
}

/// Write `text` as the file `lines.jsonl` of the project, and give its path.
fn put(project: &Project, text: &str) -> String {
    let at = project.0.join("lines.jsonl");
    fs::write(&at, text).unwrap();
    at.to_string_lossy().into_owned()
}

fn note(title: &str) -> Value {
    json!({"book": "note", "entries": {"title": title}})
}

#[test]
fn the_documents_of_a_file_are_written_and_told_in_one_answer() {
    let project = project("many-file");
    let path = put(
        &project,
        &lines(&[
            json!({"book": "note", "entries": {"title": "First", "tags": ["a, b", "c"]}, "body": "One.\n"}),
            json!({"book": "note", "key": "N-9", "entries": {"title": "Ninth"}}),
        ]),
    );
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    let keys: Vec<&str> = answer["docs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["doc"]["key"].as_str().unwrap())
        .collect();
    // The key the file asks for is passed over, as a key a file writes is.
    assert_eq!(keys, ["note:N-2", "note:N-9"]);
    assert_eq!(answer["heads"], json!(["note:N-2", "note:N-9"]));
    assert_eq!(answer["docs"][0]["file"], "notes/N-2.md");

    // The values were read as typed: the items of a list field keep their commas, the
    // body is written, and the shelf reads back clean.
    let written = fs::read_to_string(project.0.join("notes/N-2.md")).unwrap();
    assert!(written.contains("\"a, b\""), "{written}");
    assert!(written.ends_with("One.\n"), "{written}");
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    assert_eq!(json(&check)["clean"], true);
}

#[test]
fn the_standard_input_is_a_file_named_dash() {
    let project = project("many-stdin");
    let input = lines(&[note("From input"), note("Again")]);
    let output = project.run_with_stdin(&["new", "--from", "-"], &input);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["docs"].as_array().unwrap().len(), 2);
    assert_eq!(answer["docs"][0]["doc"]["key"], "note:N-2");
    assert_eq!(answer["docs"][1]["doc"]["key"], "note:N-3");
}

#[test]
fn twenty_five_lines_are_twenty_five_documents() {
    let project = project("many-25");
    let rows: Vec<Value> = (1..=25).map(|n| note(&format!("Note {n}"))).collect();
    let path = put(&project, &lines(&rows));
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    let docs = answer["docs"].as_array().unwrap();
    assert_eq!(docs.len(), 25);
    for (at, doc) in docs.iter().enumerate() {
        assert_eq!(doc["doc"]["key"], format!("note:N-{}", at + 2));
    }
    // N-1 was there; the files of the others were made.
    assert_eq!(fs::read_dir(project.0.join("notes")).unwrap().count(), 26);
    let check = project.run(&["check"]);
    assert_eq!(json(&check)["clean"], true);
}

#[test]
fn a_name_stands_for_the_key_of_its_line_and_the_file_has_the_key() {
    let project = project("many-names");
    // The first line links to the second by its name (forward) and to a document of
    // the project; the third links to the first (backward).
    let path = put(
        &project,
        &lines(&[
            json!({"book": "note", "as": "a", "entries": {"title": "A", "base": [{"as": "b"}, "N-1"]}}),
            json!({"book": "note", "as": "b", "entries": {"title": "B"}}),
            json!({"book": "note", "entries": {"title": "C", "base": [{"as": "a"}]}}),
        ]),
    );
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    // The line a link names is written before the line that links.
    let keys: Vec<&str> = answer["docs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["doc"]["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["note:N-3", "note:N-2", "note:N-4"]);
    // Every link of the run is resolved, whichever line comes first.
    for doc in answer["docs"].as_array().unwrap() {
        assert_eq!(doc["unresolved_links"], json!([]), "{doc}");
    }
    // Only the line nobody links to is a head.
    assert_eq!(answer["heads"], json!(["note:N-4"]));
    let first = fs::read_to_string(project.0.join("notes/N-2.md")).unwrap();
    assert!(first.contains("N-3") && first.contains("N-1"), "{first}");
    let third = fs::read_to_string(project.0.join("notes/N-4.md")).unwrap();
    assert!(third.contains("N-2"), "{third}");
    // No name is written to a file.
    for text in [&first, &third] {
        assert!(!text.contains("as:") && !text.contains("\"as\""), "{text}");
    }
    let check = project.run(&["check"]);
    assert_eq!(json(&check)["clean"], true);
}

#[test]
fn names_that_link_in_a_ring_are_written_in_the_order_of_the_file() {
    let project = project("many-ring");
    let path = put(
        &project,
        &lines(&[
            json!({"book": "note", "as": "a", "entries": {"title": "A", "base": [{"as": "b"}]}}),
            json!({"book": "note", "as": "b", "entries": {"title": "B", "base": [{"as": "a"}]}}),
        ]),
    );
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["docs"][0]["doc"]["key"], "note:N-2");
    assert_eq!(answer["docs"][1]["doc"]["key"], "note:N-3");
    // Both links are resolved; the ring is what `check` reports of the files.
    assert_eq!(answer["docs"][0]["unresolved_links"], json!([]));
    assert_eq!(answer["docs"][1]["unresolved_links"], json!([]));
    let check = project.run(&["check"]);
    assert_eq!(
        json(&check)["health"]["cycles"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn a_line_refused_writes_nothing_and_every_refused_line_is_told_with_its_number() {
    let project = project("many-refused");
    // Line 2 is blank (counted), line 3 has no title, line 4 asks for the key N-1.
    let text = format!(
        "{}\n\n{}\n{}\n{}\n",
        note("Fine"),
        json!({"book": "note", "entries": {}}),
        json!({"book": "note", "key": "N-1", "entries": {"title": "Taken"}}),
        note("Also fine"),
    );
    let path = put(&project, &text);
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    let problems: Vec<&str> = answer["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].starts_with("row 3: "), "{problems:?}");
    assert!(problems[1].starts_with("row 4: "), "{problems:?}");
    assert!(stderr(&output).contains("row 3: "), "{}", stderr(&output));
    assert_eq!(project.files(), before);
}

#[test]
fn a_name_no_line_has_refuses_the_run() {
    let project = project("many-unknown-name");
    let path = put(
        &project,
        &lines(&[
            note("Fine"),
            json!({"book": "note", "entries": {"title": "Links", "base": [{"as": "nobody"}]}}),
        ]),
    );
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        json(&output)["problems"],
        json!(["row 2 links `base` to `nobody`, which no row of the run is named"])
    );
    assert_eq!(project.files(), before);
}

#[test]
fn a_name_given_where_no_document_is_linked_is_refused_with_its_line() {
    let project = project("many-outside-link");
    let path = put(
        &project,
        &lines(&[
            json!({"book": "note", "as": "a", "entries": {"title": "A"}}),
            json!({"book": "note", "entries": {"title": "B", "tags": [{"as": "a"}]}}),
        ]),
    );
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        json(&output)["messages"],
        json!(["row 2: `tags` is no link, so it cannot name a row"])
    );
    assert_eq!(project.files(), before);
}

#[test]
fn a_line_that_is_no_json_object_of_the_shape_is_told_with_its_line_and_nothing_is_written() {
    let project = project("many-unreadable");
    let text = format!(
        "{}\nthis is not json\n{}\n{}\n",
        note("Fine"),
        json!({"book": "note", "entries": {"title": "T"}, "colour": "red"}),
        json!({"book": "note", "entries": {"title": 3.5}}),
    );
    let path = put(&project, &text);
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    let problems: Vec<&str> = answer["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].starts_with("row 2: "), "{problems:?}");
    assert!(
        problems[1].starts_with("row 3: ") && problems[1].contains("colour"),
        "{problems:?}"
    );
    assert!(problems[2].starts_with("row 4: "), "{problems:?}");
    assert_eq!(project.files(), before);
}

#[test]
fn an_entry_given_twice_in_a_line_is_refused_with_the_line() {
    let project = project("many-twice");
    // `json!` would drop the first: the line is written as text.
    let text = format!(
        "{}\n{}\n",
        note("Fine"),
        r#"{"book": "note", "entries": {"title": "a", "title": "b"}}"#
    );
    let path = put(&project, &text);
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        json(&output)["messages"],
        json!(["row 2: the entry `title` is given twice"])
    );
    assert_eq!(project.files(), before);
}

#[test]
fn every_row_with_a_fault_of_the_request_is_told_and_nothing_is_written() {
    let project = project("many-request-faults");
    // Line 2 gives `title` twice, line 3 is whole, line 4 has an empty name and line 5
    // names a line in a list that is no link: three lines, three faults, in one answer.
    let text = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        note("Fine"),
        r#"{"book": "note", "entries": {"title": "a", "title": "b"}}"#,
        note("Also fine"),
        json!({"book": "note", "as": "  ", "entries": {"title": "Nameless"}}),
        json!({"book": "note", "entries": {"title": "T", "tags": [{"as": "a"}]}}),
    );
    let path = put(&project, &text);
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert_eq!(
        answer["problems"],
        json!([
            "row 2: the entry `title` is given twice",
            "row 4: the name of the document is empty",
            "row 5: `tags` is no link, so it cannot name a row",
        ])
    );
    let err = stderr(&output);
    for row in ["row 2: ", "row 4: ", "row 5: "] {
        assert!(err.contains(row), "{err}");
    }
    assert_eq!(project.files(), before);
}

#[test]
fn a_line_with_no_entries_is_read_and_judged_by_its_book() {
    let project = project("many-no-entries");
    // `entries` is not required, so the line is read; the book wants a title, which is
    // the line's fault and not a line that is no object of the shape.
    let path = put(&project, "{\"book\": \"note\", \"key\": \"N-7\"}\n");
    let before = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let problems = json(&output)["problems"].clone();
    let text = problems[0].as_str().unwrap();
    assert!(text.starts_with("row 1: "), "{problems}");
    assert!(!text.contains("missing field"), "{problems}");
    assert_eq!(project.files(), before);
}

#[test]
fn a_file_that_cannot_be_read_is_a_failed_run_and_one_with_no_line_a_request_error() {
    let project = project("many-no-file");
    let before = project.files();
    // Not there.
    let output = project.run(&["new", "--from", "no-such-file.jsonl"]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["error"], "failed");
    assert!(
        answer["messages"][0]
            .as_str()
            .unwrap()
            .starts_with("--from no-such-file.jsonl: cannot read it"),
        "{answer}"
    );
    assert!(stderr(&output).contains("--from no-such-file.jsonl"));
    // Not text.
    let bytes = project.0.join("bytes.jsonl");
    fs::write(&bytes, b"{\"book\": \"note\"}\n\xff\xfe\n").unwrap();
    let output = project.run(&["new", "--from", &bytes.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(json(&output)["error"], "failed");
    // Not a file.
    let output = project.run(&["new", "--from", &project.0.join("notes").to_string_lossy()]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(json(&output)["error"], "failed");
    fs::remove_file(&bytes).unwrap();
    assert_eq!(project.files(), before);
    // A file with no line to write is a request that is wrong.
    let path = put(&project, "\n  \n");
    let before_blank = project.files();
    let output = project.run(&["new", "--from", &path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(json(&output)["error"], "request");
    assert_eq!(project.files(), before_blank);
    assert!(before.len() < before_blank.len());
}

#[test]
fn from_is_not_given_with_a_book_or_a_book_with_nothing() {
    let project = project("many-conflicts");
    let path = put(&project, &lines(&[note("T")]));
    let before = project.files();
    for args in [
        vec!["new", "note", "--from", &path],
        vec!["new", "--title", "x", "--from", &path],
        vec!["new", "--key", "N-9", "--from", &path],
        vec!["new", "--body", "x", "--from", &path],
        vec!["new", "--from", &path, "title=x"],
        // A book is needed when no file is read.
        vec!["new"],
        vec!["new", "--title", "x"],
    ] {
        let output = project.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    assert_eq!(project.files(), before);
}

#[test]
fn schema_has_the_answer_and_the_shape_of_a_line() {
    let project = project("many-schema");
    let output = project.run(&["schema"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    let out = &answer["outputs"]["new --from"];
    assert_eq!(out["properties"]["version"]["const"], 1);
    assert!(out["properties"]["docs"].is_object(), "{out}");
    assert!(out["properties"]["heads"].is_object(), "{out}");
    let line = &answer["inputs"]["new --from"];
    for property in ["book", "key", "as", "entries", "body"] {
        assert!(
            line["properties"][property].is_object(),
            "{property}: {line}"
        );
    }
    assert_eq!(line["additionalProperties"], false);
    // Only the book is needed: a line with no entries has none.
    assert_eq!(line["required"], json!(["book"]));
    // An entry may be `null`: no value, as `key:` is in a document.
    let kinds: Vec<&Value> = line["$defs"]["EntryIn"]["anyOf"]
        .as_array()
        .expect("the shapes of an entry")
        .iter()
        .map(|shape| &shape["type"])
        .collect();
    assert!(kinds.contains(&&json!("null")), "{kinds:?}");
    // The answers `new` and `init` give are told as before.
    assert!(answer["outputs"]["new"].is_object());
    assert!(answer["outputs"]["init"].is_object());
}

#[test]
fn new_of_one_document_is_answered_as_it_was() {
    let project = project("many-new-one");
    let output = project.run(&["new", "note", "--title", "One"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["doc"]["key"], "note:N-2");
    assert_eq!(answer["file"], "notes/N-2.md");
    assert!(answer.get("docs").is_none());
    let output = project.run(&["new", "note", "--title", "Two", "--key", "N-1"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json(&output)["messages"],
        json!([
            "the key cannot be the new document's",
            "the key `note:N-1` is taken"
        ])
    );
}
