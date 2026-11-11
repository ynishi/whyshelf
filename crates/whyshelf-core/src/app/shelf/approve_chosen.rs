//! The use case that approves, in one run and upstream first, the documents two rules
//! take: `judge approve --stale` (every approval that no longer holds) and
//! `judge approve --pending` (every pending document, or those named and the pending
//! ones they stand on), alone or together.
//!
//! ```text
//!   load ── --until after now? ── the references resolve, each pending? ── assess
//!     --stale:   stale ──► expired or stale ─────────┐   broken ──► left out (Broken)
//!     --pending: pending, or pending_upstream ───────┤
//!                                                    ▼
//!     upstream_first ──► one at a time:
//!        held_back by an upstream, as the run left it ──► left out (StandsOn)
//!        frame_approve refuses ─────────────────────────► left out (Refused)
//!        would repeat the approval that stands ─────────► left out (Repeats)
//!        otherwise decided, on the ledger the ones before it left
//!   every one decided ──► written, all together
//! ```
//!
//! Which documents are taken is the domain's, one rule each ([`stale`], [`pending`],
//! [`pending_upstream`]); this use case only joins the two sets. They are apart: a
//! stale document has an approval that stands, a pending one has none.
//!
//! Only what would hold once approved is approved. A broken approval is not: what it
//! stood on is gone, refused or cannot be trusted (an artifact pinned there that is
//! gone, an upstream that is not valid, a cycle). Nor is one whose frame the model
//! refuses now, nor one a `Depends` link of which names a document that is broken,
//! pending or stale and not in the run (approve it first), or itself left out for any
//! reason but a repeat ([`held_back`]). Nor is one stale only through its upstream's
//! upstream, whose hash is new, when the run approved its upstream again: what it
//! pinned is unchanged, so an approval now would repeat it ([`repeated`]), and it
//! holds again without one. Such a repeat stands, so it holds back nothing on it. Each
//! is left out with why, never passed over in silence. What neither rule takes is
//! not touched: `--stale` alone touches no pending document, and `--pending` alone
//! approves nothing again.
//!
//! The documents are taken upstream first ([`upstream_first`]), so every upstream of
//! the set is decided, or left out, before what stands on it, and one pass is the
//! whole closure: nothing left out later could have held back one decided earlier.
//! The run is decided whole before anything is written, as
//! [`judge_all`](super::ShelfApp::judge_all)'s is.
//!
//! Each document is decided on the ledger the earlier ones of the run left, and so
//! is what holds it back: after each approval decided, the ledger is built again
//! with it (as the frame and [`repeated`] read it) and assessed again, by the one
//! [`assess`](crate::domain::shelf::freshness::assess), so an upstream stale only
//! through a pending one the run approved holds nothing back. Which documents the
//! run takes is read once, from the assessment before the run. That costs one
//! assessment of the whole project per approval made.
//!
//! An artifact a link names with nothing there is pinned as absent, as any approval
//! pins it, and told in [`Judged::absent`]; what is never approved again is an
//! approval whose artifact was pinned there and is gone.
//!
//! [`stale`]: crate::domain::shelf::order::stale
//! [`pending`]: crate::domain::shelf::order::pending
//! [`pending_upstream`]: crate::domain::shelf::order::pending_upstream
//! [`upstream_first`]: crate::domain::shelf::order::upstream_first
//! [`held_back`]: crate::domain::shelf::order::held_back
//! [`repeated`]: crate::domain::shelf::ledger::repeated

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::shelf::ledger::{DecisionProblem, JudgedAt};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::order::{
    Rule, held_back, pending, pending_of, pending_upstream, stale, upstream_first,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::judge::{Decided, JudgeRequest, Judgement, taken};
use super::load::load;
use super::ports::ShelfWrite;
use super::read::identity;
use super::report::{Judged, LeftOut, LeftOutWhy, Names};

/// Which pending documents `--pending` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingScope {
    /// Every pending document.
    All,
    /// The documents named, each of which must be pending, and every pending
    /// document they stand on through `Depends` links, transitively.
    UpstreamOf(NonEmptyVec<Reference>),
}

/// Which rules take the documents of the run: at least one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choose {
    /// `--stale`: the approvals that are expired or stale.
    Stale,
    /// `--pending`: the pending documents of the scope.
    Pending(PendingScope),
    /// Both, in one run.
    StaleAndPending(PendingScope),
}

impl Choose {
    fn stale(&self) -> bool {
        matches!(self, Self::Stale | Self::StaleAndPending(_))
    }

    fn pending(&self) -> Option<&PendingScope> {
        match self {
            Self::Stale => None,
            Self::Pending(scope) | Self::StaleAndPending(scope) => Some(scope),
        }
    }
}

/// A request to approve, in one run, what the rules chosen take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproveChosenRequest {
    /// Which rules take the documents.
    pub choose: Choose,
    /// Why, for every approval made. Required.
    pub reason: NonBlank,
    /// Who judges. `None` is the person the actor port names.
    pub actor: Option<Actor>,
    /// When every approval made lapses, if they do: one expiry for the whole run. An
    /// expiry an approval had before is not carried over.
    pub until: Option<JudgedAt>,
}

/// A request to approve again every approval that is expired or stale: the run of
/// [`Choose::Stale`] alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproveStaleRequest {
    /// Why, for every approval made. Required.
    pub reason: NonBlank,
    /// Who judges. `None` is the person the actor port names.
    pub actor: Option<Actor>,
    /// When every approval made lapses, if they do: one expiry for the whole run. An
    /// expiry an approval had before is not carried over.
    pub until: Option<JudgedAt>,
}

/// One approval of the run, with the rule that took its document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenJudged {
    /// The rule.
    pub rule: Rule,
    /// The approval, as `judge` makes it.
    pub judged: Judged,
}

/// One document the run took and did not approve, with the rule that took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenLeftOut {
    /// The rule.
    pub rule: Rule,
    /// The document and why.
    pub left: LeftOut,
}

/// What a run of `judge approve --stale` or `--pending` wrote, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenApproved {
    /// The approvals made, upstream first; empty when nothing could be approved.
    pub judged: Vec<ChosenJudged>,
    /// The documents taken and not approved, by identity.
    pub left_out: Vec<ChosenLeftOut>,
    /// The keys of the documents, as the project was read.
    pub names: Names,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Approve again, in one run and upstream first, every approval that is expired or
    /// stale and would hold once approved again: [`approve_chosen`] with
    /// [`Choose::Stale`] alone, which touches no pending document.
    ///
    /// [`approve_chosen`]: ShelfApp::approve_chosen
    pub async fn approve_stale(
        &mut self,
        request: ApproveStaleRequest,
    ) -> Result<ChosenApproved, ShelfError> {
        self.approve_chosen(ApproveChosenRequest {
            choose: Choose::Stale,
            reason: request.reason,
            actor: request.actor,
            until: request.until,
        })
        .await
    }

    /// Approve, in one run and upstream first, every document the rules chosen take
    /// that would hold once approved; leave out the rest, each with why (the rule is
    /// on the module page).
    ///
    /// Every approval of the run takes one time, the time of the reading, as
    /// [`judge_all`](ShelfApp::judge_all)'s records do.
    ///
    /// `until` applies to every approval of the run, and must be after now: one that
    /// is not is refused before anything is selected, even when nothing is taken
    /// ([`DecisionProblem::ExpiresNotAfter`]). Then each reference of the
    /// [`PendingScope`] is resolved, and one that names nothing, or a document that is
    /// not pending ([`ShelfError::NotPending`]), fails the request. Nothing to approve
    /// is an empty run, which writes nothing. No refusal of a frame fails the run (the
    /// document is left out); a port that fails, or a decision that breaks an
    /// invariant, does, and then nothing is written.
    pub async fn approve_chosen(
        &mut self,
        request: ApproveChosenRequest,
    ) -> Result<ChosenApproved, ShelfError> {
        let mut loaded = load(&self.ports).await?;
        if let Some(expires) = request.until
            && expires <= loaded.now
        {
            return Err(DecisionProblem::ExpiresNotAfter {
                at: loaded.now,
                expires,
            }
            .into());
        }
        let assessment = loaded.assess();
        let names = loaded.names();
        let key_of = |id: DocId| names.key_of(id).cloned();

        let mut rules: BTreeMap<DocId, Rule> = BTreeMap::new();
        if let Some(scope) = request.choose.pending() {
            let chosen = match scope {
                PendingScope::All => pending(&loaded.shelf, &assessment),
                PendingScope::UpstreamOf(references) => {
                    let mut named = BTreeSet::new();
                    for reference in references.iter() {
                        let id = identity(&loaded, reference)?;
                        pending_of(&loaded.shelf, &assessment, id).map_err(|why| {
                            ShelfError::NotPending {
                                reference: reference.clone(),
                                subject: id,
                                why,
                            }
                        })?;
                        named.insert(id);
                    }
                    pending_upstream(&loaded.shelf, &assessment, &named)
                }
            };
            rules.extend(chosen.into_iter().map(|id| (id, Rule::Pending)));
        }
        let mut left_out: Vec<ChosenLeftOut> = Vec::new();
        if request.choose.stale() {
            let choice = stale(&assessment);
            rules.extend(choice.again.into_iter().map(|id| (id, Rule::Stale)));
            left_out.extend(
                choice
                    .broken
                    .into_iter()
                    .map(|(id, degradation)| ChosenLeftOut {
                        rule: Rule::Stale,
                        left: LeftOut {
                            doc: id,
                            key: key_of(id),
                            why: LeftOutWhy::Broken(degradation),
                        },
                    }),
            );
        }
        let run: BTreeSet<DocId> = rules.keys().copied().collect();
        // upstream_first orders exactly the members of `run`, the keys of `rules`, so
        // each comes with its rule.
        let order: Vec<(DocId, Rule)> = upstream_first(&loaded.shelf, &loaded.ledger, &run)
            .into_iter()
            .filter_map(|id| rules.get(&id).map(|rule| (id, *rule)))
            .collect();

        // Hold-back reads standings as the run has left them: the assessment is made
        // again of the ledger each approval decided leaves, the ledger the frame and
        // `repeated` already read.
        let mut now = assessment;
        let mut judged: Vec<ChosenJudged> = Vec::new();
        for (id, rule) in order {
            // An upstream left out because an approval would repeat its own stands,
            // and holds nothing back: what stands on it is decided as any other.
            let left: BTreeSet<DocId> = left_out
                .iter()
                .filter(|l| !matches!(l.left.why, LeftOutWhy::Repeats { .. }))
                .map(|l| l.left.doc)
                .collect();
            let leave = |why: LeftOutWhy| ChosenLeftOut {
                rule,
                left: LeftOut {
                    doc: id,
                    key: key_of(id),
                    why,
                },
            };
            if let Some(held) = held_back(&loaded.shelf, &now, id, &run, &left) {
                left_out.push(leave(LeftOutWhy::StandsOn {
                    key: key_of(held.upstream),
                    held,
                }));
                continue;
            }
            let decided = self
                .decide(
                    &loaded,
                    JudgeRequest {
                        judgement: Judgement::Approve {
                            until: request.until,
                            reason: Some(request.reason.clone()),
                            again: false,
                        },
                        reference: Reference::Id(id),
                        actor: request.actor.clone(),
                    },
                )
                .await;
            match decided {
                Ok(Decided::Judged(one)) => {
                    taken(&mut loaded, &one);
                    now = loaded.assess();
                    judged.push(ChosenJudged { rule, judged: one });
                }
                // A candidate stale only through its upstream's upstream, whose hash
                // is new: this run approved its upstream again (on the same hash),
                // so what its own approval pinned has not changed, one made now
                // would repeat it, and it holds again without one.
                Ok(Decided::LeftOut(left)) => left_out.push(ChosenLeftOut { rule, left }),
                Err(ShelfError::Refused(refusal)) => {
                    left_out.push(leave(LeftOutWhy::Refused(refusal)));
                }
                Err(other) => return Err(other),
            }
        }
        self.write(judged.iter().map(|one| &one.judged)).await?;
        left_out.sort_by_key(|l| l.left.doc);
        Ok(ChosenApproved {
            judged,
            left_out,
            names,
        })
    }
}
