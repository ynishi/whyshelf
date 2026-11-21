//! `rekey`, on the binary and a small project of files: two findings of one key after a
//! merge, and two specs whose `grounds` link to it. A document that writes the old key in
//! a link is decided by a word when no approval says which document it meant; one that is
//! not is refused with every such document listed; files that could not be read stop it;
//! `--dry-run` writes nothing. The approved case (the approvals hold) is in
//! `parallel.rs`.

mod common;

use std::fs;

use common::{Project, json};

const BOOKS: &str = r#"[[book]]
name = "finding"
dir = "findings"
key = { prefix = "F" }
[[book.field]]
name = "title"
type = "text"
required = true

[[book]]
name = "spec"
dir = "specs"
key = { prefix = "S" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.link]]
name = "grounds"
target = "finding"
means = "depends"
"#;

const LANDED: &str = "01a1c4f0-0000-7000-8000-000000000001";
const BRANCH: &str = "01a1c4f0-0000-7000-8000-000000000002";
const SPEC_LANDED: &str = "01a1c4f0-0000-7000-8000-000000000011";
const SPEC_BRANCH: &str = "01a1c4f0-0000-7000-8000-000000000012";

/// A finding, in the one form.
fn finding(id: &str, key: &str, title: &str) -> String {
    format!("---\nid: \"{id}\"\nkey: \"{key}\"\ntitle: \"{title}\"\n---\nThe {title}.\n")
}

/// A spec that grounds itself in these findings, in the one form.
fn spec(id: &str, key: &str, title: &str, grounds: &[&str]) -> String {
    let grounds: Vec<String> = grounds.iter().map(|key| format!("\"{key}\"")).collect();
    format!(
        "---\nid: \"{id}\"\nkey: \"{key}\"\ngrounds: [{}]\ntitle: \"{title}\"\n---\nThe {title}.\n",
        grounds.join(", ")
    )
}

fn put(project: &Project, file: &str, text: &str) {
    let path = project.0.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(project: &Project, file: &str) -> String {
    fs::read_to_string(project.0.join(file)).unwrap()
}

/// What a merge left: two findings that write `F-1` (the main branch's, and the
/// branch's), and a spec of each that links to `F-1`. Nothing is approved.
fn merged(name: &str) -> Project {
    let project = Project::empty(name);
    put(&project, ".whyshelf/books.toml", BOOKS);
    put(
        &project,
        "findings/F-1.md",
        &finding(LANDED, "F-1", "landed finding"),
    );
    put(
        &project,
        "findings/F-1-b.md",
        &finding(BRANCH, "F-1", "branch finding"),
    );
    put(
        &project,
        "specs/S-1.md",
        &spec(SPEC_LANDED, "S-1", "landed spec", &["F-1"]),
    );
    put(
        &project,
        "specs/S-2.md",
        &spec(SPEC_BRANCH, "S-2", "branch spec", &["F-1"]),
    );
    project
}

fn ok(project: &Project, args: &[&str]) -> serde_json::Value {
    let output = project.run(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)
}

/// The answer of a command that was refused, with the exit status it gave.
fn refused(project: &Project, args: &[&str], code: i32) -> serde_json::Value {
    let output = project.run(args);
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)
}

#[test]
fn a_link_that_no_approval_decides_is_decided_by_a_word_and_the_files_are_rewritten() {
    let project = merged("rekey-pending");
    let before = project.files();
    assert_eq!(project.run(&["check"]).status.code(), Some(1));

    let answer = ok(
        &project,
        &[
            "rekey",
            BRANCH,
            "--follow",
            SPEC_BRANCH,
            "--stay",
            SPEC_LANDED,
        ],
    );
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["dry_run"], false);
    assert_eq!(answer["moved"]["doc"]["id"], BRANCH);
    assert_eq!(answer["moved"]["from_key"], "finding:F-1");
    assert_eq!(answer["moved"]["to_key"], "finding:F-2");
    assert_eq!(answer["moved"]["from_file"], "findings/F-1-b.md");
    assert_eq!(answer["moved"]["to_file"], "findings/F-2.md");
    assert_eq!(answer["rewritten"][0]["file"], "specs/S-2.md");
    assert_eq!(answer["rewritten"][0]["why"], "asked");
    assert_eq!(answer["rewritten"][0]["items"][0]["link"], "grounds");
    assert_eq!(answer["rewritten"][0]["items"][0]["written"], "F-1");
    assert_eq!(answer["stayed"][0]["file"], "specs/S-1.md");
    assert_eq!(answer["both"], serde_json::json!([]));
    assert_eq!(
        answer["written"],
        serde_json::json!({
            "created": "findings/F-2.md",
            "replaced": ["specs/S-2.md"],
            "removed": "findings/F-1-b.md",
        })
    );
    // The document that took the key and each document that wrote the old one in a link,
    // read from the files after the write.
    let docs: Vec<&str> = answer["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["doc"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(docs, [BRANCH, SPEC_LANDED, SPEC_BRANCH]);
    for row in answer["approvals"].as_array().unwrap() {
        assert_eq!(row["standing"], "pending", "{row}");
    }

    // The files: the finding in the file of its key and its old file gone; the link that
    // meant it rewritten and the other left as it is; nothing else touched.
    assert_eq!(
        read(&project, "findings/F-2.md"),
        finding(BRANCH, "F-2", "branch finding")
    );
    assert!(!project.0.join("findings/F-1-b.md").exists());
    assert_eq!(
        read(&project, "specs/S-2.md"),
        spec(SPEC_BRANCH, "S-2", "branch spec", &["F-2"])
    );
    let after = project.files();
    for file in ["findings/F-1.md", "specs/S-1.md", ".whyshelf/books.toml"] {
        let path = project.0.join(file);
        assert_eq!(after[&path], before[&path], "{file}");
    }
    // Nothing is wrong now, and every file is in the form.
    ok(&project, &["check"]);
    ok(&project, &["fmt", "--check"]);
    let status = ok(&project, &["status"]);
    assert_eq!(status["docs"].as_array().unwrap().len(), 4, "{status}");
}

#[test]
fn a_link_that_means_both_keeps_the_old_key_and_gains_the_new_one() {
    let project = merged("rekey-both");
    let answer = ok(
        &project,
        &[
            "rekey",
            BRANCH,
            "--both",
            SPEC_BRANCH,
            "--stay",
            SPEC_LANDED,
        ],
    );
    assert_eq!(answer["both"][0]["file"], "specs/S-2.md");
    assert_eq!(answer["rewritten"], serde_json::json!([]));
    assert_eq!(
        read(&project, "specs/S-2.md"),
        spec(SPEC_BRANCH, "S-2", "branch spec", &["F-1", "F-2"])
    );
    ok(&project, &["check"]);
}

#[test]
fn a_document_nothing_decides_is_refused_with_every_such_document_and_nothing_is_written() {
    let project = merged("rekey-undecided");
    let before = project.files();
    // One of the two is decided, the other is not: both are told when none is.
    let answer = refused(&project, &["rekey", BRANCH], 2);
    assert_eq!(answer["error"], "request");
    let problems: Vec<&str> = answer["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(problems.len(), 2, "{answer}");
    assert!(problems[0].contains("specs/S-1.md") && problems[0].contains(SPEC_LANDED));
    assert!(problems[1].contains("specs/S-2.md") && problems[1].contains(SPEC_BRANCH));
    assert!(problems[0].contains("--follow"), "{}", problems[0]);
    assert_eq!(project.files(), before);

    // A word is no way round a document that is not one that writes the key in a link.
    let answer = refused(
        &project,
        &[
            "rekey",
            BRANCH,
            "--follow",
            LANDED,
            "--follow",
            SPEC_LANDED,
            "--stay",
            SPEC_BRANCH,
        ],
        2,
    );
    assert_eq!(answer["error"], "request");
    assert_eq!(project.files(), before);

    // A document named in two words is a wrong request.
    let answer = refused(
        &project,
        &[
            "rekey",
            BRANCH,
            "--follow",
            SPEC_LANDED,
            "--stay",
            SPEC_LANDED,
        ],
        2,
    );
    assert_eq!(answer["error"], "request");
    assert_eq!(project.files(), before);
}

#[test]
fn a_file_with_conflict_markers_left_in_it_stops_the_rekey_and_is_named() {
    let project = merged("rekey-markers");
    // A finding that was never resolved: in the book of the key, it may write the key.
    put(
        &project,
        "findings/F-3.md",
        "<<<<<<< HEAD\n---\nid: \"01a1c4f0-0000-7000-8000-000000000003\"\nkey: \"F-1\"\n---\n=======\n>>>>>>> branch\n",
    );
    // A spec that was never resolved: in a book that links into the key's book, it may
    // link to the key.
    put(
        &project,
        "specs/S-3.md",
        "<<<<<<< HEAD\nnot a front matter\n",
    );
    let before = project.files();
    let answer = refused(
        &project,
        &[
            "rekey",
            BRANCH,
            "--follow",
            SPEC_BRANCH,
            "--stay",
            SPEC_LANDED,
        ],
        1,
    );
    assert_eq!(answer["error"], "refused");
    let problems: Vec<&str> = answer["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert!(
        problems.iter().any(|p| p.contains("findings/F-3.md")),
        "{answer}"
    );
    assert!(
        problems.iter().any(|p| p.contains("specs/S-3.md")),
        "{answer}"
    );
    assert_eq!(project.files(), before);
}

#[test]
fn a_dry_run_is_the_same_plan_checked_and_writes_nothing() {
    let project = merged("rekey-dry");
    let before = project.files();
    let answer = ok(
        &project,
        &[
            "rekey",
            BRANCH,
            "--key",
            "F-7",
            "--follow",
            SPEC_BRANCH,
            "--stay",
            SPEC_LANDED,
            "--dry-run",
        ],
    );
    assert_eq!(answer["dry_run"], true);
    assert_eq!(answer["moved"]["to_key"], "finding:F-7");
    assert_eq!(answer["moved"]["to_file"], "findings/F-7.md");
    assert_eq!(answer["rewritten"][0]["file"], "specs/S-2.md");
    assert_eq!(answer["written"], serde_json::Value::Null);
    assert_eq!(answer["approvals"], serde_json::json!([]));
    assert_eq!(project.files(), before);

    // It is refused as the real one is: a document that is not decided.
    let answer = refused(&project, &["rekey", BRANCH, "--dry-run"], 2);
    assert_eq!(answer["problems"].as_array().unwrap().len(), 2, "{answer}");
    assert_eq!(project.files(), before);
}

#[test]
fn nobody_else_writing_the_key_every_link_to_it_follows_with_no_word() {
    let project = Project::empty("rekey-only");
    put(&project, ".whyshelf/books.toml", BOOKS);
    put(
        &project,
        "findings/F-1.md",
        &finding(LANDED, "F-1", "finding"),
    );
    put(
        &project,
        "specs/S-1.md",
        &spec(SPEC_LANDED, "S-1", "spec", &["F-1"]),
    );
    ok(&project, &["check"]);
    let answer = ok(&project, &["rekey", "F-1", "--key", "F-5"]);
    assert_eq!(answer["moved"]["to_file"], "findings/F-5.md");
    assert_eq!(answer["rewritten"][0]["why"], "only_writer");
    assert_eq!(
        read(&project, "specs/S-1.md"),
        spec(SPEC_LANDED, "S-1", "spec", &["F-5"])
    );
    ok(&project, &["check"]);
    // A key that is taken is a wrong request, and nothing is written.
    let before = project.files();
    refused(&project, &["rekey", "F-5", "--key", "F-5"], 2);
    assert_eq!(project.files(), before);
    // `schema` describes the answer.
    let schema = ok(&project, &["schema"]);
    assert!(schema["outputs"]["rekey"]["properties"]["moved"].is_object());
}
