//! `judge approve --pending`, on the binary and the project `init --sample` makes: it
//! approves the pending documents upstream first and leaves `stale` empty; the
//! command `init` gives approves its four documents and no other; named V-1, it
//! approves V-1 and what V-1 stands on through `depends` links, and nothing else,
//! judging what holds V-1 back as the run left it; with `--stale`, both are one run;
//! a second run writes nothing; it needs a reason, and `--stale` alone takes no
//! document.

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use common::{Project, json};

/// How many directories this file's tests have made, to keep two apart.
static MADE: AtomicUsize = AtomicUsize::new(0);

/// The project `init --sample` makes in an empty directory, removed when the test
/// ends.
fn sample(name: &str) -> Project {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_nanos();
    let count = MADE.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "whyshelf-pending-{name}-{}-{count}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let project = Project(dir);
    assert_eq!(project.run(&["init", "--sample"]).status.code(), Some(0));
    project
}

fn keys_of(answer: &serde_json::Value) -> Vec<String> {
    answer["judged"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["doc"]["key"].as_str().unwrap().to_owned())
        .collect()
}

fn records(project: &Project) -> usize {
    fs::read_dir(project.0.join(".whyshelf/ledger"))
        .map(|dir| dir.count())
        .unwrap_or(0)
}

fn pending_count(project: &Project) -> usize {
    json(&project.run(&["status"]))["docs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["declared"] == "pending")
        .count()
}

#[test]
fn approve_pending_approves_the_sample_upstream_first_and_leaves_nothing_stale() {
    let project = sample("all");
    let output = project.run(&["judge", "approve", "--pending", "--reason", "r"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(
        keys_of(&answer),
        [
            "finding:F-1",
            "requirement:REQ-1",
            "decision:DEC-1",
            "verification:V-1"
        ]
    );
    for judged in answer["judged"].as_array().unwrap() {
        assert_eq!(judged["rule"], "pending");
        assert_eq!(judged["record"]["reason"], "r");
    }
    assert_eq!(answer["left_out"], serde_json::json!([]));
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(records(&project), 4);
    assert_eq!(
        json(&project.run(&["stale"]))["stale"],
        serde_json::json!([])
    );

    // A second run finds nothing pending, and writes nothing.
    let again = project.run(&["judge", "approve", "--pending", "--reason", "r"]);
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(json(&again)["judged"], serde_json::json!([]));
    assert_eq!(records(&project), 4);
}

#[test]
fn approve_pending_of_v_1_takes_what_it_stands_on_through_depends_links_only() {
    let project = sample("v-1");
    let output = project.run(&["judge", "approve", "--pending", "V-1", "--reason", "r"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    // F-1 is reached from REQ-1 by `grounds`, an informs link: not taken.
    assert_eq!(
        keys_of(&answer),
        ["requirement:REQ-1", "decision:DEC-1", "verification:V-1"]
    );
    assert_eq!(records(&project), 3);
    assert_eq!(
        json(&project.run(&["stale"]))["stale"],
        serde_json::json!([])
    );

    // Named again, V-1 is approved now: a request error that names it.
    let again = project.run(&["judge", "approve", "--pending", "V-1", "--reason", "r"]);
    assert_eq!(again.status.code(), Some(2));
    assert_eq!(json(&again)["error"], "request");
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("`V-1` is not pending"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(records(&project), 3);
}

#[test]
fn the_command_init_gives_approves_its_four_documents_and_no_other() {
    let project = sample("init-next");
    // A document of the person's own in a book of the sample.
    assert_eq!(
        project
            .run(&["new", "finding", "--title", "Mine"])
            .status
            .code(),
        Some(0)
    );
    let output = project.run(&[
        "judge",
        "approve",
        "--pending",
        "F-1",
        "V-1",
        "--reason",
        "read, and it holds",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        keys_of(&json(&output)),
        [
            "finding:F-1",
            "requirement:REQ-1",
            "decision:DEC-1",
            "verification:V-1"
        ]
    );
    assert_eq!(records(&project), 4);
    assert_eq!(pending_count(&project), 1);
    assert_eq!(
        json(&project.run(&["stale"]))["stale"],
        serde_json::json!([])
    );
}

#[test]
fn an_upstream_stale_only_through_a_pending_one_the_run_approves_holds_nothing_back() {
    // DEC-1 approved while REQ-1 is pending: stale at once.
    let project = sample("through");
    assert_eq!(
        project.run(&["judge", "approve", "DEC-1"]).status.code(),
        Some(0)
    );
    assert_eq!(
        json(&project.run(&["stale"]))["stale"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let output = project.run(&["judge", "approve", "--pending", "V-1", "--reason", "r"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(keys_of(&answer), ["requirement:REQ-1", "verification:V-1"]);
    assert_eq!(answer["left_out"], serde_json::json!([]));
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(
        json(&project.run(&["stale"]))["stale"],
        serde_json::json!([])
    );
}

#[test]
fn approve_pending_and_stale_together_are_one_run_upstream_first() {
    // DEC-1 approved while REQ-1 is pending, then edited: stale for itself too.
    let project = sample("both");
    assert_eq!(
        project.run(&["judge", "approve", "DEC-1"]).status.code(),
        Some(0)
    );
    let path = project.0.join("books/decision/DEC-1.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}One more line.\n")).unwrap();

    let output = project.run(&["judge", "approve", "--pending", "--stale", "--reason", "r"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(
        keys_of(&answer),
        [
            "finding:F-1",
            "requirement:REQ-1",
            "decision:DEC-1",
            "verification:V-1"
        ]
    );
    let rules: Vec<&str> = answer["judged"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["pending", "pending", "stale", "pending"]);
    assert_eq!(answer["left_out"], serde_json::json!([]));
    assert_eq!(
        json(&project.run(&["stale"]))["stale"],
        serde_json::json!([])
    );
    assert_eq!(pending_count(&project), 0);
}

#[test]
fn approve_stale_alone_takes_no_document() {
    let project = sample("stale-ref");
    let output = project.run(&["judge", "approve", "--stale", "V-1", "--reason", "r"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["error"], "request");
    assert_eq!(records(&project), 0);
}

#[test]
fn approve_pending_needs_a_reason_and_takes_no_again() {
    let project = sample("args");
    for args in [
        &["judge", "approve", "--pending"][..],
        &["judge", "approve", "--pending", "V-1"],
        &["judge", "approve", "--pending", "--again", "--reason", "r"],
        &[
            "judge",
            "approve",
            "--pending",
            "--stale",
            "--again",
            "--reason",
            "r",
        ],
    ] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(json(&output)["error"], "request", "{args:?}");
    }
    assert_eq!(records(&project), 0);
    assert_eq!(pending_count(&project), 4);
}
