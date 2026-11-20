//! The fields a book declares: the type of a value ([`ScalarType`], [`FieldType`]),
//! whether a document must carry it ([`FieldPresence`]), and the field itself
//! ([`FieldDecl`]).

use crate::domain::nonempty::NonEmptySet;

use super::word::find_word;
use super::{DeclWord, FieldWord, WordOf, Words};

/// The type of a field that is one word in a declaration: what
/// [`FieldType::Scalar`] holds.
///
/// There is no floating point type: a measurement is an integer or a text, as in a
/// canonical form.
///
/// The words of a declaration for a type, a count ([`Cardinality`](super::Cardinality)) and a meaning
/// ([`Propagation`](super::Propagation)) are decided in `schema/field.rs` and `schema/link.rs` and only there: each of the three lists its
/// values (`ALL`), says the word of each (`word`) and reads a word back
/// (`from_word`, which searches `ALL` by `word`). What reads a word out of a
/// declaration is [`read_word`](super::read_word); what an answer or a problem says of a type, a count
/// or a meaning calls `word` and builds no word of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    /// Free text, of any number of lines.
    Text,
    /// A text of one line: no line break, and not blank.
    Line,
    /// A whole number.
    Integer,
    /// A truth value.
    Bool,
    /// A list of texts.
    TextList,
}

impl ScalarType {
    /// Every scalar type, in the order a problem lists their words.
    pub const ALL: [Self; 5] = [
        Self::Text,
        Self::Line,
        Self::Integer,
        Self::Bool,
        Self::TextList,
    ];

    /// The word of the type in a declaration and in the answers: `text`, `line`,
    /// `integer`, `bool` or `list`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Line => "line",
            Self::Integer => "integer",
            Self::Bool => "bool",
            Self::TextList => "list",
        }
    }

    /// The type a word names, or none.
    pub fn from_word(word: &str) -> Option<Self> {
        find_word(word)
    }
}

/// The type of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    /// A type that is one word in a declaration.
    Scalar(ScalarType),
    /// One of the words listed.
    Enum(NonEmptySet<FieldWord>),
}

impl FieldType {
    /// The word of the type in an answer: that of its [`ScalarType`], or `enum`.
    pub fn word(&self) -> &'static str {
        match self {
            Self::Scalar(scalar) => scalar.word(),
            Self::Enum(_) => "enum",
        }
    }
}

/// Whether a document must carry a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPresence {
    /// A document without the field, or whose value says nothing, is invalid. A
    /// value says nothing when it is no value (`key:`, `null`), a text that is empty
    /// or white space only (of any type), or a list with no item; an enum, integer
    /// or bool written says something. The required text of a valid document is
    /// therefore never blank.
    Required,
    /// The field may be left out, and a value that says nothing (as told at
    /// [`FieldPresence::Required`]) is read as the field left out.
    Optional,
}

/// A field a book declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    /// The type of the value.
    pub ty: FieldType,
    /// Whether the field must be there.
    pub presence: FieldPresence,
}

impl Words for ScalarType {
    fn all() -> &'static [Self] {
        &ScalarType::ALL
    }

    fn word(self) -> &'static str {
        ScalarType::word(self)
    }
}

impl DeclWord for ScalarType {
    const OF: WordOf = WordOf::Type;
}
