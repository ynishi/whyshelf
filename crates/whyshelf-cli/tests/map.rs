//! `map`, on the binary and a small project of files: it answers the books, the links
//! and their drawing, and exits 1 when the declaration cannot be used, with the
//! problems `check` gives.

mod common;

use common::{BOOKS, NOTE, Project, json};

#[test]
fn map_answers_the_books_the_links_and_their_drawing() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\ndescription = \"what was noted\"\n",
    ) + "[[book.link]]\nname = \"see\"\ntarget = \"note\"\nmeans = \"informs\"\n";
    let project = Project::with("map", &books, NOTE);
    let output = project.run(&["map"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["schema_problems"], serde_json::json!([]));
    assert_eq!(answer["books"][0]["name"], "note");
    assert_eq!(answer["books"][0]["description"], "what was noted");
    assert_eq!(answer["links"][0]["name"], "see");
    assert_eq!(answer["links"][0]["target"]["book"], "note");
    assert_eq!(
        answer["diagram"],
        "note  N-  what was noted\n└─ see  informs  0..  ──► note (own book)\n"
    );
}

#[test]
fn map_of_a_declaration_that_cannot_be_used_is_its_problems_as_check_gives_them() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\ndescription = \"\"\"\ntwo\nlines\"\"\"\n",
    );
    let project = Project::with("map-unusable", &books, NOTE);
    let check = project.run(&["check"]);
    let output = project.run(&["map"]);
    assert_eq!(output.status.code(), check.status.code());
    assert_eq!(output.status.code(), Some(1));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["schema_problems"], json(&check)["schema_problems"]);
    assert!(
        answer["schema_problems"][0]
            .as_str()
            .unwrap()
            .contains("more than one line"),
        "{answer}"
    );
    assert_eq!(answer["diagram"], serde_json::Value::Null);
    assert_eq!(answer["books"], serde_json::json!([]));
}
