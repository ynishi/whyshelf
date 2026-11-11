//! `stats` on the binary and a small project of files: the answer has its version,
//! counts two approvals of one note as one approved again and two runs, carries a
//! drawing, is the same (its drawing too) twice, echoes `--since` and lists no
//! approval before it, is the error of a request for a `--since` that is no time,
//! and creates and changes no file.
//!
//! The project is made under the system's temporary directory with the standard
//! library only, and removed when the test ends.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A project of one book of notes and the note N-1, removed when dropped.
struct Project(PathBuf);

/// How many projects this process has made: with the test's name, it keeps two
/// tests that run at once (in the same tick of a coarse clock) out of one directory.
static MADE: AtomicUsize = AtomicUsize::new(0);

impl Project {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let count = MADE.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "whyshelf-stats-{name}-{}-{count}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join(".whyshelf")).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(
            dir.join(".whyshelf/books.toml"),
            "[[book]]\nname = \"note\"\ndir = \"notes\"\nkey = { prefix = \"N\" }\n",
        )
        .unwrap();
        fs::write(
            dir.join("notes/N-1.md"),
            "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\n---\nThe body.\n",
        )
        .unwrap();
        Self(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_whyshelf"))
            .arg("--root")
            .arg(&self.0)
            .args(args)
            .env("WHYSHELF_ACTOR", "tester")
            .output()
            .expect("the binary runs")
    }

    /// Every path under the project with its bytes (none for a directory, which is
    /// what can be listed).
    fn files(&self) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
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

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

const READS: [&[&str]; 2] = [&["stats"], &["stats", "--since", "2020-01-01"]];

#[test]
fn stats_answers_from_the_ledger_and_the_same_twice() {
    let project = Project::new("answer");
    for _ in 0..2 {
        assert_eq!(
            project.run(&["judge", "approve", "N-1"]).status.code(),
            Some(0)
        );
    }
    let first = project.run(&["stats"]);
    assert_eq!(first.status.code(), Some(0));
    let answer = json(&first);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["since"], serde_json::Value::Null);
    let doc = &answer["approvals"]["docs"][0];
    assert_eq!(doc["doc"]["key"], "note:N-1");
    assert_eq!(
        (doc["approvals"].as_u64(), doc["again"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(answer["approvals"]["books"][0]["book"], "note");
    assert_eq!(answer["pending"][0]["count"], 1);
    // Two `judge approve` runs, each one run.
    assert_eq!(answer["runs"]["count"], 2);
    // The drawing is text for a person, and the same on another run.
    let drawing = answer["drawing"].as_str().expect("the drawing is a string");
    assert!(!drawing.is_empty());
    assert_eq!(
        json(&project.run(&["stats"]))["drawing"].as_str(),
        Some(drawing),
        "the same drawing twice"
    );
    assert_eq!(
        first.stdout,
        project.run(&["stats"]).stdout,
        "the same twice"
    );

    let since = json(&project.run(&["stats", "--since", "2999-01-01"]));
    assert_eq!(since["since"], "2999-01-01");
    assert_eq!(since["approvals"]["docs"], serde_json::json!([]));
    let wrong = project.run(&["stats", "--since", "never"]);
    assert_eq!(wrong.status.code(), Some(2));
    assert_eq!(json(&wrong)["error"], "request");
}

#[test]
fn stats_creates_and_changes_no_file() {
    let project = Project::new("reads");
    // Before any approval: no ledger directory exists, and none may be made.
    let before = project.files();
    for args in READS {
        assert_eq!(json(&project.run(args))["version"], 1, "{args:?}");
    }
    assert_eq!(project.files(), before, "before an approval");
    assert!(!project.0.join(".whyshelf/ledger").exists());

    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    let before = project.files();
    for args in READS {
        project.run(args);
    }
    assert_eq!(project.files(), before, "after an approval");
}
