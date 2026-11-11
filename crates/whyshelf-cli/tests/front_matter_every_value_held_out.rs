//! Held-out cases of telling every value a front matter does not hold: other kinds,
//! a number too large beside another value, a negative number beyond the parser, a
//! file with only one such value, and `fmt` telling every one.

mod common;

use common::{BOOKS, Project, json};

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

fn note(extra: &str) -> String {
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n{extra}---\nThe body.\n"
    )
}

#[test]
fn other_kinds_are_each_told() {
    let project = Project::with(
        "held-kinds",
        BOOKS,
        &note("ratio: 1.5\nnested:\n  a: 1\nmarked: !tag x\nflags: [true]\n"),
    );
    let told = problems(&json(&project.run(&["check"])), "notes/N-1.md");
    assert_eq!(told.len(), 4, "{told:#?}");
    for key in ["ratio", "nested", "marked", "flags"] {
        assert!(
            told.iter().any(|p| p.contains(&format!("`{key}`"))),
            "{key}: {told:#?}"
        );
    }
}

#[test]
fn a_number_beyond_the_parser_is_told_beside_another_value() {
    let project = Project::with(
        "held-huge",
        BOOKS,
        &note("huge: 99999999999999999999999\nempty:\n"),
    );
    let told = problems(&json(&project.run(&["check"])), "notes/N-1.md");
    assert_eq!(told.len(), 2, "{told:#?}");
    assert!(
        told.iter()
            .any(|p| p.contains("`huge` is a whole number above the largest one")),
        "{told:#?}"
    );
    assert!(
        told.iter().any(|p| p.contains("`empty` is no value")),
        "{told:#?}"
    );
    assert!(
        told.iter().all(|p| !p.contains("not valid YAML")),
        "{told:#?}"
    );
}

#[test]
fn a_negative_number_beyond_the_parser_is_told_as_too_small() {
    let project = Project::with(
        "held-negative",
        BOOKS,
        &note("low: -99999999999999999999999\n"),
    );
    let told = problems(&json(&project.run(&["check"])), "notes/N-1.md");
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains("`low` is a whole number below the smallest one"),
        "{told:#?}"
    );
    assert!(!told[0].contains("not valid YAML"), "{told:#?}");
}

#[test]
fn one_value_is_told_once_and_a_held_value_not_at_all() {
    let project = Project::with(
        "held-one",
        BOOKS,
        &note("count: 9223372036854775807\nempty:\n"),
    );
    let told = problems(&json(&project.run(&["check"])), "notes/N-1.md");
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(told[0].contains("`empty` is no value"), "{told:#?}");
}

#[test]
fn fmt_tells_every_value_of_a_file_it_leaves_alone() {
    let project = Project::with(
        "held-fmt",
        BOOKS,
        &note("big: 9223372036854775808\nempty:\n"),
    );
    let answer = json(&project.run(&["fmt", "--check"]));
    let left = answer["left_alone"].as_array().expect("left_alone");
    let detail = left
        .iter()
        .find(|entry| entry["file"] == "notes/N-1.md")
        .unwrap_or_else(|| panic!("left alone: {answer}"))["detail"]
        .as_str()
        .expect("a detail")
        .to_owned();
    assert!(detail.contains("`big`"), "{detail}");
    assert!(detail.contains("`empty`"), "{detail}");
}
