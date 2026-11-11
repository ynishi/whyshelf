//! Where a file is: a path relative to the project root.
//!
//! The domain does not touch the file system, so [`RelPath`] is only a checked
//! *name*: relative to the project root, `/`-separated, with no way to climb out
//! of the project. Whether a file exists there is the adapters' business.

use std::fmt;
use std::str::FromStr;

use super::error::ValueError;

/// A path relative to the project root, written with `/`.
///
/// No leading `/`, no empty, `.` or `..` segment, no backslash, no control
/// character. These rules keep a path from naming anything outside the project
/// and make equal places compare equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// The path as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RelPath {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ValueError::PathEmpty);
        }
        if s.starts_with('/') {
            return Err(ValueError::PathAbsolute);
        }
        if s.contains('\\') || s.chars().any(char::is_control) {
            return Err(ValueError::PathBadChar);
        }
        if s.split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return Err(ValueError::PathBadSegment);
        }
        Ok(Self(s.to_owned()))
    }
}

/// A path written in the program, held to the rules of [`RelPath`] when it is made:
/// made in a `const` item, a literal that breaks them fails the build, so the
/// [`RelPath`] made of it needs no check at run time and no error.
///
/// ```
/// use whyshelf_core::domain::location::{PathLiteral, RelPath};
/// const BOOKS: PathLiteral = PathLiteral::new(".whyshelf/books.toml");
/// assert_eq!(RelPath::from(BOOKS).as_str(), ".whyshelf/books.toml");
/// ```
///
/// ```compile_fail
/// use whyshelf_core::domain::location::PathLiteral;
/// const BAD: PathLiteral = PathLiteral::new("../out");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathLiteral(&'static str);

impl PathLiteral {
    /// The literal, held to the rules of [`RelPath`] (the same rules as its
    /// [`FromStr`]). Made in a `const` item, as it is meant to be, a literal that
    /// breaks them fails the build; outside one it would stop the program.
    pub const fn new(text: &'static str) -> Self {
        assert!(
            literal_is_relative(text),
            "a path literal is a relative path"
        );
        Self(text)
    }
}

impl From<PathLiteral> for RelPath {
    fn from(literal: PathLiteral) -> Self {
        Self(literal.0.to_owned())
    }
}

/// The rules of [`RelPath::from_str`], in a form a `const` can run: not empty, no
/// leading `/`, no backslash, no control character, no empty, `.` or `..` segment.
const fn literal_is_relative(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] == b'/' {
        return false;
    }
    let mut i = 0;
    let mut start = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'/' {
            let len = i - start;
            let dots = len <= 2 && {
                let mut all = true;
                let mut j = start;
                while j < i {
                    all = all && bytes[j] == b'.';
                    j += 1;
                }
                all
            };
            if len == 0 || dots {
                return false;
            }
            start = i + 1;
        } else {
            let b = bytes[i];
            // ASCII controls, and the C1 controls U+0080..U+009F (`0xC2 0x80..=0x9F`).
            let c1 =
                b == 0xC2 && i + 1 < bytes.len() && bytes[i + 1] >= 0x80 && bytes[i + 1] <= 0x9F;
            if b == b'\\' || b < 0x20 || b == 0x7F || c1 {
                return false;
            }
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_literal_is_held_to_the_rules_of_a_path() {
        for text in [
            "docs/spec/a.md",
            ".whyshelf/books.toml",
            "a/...",
            "a/.b",
            "ü/x",
        ] {
            assert!(literal_is_relative(text), "{text}");
            assert_eq!(
                RelPath::from(PathLiteral::new(text)),
                text.parse::<RelPath>().unwrap()
            );
        }
        for text in [
            "", "/etc/x", "a/../b", "a//b", "./a", "a\\b", "a\u{0}b", "a/", "a\u{85}b", "..",
        ] {
            assert!(!literal_is_relative(text), "{text:?}");
            assert!(text.parse::<RelPath>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn relative_paths_are_accepted() {
        assert_eq!(
            "docs/spec/a.md".parse::<RelPath>().unwrap().as_str(),
            "docs/spec/a.md"
        );
    }

    #[test]
    fn each_path_rule_has_its_own_error() {
        assert_eq!("".parse::<RelPath>(), Err(ValueError::PathEmpty));
        assert_eq!("/etc/x".parse::<RelPath>(), Err(ValueError::PathAbsolute));
        assert_eq!("a/../b".parse::<RelPath>(), Err(ValueError::PathBadSegment));
        assert_eq!("a//b".parse::<RelPath>(), Err(ValueError::PathBadSegment));
        assert_eq!("./a".parse::<RelPath>(), Err(ValueError::PathBadSegment));
        assert_eq!("a\\b".parse::<RelPath>(), Err(ValueError::PathBadChar));
        assert_eq!("a\u{0}b".parse::<RelPath>(), Err(ValueError::PathBadChar));
        assert_eq!("a/".parse::<RelPath>(), Err(ValueError::PathBadSegment));
    }
}
