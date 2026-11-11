//! The promises of the space `js-test` and its preset `js-test`, on the binary: a
//! verification whose `tests` name TypeScript tests by their titles is approved on
//! the text of their calls; an edit of another test in the same file leaves it
//! fresh, an edit of its own test makes it stale (`artifact_changed`), and a test
//! removed breaks it; a test in a file that does not lex cannot be told, and no
//! approval stands on it.
//!
//! The tests are a small TypeScript file of the project, read and lexed by the
//! binary: no program but the binary is run.

mod common;

use std::fs;

use common::{Project, json};

const BOOKS: &str = r#"[[book]]
name = "verification"
dir = "verifications"
key = { prefix = "V" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.link]]
name = "tests"
target = "artifact"
preset = "js-test"
"#;

const SOURCE: &str = r#"import { describe, it, expect } from "vitest";

describe("Cart", () => {
  // The items of a cart.
  it("adds an item", () => {
    expect(count([1, 2])).toBe(2);
  });

  it("keeps the order", () => {
    expect(order([2, 1])).toEqual([1, 2]);
  });
});
"#;

const NAME: &str = "tests/cart.test.ts::Cart::adds an item";

/// A project whose verification `V-1` names these tests.
fn project(name: &str, tests: &[&str]) -> Project {
    let tests: Vec<String> = tests.iter().map(|t| format!("\"{t}\"")).collect();
    let doc = format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"V-1\"\ntitle: \"Items are added\"\ntests: [{}]\n---\nThe body.\n",
        tests.join(", ")
    );
    let project = Project::with(name, BOOKS, "");
    fs::remove_dir_all(project.0.join("notes")).unwrap();
    fs::create_dir_all(project.0.join("verifications")).unwrap();
    fs::write(project.0.join("verifications/V-1.md"), doc).unwrap();
    fs::create_dir_all(project.0.join("tests")).unwrap();
    fs::write(project.0.join("tests/cart.test.ts"), SOURCE).unwrap();
    project
}

fn standing(project: &Project) -> (serde_json::Value, String) {
    let row = &json(&project.run(&["get", "V-1"]))["row"];
    (row["standing"].clone(), row["causes"].to_string())
}

fn edit(project: &Project, from: &str, to: &str) {
    let path = project.0.join("tests/cart.test.ts");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{from}");
    fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn a_verification_stands_on_the_text_of_the_test_calls_it_names() {
    let project = project("js-tests", &[NAME]);
    let map = json(&project.run(&["map"]));
    assert_eq!(map["links"][0]["preset"], "js-test");
    let check = json(&project.run(&["check"]));
    assert_eq!(check["clean"], true, "{check}");
    assert_eq!(check["absent"], serde_json::json!([]));
    assert_eq!(check["unknown"], serde_json::json!([]));

    assert_eq!(
        project.run(&["judge", "approve", "V-1"]).status.code(),
        Some(0)
    );
    assert_eq!(standing(&project).0, "fresh");

    // Another test of the file, and the comments and the spacing of this one.
    edit(&project, "toEqual([1, 2])", "toEqual([1, 2, 3])");
    edit(
        &project,
        "// The items of a cart.",
        "/* The items,\n said otherwise. */",
    );
    edit(
        &project,
        "  it(\"adds an item\", () => {",
        "  it(  \"adds an item\",\n    () =>   {  // why\n",
    );
    assert_eq!(standing(&project).0, "fresh");

    // Its own body.
    edit(&project, "toBe(2)", "toBe(3)");
    let (standing_now, causes) = standing(&project);
    assert_eq!(standing_now, "stale", "{causes}");
    assert!(causes.contains("artifact_changed"), "{causes}");
    assert!(causes.contains("adds an item"), "{causes}");

    // Approved again, then removed.
    assert_eq!(
        project.run(&["judge", "approve", "V-1"]).status.code(),
        Some(0)
    );
    assert_eq!(standing(&project).0, "fresh");
    edit(&project, "\"adds an item\"", "\"adds every item\"");
    let (standing_now, causes) = standing(&project);
    assert_eq!(standing_now, "broken", "{causes}");
    assert!(causes.contains("artifact_gone"), "{causes}");
}

#[test]
fn a_test_in_a_file_that_does_not_lex_is_unknown_and_no_approval_stands_on_it() {
    let project = project("js-tests-unlexed", &["tests/broken.test.ts::holds"]);
    fs::write(
        project.0.join("tests/broken.test.ts"),
        "it(\"holds\", () => {\n  expect('open).toBe(1);\n});\n",
    )
    .unwrap();
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    let check = json(&output);
    assert_eq!(check["clean"], true, "{check}");
    let unknown = &check["unknown"];
    assert_eq!(unknown[0]["name"], "tests/broken.test.ts::holds", "{check}");
    assert_eq!(unknown[0]["space"], "js-test");
    assert!(
        unknown[0]["why"]
            .as_str()
            .unwrap()
            .contains("does not lex as JavaScript or TypeScript"),
        "{check}"
    );

    let refused = project.run(&["judge", "approve", "V-1"]);
    assert_ne!(refused.status.code(), Some(0));
    assert_eq!(standing(&project).0, "pending");
}

#[test]
fn a_text_out_of_the_grammar_makes_the_verification_invalid() {
    let project = project("js-tests-no-name", &["tests/cart.test.ts"]);
    let check = json(&project.run(&["check"]));
    assert_eq!(check["clean"], false, "{check}");
    assert!(check.to_string().contains("no test name"), "{check}");
}
