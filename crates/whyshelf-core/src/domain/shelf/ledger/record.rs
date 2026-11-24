//! A record of the ledger: [`Record`], one of the kinds of judgement, with its
//! identity, the [`Revocation`] that withdraws others, and the [`RecordHash`] that
//! tells two records of one identity apart. Their canonical form is `canon.rs`'s.

use crate::domain::actor::Attribution;
use crate::domain::hash::ContentHash;
use crate::domain::nonempty::{NonBlank, NonEmptySet};

use crate::domain::id::DocId;

use super::{Decision, Exempt, JudgedAt, JudgementId, Moved, SameMeaning, SameMeaningOf};

/// The withdrawal of earlier decisions and declarations of the same meaning,
/// whoever made them and whatever they are about. It has no subject and no frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    pub(super) id: JudgementId,
    pub(super) targets: NonEmptySet<JudgementId>,
    pub(super) by: Attribution,
    pub(super) at: JudgedAt,
    pub(super) reason: NonBlank,
}

impl Revocation {
    /// A revocation of one or more decisions or declarations.
    pub fn new(
        id: JudgementId,
        targets: NonEmptySet<JudgementId>,
        by: Attribution,
        at: JudgedAt,
        reason: NonBlank,
    ) -> Self {
        Self {
            id,
            targets,
            by,
            at,
            reason,
        }
    }
}

/// One record of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// A decision about a document.
    Decision(Decision),
    /// A withdrawal of decisions or declarations.
    Revocation(Revocation),
    /// A declaration that an edit keeps the meaning.
    SameMeaning(SameMeaning),
    /// A declaration that an artifact moved from one name to another.
    Moved(Moved),
    /// A declaration that a document is not expected to be reached by one covering
    /// link.
    Exempt(Exempt),
}

/// The hash of a record's canonical form, which tells two records of one id apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordHash(pub(super) ContentHash);

impl RecordHash {
    /// The digest: of
    /// [`HashKind::ShelfJudgement`](crate::domain::hash::HashKind::ShelfJudgement) over
    /// the canonical form.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

/// What a record is about, as the record itself says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordAbout<'a> {
    /// One document: a decision's subject, the document a declaration of the same
    /// meaning or an exemption is about.
    Doc(DocId),
    /// The records it withdraws (a revocation).
    Records(&'a NonEmptySet<JudgementId>),
    /// An artifact alone: a move, a declaration of the same meaning or an exemption of
    /// an artifact.
    Artifact,
}

impl Record {
    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        match self {
            Self::Decision(d) => d.id(),
            Self::Revocation(r) => r.id,
            Self::SameMeaning(s) => s.id,
            Self::Moved(m) => m.id,
            Self::Exempt(e) => e.id,
        }
    }

    /// What the record is about, read from its own fields: the one place it is told.
    pub fn about(&self) -> RecordAbout<'_> {
        match self {
            Self::Decision(d) => RecordAbout::Doc(d.subject()),
            Self::Revocation(r) => RecordAbout::Records(&r.targets),
            Self::SameMeaning(s) => match s.of() {
                SameMeaningOf::Doc { id, .. } => RecordAbout::Doc(*id),
                SameMeaningOf::Artifact { .. } => RecordAbout::Artifact,
            },
            Self::Moved(_) => RecordAbout::Artifact,
            Self::Exempt(e) => e.of().doc().map_or(RecordAbout::Artifact, RecordAbout::Doc),
        }
    }
}
