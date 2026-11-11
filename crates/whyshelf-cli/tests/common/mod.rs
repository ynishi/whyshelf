//! What the end-to-end tests of the command line share: a small project of files,
//! the binary run on it, its answer read as JSON, and the reading commands.
//!
//! Each project is made under the system's temporary directory with the standard
//! library only, and removed when the test ends.

// Each test file includes this module and uses only some of it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const BOOKS: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
"#;

pub(crate) const NOTE: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n";

/// A project directory that is removed when it goes out of scope.
pub(crate) struct Project(pub(crate) PathBuf);

/// How many projects this process has made: with the name the test gives, it keeps
/// two tests that run at once (in the same tick of a coarse clock) out of one
/// directory.
static MADE: AtomicUsize = AtomicUsize::new(0);

impl Project {
    pub(crate) fn new(name: &str) -> Self {
        Self::with(name, BOOKS, NOTE)
    }

    /// A project with this declaration of books and this text as `notes/N-1.md`.
    pub(crate) fn with(name: &str, books: &str, note: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let count = MADE.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "whyshelf-{name}-{}-{count}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join(".whyshelf")).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join(".whyshelf/books.toml"), books).unwrap();
        fs::write(dir.join("notes/N-1.md"), note).unwrap();
        Self(dir)
    }

    pub(crate) fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_whyshelf"))
            .arg("--root")
            .arg(&self.0)
            .args(args)
            .env("WHYSHELF_ACTOR", "tester")
            .output()
            .expect("the binary runs")
    }

    /// Every file under the project with its bytes, and every directory (with none),
    /// so that a directory made empty counts too.
    pub(crate) fn files(&self) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                // A directory is what can be listed (the workspace's arch-lint refuses
                // `is_dir`, a blocking call, in every file).
                if fs::read_dir(&path).is_ok() {
                    out.insert(path.clone(), None);
                    walk(&path, out);
                } else {
                    out.insert(path.clone(), Some(fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.0, &mut out);
        out
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// The reading commands, each with the arguments it runs with on [`NOTE`].
pub(crate) const READS: [&[&str]; 14] = [
    &["fmt", "--check"],
    &["check"],
    &["map"],
    &["status"],
    &["unlinked"],
    &["stale"],
    &["stale", "--exit-code"],
    &["get", "N-1"],
    &["trace", "N-1"],
    &["get", "N-1", "--approved"],
    &["diff", "N-1"],
    &["schema"],
    &["stats"],
    &["moves"],
];
