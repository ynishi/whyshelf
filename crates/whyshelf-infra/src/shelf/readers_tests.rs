//! What each reader of a project reads today, through a symbolic link, out of the
//! root, at a directory, and what it asks Git of the work tree: the declaration, the
//! ledger, the documents, the space `file`, the space `rust-test` and the space
//! `rust-item` (where its built docs are, a page, a re-export). Each test is one row of
//! what a reader does, so that a reader moved onto another way of reading keeps it.
//!
//! The declaration and the ledger follow a link on their path out of the root, as the
//! system does. The documents follow a link on the way to the directory of a range,
//! and to the file of a document read now, out of the root too; an entry of a range
//! that is itself a link is passed over. The spaces follow a link only while it stays
//! inside the root, and `rust-item` only inside the root and the build directory (a
//! page, the directory of a module) or inside the docs (a re-export).
//!
//! Each test has a number (T1, T2, ...) so that a verification or a comment can name
//! it; T18 is not used. The rows the readers gained when they came to read through the
//! state of the project are T27 and after: a kind of file that is neither a file, a
//! directory nor a link (a pipe), which each reader tells as it tells a file it cannot
//! read and never opens, and the built docs of `rust-item` at the two sides of a change.
//!
//! Each test makes its own small project in a temporary directory: a root and, beside
//! it, a directory outside the root.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;
use uuid::Uuid;
use whyshelf_core::app::shelf::{BookSchemaSource, DocSource, LedgerAppend, LedgerRead, Side};
use whyshelf_core::domain::actor::{Actor, Attribution};
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptySet};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, ArtifactState, Comparison, NameListing, Selection, SpaceAsks,
    SpaceReadings, Untold,
};
use whyshelf_core::domain::shelf::build::DocReading;
use whyshelf_core::domain::shelf::ledger::{
    Admit, AdmittedRecord, JudgedAt, JudgementId, Ledger, Record, Revocation,
};
use whyshelf_core::domain::shelf::schema::{BookDir, BookRange};

use super::artifact::file::FileSpace;
use super::artifact::js_test::JsTestSpace;
use super::artifact::rust_item::{GENERATOR_MARK, RustItemSpace};
use super::artifact::rust_test::RustTestSpace;
use super::books::{BooksError, BooksToml};
use super::docs::{DocsError, MarkdownDocs};
use super::ledger::{JsonLedger, LedgerError};
use super::state::files::FileStates;

/// A root and a directory outside it, side by side in one temporary directory.
struct Tree {
    _dir: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
}

async fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::create_dir_all(&outside).await.unwrap();
    Tree {
        _dir: dir,
        root,
        outside,
    }
}

async fn write(dir: &Path, at: &str, text: &str) {
    let full = dir.join(at);
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    tokio::fs::write(full, text).await.unwrap();
}

/// A symbolic link at `at` under `dir`, holding `target` as written.
#[cfg(unix)]
async fn link(target: impl AsRef<Path>, dir: &Path, at: &str) {
    let full = dir.join(at);
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    tokio::fs::symlink(target, full).await.unwrap();
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

fn n(text: &str) -> ArtifactName {
    ArtifactName::new(text).unwrap()
}

fn asks(names: &[&str], comparison: Comparison) -> SpaceAsks {
    names
        .iter()
        .map(|name| (n(name), BTreeSet::from([comparison.clone()])))
        .collect()
}

/// Why the reading cannot be told under `comparison`, when it cannot.
fn untold_why(reading: &ArtifactReading, comparison: &Comparison) -> Option<String> {
    match reading.state(comparison) {
        ArtifactState::Unknown(Untold::Said(why)) => Some(why.as_str().to_owned()),
        _ => None,
    }
}

/// Whether `git` can be run here; the tests that need it pass without it.
async fn git_installed() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// Git run on `dir` with no `GIT_*` variable of the caller (a hook's `GIT_DIR` or
/// `GIT_INDEX_FILE` would point `init` and `add` at another repository).
async fn git(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let out = command
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

// The declaration (books/read.rs, `BookSchemaSource::declarations`): read as the system
// reads the path, every link on it followed, out of the root too.

const BOOKS: &str = "[[book]]\nname = \"finding\"\ndir = \"docs\"\nkey = { prefix = \"F\" }\n";

fn declaration(root: &Path) -> BooksToml {
    BooksToml::new(root, rel(".whyshelf/books.toml"))
}

/// T1: `books.toml` a link to a file elsewhere inside the root.
#[cfg(unix)]
#[tokio::test]
async fn t1_the_declaration_is_read_through_a_link_inside_the_root() {
    let t = tree().await;
    write(&t.root, "decl/books.toml", BOOKS).await;
    link("../decl/books.toml", &t.root, ".whyshelf/books.toml").await;
    let decl = declaration(&t.root)
        .declarations(Side::After)
        .await
        .unwrap();
    assert_eq!(decl.books.len(), 1);
}

/// T2: `.whyshelf` a link to a directory outside the root.
#[cfg(unix)]
#[tokio::test]
async fn t2_the_declaration_is_read_through_a_link_out_of_the_root() {
    let t = tree().await;
    write(&t.outside, "books.toml", BOOKS).await;
    link(&t.outside, &t.root, ".whyshelf").await;
    let decl = declaration(&t.root)
        .declarations(Side::After)
        .await
        .unwrap();
    assert_eq!(decl.books.len(), 1);
}

/// T3: a broken link is no declaration (`Missing`).
#[cfg(unix)]
#[tokio::test]
async fn t3_a_broken_link_is_a_missing_declaration() {
    let t = tree().await;
    link("nowhere.toml", &t.root, ".whyshelf/books.toml").await;
    let error = declaration(&t.root)
        .declarations(Side::After)
        .await
        .unwrap_err();
    assert!(matches!(error, BooksError::Missing { .. }), "{error:?}");
}

/// T3: a directory in the place of the declaration cannot be read (`Io`).
#[tokio::test]
async fn t3_a_directory_in_the_place_of_the_declaration_cannot_be_read() {
    let t = tree().await;
    tokio::fs::create_dir_all(t.root.join(".whyshelf/books.toml"))
        .await
        .unwrap();
    let error = declaration(&t.root)
        .declarations(Side::After)
        .await
        .unwrap_err();
    assert!(matches!(error, BooksError::Io { .. }), "{error:?}");
}

/// A pipe at `at` under `dir`, when the system can make one (`mkfifo`): opened for
/// reading, it would wait for a writer, so a test that ends shows it was not opened.
#[cfg(unix)]
pub(crate) async fn pipe(dir: &Path, at: &str) -> bool {
    let full = dir.join(at);
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    Command::new("mkfifo")
        .arg(full)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// What `read` gives, when it ends. A read that opened the pipe at `pipe` would wait
/// for a writer: after a while the pipe is opened for writing, which lets that read
/// end, and the test fails rather than waits.
#[cfg(unix)]
pub(crate) async fn ends<F: std::future::Future>(pipe: &Path, read: F) -> F::Output {
    let wait = std::time::Duration::from_secs(30);
    match tokio::time::timeout(wait, read).await {
        Ok(read) => read,
        Err(_) => {
            let mut writer = tokio::fs::OpenOptions::new();
            writer.write(true);
            let _ = tokio::time::timeout(wait, writer.open(pipe)).await;
            panic!(
                "{} was opened, and the read waited for a writer",
                pipe.display()
            );
        }
    }
}

/// T27: a pipe in the place of the declaration cannot be told, and is not opened.
#[cfg(unix)]
#[tokio::test]
async fn t27_a_pipe_in_the_place_of_the_declaration_cannot_be_told() {
    let t = tree().await;
    if !pipe(&t.root, ".whyshelf/books.toml").await {
        return;
    }
    let books = declaration(&t.root);
    let error = ends(
        &t.root.join(".whyshelf/books.toml"),
        books.declarations(Side::After),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, BooksError::Untold { .. }), "{error:?}");
}

// The ledger (ledger/read.rs, `LedgerRead::read`): the directory and each `.json` file read
// as the system reads them, links followed, out of the root too.

fn ledger(root: &Path) -> JsonLedger {
    JsonLedger::new(root, rel(".whyshelf/ledger"))
}

/// A record, admitted as a project that declares no rule admits it.
fn record() -> AdmittedRecord {
    let jid = |n: u128| {
        let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
        JudgementId::new(EntityId::new(Uuid::from_u128(bits)).unwrap())
    };
    let revocation = Record::Revocation(Revocation::new(
        jid(2),
        NonEmptySet::from_set(BTreeSet::from([jid(1)])).unwrap(),
        Attribution::one(Actor::new("ana").unwrap()),
        JudgedAt::new(1_700_000_000_000),
        NonBlank::new("wrong file").unwrap(),
    ));
    Ledger::build(Vec::new(), &Admit::unruled())
        .accept(revocation)
        .unwrap()
}

/// A record written in `dir`, by the ledger itself: the name of its file.
async fn record_in(dir: &Path) -> String {
    let name = dir.file_name().unwrap().to_str().unwrap();
    let at = JsonLedger::new(dir.parent().unwrap(), rel(name))
        .append(&record())
        .await
        .unwrap();
    at.as_str().rsplit('/').next().unwrap().to_owned()
}

/// T4: the directory of the ledger a link out of the root.
#[cfg(unix)]
#[tokio::test]
async fn t4_the_ledger_is_read_through_a_directory_linked_out_of_the_root() {
    let t = tree().await;
    record_in(&t.outside.join("ledger")).await;
    link(t.outside.join("ledger"), &t.root, ".whyshelf/ledger").await;
    let contents = ledger(&t.root).read(Side::After).await.unwrap();
    assert_eq!(contents.records.len(), 1);
    assert!(contents.unreadable.is_empty());
}

/// T5: the file of a record a link out of the root.
#[cfg(unix)]
#[tokio::test]
async fn t5_a_record_is_read_through_a_file_linked_out_of_the_root() {
    let t = tree().await;
    let name = record_in(&t.outside.join("kept")).await;
    link(
        t.outside.join("kept").join(&name),
        &t.root,
        &format!(".whyshelf/ledger/{name}"),
    )
    .await;
    let contents = ledger(&t.root).read(Side::After).await.unwrap();
    assert_eq!(contents.records.len(), 1);
}

/// T6: a directory whose name ends in `.json` stops the read (`Io`).
#[tokio::test]
async fn t6_a_directory_named_as_a_record_stops_the_read() {
    let t = tree().await;
    tokio::fs::create_dir_all(t.root.join(".whyshelf/ledger/x.json"))
        .await
        .unwrap();
    let error = ledger(&t.root).read(Side::After).await.unwrap_err();
    assert!(matches!(error, LedgerError::Io { .. }), "{error:?}");
}

/// T22: the directory of the ledger reached through a broken link is as none there (an
/// empty ledger), and a record file that is a broken link stops the read (`Io`).
#[cfg(unix)]
#[tokio::test]
async fn t22_a_broken_link_is_no_ledger_and_a_broken_record_stops_the_read() {
    let t = tree().await;
    link(t.outside.join("gone"), &t.root, ".whyshelf/ledger").await;
    let contents = ledger(&t.root).read(Side::After).await.unwrap();
    assert!(contents.records.is_empty() && contents.unreadable.is_empty());

    let t = tree().await;
    link("nowhere.json", &t.root, ".whyshelf/ledger/x.json").await;
    let error = ledger(&t.root).read(Side::After).await.unwrap_err();
    assert!(matches!(error, LedgerError::Io { .. }), "{error:?}");
}

/// T28: a pipe named as a record, or in the place of the directory of the ledger,
/// cannot be told and stops the read; it is not opened.
#[cfg(unix)]
#[tokio::test]
async fn t28_a_pipe_in_the_ledger_cannot_be_told_and_stops_the_read() {
    let t = tree().await;
    if !pipe(&t.root, ".whyshelf/ledger/x.json").await {
        return;
    }
    let at = t.root.join(".whyshelf/ledger/x.json");
    let error = ends(&at, ledger(&t.root).read(Side::After))
        .await
        .unwrap_err();
    assert!(matches!(error, LedgerError::Untold { .. }), "{error:?}");

    let t = tree().await;
    if !pipe(&t.root, ".whyshelf/ledger").await {
        return;
    }
    let at = t.root.join(".whyshelf/ledger");
    let error = ends(&at, ledger(&t.root).read(Side::After))
        .await
        .unwrap_err();
    assert!(matches!(error, LedgerError::Untold { .. }), "{error:?}");
}

// The documents (docs/read.rs, `DocSource::read_docs` and `current`): the directory of a
// range read as the system reads it, out of the root too; the entries in it by their
// own kind, a link among them passed over.

const DOC: &str = "---\nid: 019a6f3e-0000-7000-8000-000000000001\nkey: F-1\n---\nThe body.\n";

fn docs_range() -> BookRange {
    BookRange::of_dir(BookDir::new("docs").unwrap())
}

/// The origins of the documents the range of `docs` takes.
async fn origins(root: &Path) -> Vec<String> {
    MarkdownDocs::new(root.to_path_buf())
        .read_docs(Side::After, &[docs_range()])
        .await
        .unwrap()
        .iter()
        .map(|reading| reading.origin().as_str().to_owned())
        .collect()
}

/// T7: the directory of a range a link out of the root: its files are read, as
/// documents of their path under the root.
#[cfg(unix)]
#[tokio::test]
async fn t7_the_directory_of_a_range_is_read_through_a_link_out_of_the_root() {
    let t = tree().await;
    write(&t.outside, "docs/a.md", DOC).await;
    link(t.outside.join("docs"), &t.root, "docs").await;
    let readings = MarkdownDocs::new(t.root.clone())
        .read_docs(Side::After, &[docs_range()])
        .await
        .unwrap();
    assert_eq!(readings.len(), 1);
    assert!(
        matches!(&readings[0], DocReading::Parsed { origin, .. } if origin.as_str() == "docs/a.md"),
        "{readings:?}"
    );
}

/// T8: an entry of a range that is a link, to a file or a directory, is passed over,
/// in the root and in a directory out of it alike.
#[cfg(unix)]
#[tokio::test]
async fn t8_a_link_among_the_entries_of_a_range_is_passed_over() {
    let t = tree().await;
    write(&t.root, "docs/a.md", DOC).await;
    write(&t.root, "more/c.md", DOC).await;
    link("a.md", &t.root, "docs/b.md").await;
    link(t.root.join("more"), &t.root, "docs/sub").await;
    assert_eq!(origins(&t.root).await, ["docs/a.md"]);

    let away = tree().await;
    write(&away.outside, "docs/a.md", DOC).await;
    write(&away.outside, "more/c.md", DOC).await;
    link("a.md", &away.outside, "docs/b.md").await;
    link(away.outside.join("more"), &away.outside, "docs/sub").await;
    link(away.outside.join("docs"), &away.root, "docs").await;
    assert_eq!(origins(&away.root).await, ["docs/a.md"]);
}

/// T9: the file of a document read now (`current`, for `diff`) through a link out of
/// the root; a directory there is no file.
#[cfg(unix)]
#[tokio::test]
async fn t9_the_file_of_a_document_is_read_now_through_a_link_out_of_the_root() {
    let t = tree().await;
    write(&t.outside, "docs/a.md", DOC).await;
    link(t.outside.join("docs"), &t.root, "docs").await;
    let docs = MarkdownDocs::new(t.root.clone());
    assert_eq!(
        docs.current(Side::After, &rel("docs/a.md"))
            .await
            .unwrap()
            .as_deref(),
        Some(DOC.as_bytes())
    );
    assert_eq!(docs.current(Side::After, &rel("docs")).await.unwrap(), None);
}

/// T23: the directory of a range that is a broken link holds no documents, and the
/// file of a document that is a broken link is none now.
#[cfg(unix)]
#[tokio::test]
async fn t23_a_broken_link_holds_no_documents_and_is_no_file_now() {
    let t = tree().await;
    link(t.outside.join("gone"), &t.root, "docs").await;
    assert!(origins(&t.root).await.is_empty());

    let t = tree().await;
    link("nowhere.md", &t.root, "docs/a.md").await;
    let docs = MarkdownDocs::new(t.root.clone());
    assert_eq!(
        docs.current(Side::After, &rel("docs/a.md")).await.unwrap(),
        None
    );
}

/// T29: an entry of a range that is a pipe is passed over, as a link is; a pipe in the
/// place of the directory of a range, or of a document's file read now, cannot be told.
/// None is opened.
#[cfg(unix)]
#[tokio::test]
async fn t29_a_pipe_in_a_range_is_passed_over_and_one_in_the_place_of_a_file_cannot_be_told() {
    let t = tree().await;
    write(&t.root, "docs/a.md", DOC).await;
    if !pipe(&t.root, "docs/b.md").await {
        return;
    }
    let at = t.root.join("docs/b.md");
    assert_eq!(ends(&at, origins(&t.root)).await, ["docs/a.md"]);
    let docs = MarkdownDocs::new(t.root.clone());
    let error = ends(&at, docs.current(Side::After, &rel("docs/b.md")))
        .await
        .unwrap_err();
    assert!(matches!(error, DocsError::Untold { .. }), "{error:?}");

    let t = tree().await;
    if !pipe(&t.root, "docs").await {
        return;
    }
    let docs = MarkdownDocs::new(t.root.clone());
    let error = ends(
        &t.root.join("docs"),
        docs.read_docs(Side::After, &[docs_range()]),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, DocsError::Untold { .. }), "{error:?}");
}

/// A loop of two links at `at` under `dir`, the second beside it.
#[cfg(unix)]
async fn looped(dir: &Path, at: &str) {
    let other = format!("{at}.other");
    let name = |path: &str| path.rsplit('/').next().unwrap().to_owned();
    link(name(&other), dir, at).await;
    link(name(at), dir, &other).await;
}

/// T34: a loop of links where the declaration, the directory of the ledger, a record or
/// the directory of a range is fails the read with the system's error (`Io`), as a
/// reading of the path does; it is not read as nothing there, which a broken link is
/// (T3, T22, T23).
#[cfg(unix)]
#[tokio::test]
async fn t34_a_loop_of_links_is_the_systems_error_and_not_nothing_there() {
    let t = tree().await;
    looped(&t.root, ".whyshelf/books.toml").await;
    let error = declaration(&t.root)
        .declarations(Side::After)
        .await
        .unwrap_err();
    assert!(matches!(error, BooksError::Io { .. }), "{error:?}");

    let t = tree().await;
    looped(&t.root, ".whyshelf/ledger").await;
    let error = ledger(&t.root).read(Side::After).await.unwrap_err();
    assert!(matches!(error, LedgerError::Io { .. }), "{error:?}");

    let t = tree().await;
    looped(&t.root, ".whyshelf/ledger/x.json").await;
    let error = ledger(&t.root).read(Side::After).await.unwrap_err();
    assert!(matches!(error, LedgerError::Io { .. }), "{error:?}");

    let t = tree().await;
    looped(&t.root, "docs").await;
    let error = MarkdownDocs::new(t.root.clone())
        .read_docs(Side::After, &[docs_range()])
        .await
        .unwrap_err();
    assert!(matches!(error, DocsError::Io { .. }), "{error:?}");
}

/// T35: a link that leads through a file (`docs` to `README.md/x`) is the system's own
/// answer for a path through a file: the file of a document read now there, and the
/// link itself, are no file now, and the directory of a range there fails the read
/// (`Io`), as a listing of the path does.
#[cfg(unix)]
#[tokio::test]
async fn t35_a_link_through_a_file_is_no_file_now() {
    let t = tree().await;
    write(&t.root, "README.md", "read me\n").await;
    link("README.md/x", &t.root, "docs").await;
    let docs = MarkdownDocs::new(t.root.clone());
    assert_eq!(
        docs.current(Side::After, &rel("docs/a.md")).await.unwrap(),
        None
    );
    assert_eq!(docs.current(Side::After, &rel("docs")).await.unwrap(), None);
    let error = docs
        .read_docs(Side::After, &[docs_range()])
        .await
        .unwrap_err();
    assert!(matches!(error, DocsError::Io { .. }), "{error:?}");
}

/// T37: a record whose name is no path of the project (a backslash in it) is read as
/// the system names it: kept when it decodes; when it does not, its name cannot be
/// reported, and the read fails (`Name`, with the reason of the grammar).
#[cfg(unix)]
#[tokio::test]
async fn t37_a_record_whose_name_is_no_path_is_read_and_kept() {
    let t = tree().await;
    let kept = t.outside.join("kept");
    let name = record_in(&kept).await;
    let bytes = tokio::fs::read(kept.join(&name)).await.unwrap();
    let dir = t.root.join(".whyshelf/ledger");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("a\\b.json"), &bytes)
        .await
        .unwrap();
    let contents = ledger(&t.root).read(Side::After).await.unwrap();
    assert_eq!(contents.records.len(), 1);
    assert!(contents.unreadable.is_empty());

    tokio::fs::write(dir.join("c\\d.json"), "{}").await.unwrap();
    let error = ledger(&t.root).read(Side::After).await.unwrap_err();
    assert!(
        matches!(
            error,
            LedgerError::Name {
                source: Some(_),
                ..
            }
        ),
        "{error:?}"
    );
}

/// T38: a directory in a range whose name is no path of the project (a backslash in
/// it) is walked: one that holds no file the range takes is no error, and a file the
/// range takes in it fails the read (`Name`): it cannot be named as a document.
#[cfg(unix)]
#[tokio::test]
async fn t38_a_directory_of_a_range_whose_name_is_no_path_is_walked() {
    let t = tree().await;
    write(&t.root, "docs/a.md", DOC).await;
    write(&t.root, "docs/x\\y/notes.txt", "notes\n").await;
    assert_eq!(origins(&t.root).await, ["docs/a.md"]);

    write(&t.root, "docs/x\\y/b.md", DOC).await;
    let error = MarkdownDocs::new(t.root.clone())
        .read_docs(Side::After, &[docs_range()])
        .await
        .unwrap_err();
    assert!(matches!(error, DocsError::Name { .. }), "{error:?}");
}

// The space `file` (artifact/file/read.rs, `place_of`): a link is followed by its
// canonical path while that stays inside the canonical root; out of it, broken or a
// loop, nothing is read and the name cannot be told.

/// T10: a link inside the root, relative or an absolute path, is read through.
#[cfg(unix)]
#[tokio::test]
async fn t10_a_link_inside_the_root_relative_or_absolute_is_read_through() {
    let t = tree().await;
    write(&t.root, "a.txt", "one\n").await;
    link("a.txt", &t.root, "alias.txt").await;
    link(t.root.join("a.txt"), &t.root, "absolute.txt").await;
    let space = FileSpace::new(t.root.clone());
    let found = space
        .resolve(&asks(&["alias.txt", "absolute.txt"], Comparison::Bytes))
        .await
        .unwrap();
    for name in ["alias.txt", "absolute.txt"] {
        assert_eq!(
            found[&n(name)].state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(b"one\n")),
            "{name}"
        );
        assert_eq!(
            space.current(&n(name)).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
    }
}

/// T11: a link out of the root cannot be told, and has no bytes now.
#[cfg(unix)]
#[tokio::test]
async fn t11_a_link_out_of_the_root_cannot_be_told() {
    let t = tree().await;
    write(&t.outside, "x.txt", "out\n").await;
    link(t.outside.join("x.txt"), &t.root, "out.txt").await;
    let space = FileSpace::new(t.root.clone());
    let found = space
        .resolve(&asks(&["out.txt"], Comparison::Bytes))
        .await
        .unwrap();
    let why = untold_why(&found[&n("out.txt")], &Comparison::Bytes).unwrap();
    assert!(why.contains("leads outside the root"), "{why}");
    assert_eq!(space.current(&n("out.txt")).await.unwrap(), None);
}

/// T25, the space `rust-test` read by the rule of the space `file`: a test in a file
/// through a link inside the root is read, one through a broken link cannot be told,
/// and a directory where the file would be holds no test. T11 for this space (a link
/// out of the root cannot be told) is `a_file_through_a_link_out_of_the_root_cannot_be_told`
/// in artifact/rust_test.rs, kept there.
#[cfg(unix)]
#[tokio::test]
async fn t25_a_test_is_read_through_a_link_inside_the_root_only() {
    let t = tree().await;
    write(&t.root, "src/lib.rs", "#[test]\nfn top() {}\n").await;
    link("src", &t.root, "near").await;
    link("nowhere.rs", &t.root, "broken.rs").await;
    tokio::fs::create_dir_all(t.root.join("dir.rs"))
        .await
        .unwrap();
    let names = ["near/lib.rs::top", "broken.rs::top", "dir.rs::top"];
    let found = RustTestSpace::new(t.root.clone())
        .resolve(&asks(&names, Comparison::Bytes))
        .await
        .unwrap();
    assert!(found[&n("near/lib.rs::top")].is_present(), "{found:?}");
    let why = untold_why(&found[&n("broken.rs::top")], &Comparison::Bytes).unwrap();
    assert!(why.contains("or is broken"), "{why}");
    assert_eq!(found[&n("dir.rs::top")], ArtifactReading::absent());
}

/// T12: a broken link, and a loop of links, cannot be told and have no bytes now.
#[cfg(unix)]
#[tokio::test]
async fn t12_a_broken_link_and_a_loop_cannot_be_told() {
    let t = tree().await;
    link("nowhere.txt", &t.root, "broken.txt").await;
    link("two.txt", &t.root, "one.txt").await;
    link("one.txt", &t.root, "two.txt").await;
    let space = FileSpace::new(t.root.clone());
    let found = space
        .resolve(&asks(&["broken.txt", "one.txt"], Comparison::Bytes))
        .await
        .unwrap();
    for name in ["broken.txt", "one.txt"] {
        let why = untold_why(&found[&n(name)], &Comparison::Bytes);
        assert!(
            why.as_deref().is_some_and(|w| w.contains("or is broken")),
            "{name}: {why:?}"
        );
        assert_eq!(space.current(&n(name)).await.unwrap(), None, "{name}");
    }
}

/// T24: a directory as a name is no file: absent, with no bytes now.
#[tokio::test]
async fn t24_a_directory_as_a_name_is_absent() {
    let t = tree().await;
    write(&t.root, "dir/a.txt", "one\n").await;
    let space = FileSpace::new(t.root.clone());
    let found = space
        .resolve(&asks(&["dir"], Comparison::Bytes))
        .await
        .unwrap();
    assert_eq!(found[&n("dir")], ArtifactReading::absent());
    assert_eq!(space.current(&n("dir")).await.unwrap(), None);
}

/// T30: a pipe named in the space `file` cannot be told, and has no bytes now; it is not
/// opened.
#[cfg(unix)]
#[tokio::test]
async fn t30_a_pipe_in_the_space_file_cannot_be_told() {
    let t = tree().await;
    if !pipe(&t.root, "pipe.txt").await {
        return;
    }
    let at = t.root.join("pipe.txt");
    let space = FileSpace::new(t.root.clone());
    let asked = asks(&["pipe.txt"], Comparison::Bytes);
    let found = ends(&at, space.resolve(&asked)).await.unwrap();
    let why = untold_why(&found[&n("pipe.txt")], &Comparison::Bytes).unwrap();
    assert!(why.contains("neither a file"), "{why}");
    assert_eq!(
        ends(&at, space.current(&n("pipe.txt"))).await.unwrap(),
        None
    );
}

/// T31: a test named in a pipe, in the space `rust-test`, cannot be told; it is not
/// opened.
#[cfg(unix)]
#[tokio::test]
async fn t31_a_test_in_a_pipe_cannot_be_told() {
    let t = tree().await;
    if !pipe(&t.root, "src/lib.rs").await {
        return;
    }
    let space = RustTestSpace::new(t.root.clone());
    let asked = asks(&["src/lib.rs::top"], Comparison::Bytes);
    let found = ends(&t.root.join("src/lib.rs"), space.resolve(&asked))
        .await
        .unwrap();
    let why = untold_why(&found[&n("src/lib.rs::top")], &Comparison::Bytes).unwrap();
    assert!(why.contains("neither a file"), "{why}");
}

/// T36: a test named in a pipe, in the space `js-test`, cannot be told; it is not
/// opened.
#[cfg(unix)]
#[tokio::test]
async fn t36_a_js_test_in_a_pipe_cannot_be_told() {
    let t = tree().await;
    if !pipe(&t.root, "src/a.test.ts").await {
        return;
    }
    let space = JsTestSpace::new(t.root.clone());
    let asked = asks(&["src/a.test.ts::top"], Comparison::Bytes);
    let found = ends(&t.root.join("src/a.test.ts"), space.resolve(&asked))
        .await
        .unwrap();
    let why = untold_why(&found[&n("src/a.test.ts::top")], &Comparison::Bytes).unwrap();
    assert!(why.contains("neither a file"), "{why}");
}

/// T13: the names of a selection are the files Git lists of the work tree: none can
/// be listed outside a repository, and an ignored file is not among them.
#[tokio::test]
async fn t13_the_names_of_a_selection_are_the_files_git_lists() {
    if !git_installed().await {
        return;
    }
    let t = tree().await;
    write(&t.root, "a.txt", "one\n").await;
    write(&t.root, "ignored.txt", "two\n").await;
    write(&t.root, ".gitignore", "ignored.txt\n").await;
    let selection = Selection::new(vec!["*.txt".to_owned()], Vec::new()).unwrap();
    let space = FileSpace::new(t.root.clone());
    let listing = space.list(&selection).await.unwrap();
    assert!(matches!(listing, NameListing::CannotList(_)), "{listing:?}");
    git(&t.root, &["init", "-q"]).await;
    assert_eq!(
        space.list(&selection).await.unwrap(),
        NameListing::All(BTreeSet::from([n("a.txt")]))
    );
}

// The space `rust-item` (artifact/rust_item/lookup.rs): a page or the directory of a
// module is read only when its canonical path is under the root or the build
// directory, and a re-export followed only when its canonical path is under the docs.

/// A page of a type with a member `new`, as rustdoc writes the anchors.
fn page() -> String {
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><main><section id=\"method.new\" class=\"method\"></section></main></body></html>"
    )
}

/// The index of a module with these re-exports, each `(name, link)`.
fn index(reexports: &[(&str, &str)]) -> String {
    let entries: String = reexports
        .iter()
        .map(|(name, href)| {
            format!(
                "<dt id=\"reexport.{name}\"><code>pub use inner::<a class=\"struct\" href=\"{href}\" title=\"x\">{name}</a>;</code></dt>"
            )
        })
        .collect();
    format!(
        "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><h2 id=\"reexports\">Re-exports</h2><dl>{entries}</dl></body></html>"
    )
}

/// A project with a `Cargo.toml`, so its docs are the `doc` of its build directory, holding the crate
/// `demo`, which re-exports `Thing` from inside the docs and `Near` through a link to
/// a directory of the root outside the docs.
async fn docs_tree() -> Tree {
    let t = tree().await;
    write(&t.root, "Cargo.toml", "[workspace]\n").await;
    write(
        &t.root,
        "target/doc/demo/index.html",
        &index(&[
            ("Thing", "inner/struct.Thing.html"),
            ("Near", "near/struct.Near.html"),
        ]),
    )
    .await;
    write(&t.root, "target/doc/demo/inner/struct.Thing.html", &page()).await;
    write(&t.root, "pages/struct.Near.html", &page()).await;
    write(&t.root, "pages/struct.Moved.html", &page()).await;
    write(&t.outside, "struct.Away.html", &page()).await;
    t
}

/// T14: a page that is a link to a file elsewhere inside the root is read.
#[cfg(unix)]
#[tokio::test]
async fn t14_a_page_linked_elsewhere_inside_the_root_is_read() {
    let t = docs_tree().await;
    link(
        t.root.join("pages/struct.Moved.html"),
        &t.root,
        "target/doc/demo/struct.Moved.html",
    )
    .await;
    let found = RustItemSpace::new(t.root.clone(), None)
        .resolve(&asks(&["demo::Moved::new"], Comparison::Presence))
        .await
        .unwrap();
    assert!(found[&n("demo::Moved::new")].is_present(), "{found:?}");
}

/// T15: a page that is a link out of the root is not read, and says why.
#[cfg(unix)]
#[tokio::test]
async fn t15_a_page_linked_out_of_the_root_is_not_read_and_says_why() {
    let t = docs_tree().await;
    link(
        t.outside.join("struct.Away.html"),
        &t.root,
        "target/doc/demo/struct.Away.html",
    )
    .await;
    let found = RustItemSpace::new(t.root.clone(), None)
        .resolve(&asks(&["demo::Away"], Comparison::Presence))
        .await
        .unwrap();
    let why = untold_why(&found[&n("demo::Away")], &Comparison::Presence).unwrap();
    assert!(
        why.contains("leads outside the root of the project and the build directory"),
        "{why}"
    );
}

/// T16: a re-export whose link stays inside the docs is followed.
#[tokio::test]
async fn t16_a_reexport_inside_the_docs_is_followed() {
    let t = docs_tree().await;
    let found = RustItemSpace::new(t.root.clone(), None)
        .resolve(&asks(&["demo::Thing::new"], Comparison::Presence))
        .await
        .unwrap();
    assert!(found[&n("demo::Thing::new")].is_present(), "{found:?}");
}

/// T17: a re-export whose link leads, through a link, out of the docs to a place
/// inside the root is not followed: the name is there, and nothing under it can be
/// told.
#[cfg(unix)]
#[tokio::test]
async fn t17_a_reexport_out_of_the_docs_inside_the_root_is_not_followed() {
    let t = docs_tree().await;
    link(t.root.join("pages"), &t.root, "target/doc/demo/near").await;
    let found = RustItemSpace::new(t.root.clone(), None)
        .resolve(&asks(
            &["demo::Near", "demo::Near::new"],
            Comparison::Presence,
        ))
        .await
        .unwrap();
    let near = &found[&n("demo::Near")];
    assert!(near.is_present(), "{near:?}");
    let why = untold_why(near, &Comparison::Bytes).unwrap();
    assert!(why.contains("from docs that are not here"), "{why}");
    let why = untold_why(&found[&n("demo::Near::new")], &Comparison::Presence).unwrap();
    assert!(why.contains("from docs that are not here"), "{why}");
}

/// T26: the directory of a module reached through a link elsewhere inside the root is
/// read, and one reached through a link out of the root is not, and says why.
#[cfg(unix)]
#[tokio::test]
async fn t26_the_directory_of_a_module_is_read_through_a_link_inside_the_root_only() {
    let t = docs_tree().await;
    write(&t.root, "mods/kept/index.html", &page()).await;
    write(&t.outside, "away/index.html", &page()).await;
    link(t.root.join("mods/kept"), &t.root, "target/doc/demo/kept").await;
    link(t.outside.join("away"), &t.root, "target/doc/demo/away").await;
    let found = RustItemSpace::new(t.root.clone(), None)
        .resolve(&asks(&["demo::kept", "demo::away"], Comparison::Presence))
        .await
        .unwrap();
    assert!(found[&n("demo::kept")].is_present(), "{found:?}");
    let why = untold_why(&found[&n("demo::away")], &Comparison::Presence).unwrap();
    assert!(
        why.contains("leads outside the root of the project and the build directory"),
        "{why}"
    );
}

// The addresses of what a run reads (`FileSpace::resolve`, `DocSource::addresses`):
// the blob Git would store for each file as it is in the work tree (`git
// hash-object`), not the blob the index holds; none outside a repository.

/// T19: the address of a file read is that of its bytes in the work tree.
#[tokio::test]
async fn t19_the_address_of_a_file_is_its_blob_in_the_work_tree() {
    if !git_installed().await {
        return;
    }
    let t = tree().await;
    write(&t.root, "a.txt", "one\n").await;
    let space = FileSpace::new(t.root.clone());
    let address =
        |found: &SpaceReadings| found[&n("a.txt")].address().map(|a| a.as_str().to_owned());
    let found = space
        .resolve(&asks(&["a.txt"], Comparison::Bytes))
        .await
        .unwrap();
    assert_eq!(address(&found), None);
    git(&t.root, &["init", "-q"]).await;
    git(&t.root, &["add", "a.txt"]).await;
    write(&t.root, "a.txt", "two\n").await;
    let found = space
        .resolve(&asks(&["a.txt"], Comparison::Bytes))
        .await
        .unwrap();
    assert_eq!(
        address(&found),
        Some(git(&t.root, &["hash-object", "a.txt"]).await)
    );
}

/// T20: the address of a document's file is that of its bytes in the work tree, and a
/// file that is not there has none.
#[tokio::test]
async fn t20_the_address_of_a_document_is_its_blob_in_the_work_tree() {
    if !git_installed().await {
        return;
    }
    let t = tree().await;
    write(&t.root, "docs/a.md", DOC).await;
    let docs = MarkdownDocs::new(t.root.clone());
    let asked = BTreeSet::from([rel("docs/a.md"), rel("docs/gone.md")]);
    assert!(
        docs.addresses(Side::After, &asked)
            .await
            .unwrap()
            .is_empty()
    );
    git(&t.root, &["init", "-q"]).await;
    git(&t.root, &["add", "docs/a.md"]).await;
    write(&t.root, "docs/a.md", &format!("{DOC}More.\n")).await;
    let addresses = docs.addresses(Side::After, &asked).await.unwrap();
    assert_eq!(
        addresses
            .iter()
            .map(|(file, address)| (file.as_str().to_owned(), address.as_str().to_owned()))
            .collect::<Vec<_>>(),
        [(
            "docs/a.md".to_owned(),
            git(&t.root, &["hash-object", "docs/a.md"]).await
        )]
    );
}

// Where `rust-item` reads the built docs (artifact/rust_item.rs, `docs`): the
// variable `CARGO_TARGET_DIR`, then `build.target-dir` of `.cargo/config` (the legacy
// name, first) or `.cargo/config.toml`, then `target` beside a `Cargo.toml`, else it
// cannot be told. Each place below holds the docs of a crate of its own, so which one
// resolves tells which place was read.

/// The crates of `space` that resolve as there, among those of every place.
async fn built(space: RustItemSpace) -> Vec<&'static str> {
    let crates = ["beside", "configured", "legacy", "variable"];
    let found = space
        .resolve(&asks(&crates, Comparison::Presence))
        .await
        .unwrap();
    crates
        .into_iter()
        .filter(|krate| found[&n(krate)].is_present())
        .collect()
}

/// T21: the four ways the place of the built docs is read, in their order, the legacy
/// name of the settings before the other, and settings through a link out of the
/// root, which tell nothing and say why.
#[tokio::test]
async fn t21_the_built_docs_are_where_the_variable_the_settings_or_the_cargo_toml_say() {
    let t = tree().await;
    let r = &t.root;
    for (dir, krate) in [
        ("target", "beside"),
        ("built", "configured"),
        ("old", "legacy"),
        ("env", "variable"),
    ] {
        write(r, &format!("{dir}/doc/{krate}/index.html"), &page()).await;
    }
    let space = |target: Option<&str>| RustItemSpace::new(r.clone(), target.map(PathBuf::from));

    // Nothing to tell it by.
    let found = space(None)
        .resolve(&asks(&["beside"], Comparison::Presence))
        .await
        .unwrap();
    let why = untold_why(&found[&n("beside")], &Comparison::Presence).unwrap();
    assert!(
        why.contains("where the docs are built is not known"),
        "{why}"
    );
    // Beside the Cargo.toml.
    write(r, "Cargo.toml", "[workspace]\n").await;
    assert_eq!(built(space(None)).await, ["beside"]);
    // The settings by their name of today, then the legacy name before it.
    write(r, ".cargo/config.toml", "[build]\ntarget-dir = \"built\"\n").await;
    assert_eq!(built(space(None)).await, ["configured"]);
    write(r, ".cargo/config", "[build]\ntarget-dir = \"old\"\n").await;
    assert_eq!(built(space(None)).await, ["legacy"]);
    // The variable before the settings.
    assert_eq!(built(space(Some("env"))).await, ["variable"]);

    #[cfg(unix)]
    {
        // Settings through a link out of the root tell nothing, and say why.
        tokio::fs::remove_file(r.join(".cargo/config"))
            .await
            .unwrap();
        tokio::fs::remove_file(r.join(".cargo/config.toml"))
            .await
            .unwrap();
        write(
            &t.outside,
            "config.toml",
            "[build]\ntarget-dir = \"built\"\n",
        )
        .await;
        link(t.outside.join("config.toml"), r, ".cargo/config.toml").await;
        let found = space(None)
            .resolve(&asks(&["configured"], Comparison::Presence))
            .await
            .unwrap();
        let why = untold_why(&found[&n("configured")], &Comparison::Presence).unwrap();
        assert!(
            why.contains("leads outside the root of the project, or is broken"),
            "{why}"
        );
    }
}

// The built docs of `rust-item` at the two sides of a change (artifact/rust_item.rs,
// `docs_at`): each side tells where its docs are from its own tree, and reads its own;
// docs outside the project at a side are told at neither.

/// The crates of `space` that resolve as there at `side`, among `crates`.
async fn built_at(space: &RustItemSpace, side: Side, crates: &[&'static str]) -> Vec<&'static str> {
    use whyshelf_core::app::shelf::SpaceAdapter;
    let found = SpaceAdapter::resolve(space, side, &asks(crates, Comparison::Presence))
        .await
        .unwrap();
    crates
        .iter()
        .copied()
        .filter(|krate| found[&n(krate)].is_present())
        .collect()
}

/// T32: with a different `build.target-dir` before and after a
/// change, each side reads the docs where its own settings put them.
#[tokio::test]
async fn t32_each_side_reads_the_built_docs_its_own_settings_name() {
    let t = tree().await;
    let (before, after) = (t.outside.clone(), t.root.clone());
    for side in [&before, &after] {
        write(side, "old/doc/olden/index.html", &page()).await;
        write(side, "new/doc/newer/index.html", &page()).await;
    }
    write(
        &before,
        ".cargo/config.toml",
        "[build]\ntarget-dir = \"old\"\n",
    )
    .await;
    write(
        &after,
        ".cargo/config.toml",
        "[build]\ntarget-dir = \"new\"\n",
    )
    .await;
    let space = RustItemSpace::on(FileStates::between(before, after), None);
    let crates = ["olden", "newer"];
    assert_eq!(built_at(&space, Side::Before, &crates).await, ["olden"]);
    assert_eq!(built_at(&space, Side::After, &crates).await, ["newer"]);
}

/// T33: an absolute `CARGO_TARGET_DIR` under the root of the side after and not under
/// the directory of the side before is outside the project at the side before, which
/// cannot be told in a run of two sides; the side after reads it.
#[tokio::test]
async fn t33_built_docs_outside_the_side_before_are_untold_there_and_read_at_the_side_after() {
    let t = tree().await;
    let (before, after) = (t.outside.clone(), t.root.clone());
    write(&after, "env/doc/demo/index.html", &page()).await;
    let space = RustItemSpace::on(
        FileStates::between(before, after.clone()),
        Some(after.join("env")),
    );
    assert_eq!(built_at(&space, Side::After, &["demo"]).await, ["demo"]);
    use whyshelf_core::app::shelf::SpaceAdapter;
    let found = SpaceAdapter::resolve(&space, Side::Before, &asks(&["demo"], Comparison::Presence))
        .await
        .unwrap();
    let why = untold_why(&found[&n("demo")], &Comparison::Presence).unwrap();
    assert!(
        why.contains(
            "the built docs are outside the project, so the two sides cannot be read alike"
        ),
        "{why}"
    );
}
