//! The use case that approves again every approval that no longer holds:
//! `judge approve --stale`.
//!
//! ```text
//!   load ── --until after now? ── assess ──► the approvals that are Degraded
//!     broken ───────────────────────────────────────────► left out (Broken)
//!     expired or stale ── upstream_first ──► one at a time:
//!        held_back by an upstream ──────────────────────► left out (StandsOn)
//!        frame_approve refuses ─────────────────────────► left out (Refused)
//!        otherwise decided, on the ledger the ones before it left
//!   every one decided ──► written, all together
//! ```
//!
//! Only what would hold once approved again is approved. A broken approval is not:
//! what it stood on is gone, refused or cannot be trusted (an artifact pinned there
//! that is gone, an upstream that is not valid, a cycle). Nor is one whose frame the
//! model refuses now, nor one a `Depends` link of which names a document that is
//! broken, pending (never approved: approve it first with `judge approve`) or itself
//! left out ([`held_back`]). Each is left out with why, never passed over in
//! silence. A pending document is not touched.
//!
//! The documents are taken upstream first ([`upstream_first`]), so every upstream of
//! the set is decided, or left out, before what stands on it, and one pass is the
//! whole closure: nothing left out later could have held back one decided earlier.
//! The run is decided whole before anything is written, as
//! [`judge_all`](super::ShelfApp::judge_all)'s is.
//!
//! An artifact a link names with nothing there is pinned as absent, as any approval
//! pins it, and told in [`Judged::absent`]; what `--stale` never approves again is an
//! approval whose artifact was pinned there and is gone.
//!
//! [`upstream_first`]: crate::domain::shelf::order::upstream_first
//! [`held_back`]: crate::domain::shelf::order::held_back

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::freshness::{Degradation, Freshness, Severity, Standing};
use crate::domain::shelf::ledger::{DecisionProblem, FrameRefusal, JudgedAt};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::order::{HeldBack, held_back, upstream_first};

use super::ShelfApp;
use super::error::ShelfError;
use super::judge::{JudgeRequest, Judgement, taken};
use super::load::load;
use super::ports::ShelfWrite;
use super::report::{Judged, Names};

/// A request to approve again every approval that is expired or stale.
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

/// Why an approval that does not hold was not approved again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOutWhy {
    /// Its approval is broken now.
    Broken(Degradation),
    /// The model refuses its frame now (an upstream decided against, an artifact that
    /// cannot be told, a link that cannot be followed, a cycle it would close).
    Refused(FrameRefusal),
    /// A `Depends` link names an upstream that is broken, pending, or left out.
    StandsOn {
        /// The link, the upstream and why.
        held: HeldBack,
        /// The upstream's key, when it is valid.
        key: Option<DocKey>,
    },
}

/// An approval that does not hold and was not approved again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// The document.
    pub doc: DocId,
    /// Its key, when it is valid.
    pub key: Option<DocKey>,
    /// Why it was left out.
    pub why: LeftOutWhy,
}

/// What `judge approve --stale` wrote, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleApproved {
    /// The approvals made, upstream first; empty when nothing could be approved.
    pub judged: Vec<Judged>,
    /// The approvals left out, by identity.
    pub left_out: Vec<LeftOut>,
    /// The keys of the documents, as the project was read.
    pub names: Names,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Approve again, in one run and upstream first, every approval that is expired or
    /// stale and would hold once approved again; leave out the rest, each with why
    /// (the rule is on the module page).
    ///
    /// Every approval of the run takes one time, the time of the reading, as
    /// [`judge_all`](ShelfApp::judge_all)'s records do.
    ///
    /// `until` applies to every approval of the run, and must be after now: one that
    /// is not is refused before anything is selected, even when nothing is stale
    /// ([`DecisionProblem::ExpiresNotAfter`]). Nothing to approve is an empty run,
    /// which writes nothing. No refusal of a frame fails the run (the document is left
    /// out); a port that fails, or a decision that breaks an invariant, does, and then
    /// nothing is written.
    pub async fn approve_stale(
        &mut self,
        request: ApproveStaleRequest,
    ) -> Result<StaleApproved, ShelfError> {
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
        let mut again = BTreeSet::new();
        let mut left_out = Vec::new();
        for (id, standing) in assessment.standings() {
            let Standing::Approved(Freshness::Degraded(degradation)) = standing else {
                continue;
            };
            match degradation.level() {
                Severity::Expired | Severity::Stale => {
                    again.insert(*id);
                }
                Severity::Broken => left_out.push(LeftOut {
                    doc: *id,
                    key: key_of(*id),
                    why: LeftOutWhy::Broken(degradation.clone()),
                }),
            }
        }

        let mut judged = Vec::new();
        for id in upstream_first(&loaded.shelf, &loaded.ledger, &again) {
            let left: BTreeSet<DocId> = left_out.iter().map(|l| l.doc).collect();
            if let Some(held) = held_back(&loaded.shelf, &assessment, id, &left) {
                left_out.push(LeftOut {
                    doc: id,
                    key: key_of(id),
                    why: LeftOutWhy::StandsOn {
                        key: key_of(held.upstream),
                        held,
                    },
                });
                continue;
            }
            let decided = self
                .decide(
                    &loaded,
                    JudgeRequest {
                        judgement: Judgement::Approve {
                            until: request.until,
                            reason: Some(request.reason.clone()),
                        },
                        reference: Reference::Id(id),
                        actor: request.actor.clone(),
                    },
                )
                .await;
            match decided {
                Ok(one) => {
                    taken(&mut loaded, &one);
                    judged.push(one);
                }
                Err(ShelfError::Refused(refusal)) => left_out.push(LeftOut {
                    doc: id,
                    key: key_of(id),
                    why: LeftOutWhy::Refused(refusal),
                }),
                Err(other) => return Err(other),
            }
        }
        self.write(&judged).await?;
        left_out.sort_by_key(|l| l.doc);
        Ok(StaleApproved {
            judged,
            left_out,
            names,
        })
    }
}
