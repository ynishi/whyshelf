//! The promises of the space `rust-item` and its preset `rustdoc`, on the binary: a
//! link that uses the preset tells a path its docs do not have as absent and one of
//! a crate whose docs are not built as unknown, each on stderr too; it refuses an
//! approval that would pin the latter, keeps an approval through an edit of an
//! item's docs, and breaks it when the item goes away; `stale --exit-code` fails
//! while an approval stands on a name that cannot be told.
//!
//! The docs are a small tree of HTML laid out as rustdoc writes it, in a build
//! directory `CARGO_TARGET_DIR` names, so no crate is built and no program but the
//! binary is run.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const BOOKS: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
[[book.link]]
name = "items"
target = "artifact"
preset = "rustdoc"
"#;

const GENERATOR: &str = "<meta name=\"generator\" content=\"rustdoc\">";

/// A note whose link names these paths.
fn note(items: &[&str]) -> String {
    let items: Vec<String> = items.iter().map(|i| format!("\"{i}\"")).collect();
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nitems: [{}]\n---\nThe body.\n",
        items.join(", ")
    )
}

/// A page of rustdoc with these anchors of members.
fn page(ids: &[&str], text: &str) -> String {
    let anchors: String = ids
        .iter()
        .map(|id| format!("<section id=\"{id}\"></section>"))
        .collect();
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR}</head><body><p>{text}</p>{anchors}</body></html>"
    )
}

/// A project and the docs of a crate `demo` in a build directory of its own;
/// removed when it goes out of scope.
struct Project(PathBuf);

/// How many projects this process has made: with the test's name, it keeps two
/// tests that run at once (in the same tick of a coarse clock) out of one directory.
static MADE: AtomicUsize = AtomicUsize::new(0);

impl Project {
    /// The project of the test `test`, whose note links to these items.
    fn new(test: &str, items: &[&str]) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let count = MADE.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "whyshelf-rust-items-{test}-{}-{count}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join(".whyshelf")).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::create_dir_all(dir.join("build/doc/demo")).unwrap();
        fs::write(dir.join(".whyshelf/books.toml"), BOOKS).unwrap();
        fs::write(dir.join("notes/N-1.md"), note(items)).unwrap();
        fs::write(dir.join("build/doc/demo/index.html"), page(&[], "demo")).unwrap();
        let project = Self(dir);
        project.shelf(&["method.build"], "A shelf.");
        project
    }

    /// The page of the struct `demo::Shelf`, with these members.
    fn shelf(&self, ids: &[&str], text: &str) {
        fs::write(
            self.0.join("build/doc/demo/struct.Shelf.html"),
            page(ids, text),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_whyshelf"))
            .arg("--root")
            .arg(&self.0)
            .args(args)
            .env("WHYSHELF_ACTOR", "tester")
            .env("CARGO_TARGET_DIR", self.0.join("build"))
            .output()
            .expect("the binary runs")
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        json(&self.run(args))
    }

    fn standing(&self) -> (serde_json::Value, String) {
        let row = &self.json(&["get", "N-1"])["row"];
        (row["standing"].clone(), row["causes"].to_string())
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn names(list: &serde_json::Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_owned())
        .collect()
}

fn stderr(output: &Output) -> Vec<String> {
    String::from_utf8(output.stderr.clone())
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_link_of_the_preset_rustdoc_stands_on_the_items_being_there() {
    let project = Project::new(
        "preset",
        &["demo::Shelf", "demo::Shelf::build", "demo::Missing"],
    );
    let map = project.json(&["map"]);
    assert_eq!(map["links"][0]["preset"], "rustdoc");

    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    let check = json(&output);
    assert_eq!(check["clean"], true, "{check}");
    assert_eq!(names(&check["absent"]), ["demo::Missing"]);
    assert_eq!(check["absent"][0]["space"], "rust-item");
    assert_eq!(check["unknown"], serde_json::json!([]));
    assert_eq!(
        stderr(&output),
        ["warning: note:N-1 names demo::Missing in rust-item, which has nothing behind it"]
    );

    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    assert_eq!(project.standing().0, "fresh");
    assert_eq!(
        project.run(&["stale", "--exit-code"]).status.code(),
        Some(0)
    );

    // An edit of the docs of an item changes nothing.
    project.shelf(&["method.build"], "A shelf, said otherwise.");
    assert_eq!(project.standing().0, "fresh");

    // An item that goes away breaks the approval.
    project.shelf(&[], "A shelf.");
    let (standing, causes) = project.standing();
    assert_eq!(standing, "broken", "{causes}");
    assert!(causes.contains("artifact_gone"), "{causes}");
    assert!(causes.contains("demo::Shelf::build"), "{causes}");
}

#[test]
fn a_crate_whose_docs_are_not_built_is_unknown_and_no_approval_stands_on_it() {
    let project = Project::new("not-built", &["demo::Shelf", "other::Item"]);
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    let check = json(&output);
    assert_eq!(check["clean"], true, "{check}");
    assert_eq!(names(&check["unknown"]), ["other::Item"]);
    assert_eq!(check["absent"], serde_json::json!([]));
    let lines = stderr(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with(
            "warning: note:N-1 names other::Item in rust-item, which cannot be told: the docs of the crate `other` are not built"
        ),
        "{lines:?}"
    );

    let refused = project.run(&["judge", "approve", "N-1"]);
    assert_ne!(refused.status.code(), Some(0));
    assert_eq!(project.standing().0, "pending");
}

#[test]
fn the_gate_fails_while_an_approval_stands_on_a_name_that_cannot_be_told() {
    let project = Project::new("gate", &["demo::Shelf"]);
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    // The docs are not rustdoc's any more: nothing of the crate can be told.
    fs::write(
        project.0.join("build/doc/demo/index.html"),
        "<html><body>not rustdoc</body></html>",
    )
    .unwrap();
    // Its freshness is unchanged; the gate cannot say it holds.
    assert_eq!(project.standing().0, "fresh");
    let plain = project.run(&["stale"]);
    assert_eq!(plain.status.code(), Some(0));
    let gate = project.run(&["stale", "--exit-code"]);
    assert_eq!(gate.status.code(), Some(1));
    let answer = json(&gate);
    assert_eq!(answer["stale"], serde_json::json!([]));
    // The answer names what the gate could not tell, as `check` does.
    let unknown = &answer["unknown"];
    assert_eq!(names(unknown), ["demo::Shelf"], "{answer}");
    assert_eq!(unknown[0]["space"], "rust-item");
    assert_eq!(unknown[0]["link"], "items");
    assert_eq!(unknown[0]["doc"]["key"], "note:N-1");
    assert!(
        unknown[0]["why"]
            .as_str()
            .unwrap()
            .contains("is not rustdoc's"),
        "{answer}"
    );
    assert_eq!(json(&plain)["unknown"], *unknown);
    let lines = stderr(&gate);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with(
            "warning: note:N-1 pins demo::Shelf in rust-item, which cannot be told now"
        ),
        "{lines:?}"
    );
    assert!(lines[0].contains("is not rustdoc's"), "{lines:?}");
}

#[test]
fn a_text_that_is_no_rust_path_makes_the_document_invalid() {
    let project = Project::new("no-path", &["demo::Vec<T>"]);
    let check = project.json(&["check"]);
    assert_eq!(check["clean"], false, "{check}");
    assert!(check.to_string().contains("no Rust path"), "{check}");
}
