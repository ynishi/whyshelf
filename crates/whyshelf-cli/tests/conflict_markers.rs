//! A file that merge conflict markers left both sides in, on the binary and a small
//! project of files: `check` names it as a file that holds conflict markers (not as a
//! YAML error), and a document it may hold is not told to be gone or invalid by the
//! side that happens to open the file. Where the two sides show different identities
//! the file holds the document of one of them: `status` tells each as one that cannot
//! be told, naming the file as one whose side shows it, and every other document as
//! it would be without the file; where they show the same one, the file is that
//! document's, and invalid.

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
    status_of(project, &[])
}

fn status_of(project: &Project, args: &[&str]) -> Value {
    let mut all = vec!["status"];
    all.extend_from_slice(args);
    let output = project.run(&all);
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
            value!({"state": "unknown", "shown": ["notes/N-1.md"], "unreadable": []}),
            "{id}"
        );
    }
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

/// A document whose file is gone and that no side of the conflict shows is gone, as it
/// would be without the file.
#[test]
fn a_document_no_side_shows_whose_file_is_gone_is_gone_while_a_conflict_is_there() {
    let project = merged("conflict-unrelated");
    let answer = status(&project);
    assert_eq!(row(&answer, C)["presence"], value!({"state": "gone"}));
}

#[test]
fn check_tells_the_identities_the_sides_of_the_file_show() {
    let project = merged("conflict-sides");
    let answer = json(&project.run(&["check"]));
    let invalid = answer["health"]["invalid"].as_array().unwrap();
    let [file] = invalid.as_slice() else {
        panic!("one file: {answer}");
    };
    assert_eq!(file["id"], Value::Null);
    assert_eq!(file["sides"], value!([A, B]));
    // A file of one identity has no sides.
    let project = Project::new("conflict-no-sides");
    put(&project, "notes/N-1.md", &one_note_in_conflict(A));
    let answer = json(&project.run(&["check"]));
    assert_eq!(answer["health"]["invalid"][0]["sides"], value!([]));
}

/// A document of a side that has no record is found by its identifier, and is told as
/// one that cannot be told, naming the file.
#[test]
fn a_document_of_a_side_with_no_record_is_found_by_its_id_and_told_unknown_in_the_file() {
    let project = Project::new("conflict-lookup");
    put(&project, "notes/N-1.md", &both_sides(A, B));
    let output = project.run(&["get", A]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["row"]["doc"]["id"], A);
    assert_eq!(
        answer["row"]["presence"],
        value!({"state": "unknown", "shown": ["notes/N-1.md"], "unreadable": []})
    );
    // An identifier no side shows names nothing.
    let output = project.run(&["get", C]);
    assert_eq!(output.status.code(), Some(2), "{}", json(&output));
}

/// Another file that writes the identity of one side, and the file that holds the
/// conflict, are both invalid, each naming the other.
#[test]
fn another_file_of_the_identity_of_a_side_and_the_file_of_the_conflict_are_both_invalid() {
    let project = Project::new("conflict-duplicate");
    put(&project, "notes/N-1.md", &both_sides(A, B));
    put(&project, "notes/N-3.md", &note(A, "N-3", "Another"));
    let answer = status(&project);
    let presence = &row(&answer, A)["presence"];
    assert_eq!(presence["state"], "invalid", "{presence}");
    assert_eq!(presence["files"], value!(["notes/N-3.md"]));
    let invalid = answer["health"]["invalid"].as_array().unwrap();
    let mut files: Vec<&str> = invalid
        .iter()
        .map(|file| file["file"].as_str().unwrap())
        .collect();
    files.sort_unstable();
    assert_eq!(files, ["notes/N-1.md", "notes/N-3.md"]);
    for (file, other) in [
        ("notes/N-1.md", "notes/N-3.md"),
        ("notes/N-3.md", "notes/N-1.md"),
    ] {
        let problems = &invalid.iter().find(|found| found["file"] == file).unwrap()["problems"];
        let said = format!(
            "the id {A} may be carried twice, by this file and {other}: one of them holds both sides of a merge conflict"
        );
        assert!(
            problems.as_array().unwrap().iter().any(|p| p == &said),
            "{file}: {problems}"
        );
    }
}

/// `status --book` lists a document of a side that has a record under the book of the
/// file, and leaves out one that is gone.
#[test]
fn status_of_a_book_lists_the_documents_of_both_sides_with_their_records() {
    let project = merged("conflict-book");
    let answer = status_of(&project, &["--book", "note"]);
    let mut ids: Vec<&str> = answer["docs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["doc"]["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [A, B]);
}

/// `impact --within` does not pass while a file of the end after is left in a merge
/// conflict, which it names apart from the files whose identity cannot be read; a
/// document gone before that a side shows after cannot be told, with the file.
#[test]
fn impact_within_does_not_pass_while_a_file_is_left_in_a_conflict() {
    // Before: A (`N-1`) and C (`N-3`), approved, and B, denied (so no approval of it
    // has a cause the change could add or take away) and then removed.
    let before = Project::new("conflict-within-before");
    put(&before, "notes/N-2.md", &note(B, "N-2", "Theirs"));
    put(&before, "notes/N-3.md", &note(C, "N-3", "Unrelated"));
    for key in ["N-1", "N-3"] {
        approve(&before, key);
    }
    let denied = before.run(&["judge", "deny", "N-2"]);
    assert_eq!(denied.status.code(), Some(0), "{denied:?}");
    fs::remove_file(before.0.join("notes/N-2.md")).unwrap();
    // After: the same records, and `N-1.md` holds both sides of A and B.
    let after = Project::new("conflict-within-after");
    fs::create_dir_all(after.0.join(".whyshelf/ledger")).unwrap();
    for entry in fs::read_dir(before.0.join(".whyshelf/ledger")).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(
            &path,
            after
                .0
                .join(".whyshelf/ledger")
                .join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    put(&after, "notes/N-3.md", &note(C, "N-3", "Unrelated"));
    put(&after, "notes/N-1.md", &both_sides(A, B));
    let output = after.run(&[
        "impact",
        "--before",
        before.0.to_str().unwrap(),
        "--within",
        "N-3",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(1), "{answer}");
    let within = &answer["within"];
    assert_eq!(within["passes"], false);
    assert_eq!(
        within["unreadable"]["sided_after"],
        value!(["notes/N-1.md"])
    );
    assert_eq!(within["unreadable"]["sided_before"], value!([]));
    assert_eq!(within["unreadable"]["files_after"], value!([]));
    let b = within["docs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|doc| doc["doc"]["id"] == B)
        .unwrap_or_else(|| panic!("a verdict of B: {answer}"));
    assert_eq!(
        b["verdict"]["untold"]["shelf"],
        value!({
            "before": {"shown": [], "unreadable": []},
            "after": {"shown": ["notes/N-1.md"], "unreadable": []},
        }),
        "{b}"
    );
}
