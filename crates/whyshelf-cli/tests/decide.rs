//! Who may decide, on the binary and a small project of files where only the group
//! `maintainers` (`ana`) may approve and revoke the requirements: `judge approve` by
//! another name is refused and writes nothing; a record written by hand under that
//! name is not used and fails `check`; `bo` may not withdraw it and `ana` may, and
//! `check` passes again; `--stale` leaves out what the actor may not approve as
//! `not_admitted`. A project that declares no rule takes any name, as before.

mod common;

use std::fs;

use common::{Project, json};

/// The rule: only `maintainers` may approve and revoke the requirements, and the
/// records of no book.
const RULED: &str = r#"[[group]]
name = "maintainers"
members = ["ana"]

[decide]
default = ["maintainers"]

[[book]]
name = "requirement"
dir = "reqs"
key = { prefix = "REQ" }
[[book.field]]
name = "title"
type = "text"
required = true
[book.decide]
approve = ["maintainers"]
revoke = ["maintainers"]
"#;

/// The same books, with no rule.
const UNRULED: &str = r#"[[book]]
name = "requirement"
dir = "reqs"
key = { prefix = "REQ" }
[[book.field]]
name = "title"
type = "text"
required = true
"#;

const REQ: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"REQ-1\"\ntitle: \"A requirement\"\n---\nThe body.\n";

/// The identity of the record written by hand.
const HAND: &str = "01a1c4f0-0000-7000-8000-0000000000ff";

/// A project of these books and REQ-1.
fn project(name: &str, books: &str) -> Project {
    let project = Project::empty(name);
    fs::create_dir_all(project.0.join(".whyshelf")).unwrap();
    fs::create_dir_all(project.0.join("reqs")).unwrap();
    fs::write(project.0.join(".whyshelf/books.toml"), books).unwrap();
    fs::write(project.0.join("reqs/REQ-1.md"), REQ).unwrap();
    project
}

#[test]
fn an_approval_by_a_name_the_rule_does_not_admit_is_refused_and_writes_nothing() {
    let project = project("decide-refused", RULED);
    let before = project.files();
    let output = project.run(&["judge", "approve", "REQ-1", "--actor", "bo"]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(1), "{answer}");
    assert_eq!(answer["error"], "refused", "{answer}");
    let messages = answer["messages"].to_string();
    assert!(
        messages.contains("`bo` may not approve") && messages.contains("`maintainers`"),
        "{answer}"
    );
    assert_eq!(project.files(), before, "nothing is written");

    let output = project.run(&["judge", "approve", "REQ-1", "--actor", "ana"]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["judged"][0]["record"]["book"], "requirement");
}

#[test]
fn a_record_written_by_hand_under_that_name_fails_check_until_an_admitted_name_withdraws_it() {
    let project = project("decide-hand", RULED);
    let approved = project.run(&["judge", "approve", "REQ-1", "--actor", "ana"]);
    assert_eq!(approved.status.code(), Some(0));
    let id = json(&approved)["judged"][0]["record"]["id"]
        .as_str()
        .expect("the record's id")
        .to_owned();
    // The same record, written by hand under another name and identity, in the place
    // of ana's.
    let ledger = project.0.join(".whyshelf/ledger");
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(ledger.join(format!("{id}.json"))).unwrap())
            .unwrap();
    record["actor"] = "bo".into();
    record["id"] = HAND.into();
    fs::remove_file(ledger.join(format!("{id}.json"))).unwrap();
    fs::write(
        ledger.join(format!("{HAND}.json")),
        serde_json::to_string_pretty(&record).unwrap() + "\n",
    )
    .unwrap();

    let check = project.run(&["check"]);
    let answer = json(&check);
    assert_eq!(check.status.code(), Some(1), "{answer}");
    let problems = answer["health"]["ledger_problems"].to_string();
    assert!(
        problems.contains(HAND) && problems.contains("`bo` may not approve"),
        "{answer}"
    );
    // The record is not used: no approval stands.
    let status = json(&project.run(&["status"]));
    assert_eq!(status["docs"][0]["declared"], "pending", "{status}");

    // bo may not revoke in the book either; ana may.
    let before = project.files();
    let refused = project.run(&[
        "judge", "withdraw", HAND, "--reason", "not mine", "--actor", "bo",
    ]);
    assert_eq!(refused.status.code(), Some(1), "{}", json(&refused));
    assert_eq!(project.files(), before, "nothing is written");
    let withdrawn = project.run(&[
        "judge",
        "withdraw",
        HAND,
        "--reason",
        "written under a name the rule does not admit",
        "--actor",
        "ana",
    ]);
    assert_eq!(withdrawn.status.code(), Some(0), "{}", json(&withdrawn));
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", json(&check));
}

#[test]
fn approve_stale_leaves_out_what_the_actor_may_not_approve() {
    let project = project("decide-stale", RULED);
    let approved = project.run(&["judge", "approve", "REQ-1", "--actor", "ana"]);
    assert_eq!(approved.status.code(), Some(0));
    fs::write(project.0.join("reqs/REQ-1.md"), REQ.replace("body", "text")).unwrap();
    let before = project.files();
    let output = project.run(&[
        "judge", "approve", "--stale", "--reason", "read", "--actor", "bo",
    ]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["judged"], serde_json::json!([]), "{answer}");
    assert_eq!(answer["left_out"][0]["reason"], "not_admitted", "{answer}");
    assert_eq!(project.files(), before, "nothing is written");
}

#[test]
fn a_project_that_declares_no_rule_takes_any_name() {
    let project = project("decide-none", UNRULED);
    let output = project.run(&["judge", "approve", "REQ-1", "--actor", "bo"]);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    assert_eq!(project.run(&["check"]).status.code(), Some(0));
}
