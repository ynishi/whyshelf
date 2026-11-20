//! A decision about one document: [`Decision`], what it does ([`DecisionVerb`]), and
//! why it cannot be made ([`DecisionProblem`]). Every way of making one goes through
//! `Decision::assemble`, which holds its invariants, the expiry's among them.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;

use super::super::artifact::{ArtifactKey, ContentAddress, Revision};
use super::super::doc::DocHash;
use super::super::schema::{BookName, EntryName};
use super::{
    ApproveFrame, ArtifactPin, FramePin, JudgedAt, JudgementId, PresentFrame, RetireFrame, Seen,
};

/// What a decision does.
///
/// Each verb holds what only it can have: an approval stands on a frame and may
/// expire, a retirement may have seen a document that is no longer valid, the others
/// saw one hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionVerb {
    /// The document is sound as it was seen.
    Approve {
        /// The hash that was read and approved.
        seen: DocHash,
        /// The upstreams the approval stands on.
        frame: BTreeSet<FramePin>,
        /// The artifacts the approval stands on (rule A2).
        artifacts: BTreeSet<ArtifactPin>,
        /// When the approval lapses, if it does.
        expires: Option<JudgedAt>,
        /// Where the content of the approved document's file, as it was approved,
        /// can be read back (rule A5).
        seen_address: Option<ContentAddress>,
        /// The point of the project's history the approval was made at, as a hint
        /// (rule A5).
        revision: Option<Revision>,
    },
    /// The document is refused as it was seen.
    Deny {
        /// The hash that was read.
        seen: DocHash,
    },
    /// The document is to be replaced.
    Deprecate {
        /// The hash that was read.
        seen: DocHash,
    },
    /// The document is no longer kept.
    Retire {
        /// What was seen of it.
        seen: Seen,
    },
}

/// Why a decision cannot be made.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecisionProblem {
    /// An approval that lapses at or before the moment it is made.
    #[error("an approval that expires must expire after it is made")]
    ExpiresNotAfter {
        /// When it is made.
        at: JudgedAt,
        /// When it would lapse.
        expires: JudgedAt,
    },
    /// An approval that stands on the document it approves.
    #[error("an approval's frame does not name the approved document")]
    FrameNamesSubject,
    /// An approval whose frame pins one link to one document twice (with two
    /// hashes): it cannot say which it saw.
    #[error("an approval's frame pins `{link}` to {target} twice")]
    FramePinsTwice {
        /// The link.
        link: EntryName,
        /// The document.
        target: DocId,
    },
    /// An approval whose frame pins one artifact of one link twice (with two
    /// states): it cannot say which it saw.
    #[error("an approval's frame pins `{link}` to {artifact} twice")]
    ArtifactPinsTwice {
        /// The link.
        link: EntryName,
        /// The artifact.
        artifact: ArtifactKey,
    },
}

/// A decision about a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    id: JudgementId,
    subject: DocId,
    verb: DecisionVerb,
    actor: Actor,
    at: JudgedAt,
    reason: Option<NonBlank>,
    book: Option<BookName>,
}

impl Decision {
    /// An approval.
    ///
    /// `expires` must be after `at`.
    pub fn approve(
        id: JudgementId,
        frame: ApproveFrame,
        expires: Option<JudgedAt>,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Approve {
            seen: frame.seen,
            frame: frame.frame,
            artifacts: frame.artifacts,
            expires,
            seen_address: frame.seen_address,
            revision: frame.revision,
        };
        Self::assemble(id, frame.subject, verb, actor, at, reason, frame.book)
    }

    /// A denial.
    pub fn deny(
        id: JudgementId,
        frame: PresentFrame,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Deny { seen: frame.seen };
        Self::assemble(id, frame.subject, verb, actor, at, reason, frame.book)
    }

    /// A deprecation.
    pub fn deprecate(
        id: JudgementId,
        frame: PresentFrame,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Deprecate { seen: frame.seen };
        Self::assemble(id, frame.subject, verb, actor, at, reason, frame.book)
    }

    /// A retirement.
    pub fn retire(
        id: JudgementId,
        frame: RetireFrame,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Retire { seen: frame.seen };
        Self::assemble(id, frame.subject, verb, actor, at, reason, frame.book)
    }

    /// The one place the invariants of a decision are checked (the expiry's by
    /// [`DecisionProblem::expires_after`]): every constructor and the reader of the canonical form go
    /// through it. `book` is the book of the subject the record names (its key `book`),
    /// when it names one.
    pub(super) fn assemble(
        id: JudgementId,
        subject: DocId,
        verb: DecisionVerb,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
        book: Option<BookName>,
    ) -> Result<Self, DecisionProblem> {
        if let DecisionVerb::Approve {
            frame,
            artifacts,
            expires,
            ..
        } = &verb
        {
            let mut pinned = BTreeSet::new();
            for pin in artifacts {
                if !pinned.insert((pin.link(), pin.artifact())) {
                    return Err(DecisionProblem::ArtifactPinsTwice {
                        link: pin.link().clone(),
                        artifact: pin.artifact().clone(),
                    });
                }
            }
            DecisionProblem::expires_after(at, *expires)?;
            if frame.iter().any(|pin| pin.target() == subject) {
                return Err(DecisionProblem::FrameNamesSubject);
            }
            let mut seen = BTreeSet::new();
            for pin in frame {
                if !seen.insert((pin.link(), pin.target())) {
                    return Err(DecisionProblem::FramePinsTwice {
                        link: pin.link().clone(),
                        target: pin.target(),
                    });
                }
            }
        }
        Ok(Self {
            id,
            subject,
            verb,
            actor,
            at,
            reason,
            book,
        })
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The document it is about.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// What it does, with what only that verb has.
    pub fn verb(&self) -> &DecisionVerb {
        &self.verb
    }

    /// Who decided.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// When.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// Why, as it was given, if it was.
    pub fn reason(&self) -> Option<&NonBlank> {
        self.reason.as_ref()
    }

    /// The book its document was in when it was made, as the record names it (its
    /// key `book`): none for a record made before records named one, or of a
    /// document whose book the shelf could not tell. Only who may decide reads it.
    pub fn book(&self) -> Option<&BookName> {
        self.book.as_ref()
    }
}

impl DecisionProblem {
    /// An approval made at `at` that lapses at `expires` has not lapsed by then
    /// ([`JudgedAt::lapsed_by`]), or has no expiry: the check of
    /// [`DecisionProblem::ExpiresNotAfter`], made by a decision as it is assembled and
    /// by a run before it asks for its actor.
    pub fn expires_after(at: JudgedAt, expires: Option<JudgedAt>) -> Result<(), Self> {
        match expires {
            Some(expires) if expires.lapsed_by(at) => {
                Err(DecisionProblem::ExpiresNotAfter { at, expires })
            }
            _ => Ok(()),
        }
    }
}
