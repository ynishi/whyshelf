//! What a person typed, resolved to an identity.
//!
//! A text is read once, where it comes in, into a [`Reference`] (its `FromStr` is the
//! one reader of the grammar below); [`resolve`] is the one entry that turns a
//! reference into an identity. A reference has four shapes, read in a fixed order
//! (the first that applies decides, so a text means one thing):
//!
//! 1. `<book>:<key>`: a key in that book's grammar, looked up with
//!    `Shelf::find_key`. A valid and an invalid document are both *found*; what
//!    state the document is in is [`Shelf::state_of`]'s answer, not this one's.
//! 2. A UUID in its one written form: a document of the shelf, or failing that, a
//!    subject of the ledger.
//! 3. A short id ([`ShortId::looks_like`]): matched against the end of the
//!    identities of the shelf and of the ledger's subjects. A prefixed key has a `-`
//!    and a slug is refused when it looks like a short id, so the two never meet.
//! 4. A key alone: tried in every book whose grammar accepts it, through
//!    `Shelf::find_key`. A hit in more than one document, in one book or across
//!    books, is ambiguous and is listed, not chosen from.
//!
//! The ledger is read too, so that a document whose file was deleted can still be
//! named by its identifier (to look at its history, or to retire it): that is a
//! [`Resolution::LedgerOnly`].

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::id::{DocId, EntityId, ShortId};
use crate::domain::location::RelPath;
use crate::domain::nonempty::{Count, NonEmptyVec, TwoOrMore};

use super::build::{DocState, KeyHit, Shelf};
use super::ledger::Ledger;
use super::schema::{BookName, KeyProblem, NameProblem};

/// A key as a person wrote it, before a book's grammar is asked: not empty, no `:`
/// and no white space. Which grammar it is read in is [`resolve`]'s work, since a
/// bare key may be tried in every book.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WrittenKey(String);

impl WrittenKey {
    /// The key as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WrittenKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a person names a document by.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reference {
    /// `<book>:<key>`: a key in one book.
    InBook {
        /// The book.
        book: BookName,
        /// The key, to be read in the book's grammar.
        key: WrittenKey,
    },
    /// A UUID in its one written form.
    Id(DocId),
    /// The end of a UUID.
    Short(ShortId),
    /// A key alone, tried in every book.
    Key(WrittenKey),
}

/// Why a text is no reference.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReferenceProblem {
    /// Nothing was written.
    #[error("a reference is not empty")]
    Empty,
    /// The book before the `:` is not a name of a book.
    #[error("`{book}` is not a name of a book")]
    BadBook {
        /// The text before the `:`.
        book: String,
        /// Why.
        #[source]
        why: NameProblem,
    },
    /// The key is empty, or has a `:` or white space.
    #[error("`{key}` is not a key: a key is not empty and has no `:` and no space")]
    BadKey {
        /// The key as written.
        key: String,
    },
    /// It has the shape of a short identifier and is not one.
    #[error("`{text}` is not a short identifier")]
    BadShortId {
        /// The text.
        text: String,
        /// Why.
        #[source]
        why: ValueError,
    },
}

fn written_key(text: &str) -> Result<WrittenKey, ReferenceProblem> {
    if text.is_empty() || text.contains(':') || text.chars().any(char::is_whitespace) {
        return Err(ReferenceProblem::BadKey {
            key: text.to_owned(),
        });
    }
    Ok(WrittenKey(text.to_owned()))
}

impl FromStr for Reference {
    type Err = ReferenceProblem;

    /// The one reader of the grammar of a reference; the order is in the module page.
    fn from_str(text: &str) -> Result<Self, ReferenceProblem> {
        if text.is_empty() {
            return Err(ReferenceProblem::Empty);
        }
        if let Some((book, key)) = text.split_once(':') {
            let book = BookName::new(book).map_err(|why| ReferenceProblem::BadBook {
                book: book.to_owned(),
                why,
            })?;
            return Ok(Self::InBook {
                book,
                key: written_key(key)?,
            });
        }
        if let Ok(id) = text.parse::<EntityId>() {
            return Ok(Self::Id(DocId::new(id)));
        }
        if ShortId::looks_like(text) {
            return text.parse::<ShortId>().map(Self::Short).map_err(|why| {
                ReferenceProblem::BadShortId {
                    text: text.to_owned(),
                    why,
                }
            });
        }
        written_key(text).map(Self::Key)
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InBook { book, key } => write!(f, "{book}:{key}"),
            Self::Id(id) => write!(f, "{id}"),
            Self::Short(short) => write!(f, "{}", short.as_str()),
            Self::Key(key) => write!(f, "{key}"),
        }
    }
}

/// Why the shelf has no file for an identity the ledger knows: the two
/// [`DocFailure`]s that are not [`Invalid`](super::build::DocFailure::Invalid) (an
/// invalid file is a document of the shelf, and [`Resolution::Found`]).
///
/// [`DocFailure`]: super::build::DocFailure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Absence {
    /// No file carries it.
    Gone,
    /// No file is known to carry it, but these files' identities could not be read.
    Unknown(NonEmptyVec<RelPath>),
}

/// What a reference comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A document of the shelf, valid or invalid; ask [`Shelf::state_of`] which.
    Found(DocId),
    /// No file of the shelf has the identity, but the ledger has records about it.
    LedgerOnly {
        /// The identity.
        id: DocId,
        /// Why the shelf has no document: it is gone, or cannot be told.
        state: Absence,
    },
    /// Nothing: the reference is well formed and names no document.
    Missing,
    /// More than one document.
    Ambiguous(TwoOrMore<DocId>),
    /// `<book>:<key>` names a book the schema does not declare.
    UnknownBook(BookName),
    /// `<book>:<key>` writes a key the book's grammar refuses.
    NotAKeyOf {
        /// The book.
        book: BookName,
        /// Why its grammar refuses the key.
        problem: KeyProblem,
    },
    /// A key alone that no book's grammar accepts.
    NotAKeyOfAnyBook,
}

/// The identity a reference names.
///
/// The one entry for what a person types (a `get`, a `trace`, a judgement); the order
/// of the shapes is in the module page.
pub fn resolve(shelf: &Shelf, ledger: &Ledger, reference: &Reference) -> Resolution {
    match reference {
        Reference::InBook { book, key } => in_book(shelf, book, key),
        Reference::Id(id) => by_identity(shelf, ledger, *id),
        Reference::Short(short) => by_short_id(shelf, ledger, short),
        Reference::Key(key) => in_any_book(shelf, key),
    }
}

fn in_book(shelf: &Shelf, name: &BookName, key: &WrittenKey) -> Resolution {
    let Some(book) = shelf.schema().book(name) else {
        return Resolution::UnknownBook(name.clone());
    };
    let form = match book.key().parse(key.as_str()) {
        Ok(form) => form,
        Err(problem) => {
            return Resolution::NotAKeyOf {
                book: name.clone(),
                problem,
            };
        }
    };
    match shelf.find_key(book.name(), &form) {
        KeyHit::Valid(id) | KeyHit::Invalid(id) => Resolution::Found(id),
        KeyHit::Ambiguous(ids) => Resolution::Ambiguous(ids),
        KeyHit::Missing => Resolution::Missing,
    }
}

fn by_identity(shelf: &Shelf, ledger: &Ledger, id: DocId) -> Resolution {
    let absence = match shelf.state_of(id) {
        DocState::Valid(_) | DocState::Invalid(_) => return Resolution::Found(id),
        DocState::Gone => Absence::Gone,
        DocState::Unknown { unreadable } => Absence::Unknown(unreadable),
    };
    if ledger.subjects().contains(&id) {
        Resolution::LedgerOnly { id, state: absence }
    } else {
        Resolution::Missing
    }
}

fn by_short_id(shelf: &Shelf, ledger: &Ledger, short: &ShortId) -> Resolution {
    let candidates: BTreeSet<DocId> = shelf
        .ids()
        .into_iter()
        .chain(ledger.subjects().iter().copied())
        .filter(|id| short.matches(id.entity()))
        .collect();
    match Count::of(candidates) {
        Count::None => Resolution::Missing,
        Count::One(id) => by_identity(shelf, ledger, id),
        Count::Many(ids) => Resolution::Ambiguous(ids),
    }
}

fn in_any_book(shelf: &Shelf, key: &WrittenKey) -> Resolution {
    let mut hits: BTreeSet<DocId> = BTreeSet::new();
    let mut read_by_some_book = false;
    for book in shelf.schema().books().values() {
        let Ok(form) = book.key().parse(key.as_str()) else {
            continue;
        };
        read_by_some_book = true;
        match shelf.find_key(book.name(), &form) {
            KeyHit::Valid(id) | KeyHit::Invalid(id) => {
                hits.insert(id);
            }
            KeyHit::Ambiguous(ids) => hits.extend(ids.into_vec()),
            KeyHit::Missing => {}
        }
    }
    match Count::of(hits) {
        Count::None if !read_by_some_book => Resolution::NotAKeyOfAnyBook,
        Count::None => Resolution::Missing,
        Count::One(id) => Resolution::Found(id),
        Count::Many(ids) => Resolution::Ambiguous(ids),
    }
}

#[cfg(test)]
mod tests;
