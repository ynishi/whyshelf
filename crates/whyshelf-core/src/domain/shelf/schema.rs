//! What a book is: the declaration of its documents, checked once.
//!
//! A project declares its kinds of document as *books* (a directory, a grammar of
//! keys, the fields a front matter may hold and the links between books). The
//! declaration arrives as a [`BookSchemaDecl`] per book, built by an adapter from
//! whatever file it reads, and [`Schema::validate`] is the one place that decides
//! whether the whole of it is sound. It returns every problem at once, so a person
//! fixes a declaration in one pass, and it returns a [`Schema`] only when there is
//! none: nothing downstream re-checks a name, a prefix or a link target. A
//! declaration as a project writes it may name presets of links to artifacts and a
//! default ([`ShelfDecl`]); [`expand`] puts their values in first, and
//! [`Schema::declared`] does both.
//!
//! The key grammar of a book ([`KeyPattern`]) is also decided here and only here:
//! [`KeyPattern::parse`] turns the text of a key into a [`KeyForm`], and both the
//! shelf that reads a document and the lookup of what a person typed call it.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::id::ShortId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{Count, NonEmptySet, NonEmptyVec, TwoOrMore};

use super::artifact::{Comparison, ComparisonProblem, Space, SpaceProblem};

/// The most characters of the name of a book, a field, a link or an enum word.
const NAME_MAX: usize = 64;
/// The most characters of a name in the tail of a key (`FR-LATENCY-BUDGET`): its own
/// limit, which happens to be the same as the one of a name of a field.
const KEY_NAME_MAX: usize = 64;
/// The most characters of the prefix of a book's keys.
const PREFIX_MAX: usize = 16;
const NUMBER_MAX_DIGITS: usize = 19;
const SLUG_MAX: usize = 128;

/// The entry of a front matter that holds the identity of a document.
pub(super) const ID_ENTRY: &str = "id";
/// The entry of a front matter that holds the key of a document.
pub(super) const KEY_ENTRY: &str = "key";

/// The names the front matter keeps for itself: a field or a link cannot be called
/// one of them.
const RESERVED: [&str; 2] = [ID_ENTRY, KEY_ENTRY];

/// Why a text is not a name of a book, a field, a link or an enum word.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameProblem {
    /// The name is empty.
    #[error("a name is empty")]
    Empty,
    /// The name is longer than the limit.
    #[error("a name has at most {max} characters, found {len}")]
    TooLong {
        /// Characters found.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The name holds a character outside `a-z 0-9 - _`.
    #[error("a name holds lowercase letters, digits, `-` and `_`, found `{ch}`")]
    BadChar {
        /// The offending character.
        ch: char,
    },
    /// The name is one the front matter keeps for itself (`id`, `key`).
    #[error("`{name}` is kept by the front matter and cannot name a field or a link")]
    Reserved {
        /// The reserved name.
        name: String,
    },
}

fn check_name(text: &str) -> Result<(), NameProblem> {
    if text.is_empty() {
        return Err(NameProblem::Empty);
    }
    let len = text.chars().count();
    if len > NAME_MAX {
        return Err(NameProblem::TooLong { len, max: NAME_MAX });
    }
    match text
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '_'))
    {
        Some(ch) => Err(NameProblem::BadChar { ch }),
        None => Ok(()),
    }
}

macro_rules! name_type {
    ($(#[$meta:meta])* $name:ident, reserved = $reserved:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// A name from a text, refused when it breaks the grammar of names.
            pub fn new(text: &str) -> Result<Self, NameProblem> {
                check_name(text)?;
                if $reserved && RESERVED.contains(&text) {
                    return Err(NameProblem::Reserved {
                        name: text.to_owned(),
                    });
                }
                Ok(Self(text.to_owned()))
            }

            /// The name as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
    };
}

name_type!(
    /// The name of a book: 1 to 64 characters of `a-z 0-9 - _`.
    ///
    /// Why a type: it is written in a key reference (`<book>:<key>`), in a record of
    /// the ledger and in a hash, so a name that breaks the grammar must not exist.
    BookName,
    reserved = false
);
name_type!(
    /// The name of a field or of a link: the grammar of [`BookName`], and neither
    /// `id` nor `key`, which the front matter keeps for itself.
    ///
    /// Fields and links share one name space inside a book, because a front matter
    /// is one flat mapping: [`Schema::validate`] refuses a name that is both.
    EntryName,
    reserved = true
);
name_type!(
    /// A word of an enum field: the grammar of [`BookName`].
    FieldWord,
    reserved = false
);
name_type!(
    /// The name of a preset of links to artifacts ([`expand`]): the grammar of
    /// [`BookName`].
    PresetName,
    reserved = false
);

/// The directory of a book, relative to the project root.
///
/// A file belongs to a book when it is under the directory of that book and the
/// book's range takes it ([`BookRange::takes`], see `Schema::membership`). Two books
/// cannot share a directory or nest one in the other, whatever their ranges, so a
/// file belongs to at most one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BookDir(RelPath);

impl BookDir {
    /// A directory from text, refused when it is not a relative path.
    pub fn new(text: &str) -> Result<Self, ValueError> {
        text.parse::<RelPath>().map(Self)
    }

    /// The directory as a path.
    pub fn as_rel(&self) -> &RelPath {
        &self.0
    }

    /// Whether `path` is a file under this directory.
    pub fn contains(&self, path: &RelPath) -> bool {
        self.relative(path).is_some()
    }

    /// The path below this directory (`a/b.md` for `dir/a/b.md`), or none when
    /// `path` is not under it.
    pub fn relative<'a>(&self, path: &'a RelPath) -> Option<&'a str> {
        path.as_str()
            .strip_prefix(self.0.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
    }

    /// Whether the two directories are the same or one is under the other.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.0 == other.0
            || Self::is_below(self.0.as_str(), other.0.as_str())
            || Self::is_below(other.0.as_str(), self.0.as_str())
    }

    fn is_below(dir: &str, path: &str) -> bool {
        path.strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl fmt::Display for BookDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// What a book is for, in one line of text (`description` in the declaration).
///
/// Why a type: it is shown in a drawing of the books, one line per book, so a
/// description that is blank, breaks the line or moves the cursor must not exist.
///
/// A text is refused when:
///
/// - it is blank: every character is white space or one of the characters that
///   show nothing (U+200B, U+200C, U+200D, U+2060, U+FEFF), the empty text
///   included;
/// - it breaks the line: `\n`, `\r`, U+0085, U+2028 or U+2029;
/// - it holds any other control character (`char::is_control`: a tab, ESC, U+000B,
///   U+000C, ...) or a character that sets the direction of the text (U+202A to
///   U+202E, U+2066 to U+2069, U+200E, U+200F, U+061C).
///
/// Anything else is kept as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookDescription(String);

/// The characters that show nothing and are no white space: a description of
/// these alone is blank.
const INVISIBLE: [char; 5] = ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];

/// Whether a character sets the direction of the text around it (U+202A to
/// U+202E, U+2066 to U+2069, U+200E, U+200F, U+061C): it can reorder a line of a
/// terminal, so a description is refused with it as with a control character.
fn is_bidi_format(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}

impl BookDescription {
    /// A description from a text, kept as written, or why it is refused (above).
    pub fn new(text: &str) -> Result<Self, DescriptionProblem> {
        if text
            .chars()
            .all(|c| c.is_whitespace() || INVISIBLE.contains(&c))
        {
            return Err(DescriptionProblem::Blank);
        }
        if text
            .chars()
            .any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
        {
            return Err(DescriptionProblem::MoreThanOneLine);
        }
        if let Some(ch) = text.chars().find(|c| c.is_control() || is_bidi_format(*c)) {
            return Err(DescriptionProblem::ControlCharacter { ch });
        }
        Ok(Self(text.to_owned()))
    }

    /// The description as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BookDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a text is not a [`BookDescription`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DescriptionProblem {
    /// The text is empty, or only white space and characters that show nothing.
    #[error("is blank")]
    Blank,
    /// The text holds a line break.
    #[error("is more than one line")]
    MoreThanOneLine,
    /// The text holds a control character that breaks no line, or a character
    /// that sets the direction of the text.
    #[error("holds the control character {ch:?}")]
    ControlCharacter {
        /// The character.
        ch: char,
    },
}

/// Why a text is not a key of a book.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyProblem {
    /// The key is empty.
    #[error("a key is empty")]
    Empty,
    /// The key has no `-` between a prefix and a tail.
    #[error("a key of this book is `<prefix>-<tail>`, with one `-` after the prefix")]
    NoSeparator,
    /// The part before the first `-` is not capital letters and digits starting with
    /// a letter, 1 to 16 of them.
    #[error(
        "`{text}` is not a key prefix: 1 to {PREFIX_MAX} capital letters and digits, starting with a letter"
    )]
    BadPrefix {
        /// The prefix as written.
        text: String,
    },
    /// The prefix is well formed but is not the one of this book.
    #[error("this book's keys start with `{expected}-`, not `{found}-`")]
    WrongPrefix {
        /// The prefix of the book.
        expected: KeyPrefix,
        /// The prefix as written.
        found: String,
    },
    /// A number tail starts with `0`: `FR-012` is written `FR-12`, so a key has one
    /// spelling.
    #[error("a number in a key has no leading zero")]
    NumberLeadingZero,
    /// A number tail has more digits than a `u64` is sure to hold.
    #[error("a number in a key has at most {max} digits, found {len}")]
    NumberTooLong {
        /// Digits found.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// A tail that is neither a number nor a name (capital letter first, then capital
    /// letters, digits, `-` and `_`, up to 64).
    #[error(
        "`{text}` is neither a number nor a name: capital letter first, then capital letters, digits, `-` and `_`, up to {KEY_NAME_MAX}"
    )]
    BadTail {
        /// The tail as written.
        text: String,
    },
    /// A slug longer than the limit.
    #[error("a slug has at most {max} characters, found {len}")]
    SlugTooLong {
        /// Characters found.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// A slug with a character outside `a-z 0-9 . - _ /`.
    #[error("a slug holds lowercase letters, digits and `. - _ /`, found `{ch}`")]
    SlugBadChar {
        /// The offending character.
        ch: char,
    },
    /// A slug that starts or ends with `/`, or holds an empty segment.
    #[error("a slug has no empty path segment")]
    SlugEmptySegment,
    /// A slug of 8 to 32 hexadecimal characters, which is how a short id is written.
    #[error("a slug is not 8 to 32 hexadecimal characters, which is how a short id is written")]
    SlugLooksLikeShortId,
    /// A slug in the form of a UUID.
    #[error("a slug is not written as a UUID")]
    SlugLooksLikeUuid,
}

fn is_upper_or_digit(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

/// The prefix of a book's keys: capital letters and digits, starting with a letter,
/// 1 to 16 of them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyPrefix(String);

impl KeyPrefix {
    pub(super) fn new(text: &str) -> Result<Self, KeyProblem> {
        let mut chars = text.chars();
        let shaped = chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(is_upper_or_digit)
            && text.len() <= PREFIX_MAX;
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(KeyProblem::BadPrefix {
                text: text.to_owned(),
            })
        }
    }

    /// The prefix as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A name in the tail of a key: a capital letter first, then capital letters, digits,
/// `-` and `_`, 1 to 64 of them (`FR-LATENCY-BUDGET`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyName(String);

impl KeyName {
    pub(super) fn new(text: &str) -> Result<Self, KeyProblem> {
        let mut chars = text.chars();
        let shaped = chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| is_upper_or_digit(c) || c == '-' || c == '_')
            && text.len() <= KEY_NAME_MAX;
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(KeyProblem::BadTail {
                text: text.to_owned(),
            })
        }
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A key written as a path of lowercase words, such as `cache/timeout`.
///
/// The rules are those of an alias of the older model, and one more: a text in the
/// form of a UUID is refused too, so that a slug is never mistaken for an identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slug(String);

impl Slug {
    pub(super) fn new(text: &str) -> Result<Self, KeyProblem> {
        if text.is_empty() {
            return Err(KeyProblem::Empty);
        }
        if ShortId::looks_like(text) {
            return Err(KeyProblem::SlugLooksLikeShortId);
        }
        if looks_like_uuid(text) {
            return Err(KeyProblem::SlugLooksLikeUuid);
        }
        let len = text.chars().count();
        if len > SLUG_MAX {
            return Err(KeyProblem::SlugTooLong { len, max: SLUG_MAX });
        }
        if let Some(ch) = text
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_/".contains(*c)))
        {
            return Err(KeyProblem::SlugBadChar { ch });
        }
        if text.split('/').any(str::is_empty) {
            return Err(KeyProblem::SlugEmptySegment);
        }
        Ok(Self(text.to_owned()))
    }

    /// The slug as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a text is 8-4-4-4-12 hexadecimal digits: the hyphenated form of a UUID.
/// (The form without hyphens is 32 hexadecimal characters, a short id's shape.)
fn looks_like_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8usize, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The tail of a prefixed key: a number or a name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyTail {
    /// A positive number, written without a leading zero.
    Number(u64),
    /// A name of capital letters, digits, `-` and `_`.
    Name(KeyName),
}

/// A key after its grammar is checked: what [`KeyPattern::parse`] returns.
///
/// It lives here, and not with the document, because the pattern that makes it is
/// part of the declaration of a book. The parts are checked values, so a form can be
/// put together from them where a record is read back.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyForm {
    /// `<prefix>-<tail>`, such as `FR-12` or `FR-LATENCY`.
    Prefixed {
        /// The prefix of the book.
        prefix: KeyPrefix,
        /// What follows the first `-`.
        tail: KeyTail,
    },
    /// A path of lowercase words.
    Slug(Slug),
}

impl fmt::Display for KeyForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prefixed { prefix, tail } => match tail {
                KeyTail::Number(n) => write!(f, "{prefix}-{n}"),
                KeyTail::Name(name) => write!(f, "{prefix}-{name}"),
            },
            Self::Slug(slug) => fmt::Display::fmt(slug, f),
        }
    }
}

/// The grammar of the keys of one book.
///
/// It closes to two shapes, so that what a person types can be told from a short id
/// and from an identifier: the separator of a prefixed key is a `-`, which a
/// hexadecimal text has none of, and a slug is refused when it looks like either.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyPattern {
    /// `<prefix>-<number or name>`: `FR-12`, `FR-LATENCY`.
    Prefixed(KeyPrefix),
    /// A path of lowercase words: `cache/timeout`.
    Slug,
}

impl KeyPattern {
    /// The key a text spells, in this book's grammar.
    ///
    /// The one entry of the key grammar: the shelf reads the `key:` of a document and
    /// the link of one document to another through it, and the lookup of what a
    /// person types does too.
    pub fn parse(&self, text: &str) -> Result<KeyForm, KeyProblem> {
        if text.is_empty() {
            return Err(KeyProblem::Empty);
        }
        match self {
            Self::Slug => Slug::new(text).map(KeyForm::Slug),
            Self::Prefixed(expected) => {
                let (head, tail) = text.split_once('-').ok_or(KeyProblem::NoSeparator)?;
                let prefix = KeyPrefix::new(head)?;
                if prefix != *expected {
                    return Err(KeyProblem::WrongPrefix {
                        expected: expected.clone(),
                        found: head.to_owned(),
                    });
                }
                let tail = parse_tail(tail)?;
                Ok(KeyForm::Prefixed { prefix, tail })
            }
        }
    }
}

/// The tail of a prefixed key: the one reader of its grammar, shared by
/// [`KeyPattern::parse`] and the reader of a record.
pub(super) fn parse_tail(text: &str) -> Result<KeyTail, KeyProblem> {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        if text.starts_with('0') {
            return Err(KeyProblem::NumberLeadingZero);
        }
        if text.len() > NUMBER_MAX_DIGITS {
            return Err(KeyProblem::NumberTooLong {
                len: text.len(),
                max: NUMBER_MAX_DIGITS,
            });
        }
        return text
            .parse::<u64>()
            .map(KeyTail::Number)
            .map_err(|_| KeyProblem::BadTail {
                text: text.to_owned(),
            });
    }
    KeyName::new(text).map(KeyTail::Name)
}

/// The type of a field.
///
/// There is no floating point type: a measurement is an integer or a text, as in a
/// canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    /// Free text.
    Text,
    /// A whole number.
    Integer,
    /// A truth value.
    Bool,
    /// A list of texts.
    TextList,
    /// One of the words listed.
    Enum(NonEmptySet<FieldWord>),
}

/// Whether a document must carry a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPresence {
    /// A document without the field is invalid.
    Required,
    /// The field may be left out.
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
    /// Whether a document may write `count` links of this kind.
    pub(super) fn admits(self, count: usize) -> bool {
        match self {
            Self::One => count == 1,
            Self::Optional => count <= 1,
            Self::Many => true,
            Self::AtLeastOne => count >= 1,
        }
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

/// What a link to an artifact (a thing outside the documents) means for freshness:
/// one of the two meanings of [`Propagation`] an artifact can have.
///
/// There is no `Replaces`: an artifact takes no document's place, so a declaration
/// that says so is refused ([`SchemaProblem::ArtifactReplaces`]) and the type cannot
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
    /// the spaces [`Schema::validate`] was given.
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
    /// The grammar of the keys of that book, put here by [`Schema::validate`] (which
    /// has refused a link into a book it does not declare), so that a key written in
    /// the link is read without asking the schema for the book again.
    pub(super) target_key: KeyPattern,
    /// How many links a document holds.
    pub cardinality: Cardinality,
    /// What the link means for freshness.
    pub propagation: Propagation,
    /// How strongly every pending or approved document of the target book is
    /// expected to be reached by this link from a pending or approved document;
    /// `None` when it is not expected ([`CoverSeverity::read`] says how `cover` is
    /// read). One that is not reached is a
    /// [`Notice::NotCovered`](super::freshness::Notice::NotCovered) on it, with the
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

/// The declaration of one book, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSchema {
    name: BookName,
    range: BookRange,
    key: KeyPattern,
    description: Option<BookDescription>,
    fields: Declared<EntryName, FieldDecl>,
    /// Every link, into books and to artifacts, in one list: a name is one link.
    links: Declared<EntryName, DeclaredLink>,
    /// The preset each link to artifacts was expanded from, for the links that were.
    presets: BTreeMap<EntryName, PresetOrigin>,
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
    fn book(&self) -> Option<&LinkKind> {
        match self {
            Self::Book(kind) => Some(kind),
            Self::Artifact(_) => None,
        }
    }

    fn artifact(&self) -> Option<&ArtifactLinkKind> {
        match self {
            Self::Artifact(kind) => Some(kind),
            Self::Book(_) => None,
        }
    }
}

impl BookSchema {
    /// The name of the book.
    pub fn name(&self) -> &BookName {
        &self.name
    }

    /// What the book is for, when the declaration says.
    pub fn description(&self) -> Option<&BookDescription> {
        self.description.as_ref()
    }

    /// Every link of the book, into books and to artifacts, in the order of the
    /// declaration.
    pub fn all_links(&self) -> &Declared<EntryName, DeclaredLink> {
        &self.links
    }

    /// The directory of the book.
    pub fn dir(&self) -> &BookDir {
        self.range.dir()
    }

    /// The range of the book inside its directory.
    pub fn scope(&self) -> &BookScope {
        self.range.scope()
    }

    /// The directory with its range: what a listing of files and `new` ask whether
    /// a path is a document of the book.
    pub fn range(&self) -> &BookRange {
        &self.range
    }

    /// Whether `path` is a document of the book ([`BookRange::takes`]).
    pub fn takes(&self, path: &RelPath) -> bool {
        self.range.takes(path)
    }

    /// The grammar of the keys of the book.
    pub fn key(&self) -> &KeyPattern {
        &self.key
    }

    /// The fields of the book, by name, in the order of the declaration.
    pub fn fields(&self) -> &Declared<EntryName, FieldDecl> {
        &self.fields
    }

    /// The links of the book into books, by name, in the order of the declaration.
    pub fn links(&self) -> Picked<'_, EntryName, DeclaredLink, LinkKind> {
        Picked::new(&self.links, DeclaredLink::book)
    }

    /// The links of the book to artifacts, by name, in the order of the
    /// declaration. A name is a link into a book or a link to artifacts, never both.
    pub fn artifact_links(&self) -> Picked<'_, EntryName, DeclaredLink, ArtifactLinkKind> {
        Picked::new(&self.links, DeclaredLink::artifact)
    }

    /// The preset a link to artifacts was expanded from ([`expand`]), or none when
    /// it names none and no default applied: what `map` tells beside the values,
    /// which are in [`artifact_links`](Self::artifact_links) whatever their origin.
    pub fn preset_of<Q>(&self, link: &Q) -> Option<&PresetOrigin>
    where
        EntryName: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.presets.get(link)
    }
}

/// A book as an adapter read it: text and lists, nothing checked.
///
/// Fields and links come as lists of named entries, not as maps, so that a name
/// written twice can be reported and not silently kept once. The links are
/// [`LinkEntryDecl`]s, as [`Schema::validate`] takes them, once [`expand`] has put
/// the values of the presets into them; as written, they are
/// [`WrittenLinkDecl`]s ([`WrittenBookDecl`]).
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
    /// A path of lowercase words.
    Slug,
}

/// The type of a field as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldTypeDecl {
    /// Free text.
    Text,
    /// A whole number.
    Integer,
    /// A truth value.
    Bool,
    /// A list of texts.
    TextList,
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
    /// declares (an adapter reads it from the word [`ARTIFACT_TARGET`]).
    Artifact,
}

/// A link as declared, with every value a preset gives put in ([`expand`]): what
/// [`Schema::validate`] checks.
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
    /// For a link into a book: how strongly every document of the target book is
    /// expected to be reached by it, as written; `None` when not written. Refused on
    /// a link to artifacts, unless it declares no expectation.
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

/// Where a name was written, for a problem to say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePlace {
    /// The name of a book.
    Book,
    /// The name of a field of a book.
    Field {
        /// The book, as written.
        book: String,
    },
    /// The name of a link of a book.
    Link {
        /// The book, as written.
        book: String,
    },
    /// The name of a preset.
    Preset,
    /// The target of a link.
    LinkTarget {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// A word of an enum field.
    EnumWord {
        /// The book, as written.
        book: String,
        /// The field, as written.
        field: String,
    },
}

impl fmt::Display for NamePlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Book => f.write_str("the name of a book"),
            Self::Field { book } => write!(f, "a field of book `{book}`"),
            Self::Link { book } => write!(f, "a link of book `{book}`"),
            Self::Preset => f.write_str("the name of a preset"),
            Self::LinkTarget { book, link } => {
                write!(f, "the target of link `{link}` of book `{book}`")
            }
            Self::EnumWord { book, field } => {
                write!(f, "a word of field `{field}` of book `{book}`")
            }
        }
    }
}

/// One thing wrong in a declaration. [`Schema::validate`] returns all of them.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchemaProblem {
    /// A name that breaks the grammar of names (or is reserved).
    #[error("{place}: `{text}`: {why}")]
    BadName {
        /// Where it was written.
        place: NamePlace,
        /// The name as written.
        text: String,
        /// What is wrong with it.
        why: NameProblem,
    },
    /// A directory that is not a relative path.
    #[error("book `{book}`: the directory is not a relative path")]
    BadDir {
        /// The book, as written.
        book: String,
        /// Why the path is refused.
        #[source]
        source: ValueError,
    },
    /// A description that is blank or more than one line.
    #[error("book `{book}`: the description {why}")]
    BadDescription {
        /// The book, as written.
        book: String,
        /// What is wrong with it.
        why: DescriptionProblem,
    },
    /// A key prefix that breaks its grammar.
    #[error("book `{book}`: {why}")]
    BadKeyPrefix {
        /// The book, as written.
        book: String,
        /// What is wrong with it.
        why: KeyProblem,
    },
    /// An enum field with no word.
    #[error("book `{book}`: the enum field `{field}` lists no word")]
    EmptyEnum {
        /// The book, as written.
        book: String,
        /// The field, as written.
        field: String,
    },
    /// Two books of one name.
    #[error("the book `{name}` is declared twice")]
    DuplicateBook {
        /// The name.
        name: BookName,
    },
    /// A field written twice in a book.
    #[error("book `{book}`: the field `{name}` is declared twice")]
    DuplicateField {
        /// The book, as written.
        book: String,
        /// The field.
        name: EntryName,
    },
    /// A link written twice in a book.
    #[error("book `{book}`: the link `{name}` is declared twice")]
    DuplicateLink {
        /// The book, as written.
        book: String,
        /// The link.
        name: EntryName,
    },
    /// A name that is a field and a link of one book.
    #[error("book `{book}`: `{name}` is both a field and a link")]
    NameCollision {
        /// The book, as written.
        book: String,
        /// The name.
        name: EntryName,
    },
    /// A link into a book that is not declared.
    #[error("book `{book}`: the link `{link}` points into `{target}`, which is not declared")]
    UnknownLinkTarget {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The book it points into.
        target: BookName,
    },
    /// Books that use one prefix for their keys.
    #[error("the books {books:?} use the key prefix `{prefix}`")]
    PrefixClash {
        /// The prefix.
        prefix: KeyPrefix,
        /// The books that use it.
        books: TwoOrMore<BookName>,
    },
    /// Two books whose directories are one and the same, or one under the other.
    #[error("the directories of `{first}` and `{second}` are the same or one is under the other")]
    DirOverlap {
        /// One book.
        first: BookName,
        /// The other.
        second: BookName,
    },
    /// A pattern of `include` or `exclude` that is not a relative glob staying in the
    /// directory of the book.
    #[error("book `{book}`: the `{list}` pattern `{pattern}` {why}")]
    BadScopePattern {
        /// The book, as written.
        book: String,
        /// The list the pattern is in.
        list: ScopeList,
        /// The pattern, as written.
        pattern: String,
        /// What is wrong with it.
        why: PatternProblem,
    },
    /// `include = []`: the book would take no file.
    #[error("book `{book}`: `include` lists no pattern, so the book would take no file")]
    EmptyInclude {
        /// The book, as written.
        book: String,
    },
    /// A `without_front_matter` that is neither `"invalid"` nor `"skip"`.
    #[error(
        "book `{book}`: `without_front_matter` is \"{word}\", and it takes \"invalid\" or \"skip\""
    )]
    BadWithoutFrontMatter {
        /// The book, as written.
        book: String,
        /// The word, as written.
        word: String,
    },
    /// A `Replaces` link that points outside its own book.
    #[error(
        "book `{book}`: the link `{link}` replaces, so it points into this book, not `{target}`"
    )]
    ReplacesForeign {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The book it points into.
        target: BookName,
    },
    /// A link to artifacts that names no space: there is no space a link has when
    /// it names none.
    #[error(
        "book `{book}`: the link `{link}` points at artifacts and names no `space` they are in"
    )]
    NoSpace {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A link to artifacts whose `space` is no space.
    #[error("book `{book}`: the link `{link}`: {why}")]
    BadSpace {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// Why.
        why: SpaceProblem,
    },
    /// A link to artifacts whose space no adapter offers.
    #[error(
        "book `{book}`: the link `{link}` names the space `{space}`, which no adapter offers (it offers: {offered})"
    )]
    UnknownSpace {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The space it names.
        space: Space,
        /// The spaces offered, as one text.
        offered: String,
    },
    /// `space` on a link into a book: only artifacts are in a space.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes `space`"
    )]
    SpaceOnBookLink {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A link to artifacts that says it replaces them: an artifact takes no
    /// document's place, so such a link `depends` or `informs`.
    #[error(
        "book `{book}`: the link `{link}` points at artifacts, so it depends or informs and does not replace"
    )]
    ArtifactReplaces {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A book named with the word a link uses for artifacts.
    #[error("`{ARTIFACT_TARGET}` is the target of a link to artifacts and cannot name a book")]
    ReservedBookName,
    /// `compare` on a link into a book: a document is compared by its hash, always.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes `compare`"
    )]
    CompareOnBookLink {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// `cover` with a strength on a link to artifacts: only a document can be
    /// expected to be reached.
    #[error(
        "book `{book}`: the link `{link}` points at artifacts, and a link to artifacts takes no cover expectation (only `cover = false`)"
    )]
    CoverOnArtifactLink {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A `cover` that is no strength ([`CoverSeverity::read`]).
    #[error(
        "book `{book}`: the `cover` of the link `{link}` is \"{value}\", and it takes true, false, \"error\" or \"warn\""
    )]
    BadCover {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The word, as written.
        value: String,
    },
    /// A `compare` that is no comparison.
    #[error("book `{book}`: the `compare` of the link `{link}` is not one")]
    BadComparison {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// Why.
        #[source]
        why: ComparisonProblem,
    },
    /// A link, a `from` or the default that names a preset that is neither
    /// declared by the project nor offered by an adapter.
    #[error(
        "{place} names the preset `{name}`, which is not where it is looked for (the presets of the project, then those the adapters offer; for a `from` of the preset's own name, only those the adapters offer)"
    )]
    UnknownPreset {
        /// Where the name was written.
        place: PresetPlace,
        /// The name, as written.
        name: String,
    },
    /// Presets that each come `from` the next, back to the first.
    #[error("the presets {} come from one another in a cycle", cycle(presets))]
    PresetCycle {
        /// The presets of the cycle: the first is the one of them written first in
        /// the declaration, and each next one is the `from` of the one before it.
        /// The `from` of the last is the first again.
        presets: NonEmptyVec<PresetName>,
    },
    /// A preset whose `space` is no space.
    #[error("the preset `{preset}`: {why}")]
    PresetBadSpace {
        /// The preset, as written.
        preset: String,
        /// Why.
        why: SpaceProblem,
    },
    /// A preset whose `space` no adapter offers.
    #[error(
        "the preset `{preset}` names the space `{space}`, which no adapter offers (it offers: {offered})"
    )]
    PresetUnknownSpace {
        /// The preset, as written.
        preset: String,
        /// The space it names.
        space: Space,
        /// The spaces offered, as one text.
        offered: String,
    },
    /// A preset whose `compare` and `ignore`, as it writes them over those it comes
    /// from, are no comparison.
    #[error("the `compare` of the preset `{preset}` is not one")]
    PresetBadComparison {
        /// The preset, as written.
        preset: String,
        /// Why.
        #[source]
        why: ComparisonProblem,
    },
    /// Two presets of one name declared by the project.
    #[error("the preset `{name}` is declared twice")]
    DuplicatePreset {
        /// The name.
        name: PresetName,
    },
    /// `preset` on a link into a book: a preset holds the values of a link to
    /// artifacts.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes `preset`"
    )]
    PresetOnBookLink {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// A preset that says it replaces: it gives the values of a link to artifacts,
    /// which depends or informs. The one value a preset cannot hold (a value of
    /// another kind is no part of a preset at all, and the adapter that reads the
    /// declaration refuses it).
    #[error(
        "the preset `{preset}` means `replaces`, and a preset of links to artifacts depends or informs"
    )]
    PresetReplaces {
        /// The preset, as written.
        preset: String,
    },
    /// A link that says no `means`, and whose preset gives none.
    #[error("book `{book}`: the link `{link}` says no `means`, and no preset gives one")]
    NoMeans {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
}

/// The presets of a cycle, as a problem says them: `` `a` -> `b` -> `a` ``.
fn cycle(presets: &NonEmptyVec<PresetName>) -> String {
    presets
        .iter()
        .chain([presets.first()])
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// Whether a file belongs to a book: what `Schema::membership` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Membership<'a> {
    /// The file is under the directory of this book.
    In(&'a BookSchema),
    /// The file is under no book's directory.
    Outside,
}

/// The books of a project, checked as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    books: Declared<BookName, BookSchema>,
}

/// What [`Schema::validate`] learns of one declaration before it can be judged
/// against the others. Every part that could be read is kept, so the problems that
/// span books are reported even when a book has a problem of its own.
struct Draft {
    label: String,
    name: Option<BookName>,
    dir: Option<BookDir>,
    scope: Option<BookScope>,
    key: Option<KeyPattern>,
    description: Option<BookDescription>,
    fields: Declared<EntryName, FieldDecl>,
    links: Declared<EntryName, DraftEntry>,
    presets: BTreeMap<EntryName, PresetOrigin>,
    clean: bool,
}

/// A link of a draft, into a book or to artifacts.
enum DraftEntry {
    Book(DraftLink),
    Artifact(ArtifactLinkKind),
}

/// A link of a draft into a book: what is read of the declaration, before the
/// books are known together (the grammar of the target's keys is added when they
/// are).
struct DraftLink {
    target: BookName,
    cardinality: Cardinality,
    propagation: Propagation,
    cover: Option<CoverSeverity>,
}

impl Draft {
    fn read(
        decl: BookSchemaDecl,
        spaces: &BTreeSet<Space>,
        origins: &Origins,
        problems: &mut Vec<SchemaProblem>,
    ) -> Self {
        let label = decl.name.clone();
        let first_problem = problems.len();
        let name = read_book_name(&decl.name, problems);
        let dir = read_dir(&label, &decl.dir, problems);
        let key = read_key_pattern(&label, &decl.key, problems);
        let scope = read_scope(&label, decl.scope, problems);
        let description = read_description(&label, decl.description.as_deref(), problems);
        let fields = read_fields(&label, decl.fields, problems);
        let links = read_links(&label, decl.links, &fields, spaces, problems);
        let presets = links
            .iter()
            .filter(|(_, entry)| matches!(entry, DraftEntry::Artifact(_)))
            .filter_map(|(link, _)| Some((link.clone(), origins.of(&label, link.as_str())?)))
            .collect();
        Self {
            label,
            name,
            dir,
            scope,
            key,
            description,
            fields,
            links,
            presets,
            clean: problems.len() == first_problem,
        }
    }

    /// The links of the draft into books.
    fn book_links(&self) -> impl Iterator<Item = (&EntryName, &DraftLink)> {
        self.links.iter().filter_map(|(name, entry)| match entry {
            DraftEntry::Book(link) => Some((name, link)),
            DraftEntry::Artifact(_) => None,
        })
    }

    /// The book, when the declaration had no problem of its own. `key_of` gives the
    /// grammar of the keys of every book whose name and key were read; a link into
    /// a book that is not there was refused before, so a clean draft finds its
    /// targets.
    fn into_schema(self, key_of: &BTreeMap<BookName, KeyPattern>) -> Option<BookSchema> {
        if !self.clean {
            return None;
        }
        let mut links = Declared::default();
        for (name, entry) in self.links.into_entries() {
            let link = match entry {
                DraftEntry::Artifact(kind) => DeclaredLink::Artifact(kind),
                DraftEntry::Book(link) => DeclaredLink::Book(LinkKind {
                    target_key: key_of.get(&link.target)?.clone(),
                    target: link.target,
                    cardinality: link.cardinality,
                    propagation: link.propagation,
                    cover: link.cover,
                }),
            };
            links.insert(name, link);
        }
        Some(BookSchema {
            name: self.name?,
            range: BookRange::new(self.dir?, self.scope?),
            key: self.key?,
            description: self.description,
            fields: self.fields,
            links,
            presets: self.presets,
        })
    }
}

fn read_description(
    book: &str,
    text: Option<&str>,
    problems: &mut Vec<SchemaProblem>,
) -> Option<BookDescription> {
    match BookDescription::new(text?) {
        Ok(description) => Some(description),
        Err(why) => {
            problems.push(SchemaProblem::BadDescription {
                book: book.to_owned(),
                why,
            });
            None
        }
    }
}

fn read_book_name(text: &str, problems: &mut Vec<SchemaProblem>) -> Option<BookName> {
    if text == ARTIFACT_TARGET {
        problems.push(SchemaProblem::ReservedBookName);
        return None;
    }
    match BookName::new(text) {
        Ok(name) => Some(name),
        Err(why) => {
            problems.push(SchemaProblem::BadName {
                place: NamePlace::Book,
                text: text.to_owned(),
                why,
            });
            None
        }
    }
}

fn read_dir(book: &str, text: &str, problems: &mut Vec<SchemaProblem>) -> Option<BookDir> {
    match BookDir::new(text) {
        Ok(dir) => Some(dir),
        Err(source) => {
            problems.push(SchemaProblem::BadDir {
                book: book.to_owned(),
                source,
            });
            None
        }
    }
}

fn read_key_pattern(
    book: &str,
    decl: &KeyPatternDecl,
    problems: &mut Vec<SchemaProblem>,
) -> Option<KeyPattern> {
    match decl {
        KeyPatternDecl::Slug => Some(KeyPattern::Slug),
        KeyPatternDecl::Prefixed { prefix } => match KeyPrefix::new(prefix) {
            Ok(prefix) => Some(KeyPattern::Prefixed(prefix)),
            Err(why) => {
                problems.push(SchemaProblem::BadKeyPrefix {
                    book: book.to_owned(),
                    why,
                });
                None
            }
        },
    }
}

/// The fields of a book, in the order written; one that breaks a rule is reported
/// and left out.
fn read_fields(
    book: &str,
    entries: Vec<FieldEntryDecl>,
    problems: &mut Vec<SchemaProblem>,
) -> Declared<EntryName, FieldDecl> {
    let mut fields = Declared::default();
    for entry in entries {
        let name = match EntryName::new(&entry.name) {
            Ok(name) => name,
            Err(why) => {
                problems.push(SchemaProblem::BadName {
                    place: NamePlace::Field {
                        book: book.to_owned(),
                    },
                    text: entry.name,
                    why,
                });
                continue;
            }
        };
        let Some(ty) = read_field_type(book, &entry.name, entry.ty, problems) else {
            continue;
        };
        let decl = FieldDecl {
            ty,
            presence: entry.presence,
        };
        if !fields.insert(name.clone(), decl) {
            problems.push(SchemaProblem::DuplicateField {
                book: book.to_owned(),
                name,
            });
        }
    }
    fields
}

/// The type of a field; none when it is an enum that cannot be made (the problem is
/// reported).
fn read_field_type(
    book: &str,
    field: &str,
    ty: FieldTypeDecl,
    problems: &mut Vec<SchemaProblem>,
) -> Option<FieldType> {
    let words = match ty {
        FieldTypeDecl::Text => return Some(FieldType::Text),
        FieldTypeDecl::Integer => return Some(FieldType::Integer),
        FieldTypeDecl::Bool => return Some(FieldType::Bool),
        FieldTypeDecl::TextList => return Some(FieldType::TextList),
        FieldTypeDecl::Enum(words) => words,
    };
    let mut set = BTreeSet::new();
    let before_words = problems.len();
    for word in words {
        match FieldWord::new(&word) {
            Ok(word) => {
                set.insert(word);
            }
            Err(why) => problems.push(SchemaProblem::BadName {
                place: NamePlace::EnumWord {
                    book: book.to_owned(),
                    field: field.to_owned(),
                },
                text: word,
                why,
            }),
        }
    }
    match NonEmptySet::from_set(set) {
        Ok(set) => Some(FieldType::Enum(set)),
        Err(_) => {
            // A word that broke the grammar was reported above; an enum that listed
            // nothing at all is the problem here.
            if problems.len() == before_words {
                problems.push(SchemaProblem::EmptyEnum {
                    book: book.to_owned(),
                    field: field.to_owned(),
                });
            }
            None
        }
    }
}

/// The links of a book, into books and to artifacts; one that breaks a rule is
/// reported and left out. `fields` are the fields already read, for a name that is
/// both. A name is one link, whichever its target: a name written twice is reported
/// whether the two point at books or at artifacts. `spaces` are the spaces the
/// adapters offer. The links kept are in the order written.
fn read_links(
    book: &str,
    entries: Vec<LinkEntryDecl>,
    fields: &Declared<EntryName, FieldDecl>,
    spaces: &BTreeSet<Space>,
    problems: &mut Vec<SchemaProblem>,
) -> Declared<EntryName, DraftEntry> {
    let mut links = Declared::default();
    for entry in entries {
        let name = match EntryName::new(&entry.name) {
            Ok(name) => name,
            Err(why) => {
                problems.push(SchemaProblem::BadName {
                    place: NamePlace::Link {
                        book: book.to_owned(),
                    },
                    text: entry.name,
                    why,
                });
                continue;
            }
        };
        let target = match entry.target {
            LinkTargetDecl::Artifact => None,
            LinkTargetDecl::Book(target) => match BookName::new(&target) {
                Ok(target) => Some(target),
                Err(why) => {
                    problems.push(SchemaProblem::BadName {
                        place: NamePlace::LinkTarget {
                            book: book.to_owned(),
                            link: entry.name,
                        },
                        text: target,
                        why,
                    });
                    continue;
                }
            },
        };
        if fields.contains_key(&name) {
            problems.push(SchemaProblem::NameCollision {
                book: book.to_owned(),
                name: name.clone(),
            });
        }
        if links.contains_key(&name) {
            problems.push(SchemaProblem::DuplicateLink {
                book: book.to_owned(),
                name: name.clone(),
            });
        }
        let cover = match entry.cover.as_ref().map(CoverSeverity::read) {
            None => None,
            Some(Ok(cover)) => cover,
            Some(Err(value)) => {
                problems.push(SchemaProblem::BadCover {
                    book: book.to_owned(),
                    link: name,
                    value,
                });
                continue;
            }
        };
        match target {
            Some(target) => {
                if entry.compare.is_some() {
                    problems.push(SchemaProblem::CompareOnBookLink {
                        book: book.to_owned(),
                        link: name,
                    });
                    continue;
                }
                if entry.space.is_some() {
                    problems.push(SchemaProblem::SpaceOnBookLink {
                        book: book.to_owned(),
                        link: name,
                    });
                    continue;
                }
                let link = DraftLink {
                    target,
                    cardinality: entry.cardinality,
                    propagation: entry.propagation,
                    cover,
                };
                links.insert(name, DraftEntry::Book(link));
            }
            None => {
                if cover.is_some() {
                    problems.push(SchemaProblem::CoverOnArtifactLink {
                        book: book.to_owned(),
                        link: name,
                    });
                    continue;
                }
                let Some(space) = read_space(book, &name, entry.space.as_deref(), spaces, problems)
                else {
                    continue;
                };
                let comparison = match &entry.compare {
                    None => Comparison::Bytes,
                    Some(decl) => match Comparison::parse(&decl.kind, &decl.ignore) {
                        Ok(comparison) => comparison,
                        Err(why) => {
                            problems.push(SchemaProblem::BadComparison {
                                book: book.to_owned(),
                                link: name,
                                why,
                            });
                            continue;
                        }
                    },
                };
                let propagation = match entry.propagation {
                    Propagation::Depends => ArtifactPropagation::Depends,
                    Propagation::Informs => ArtifactPropagation::Informs,
                    Propagation::Replaces => {
                        problems.push(SchemaProblem::ArtifactReplaces {
                            book: book.to_owned(),
                            link: name,
                        });
                        continue;
                    }
                };
                let link = ArtifactLinkKind {
                    space,
                    cardinality: entry.cardinality,
                    propagation,
                    comparison,
                };
                links.insert(name, DraftEntry::Artifact(link));
            }
        }
    }
    links
}

/// The space of a link to artifacts: written, a space, and one the adapters offer
/// (the spaces come from the port, as a value). Each of the three that is not so is
/// reported.
fn read_space(
    book: &str,
    link: &EntryName,
    written: Option<&str>,
    spaces: &BTreeSet<Space>,
    problems: &mut Vec<SchemaProblem>,
) -> Option<Space> {
    let Some(written) = written else {
        problems.push(SchemaProblem::NoSpace {
            book: book.to_owned(),
            link: link.clone(),
        });
        return None;
    };
    let space = match Space::new(written) {
        Ok(space) => space,
        Err(why) => {
            problems.push(SchemaProblem::BadSpace {
                book: book.to_owned(),
                link: link.clone(),
                why,
            });
            return None;
        }
    };
    if !spaces.contains(&space) {
        problems.push(SchemaProblem::UnknownSpace {
            book: book.to_owned(),
            link: link.clone(),
            space,
            offered: offered(spaces),
        });
        return None;
    }
    Some(space)
}

/// The spaces offered, as a problem says them: `` `a`, `b` ``, or `none`.
fn offered(spaces: &BTreeSet<Space>) -> String {
    let offered: Vec<String> = spaces.iter().map(|s| format!("`{s}`")).collect();
    if offered.is_empty() {
        "none".to_owned()
    } else {
        offered.join(", ")
    }
}

/// The names of the books declared, each once; a name written twice is reported
/// (once, however many times it is written).
fn declared_names<'a>(
    drafts: &'a [Draft],
    problems: &mut Vec<SchemaProblem>,
) -> BTreeSet<&'a BookName> {
    let mut seen = BTreeSet::new();
    let mut reported = BTreeSet::new();
    for name in drafts.iter().filter_map(|d| d.name.as_ref()) {
        if !seen.insert(name) && reported.insert(name) {
            problems.push(SchemaProblem::DuplicateBook { name: name.clone() });
        }
    }
    seen
}

/// A link into a book that is not declared, and a `Replaces` link that leaves its
/// own book.
fn check_links(
    drafts: &[Draft],
    declared: &BTreeSet<&BookName>,
    problems: &mut Vec<SchemaProblem>,
) {
    for draft in drafts {
        for (link, kind) in draft.book_links() {
            if !declared.contains(&kind.target) {
                problems.push(SchemaProblem::UnknownLinkTarget {
                    book: draft.label.clone(),
                    link: link.clone(),
                    target: kind.target.clone(),
                });
            }
            if kind.propagation == Propagation::Replaces
                && draft.name.as_ref().is_some_and(|own| *own != kind.target)
            {
                problems.push(SchemaProblem::ReplacesForeign {
                    book: draft.label.clone(),
                    link: link.clone(),
                    target: kind.target.clone(),
                });
            }
        }
    }
}

/// Two or more books that use one key prefix.
fn check_prefixes(drafts: &[Draft], problems: &mut Vec<SchemaProblem>) {
    let mut by_prefix: BTreeMap<&KeyPrefix, BTreeSet<&BookName>> = BTreeMap::new();
    for draft in drafts {
        if let (Some(name), Some(KeyPattern::Prefixed(prefix))) = (&draft.name, &draft.key) {
            by_prefix.entry(prefix).or_default().insert(name);
        }
    }
    for (prefix, books) in by_prefix {
        if let Count::Many(books) = Count::of(books.into_iter().cloned()) {
            problems.push(SchemaProblem::PrefixClash {
                prefix: prefix.clone(),
                books,
            });
        }
    }
}

/// Two books whose directories are the same or nest.
fn check_dirs(drafts: &[Draft], problems: &mut Vec<SchemaProblem>) {
    for (i, a) in drafts.iter().enumerate() {
        for b in &drafts[i + 1..] {
            if let (Some(an), Some(ad), Some(bn), Some(bd)) = (&a.name, &a.dir, &b.name, &b.dir)
                && an != bn
                && ad.overlaps(bd)
            {
                problems.push(SchemaProblem::DirOverlap {
                    first: an.clone(),
                    second: bn.clone(),
                });
            }
        }
    }
}

impl Schema {
    /// The declaration of a project, checked as a whole.
    ///
    /// Every problem is returned, not the first: a name that breaks the grammar, a
    /// reserved name, a book, a field or a link declared twice, a name that is both
    /// a field and a link, a link into a book that is not declared, an enum with no
    /// word, two books that use one key prefix, two directories that are the same or
    /// nest, a `Replaces` link that leaves its own book, a link to artifacts that
    /// replaces, one that names no space, a text that is no space, or a space not in
    /// `spaces` (the spaces the adapters offer, given as a value), a book named
    /// [`ARTIFACT_TARGET`], a description that is no [`BookDescription`], a
    /// pattern of `include` or `exclude` that is no [`Pattern`], an `include` with
    /// none, and a `without_front_matter` that is no [`WithoutFrontMatter`]. A
    /// [`Schema`] exists only when there is none, so nothing after this re-checks a
    /// declaration. The books, and the fields and links of each, are kept in the
    /// order the declaration writes them, once ([`Declared`]).
    ///
    /// It takes declarations with no preset left in them: those of a project come
    /// through [`Schema::declared`], which [`expand`]s them first.
    // The problems are the answer, read once per run; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn validate(
        decls: Vec<BookSchemaDecl>,
        spaces: &BTreeSet<Space>,
    ) -> Result<Self, NonEmptyVec<SchemaProblem>> {
        Self::validate_expanded(Expanded::plain(decls), spaces)
    }

    /// The declaration of a project as it is written, with its presets and its
    /// default: [`expand`]ed with the presets the adapters offer (`builtins`), then
    /// checked as [`validate`](Self::validate) checks it, against `spaces`. What
    /// expanded is checked even when the expansion has problems (a link it could not
    /// expand is left out of the check, its problem told once), and the problems of
    /// both are returned at once, those of the expansion first. The schema keeps,
    /// for each link to artifacts, the preset it came from
    /// ([`BookSchema::preset_of`]).
    #[allow(clippy::result_large_err)]
    pub fn declared(
        decl: ShelfDecl,
        builtins: &BuiltinPresets,
        spaces: &BTreeSet<Space>,
    ) -> Result<Self, NonEmptyVec<SchemaProblem>> {
        let (expanded, problems) = expand_all(decl, builtins, spaces);
        let checked = Self::validate_expanded(expanded, spaces);
        match (NonEmptyVec::from_vec(problems), checked) {
            (Err(_), checked) => checked,
            (Ok(problems), Ok(_)) => Err(problems),
            (Ok(problems), Err(more)) => Err(problems.extended(more.into_vec())),
        }
    }

    #[allow(clippy::result_large_err)]
    fn validate_expanded(
        expanded: Expanded,
        spaces: &BTreeSet<Space>,
    ) -> Result<Self, NonEmptyVec<SchemaProblem>> {
        let (decls, origins) = expanded.into_parts();
        let mut problems = Vec::new();
        let drafts: Vec<Draft> = decls
            .into_iter()
            .map(|decl| Draft::read(decl, spaces, &origins, &mut problems))
            .collect();

        let declared = declared_names(&drafts, &mut problems);
        check_links(&drafts, &declared, &mut problems);
        check_prefixes(&drafts, &mut problems);
        check_dirs(&drafts, &mut problems);

        if let Ok(problems) = NonEmptyVec::from_vec(problems) {
            return Err(problems);
        }
        let key_of: BTreeMap<BookName, KeyPattern> = drafts
            .iter()
            .filter_map(|d| Some((d.name.clone()?, d.key.clone()?)))
            .collect();
        let mut books = Declared::default();
        for book in drafts
            .into_iter()
            .filter_map(|draft| draft.into_schema(&key_of))
        {
            books.insert(book.name.clone(), book);
        }
        Ok(Self { books })
    }

    /// The book a file belongs to: the one whose directory it is under.
    ///
    /// The one entry of the question; the shelf asks it of every file it reads.
    pub(super) fn membership(&self, path: &RelPath) -> Membership<'_> {
        self.books
            .values()
            .find(|book| book.takes(path))
            .map_or(Membership::Outside, Membership::In)
    }

    /// The directory and range of every book, in the order of the declaration: what
    /// a listing of the documents is given.
    pub fn ranges(&self) -> Vec<BookRange> {
        self.books
            .values()
            .map(|book| book.range().clone())
            .collect()
    }

    /// A book by name, given as a [`BookName`] or as the text of one.
    pub fn book<Q>(&self, name: &Q) -> Option<&BookSchema>
    where
        BookName: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.books.get(name)
    }

    /// The books, by name, in the order of the declaration.
    pub fn books(&self) -> &Declared<BookName, BookSchema> {
        &self.books
    }
}

mod declared;
mod preset;
mod scope;

use scope::read_scope;
pub use scope::{
    BookRange, BookScope, Pattern, PatternProblem, ScopeDecl, ScopeList, WithoutFrontMatter,
};

pub use declared::{Declared, Picked};
pub use preset::{
    BuiltinPresets, DefaultsDecl, Expanded, PresetDecl, PresetOrigin, PresetPlace, PresetValues,
    ShelfDecl, WrittenBookDecl, WrittenLinkDecl, expand,
};
use preset::{Origins, expand_all};

#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod preset_tests;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod tests;
