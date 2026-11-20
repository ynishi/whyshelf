//! The names a declaration gives: of a book, a field or a link, an enum word, a
//! preset and a group ([`BookName`], [`EntryName`], [`FieldWord`], [`PresetName`],
//! [`GroupName`]), with the one
//! grammar of names they share, the directory of a book ([`BookDir`]) and what a book
//! is for ([`BookDescription`]).

use std::borrow::Borrow;
use std::fmt;

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::location::RelPath;

/// The most characters of the name of a book, a field, a link or an enum word.
const NAME_MAX: usize = 64;
/// The most characters of a name in the tail of a key (`FR-LATENCY-BUDGET`): its own
/// limit, which happens to be the same as the one of a name of a field.
pub(super) const KEY_NAME_MAX: usize = 64;
/// The most characters of the prefix of a book's keys.
pub(super) const PREFIX_MAX: usize = 16;
pub(super) const NUMBER_MAX_DIGITS: usize = 19;
pub(super) const SLUG_MAX: usize = 128;

/// The entry of a front matter that holds the identity of a document.
pub(in crate::domain::shelf) const ID_ENTRY: &str = "id";
/// The entry of a front matter that holds the key of a document.
pub(in crate::domain::shelf) const KEY_ENTRY: &str = "key";

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
    /// is one flat mapping: [`Schema::validate`](super::Schema::validate) refuses a name that is both.
    EntryName,
    reserved = true
);
name_type!(
    /// A word of an enum field: the grammar of [`BookName`].
    FieldWord,
    reserved = false
);
name_type!(
    /// The name of a preset of links to artifacts ([`expand`](super::expand)): the grammar of
    /// [`BookName`].
    PresetName,
    reserved = false
);
name_type!(
    /// The name of a group of actors ([`Group`](super::Group)): the grammar of
    /// [`BookName`].
    GroupName,
    reserved = false
);

/// The directory of a book, relative to the project root.
///
/// A file belongs to a book when it is under the directory of that book and the
/// book's range takes it ([`BookRange::takes`](super::BookRange::takes), see `Schema::membership`). Two books
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
