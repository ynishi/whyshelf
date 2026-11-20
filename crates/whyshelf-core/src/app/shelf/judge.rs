//! The use case that writes decisions to the ledger: `judge` (the ledger is also
//! written by [`same_meaning`](super::ShelfApp::same_meaning), a declaration that an
//! edit keeps the meaning, and by [`withdraw`](super::ShelfApp::withdraw), a
//! revocation of records named by their identity; the other writes are
//! [`new_doc`](super::ShelfApp::new_doc), which makes a document's file, and
//! [`fmt`](super::ShelfApp::fmt), which writes documents' files over in the form).
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
//!   record    ---- RunWrites::record --------------> the ledger (create-only), every
//!                                                   record put on disk once, at the end
//! ```
//!
//! An approval is given, between the frame and the record, the address of each thing
//! it stands on and the revision now ([`ApproveFrame::addressed`], rule A5): the
//! addresses of the artifacts from the load (what the
//! [`ArtifactSource`](super::ArtifactSource) answered),
//! those of the documents' files from the [`DocSource`], the revision from the
//! [`RevisionSource`]. None of them changes what is refused.
//!
//! The time is the one the project was read at. A run first checks what it is asked
//! (every reference resolves, every expiry is after now), then asks for the actor
//! once, before any judgement is decided, since what an approval is depends on who
//! makes it ([`Ledger::kind_of`]): with no name the run fails, whatever it would have
//! written. The identity of each record is fetched after its frame (or what stands to
//! be revoked, or the successor) is checked, and before the decision's own
//! invariants are, which may still refuse it. The write is the last step, for every
//! judgement of a run after all of them are decided, so a refusal writes nothing.
//!
//! **`--successor`.** A deprecation records only that the document is to be replaced;
//! who replaces it is a fact of the links ([`Shelf::replacers_of`]) and a verdict of
//! [`Successors::of`], and is not written into the record. A successor named with a
//! deprecation is therefore a check: the judgement is refused unless the document
//! named is one of [`Successors::of`] the document deprecated.
//!
//! **An approval that adds nothing.** Between the frame and the record, an approval
//! is asked of the domain what it would be beside the readers of the content that
//! stands ([`Ledger::kind_of`]), and whether a run that named the document leaves it
//! out ([`ApprovalKind::left_out`]): a repeat of the reading of the one who approves
//! is, unless the request says `again` ([`LeftOutWhy::Repeats`]: no record, no
//! refusal). Another actor's approval of the same content is a second reading and is
//! written.
//!
//! **`revoke`.** A revocation withdraws a decision, not a document, so the reference
//! names the document and the decision withdrawn is the one that stands about it now
//! ([`Ledger::status_of`]).
//!
//! [`Shelf::replacers_of`]: crate::domain::shelf::build::Shelf::replacers_of
//! [`Successors::of`]: crate::domain::shelf::freshness::Successors::of
//! [`Ledger::status_of`]: crate::domain::shelf::ledger::Ledger::status_of
//! [`Ledger::kind_of`]: crate::domain::shelf::ledger::Ledger::kind_of
//! [`ApprovalKind::left_out`]: crate::domain::shelf::ledger::ApprovalKind::left_out

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::freshness::Successors;
use crate::domain::shelf::ledger::{
    ApproveFrame, Asked, Before, Decision, DecisionProblem, DecisionVerb, DeclaredStatus, JudgedAt,
    JudgementId, Ledger, PresentFrame, Record, RetireFrame, Revocation, Taking, frame_approve,
    frame_present, frame_retire,
};
use crate::domain::shelf::lookup::Reference;

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, load};
use super::ports::{ActorSource, DocSource, IdSource, RevisionSource, ShelfPort, ShelfWrite, port};
use super::read::identity;
use super::report::{Judged, JudgedAll, LeftOut, LeftOutWhy};
use super::run_writes::RunWrites;

/// Take a judgement decided into the reading, so the next of the run is decided on
/// the ledger as it left it.
pub(super) fn taken(loaded: &mut Loaded, one: &Judged) {
    loaded.records.push(one.record.clone());
    loaded.ledger = Ledger::build(loaded.records.clone());
}

/// What a request asks, checked before the run asks for its actor: its reference and
/// a deprecation's successor resolve, and an approval's expiry is after now. The rest
/// of what may refuse it is checked as it is decided.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
fn checked(loaded: &Loaded, request: &JudgeRequest) -> Result<(), ShelfError> {
    identity(loaded, &request.reference)?;
    match &request.judgement {
        Judgement::Approve { until, .. } => expires_after_now(loaded, *until),
        Judgement::Deprecate {
            successor: Some(successor),
            ..
        } => identity(loaded, successor).map(|_| ()),
        Judgement::Deny { .. }
        | Judgement::Deprecate { .. }
        | Judgement::Retire { .. }
        | Judgement::Revoke { .. } => Ok(()),
    }
}

/// An approval's expiry, when it has one, is after the time the project was read at
/// ([`DecisionProblem::ExpiresNotAfter`]): checked before a run asks for its actor.
#[allow(clippy::result_large_err)]
pub(super) fn expires_after_now(
    loaded: &Loaded,
    until: Option<JudgedAt>,
) -> Result<(), ShelfError> {
    DecisionProblem::expires_after(loaded.now, until).map_err(Into::into)
}

/// What a person decides about a document, with what only that decision has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// The document is sound as it is now, and the approval stands on what it links to.
    Approve {
        /// When the approval lapses, if it does.
        until: Option<JudgedAt>,
        /// Why, if the person says.
        reason: Option<NonBlank>,
        /// Make it even when it would only repeat the reading of the one who approves
        /// ([`ApprovalKind::Repeat`]): for a person who read the document again and
        /// wants that recorded. Without it, such an approval is left out.
        ///
        /// [`ApprovalKind::Repeat`]: crate::domain::shelf::ledger::ApprovalKind::Repeat
        again: bool,
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

/// One judgement decided: a record to write, or an approval left out.
pub(super) enum Decided {
    /// The judgement, with the record to write.
    Judged(Judged),
    /// An approval not made, with why.
    LeftOut(LeftOut),
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Make a judgement and append it to the ledger, or leave out an approval that
    /// would only repeat the one that stands: a run of one, as
    /// [`judge_all`](ShelfApp::judge_all) makes it.
    ///
    /// A judgement the model refuses (a document that is not valid, a link that
    /// cannot be followed, a cycle, an expiry that is not in the future, nothing to
    /// revoke, a successor that is none) writes nothing.
    pub async fn judge(&mut self, request: JudgeRequest) -> Result<JudgedAll, ShelfError> {
        self.judge_all(vec![request]).await
    }

    /// Make several judgements in order, on one reading of the project, and append
    /// them all, or none.
    ///
    /// Every record of the run takes one time, the time of the reading: the clock
    /// is read once, by the load, and never again in the run. So the records of one
    /// run share their time and their actor, which is how
    /// [`stats`](crate::domain::shelf::stats) tells a run.
    ///
    /// Each is decided on the ledger as the ones before it left it, so a run that
    /// approves a finding and then the spec that stands on it frames the spec on
    /// that approval. Every judgement is decided before anything is written: when one
    /// is refused, the error names it and nothing is written. (A port that fails
    /// half-way through the writes can still leave the first of them written, each
    /// whole and put on disk; the answer then says which were written
    /// ([`ShelfError::PartWay`]), and each is a record of its own, so what is there
    /// is sound.)
    ///
    /// Before any is decided, the run checks what it is asked: every reference
    /// resolves (a successor's too), and every approval's expiry is after now
    /// ([`DecisionProblem::ExpiresNotAfter`]). Then it asks the actor port once, for
    /// the requests that name no actor, whatever they would write: what an approval
    /// is depends on who makes it, so a run with no name fails even when it would
    /// leave out every document.
    ///
    /// An approval that would only repeat the reading of the one who approves
    /// ([`ApprovalKind::Repeat`]: by a reader of the content that stands, whose
    /// reading and the approval that stands have not lapsed and lapse when the new
    /// one would) is not made unless
    /// the request says `again`: it is left out with why ([`LeftOutWhy::Repeats`]),
    /// and no record is written for it. Another actor's approval of the same content
    /// is a second reading and is written. A document left out is no refusal: the
    /// others of the run are judged and written as before, and a run that leaves out
    /// every one writes nothing and fails nothing.
    ///
    /// [`ApprovalKind::Repeat`]: crate::domain::shelf::ledger::ApprovalKind::Repeat
    pub async fn judge_all(
        &mut self,
        requests: Vec<JudgeRequest>,
    ) -> Result<JudgedAll, ShelfError> {
        let mut loaded = load(&self.ports).await?;
        for request in &requests {
            checked(&loaded, request)?;
        }
        let mut port_actor: Option<Actor> = None;
        let mut actors = Vec::with_capacity(requests.len());
        for request in &requests {
            let actor = match (&request.actor, &port_actor) {
                (Some(actor), _) | (None, Some(actor)) => actor.clone(),
                (None, None) => {
                    let actor = self.port_actor().await?;
                    port_actor = Some(actor.clone());
                    actor
                }
            };
            actors.push(actor);
        }
        let mut judged = Vec::with_capacity(requests.len());
        let mut left_out = Vec::new();
        for (request, actor) in requests.into_iter().zip(actors) {
            match self.decide(&loaded, request, actor, Taking::Named).await? {
                Decided::Judged(one) => {
                    taken(&mut loaded, &one);
                    judged.push(one);
                }
                Decided::LeftOut(left) => left_out.push(left),
            }
        }
        self.write(&judged).await?;
        Ok(JudgedAll { judged, left_out })
    }

    /// Append the records of a run, every one decided, in order, and put them on disk
    /// once, at the end ([`RunWrites::run`]). The one write of [`judge_all`](Self::judge_all)
    /// and of `approve_chosen`.
    pub(super) async fn write<'a>(
        &mut self,
        judged: impl IntoIterator<Item = &'a Judged>,
    ) -> Result<(), ShelfError> {
        let records = judged
            .into_iter()
            .map(|one| &one.record)
            .collect::<Vec<_>>();
        RunWrites::run(&self.ports, |writes| {
            Box::pin(async move {
                for record in records {
                    writes.record(record).await?;
                }
                Ok(())
            })
        })
        .await
    }

    /// The actor the actor port names: asked once per run, before any judgement of it
    /// is decided.
    pub(super) async fn port_actor(&self) -> Result<Actor, ShelfError> {
        self.ports
            .actor_source()
            .actor()
            .await
            .map_err(port(ShelfPort::Actor))
    }

    /// One judgement by `actor`, decided on what is loaded, of a document the run
    /// took by `taking`; nothing is written. The request's own actor is not read:
    /// the run asked for it first.
    pub(super) async fn decide(
        &self,
        loaded: &Loaded,
        request: JudgeRequest,
        actor: Actor,
        taking: Taking,
    ) -> Result<Decided, ShelfError> {
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
            Judgement::Approve {
                until,
                reason,
                again,
            } => {
                // The frame first, so what the model refuses stays refused; then
                // what the approval would be and whether the run leaves it out, which
                // are the domain's.
                let frame = frame_approve(&loaded.shelf, &loaded.ledger, subject)?;
                let kind = loaded
                    .ledger
                    .kind_of(&Asked::frame(&frame, &actor, until, now), Before::Now);
                if let Some(leave) = kind.left_out(taking, again) {
                    return Ok(Decided::LeftOut(LeftOut {
                        doc: subject,
                        key,
                        why: LeftOutWhy::from(leave),
                    }));
                }
                (Plan::Approve(frame, until), reason)
            }
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

        let id = self
            .ports
            .ids()
            .new_judgement_id()
            .await
            .map_err(port(ShelfPort::Ids))?;

        let record = match plan {
            Plan::Approve(frame, until) => {
                let frame = self.addressed(loaded, frame).await?;
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

        // An approval pins a name of a depends link with nothing there as absent; say
        // so, since a wrong name is pinned in the same way as evidence not written yet.
        let absent = match &record {
            Record::Decision(decision)
                if matches!(decision.verb(), DecisionVerb::Approve { .. }) =>
            {
                loaded.shelf.doc(subject).map_or_else(Vec::new, |doc| {
                    doc.absent_artifacts()
                        .into_iter()
                        .map(|(link, artifact)| (link.clone(), artifact.clone()))
                        .collect()
                })
            }
            _ => Vec::new(),
        };
        Ok(Decided::Judged(Judged {
            record,
            subject,
            key,
            successors,
            absent,
        }))
    }

    /// The frame of an approval with the address of each document and artifact it
    /// stands on and the revision now (rule A5); which address goes on which item is
    /// the domain's ([`ApproveFrame::addressed`]).
    ///
    /// The address of an artifact is the one its adapter answered with its hash when
    /// the project was loaded, so the two are of one reading; the files of the
    /// documents are asked of the [`DocSource`] now. An adapter that can say no
    /// address (outside version control) leaves the record without one, and that is
    /// no error.
    async fn addressed(
        &self,
        loaded: &Loaded,
        frame: ApproveFrame,
    ) -> Result<ApproveFrame, ShelfError> {
        let addresses = self
            .ports
            .docs()
            .addresses(&frame.files())
            .await
            .map_err(port(ShelfPort::Docs))?;
        let revision = self
            .ports
            .revisions()
            .revision()
            .await
            .map_err(port(ShelfPort::Revision))?;
        Ok(frame.addressed(&addresses, &loaded.artifacts, revision))
    }
}
