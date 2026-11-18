//! The words a problem says, on the binary: a count, a type and a book are named as
//! the declaration names them, and a word the declaration does not have is a
//! problem of the declaration (exit 1, `schema_problems`), not a failure to read it.

mod common;

use common::{BOOKS, NOTE, Project, json};

/// `BOOKS` with an integer field `size` and a link `grounds` of the count `optional`.
fn books_with_a_count() -> String {
    format!(
        "{BOOKS}[[book.field]]\nname = \"size\"\ntype = \"integer\"\n\
         [[book.link]]\nname = \"grounds\"\ntarget = \"note\"\ncount = \"optional\"\nmeans = \"depends\"\n"
    )
}

/// The problems `new note --title X` with these arguments answers (exit 2).
fn new_problems(project: &Project, args: &[&str]) -> Vec<String> {
    let mut command = vec!["new", "note", "--title", "X"];
    command.extend_from_slice(args);
    let output = project.run(&command);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer = json(&output);
    answer["problems"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of problems: {answer}"))
        .iter()
        .map(|p| p.as_str().expect("a text").to_owned())
        .collect()
}

#[test]
fn a_problem_says_the_count_the_type_and_the_book_as_the_declaration_writes_them() {
    let project = Project::with("wording", &books_with_a_count(), NOTE);

    let problems = new_problems(&project, &["grounds+=N-1", "grounds+=N-2"]);
    assert!(
        problems.iter().any(|p| p
            == "the link `grounds` has 2 keys, which its count `optional` (none or one) does not admit"),
        "{problems:?}"
    );

    let problems = new_problems(&project, &["size=big"]);
    assert!(
        problems
            .iter()
            .any(|p| p == "the field `size` is a text, not `integer`"),
        "{problems:?}"
    );

    let problems = new_problems(&project, &["source=N-1"]);
    assert!(
        problems.iter().any(|p| p
            == "the book `note` declares no field or link `source`: declare it in the book first"),
        "{problems:?}"
    );
    for problem in &problems {
        assert!(
            !problem.contains("Optional") && !problem.contains("Text"),
            "{problem}"
        );
    }
}

#[test]
fn a_word_of_a_count_the_declaration_does_not_have_is_a_problem_of_the_declaration() {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"grounds\"\ntarget = \"note\"\ncount = \"several\"\nmeans = \"depends\"\n"
    );
    let project = Project::with("unknown-count", &books, NOTE);
    let output = project.run(&["check"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer = json(&output);
    assert_eq!(answer["clean"], false, "{answer}");
    assert_eq!(
        answer["schema_problems"],
        serde_json::json!([
            "book `note`: the count of the link `grounds` is \"several\", and it takes one of \
             \"one\", \"optional\", \"many\", \"at-least-one\""
        ])
    );
}

/// What `check` answers (exit 1) for this declaration: its problems of the declaration.
fn schema_problems_of(name: &str, books: &str) -> serde_json::Value {
    let project = Project::with(name, books, NOTE);
    let output = project.run(&["check"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer = json(&output);
    assert_eq!(answer["clean"], false, "{answer}");
    answer["schema_problems"].clone()
}

#[test]
fn a_word_of_a_key_the_declaration_does_not_have_is_a_problem_of_the_declaration() {
    let books = "[[book]]\nname = \"note\"\ndir = \"notes\"\nkey = \"uuid\"\n";
    assert_eq!(
        schema_problems_of("unknown-key-word", books),
        serde_json::json!([
            "book `note`: `key` is \"uuid\", and it takes \"slug\" or a table { prefix = \"...\" }"
        ])
    );
}

#[test]
fn a_word_of_without_front_matter_the_declaration_does_not_have_is_a_problem_of_the_declaration() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\nwithout_front_matter = \"maybe\"\n",
    );
    assert_eq!(
        schema_problems_of("unknown-front-matter-word", &books),
        serde_json::json!([
            "book `note`: `without_front_matter` is \"maybe\", and it takes \"invalid\" or \"skip\""
        ])
    );
}

#[test]
fn a_key_that_is_neither_a_word_nor_a_table_still_fails_the_read_of_the_declaration() {
    for (name, key) in [("key-number", "3"), ("key-list", "[]")] {
        let books = format!("[[book]]\nname = \"note\"\ndir = \"notes\"\nkey = {key}\n");
        let project = Project::with(name, &books, NOTE);
        let output = project.run(&["check"]);
        assert_eq!(
            output.status.code(),
            Some(3),
            "key = {key}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
