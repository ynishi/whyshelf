//! `judge approve --stale`, on the binary and a small project of files: it approves
//! again what an edit made stale, upstream first, and leaves `stale` empty; it leaves
//! out a broken approval and one it cannot approve, each with a warning line on
//! stderr, and writes the rest; it tells a name it pinned with nothing there; and it
//! takes no document and needs a reason.

mod common;

use std::fs;

use common::{BOOKS, Project, json};

/// Notes that stand on other notes (`on`) and on files (`files`).
fn linked_books() -> String {
    format!(
        "{BOOKS}[[book.link]]\nname = \"on\"\ntarget = \"note\"\nmeans = \"depends\"\n\
         [[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n\
         [[book.link]]\nname = \"replaces\"\ntarget = \"note\"\nmeans = \"replaces\"\n"
    )
}

/// A note `N-<n>` with these entries after its title.
fn note(n: u32, entries: &str) -> String {
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-00000000000{n}\"\nkey: \"N-{n}\"\ntitle: \"Note {n}\"\n{entries}---\nThe body.\n"
    )
}

fn keys_of(list: &serde_json::Value, at: &str) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|e| e[at]["key"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn approve_stale_approves_again_what_an_edit_made_stale_upstream_first() {
    // N-1 stands on N-2, so N-2 is upstream though its key comes after.
    let project = Project::with("approve-stale", &linked_books(), &note(1, "on: [N-2]\n"));
    fs::write(project.0.join("notes/N-2.md"), note(2, "")).unwrap();
    let nothing = project.run(&["judge", "approve", "--stale", "--reason", "x"]);
    assert_eq!(nothing.status.code(), Some(0));
    assert_eq!(json(&nothing)["judged"], serde_json::json!([]));
    assert_eq!(
        project
            .run(&["judge", "approve", "N-2", "N-1"])
            .status
            .code(),
        Some(0)
    );
    fs::write(
        project.0.join("notes/N-2.md"),
        note(2, "").replace("The body.", "An edited body."),
    )
    .unwrap();
    assert_eq!(
        json(&project.run(&["stale"]))["stale"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let output = project.run(&["judge", "approve", "--stale", "--reason", "x"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(keys_of(&answer["judged"], "doc"), ["note:N-2", "note:N-1"]);
    assert_eq!(answer["judged"][0]["record"]["reason"], "x");
    assert_eq!(answer["left_out"], serde_json::json!([]));
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(
        json(&project.run(&["stale"]))["stale"],
        serde_json::json!([])
    );
}

#[test]
fn approve_stale_leaves_out_a_broken_approval_and_warns_of_it() {
    let project = Project::with("approve-broken", &linked_books(), &note(1, ""));
    fs::write(project.0.join("notes/N-3.md"), note(3, "files: [a.txt]\n")).unwrap();
    fs::write(project.0.join("a.txt"), "evidence\n").unwrap();
    assert_eq!(
        project
            .run(&["judge", "approve", "N-1", "N-3"])
            .status
            .code(),
        Some(0)
    );
    fs::remove_file(project.0.join("a.txt")).unwrap();
    fs::write(
        project.0.join("notes/N-1.md"),
        note(1, "").replace("The body.", "An edited body."),
    )
    .unwrap();

    let output = project.run(&["judge", "approve", "--stale", "--reason", "x"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(keys_of(&answer["judged"], "doc"), ["note:N-1"]);
    assert_eq!(keys_of(&answer["left_out"], "doc"), ["note:N-3"]);
    assert_eq!(
        answer["left_out"][0]["kinds"],
        serde_json::json!(["artifact_gone"])
    );
    assert_eq!(answer["left_out"][0]["reason"], "broken");
    assert_eq!(
        answer["left_out"][0]["detail"],
        "its approval is broken (artifact_gone)"
    );
    assert_eq!(answer["left_out"][0]["causes"][0]["severity"], "broken");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "warning: note:N-3 is broken and was not approved again (artifact_gone)\n"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("warning:"));
    // The broken approval still does not hold: it was never approved in silence.
    let stale = json(&project.run(&["stale"]));
    assert_eq!(stale["stale"].as_array().map(Vec::len), Some(1));
    assert_eq!(stale["stale"][0]["row"]["doc"]["key"], "note:N-3");
}

#[test]
fn approve_stale_leaves_out_one_it_cannot_approve_and_writes_the_rest() {
    // N-2 stands on N-1, which is deprecated for N-3; N-4 is only edited.
    let project = Project::with("approve-refused", &linked_books(), &note(1, ""));
    fs::write(project.0.join("notes/N-2.md"), note(2, "on: [N-1]\n")).unwrap();
    fs::write(project.0.join("notes/N-3.md"), note(3, "replaces: [N-1]\n")).unwrap();
    fs::write(project.0.join("notes/N-4.md"), note(4, "")).unwrap();
    assert_eq!(
        project
            .run(&["judge", "approve", "N-1", "N-2", "N-4"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        project.run(&["judge", "deprecate", "N-1"]).status.code(),
        Some(0)
    );
    fs::write(
        project.0.join("notes/N-4.md"),
        note(4, "").replace("The body.", "An edited body."),
    )
    .unwrap();

    let output = project.run(&["judge", "approve", "--stale", "--reason", "x"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(keys_of(&answer["judged"], "doc"), ["note:N-4"]);
    assert_eq!(keys_of(&answer["left_out"], "doc"), ["note:N-2"]);
    assert_eq!(answer["left_out"][0]["reason"], "refused");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "warning: note:N-2 was not approved again: the link `on` names note:N-1, which is deprecated: an approval cannot stand on it\n"
    );
    let stale = json(&project.run(&["stale"]));
    assert_eq!(stale["stale"].as_array().map(Vec::len), Some(1));
    assert_eq!(stale["stale"][0]["row"]["doc"]["key"], "note:N-2");
}

#[test]
fn approve_stale_tells_a_name_it_pinned_with_nothing_there() {
    let project = Project::with("approve-absent", &linked_books(), &note(1, ""));
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    fs::write(
        project.0.join("notes/N-1.md"),
        note(1, "files: [later.txt]\n"),
    )
    .unwrap();

    let output = project.run(&["judge", "approve", "--stale", "--reason", "x"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["judged"][0]["absent"][0]["name"], "later.txt");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "warning: note:N-1 pins later.txt in file, which has nothing behind it\n"
    );
}

#[test]
fn approve_stale_takes_no_document_and_needs_a_reason() {
    let project = Project::new("approve-stale-args");
    for args in [
        &["judge", "approve", "--stale", "--reason", "x", "N-1"][..],
        &["judge", "approve", "--stale"],
    ] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(json(&output)["error"], "request", "{args:?}");
    }
    assert!(!project.0.join(".whyshelf/ledger").exists());
}
