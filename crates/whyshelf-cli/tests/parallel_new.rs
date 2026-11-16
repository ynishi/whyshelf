//! `new` with no key, run many at once in one working tree: each run writes a
//! document of its own, at a key of its own, and a key that was asked for is still
//! refused when its file is there.

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{Project, json};

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// How many runs start at once.
const RUNS: usize = 8;

#[test]
fn runs_of_new_with_no_key_at_once_each_write_a_document_at_a_key_of_their_own() {
    let project = Project::new("parallel-new");
    // All are started, and only then is any waited for, so that they read the shelf
    // while none has written.
    let children: Vec<_> = (0..RUNS)
        .map(|n| project.spawn(&["new", "note", "--title", &format!("Note {n}")]))
        .collect();
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().expect("the run ends"))
        .collect();

    let mut keys = BTreeSet::new();
    for output in &outputs {
        assert_eq!(output.status.code(), Some(0), "{}", stderr(output));
        let key = json(output)["doc"]["key"]
            .as_str()
            .expect("a key")
            .to_owned();
        assert!(keys.insert(key.clone()), "{key} was written twice");
    }
    assert_eq!(keys.len(), RUNS);

    // Each key is a file of its own beside the note that was there, and the shelf is
    // clean.
    let files = fs::read_dir(project.0.join("notes")).unwrap().count();
    assert_eq!(files, RUNS + 1);
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    assert_eq!(json(&check)["clean"], true);
}

#[test]
fn a_key_that_was_asked_for_is_still_refused_when_it_is_taken() {
    let project = Project::new("parallel-new-asked");
    let before = project.files();
    let output = project.run(&["new", "note", "--title", "Again", "--key", "N-1"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(json(&output)["error"], "request");
    assert_eq!(project.files(), before);
}

#[test]
fn a_file_at_every_key_tried_is_one_refusal_that_says_what_to_do_and_writes_nothing() {
    let project = Project::new("parallel-new-no-free-key");
    // The shelf offers N-2; the files at N-2 to N-65 show no key, and 64 keys are
    // tried.
    for n in 2..=65 {
        fs::write(
            project.0.join(format!("notes/N-{n}.md")),
            "no front matter here\n",
        )
        .unwrap();
    }
    let before = project.files();
    let output = project.run(&["new", "note", "--title", "Never"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert_eq!(
        answer["messages"],
        serde_json::json!([
            "the book `note` had a file at every key from `N-2` to `N-65`; nothing was written: \
             other runs may be writing at the same time, or files that are no valid document \
             are in the way (`check` lists them); run `new` again once they are done or put right"
        ])
    );
    assert_eq!(project.files(), before);
}

#[test]
fn the_help_of_key_says_how_many_keys_are_passed_over() {
    let project = Project::new("parallel-new-help");
    let output = project.run(&["new", "--help"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    // The text wraps at the width of the terminal: read it as words.
    let help = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        help.contains(
            "The key (default: the book's next number; a book of slugs needs one). Without \
             --key, a key whose file is already there is passed over for the next number, up to \
             64 keys."
        ),
        "{help}"
    );
}

#[test]
fn a_file_whose_front_matter_cannot_be_read_does_not_stop_new_with_no_key() {
    let project = Project::new("parallel-new-unreadable");
    // The shelf shows no key for it, so the next key is N-2, which is its name.
    fs::write(project.0.join("notes/N-2.md"), "no front matter here\n").unwrap();
    let output = project.run(&["new", "note", "--title", "Third"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(json(&output)["doc"]["key"], "note:N-3");
    assert_eq!(
        fs::read_to_string(project.0.join("notes/N-2.md")).unwrap(),
        "no front matter here\n"
    );
    assert!(project.0.join("notes/N-3.md").exists());
}
