//! A renamed module declared once, on the binary: `judge moved --prefix` moves every
//! test name under the module's path at once, and the approval stands again; one name
//! left behind in the old module is refused by name and writes nothing; a prefix
//! nothing pinned is under, or two that are one, is a request.
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

const OLD: &[&str] = &[
    "tests/cache.rs::tests::expires",
    "tests/cache.rs::tests::keeps",
];

const NEW: &[&str] = &[
    "tests/cache.rs::checks::expires",
    "tests/cache.rs::checks::keeps",
];

/// The verification `V-1`, naming these tests.
fn doc(tests: &[&str]) -> String {
    let tests: Vec<String> = tests.iter().map(|t| format!("\"{t}\"")).collect();
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"V-1\"\ntitle: \"Entries expire\"\ntests: [{}]\n---\nThe body.\n",
        tests.join(", ")
    )
}

/// A project whose `V-1` names both tests of the module `tests`, approved.
fn approved(name: &str) -> Project {
    let project = Project::with(name, BOOKS, "");
    fs::remove_dir_all(project.0.join("notes")).unwrap();
    fs::create_dir_all(project.0.join("verifications")).unwrap();
    fs::write(project.0.join("verifications/V-1.md"), doc(OLD)).unwrap();
    fs::create_dir_all(project.0.join("tests")).unwrap();
    fs::write(project.0.join("tests/cache.rs"), SOURCE).unwrap();
    let out = project.run(&["judge", "approve", "V-1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    project
}

fn source(project: &Project, text: &str) {
    fs::write(project.0.join("tests/cache.rs"), text).unwrap();
}

fn relink(project: &Project, tests: &[&str]) {
    fs::write(project.0.join("verifications/V-1.md"), doc(tests)).unwrap();
}

fn standing(project: &Project) -> serde_json::Value {
    json(&project.run(&["get", "V-1"]))["row"]["standing"].clone()
}

fn declare(project: &Project, from: &str, to: &str) -> std::process::Output {
    project.run(&[
        "judge",
        "moved",
        "--prefix",
        from,
        to,
        "--reason",
        "the module was renamed",
    ])
}

#[test]
fn a_renamed_module_is_declared_once_and_the_approval_stands_again() {
    let project = approved("moved-prefix");
    source(&project, &SOURCE.replace("mod tests", "mod checks"));
    relink(&project, NEW);
    assert_eq!(standing(&project), "stale");

    let out = declare(&project, "tests/cache.rs::tests", "tests/cache.rs::checks");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let done = json(&out);
    let moved = done["moved"].as_array().unwrap();
    // One declaration for each test, in the order of the names, each its own record.
    let names: Vec<(&str, &str, &str)> = moved
        .iter()
        .map(|m| {
            (
                m["space"].as_str().unwrap(),
                m["from"].as_str().unwrap(),
                m["to"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            (
                "rust-test",
                "tests/cache.rs::tests::expires",
                "tests/cache.rs::checks::expires"
            ),
            (
                "rust-test",
                "tests/cache.rs::tests::keeps",
                "tests/cache.rs::checks::keeps"
            ),
        ]
    );
    assert_eq!(moved[0]["record"]["kind"], "moved");
    assert_ne!(moved[0]["record"]["id"], moved[1]["record"]["id"]);
    assert_eq!(done["not_asked"], serde_json::json!([]));

    assert_eq!(standing(&project), "fresh");
    let stale = project.run(&["stale", "--exit-code"]);
    assert!(
        stale.status.success(),
        "{}",
        String::from_utf8_lossy(&stale.stdout)
    );

    // Each declaration is one record, taken back by its identity: the way back.
    let id = moved[0]["record"]["id"].as_str().unwrap();
    let out = project.run(&["judge", "withdraw", id, "--reason", "a mistake"]);
    assert!(out.status.success());
    assert_eq!(standing(&project), "stale");
}

#[test]
fn a_test_left_in_the_old_module_is_refused_by_name_and_nothing_is_written() {
    let project = approved("moved-prefix-left");
    // `expires` moved; `keeps` is still in the module it was in, and so still at its
    // old name.
    source(
        &project,
        r#"#[cfg(test)]
mod checks {
    #[test]
    fn expires() {
        assert!(expired(10));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps() {
        assert!(!expired(0));
    }
}
"#,
    );
    let before = project.files();
    let out = declare(&project, "tests/cache.rs::tests", "tests/cache.rs::checks");
    assert_eq!(out.status.code(), Some(1), "{}", json(&out));
    let error = json(&out);
    assert_eq!(error["error"], "refused");
    let refused = error["refused"].as_array().unwrap();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["name"], "tests/cache.rs::tests::keeps");
    assert_eq!(refused[0]["space"], "rust-test");
    assert!(
        refused[0]["why"]
            .as_str()
            .unwrap()
            .contains("is still there"),
        "{refused:?}"
    );
    assert_eq!(project.files(), before, "a refused move writes no record");
}

#[test]
fn nothing_under_the_prefix_and_two_prefixes_that_are_one_are_requests() {
    let project = approved("moved-prefix-request");
    let before = project.files();
    let out = declare(&project, "src/a.rs::m", "src/a.rs::n");
    assert_eq!(out.status.code(), Some(2));
    let error = json(&out);
    assert_eq!(error["error"], "request");
    assert_eq!(error["refused"], serde_json::json!([]));
    assert!(
        error["messages"]
            .to_string()
            .contains("under `src/a.rs::m`"),
        "{error}"
    );

    let out = declare(&project, "tests/cache.rs::tests", "tests/cache.rs::tests");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(project.files(), before);
}

#[test]
fn the_answer_of_a_prefix_move_has_its_schema() {
    let project = approved("moved-prefix-schema");
    let schema = json(&project.run(&["schema"]));
    let out = &schema["outputs"]["judge moved --prefix"];
    assert_eq!(out["properties"]["moved"]["type"], "array", "{out}");
    assert!(out["properties"]["not_asked"].is_object(), "{out}");
}
