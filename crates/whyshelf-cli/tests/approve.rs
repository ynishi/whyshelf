//! `judge approve`, on the binary and a small project of files: the same approval run
//! twice writes its record once, and the second run lists the document as left out,
//! `repeats`, with a warning line on stderr and exit 0; `--again` writes it anyway.

mod common;

use std::fs;

use common::{Project, json};

/// How many records the ledger holds.
fn records(project: &Project) -> usize {
    fs::read_dir(project.0.join(".whyshelf/ledger"))
        .map(|dir| dir.count())
        .unwrap_or(0)
}

#[test]
fn approving_a_fresh_approval_again_leaves_it_out_and_again_writes_it() {
    let project = Project::new("approve-fresh");
    let first = project.run(&["judge", "approve", "N-1"]);
    assert_eq!(first.status.code(), Some(0));
    let first = json(&first);
    assert_eq!(first["left_out"], serde_json::json!([]));
    let standing = first["judged"][0]["record"]["id"]
        .as_str()
        .expect("the record's id")
        .to_owned();
    assert_eq!(records(&project), 1);

    let second = project.run(&["judge", "approve", "N-1"]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(records(&project), 1, "nothing written");
    let answer = json(&second);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"], serde_json::json!([]));
    let left = &answer["left_out"][0];
    assert_eq!(left["doc"]["key"], "note:N-1");
    assert_eq!(left["reason"], "repeats");
    assert_eq!(left["approval"], standing.as_str());
    assert_eq!(answer["left_out"].as_array().map(Vec::len), Some(1));
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.starts_with("warning: note:N-1 was not approved again: ")
            && stderr.contains("--again")
            && stderr.lines().count() == 1,
        "{stderr}"
    );

    let again = project.run(&["judge", "approve", "N-1", "--again"]);
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(records(&project), 2);
    let answer = json(&again);
    assert_eq!(answer["judged"][0]["doc"]["key"], "note:N-1");
    assert_eq!(answer["left_out"], serde_json::json!([]));
    assert_eq!(String::from_utf8_lossy(&again.stderr), "");

    // `--again` is not for `--stale`.
    let both = project.run(&["judge", "approve", "--stale", "--again", "--reason", "x"]);
    assert_eq!(both.status.code(), Some(2));
}
