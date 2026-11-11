//! `unlinked`, on the binary and a small project of files: it lists the documents no
//! link reaches and those that hold none, each in one list as it fits, leaves out a
//! book its declaration cuts off, and answers the problems of a declaration that
//! cannot be used.
//!
//! One test reads this repository's own books, with a reading command only.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{BOOKS, NOTE, Project, json};

/// The keys of a list of `unlinked`, or none when it is null.
fn unlinked_keys(list: &serde_json::Value) -> Option<Vec<String>> {
    list.as_array().map(|entries| {
        entries
            .iter()
            .map(|e| e["row"]["doc"]["key"].as_str().unwrap().to_owned())
            .collect()
    })
}

#[test]
fn unlinked_isolates_a_lone_document_and_lists_a_linked_pair_as_each_fits() {
    // A note may answer a topic, and a topic is about notes.
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"answers\"\ntarget = \"topic\"\nmeans = \"informs\"\n\
         [[book]]\nname = \"topic\"\ndir = \"topics\"\nkey = {{ prefix = \"T\" }}\n\
         [[book.field]]\nname = \"title\"\ntype = \"text\"\nrequired = true\n\
         [[book.link]]\nname = \"about\"\ntarget = \"note\"\nmeans = \"informs\"\n"
    );
    let project = Project::with("unlinked", &books, NOTE);
    fs::create_dir_all(project.0.join("topics")).unwrap();
    let output = project.run(&["unlinked"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    let none = Some(Vec::<String>::new());
    assert_eq!(
        unlinked_keys(&answer["isolated"]),
        Some(vec!["note:N-1".to_owned()])
    );
    assert_eq!(answer["isolated"][0]["book"], "note");
    assert_eq!(answer["isolated"][0]["row"]["standing"], "pending");
    // The lists are disjoint: an isolated document is in no other.
    assert_eq!(unlinked_keys(&answer["no_incoming"]), none);
    assert_eq!(unlinked_keys(&answer["no_outgoing"]), none);
    assert_eq!(unlinked_keys(&answer["decided_against"]["isolated"]), none);
    assert_eq!(answer["health"]["clean"], true);

    fs::write(
        project.0.join("topics/T-1.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000002\"\nkey: \"T-1\"\ntitle: \"A topic\"\nabout: [N-1]\n---\nIt is about N-1.\n",
    )
    .unwrap();
    let answer = json(&project.run(&["unlinked"]));
    assert_eq!(unlinked_keys(&answer["isolated"]), none);
    assert_eq!(
        unlinked_keys(&answer["no_incoming"]),
        Some(vec!["topic:T-1".to_owned()])
    );
    assert_eq!(
        unlinked_keys(&answer["no_outgoing"]),
        Some(vec!["note:N-1".to_owned()])
    );

    let answer = json(&project.run(&["unlinked", "--no-outgoing"]));
    assert_eq!(answer["no_incoming"], serde_json::Value::Null);
    assert_eq!(
        answer["decided_against"]["no_incoming"],
        serde_json::Value::Null
    );
    assert_eq!(
        unlinked_keys(&answer["no_outgoing"]),
        Some(vec!["note:N-1".to_owned()])
    );
    assert_eq!(unlinked_keys(&answer["isolated"]), none);

    let answer = json(&project.run(&["unlinked", "--no-incoming", "--book", "topic"]));
    assert_eq!(
        unlinked_keys(&answer["no_incoming"]),
        Some(vec!["topic:T-1".to_owned()])
    );
    assert_eq!(answer["no_outgoing"], serde_json::Value::Null);

    let output = project.run(&["unlinked", "--book", "nothing"]);
    assert_eq!(output.status.code(), Some(2));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert_eq!(answer["messages"][0], "there is no book `nothing`");
}

#[test]
fn unlinked_leaves_out_a_book_its_declaration_cuts_off() {
    // The one book declares no link and no link points into it.
    let project = Project::new("unlinked-bare");
    for args in [
        &["unlinked"][..],
        &["unlinked", "--no-incoming"],
        &["unlinked", "--no-outgoing"],
    ] {
        let answer = json(&project.run(args));
        assert_eq!(answer["isolated"], serde_json::json!([]), "{args:?}");
    }
}

#[test]
fn unlinked_of_a_declaration_that_cannot_be_used_is_its_problems_with_the_lists_asked_for() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\ndescription = \"\"\"\ntwo\nlines\"\"\"\n",
    );
    let project = Project::with("unlinked-unusable", &books, NOTE);
    let check = project.run(&["check"]);
    let output = project.run(&["unlinked", "--no-incoming"]);
    assert_eq!(output.status.code(), Some(1));
    let answer = json(&output);
    assert_eq!(answer["schema_problems"], json(&check)["schema_problems"]);
    assert_eq!(answer["isolated"], serde_json::json!([]));
    assert_eq!(answer["no_incoming"], serde_json::json!([]));
    assert_eq!(answer["no_outgoing"], serde_json::Value::Null);
    assert_eq!(answer["decided_against"]["isolated"], serde_json::json!([]));
    assert_eq!(
        answer["decided_against"]["no_incoming"],
        serde_json::json!([])
    );
    assert_eq!(
        answer["decided_against"]["no_outgoing"],
        serde_json::Value::Null
    );
    assert_eq!(answer["health"], serde_json::Value::Null);
}

/// Run the binary on this repository's root, read-only.
fn on_this_repository(args: &[&str]) -> serde_json::Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&root)
        .args(args)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    json(&output)
}

#[test]
fn unlinked_on_this_repository_agrees_with_trace_and_lists_each_document_once() {
    let answer = on_this_repository(&["unlinked"]);
    let lists = [
        &answer["isolated"],
        &answer["no_incoming"],
        &answer["no_outgoing"],
        &answer["decided_against"]["isolated"],
        &answer["decided_against"]["no_incoming"],
        &answer["decided_against"]["no_outgoing"],
    ];
    let mut seen = std::collections::BTreeSet::new();
    for list in lists {
        for key in unlinked_keys(list).expect("every list is asked for") {
            assert!(seen.insert(key.clone()), "{key} is in two lists");
        }
    }
    let isolated = unlinked_keys(&answer["isolated"])
        .into_iter()
        .chain(unlinked_keys(&answer["decided_against"]["isolated"]))
        .flatten();
    for key in isolated {
        let traced = on_this_repository(&["trace", &key]);
        assert_eq!(traced["upstream"], serde_json::json!([]), "{key}");
        assert_eq!(traced["downstream"], serde_json::json!([]), "{key}");
    }
}
