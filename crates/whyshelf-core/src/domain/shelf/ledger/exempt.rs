//! Declarations that a document or an artifact is not expected to be reached by one
//! covering link: [`Exempt`], the record.
//!
//! A link declared with `cover` expects every document of its target book that no
//! decision went against ([`NotStanding`]) to be reached by it ([`LinkKind::cover`]),
//! and a link to artifacts with `cover` expects every name of its space its selection
//! takes ([`ArtifactLinkKind::cover`]). A person may say, for one document or one
//! name and one such link, that it is meant to stay unreached, and why (a finding
//! kept on purpose that grounds no requirement). The rule stays declared for the
//! whole book; the exception is a record of its own, by an actor, at a time, for a
//! reason, read on every assessment like every other record ([`Ledger::exemptions`]),
//! and withdrawn by a [`Revocation`](super::Revocation), as a decision is. Its
//! canonical form is the ledger's, beside the other records': the subject is a text
//! (the identity of a document) or a map of the space and the name (an artifact).
//!
//! What an exemption may be made of, and how one reads now, is the module
//! [`exemption`](super::super::exemption): it reads both the ledger and the
//! documents, so it is above both, and the ledger knows nothing of it.
//!
//! [`NotStanding`]: super::NotStanding
//! [`LinkKind::cover`]: super::super::schema::LinkKind::cover
//! [`ArtifactLinkKind::cover`]: super::super::schema::ArtifactLinkKind::cover
//! [`Ledger::exemptions`]: super::Ledger::exemptions

use crate::domain::actor::{Actor, Attribution};
use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;

use super::super::artifact::ArtifactKey;
use super::super::schema::{BookName, EntryName};
use super::{JudgedAt, JudgementId};

/// What an exemption is about: a document, or an artifact (a name in a space).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExemptSubject {
    /// A document, by its identity.
    Doc(DocId),
    /// An artifact, by its space and its name.
    Artifact(ArtifactKey),
}

/// What an exemption is of: one subject (a document or an artifact), and the
/// covering link, by its book and its name, that is not expected to reach it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExemptOf {
    subject: ExemptSubject,
    book: BookName,
    link: EntryName,
}

impl ExemptOf {
    /// The document `doc`, from the link `link` of the book `book`. Whether the link
    /// covers the document's book is
    /// [`frame_exempt`](super::super::exemption::frame_exempt)'s to say.
    pub fn new(doc: DocId, book: BookName, link: EntryName) -> Self {
        Self {
            subject: ExemptSubject::Doc(doc),
            book,
            link,
        }
    }

    /// The artifact `artifact`, from the link `link` of the book `book`. Whether the
    /// link covers the names of the artifact's space is
    /// [`frame_exempt_artifact`](super::super::exemption::frame_exempt_artifact)'s to
    /// say.
    pub fn of_artifact(artifact: ArtifactKey, book: BookName, link: EntryName) -> Self {
        Self {
            subject: ExemptSubject::Artifact(artifact),
            book,
            link,
        }
    }

    /// The document or the artifact.
    pub fn subject(&self) -> &ExemptSubject {
        &self.subject
    }

    /// The document, when the subject is one.
    pub fn doc(&self) -> Option<DocId> {
        match &self.subject {
            ExemptSubject::Doc(id) => Some(*id),
            ExemptSubject::Artifact(_) => None,
        }
    }

    /// The artifact, when the subject is one.
    pub fn artifact(&self) -> Option<&ArtifactKey> {
        match &self.subject {
            ExemptSubject::Artifact(key) => Some(key),
            ExemptSubject::Doc(_) => None,
        }
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
    pub(super) by: Attribution,
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
        by: Attribution,
        at: JudgedAt,
        reason: NonBlank,
    ) -> Self {
        Self {
            id,
            of,
            by,
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
        self.by.actor()
    }

    /// Who declared it, and who ran the command when it was another.
    pub fn attribution(&self) -> &Attribution {
        &self.by
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
