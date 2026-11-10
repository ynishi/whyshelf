//! The one use case that writes: `judge`.
//!
//! A judgement is made in five steps, and the application decides none of them:
//!
//! ```text
//!   reference ---- resolve ----------------------> the document's identity
//!   identity  ---- frame_approve / frame_present
//!                  / frame_retire / status_of ----> the facts the decision stands on,
//!                                                   or a FrameRefusal
//!   frame + time + actor + new id
//!             ---- Decision::approve / deny /
//!                  deprecate / retire,
//!                  Revocation::new ---------------> a Record, or a DecisionProblem
//!   record    ---- LedgerAppend::append ----------> the ledger (create-only)
//! ```
//!
//! The time, the identity of the record and the actor are fetched only after every
//! check has passed, so a refused judgement uses none of them. The write is the last
//! step, so a refusal writes nothing.
//!
//! **`--successor`.** A deprecation records only that the document is to be replaced;
//! who replaces it is a fact of the links ([`Shelf::replacers_of`]) and a verdict of
//! [`Successors::of`], and is not written into the record. A successor named with a
//! deprecation is therefore a check: the judgement is refused unless the document
//! named is one of [`Successors::of`] the document deprecated.
//!
//! **`revoke`.** A revocation withdraws a decision, not a document, so the reference
//! names the document and the decision withdrawn is the one that stands about it now
//! ([`Ledger::status_of`]).
//!
//! [`Shelf::replacers_of`]: crate::domain::shelf::build::Shelf::replacers_of
//! [`Successors::of`]: crate::domain::shelf::freshness::Successors::of
//! [`Ledger::status_of`]: crate::domain::shelf::ledger::Ledger::status_of

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::freshness::Successors;
use crate::domain::shelf::ledger::{
    ApproveFrame, Decision, DeclaredStatus, JudgedAt, JudgementId, PresentFrame, Record,
    RetireFrame, Revocation, frame_approve, frame_present, frame_retire,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::load;
use super::ports::{ActorSource, IdSource, LedgerAppend, ShelfPort, ShelfWrite, port};
use super::read::identity;
use super::report::Judged;

/// What a person decides about a document, with what only that decision has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// The document is sound as it is now, and the approval stands on what it links to.
    Approve {
        /// When the approval lapses, if it does.
        until: Option<JudgedAt>,
        /// Why, if the person says.
        reason: Option<NonBlank>,
    },
    /// The document is refused as it is now.
    Deny {
        /// Why, if the person says.
        reason: Option<NonBlank>,
    },
    /// The document is to be replaced.
    Deprecate {
        /// A document that is said to replace it, as typed: checked, not recorded.
        successor: Option<String>,
        /// Why, if the person says.
        reason: Option<NonBlank>,
    },
    /// The document is no longer kept. It need not be valid, nor have a file.
    Retire {
        /// Why, if the person says.
        reason: Option<NonBlank>,
    },
    /// The decision that stands about the document is withdrawn.
    Revoke {
        /// Why. A withdrawal without a reason is not made.
        reason: NonBlank,
    },
}

/// A request to judge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeRequest {
    /// What is decided.
    pub judgement: Judgement,
    /// The document, as typed: a key, an identifier or a short identifier.
    pub reference: String,
    /// Who judges. `None` is the person the actor port names.
    pub actor: Option<Actor>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Make a judgement and append it to the ledger. The only use case that writes.
    ///
    /// A judgement the model refuses (a document that is not valid, a link that
    /// cannot be followed, a cycle, an expiry that is not in the future, nothing to
    /// revoke, a successor that is none) writes nothing.
    pub async fn judge(&mut self, request: JudgeRequest) -> Result<Judged, ShelfError> {
        let loaded = load(&self.ports).await?;
        let subject = identity(&loaded, &request.reference)?;
        let key = loaded.shelf.doc(subject).map(|doc| doc.key().clone());
        let now = loaded.now;

        // Every check first: the frame (or what stands to be revoked) and, for a
        // deprecation, the successor.
        enum Plan {
            Approve(ApproveFrame, Option<JudgedAt>),
            Deny(PresentFrame),
            Deprecate(PresentFrame),
            Retire(RetireFrame),
            Revoke(NonEmptySet<JudgementId>, NonBlank),
        }
        let mut successors = None;
        let (plan, reason) = match request.judgement {
            Judgement::Approve { until, reason } => (
                Plan::Approve(
                    frame_approve(&loaded.shelf, &loaded.ledger, subject)?,
                    until,
                ),
                reason,
            ),
            Judgement::Deny { reason } => {
                (Plan::Deny(frame_present(&loaded.shelf, subject)?), reason)
            }
            Judgement::Deprecate { successor, reason } => {
                let frame = frame_present(&loaded.shelf, subject)?;
                let of = Successors::of(&loaded.shelf, &loaded.ledger, subject);
                if let Some(text) = successor {
                    let named = identity(&loaded, &text)?;
                    let listed = match &of {
                        Successors::None => false,
                        Successors::Some(ids) => ids.iter().any(|id| *id == named),
                    };
                    if !listed {
                        return Err(ShelfError::NotASuccessor {
                            subject,
                            successor: named,
                        });
                    }
                }
                successors = Some(of);
                (Plan::Deprecate(frame), reason)
            }
            Judgement::Retire { reason } => {
                (Plan::Retire(frame_retire(&loaded.shelf, subject)), reason)
            }
            Judgement::Revoke { reason } => {
                let standing = match loaded.ledger.status_of(subject) {
                    DeclaredStatus::Pending => return Err(ShelfError::NothingToRevoke { subject }),
                    DeclaredStatus::Approved { judgement, .. }
                    | DeclaredStatus::Denied { judgement }
                    | DeclaredStatus::Deprecated { judgement }
                    | DeclaredStatus::Retired { judgement } => judgement,
                };
                let targets = NonEmptySet::from_set(BTreeSet::from([standing]))?;
                (Plan::Revoke(targets, reason), None)
            }
        };

        let actor = match request.actor {
            Some(actor) => actor,
            None => self
                .ports
                .actor_source()
                .actor()
                .await
                .map_err(port(ShelfPort::Actor))?,
        };
        let id = self
            .ports
            .ids()
            .new_judgement_id()
            .await
            .map_err(port(ShelfPort::Ids))?;

        let record = match plan {
            Plan::Approve(frame, until) => {
                Record::Decision(Decision::approve(id, frame, until, actor, now, reason)?)
            }
            Plan::Deny(frame) => Record::Decision(Decision::deny(id, frame, actor, now, reason)?),
            Plan::Deprecate(frame) => {
                Record::Decision(Decision::deprecate(id, frame, actor, now, reason)?)
            }
            Plan::Retire(frame) => {
                Record::Decision(Decision::retire(id, frame, actor, now, reason)?)
            }
            Plan::Revoke(targets, reason) => {
                Record::Revocation(Revocation::new(id, targets, actor, now, reason))
            }
        };

        self.ports
            .append()
            .append(&record)
            .await
            .map_err(port(ShelfPort::LedgerAppend))?;
        Ok(Judged {
            record,
            subject,
            key,
            successors,
        })
    }
}
