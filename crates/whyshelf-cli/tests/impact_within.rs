//! `impact --within`, on the binary and one small project: two requirements, a
//! decision serving each on a file of its own, and a note, all approved. A change
//! that edits only the file of the other requirement's decision is outside the reach
//! of the one named, read through two directories and through Git, and `--exit-code`
//! exits 1 on it, as it does while a file of the books whose identity cannot be read
//! is at either end, or a file of the ledger that is no record; a decision the change
//! adds under the named requirement is unproven unless it is named too; an edit of a
//! pending document outside the reach, a link the change gives a document outside to
//! the named, and a book taken out of the declaration, are outside; `--exit-code` without `--within`,
//! and a reference that names no document, are request errors (exit 2), and
//! `--within` alone answers with exit 0.
//!
//! Each test writes its own project; the part that needs Git passes without it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{Project, json};
use serde_json::Value;

const NOTES: &str = r#"
[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "line"
required = true
"#;

const BOOKS: &str = r#"[defaults]
artifact = "file"

[[book]]
name = "requirement"
dir = "requirements"
key = { prefix = "R" }
[[book.field]]
name = "title"
type = "line"
required = true

[[book]]
name = "decision"
dir = "decisions"
key = { prefix = "D" }
[[book.field]]
name = "title"
type = "line"
required = true
[[book.link]]
name = "serves"
target = "requirement"
means = "depends"
[[book.link]]
name = "data"
target = "artifact"
preset = "file"
"#;

/// The document `<prefix>-<n>` of the book in `dir`, its identity told apart by
/// `book`, with these lines of entries and this body.
fn write_doc(project: &Path, dir: &str, prefix: &str, book: u8, n: u8, extra: &str, body: &str) {
    let text = format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000{book:03}{n:02}\"\nkey: \"{prefix}-{n}\"\ntitle: \"{prefix} {n}\"\n{extra}---\n{body}\n"
    );
    fs::write(project.join(format!("{dir}/{prefix}-{n}.md")), text).unwrap();
}

fn requirement(project: &Path, n: u8) {
    write_doc(project, "requirements", "R", 1, n, "", "The need.");
}

/// The decision `D-<n>`, serving `R-<serves>` and standing on the file `data`.
fn decision(project: &Path, n: u8, serves: u8, data: &str) {
    let extra = format!("serves: [\"R-{serves}\"]\ndata: [\"{data}\"]\n");
    write_doc(project, "decisions", "D", 2, n, &extra, "The rule.");
}

fn note(project: &Path, n: u8, body: &str) {
    write_doc(project, "notes", "N", 3, n, "", body);
}

/// The project: `D-1` serves `R-1` on `a.txt`, `D-2` serves `R-2` on `b.txt`, and the
/// note `N-1`; all approved.
fn project(name: &str) -> Project {
    let project = Project::empty(name);
    let dir = &project.0;
    for sub in [".whyshelf", "requirements", "decisions", "notes"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    fs::write(dir.join(".whyshelf/books.toml"), format!("{BOOKS}{NOTES}")).unwrap();
    fs::write(dir.join("a.txt"), "one\n").unwrap();
    fs::write(dir.join("b.txt"), "two\n").unwrap();
    requirement(dir, 1);
    requirement(dir, 2);
    decision(dir, 1, 1, "a.txt");
    decision(dir, 2, 2, "b.txt");
    note(dir, 1, "A note.");
    let approved = project.run(&["judge", "approve", "R-1", "R-2", "D-1", "D-2", "N-1"]);
    assert_eq!(approved.status.code(), Some(0), "{approved:?}");
    project
}

/// A copy of the project beside it, as the end before a change; removed when dropped.
struct Before(PathBuf);

impl Before {
    fn of(project: &Project) -> Self {
        let dir = project.0.with_file_name(format!(
            "{}-before",
            project.0.file_name().unwrap().to_string_lossy()
        ));
        copy(&project.0, &dir);
        Self(dir)
    }

    fn arg(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Before {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every file and directory under `from`, copied under `to`.
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if fs::read_dir(&path).is_ok() {
            copy(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// Whether `git` can be run here.
fn git_installed() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Git run on `dir` with no `GIT_*` variable of the caller.
fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let status = command
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "core.autocrlf=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

/// The gate's entry of the document `key` in an answer of `impact --within`.
fn verdict<'a>(answer: &'a Value, key: &str) -> &'a Value {
    answer["within"]["docs"]
        .as_array()
        .unwrap_or_else(|| panic!("a gate in {answer}"))
        .iter()
        .find(|doc| doc["doc"]["key"] == key)
        .unwrap_or_else(|| panic!("{key} in {answer}"))
}

/// The kind of a document's verdict: `not_claimed`, `inside`, and so on.
fn kind(doc: &Value) -> &Value {
    &doc["verdict"]["verdict"]
}

/// The gate's verdict of every document but those of `keys` is `not_claimed`.
fn only_claimed(answer: &Value, keys: &[&str]) {
    for doc in answer["within"]["docs"].as_array().unwrap() {
        let key = doc["doc"]["key"].as_str().unwrap();
        if !keys.contains(&key) {
            assert_eq!(kind(doc), "not_claimed", "{doc}");
        }
    }
}

/// `impact --before <before> --within <refs> --exit-code` on `project`: its exit and
/// its answer.
fn gate(project: &Project, before: &Before, refs: &[&str]) -> (Option<i32>, Value) {
    let mut args = vec!["impact", "--before", before.arg(), "--within"];
    args.extend_from_slice(refs);
    args.push("--exit-code");
    let output = project.run(&args);
    (output.status.code(), json(&output))
}

/// Done when 1: an edit of only the file another requirement's decision stands on is
/// outside the reach of the one named, through two directories and through Git; it is
/// inside the reach of that other requirement.
#[test]
fn an_edit_of_what_another_requirement_stands_on_is_outside_the_reach_named() {
    let project = project("within-outside");
    let before = Before::of(&project);
    fs::write(project.0.join("b.txt"), "two, edited\n").unwrap();
    let files = project.files();
    let output = project.run(&[
        "impact",
        "--before",
        before.arg(),
        "--within",
        "R-1",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(1), "{answer}");
    assert_eq!(project.files(), files, "nothing is written");
    let two = verdict(&answer, "decision:D-2");
    assert_eq!(kind(two), "outside", "{two}");
    assert_eq!(
        two["verdict"]["witnesses"],
        serde_json::json!(["adds_cause"])
    );
    assert_eq!(two["passes"], false);
    only_claimed(&answer, &["decision:D-2"]);
    assert_eq!(answer["within"]["passes"], false);
    assert_eq!(answer["within"]["named"][0]["key"], "requirement:R-1");
    let reached: Vec<&Value> = answer["within"]["reach_before"]["reached"]
        .as_array()
        .unwrap()
        .iter()
        .map(|doc| &doc["key"])
        .collect();
    assert_eq!(reached, ["requirement:R-1", "decision:D-1"]);
    // Named the other requirement, the decision is inside its reach.
    let output = project.run(&[
        "impact",
        "--before",
        before.arg(),
        "--within",
        "R-2",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    let two = verdict(&answer, "decision:D-2");
    assert_eq!(kind(two), "inside");
    assert_eq!(two["verdict"]["by"], "reached");
    assert_eq!(answer["within"]["unreadable"], Value::Null);
    // A file whose identity cannot be read may be any document, whether the change
    // adds it or takes it away: the gate does not pass, and says which file and where.
    let broken = "---\nid: \"01a1c4f0-broken\nkey: \"R-9\"\n---\nBroken.\n";
    for (dir, side) in [(&project.0, "files_after"), (&before.0, "files_before")] {
        let file = dir.join("requirements/R-9.md");
        fs::write(&file, broken).unwrap();
        let (exit, answer) = gate(&project, &before, &["R-2"]);
        fs::remove_file(&file).unwrap();
        assert_eq!(exit, Some(1), "{answer}");
        assert_eq!(
            answer["within"]["unreadable"][side],
            serde_json::json!(["requirements/R-9.md"]),
            "{answer}"
        );
        assert_eq!(answer["within"]["passes"], false);
    }
    // So may a file of the ledger that is no record.
    let record = project.0.join(".whyshelf/ledger/broken.json");
    fs::write(&record, "no record\n").unwrap();
    let (exit, answer) = gate(&project, &before, &["R-2"]);
    fs::remove_file(&record).unwrap();
    assert_eq!(exit, Some(1), "{answer}");
    assert_eq!(
        answer["within"]["unreadable"]["records_after"],
        serde_json::json!([".whyshelf/ledger/broken.json"]),
        "{answer}"
    );
    if !git_installed() {
        return;
    }
    fs::write(project.0.join("b.txt"), "two\n").unwrap();
    git(&project.0, &["init", "-q"]);
    git(&project.0, &["add", "-A"]);
    git(&project.0, &["commit", "-q", "-m", "approved"]);
    fs::write(project.0.join("b.txt"), "two, edited\n").unwrap();
    git(&project.0, &["commit", "-q", "-am", "edited"]);
    let output = project.run(&[
        "impact",
        "--since",
        "HEAD~1",
        "--within",
        "R-1",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(1), "{answer}");
    assert_eq!(kind(verdict(&answer, "decision:D-2")), "outside");
    only_claimed(&answer, &["decision:D-2"]);
}

/// Done when 2: a decision the change adds under the named requirement, and approves,
/// is unproven unless it is named too.
#[test]
fn a_decision_added_under_the_named_requirement_is_unproven_unless_named() {
    let project = project("within-added");
    let before = Before::of(&project);
    decision(&project.0, 3, 1, "a.txt");
    let approved = project.run(&["judge", "approve", "D-3"]);
    assert_eq!(approved.status.code(), Some(0), "{approved:?}");
    let output = project.run(&[
        "impact",
        "--before",
        before.arg(),
        "--within",
        "R-1",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(1), "{answer}");
    let three = verdict(&answer, "decision:D-3");
    assert_eq!(kind(three), "unproven", "{three}");
    assert_eq!(three["verdict"]["place"]["place"], "not_before");
    assert_eq!(
        three["verdict"]["witnesses"],
        serde_json::json!(["decided", "on_shelf", "recorded"])
    );
    only_claimed(&answer, &["decision:D-3"]);
    // Named too, at the end after, where it is: the gate passes.
    let output = project.run(&[
        "impact",
        "--before",
        before.arg(),
        "--within",
        "R-1",
        "D-3",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(verdict(&answer, "decision:D-3")["verdict"]["by"], "named");
    assert_eq!(answer["within"]["passes"], true);
}

/// An edit outside the reach exits 1: of a pending document's text, and of a link
/// that gives a document outside the reach a depends link to the named (the reach is
/// the shelf's before the change, so the link the change writes does not put it
/// inside).
#[test]
fn an_edit_outside_the_reach_of_a_pending_text_or_of_a_link_to_the_named_exits_one() {
    let project = project("within-edits");
    note(&project.0, 2, "Pending.");
    let before = Before::of(&project);
    note(&project.0, 2, "Pending, and edited.");
    let (exit, answer) = gate(&project, &before, &["R-1"]);
    assert_eq!(exit, Some(1), "{answer}");
    let two = verdict(&answer, "note:N-2");
    assert_eq!(kind(two), "outside", "{two}");
    assert_eq!(two["verdict"]["witnesses"], serde_json::json!(["edited"]));
    only_claimed(&answer, &["note:N-2"]);
    // The note as it was, and D-2 made to serve R-1.
    note(&project.0, 2, "Pending.");
    decision(&project.0, 2, 1, "b.txt");
    let (exit, answer) = gate(&project, &before, &["R-1"]);
    assert_eq!(exit, Some(1), "{answer}");
    let d2 = verdict(&answer, "decision:D-2");
    assert_eq!(kind(d2), "outside", "{d2}");
    assert!(
        d2["verdict"]["witnesses"]
            .as_array()
            .unwrap()
            .contains(&Value::from("edited")),
        "{d2}"
    );
    only_claimed(&answer, &["decision:D-2"]);
}

/// A book taken out of the declaration takes its documents off the shelf: outside,
/// unless they are in the reach of the documents named.
#[test]
fn a_book_taken_out_of_the_declaration_is_outside_unless_in_the_reach() {
    let project = project("within-declaration");
    let before = Before::of(&project);
    fs::write(project.0.join(".whyshelf/books.toml"), BOOKS).unwrap();
    let output = project.run(&[
        "impact",
        "--before",
        before.arg(),
        "--within",
        "R-1",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(1), "{answer}");
    let one = verdict(&answer, "note:N-1");
    assert_eq!(kind(one), "outside", "{one}");
    assert!(
        one["verdict"]["witnesses"]
            .as_array()
            .unwrap()
            .contains(&Value::from("on_shelf")),
        "{one}"
    );
    only_claimed(&answer, &["note:N-1"]);
    // Named, the note is inside, and nothing else is reached.
    let output = project.run(&[
        "impact",
        "--before",
        before.arg(),
        "--within",
        "N-1",
        "--exit-code",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(kind(verdict(&answer, "note:N-1")), "inside");
}

/// The exits: `--within` alone answers and exits 0; `--exit-code` without `--within`,
/// and a reference that names no document at either end (with `--exit-code` or not),
/// are request errors (exit 2); without `--within` there is no gate.
#[test]
fn the_gate_answers_alone_and_refuses_what_it_cannot_ask() {
    let project = project("within-exits");
    let before = Before::of(&project);
    fs::write(project.0.join("b.txt"), "two, edited\n").unwrap();
    let output = project.run(&["impact", "--before", before.arg(), "--within", "R-1"]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["within"]["passes"], false);
    let output = project.run(&["impact", "--before", before.arg(), "--exit-code"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    for args in [
        &["impact", "--before", before.arg(), "--within", "R-9"][..],
        &[
            "impact",
            "--before",
            before.arg(),
            "--within",
            "R-1",
            "R-9",
            "--exit-code",
        ],
    ] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(json(&output)["error"], "request", "{}", json(&output));
    }
    let output = project.run(&["impact", "--before", before.arg()]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["within"], Value::Null);
}
