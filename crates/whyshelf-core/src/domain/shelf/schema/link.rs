//! The links a book declares: how many a document holds ([`Cardinality`]), what they
//! mean for freshness ([`Propagation`], [`ArtifactPropagation`]), a link into a book
//! ([`LinkKind`]) or to artifacts ([`ArtifactLinkKind`]), whether it is expected to
//! cover its target ([`CoverSeverity`], [`CoverDecl`]), and the two in one
//! ([`DeclaredLink`]).

use super::super::artifact::{Comparison, Space};
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
    pub fn read(decl: &CoverDecl) -> Result<Option<Self>, String> {
        match decl {
            CoverDecl::Truth(false) => Ok(None),
            CoverDecl::Truth(true) => Ok(Some(Self::Error)),
            CoverDecl::Word(word) => [Self::Warn, Self::Error]
                .into_iter()
                .find(|s| s.word() == word)
                .map(Some)
                .ok_or_else(|| word.clone()),
        }
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

/// `cover` on a link, as declared: a truth value or a word, read by
/// [`CoverSeverity::read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverDecl {
    /// `true` or `false`.
    Truth(bool),
    /// A word, as written.
    Word(String),
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
