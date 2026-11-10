//! The binary on a small project of files: `stale --exit-code` exits 1 when an
//! approval does not hold, and 0 when every one does; `stale` alone exits 0.
//!
//! The project is made under the system's temporary directory with the standard
//! library only, and removed at the end.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BOOKS: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
"#;

const NOTE: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n";

/// A fresh, empty project directory of its own.
fn project() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("whyshelf-stale-{}-{nanos}", std::process::id()));
    fs::create_dir_all(dir.join(".whyshelf")).expect("the project directory");
    fs::create_dir_all(dir.join("notes")).expect("the book directory");
    fs::write(dir.join(".whyshelf/books.toml"), BOOKS).expect("the declaration");
    fs::write(dir.join("notes/N-1.md"), NOTE).expect("the note");
    dir
}

/// The exit status of `whyshelf --root <dir> <args>`.
fn exit_of(dir: &Path, args: &[&str]) -> i32 {
    Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(dir)
        .args(args)
        .env("WHYSHELF_ACTOR", "tester")
        .output()
        .expect("the binary runs")
        .status
        .code()
        .expect("an exit status")
}

#[test]
fn stale_exit_code_follows_whether_an_approval_holds() {
    let dir = project();
    assert_eq!(exit_of(&dir, &["judge", "approve", "N-1"]), 0);
    assert_eq!(exit_of(&dir, &["stale", "--exit-code"]), 0, "fresh");

    fs::write(
        dir.join("notes/N-1.md"),
        NOTE.replace("The body.", "Another body."),
    )
    .unwrap();
    assert_eq!(exit_of(&dir, &["stale", "--exit-code"]), 1, "stale");
    assert_eq!(exit_of(&dir, &["stale"]), 0, "without the flag");

    fs::remove_dir_all(&dir).unwrap();
}
