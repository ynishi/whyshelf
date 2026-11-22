//! Reading the workspace's own source: where it is, its files but those of tests, and
//! the code of a file with its comments and the contents of its literals taken out.

use std::path::{Path, PathBuf};

pub const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

/// The workspace's root, two directories above this crate's.
pub fn workspace() -> &'static Path {
    Path::new(MANIFEST).parent().unwrap().parent().unwrap()
}

#[arch_lint::allow(
    no_sync_io,
    reason = "a test with no async runtime reads the workspace's own source; whyshelf-core has no I/O crate"
)]
pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[arch_lint::allow(
    no_sync_io,
    reason = "a test with no async runtime lists the workspace's own source; whyshelf-core has no I/O crate"
)]
pub fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
}

pub fn is_tests(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_str().unwrap();
    name == "tests.rs" || name.ends_with("_tests.rs")
}

pub fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `source` with its comments and the contents of its string and char literals taken out,
/// its lines kept, and cut where a module of tests written inline begins.
///
/// Only a module of tests named `tests` and written as `#[cfg(test)]` on the line above
/// it is cut: a module of tests of another name is read as code.
pub fn code(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let keep_lines = |out: &mut String, text: &[char]| {
        out.extend(text.iter().filter(|c| **c == '\n'));
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let after_ident = i > 0 && is_ident(chars[i - 1]);
        // The `r` of `r".."`, or of `br".."` and `cr".."` after their `b` or `c`.
        let raw_start = !after_ident
            || (matches!(chars[i - 1], 'b' | 'c') && !(i > 1 && is_ident(chars[i - 2])));
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let start = i;
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            keep_lines(&mut out, &chars[start..i.min(chars.len())]);
        } else if c == 'r' && raw_start && matches!(next, Some('"' | '#')) {
            let mut j = i + 1;
            while chars.get(j) == Some(&'#') {
                j += 1;
            }
            if chars.get(j) != Some(&'"') {
                out.push(c);
                i += 1;
                continue;
            }
            let hashes = j - i - 1;
            let start = j;
            j += 1;
            while j < chars.len()
                && !(chars[j] == '"' && (1..=hashes).all(|h| chars.get(j + h) == Some(&'#')))
            {
                j += 1;
            }
            keep_lines(&mut out, &chars[start..j.min(chars.len())]);
            out.push_str("\"\"");
            i = j + 1 + hashes;
        } else if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            keep_lines(&mut out, &chars[start..i.min(chars.len())]);
            out.push_str("\"\"");
            i += 1;
        } else if c == '\'' && next == Some('\\') {
            let mut j = i + 3;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            out.push_str("' '");
            i = j + 1;
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            out.push_str("' '");
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    match out.find("\n#[cfg(test)]\nmod tests {") {
        Some(at) => out[..at].to_owned(),
        None => out,
    }
}

/// Every file of the source but those of tests, under `dir`.
#[arch_lint::allow(
    no_sync_io,
    reason = "a test with no async runtime lists the workspace's own source; whyshelf-core has no I/O crate"
)]
pub fn sources(dir: &Path) -> Vec<PathBuf> {
    entries(dir)
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                sources(&path)
            } else if path.extension().is_some_and(|e| e == "rs") && !is_tests(&path) {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}
