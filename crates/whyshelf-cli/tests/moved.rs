//! A pinned file moved, on the binary and a small project of files: `judge moved`
//! declares it, and the approval stays fresh; moved with an edit, it is stale with
//! `artifact_changed` and not broken; a link left on the old name is stale and
//! `check` warns of it; `get --approved` shows the name pinned and the name it is
//! read as.

mod common;

use std::fs;

use common::{BOOKS, NOTE, Project, json};

/// A project whose note links the files `files` (a `depends` link of the space
/// `file`), each holding `assert!(fast);`, approved.
fn approved(name: &str, files: &[&str]) -> Project {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n"
    );
    let project = Project::with(name, &books, &note_on(files));
    for file in files {
        fs::write(project.0.join(file), "assert!(fast);\n").unwrap();
    }
    let out = project.run(&["judge", "approve", "N-1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    project
}

/// The note, its `files` link writing `files`.
fn note_on(files: &[&str]) -> String {
    let quoted: Vec<String> = files.iter().map(|f| format!("\"{f}\"")).collect();
    NOTE.replace(
        "title: \"A note\"\n",
        &format!("title: \"A note\"\nfiles: [{}]\n", quoted.join(", ")),
    )
}

/// The note's link rewritten to `files`: the note's hash does not cover its links.
fn relink(project: &Project, files: &[&str]) {
    fs::write(project.0.join("notes/N-1.md"), note_on(files)).unwrap();
}

fn standing(project: &Project) -> serde_json::Value {
    json(&project.run(&["get", "N-1"]))["row"]["standing"].clone()
}

fn cause_kinds(project: &Project) -> Vec<String> {
    json(&project.run(&["get", "N-1"]))["row"]["causes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap().to_owned())
        .collect()
}

fn declare(project: &Project, from: &str, to: &str) -> std::process::Output {
    project.run(&[
        "judge",
        "moved",
        from,
        to,
        "--reason",
        "the tests were split by module",
    ])
}

#[test]
fn a_moved_file_declared_keeps_its_approval_and_an_edit_is_a_change() {
    let project = approved("moved", &["a.rs"]);
    fs::rename(project.0.join("a.rs"), project.0.join("b.rs")).unwrap();
    relink(&project, &["b.rs"]);
    assert_eq!(standing(&project), "stale");

    let out = declare(&project, "a.rs", "b.rs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let declared = json(&out);
    assert_eq!(declared["moved"][0]["space"], "file");
    assert_eq!(declared["moved"][0]["from"], "a.rs");
    assert_eq!(declared["moved"][0]["to"], "b.rs");
    let record = &declared["moved"][0]["record"];
    assert_eq!(record["kind"], "moved");
    assert_eq!(record["actor"], "tester");
    assert_eq!(record["reason"], "the tests were split by module");
    assert!(record["id"].is_string());

    assert_eq!(standing(&project), "fresh");
    let stale = project.run(&["stale", "--exit-code"]);
    assert!(
        stale.status.success(),
        "{}",
        String::from_utf8_lossy(&stale.stdout)
    );

    // What the approval saw: the name pinned, and the name it is read as.
    let approved = json(&project.run(&["get", "N-1", "--approved"]));
    let item = approved["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["item"]["role"] == "artifact")
        .unwrap()
        .clone();
    assert_eq!(item["item"]["name"], "a.rs");
    assert_eq!(item["item"]["read_as"], "b.rs");
    let diff = json(&project.run(&["diff", "N-1"]));
    let item = diff["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["item"]["role"] == "artifact")
        .unwrap()
        .clone();
    assert_eq!(item["changed"], false);
    assert_eq!(item["now"], "present");
    assert_eq!(item["file"], "b.rs");

    // Edited after the move: stale, with the change at the new name, not broken.
    fs::write(project.0.join("b.rs"), "assert!(faster);\n").unwrap();
    assert_eq!(standing(&project), "stale");
    let got = json(&project.run(&["get", "N-1"]));
    let causes = got["row"]["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 1, "{causes:?}");
    assert_eq!(causes[0]["kind"], "artifact_changed");
    assert_eq!(causes[0]["artifact"], "a.rs");
    assert_eq!(causes[0]["read_as"], "b.rs");
    let stale = json(&project.run(&["stale"]));
    assert_eq!(stale["origins"][0]["start"]["name"], "b.rs");
}

#[test]
fn a_file_moved_and_edited_before_the_declaration_is_stale_not_broken() {
    let project = approved("moved-edited", &["a.rs"]);
    fs::remove_file(project.0.join("a.rs")).unwrap();
    fs::write(project.0.join("b.rs"), "assert!(faster);\n").unwrap();
    let out = declare(&project, "a.rs", "b.rs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    relink(&project, &["b.rs"]);
    assert_eq!(standing(&project), "stale");
    assert_eq!(cause_kinds(&project), ["artifact_changed"]);
}

#[test]
fn a_link_left_on_the_old_name_is_stale_and_check_warns() {
    let project = approved("moved-link", &["a.rs"]);
    fs::rename(project.0.join("a.rs"), project.0.join("b.rs")).unwrap();
    // Not declared: the file is gone.
    assert_eq!(standing(&project), "broken");
    assert_eq!(cause_kinds(&project), ["artifact_gone"]);

    let out = declare(&project, "a.rs", "b.rs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(standing(&project), "stale");
    // One cause, and no change of the link claimed.
    assert_eq!(cause_kinds(&project), ["artifact_moved"]);
    let got = json(&project.run(&["get", "N-1"]));
    let cause = &got["row"]["causes"][0];
    assert_eq!(cause["severity"], "stale");
    assert_eq!(cause["link"], "files");
    assert_eq!(cause["artifact"], "a.rs");
    assert_eq!(cause["read_as"], "b.rs");

    let check = project.run(&["check"]);
    assert!(
        check.status.success(),
        "a moved name leaves the check clean"
    );
    let answer = json(&check);
    assert_eq!(answer["moved"][0]["name"], "a.rs");
    assert_eq!(answer["moved"][0]["moved_to"], "b.rs");
    assert_eq!(answer["moved"][0]["link"], "files");
    assert!(
        answer["moved"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("the pin is read as `b.rs` now")
    );
    assert_eq!(answer["moved"][0]["declared"].as_array().unwrap().len(), 1);
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("names a.rs in file under `files`, which was declared moved to b.rs"),
        "{stderr}"
    );

    // Written as the new name, it holds again.
    relink(&project, &["b.rs"]);
    assert_eq!(standing(&project), "fresh");
    assert_eq!(
        json(&project.run(&["check"]))["moved"],
        serde_json::json!([])
    );
}

#[test]
fn a_copy_or_a_move_to_nothing_is_refused_and_writes_nothing() {
    let project = approved("moved-refused", &["a.rs"]);
    let before = project.files();
    fs::write(project.0.join("b.rs"), "assert!(fast);\n").unwrap();
    let out = declare(&project, "a.rs", "b.rs");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["error"], "refused");
    fs::remove_file(project.0.join("b.rs")).unwrap();

    fs::remove_file(project.0.join("a.rs")).unwrap();
    let out = declare(&project, "a.rs", "b.rs");
    assert_eq!(out.status.code(), Some(1));

    let out = declare(&project, "x.rs", "y.rs");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(json(&out)["error"], "request");

    let mut after = project.files();
    after.remove(&project.0.join("a.rs"));
    let mut before = before;
    before.remove(&project.0.join("a.rs"));
    assert_eq!(after, before, "a refused move writes no record");
}
