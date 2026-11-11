//! This repository's own verifications name the end-to-end tests feature by feature:
//! no file under `crates/whyshelf-cli/tests/` but the helpers they share
//! (`common/mod.rs`) is named in `tests` by more than two verifications, so an edit
//! of one feature's tests leaves the approvals of the others fresh; and
//! `common/mod.rs` holds helpers only, no test.
//!
//! A verification's `tests` is read as the books' one form (`whyshelf fmt`) writes
//! it: one line, `tests: ` and a JSON array of strings, in a file of LF line ends.
//! Anything else written as `tests` fails the guard, with the file named. A
//! verification with no `tests` at all (a `review`) names no file.
//!
//! What it does not see:
//!
//! - names outside `crates/whyshelf-cli/tests/`: they are not counted;
//! - files a test file pulls in through `mod` or `#[path]`: only the names written
//!   in `tests` are counted, not what each file includes;
//! - a name written in another letter case than the file's (`Answers.rs`): it is
//!   counted as its own name; `whyshelf check` reports it as absent.
//!
//! Read-only: the files of the book `verification` and `common/mod.rs` are read, and
//! nothing else.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The most verifications that may name one file of the end-to-end tests.
const MOST: usize = 2;

const TESTS_DIR: &str = "crates/whyshelf-cli/tests/";

/// The helpers every file of the end-to-end tests includes; any number may name it.
const SHARED: &str = "crates/whyshelf-cli/tests/common/mod.rs";

/// The names in the `tests` entry of a document's front matter, each once, or why
/// they cannot be read.
fn tests_of(text: &str) -> Result<BTreeSet<String>, String> {
    if text.contains('\r') {
        return Err("it has CRLF line ends; the one form writes LF".to_owned());
    }
    let front = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(front, _)| front)
        .ok_or("it does not open with front matter between `---` lines")?;
    let entries: Vec<&str> = front
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "tests")
        })
        .collect();
    match entries[..] {
        [] => Ok(BTreeSet::new()),
        [line] => line
            .strip_prefix("tests: ")
            .and_then(|list| serde_json::from_str::<Vec<String>>(list).ok())
            .map(|names| names.into_iter().collect())
            .ok_or_else(|| {
                format!("`{line}` is not `tests: ` and a JSON array of strings on one line")
            }),
        _ => Err(format!("it writes `tests` {} times", entries.len())),
    }
}

#[test]
fn no_file_of_the_end_to_end_tests_is_named_by_more_than_two_verifications() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("books/verification");
    let mut problems = Vec::new();

    let shared = root.join(SHARED);
    match fs::read_to_string(&shared) {
        Ok(text) if text.contains("#[test]") => problems.push(format!(
            "{SHARED} holds a #[test]; it is for helpers only: move the test to its feature's file"
        )),
        Ok(_) => {}
        Err(e) => problems.push(format!("{SHARED} cannot be read: {e}")),
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} cannot be listed: {e}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("{} cannot be listed: {e}", dir.display()))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    paths.sort();

    let mut named: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in paths {
        let key = path.file_stem().unwrap().to_string_lossy().into_owned();
        let read = fs::read_to_string(&path)
            .map_err(|e| format!("it cannot be read: {e}"))
            .and_then(|text| tests_of(&text));
        match read {
            Ok(names) => {
                for file in names {
                    if file.starts_with(TESTS_DIR) && file != SHARED {
                        named.entry(file).or_default().push(key.clone());
                    }
                }
            }
            Err(why) => problems.push(format!("{}: {why}", path.display())),
        }
    }

    for (file, mut keys) in named {
        if keys.len() > MOST {
            // V-9 before V-10: shorter keys first, then as written.
            keys.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            problems.push(format!(
                "{file} is named by {} verifications, more than {MOST}: {}; split it by feature",
                keys.len(),
                keys.join(", ")
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
