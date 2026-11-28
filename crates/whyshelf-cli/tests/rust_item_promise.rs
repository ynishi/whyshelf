//! The preset `rustdoc-promise` of the space `rust-item`, on the binary and on the
//! docs the real `cargo doc` writes: an approval of a link to items stands on what
//! each item promises (its declaration and its docs), so an edit of a body, a new
//! method or a new impl leaves it fresh, and an edit of the docs, of a signature, of
//! the docs of a variant's field, or docs taken away makes it stale
//! (`artifact_changed`); a link that changes from the preset `rustdoc` to this one
//! changes its comparison (`artifact_comparison_changed`).
//!
//! This file runs `cargo doc` (offline, `--no-deps`) on a tiny crate with no
//! dependency, written into a temporary directory by each test, and builds into a
//! temporary directory of its own; `RUSTFLAGS`, `RUSTDOCFLAGS` and `RUSTDOC` are
//! taken out of what it runs, so no setting of the machine changes the pages. The
//! toolchain file of this repository is copied beside the crate, and the rustdoc that
//! wrote the pages must be of the channel it names (a test fails otherwise: the
//! layout of the pinned toolchain would not be the one watched). No generated page is
//! kept.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
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
preset = "rustdoc-promise"
"#;

/// Every kind of item and of member the layout of a page has: a crate, a module, a
/// function, a struct with a field and a method, an enum with a variant that has a
/// field, and a method of a trait.
const ITEMS: [&str; 8] = [
    "demo",
    "demo::inner",
    "demo::free",
    "demo::Shelf",
    "demo::Shelf::count",
    "demo::Shelf::put",
    "demo::Cause::Moved",
    "demo::Source::resolve",
];

const CARGO_TOML: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n[lib]\npath = \"src/lib.rs\"\n";

const SOURCE: &str = r#"//! The demo crate: a shelf of books.

/// A shelf of books.
pub struct Shelf {
    /// How many books it holds.
    pub count: u32,
}

impl Shelf {
    /// Puts a book on the shelf.
    pub fn put(&mut self, n: u32) -> u32 {
        self.count += n;
        self.count
    }
}

/// Where a book went.
pub enum Cause {
    /// It is gone.
    Gone,
    /// It moved.
    Moved {
        /// Where it was.
        from: String,
    },
}

/// A source of books.
pub trait Source {
    /// Resolves a book.
    fn resolve(&self) -> u32;
}

/// A free function.
pub fn free() -> u32 {
    1
}

/// A module of the crate.
pub mod inner {
    /// A constant.
    pub const MAX: u32 = 1;
}
"#;

fn note(items: &[&str], link: &str) -> String {
    let items: Vec<String> = items.iter().map(|i| format!("\"{i}\"")).collect();
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n{link}: [{}]\n---\nThe body.\n",
        items.join(", ")
    )
}

/// The repository's toolchain file.
fn toolchain_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-toolchain.toml")
}

/// The channel the toolchain file names.
fn channel() -> String {
    let text = fs::read_to_string(toolchain_file()).expect("the repository's toolchain file");
    text.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "channel")
        .map(|(_, value)| value.trim().trim_matches('"').to_owned())
        .expect("a channel in the toolchain file")
}

/// A project whose note links to these items, with the crate `demo` beside its
/// books, in a directory of its own (the root has no manifest, so the crate's own is
/// read, at the top of the tree); its docs are built into `build`.
fn project(name: &str, books: &str, items: &[&str], link: &str) -> Project {
    let project = Project::with(name, books, &note(items, link));
    let krate = project.0.join("crate");
    fs::create_dir_all(krate.join("src")).unwrap();
    fs::write(krate.join("Cargo.toml"), CARGO_TOML).unwrap();
    fs::write(krate.join("src/lib.rs"), SOURCE).unwrap();
    fs::copy(toolchain_file(), krate.join("rust-toolchain.toml")).unwrap();
    project
}

/// Build the docs of the crate, as the rest of the world would: nothing of the
/// machine's flags, the toolchain of the outside.
fn cargo_doc(project: &Project) {
    // Pages an earlier build left of an item that is gone are read as there: start
    // from none.
    let _ = fs::remove_dir_all(project.0.join("build/doc"));
    let cargo = std::env::var("CARGO").expect("cargo sets CARGO for the tests it runs");
    let output = Command::new(cargo)
        .args(["doc", "--offline", "--no-deps"])
        .current_dir(project.0.join("crate"))
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
    // The layout watched is the pinned toolchain's.
    let index = fs::read_to_string(project.0.join("build/doc/demo/index.html")).unwrap();
    let version = index
        .split_once("data-rustdoc-version=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(version, _)| version.to_owned())
        .expect("the page says the version of rustdoc");
    let channel = channel();
    assert!(
        version.starts_with(&channel),
        "rustdoc {version} wrote the pages, and the toolchain file pins {channel}: the layout watched is not the pinned one"
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

fn standing(project: &Project) -> (serde_json::Value, String) {
    let row = &json(&run(project, &["get", "N-1"]))["row"];
    (row["standing"].clone(), row["causes"].to_string())
}

/// Edit the source of the crate and build its docs again.
fn edit(project: &Project, from: &str, to: &str) {
    let path = project.0.join("crate/src/lib.rs");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{from}");
    fs::write(path, text.replace(from, to)).unwrap();
    cargo_doc(project);
}

fn approve(project: &Project) {
    let output = run(project, &["judge", "approve", "N-1"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(standing(project).0, "fresh");
}

/// The note is stale for an item, for this reason.
fn assert_stale(project: &Project, what: &str, item: &str) {
    let (standing, causes) = standing(project);
    assert_eq!(standing, "stale", "{what}: {causes}");
    assert!(causes.contains("artifact_changed"), "{what}: {causes}");
    assert!(causes.contains(item), "{what}: {causes}");
}

#[test]
fn an_approval_stands_on_what_an_item_promises_in_the_docs_cargo_doc_writes() {
    let project = project("rust-item-promise", BOOKS, &ITEMS, "items");
    cargo_doc(&project);

    // Every kind of name has a promise to read in the real layout: a name that did
    // not would be cannot-tell, and could not be approved.
    let map = json(&run(&project, &["map"]));
    assert_eq!(map["links"][0]["preset"], "rustdoc-promise");
    let check = json(&run(&project, &["check"]));
    assert_eq!(check["clean"], true, "{check}");
    assert_eq!(check["absent"], serde_json::json!([]), "{check}");
    assert_eq!(check["unknown"], serde_json::json!([]), "{check}");
    approve(&project);

    // A body, a method added after it, an impl added: the promises are as they were.
    edit(
        &project,
        "self.count += n;",
        "self.count = self.count.saturating_add(n);",
    );
    edit(
        &project,
        "        self.count\n    }\n}\n",
        "        self.count\n    }\n\n    pub fn len(&self) -> u32 {\n        self.count\n    }\n}\n\nimpl Default for Shelf {\n    fn default() -> Self {\n        Shelf { count: 0 }\n    }\n}\n",
    );
    let (now, causes) = standing(&project);
    assert_eq!(now, "fresh", "{causes}");

    // The docs of a method.
    edit(
        &project,
        "/// Puts a book on the shelf.",
        "/// Puts books on the shelf.",
    );
    assert_stale(&project, "the docs of a method", "demo::Shelf::put");
    approve(&project);

    // Its signature.
    edit(
        &project,
        "pub fn put(&mut self, n: u32) -> u32 {",
        "pub fn put(&mut self, n: u32, _tag: u8) -> u32 {",
    );
    assert_stale(&project, "a signature", "demo::Shelf::put");
    approve(&project);

    // The docs of a field of a variant: part of the variant's promise.
    edit(&project, "/// Where it was.", "/// Where it was before.");
    assert_stale(
        &project,
        "the docs of a variant's field",
        "demo::Cause::Moved",
    );
    approve(&project);

    // The docs taken away.
    edit(&project, "    /// Puts books on the shelf.\n", "");
    assert_stale(&project, "docs taken away", "demo::Shelf::put");
    approve(&project);

    // And a name that goes away breaks the approval, as for the preset `rustdoc`.
    edit(
        &project,
        "/// A free function.\npub fn free",
        "pub fn freed",
    );
    let (now, causes) = standing(&project);
    assert_eq!(now, "broken", "{causes}");
    assert!(causes.contains("artifact_gone"), "{causes}");
    assert!(causes.contains("demo::free"), "{causes}");
}

#[test]
fn a_link_that_changes_from_the_preset_rustdoc_to_rustdoc_promise_changes_its_comparison() {
    const PRESENCE: &str = r#"[[book]]
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
    let project = project(
        "rust-item-promise-presence",
        PRESENCE,
        &["demo::Shelf::put"],
        "items",
    );
    cargo_doc(&project);
    approve(&project);

    // The preset `rustdoc` stands on the item being there: its docs are not read.
    edit(
        &project,
        "/// Puts a book on the shelf.",
        "/// Puts books on the shelf.",
    );
    let (now, causes) = standing(&project);
    assert_eq!(now, "fresh", "{causes}");
    // `diff` is of what was approved, which holds no content for this space: it
    // says the item is there and unchanged, and has no text to set beside.
    let diff = json(&run(&project, &["diff", "N-1"]));
    let item = diff["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["item"]["role"] == "artifact")
        .unwrap()
        .clone();
    assert_eq!(item["now"], "present", "{diff}");
    assert_eq!(item["changed"], false, "{diff}");
    assert_eq!(item["retrieval"], "no_address", "{diff}");

    // The link now says `rustdoc-promise`: the comparison changed.
    fs::write(project.0.join(".whyshelf/books.toml"), BOOKS).unwrap();
    let (now, causes) = standing(&project);
    assert_eq!(now, "stale", "{causes}");
    assert!(causes.contains("artifact_comparison_changed"), "{causes}");
    approve(&project);
}
