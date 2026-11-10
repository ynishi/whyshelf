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

#[cfg(test)]
mod tests {
    use super::*;

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
