//! The declaration as an adapter reads it, nothing checked: a book ([`BookSchemaDecl`])
//! and the grammar of its keys, its fields and its links as written, the input
//! [`Schema::validate`](super::Schema::validate) takes.

use super::{Cardinality, CoverDecl, FieldPresence, KeyWord, Propagation, ScopeDecl};

/// A book as an adapter read it: text and lists, nothing checked.
///
/// Fields and links come as lists of named entries, not as maps, so that a name
/// written twice can be reported and not silently kept once. The links are
/// [`LinkEntryDecl`]s, as [`Schema::validate`](super::Schema::validate) takes them, once [`expand`](super::expand) has put
/// the values of the presets into them; as written, they are
/// [`WrittenLinkDecl`](super::WrittenLinkDecl)s ([`WrittenBookDecl`](super::WrittenBookDecl)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSchemaDecl<L = LinkEntryDecl> {
    /// The name of the book.
    pub name: String,
    /// The directory of the book, relative to the project root.
    pub dir: String,
    /// Which files under the directory are documents, and what one with no front
    /// matter is, as written.
    pub scope: ScopeDecl,
    /// The grammar of the keys.
    pub key: KeyPatternDecl,
    /// What the book is for, as written (one line), when the declaration says.
    pub description: Option<String>,
    /// The fields.
    pub fields: Vec<FieldEntryDecl>,
    /// The links.
    pub links: Vec<L>,
}

/// The grammar of keys as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPatternDecl {
    /// `<prefix>-<number or name>` with this prefix.
    Prefixed {
        /// The prefix as written.
        prefix: String,
    },
    /// A word, as written: [`Schema::validate`](super::Schema::validate) reads it ([`read_book_word`](super::read_book_word)) and
    /// refuses one that is no grammar ([`SchemaProblem::UnknownBookWord`](super::SchemaProblem::UnknownBookWord)).
    Word(String),
}

impl KeyPatternDecl {
    /// The grammar of a path of lowercase words, written as its word ([`KeyWord`]).
    pub fn slug() -> Self {
        Self::Word(KeyWord::Slug.word().to_owned())
    }
}

/// The type of a field as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldTypeDecl {
    /// A word, as written: [`Schema::validate`](super::Schema::validate) reads it ([`ScalarType::from_word`](super::ScalarType::from_word))
    /// and refuses one that is no type ([`SchemaProblem::UnknownWord`](super::SchemaProblem::UnknownWord)).
    Word(String),
    /// One of these words (an empty list is refused).
    Enum(Vec<String>),
}

/// A field as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEntryDecl {
    /// The name of the field.
    pub name: String,
    /// Its type.
    pub ty: FieldTypeDecl,
    /// Whether it must be there.
    pub presence: FieldPresence,
}

/// What a declared link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTargetDecl {
    /// Documents of the book of this name, as written.
    Book(String),
    /// Artifacts, things outside the documents, by name in the space the link
    /// declares (an adapter reads it from the word [`ARTIFACT_TARGET`](super::ARTIFACT_TARGET)).
    Artifact,
}

/// A link as declared, with every value a preset gives put in ([`expand`](super::expand)): what
/// [`Schema::validate`](super::Schema::validate) checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEntryDecl {
    /// The name of the link.
    pub name: String,
    /// What it points at.
    pub target: LinkTargetDecl,
    /// How many links a document holds.
    pub cardinality: Cardinality,
    /// What the link means for freshness.
    pub propagation: Propagation,
    /// For a link to artifacts, the space its names are in, as written. Every link
    /// to artifacts names one, and only such a link takes it.
    pub space: Option<String>,
    /// For a link to artifacts, what of each artifact is compared, as written: the
    /// kind (`bytes`, `lines` or `presence`) and the prefixes of the lines `lines`
    /// leaves out. `None` is `bytes`.
    pub compare: Option<CompareDecl>,
    /// What the link is expected to reach, as written; `None` when not written. For
    /// a link into a book, how strongly every document of the target book is
    /// expected to be reached by it (a truth value or a word); for a link to
    /// artifacts, a table of the strength and of the selection of the names of its
    /// space that are expected to be reached. One that is not of the kind the link
    /// points at is refused.
    pub cover: Option<CoverDecl>,
}

/// What of an artifact a link to artifacts compares, as declared.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompareDecl {
    /// `bytes`, `lines` or `presence`.
    pub kind: String,
    /// The prefixes of the lines `lines` leaves out.
    pub ignore: Vec<String>,
}
