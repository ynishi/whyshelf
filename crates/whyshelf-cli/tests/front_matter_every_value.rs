//! The binary on a project whose front matters hold values of kinds a front matter
//! does not hold: `check` tells every such value, each with its key and its kind,
//! and a whole number too large for the parser is told as a number too large, not
//! as YAML that is not valid.

mod common;

use common::{BOOKS, Project, json};

/// The problems `check` tells for the file `file`.
fn problems(answer: &serde_json::Value, file: &str) -> Vec<String> {
    let invalid = answer["health"]["invalid"]
        .as_array()
        .expect("a list of invalid files");
    let entry = invalid
        .iter()
        .find(|entry| entry["file"] == file)
        .unwrap_or_else(|| panic!("{file} is invalid: {answer}"));
    entry["problems"]
        .as_array()
        .expect("a list of problems")
        .iter()
        .map(|problem| problem.as_str().expect("a problem is a text").to_owned())
        .collect()
}

#[test]
fn every_value_a_front_matter_does_not_hold_is_told() {
    let note = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nbig: 9223372036854775808\nempty:\ntags:\n  - a: b\n---\nThe body.\n";
    let project = Project::with("every-value", BOOKS, note);
    let answer = json(&project.run(&["check"]));
    let told = problems(&answer, "notes/N-1.md");
    assert_eq!(told.len(), 3, "{told:#?}");
    for (key, kind) in [
        ("big", "a whole number above the largest one"),
        ("empty", "no value"),
        ("tags", "a list with an item that is a mapping"),
    ] {
        assert!(
            told.iter()
                .any(|problem| problem.contains(&format!("`{key}` is {kind}"))),
            "`{key}` as {kind}: {told:#?}"
        );
    }
}

#[test]
fn a_number_too_large_for_the_parser_is_told_as_too_large() {
    let note = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nhuge: 18446744073709551616\n---\nThe body.\n";
    let project = Project::with("too-large", BOOKS, note);
    let answer = json(&project.run(&["check"]));
    let told = problems(&answer, "notes/N-1.md");
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains("`huge` is a whole number above the largest one"),
        "{told:#?}"
    );
    assert!(!told[0].contains("not valid YAML"), "{told:#?}");
}
