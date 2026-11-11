//! Declarations that a document is not expected to be reached by one covering link:
//! [`Exempt`], the record.
//!
//! A link declared with `cover` expects every pending or approved document of its
//! target book to be reached by it ([`LinkKind::cover`]). A person may say, for one
//! document and one such link, that it is meant to stay unreached, and why (a finding
//! kept on purpose that grounds no requirement). The rule stays declared for the
//! whole book; the exception is a record of its own, by an actor, at a time, for a
//! reason, read on every assessment like every other record
//! ([`Ledger::exemptions`]), and withdrawn by a [`Revocation`](super::Revocation), as
//! a decision is. Its canonical form is the ledger's, beside the other records'.
//!
//! What an exemption may be made of, and how one reads now, is the module
//! [`exemption`](super::super::exemption): it reads both the ledger and the
//! documents, so it is above both, and the ledger knows nothing of it.
//!
//! [`LinkKind::cover`]: super::super::schema::LinkKind::cover
//! [`Ledger::exemptions`]: super::Ledger::exemptions

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;

use super::super::schema::{BookName, EntryName};
use super::{JudgedAt, JudgementId};

/// What an exemption is of: one document, and the covering link, by its book and its
/// name, that is not expected to reach it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExemptOf {
    doc: DocId,
    book: BookName,
    link: EntryName,
}

impl ExemptOf {
    /// The document `doc`, from the link `link` of the book `book`. Whether the link
    /// covers the document's book is
    /// [`frame_exempt`](super::super::exemption::frame_exempt)'s to say.
    pub fn new(doc: DocId, book: BookName, link: EntryName) -> Self {
        Self { doc, book, link }
    }

    /// The document.
    pub fn doc(&self) -> DocId {
        self.doc
    }

    /// The book that declares the link.
    pub fn book(&self) -> &BookName {
        &self.book
    }

    /// The link.
    pub fn link(&self) -> &EntryName {
        &self.link
    }
}

/// A person's declaration that a document is not expected to be reached by one
/// covering link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exempt {
    pub(super) id: JudgementId,
    pub(super) of: ExemptOf,
    pub(super) actor: Actor,
    pub(super) at: JudgedAt,
    pub(super) reason: NonBlank,
}

impl Exempt {
    /// An exemption ([`frame_exempt`](super::super::exemption::frame_exempt) framed
    /// what it is of). The reason is required: an exemption is a claim a reader must
    /// be able to check.
    pub fn new(
        id: JudgementId,
        of: ExemptOf,
        actor: Actor,
        at: JudgedAt,
        reason: NonBlank,
    ) -> Self {
        Self {
            id,
            of,
            actor,
            at,
            reason,
        }
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The document and the link.
    pub fn of(&self) -> &ExemptOf {
        &self.of
    }

    /// Who declared it.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// When.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// Why.
    pub fn reason(&self) -> &NonBlank {
        &self.reason
    }
}
