//! Why a value is refused by its rule, one variant per reason.
//!
//! A caller branches on the kind of failure, so no error here is a message
//! string or a list of strings. Each constructor of a value with rules returns
//! [`ValueError`] and keeps the cause (`#[source]`) when there is one. The errors
//! of the model itself are in [`shelf`](super::shelf).

use thiserror::Error;

/// Why a text could not become a value object.
///
/// One enum for all the small newtypes keeps the constructors uniform
/// (`TryFrom` / `FromStr` return it) while still telling a caller exactly which
/// rule was broken.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValueError {
    /// A hash text did not start with the `sha256:` scheme.
    #[error("a content hash starts with `sha256:`")]
    HashPrefix,
    /// A hash digest was not 64 characters long.
    #[error("a content hash digest has 64 hex characters, found {len}")]
    HashLength {
        /// Number of characters found after the scheme.
        len: usize,
    },
    /// A hash digest held a character that is not lowercase hex.
    #[error("a content hash digest is lowercase hex")]
    HashNotLowerHex,
    /// An identifier text was not a UUID.
    #[error("an entity id is a UUID")]
    IdNotUuid(#[source] uuid::Error),
    /// An identifier text was a UUID, but not in the one form an identifier is
    /// written in: lowercase, with hyphens.
    #[error("an entity id is written as a lowercase hyphenated UUID")]
    IdNotCanonical,
    /// An identifier was a UUID of another version than 7.
    #[error("an entity id is a UUID v7, found version {version}")]
    IdNotV7 {
        /// The UUID version that was found.
        version: usize,
    },
    /// A free text that must carry content was empty or only whitespace.
    #[error("the text is empty")]
    BlankText,
    /// A list that must have an element was empty.
    #[error("the list is empty")]
    EmptyList,
    /// A path was empty.
    #[error("a path is empty")]
    PathEmpty,
    /// A path was absolute.
    #[error("a path is relative to the project root")]
    PathAbsolute,
    /// A path held an empty, `.` or `..` segment.
    #[error("a path has no empty, `.` or `..` segment")]
    PathBadSegment,
    /// A path used a backslash or a control character.
    #[error("a path uses `/` and no control character")]
    PathBadChar,
    /// A name of a person was empty.
    #[error("an actor name is empty")]
    ActorEmpty,
    /// A name of a person held a line break or was too long.
    #[error("an actor name is one short line")]
    ActorBadShape,
    /// A short id had fewer characters than the least that cannot match by accident.
    #[error("a short id has at least {min} hexadecimal characters, found {len}")]
    ShortIdTooShort {
        /// Characters found.
        len: usize,
        /// The least.
        min: usize,
    },
    /// A short id had more characters than the hexadecimal digits of a UUID.
    #[error("a short id has at most {max} hexadecimal characters, found {len}")]
    ShortIdTooLong {
        /// Characters found.
        len: usize,
        /// The most.
        max: usize,
    },
    /// A short id held a character that is not lowercase hexadecimal.
    #[error("a short id is lowercase hexadecimal, found `{ch}`")]
    ShortIdBadChar {
        /// The offending character.
        ch: char,
    },
}
