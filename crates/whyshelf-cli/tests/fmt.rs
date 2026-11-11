//! `fmt`, on the binary and a small project of files: it writes an approved file in
//! the form and leaves its approval fresh, a CRLF file and a body with trailing white
//! space included, and leaves alone a file with a comment; `fmt --check` exits 1
//! while a file is not in the form, and creates and changes no file.

mod common;

use std::fs;

use common::{BOOKS, Project, json};

/// A note written by hand: keys out of order, nothing quoted, spaces at the ends of
/// the body's lines.
const UNQUOTED: &str =
    "---\nkey: N-1\nid: 01a1c4f0-0000-7000-8000-000000000001\ntitle: A note\n---\nThe body.  \n";

#[test]
fn fmt_writes_an_approved_file_in_the_form_and_its_approval_holds() {
    let project = Project::with("fmt", BOOKS, UNQUOTED);
    let commented =
        "---\nid: 01a1c4f0-0000-7000-8000-000000000002\nkey: N-2\ntitle: B # why\n---\n";
    fs::write(project.0.join("notes/N-2.md"), commented).unwrap();
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );

    // Before fmt: --check exits 1, lists the file, and writes nothing.
    let before = project.files();
    let check = project.run(&["fmt", "--check"]);
    assert_eq!(check.status.code(), Some(1));
    let answer = json(&check);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["not_in_form"], serde_json::json!(["notes/N-1.md"]));
    assert_eq!(answer["rewritten"], serde_json::json!([]));
    assert_eq!(answer["in_form"], 0);
    assert_eq!(answer["left_alone"][0]["file"], "notes/N-2.md");
    assert_eq!(answer["left_alone"][0]["reason"], "comment");
    assert_eq!(answer["left_alone"][0]["line"], 4);
    assert_eq!(project.files(), before);

    let output = project.run(&["fmt"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["rewritten"], serde_json::json!(["notes/N-1.md"]));
    assert_eq!(answer["not_in_form"], serde_json::Value::Null);
    assert_eq!(answer["left_alone"][0]["reason"], "comment");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .starts_with("warning: notes/N-2.md is left as it is: "),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.0.join("notes/N-1.md")).unwrap(),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n"
    );
    assert_eq!(
        fs::read_to_string(project.0.join("notes/N-2.md")).unwrap(),
        commented
    );

    // The approval still holds, and the file written is in the form.
    let stale = project.run(&["stale", "--exit-code"]);
    assert_eq!(stale.status.code(), Some(0));
    assert_eq!(json(&stale)["stale"], serde_json::json!([]));
    // The file left alone is not in the form either: --check still fails on it.
    let check = project.run(&["fmt", "--check"]);
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(json(&check)["not_in_form"], serde_json::json!([]));
    assert_eq!(json(&check)["left_alone"][0]["file"], "notes/N-2.md");
    assert_eq!(json(&check)["in_form"], 1);
    // Once its comment is gone, fmt writes it, and every file is in the form.
    fs::write(
        project.0.join("notes/N-2.md"),
        commented.replace(" # why", ""),
    )
    .unwrap();
    assert_eq!(project.run(&["fmt"]).status.code(), Some(0));
    let check = project.run(&["fmt", "--check"]);
    assert_eq!(check.status.code(), Some(0));
    assert_eq!(json(&check)["in_form"], 2);
    assert_eq!(json(&check)["left_alone"], serde_json::json!([]));
}

/// The hash of the document now and as approved, from `diff`.
fn doc_hashes(project: &Project, key: &str) -> (serde_json::Value, serde_json::Value) {
    let diff = json(&project.run(&["diff", key]));
    let item = diff["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["item"]["role"] == "subject")
        .unwrap_or_else(|| panic!("a doc item: {diff}"))
        .clone();
    (
        item["item"]["approved_hash"].clone(),
        item["current_hash"].clone(),
    )
}

#[test]
fn fmt_writes_a_crlf_file_and_a_body_with_trailing_white_space_normalized() {
    let in_form = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n\nMore.\n";
    for (name, text) in [
        ("fmt-crlf", in_form.replace('\n', "\r\n")),
        (
            "fmt-trailing",
            in_form.replace("The body.\n\nMore.\n", "The body.  \n\t\nMore.\t\n\n\n"),
        ),
    ] {
        let project = Project::with(name, BOOKS, &text);
        assert_eq!(
            project.run(&["judge", "approve", "N-1"]).status.code(),
            Some(0)
        );
        let (approved, now) = doc_hashes(&project, "N-1");
        assert_eq!(approved, now, "{name}");

        // A reading command: on a file not in the form, it creates and changes none.
        let before = project.files();
        assert_eq!(project.run(&["fmt", "--check"]).status.code(), Some(1));
        assert_eq!(project.files(), before, "{name}");
        let output = project.run(&["fmt"]);
        assert_eq!(output.status.code(), Some(0), "{name}");
        assert_eq!(
            json(&output)["rewritten"],
            serde_json::json!(["notes/N-1.md"])
        );
        assert_eq!(
            fs::read_to_string(project.0.join("notes/N-1.md")).unwrap(),
            in_form,
            "{name}"
        );

        // The hash is the one approved, and a second check finds the file in the form.
        assert_eq!(doc_hashes(&project, "N-1"), (approved.clone(), approved));
        let stale = project.run(&["stale", "--exit-code"]);
        assert_eq!(stale.status.code(), Some(0), "{name}");
        let check = project.run(&["fmt", "--check"]);
        assert_eq!(check.status.code(), Some(0), "{name}");
        assert_eq!(json(&check)["in_form"], 1);
    }
}
