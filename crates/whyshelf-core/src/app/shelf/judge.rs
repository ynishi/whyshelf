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
    ApproveFrame, Decision, DeclaredStatus, JudgedAt, JudgementId, Ledger, PresentFrame, Record,
    RetireFrame, Revocation, frame_approve, frame_present, frame_retire,
};
use crate::domain::shelf::lookup::Reference;

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, load};
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
        /// A document that is said to replace it: checked, not recorded.
        successor: Option<Reference>,
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
    /// The document.
    pub reference: Reference,
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
        let mut judged = self.judge_all(vec![request]).await?;
        judged.pop().ok_or(ShelfError::NothingRequested)
    }

    /// Make several judgements in order, on one reading of the project, and append
    /// them all, or none.
    ///
    /// Each is decided on the ledger as the ones before it left it, so a run that
    /// approves a finding and then the spec that stands on it frames the spec on
    /// that approval. Every judgement is decided before anything is written: when one
    /// is refused, the error names it and nothing is written. (A port that fails
    /// half-way through the writes can still leave the first of them written; each is
    /// a record of its own, so what is there is sound.)
    pub async fn judge_all(
        &mut self,
        requests: Vec<JudgeRequest>,
    ) -> Result<Vec<Judged>, ShelfError> {
        let mut loaded = load(&self.ports).await?;
        let mut judged = Vec::with_capacity(requests.len());
        for request in requests {
            let one = self.decide(&loaded, request).await?;
            loaded.records.push(one.record.clone());
            loaded.ledger = Ledger::build(loaded.records.clone());
            judged.push(one);
        }
        for one in &judged {
            self.ports
                .append()
                .append(&one.record)
                .await
                .map_err(port(ShelfPort::LedgerAppend))?;
        }
        Ok(judged)
    }

    /// One judgement, decided on what is loaded; nothing is written.
    async fn decide(&self, loaded: &Loaded, request: JudgeRequest) -> Result<Judged, ShelfError> {
        let subject = identity(loaded, &request.reference)?;
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
                if let Some(successor) = successor {
                    let named = identity(loaded, &successor)?;
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

        Ok(Judged {
            record,
            subject,
            key,
            successors,
        })
    }
}
