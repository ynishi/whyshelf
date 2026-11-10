//! What a person typed, resolved to an identity.
//!
//! [`resolve`] is the one entry. A reference has four shapes, tried in a fixed order
//! (the first that applies decides, so a text means one thing):
//!
//! 1. `<book>:<key>`: a key in that book's grammar, looked up with
//!    [`Shelf::find_key`]. A valid and an invalid document are both *found*; what
//!    state the document is in is [`Shelf::state_of`]'s answer, not this one's.
//! 2. A UUID in its one written form: a document of the shelf, or failing that, a
//!    subject of the ledger.
//! 3. A short id ([`ShortId::looks_like`]): matched against the end of the
//!    identities of the shelf and of the ledger's subjects. A prefixed key has a `-`
//!    and a slug is refused when it looks like a short id, so the two never meet.
//! 4. A key alone: tried in every book whose grammar accepts it, through
//!    [`Shelf::find_key`]. A hit in more than one document, in one book or across
//!    books, is ambiguous and is listed, not chosen from.
//!
//! The ledger is read too, so that a document whose file was deleted can still be
//! named by its identifier (to look at its history, or to retire it): that is a
//! [`Resolution::LedgerOnly`].

use std::collections::BTreeSet;

use crate::domain::id::{DocId, EntityId, ShortId};
use crate::domain::nonempty::TwoOrMore;

use super::build::{DocStateKind, KeyHit, Shelf};
use super::ledger::Ledger;

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
        state: DocStateKind,
    },
    /// Nothing.
    Missing,
    /// More than one document.
    Ambiguous(TwoOrMore<DocId>),
}

/// The identity a reference names.
///
/// The one entry for what a person types (a `get`, a `trace`, a judgement); the order
/// of the shapes is in the module page.
pub fn resolve(shelf: &Shelf, ledger: &Ledger, text: &str) -> Resolution {
    if let Some((book, key)) = text.split_once(':') {
        return in_book(shelf, book, key);
    }
    if let Ok(id) = text.parse::<EntityId>() {
        return by_identity(shelf, ledger, DocId::new(id));
    }
    if ShortId::looks_like(text) {
        return by_short_id(shelf, ledger, text);
    }
    in_any_book(shelf, text)
}

fn in_book(shelf: &Shelf, book: &str, key: &str) -> Resolution {
    let Some(book) = shelf.schema().book_named(book) else {
        return Resolution::Missing;
    };
    let Ok(form) = book.key().parse(key) else {
        return Resolution::Missing;
    };
    match shelf.find_key(book.name(), &form) {
        KeyHit::Valid(id) | KeyHit::Invalid(id) => Resolution::Found(id),
        KeyHit::Ambiguous(ids) => Resolution::Ambiguous(ids),
        KeyHit::Missing => Resolution::Missing,
    }
}

fn by_identity(shelf: &Shelf, ledger: &Ledger, id: DocId) -> Resolution {
    match shelf.state_of(id).valid_or_kind() {
        Ok(_) | Err(DocStateKind::Invalid(_)) => Resolution::Found(id),
        Err(state) if ledger.subjects().contains(&id) => Resolution::LedgerOnly { id, state },
        Err(_) => Resolution::Missing,
    }
}

fn by_short_id(shelf: &Shelf, ledger: &Ledger, text: &str) -> Resolution {
    let Ok(short) = text.parse::<ShortId>() else {
        return Resolution::Missing;
    };
    let candidates: BTreeSet<DocId> = shelf
        .ids()
        .into_iter()
        .chain(ledger.subjects().iter().copied())
        .filter(|id| short.matches(id.entity()))
        .collect();
    let mut ids = candidates.into_iter();
    match (ids.next(), ids.next()) {
        (None, _) => Resolution::Missing,
        (Some(id), None) => by_identity(shelf, ledger, id),
        (Some(first), Some(second)) => {
            Resolution::Ambiguous(TwoOrMore::new(first, second, ids.collect()))
        }
    }
}

fn in_any_book(shelf: &Shelf, text: &str) -> Resolution {
    let mut hits: BTreeSet<DocId> = BTreeSet::new();
    for book in shelf.schema().books().values() {
        let Ok(form) = book.key().parse(text) else {
            continue;
        };
        match shelf.find_key(book.name(), &form) {
            KeyHit::Valid(id) | KeyHit::Invalid(id) => {
                hits.insert(id);
            }
            KeyHit::Ambiguous(ids) => hits.extend(ids.into_vec()),
            KeyHit::Missing => {}
        }
    }
    let mut ids = hits.into_iter();
    match (ids.next(), ids.next()) {
        (None, _) => Resolution::Missing,
        (Some(id), None) => Resolution::Found(id),
        (Some(first), Some(second)) => {
            Resolution::Ambiguous(TwoOrMore::new(first, second, ids.collect()))
        }
    }
}

#[cfg(test)]
mod tests;
