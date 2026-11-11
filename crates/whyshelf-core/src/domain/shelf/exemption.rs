//! The rule of an exemption from a covering link ([`Exempt`]): what one may be made
//! of, and how one reads now.
//!
//! The record is the ledger's ([`Exempt`], [`ExemptOf`], and its canonical form);
//! the rule reads the ledger and the documents both, so it is here, above them, and
//! freshness tells its notices with it:
//!
//! ```text
//!   exemption ──► schema, doc, build, ledger, cover
//!   freshness ──► cover, exemption       (NotCovered, Exempt, ExemptionUnneeded,
//!                                         ExemptionRepeated)
//! ```
//!
//! One reading, [`exemption_reading`], says whether an exemption is needed now or
//! why it is not ([`Unneeded`]). [`frame_exempt`] accepts an exemption only when that
//! reading says it would be needed the moment it is written, so acceptance and
//! reading cannot disagree: whatever would read as unneeded at once is refused, as an
//! approval that would only repeat one is not made.
//!
//! ```text
//!   requirement.grounds  cover --> finding        F-1 reached          refused / exemption_unneeded
//!                                                 F-2 not reached      not_covered
//!   exempt F-2 from requirement.grounds           F-2 not reached      exempt
//!                                                 F-2 reached later    exemption_unneeded
//! ```

use std::collections::BTreeMap;

use thiserror::Error;

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;

use super::build::{FailureTag, Shelf};
use super::cover::{Decided, Reached, counts, covering_reach};
use super::doc::DocKey;
use super::ledger::{Exempt, ExemptOf, JudgementId, Ledger};
use super::schema::{BookName, CoverSeverity, EntryName, Schema};

/// Why a link of a book is no covering link into a document's book.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoverMiss {
    /// The book is not declared, or declares no link of that name into a book.
    #[error("no link of that name into a book is declared")]
    NotDeclared,
    /// The link is declared and expects nothing (`cover` is false, or not written).
    #[error("the link is declared with no cover")]
    NoCover,
    /// The link covers another book than the document's.
    #[error("the link points into `{target}`, and the document is of `{doc_book}`")]
    OtherBook {
        /// The book the link points into.
        target: BookName,
        /// The document's book.
        doc_book: BookName,
    },
}

/// The strength with which the link `link` of the book `book` expects to reach every
/// document of `doc_book`, or why it expects nothing of it. With no `doc_book` (a
/// document that is not valid now has no book to tell), only whether the link is
/// declared with a cover is asked.
pub fn cover_of(
    schema: &Schema,
    book: &BookName,
    link: &EntryName,
    doc_book: Option<&BookName>,
) -> Result<CoverSeverity, CoverMiss> {
    let kind = schema
        .book(book)
        .and_then(|b| b.links().get(link))
        .ok_or(CoverMiss::NotDeclared)?;
    let severity = kind.cover.ok_or(CoverMiss::NoCover)?;
    match doc_book {
        Some(doc_book) if *doc_book != kind.target => Err(CoverMiss::OtherBook {
            target: kind.target.clone(),
            doc_book: doc_book.clone(),
        }),
        _ => Ok(severity),
    }
}

/// Why an exemption is not needed now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unneeded {
    /// The link reaches the document now, from these pending or approved documents,
    /// by identity.
    Reached {
        /// The documents it is reached from.
        by: NonEmptyVec<DocId>,
    },
    /// The link is no covering link into the document's book now: no longer
    /// declared, declared with no cover, or pointing into another book
    /// ([`cover_of`]).
    Cover(CoverMiss),
    /// The document is no valid document now: no book of it can be told.
    NotValid(FailureTag),
    /// The document was decided against (denied, deprecated or retired): no
    /// covering link expects to reach it.
    DecidedAgainst,
}

/// How an exemption of `of` reads now: needed, with the strength of the link, or
/// unneeded, with why. The one rule of it, for both its readers: freshness tells it
/// as a notice of every exemption that stands, and [`frame_exempt`] refuses an
/// exemption it would read as unneeded the moment it is written.
///
/// | case, in this order | reading |
/// |---|---|
/// | the link is no covering link into the document's book now ([`cover_of`]; of a document not valid, only whether the link is declared with a cover) | [`Unneeded::Cover`] |
/// | the document is no valid document now | [`Unneeded::NotValid`] |
/// | the document does not count ([`counts`]: it is denied, deprecated or retired) | [`Unneeded::DecidedAgainst`] |
/// | the link reaches the document now, from a document that counts | [`Unneeded::Reached`], with those documents |
/// | otherwise | needed |
pub fn exemption_reading(
    shelf: &Shelf,
    ledger: &Ledger,
    of: &ExemptOf,
) -> Result<CoverSeverity, Unneeded> {
    let counted = |id| counts(Decided::from(&ledger.status_of(id)));
    read_exemption(shelf, counted, &covering_reach(shelf, counted), of)
}

/// [`exemption_reading`], on what counts and what the covering links reach, as an
/// assessment has them already.
pub(super) fn read_exemption(
    shelf: &Shelf,
    counted: impl Fn(DocId) -> bool,
    reached: &Reached,
    of: &ExemptOf,
) -> Result<CoverSeverity, Unneeded> {
    let doc = shelf.state_of(of.doc()).valid_or_failure();
    let doc_book = doc.as_ref().ok().map(|d| &d.key().book);
    let severity =
        cover_of(shelf.schema(), of.book(), of.link(), doc_book).map_err(Unneeded::Cover)?;
    if let Err(failure) = doc {
        return Err(Unneeded::NotValid(failure.tag()));
    }
    if !counted(of.doc()) {
        return Err(Unneeded::DecidedAgainst);
    }
    let from = reached
        .get(&(of.doc(), of.book().clone(), of.link().clone()))
        .map(|from| from.iter().copied().collect::<Vec<_>>())
        .and_then(|from| NonEmptyVec::from_vec(from).ok());
    match from {
        Some(by) => Err(Unneeded::Reached { by }),
        None => Ok(severity),
    }
}

/// The exemptions that stand, by what they are of, each group in the order of
/// identity. Two or more of one (document, book, link) come of branches merged,
/// each of which made one: freshness tells them as repeated.
pub(super) fn by_subject(ledger: &Ledger) -> BTreeMap<&ExemptOf, Vec<&Exempt>> {
    let mut groups: BTreeMap<&ExemptOf, Vec<&Exempt>> = BTreeMap::new();
    for exempt in ledger.exemptions() {
        groups.entry(exempt.of()).or_default().push(exempt);
    }
    groups
}

/// Why an exemption is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExemptRefusal {
    /// The document is not a valid document now, so its book cannot be told.
    #[error("{id} is not a valid document now: {tag}")]
    DocNotValid {
        /// The document.
        id: DocId,
        /// How it is not.
        tag: FailureTag,
    },
    /// The link is no covering link into the document's book. The cause is the
    /// source, told once.
    #[error("{book}.{link} cannot exempt it")]
    NotCovering {
        /// The book named.
        book: BookName,
        /// The link named.
        link: EntryName,
        /// Why.
        #[source]
        miss: CoverMiss,
    },
    /// The document is denied, deprecated or retired: no covering link expects to
    /// reach it, so there is nothing to exempt it from.
    #[error("{} is decided against: no covering link expects to reach it", label(*id, key.as_ref()))]
    DecidedAgainst {
        /// The document.
        id: DocId,
        /// Its key, by which the message names it; the identity names it when it has
        /// none.
        key: Option<DocKey>,
    },
    /// The link reaches the document now: the exemption would be unneeded at once.
    #[error("{book}.{link} reaches it now, from {}", keys(by))]
    Reached {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
        /// The documents it is reached from, by their keys, in the order of identity.
        by: Vec<DocKey>,
    },
    /// An exemption that stands is already of the same document from the same link.
    #[error("{record} already exempts it from {book}.{link}")]
    AlreadyExempt {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
        /// The exemption that stands.
        record: JudgementId,
    },
    /// An earlier exemption of the same run, not written yet, is of the same
    /// document from the same link: the document is named twice. The application
    /// tells this apart from [`ExemptRefusal::AlreadyExempt`], since only it knows
    /// which records are of the run.
    #[error("an earlier exemption of this run already exempts it from {book}.{link}")]
    AlreadyExemptInRun {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
    },
}

/// A document as a message names it: by its key, else by its identity.
fn label(id: DocId, key: Option<&DocKey>) -> String {
    key.map_or_else(|| id.to_string(), ToString::to_string)
}

/// Keys, joined for a sentence.
fn keys(keys: &[DocKey]) -> String {
    keys.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What an exemption of the document `id` from the link `link` of the book `book`
/// would be of.
///
/// Accepted when the exemption would be needed the moment it is written, as
/// [`exemption_reading`] reads it (the document valid now, the link declared with a
/// cover of either strength into its book, the document not decided against, and
/// not reached by the link now), and no exemption that stands
/// ([`Ledger::exemptions`]) is of the same document from the same link. Refused
/// otherwise, before any record exists: an exemption unneeded at once adds nothing
/// to the ledger. A second exemption is refused, not repeated: it would be a second
/// record of one fact, with a reason of its own, and withdrawing one of the two
/// would leave the document exempt with no word of it.
///
/// The one place what an exemption may be made of is decided.
pub fn frame_exempt(
    shelf: &Shelf,
    ledger: &Ledger,
    id: DocId,
    book: BookName,
    link: EntryName,
) -> Result<ExemptOf, ExemptRefusal> {
    if let Err(failure) = shelf.state_of(id).valid_or_failure() {
        return Err(ExemptRefusal::DocNotValid {
            id,
            tag: failure.tag(),
        });
    }
    let of = ExemptOf::new(id, book, link);
    let refusal = match exemption_reading(shelf, ledger, &of) {
        Ok(_) => None,
        Err(Unneeded::Cover(miss)) => Some(ExemptRefusal::NotCovering {
            book: of.book().clone(),
            link: of.link().clone(),
            miss,
        }),
        Err(Unneeded::NotValid(tag)) => Some(ExemptRefusal::DocNotValid { id, tag }),
        Err(Unneeded::DecidedAgainst) => Some(ExemptRefusal::DecidedAgainst {
            id,
            key: shelf.doc(id).map(|doc| doc.key().clone()),
        }),
        // A document a covering link reaches from is valid, so it has a key.
        Err(Unneeded::Reached { by }) => Some(ExemptRefusal::Reached {
            book: of.book().clone(),
            link: of.link().clone(),
            by: by
                .iter()
                .filter_map(|from| shelf.doc(*from).map(|doc| doc.key().clone()))
                .collect(),
        }),
    };
    if let Some(refusal) = refusal {
        return Err(refusal);
    }
    if let Some(standing) = ledger.exemptions().find(|e| *e.of() == of) {
        return Err(ExemptRefusal::AlreadyExempt {
            book: of.book().clone(),
            link: of.link().clone(),
            record: standing.id(),
        });
    }
    Ok(of)
}

#[cfg(test)]
mod tests;
