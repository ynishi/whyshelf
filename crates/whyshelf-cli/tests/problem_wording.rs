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
