//! A file that merge conflict markers left both sides in, on the binary and a small
//! project of files: `check` names it as a file that holds conflict markers (not as a
//! YAML error), and a document it may hold is not told to be gone or invalid by the
//! side that happens to open the file. Where the two sides show different identities
//! the file is one whose identity cannot be told, so `status` says so of the document
//! of either side; where they show the same one, the file is that document's, and
//! invalid.
//!
//! The last two tests hold what is left as it is: what a file whose identity cannot be
//! told does to the answers about other documents is the shelf's rule for such a file,
//! and a conflict is one more way into it.

mod common;

use std::fs;

use common::{Project, json};
use serde_json::{Value, json as value};

const A: &str = "01a1c4f0-0000-7000-8000-000000000001";
const B: &str = "01a1c4f0-0000-7000-8000-000000000002";
const C: &str = "01a1c4f0-0000-7000-8000-000000000003";

/// A note, in the one form.
fn note(id: &str, key: &str, title: &str) -> String {
    format!("---\nid: \"{id}\"\nkey: \"{key}\"\ntitle: \"{title}\"\n---\nThe {title}.\n")
}

/// What Git leaves where two branches each added a note at one path: the line `---`
/// both begin with outside the conflict, then each side's front matter and body.
fn both_sides(ours: &str, theirs: &str) -> String {
    format!(
        "---\n<<<<<<< ours\nid: \"{ours}\"\nkey: \"N-1\"\ntitle: \"Ours\"\n---\nOurs.\n=======\nid: \"{theirs}\"\nkey: \"N-2\"\ntitle: \"Theirs\"\n---\nTheirs.\n>>>>>>> theirs\n"
    )
}

/// What Git leaves where two branches changed one note's title.
fn one_note_in_conflict(id: &str) -> String {
    format!(
        "---\nid: \"{id}\"\nkey: \"N-1\"\n<<<<<<< ours\ntitle: \"Ours\"\n=======\ntitle: \"Theirs\"\n>>>>>>> theirs\n---\nThe body.\n"
    )
}

fn put(project: &Project, file: &str, text: &str) {
    fs::write(project.0.join(file), text).unwrap();
}

fn approve(project: &Project, key: &str) {
    let output = project.run(&["judge", "approve", key]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{key}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The row of `status` of the document of this identity.
fn row(status: &Value, id: &str) -> Value {
    status["docs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["doc"]["id"] == id)
        .unwrap_or_else(|| panic!("no row of {id}: {status}"))
        .clone()
}

/// Three notes, `A` (`N-1`), `B` (`N-2`) and `C` (`N-3`), each approved. Then the
/// branches' merge leaves `N-1.md` with both sides of an add (`A` and `B`) in it, and
/// the files of `B` and `C` gone.
fn merged(name: &str) -> Project {
    let project = Project::new(name);
    put(&project, "notes/N-2.md", &note(B, "N-2", "Theirs"));
    put(&project, "notes/N-3.md", &note(C, "N-3", "Unrelated"));
    for key in ["N-1", "N-2", "N-3"] {
        approve(&project, key);
    }
    put(&project, "notes/N-1.md", &both_sides(A, B));
    fs::remove_file(project.0.join("notes/N-2.md")).unwrap();
    fs::remove_file(project.0.join("notes/N-3.md")).unwrap();
    project
}

fn status(project: &Project) -> Value {
    let output = project.run(&["status"]);
    assert_eq!(output.status.code(), Some(0));
    json(&output)
}

#[test]
fn the_documents_of_both_sides_of_a_file_are_neither_gone_nor_invalid_but_unknown() {
    let project = merged("conflict-both-sides");
    let answer = status(&project);
    for id in [A, B] {
        assert_eq!(
            row(&answer, id)["presence"],
            value!({"state": "unknown", "unreadable": ["notes/N-1.md"]}),
            "{id}"
        );
    }
}

#[test]
fn a_document_whose_file_is_gone_is_unknown_while_a_conflict_is_there() {
    let project = merged("conflict-unrelated");
    let answer = status(&project);
    assert_eq!(
        row(&answer, C)["presence"],
        value!({"state": "unknown", "unreadable": ["notes/N-1.md"]})
    );
}

#[test]
fn check_names_the_file_as_one_that_holds_conflict_markers_and_gives_it_no_identity() {
    let project = merged("conflict-check");
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let answer = json(&output);
    assert_eq!(answer["clean"], false);
    let invalid = answer["health"]["invalid"].as_array().unwrap();
    let [file] = invalid.as_slice() else {
        panic!("one file: {answer}");
    };
    assert_eq!(file["file"], "notes/N-1.md");
    assert_eq!(file["id"], Value::Null);
    let [problem] = file["problems"].as_array().unwrap().as_slice() else {
        panic!("one problem: {file}");
    };
    let problem = problem.as_str().unwrap();
    assert!(problem.contains("merge conflict markers"), "{problem}");
    assert!(!problem.contains("YAML"), "{problem}");
}

#[test]
fn two_sides_of_one_identity_are_that_documents_file_and_invalid_for_its_conflict() {
    let project = Project::new("conflict-one-note");
    approve(&project, "N-1");
    put(&project, "notes/N-1.md", &one_note_in_conflict(A));
    let answer = status(&project);
    let presence = &row(&answer, A)["presence"];
    assert_eq!(presence["state"], "invalid", "{presence}");
    assert_eq!(presence["files"], value!(["notes/N-1.md"]));
    let problems = presence["problems"].as_array().unwrap();
    assert!(
        problems
            .iter()
            .all(|problem| problem.as_str().unwrap().contains("merge conflict markers")),
        "{problems:?}"
    );
    assert!(!problems.is_empty());
}

/// What is left as it is: while a file holds both sides of two documents, a document
/// no record is kept of is looked up as absent, though a file that cannot be told may
/// hold it. The rule is the shelf's for any file whose identity cannot be told.
#[test]
fn a_document_with_no_record_is_looked_up_as_absent_while_a_conflict_may_hold_it() {
    let project = Project::new("conflict-lookup");
    put(&project, "notes/N-1.md", &both_sides(A, B));
    let output = project.run(&["get", A]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(2), "{answer}");
    assert_eq!(
        answer["messages"],
        value!([format!("`{A}` names no document")])
    );
}

/// What is left as it is: another file that writes the identity of one side is valid
/// while the file that holds the conflict is there, and is not told to repeat it.
#[test]
fn another_file_of_the_identity_of_a_side_stays_valid_beside_the_conflict() {
    let project = Project::new("conflict-duplicate");
    put(&project, "notes/N-1.md", &both_sides(A, B));
    put(&project, "notes/N-3.md", &note(A, "N-3", "Another"));
    let answer = status(&project);
    let presence = &row(&answer, A)["presence"];
    assert_eq!(presence["state"], "valid", "{presence}");
    assert_eq!(presence["file"], "notes/N-3.md");
    let invalid = answer["health"]["invalid"].as_array().unwrap();
    let files: Vec<&str> = invalid
        .iter()
        .map(|file| file["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["notes/N-1.md"]);
}
