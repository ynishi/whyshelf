//! The built docs of the space `rust-item` held against the sources they were built
//! from, on the binary and on the docs the real `cargo doc` writes: docs built before
//! an item was added, or before one was removed, tell that item neither there nor gone
//! but that the docs are not of the sources as they are (`kind: "not_of_sources"`, the
//! file that differs, and what to do), and once the docs are built again as that says,
//! the item is there, or gone.
//!
//! This file runs `cargo doc` (offline, `--no-deps`) on a tiny crate with no
//! dependency, written into a temporary directory by each test, and builds into a
//! temporary directory of its own; `RUSTFLAGS`, `RUSTDOCFLAGS` and `RUSTDOC` are taken
//! out of what it runs, so no setting of the machine changes the pages. No generated
//! page is kept.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{Project, json};

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

const CARGO_TOML: &str =
    "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n";

const SOURCE: &str = "//! The demo crate.\n\n/// A shelf of books.\npub struct Shelf;\n";

fn note(items: &[&str]) -> String {
    let items: Vec<String> = items.iter().map(|i| format!("\"{i}\"")).collect();
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nitems: [{}]\n---\nThe body.\n",
        items.join(", ")
    )
}

/// A project whose note links to these items, with the crate `demo` at its root and
/// the toolchain file of this repository beside it; its docs are built into `build`.
fn project(name: &str, items: &[&str]) -> Project {
    let project = Project::with(name, BOOKS, &note(items));
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::write(project.0.join("Cargo.toml"), CARGO_TOML).unwrap();
    fs::write(project.0.join("src/lib.rs"), SOURCE).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-toolchain.toml"),
        project.0.join("rust-toolchain.toml"),
    )
    .unwrap();
    project
}

/// Build the docs of the crate, as the hint of the answer says: nothing of the
/// machine's flags.
fn cargo_doc(project: &Project) {
    let cargo = std::env::var("CARGO").expect("cargo sets CARGO for the tests it runs");
    let output = Command::new(cargo)
        .args(["doc", "--offline", "--no-deps"])
        .current_dir(&project.0)
        .env("CARGO_TARGET_DIR", project.0.join("build"))
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .env_remove("RUSTDOC")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTDOCFLAGS")
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo doc failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(project: &Project, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&project.0)
        .args(args)
        .env("WHYSHELF_ACTOR", "tester")
        .env("CARGO_TARGET_DIR", project.0.join("build"))
        .output()
        .expect("the binary runs")
}

fn approve(project: &Project) {
    let output = run(project, &["judge", "approve", "N-1"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The source of the crate with `from` replaced by `to`; the docs are not built again.
fn edit(project: &Project, from: &str, to: &str) {
    let path = project.0.join("src/lib.rs");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{from}");
    fs::write(path, text.replace(from, to)).unwrap();
}

/// The names `check` cannot tell are `names`, none absent, each because the docs are
/// not of the sources: the file of the crate that differs, what was read and what to
/// do.
fn assert_not_of_sources(project: &Project, names: &[&str]) {
    let check = json(&run(project, &["check"]));
    assert_eq!(check["absent"], serde_json::json!([]), "{check}");
    let unknown = check["unknown"].as_array().unwrap();
    let told: Vec<&str> = unknown
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert_eq!(told, names, "{check}");
    for one in unknown {
        assert_eq!(one["kind"], "not_of_sources", "{check}");
        assert_eq!(one["reason"], "differs", "{check}");
        assert_eq!(one["file"], "src/lib.rs", "{check}");
        assert!(
            one["read"]
                .as_str()
                .unwrap()
                .starts_with("the built docs at "),
            "{check}"
        );
        assert!(
            one["hint"].as_str().unwrap().contains("cargo doc"),
            "{check}"
        );
        assert!(
            one["why"]
                .as_str()
                .unwrap()
                .contains("is not of the sources as they are"),
            "{check}"
        );
    }
    // The gate cannot say the approval holds.
    assert_eq!(
        run(project, &["stale", "--exit-code"]).status.code(),
        Some(1)
    );
}

#[test]
fn an_item_added_after_the_docs_were_built_is_not_told_absent_and_is_there_once_they_are_built_again()
 {
    let project = project("rust-item-sources-added", &["demo::Shelf", "demo::Added"]);
    cargo_doc(&project);
    let check = json(&run(&project, &["check"]));
    assert_eq!(check["absent"][0]["name"], "demo::Added", "{check}");
    approve(&project);

    edit(
        &project,
        "pub struct Shelf;\n",
        "pub struct Shelf;\n\n/// Added.\npub struct Added;\n",
    );
    // Neither name is told, the one there before either.
    assert_not_of_sources(&project, &["demo::Shelf", "demo::Added"]);

    cargo_doc(&project);
    let check = json(&run(&project, &["check"]));
    assert_eq!(check["unknown"], serde_json::json!([]), "{check}");
    assert_eq!(check["absent"], serde_json::json!([]), "{check}");
    let row = &json(&run(&project, &["get", "N-1"]))["row"];
    assert!(
        row["causes"].to_string().contains("artifact_arrived"),
        "{row}"
    );
}

#[test]
fn an_item_removed_after_the_docs_were_built_is_not_told_there_and_is_gone_once_they_are_built_again()
 {
    let project = project("rust-item-sources-removed", &["demo::Shelf"]);
    cargo_doc(&project);
    approve(&project);

    edit(&project, "/// A shelf of books.\npub struct Shelf;\n", "");
    assert_not_of_sources(&project, &["demo::Shelf"]);
    let row = &json(&run(&project, &["get", "N-1"]))["row"];
    assert_eq!(row["standing"], "fresh", "{row}");

    cargo_doc(&project);
    let check = json(&run(&project, &["check"]));
    assert_eq!(check["absent"][0]["name"], "demo::Shelf", "{check}");
    assert_eq!(check["unknown"], serde_json::json!([]), "{check}");
    let row = &json(&run(&project, &["get", "N-1"]))["row"];
    assert_eq!(row["standing"], "broken", "{row}");
}
