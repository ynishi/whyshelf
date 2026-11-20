//! The grammar of the keys of a book: [`KeyPattern`], the one reader of a key
//! ([`KeyPattern::parse`]) and the parts it checks ([`KeyForm`], [`KeyPrefix`],
//! [`KeyTail`], [`KeyName`], [`Slug`]), with the word a declaration has for a grammar
//! ([`KeyWord`]).

use std::fmt;

use thiserror::Error;

use crate::domain::id::ShortId;

use super::name::{KEY_NAME_MAX, NUMBER_MAX_DIGITS, PREFIX_MAX, SLUG_MAX};
use super::{BookEntry, BookWord, Words};

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
    pub(in crate::domain::shelf) fn new(text: &str) -> Result<Self, KeyProblem> {
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
    pub(in crate::domain::shelf) fn new(text: &str) -> Result<Self, KeyProblem> {
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
/// A slug is not empty and has at most 128 characters. It holds only ASCII
/// lowercase letters, digits, `.`, `-`, `_` and `/`, and none of the segments `/`
/// parts it into is empty, so it neither begins nor ends with `/`. A text in the
/// form of a short id or of a UUID is refused, so that a slug is never mistaken
/// for an identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slug(String);

impl Slug {
    pub(in crate::domain::shelf) fn new(text: &str) -> Result<Self, KeyProblem> {
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
    /// The word a declaration has for this grammar ([`KeyWord`]), when it has one: a
    /// prefixed key is written as a table, which is no word.
    pub fn word(&self) -> Option<&'static str> {
        match self {
            Self::Slug => Some(KeyWord::Slug.word()),
            Self::Prefixed(_) => None,
        }
    }

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
pub(in crate::domain::shelf) fn parse_tail(text: &str) -> Result<KeyTail, KeyProblem> {
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

/// The one word a declaration has for the grammar of keys that takes no argument: a
/// path of lowercase words. The other grammar is a table (`{ prefix = "..." }`), which
/// is no word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyWord {
    /// A path of lowercase words: [`KeyPattern::Slug`].
    Slug,
}

impl KeyWord {
    /// Every word, in the order a problem lists them.
    pub const ALL: [Self; 1] = [Self::Slug];

    /// The word of the grammar in a declaration and in the answers: `slug`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Slug => "slug",
        }
    }
}

impl Words for KeyWord {
    fn all() -> &'static [Self] {
        &Self::ALL
    }

    fn word(self) -> &'static str {
        KeyWord::word(self)
    }
}

impl BookWord for KeyWord {
    const ENTRY: BookEntry = BookEntry::Key;
}
