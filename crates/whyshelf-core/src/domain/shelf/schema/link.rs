//! The links a book declares: how many a document holds ([`Cardinality`]), what they
//! mean for freshness ([`Propagation`], [`ArtifactPropagation`]), a link into a book
//! ([`LinkKind`]) or to artifacts ([`ArtifactLinkKind`]), whether it is expected to
//! cover its target ([`CoverSeverity`], [`CoverDecl`]), and the two in one
//! ([`DeclaredLink`]).

use super::super::artifact::{Comparison, Selection, SelectionProblem, Space};
use super::word::find_word;
use super::{BookName, DeclWord, KeyPattern, WordOf, Words};

/// How many links of a kind a document holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// Exactly one.
    One,
    /// None or one.
    Optional,
    /// Any number.
    Many,
    /// One or more.
    AtLeastOne,
}

impl Cardinality {
    /// The count of a link that writes none: any number.
    pub const WHEN_UNWRITTEN: Self = Self::Many;

    /// Every count, in the order a problem lists their words.
    pub const ALL: [Self; 4] = [Self::One, Self::Optional, Self::Many, Self::AtLeastOne];

    /// Whether a document may write `count` links of this kind.
    pub(in crate::domain::shelf) fn admits(self, count: usize) -> bool {
        match self {
            Self::One => count == 1,
            Self::Optional => count <= 1,
            Self::Many => true,
            Self::AtLeastOne => count >= 1,
        }
    }

    /// How many links this count admits, in words, as a problem says it: `exactly
    /// one`, `none or one`, `any number` or `one or more`. It is what `admits` decides,
    /// said aloud.
    pub fn admits_in_words(self) -> &'static str {
        match self {
            Self::One => "exactly one",
            Self::Optional => "none or one",
            Self::Many => "any number",
            Self::AtLeastOne => "one or more",
        }
    }

    /// The word of the count in a declaration and in the answers: `one`, `optional`,
    /// `many` or `at-least-one`.
    pub fn word(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::Optional => "optional",
            Self::Many => "many",
            Self::AtLeastOne => "at-least-one",
        }
    }

    /// The count a word names, or none.
    pub fn from_word(word: &str) -> Option<Self> {
        find_word(word)
    }
}

/// What a link means for the freshness of the document that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Propagation {
    /// The document stands on the target: an approval pins the target, and a change
    /// of the target makes the approval stale.
    Depends,
    /// The document points at the target for information: a target that is not fresh
    /// is a notice, never a staleness.
    Informs,
    /// The document takes the place of the target: it is a successor candidate of a
    /// deprecated target, and affects no freshness.
    Replaces,
}

impl Propagation {
    /// Every meaning, in the order a problem lists their words.
    pub const ALL: [Self; 3] = [Self::Depends, Self::Informs, Self::Replaces];

    /// The word of the meaning in a declaration and in the answers: `depends`,
    /// `informs` or `replaces`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Depends => "depends",
            Self::Informs => "informs",
            Self::Replaces => "replaces",
        }
    }

    /// The meaning a word names, or none.
    pub fn from_word(word: &str) -> Option<Self> {
        find_word(word)
    }
}

impl Words for Cardinality {
    fn all() -> &'static [Self] {
        &Cardinality::ALL
    }

    fn word(self) -> &'static str {
        Cardinality::word(self)
    }
}

impl DeclWord for Cardinality {
    const OF: WordOf = WordOf::Count;
}

impl Words for Propagation {
    fn all() -> &'static [Self] {
        &Propagation::ALL
    }

    fn word(self) -> &'static str {
        Propagation::word(self)
    }
}

impl DeclWord for Propagation {
    const OF: WordOf = WordOf::Means;
}

/// What a link to an artifact (a thing outside the documents) means for freshness:
/// one of the two meanings of [`Propagation`] an artifact can have.
///
/// There is no `Replaces`: an artifact takes no document's place, so a declaration
/// that says so is refused ([`SchemaProblem::ArtifactReplaces`](super::SchemaProblem::ArtifactReplaces)) and the type cannot
/// hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactPropagation {
    /// The document stands on the artifact: an approval pins what is there (rule
    /// A2), and a change makes the approval stale (A3).
    Depends,
    /// The document points at the artifact for information; it changes no freshness.
    Informs,
}

impl ArtifactPropagation {
    /// The meaning as a [`Propagation`], for an answer that lists every link.
    pub fn propagation(self) -> Propagation {
        match self {
            Self::Depends => Propagation::Depends,
            Self::Informs => Propagation::Informs,
        }
    }
}

/// A kind of link a book declares to artifacts: things outside the documents, named
/// in one [`Space`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactLinkKind {
    /// The space its names are in (`space` in the declaration, which every link to
    /// artifacts writes: there is no space a link has when it names none). One of
    /// the spaces [`Schema::validate`](super::Schema::validate) was given.
    pub space: Space,
    /// How many names a document writes.
    pub cardinality: Cardinality,
    /// What the link means for freshness.
    pub propagation: ArtifactPropagation,
    /// What of each artifact is compared (`compare` in the declaration; `bytes` when
    /// it says none).
    pub comparison: Comparison,
    /// How strongly, and which, of the names in its space are expected to be reached
    /// by this link from a pending or approved document; `None` when none is
    /// expected. One that is not reached is a gap of the cover assessment, with the
    /// strength.
    pub cover: Option<ArtifactCover>,
}

/// What a link to artifacts expects to reach: the names of its space a [`Selection`]
/// takes, with how strongly ([`CoverSeverity`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCover {
    severity: CoverSeverity,
    selection: Selection,
}

impl ArtifactCover {
    /// The names `selection` takes, expected with this strength.
    pub fn new(severity: CoverSeverity, selection: Selection) -> Self {
        Self {
            severity,
            selection,
        }
    }

    /// How strongly the names are expected to be reached.
    pub fn severity(&self) -> CoverSeverity {
        self.severity
    }

    /// Which names of the space are expected to be reached: opaque to the domain,
    /// read by the adapter of the space.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
}

/// The name no book can have: the word a declaration uses for the target of a link to
/// artifacts (`target = "artifact"`), so that the word never names a book as well.
pub const ARTIFACT_TARGET: &str = "artifact";

/// A kind of link a book declares to another book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkKind {
    /// The book the link points into.
    pub target: BookName,
    /// The grammar of the keys of that book, put here by [`Schema::validate`](super::Schema::validate) (which
    /// has refused a link into a book it does not declare), so that a key written in
    /// the link is read without asking the schema for the book again.
    pub(in crate::domain::shelf) target_key: KeyPattern,
    /// How many links a document holds.
    pub cardinality: Cardinality,
    /// What the link means for freshness.
    pub propagation: Propagation,
    /// How strongly every pending or approved document of the target book is
    /// expected to be reached by this link from a pending or approved document;
    /// `None` when it is not expected ([`CoverSeverity::read`] says how `cover` is
    /// read). One that is not reached is a
    /// [`Notice::NotCovered`](super::super::freshness::Notice::NotCovered) on it, with the
    /// strength, and is listed by a check of the books; its freshness is unchanged. A
    /// link into its own book is met only by another document of that book, so the
    /// first of a chain of `depends` links within one book can never be reached.
    pub cover: Option<CoverSeverity>,
}

/// How strongly a covering link expects to reach every document of its target
/// book. What each strength does to a check of the books is stated on
/// [`BooksCheck::is_clean`](crate::app::shelf::BooksCheck::is_clean).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverSeverity {
    /// The weaker strength.
    Warn,
    /// The stronger strength.
    Error,
}

impl CoverSeverity {
    /// The strength `cover` declares, or none.
    ///
    /// | `cover` | strength |
    /// |---|---|
    /// | not written, or `false` | none |
    /// | `true` or `"error"` | [`Error`](Self::Error) |
    /// | `"warn"` | [`Warn`](Self::Warn) |
    /// | any other word | refused: the word, as written |
    /// | a table of a selection | refused here: only a link to artifacts takes one, and the reader of a link's `cover` reads it |
    pub fn read(decl: &CoverDecl) -> Result<Option<Self>, String> {
        match decl {
            CoverDecl::Truth(false) => Ok(None),
            CoverDecl::Truth(true) => Ok(Some(Self::Error)),
            CoverDecl::Word(word) => Self::of_word(word).map(Some).ok_or_else(|| word.clone()),
            CoverDecl::Table { .. } => Err("a table".to_owned()),
        }
    }

    /// The strength a word names: `warn` or `error`.
    fn of_word(word: &str) -> Option<Self> {
        [Self::Warn, Self::Error]
            .into_iter()
            .find(|s| s.word() == word)
    }

    /// The word of the strength in a declaration and in the answers: `warn` or
    /// `error`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// `cover` on a link, as declared: a truth value, a word or a table, read by
/// [`CoverSeverity::read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverDecl {
    /// `true` or `false`.
    Truth(bool),
    /// A word, as written.
    Word(String),
    /// A table of a strength and a selection, for a link to artifacts: `{ strength,
    /// include, exclude }`, as written.
    Table {
        /// The strength, as a word, when written (a strength not written is `error`).
        strength: Option<String>,
        /// The texts to include, as written.
        include: Vec<String>,
        /// The texts to leave out, as written.
        exclude: Vec<String>,
    },
}

/// What a link points at, as the reader of its `cover` needs to know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CoverTo {
    /// A book: `cover` is a strength.
    Book,
    /// Artifacts: `cover` is a table of a selection, with a strength when it is not
    /// `error`.
    Artifacts,
}

/// What the `cover` of a link comes to once read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReadCover {
    /// Not written, or `false`: nothing is expected.
    Nothing,
    /// A strength: every document of the book the link points into is expected.
    Book(CoverSeverity),
    /// A strength and a selection: the names the selection takes are expected.
    Artifacts(ArtifactCover),
}

/// Why a `cover` is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CoverRefusal {
    /// A word that is no strength, as written.
    Word(String),
    /// A table of a selection on a link into a book.
    SelectionOnBookLink,
    /// `true` or a strength alone on a link to artifacts, which takes a selection.
    NeedsSelection,
    /// A selection that cannot be made.
    Selection(SelectionProblem),
}

/// The `cover` of a link as declared, read for what the link points at.
///
/// The one reading of it: [`Schema::validate`](super::Schema::validate) reads every
/// link's `cover` with it, and so does [`Schema::selection_asks`](super::Schema::selection_asks),
/// which asks the adapters of the spaces about the selections it finds. A selection is
/// kept as written ([`Selection`] is opaque here); whether a space reads it is the
/// adapter's to say, not this reader's.
///
/// | `to` | `cover` | read as |
/// |---|---|---|
/// | any | not written, or `false` | [`ReadCover::Nothing`] |
/// | a book | `true`, `"error"` or `"warn"` | [`ReadCover::Book`] ([`CoverSeverity::read`]) |
/// | a book | a table | refused ([`CoverRefusal::SelectionOnBookLink`]) |
/// | artifacts | a table (`strength` `error` when not written, `include`, `exclude`) | [`ReadCover::Artifacts`] |
/// | artifacts | `true`, `"error"` or `"warn"` | refused ([`CoverRefusal::NeedsSelection`]) |
/// | any | a word that is no strength, as the strength of a table or alone | refused ([`CoverRefusal::Word`]) |
/// | artifacts | a table whose selection cannot be made | refused ([`CoverRefusal::Selection`]) |
pub(super) fn read_cover(decl: Option<&CoverDecl>, to: CoverTo) -> Result<ReadCover, CoverRefusal> {
    let Some(decl) = decl else {
        return Ok(ReadCover::Nothing);
    };
    match (to, decl) {
        (_, CoverDecl::Truth(false)) => Ok(ReadCover::Nothing),
        (CoverTo::Book, CoverDecl::Table { .. }) => Err(CoverRefusal::SelectionOnBookLink),
        (CoverTo::Book, decl) => Ok(CoverSeverity::read(decl)
            .map_err(CoverRefusal::Word)?
            .map_or(ReadCover::Nothing, ReadCover::Book)),
        (CoverTo::Artifacts, CoverDecl::Truth(true)) => Err(CoverRefusal::NeedsSelection),
        (CoverTo::Artifacts, CoverDecl::Word(word)) => match CoverSeverity::of_word(word) {
            Some(_) => Err(CoverRefusal::NeedsSelection),
            None => Err(CoverRefusal::Word(word.clone())),
        },
        (
            CoverTo::Artifacts,
            CoverDecl::Table {
                strength,
                include,
                exclude,
            },
        ) => {
            let severity = match strength {
                None => CoverSeverity::Error,
                Some(word) => {
                    CoverSeverity::of_word(word).ok_or_else(|| CoverRefusal::Word(word.clone()))?
                }
            };
            let selection = Selection::new(include.clone(), exclude.clone())
                .map_err(CoverRefusal::Selection)?;
            Ok(ReadCover::Artifacts(ArtifactCover::new(
                severity, selection,
            )))
        }
    }
}

/// A link of a book as declared, into a book or to artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredLink {
    /// A link into a book.
    Book(LinkKind),
    /// A link to artifacts.
    Artifact(ArtifactLinkKind),
}

impl DeclaredLink {
    pub(super) fn book(&self) -> Option<&LinkKind> {
        match self {
            Self::Book(kind) => Some(kind),
            Self::Artifact(_) => None,
        }
    }

    pub(super) fn artifact(&self) -> Option<&ArtifactLinkKind> {
        match self {
            Self::Artifact(kind) => Some(kind),
            Self::Book(_) => None,
        }
    }
}
