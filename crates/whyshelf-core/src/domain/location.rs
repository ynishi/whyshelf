//! Where things are: a file in the project, a line of code, a source outside.
//!
//! The domain does not touch the file system, so [`RelPath`] is only a checked
//! *name*: relative to the project root, `/`-separated, with no way to climb out
//! of the project. Whether a file exists there is the adapters' business.

use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::{GlobProblem, ValueError};
use super::nonempty::NonBlank;

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

impl ToCanon for RelPath {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// The files that count as tests: globs relative to the project root, checked.
///
/// Which pins are found is decided by where they are written (spec 3.1: only a test
/// file carries a pin that counts). The set holds the globs as the settings wrote them
/// (`tests`, `src/**/*_test.go`); matching them against the files of a revision is the
/// adapter's work, which hands them to the version-control tool. The domain checks
/// only that a glob names a place inside the project and cannot be taken for an option
/// or for a magic of the tool.
///
/// A language that keeps its tests in the file they test (Rust's `#[cfg(test)] mod
/// tests`) cannot narrow the set by file: the whole of the scan directories is the set
/// ([`TestFileSet::whole`]), which is also what a settings file without `files` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFileSet(Vec<String>);

impl TestFileSet {
    /// A set of globs; an empty list and any glob that breaks a rule are refused.
    pub fn new(globs: Vec<String>) -> Result<Self, ValueError> {
        if globs.is_empty() {
            return Err(ValueError::TestGlob {
                glob: String::new(),
                reason: GlobProblem::NoGlobs,
            });
        }
        for glob in &globs {
            let refuse = |reason| ValueError::TestGlob {
                glob: glob.clone(),
                reason,
            };
            if glob.trim().is_empty() {
                return Err(refuse(GlobProblem::Empty));
            }
            if glob.starts_with('/') {
                return Err(refuse(GlobProblem::Absolute));
            }
            if glob.starts_with(':') {
                return Err(refuse(GlobProblem::Magic));
            }
            if glob.contains('\\') || glob.chars().any(char::is_control) {
                return Err(refuse(GlobProblem::BadChar));
            }
            if glob.split('/').any(|segment| segment == "..") {
                return Err(refuse(GlobProblem::Climbs));
            }
        }
        Ok(Self(globs))
    }

    /// The whole of these directories: every file under them is a test file. `None`
    /// when there is no directory, which leaves no file.
    pub fn whole(dirs: &[RelPath]) -> Option<Self> {
        let globs: Vec<String> = dirs.iter().map(|dir| dir.as_str().to_owned()).collect();
        Self::new(globs).ok()
    }

    /// The globs as written.
    pub fn globs(&self) -> &[String] {
        &self.0
    }
}

/// A place in the code: a file and a line, as in `src/cache.rs:204`.
///
/// For a `code_observation` Finding this is a record of what was looked at, at
/// the time it was looked at. It is not kept in step with the code; a fact that
/// must follow the code is a PIN instead.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodeLocation {
    path: RelPath,
    line: NonZeroU32,
}

impl CodeLocation {
    /// A location from a path and a line (1-based).
    pub fn new(path: RelPath, line: u32) -> Result<Self, ValueError> {
        let line = NonZeroU32::new(line).ok_or(ValueError::LocationZeroLine)?;
        Ok(Self { path, line })
    }

    /// The file.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// The line, starting at 1.
    pub fn line(&self) -> u32 {
        self.line.get()
    }
}

impl fmt::Display for CodeLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.path, self.line)
    }
}

impl FromStr for CodeLocation {
    type Err = ValueError;

    /// Parses `path:line`; the last `:` separates the line.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (path, line) = s.rsplit_once(':').ok_or(ValueError::LocationNoLine)?;
        let path = path
            .parse::<RelPath>()
            .map_err(|e| ValueError::LocationBadPath(Box::new(e)))?;
        let line: u32 = line.parse().map_err(ValueError::LocationBadLine)?;
        Self::new(path, line)
    }
}

impl ToCanon for CodeLocation {
    fn to_canon(&self) -> Canon {
        Canon::str(self.to_string())
    }
}

/// A URI with a scheme, such as `https://example.org/paper`.
///
/// Only the outline is checked (a scheme, something after it, no white space);
/// reaching the address is not the domain's concern.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uri(String);

impl Uri {
    /// The URI as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Uri {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.chars().any(char::is_whitespace) {
            return Err(ValueError::UriWhitespace);
        }
        let (scheme, rest) = s.split_once(':').ok_or(ValueError::UriNoScheme)?;
        let mut chars = scheme.chars();
        let starts_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
        if !starts_ok {
            return Err(ValueError::UriNoScheme);
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) {
            return Err(ValueError::UriBadScheme);
        }
        if rest.is_empty() {
            return Err(ValueError::UriEmptyRest);
        }
        Ok(Self(s.to_owned()))
    }
}

/// Where an `external_source` Finding comes from.
///
/// Either an address that has a scheme, or a bibliographic citation written in
/// free text when there is no address (a book, a paper without a stable URL).
/// The two are different variants so a citation is never mistaken for a link.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceUri {
    /// A URI with a scheme.
    Uri(Uri),
    /// A bibliographic citation, free text.
    Citation(NonBlank),
}

impl FromStr for SourceUri {
    type Err = ValueError;

    /// A text that parses as a URI is a URI; any other non-blank text is a
    /// citation. A blank text is refused.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<Uri>() {
            Ok(uri) => Ok(Self::Uri(uri)),
            Err(_) => Ok(Self::Citation(NonBlank::new(s)?)),
        }
    }
}

impl fmt::Display for SourceUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uri(uri) => fmt::Display::fmt(uri, f),
            Self::Citation(text) => fmt::Display::fmt(text, f),
        }
    }
}

impl ToCanon for SourceUri {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Uri(uri) => Canon::map([("uri", Canon::str(uri.as_str()))]),
            Self::Citation(text) => Canon::map([("citation", text.to_canon())]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn globs(texts: &[&str]) -> Result<TestFileSet, ValueError> {
        TestFileSet::new(texts.iter().map(|t| (*t).to_owned()).collect())
    }

    #[test]
    fn a_test_glob_names_a_place_inside_the_project_and_is_no_magic() {
        assert_eq!(
            globs(&["tests", "src/**/*_test.go"]).unwrap().globs(),
            ["tests", "src/**/*_test.go"]
        );
        let reason = |texts: &[&str]| match globs(texts) {
            Err(ValueError::TestGlob { reason, .. }) => reason,
            other => panic!("expected a glob error, got {other:?}"),
        };
        assert_eq!(reason(&[]), GlobProblem::NoGlobs);
        assert_eq!(reason(&["  "]), GlobProblem::Empty);
        assert_eq!(reason(&["/etc"]), GlobProblem::Absolute);
        assert_eq!(reason(&[":(top)src"]), GlobProblem::Magic);
        assert_eq!(reason(&["src/../.."]), GlobProblem::Climbs);
        assert_eq!(reason(&["a\\b"]), GlobProblem::BadChar);
        assert_eq!(reason(&["ok", "line\nbreak"]), GlobProblem::BadChar);
    }

    #[test]
    fn the_whole_of_the_scan_directories_is_a_set_of_their_names() {
        let dirs: Vec<RelPath> = ["src", "tests"]
            .iter()
            .map(|d| d.parse().unwrap())
            .collect();
        assert_eq!(TestFileSet::whole(&dirs).unwrap().globs(), ["src", "tests"]);
        assert!(TestFileSet::whole(&[]).is_none());
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

    #[test]
    fn code_location_parses_file_and_line() {
        let loc: CodeLocation = "src/cache.rs:204".parse().unwrap();
        assert_eq!(loc.path().as_str(), "src/cache.rs");
        assert_eq!(loc.line(), 204);
        assert_eq!(loc.to_string(), "src/cache.rs:204");
    }

    #[test]
    fn code_location_rejects_each_malformed_shape() {
        assert_eq!(
            "src/a.rs".parse::<CodeLocation>(),
            Err(ValueError::LocationNoLine)
        );
        assert_eq!(
            "src/a.rs:0".parse::<CodeLocation>(),
            Err(ValueError::LocationZeroLine)
        );
        assert!(matches!(
            "src/a.rs:x".parse::<CodeLocation>(),
            Err(ValueError::LocationBadLine(_))
        ));
        assert!(matches!(
            "/abs:3".parse::<CodeLocation>(),
            Err(ValueError::LocationBadPath(_))
        ));
        assert!(matches!(
            "src/a.rs:-1".parse::<CodeLocation>(),
            Err(ValueError::LocationBadLine(_))
        ));
    }

    #[test]
    fn uri_needs_a_scheme_and_no_white_space() {
        assert!("https://example.org/x".parse::<Uri>().is_ok());
        assert_eq!("example.org".parse::<Uri>(), Err(ValueError::UriNoScheme));
        assert_eq!("1http://x".parse::<Uri>(), Err(ValueError::UriNoScheme));
        assert_eq!("ht tp://x".parse::<Uri>(), Err(ValueError::UriWhitespace));
        assert_eq!("h_t:x".parse::<Uri>(), Err(ValueError::UriBadScheme));
        assert_eq!("https:".parse::<Uri>(), Err(ValueError::UriEmptyRest));
    }

    #[test]
    fn source_is_a_uri_or_a_citation() {
        assert!(matches!(
            "https://example.org/p".parse::<SourceUri>().unwrap(),
            SourceUri::Uri(_)
        ));
        assert!(matches!(
            "A. Writer, Caching in Practice, 2020"
                .parse::<SourceUri>()
                .unwrap(),
            SourceUri::Citation(_)
        ));
        assert_eq!("  ".parse::<SourceUri>(), Err(ValueError::BlankText));
    }

    #[test]
    fn uri_and_citation_with_equal_text_hash_differently() {
        let uri = SourceUri::Uri("a:b".parse().unwrap());
        let cite = SourceUri::Citation(NonBlank::new("a:b").unwrap());
        assert_ne!(uri.to_canon(), cite.to_canon());
    }
}
