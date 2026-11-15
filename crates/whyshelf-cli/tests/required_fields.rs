//! A required field that says nothing, on the binary and a project made by `init`
//! (its `note` book requires `title`): `new note --title ""` exits 2 and writes no
//! file, and its problems say `title` is empty; a note written by hand with
//! `title: ""` is invalid for `check` (exit 1), which says so too.

mod common;

use std::fs;

use common::{Project, json};

/// A project made by `init`, removed when the test ends.
fn initialised(name: &str) -> Project {
    let project = Project::empty(&format!("required-{name}"));
    let output = project.run(&["init"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    project
}

#[test]
fn new_refuses_a_blank_required_title_and_writes_no_file() {
    let project = initialised("new");
    let before = project.files();
    let output = project.run(&["new", "note", "--title", ""]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer = json(&output);
    let problems = answer["problems"].to_string();
    assert!(
        problems.contains("the required field `title` is empty or blank"),
        "{answer}"
    );
    assert_eq!(project.files(), before);
}

#[test]
fn check_finds_a_note_written_with_an_empty_title_invalid() {
    let project = initialised("check");
    let note = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"\"\n---\nThe body.\n";
    fs::create_dir_all(project.0.join("books/note")).unwrap();
    fs::write(project.0.join("books/note/N-1.md"), note).unwrap();
    let output = project.run(&["check"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer = json(&output);
    let invalid = answer["health"]["invalid"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of invalid files: {answer}"));
    assert_eq!(invalid.len(), 1, "{answer}");
    assert_eq!(invalid[0]["file"], "books/note/N-1.md");
    let problems = invalid[0]["problems"].to_string();
    assert!(
        problems.contains("the required field `title` is empty or blank"),
        "{problems}"
    );
}
