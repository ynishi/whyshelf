//! The range of a book: which files under its directory are its documents.
//!
//! A book reads the `.md` files under its directory. A project brought into an
//! existing directory finds files there that are not documents, so the declaration
//! of a book may narrow the range with `include` and `exclude` (glob patterns
//! relative to the directory) and say what a file in the range with no front
//! matter is (`without_front_matter`). This module is the one owner of both
//! questions:
//!
//! ```text
//!   is this path a document of the book?     BookScope::admits / BookRange::takes
//!        listing a directory (adapter), the shelf (Schema::membership), and
//!        `new` and `init` (Shelf::claim_key, Shelf::next_key, Shelf::check_new)
//!        all ask it; nobody matches a pattern anywhere else
//!   what is a file in range with no front    WithoutFrontMatter, read by Shelf::build
//!   matter?                                  (the adapter says "no front matter" once,
//!                                            beside its reader of a front matter)
//! ```
//!
//! # The rule
//!
//! A file is a document of a book when it is a `.md` file under the book's
//! directory, its path below the directory matches some `include` (every file when
//! `include` is not written) and matches no `exclude`. The path matched is the one
//! below the directory, with `/` between its segments, and a pattern is matched
//! against it segment by segment, in the grammar of [`PathGlob`] (`**`, `*` and `?`
//! only; the table is on its page), which this module calls and does not copy.
//! So `*.md` is the files of the directory itself, `**/*.md` every file below it,
//! `archive/**` everything under `archive`.
//!
//! Every form [`PathGlob`] refuses is refused here, as a problem of the declaration
//! ([`SchemaProblem::BadScopePattern`](super::SchemaProblem::BadScopePattern)), and
//! so is one thing more, which is the book's own: a pattern whose last segment has
//! no `*` or `?` and does not end in `.md`, which names a directory or a file that
//! is no document and so takes nothing (`archive` for `archive/**`). A [`Pattern`]
//! is a [`PathGlob`] with that rule.
//!
//! The range does not change which books may share a directory: two books whose
//! directories are the same or nest are still refused
//! ([`SchemaProblem::DirOverlap`](super::SchemaProblem::DirOverlap)), even when their
//! ranges would not meet, so a file is under at most one book's directory and a
//! book's range is the only thing asked of it.
//!
//! Why the matching is here and not in the adapter that lists the files: `new` and
//! `init` must refuse a path the range does not take, and the list of files must
//! agree with them to the letter. Matching is pure computation, so the domain stays
//! pure, and there is one rule to test.

use std::fmt;

use thiserror::Error;

use crate::domain::location::{GlobProblem, PathGlob, RelPath};
use crate::domain::words::{Join, Mark, listed};

use super::{BookDir, BookEntry, BookWord, Words, read_book_word, report};

/// What a file in a book's range with no front matter at all is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WithoutFrontMatter {
    /// An invalid document (the default).
    #[default]
    Invalid,
    /// Not a document: left out of the book, and told by `check`. A file whose front
    /// matter is there but broken is invalid whatever this says.
    Skip,
}

impl WithoutFrontMatter {
    /// Every value, in the order a problem lists their words.
    pub const ALL: [Self; 2] = [Self::Invalid, Self::Skip];

    /// The word of the declaration.
    pub fn word(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Skip => "skip",
        }
    }
}

impl Words for WithoutFrontMatter {
    fn all() -> &'static [Self] {
        &Self::ALL
    }

    fn word(self) -> &'static str {
        WithoutFrontMatter::word(self)
    }
}

impl BookWord for WithoutFrontMatter {
    const ENTRY: BookEntry = BookEntry::WithoutFrontMatter;
}

impl fmt::Display for WithoutFrontMatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// Which list of patterns a pattern is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeList {
    /// `include`.
    Include,
    /// `exclude`.
    Exclude,
}

impl fmt::Display for ScopeList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Include => "include",
            Self::Exclude => "exclude",
        })
    }
}

/// The range of a book as declared: text and lists, nothing checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeDecl {
    /// `include`, when written.
    pub include: Option<Vec<String>>,
    /// `exclude`, when written.
    pub exclude: Option<Vec<String>>,
    /// `without_front_matter`, as written, when written.
    pub without_front_matter: Option<String>,
}

/// Why a text is not a pattern of a book's range.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatternProblem {
    /// The pattern is empty.
    #[error("is empty")]
    Empty,
    /// The pattern is absolute: it is relative to the directory of the book.
    #[error("is absolute, and a pattern is relative to the directory of the book")]
    Absolute,
    /// The pattern has a `..` segment, which leaves the directory of the book.
    #[error("has a `..` segment, which leaves the directory of the book")]
    ParentSegment,
    /// The pattern begins with `!`, which richer grammars read as a negation and
    /// this one would match as itself: what to leave out is written in `exclude`.
    #[error("begins with `!`: write what to leave out in `exclude`")]
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
    /// The last segment has no `*` or `?` and does not end in `.md`: the pattern
    /// names a directory, or a file that is no document, and takes no file.
    #[error("names no `.md` file: to take everything under a directory, write `{text}/**`")]
    NoFile {
        /// The pattern, as written.
        text: String,
    },
    /// The pattern is not a glob.
    #[error("is not a glob: {why}")]
    NotAGlob {
        /// What the glob parser said.
        why: String,
    },
}

impl From<GlobProblem> for PatternProblem {
    /// The same refusal, in the words of a book's range.
    fn from(problem: GlobProblem) -> Self {
        match problem {
            GlobProblem::Empty => Self::Empty,
            GlobProblem::Absolute => Self::Absolute,
            GlobProblem::ParentSegment => Self::ParentSegment,
            GlobProblem::Negated => Self::Negated,
            GlobProblem::Backslash => Self::Backslash,
            GlobProblem::EmptySegment => Self::EmptySegment,
            GlobProblem::Reserved { ch } => Self::Reserved { ch },
            GlobProblem::SplitDoubleStar => Self::SplitDoubleStar,
            GlobProblem::NotAGlob { why } => Self::NotAGlob { why },
        }
    }
}

/// A pattern of a book's range: a [`PathGlob`] relative to the directory of the
/// book, with the one rule that is the book's (its last segment names a `.md` file or
/// has a wildcard), checked once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    glob: PathGlob,
}

impl Pattern {
    /// A pattern from text, refused when it is not a relative glob that stays in the
    /// directory.
    pub fn new(text: &str) -> Result<Self, PatternProblem> {
        let glob = PathGlob::new(text)?;
        let last = text.rsplit('/').next().unwrap_or(text);
        if !last.contains(['*', '?']) && !last.ends_with(".md") {
            return Err(PatternProblem::NoFile {
                text: text.to_owned(),
            });
        }
        Ok(Self { glob })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        self.glob.as_str()
    }

    fn matches(&self, below_dir: &str) -> bool {
        self.glob.matches(below_dir)
    }

    /// Whether the pattern takes every path under the directory `dir` (below the
    /// book's directory): [`PathGlob::takes_all_under`].
    fn takes_all_under(&self, dir: &str) -> bool {
        self.glob.takes_all_under(dir)
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.glob, f)
    }
}

/// The range of a book, checked: which paths below its directory are documents, and
/// what a file with no front matter is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookScope {
    include: Option<Vec<Pattern>>,
    exclude: Option<Vec<Pattern>>,
    without_front_matter: Option<WithoutFrontMatter>,
}

impl BookScope {
    /// The patterns of `include`, as written; none when not written, and then every
    /// `.md` file is in.
    pub fn include(&self) -> Option<&[Pattern]> {
        self.include.as_deref()
    }

    /// The patterns of `exclude`, as written; none when not written.
    pub fn exclude(&self) -> Option<&[Pattern]> {
        self.exclude.as_deref()
    }

    /// `without_front_matter`, as written; none when not written.
    pub fn without_front_matter_declared(&self) -> Option<WithoutFrontMatter> {
        self.without_front_matter
    }

    /// What a file in range with no front matter is: as written, or the default.
    pub fn without_front_matter(&self) -> WithoutFrontMatter {
        self.without_front_matter.unwrap_or_default()
    }

    fn excluded(&self) -> &[Pattern] {
        self.exclude.as_deref().unwrap_or_default()
    }

    /// Whether no path under the directory `dir` (below the book's directory, `a/b`)
    /// can be in the range, because an `exclude` takes everything under it
    /// (`archive/**`, `**/drafts/**`): what lets a listing of the directory not go
    /// into it at all. An answer of false says nothing; the files are asked one by
    /// one.
    pub fn leaves_out_all_under(&self, dir: &str) -> bool {
        self.excluded().iter().any(|p| p.takes_all_under(dir))
    }

    /// Whether a path below the directory of the book (`archive/a.md`, no leading
    /// `/`) matches some `include` and no `exclude`.
    ///
    /// The one place patterns are matched. That the file is a `.md` file is not
    /// asked here: [`BookRange::takes`] asks it.
    pub fn admits(&self, below_dir: &str) -> bool {
        let included = self
            .include
            .as_ref()
            .is_none_or(|patterns| patterns.iter().any(|p| p.matches(below_dir)));
        included && !self.excluded().iter().any(|p| p.matches(below_dir))
    }
}

impl fmt::Display for BookScope {
    /// The patterns, as a refusal names them: ``include `a`, `b`; exclude `c` ``,
    /// an `include` not written told as every `.md` file.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let quoted = |patterns: &[Pattern]| listed(patterns, Mark::Backtick, Join::Comma);
        match &self.include {
            Some(patterns) => write!(f, "include {}", quoted(patterns))?,
            None => f.write_str("include every `.md` file (not written)")?,
        }
        if !self.excluded().is_empty() {
            write!(f, "; exclude {}", quoted(self.excluded()))?;
        }
        Ok(())
    }
}

/// The directory of a book with its range: what answers "is this path a document of
/// the book", for the listing of files and for `new` and `init` alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRange {
    dir: BookDir,
    scope: BookScope,
}

impl BookRange {
    /// A directory with a range.
    pub fn new(dir: BookDir, scope: BookScope) -> Self {
        Self { dir, scope }
    }

    /// A directory with the default range: every `.md` file under it.
    pub fn of_dir(dir: BookDir) -> Self {
        Self::new(dir, BookScope::default())
    }

    /// The directory.
    pub fn dir(&self) -> &BookDir {
        &self.dir
    }

    /// The range inside the directory.
    pub fn scope(&self) -> &BookScope {
        &self.scope
    }

    /// Whether `path` is a document of the book: a `.md` file (with a name before
    /// the extension) under the directory that the range takes.
    ///
    /// The one answer to "is this path in the book's range".
    pub fn takes(&self, path: &RelPath) -> bool {
        self.dir
            .relative(path)
            .is_some_and(|below| self.takes_below(below))
    }

    /// Whether the file at `below`, a path below the directory with `/` between its
    /// segments, is a document of the book: [`takes`](Self::takes) once the
    /// directory is set aside, for a listing that asks before it has made the path
    /// a [`RelPath`] (so that a name it cannot make one of is not looked at unless
    /// the range takes it).
    pub fn takes_below(&self, below: &str) -> bool {
        let name = below.rsplit('/').next().unwrap_or(below);
        name.len() > ".md".len() && name.ends_with(".md") && self.scope.admits(below)
    }

    /// Whether a listing may leave out the directory `below` (below the book's
    /// directory) without looking in it: the range takes nothing under it
    /// ([`BookScope::leaves_out_all_under`]).
    pub fn leaves_out_dir(&self, below: &str) -> bool {
        self.scope.leaves_out_all_under(below)
    }
}

/// The patterns of one list, each read; the problems of the ones that are none are
/// pushed, with the book and the list.
fn read_patterns(
    book: &str,
    list: ScopeList,
    texts: &[String],
    problems: &mut Vec<super::SchemaProblem>,
) -> Vec<Pattern> {
    let mut patterns = Vec::new();
    for text in texts {
        match Pattern::new(text) {
            Ok(pattern) => patterns.push(pattern),
            Err(why) => problems.push(super::SchemaProblem::BadScopePattern {
                book: book.to_owned(),
                list,
                pattern: text.clone(),
                why,
            }),
        }
    }
    patterns
}

/// The range of a book from its declaration, with every problem of it pushed.
///
/// A range that does not take the files `new` writes (`<dir>/<key>.md`) is no
/// problem of the declaration: a book brought into a directory may hold its
/// documents where they already are. `new` refuses such a key, key by key, before
/// anything is written ([`Shelf::claim_key`], [`Shelf::next_key`]).
///
/// [`Shelf::claim_key`]: crate::domain::shelf::build::Shelf::claim_key
/// [`Shelf::next_key`]: crate::domain::shelf::build::Shelf::next_key
pub(super) fn read_scope(
    book: &str,
    decl: ScopeDecl,
    problems: &mut Vec<super::SchemaProblem>,
) -> Option<BookScope> {
    let first_problem = problems.len();
    let include = decl.include.as_ref().map(|texts| {
        if texts.is_empty() {
            problems.push(super::SchemaProblem::EmptyInclude {
                book: book.to_owned(),
            });
        }
        read_patterns(book, ScopeList::Include, texts, problems)
    });
    let exclude = decl
        .exclude
        .as_ref()
        .map(|texts| read_patterns(book, ScopeList::Exclude, texts, problems));
    let without_front_matter = decl
        .without_front_matter
        .as_deref()
        .and_then(|word| report(read_book_word(word, book), problems));
    if problems.len() != first_problem {
        return None;
    }
    Some(BookScope {
        include,
        exclude,
        without_front_matter,
    })
}
