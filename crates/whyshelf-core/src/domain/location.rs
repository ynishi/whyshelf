//! Where a file is: a path relative to the project root, and the pattern that takes
//! some of them.
//!
//! The domain does not touch the file system, so [`RelPath`] is only a checked
//! *name*: relative to the project root, `/`-separated, with no way to climb out
//! of the project. Whether a file exists there is the adapters' business. A
//! [`PathGlob`] is the one grammar of a pattern over such paths, and the one place
//! one is matched: the range of a book and the selection of the files of a space
//! both call it.

use std::fmt;
use std::str::FromStr;

use globset::{Glob, GlobBuilder, GlobMatcher};
use thiserror::Error;

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

/// Why a text is not a [`PathGlob`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GlobProblem {
    /// The pattern is empty.
    #[error("is empty")]
    Empty,
    /// The pattern is absolute: it is relative to the base it is matched below.
    #[error("is absolute, and a pattern is relative to the base it is matched below")]
    Absolute,
    /// The pattern has a `..` segment, which leaves the base.
    #[error("has a `..` segment, which leaves the base it is matched below")]
    ParentSegment,
    /// The pattern begins with `!`, which richer grammars read as a negation and
    /// this one would match as itself: what to leave out is written apart.
    #[error("begins with `!`: write what to leave out apart, as an exclude")]
    Negated,
    /// The pattern holds `\`: `/` is the one separator, and nothing is escaped.
    #[error("holds `\\`: `/` is the one separator of a pattern, and nothing is escaped")]
    Backslash,
    /// The pattern has an empty or `.` segment, which no path has.
    #[error("has an empty or `.` segment, which no path has")]
    EmptySegment,
    /// The pattern holds `[`, `]`, `{` or `}`: classes and alternatives are not in
    /// the grammar, which has `*`, `?` and `**` only.
    #[error("holds `{ch}`, and a pattern has only `*`, `?` and `**`: no class, no alternative")]
    Reserved {
        /// The character.
        ch: char,
    },
    /// `**` is written beside other characters in a segment (`sub**`, `**sub`).
    #[error("writes `**` inside a segment: `**` is a whole segment, between `/`s")]
    SplitDoubleStar,
    /// The pattern is not a glob.
    #[error("is not a glob: {why}")]
    NotAGlob {
        /// What the glob parser said.
        why: String,
    },
}

/// The characters of the grammars of classes and alternatives, refused.
const RESERVED: [char; 4] = ['[', ']', '{', '}'];

/// A pattern over the `/`-separated paths below a base, checked once: the one
/// grammar of a segment glob, and the one place it is matched.
///
/// The path matched has `/` between its segments, no leading `/`, and is matched
/// against the pattern segment by segment:
///
/// | written | matches |
/// |---|---|
/// | `**`, a whole segment | any number of segments, none included |
/// | `*` in a segment | any characters of one segment, none included |
/// | `?` in a segment | one character of one segment |
/// | any other character | itself, byte for byte |
///
/// So `*.md` is the files of the base itself, `**/*.md` every file below it,
/// `archive/**` everything under `archive`. Case is matched as written (the ASCII
/// case rule of the space `file`), and Unicode is compared byte for byte, so a
/// pattern in NFC does not take a name in NFD.
///
/// Every other form is refused ([`GlobProblem`]), so that nothing is matched
/// otherwise than it reads: an empty pattern, an absolute one (`/x`), a leading `!`
/// (a negation elsewhere), a `\` (an escape or a separator elsewhere), `[`, `]`,
/// `{` and `}` (the classes and alternatives of richer grammars, which could cross
/// a `/`), an empty, `.` or `..` segment, and a `**` inside a segment (`sub**`).
///
/// The matching is done by `globset` (the glob crate of ripgrep: pure, mature), held
/// to this grammar: `*` and `?` stop at `/` (`literal_separator`), case as written,
/// no escape. What a path must also be to count (the `.md` of a book's documents)
/// is the caller's, never this grammar's.
#[derive(Debug, Clone)]
pub struct PathGlob {
    text: String,
    matcher: GlobMatcher,
    /// For a pattern that ends in a whole segment `**` (`archive/**`, `**`): what
    /// the pattern takes everything under.
    under: Option<Under>,
}

/// What a pattern ending in `/**` takes everything under.
#[derive(Debug, Clone)]
enum Under {
    /// `**`: every directory.
    Everything,
    /// `<prefix>/**`: every directory the prefix matches, and what is below it.
    Prefix(GlobMatcher),
}

impl PartialEq for PathGlob {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for PathGlob {}

impl PathGlob {
    /// A pattern from text, refused when it is not a relative glob that stays below
    /// its base.
    pub fn new(text: &str) -> Result<Self, GlobProblem> {
        if text.is_empty() {
            return Err(GlobProblem::Empty);
        }
        if text.starts_with('/') {
            return Err(GlobProblem::Absolute);
        }
        if text.starts_with('!') {
            return Err(GlobProblem::Negated);
        }
        if text.contains('\\') {
            return Err(GlobProblem::Backslash);
        }
        if let Some(ch) = text.chars().find(|c| RESERVED.contains(c)) {
            return Err(GlobProblem::Reserved { ch });
        }
        let segments: Vec<&str> = text.split('/').collect();
        if segments.contains(&"..") {
            return Err(GlobProblem::ParentSegment);
        }
        if segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == ".")
        {
            return Err(GlobProblem::EmptySegment);
        }
        if segments
            .iter()
            .any(|segment| *segment != "**" && segment.contains("**"))
        {
            return Err(GlobProblem::SplitDoubleStar);
        }
        let under = match text.strip_suffix("**") {
            Some("") => Some(Under::Everything),
            Some(prefix) => match prefix.strip_suffix('/') {
                Some(prefix) => Some(Under::Prefix(glob(prefix)?)),
                None => None,
            },
            None => None,
        };
        Ok(Self {
            text: text.to_owned(),
            matcher: glob(text)?,
            under,
        })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the pattern takes `path`, a path below the base with `/` between its
    /// segments and no leading `/`.
    pub fn matches(&self, path: &str) -> bool {
        self.matcher.is_match(path)
    }

    /// Whether the pattern takes every path under the directory `dir` (below the
    /// base): it ends in a whole segment `**`, and what comes before it matches
    /// `dir` or a directory above it. Since `*` and `?` stop at `/`, a prefix that
    /// matches a directory takes it whole.
    pub(crate) fn takes_all_under(&self, dir: &str) -> bool {
        match &self.under {
            None => false,
            Some(Under::Everything) => true,
            Some(Under::Prefix(prefix)) => {
                let mut end = 0;
                dir.split('/').any(|segment| {
                    end += segment.len();
                    let above = &dir[..end];
                    end += 1;
                    prefix.is_match(above)
                })
            }
        }
    }
}

impl fmt::Display for PathGlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The matcher of a text of the grammar: `*` and `?` stop at `/`, case as written,
/// and `\` is no escape (it is refused before).
fn glob(text: &str) -> Result<GlobMatcher, GlobProblem> {
    let glob: Glob = GlobBuilder::new(text)
        .literal_separator(true)
        .case_insensitive(false)
        .backslash_escape(false)
        .build()
        .map_err(|error| GlobProblem::NotAGlob {
            why: error.kind().to_string(),
        })?;
    Ok(glob.compile_matcher())
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
