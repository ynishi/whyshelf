//! The promises of the space `rust-test` and its preset `test`, on the binary: a
//! verification whose `tests` name test functions is approved on their text; an
//! edit of another test in the same file leaves it fresh, an edit of its own test
//! makes it stale (`artifact_changed`), and a test removed breaks it; a test in a
//! file that does not parse cannot be told, and no approval stands on it.
//!
//! The tests are a small Rust file of the project, read and parsed by the binary:
//! no program but the binary is run.

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
preset = "test"
"#;

const SOURCE: &str = r#"#[cfg(test)]
mod tests {
    /// The entry expires.
    #[test]
    fn expires() {
        assert!(expired(10));
    }

    #[test]
    fn keeps() {
        assert!(!expired(0));
    }
}
"#;

/// A project whose verification `V-1` names these tests.
fn project(name: &str, tests: &[&str]) -> Project {
    let tests: Vec<String> = tests.iter().map(|t| format!("\"{t}\"")).collect();
    let doc = format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"V-1\"\ntitle: \"Entries expire\"\ntests: [{}]\n---\nThe body.\n",
        tests.join(", ")
    );
    let project = Project::with(name, BOOKS, "");
    fs::remove_dir_all(project.0.join("notes")).unwrap();
    fs::create_dir_all(project.0.join("verifications")).unwrap();
    fs::write(project.0.join("verifications/V-1.md"), doc).unwrap();
    fs::create_dir_all(project.0.join("tests")).unwrap();
    fs::write(project.0.join("tests/cache.rs"), SOURCE).unwrap();
    project
}

fn standing(project: &Project) -> (serde_json::Value, String) {
    let row = &json(&project.run(&["get", "V-1"]))["row"];
    (row["standing"].clone(), row["causes"].to_string())
}

fn edit(project: &Project, from: &str, to: &str) {
    let path = project.0.join("tests/cache.rs");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{from}");
    fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn a_verification_stands_on_the_text_of_the_tests_it_names() {
    let project = project("rust-tests", &["tests/cache.rs::tests::expires"]);
    let map = json(&project.run(&["map"]));
    assert_eq!(map["links"][0]["preset"], "test");
    let check = json(&project.run(&["check"]));
    assert_eq!(check["clean"], true, "{check}");
    assert_eq!(check["absent"], serde_json::json!([]));
    assert_eq!(check["unknown"], serde_json::json!([]));

    assert_eq!(
        project.run(&["judge", "approve", "V-1"]).status.code(),
        Some(0)
    );
    assert_eq!(standing(&project).0, "fresh");

    // Another test of the file, and the docs and the spacing of this one.
    edit(&project, "assert!(!expired(0));", "assert!(!expired(1));");
    edit(
        &project,
        "/// The entry expires.",
        "/// An entry expires, said otherwise.",
    );
    edit(
        &project,
        "    fn expires() {",
        "    fn  expires( )  {  // why\n",
    );
    assert_eq!(standing(&project).0, "fresh");

    // Its own body.
    edit(&project, "assert!(expired(10));", "assert!(expired(11));");
    let (standing_now, causes) = standing(&project);
    assert_eq!(standing_now, "stale", "{causes}");
    assert!(causes.contains("artifact_changed"), "{causes}");
    assert!(
        causes.contains("tests/cache.rs::tests::expires"),
        "{causes}"
    );
    // The text is made, not stored: `diff` says it changed and has nothing to set
    // beside it.
    let diff = json(&project.run(&["diff", "V-1"]));
    let item = &diff["items"][1];
    assert_eq!(item["changed"], true, "{diff}");
    assert_eq!(item["retrieval"], "no_address", "{diff}");

    // Approved again, then removed.
    assert_eq!(
        project.run(&["judge", "approve", "V-1"]).status.code(),
        Some(0)
    );
    assert_eq!(standing(&project).0, "fresh");
    edit(&project, "fn  expires", "fn expires_soon");
    let (standing_now, causes) = standing(&project);
    assert_eq!(standing_now, "broken", "{causes}");
    assert!(causes.contains("artifact_gone"), "{causes}");
}

#[test]
fn a_test_in_a_file_that_does_not_parse_is_unknown_and_no_approval_stands_on_it() {
    let project = project("rust-tests-unparsed", &["tests/broken.rs::holds"]);
    fs::write(project.0.join("tests/broken.rs"), "#[test]\nfn holds() {\n").unwrap();
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    let check = json(&output);
    assert_eq!(check["clean"], true, "{check}");
    let unknown = &check["unknown"];
    assert_eq!(unknown[0]["name"], "tests/broken.rs::holds", "{check}");
    assert_eq!(unknown[0]["space"], "rust-test");
    assert!(
        unknown[0]["why"]
            .as_str()
            .unwrap()
            .contains("does not parse as Rust"),
        "{check}"
    );

    let refused = project.run(&["judge", "approve", "V-1"]);
    assert_ne!(refused.status.code(), Some(0));
    assert_eq!(standing(&project).0, "pending");
}

#[test]
fn a_text_out_of_the_grammar_makes_the_verification_invalid() {
    let project = project("rust-tests-no-name", &["tests/cache.rs"]);
    let check = json(&project.run(&["check"]));
    assert_eq!(check["clean"], false, "{check}");
    assert!(check.to_string().contains("no test name"), "{check}");
}
