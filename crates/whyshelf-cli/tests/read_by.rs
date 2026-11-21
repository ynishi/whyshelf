//! Who read the content an approval stands on, on the binary and a small project of
//! files that declares no rule of who may decide: every approved row says how many
//! names read it (`readers`), and `status --readers` lists the documents read by
//! that many. An approval by one name is read by `one`; once a second name approves
//! the same content, by `several`; an edit approved again by one name is read by
//! `one` again; and an approval every reading of which lapsed is read by `nobody`.

mod common;

use std::fs;
use std::thread::sleep;
use std::time::{Duration, Instant};

use common::{NOTE, Project, in_seconds, json};
use serde_json::{Value, json as value};

/// The keys of the documents `status --readers <word>` lists, for each word given.
fn listed(project: &Project, words: &[&str]) -> Vec<String> {
    let mut args = vec!["status"];
    for word in words {
        args.extend(["--readers", *word]);
    }
    let output = project.run(&args);
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    json(&output)["docs"]
        .as_array()
        .expect("docs")
        .iter()
        .map(|doc| doc["doc"]["key"].as_str().unwrap().to_owned())
        .collect()
}

/// The `readers` of the row of N-1, as `status` shows it; none when it has none.
fn readers(project: &Project) -> Option<Value> {
    json(&project.run(&["status"]))["docs"][0]
        .get("readers")
        .cloned()
}

fn approve_as(project: &Project, actor: &str) {
    let output = project.run(&["judge", "approve", "N-1", "--actor", actor]);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
}

/// Which of the three words list N-1 (`note:N-1` in an answer).
fn which(project: &Project) -> Vec<&'static str> {
    ["nobody", "one", "several"]
        .into_iter()
        .filter(|word| listed(project, &[*word]) == ["note:N-1"])
        .collect()
}

#[test]
fn an_approval_is_read_by_one_then_by_several_and_by_one_again_after_an_edit() {
    let project = Project::new("read-by");
    // No approval: no readers, and no word lists it.
    assert_eq!(readers(&project), None);
    assert_eq!(which(&project), Vec::<&str>::new());

    // One name approves: it is the document that no second name has read.
    approve_as(&project, "ana");
    assert_eq!(
        readers(&project),
        Some(value!({"count": "one", "counted": ["ana"], "lapsed": []}))
    );
    assert_eq!(which(&project), ["one"]);

    // A second name approves the same content: it is no longer listed as one.
    approve_as(&project, "bo");
    assert_eq!(
        readers(&project),
        Some(value!({"count": "several", "counted": ["ana", "bo"], "lapsed": []}))
    );
    assert_eq!(which(&project), ["several"]);

    // Edited, and approved again by one name: that content has one reader.
    fs::write(project.0.join("notes/N-1.md"), NOTE.replace("body", "text")).unwrap();
    approve_as(&project, "ana");
    assert_eq!(
        readers(&project),
        Some(value!({"count": "one", "counted": ["ana"], "lapsed": []}))
    );
    assert_eq!(which(&project), ["one"]);
}

#[test]
fn a_repeated_word_is_either_and_the_filters_are_carried_as_given() {
    let project = Project::new("read-by-either");
    approve_as(&project, "ana");
    assert_eq!(listed(&project, &["one", "several"]), ["note:N-1"]);
    assert_eq!(
        listed(&project, &["nobody", "several"]),
        Vec::<String>::new()
    );

    let output = project.run(&["status", "--readers", "several", "--readers", "nobody"]);
    assert_eq!(output.status.code(), Some(0));
    // In a fixed order, as given or not.
    assert_eq!(
        json(&output)["filters"]["readers"],
        value!(["nobody", "several"])
    );
    // It is one more kind beside the others: it must hold too.
    let both = project.run(&["status", "--readers", "one", "--standing", "stale"]);
    assert_eq!(json(&both)["docs"], value!([]));
}

#[test]
fn a_word_that_is_no_count_is_a_request_error_with_the_words_there_are() {
    let project = Project::new("read-by-word");
    for word in ["two", "One", ""] {
        let output = project.run(&["status", "--readers", word]);
        assert_eq!(output.status.code(), Some(2), "{word:?}");
        let answer = json(&output);
        assert_eq!(answer["error"], "request", "{answer}");
        let message = answer["messages"][0].as_str().unwrap();
        assert!(
            message.starts_with(&format!("--readers `{word}`")),
            "{message}"
        );
        for known in ["nobody", "one", "several"] {
            assert!(message.contains(known), "{known}: {message}");
        }
    }
}

#[test]
fn an_approval_every_reading_of_which_lapsed_is_read_by_nobody() {
    let project = Project::new("read-by-lapsed");
    // Both readings lapse no sooner than 5 s from here (the seconds of now are
    // whole). Only the two approvals have to run before that, so a loaded machine
    // has the whole window for them; that two names read it before is the first
    // test's.
    let start = Instant::now();
    let until = in_seconds(6);
    for actor in ["ana", "bo"] {
        let output = project.run(&[
            "judge", "approve", "N-1", "--until", &until, "--actor", actor,
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    }

    sleep(Duration::from_millis(7_000).saturating_sub(start.elapsed()));
    // Lapsed readings are told apart and not counted: it is not `one`, nor `several`.
    assert_eq!(
        readers(&project),
        Some(value!({"count": "nobody", "counted": [], "lapsed": ["ana", "bo"]}))
    );
    assert_eq!(which(&project), ["nobody"]);
}
