//! Who may decide, on the binary and a small project of files where only the group
//! `maintainers` (`ana`) may approve and revoke the requirements: `judge approve` by
//! another name is refused and writes nothing; a record written by hand under that
//! name is not used and fails `check`; `bo` may not withdraw it and `ana` may, and
//! `check` passes again; `--stale` leaves out what the actor may not approve as
//! `not_admitted`. Where the rule asks for two different names to approve, a
//! requirement approved by one awaits the second, is fresh once another name
//! approves it, and awaits again once the first approval lapses. A project that
//! declares no rule takes any name, as before.

mod common;

use std::fs;
use std::thread::sleep;
use std::time::{Duration, Instant};

use common::{Project, in_seconds, json};

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

/// The rule: a requirement, and a record of no book, needs two different names of
/// `maintainers` (`ana`, `bo`).
const TWO_NAMES: &str = r#"[[group]]
name = "maintainers"
members = ["ana", "bo"]

[decide]
default = ["maintainers"]
approvals = 2

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
approvals = 2
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

/// REQ-1 as `status` shows it.
fn req(project: &Project) -> serde_json::Value {
    json(&project.run(&["status"]))["docs"][0].clone()
}

#[test]
fn with_two_names_asked_an_approval_awaits_the_second_and_again_once_the_first_lapses() {
    let project = project("decide-two", TWO_NAMES);
    // ana's approval lapses no sooner than 5 s from here (the seconds of now are
    // whole), and four runs come before what holds only until then.
    let start = Instant::now();
    let until = in_seconds(6);
    let first = project.run(&[
        "judge", "approve", "REQ-1", "--until", &until, "--actor", "ana",
    ]);
    assert_eq!(first.status.code(), Some(0), "{}", json(&first));

    // One name: not fresh, awaiting the second.
    let one = req(&project);
    assert_eq!(one["standing"], "awaiting", "{one}");
    assert_eq!(
        one["awaiting"],
        serde_json::json!({"readers": ["ana"], "need": 2}),
        "{one}"
    );

    // Another name approves the same content: fresh.
    let second = project.run(&["judge", "approve", "REQ-1", "--actor", "bo"]);
    assert_eq!(second.status.code(), Some(0), "{}", json(&second));
    let two = req(&project);
    assert_eq!(two["standing"], "fresh", "{two}");
    assert!(two.get("awaiting").is_none(), "{two}");

    // ana's approval lapses: bo's stands alone, and awaits again.
    sleep(Duration::from_millis(7_000).saturating_sub(start.elapsed()));
    let lapsed = req(&project);
    assert_eq!(lapsed["standing"], "awaiting", "{lapsed}");
    assert_eq!(
        lapsed["awaiting"],
        serde_json::json!({"readers": ["bo"], "need": 2}),
        "{lapsed}"
    );

    // An approval that awaits is nothing wrong: check passes and lists it, with a
    // note; stale --exit-code passes; status --standing awaiting lists it.
    let check = project.run(&["check"]);
    let answer = json(&check);
    assert_eq!(check.status.code(), Some(0), "{answer}");
    assert_eq!(answer["awaiting"][0]["standing"], "awaiting", "{answer}");
    let notes = String::from_utf8_lossy(&check.stderr);
    assert!(
        notes.contains("note: requirement:REQ-1 is approved by `bo`"),
        "{notes}"
    );
    assert_eq!(
        project.run(&["stale", "--exit-code"]).status.code(),
        Some(0)
    );
    let awaiting = json(&project.run(&["status", "--standing", "awaiting"]));
    let listed = awaiting["docs"].as_array().map(Vec::len);
    assert_eq!(listed, Some(1), "{awaiting}");
}

#[test]
fn a_fresh_approval_with_its_names_is_not_listed_as_awaiting_by_check() {
    let project = project("decide-two-met", TWO_NAMES);
    for actor in ["ana", "bo"] {
        let output = project.run(&["judge", "approve", "REQ-1", "--actor", actor]);
        assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    }
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0));
    assert!(json(&check).get("awaiting").is_none());
}

#[test]
fn a_project_that_declares_no_rule_takes_any_name() {
    let project = project("decide-none", UNRULED);
    let output = project.run(&["judge", "approve", "REQ-1", "--actor", "bo"]);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    assert_eq!(project.run(&["check"]).status.code(), Some(0));
}
