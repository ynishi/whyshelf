//! The judgements people made, as records, and what the records say.
//!
//! A [`Record`] is a [`Decision`] about one document (approve, deny, deprecate,
//! retire), a [`SameMeaning`] (a declaration that an edit of a document or a file
//! keeps the meaning; see its page), a [`Moved`] (a declaration that an artifact moved
//! from one name to another; see its page), an [`Exempt`] (a declaration that a
//! document is not expected to be reached by one covering link; see its page), or a
//! [`Revocation`] of decisions and declarations. The domain holds a record as a value
//! with no wire format: [`Record::to_canon`] and [`Record::from_canon`] are the two
//! ends of its canonical form, and an adapter turns that form into JSON and back.
//! [`Record::from_canon`] is strict (a version it does not know, a key too many or
//! too few, a verb it does not know, an identifier that is not a UUID v7, an
//! invariant that does not hold are all refused), so a record read from a file is as
//! sound as one made here.
//!
//! # From records to a status
//!
//! [`Ledger::build`] takes every record and sets aside what contradicts itself
//! (two records of one id that differ, a revocation of something that is not there,
//! of a revocation, or of a conflict), and what the rules of who may decide do not
//! admit ([`Admit`]), as [`LedgerProblem`]s. [`Ledger::status_of`]
//! then reads the *declared* status of a document from the latest decision about it
//! that is not revoked. The declared status says what people decided; whether the
//! decision still holds is [`freshness`](super::freshness)'s question.
//!
//! # Making a record
//!
//! A decision is made from a *frame*, the facts it stands on, which the shelf
//! supplies: [`frame_approve`], [`frame_present`] and [`frame_retire`] return an
//! [`ApproveFrame`], a [`PresentFrame`] or a [`RetireFrame`] (or a
//! [`FrameRefusal`]), and [`Decision::approve`] and its three siblings take no
//! other. So an approval cannot carry a `seen` hash nobody read, a retirement cannot
//! pretend to know the hash of a document that is gone, and a decision about a
//! document that is not valid is refused before any record exists. Every
//! constructor, and the reader of the canonical form, goes through one function,
//! `Decision::assemble`, visible to the ledger's modules only, which holds the
//! invariants of a decision.
//!
//! # The clock
//!
//! [`JudgedAt`] is unix milliseconds and has an order, which "the latest decision"
//! and an expiry need. It is a number of milliseconds, not a text. The domain has no
//! clock of its own: the caller passes in each time.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::id::DocId;
use crate::domain::nonempty::{Count, NonEmptyVec, TwoOrMore};

use super::artifact::{ArtifactHash, ArtifactKey};
use super::doc::DocHash;
use super::graph::{Cycle, CycleSource, components, cycles_of};

/// What the records of a ledger say about one document: the declared status.
///
/// It says what people decided, not whether it still holds: that is what
/// [`assess`](super::freshness::assess) adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredStatus {
    /// No decision stands about the document.
    Pending,
    /// The latest decision approves it.
    Approved {
        /// The decision.
        judgement: JudgementId,
        /// The hash that was approved.
        seen: DocHash,
        /// What the approval stands on.
        frame: BTreeSet<FramePin>,
        /// The artifacts the approval stands on.
        artifacts: BTreeSet<ArtifactPin>,
        /// When it lapses.
        expires: Option<JudgedAt>,
    },
    /// The latest decision denies it.
    Denied {
        /// The decision.
        judgement: JudgementId,
    },
    /// The latest decision deprecates it.
    Deprecated {
        /// The decision.
        judgement: JudgementId,
    },
    /// The latest decision retires it.
    Retired {
        /// The decision.
        judgement: JudgementId,
    },
}

/// A contradiction among the records, which the ledger sets aside.
// `Conflict` holds two or more whole records inline (the enum is as large as that), and
// the other variants hold two identifiers, or why one record is not admitted. A ledger
// has few problems, so the size costs little, and boxing the records would only add an
// allocation per conflict and a deref in each match.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LedgerProblem {
    /// Two or more different records with one id: none of them is used.
    #[error("{} different records have the id {id}", records.len())]
    Conflict {
        /// The id.
        id: JudgementId,
        /// The records, which differ.
        records: TwoOrMore<Record>,
    },
    /// A revocation of an id no record has.
    #[error("{revocation} revokes {target}, which is not in the ledger")]
    RevokesUnknown {
        /// The revocation.
        revocation: JudgementId,
        /// The id it names.
        target: JudgementId,
    },
    /// A revocation of a revocation.
    #[error("{revocation} revokes {target}, which is itself a revocation")]
    RevokesRevocation {
        /// The revocation.
        revocation: JudgementId,
        /// The id it names.
        target: JudgementId,
    },
    /// A revocation of an id whose records conflict.
    #[error("{revocation} revokes {target}, whose records conflict")]
    RevokesConflicted {
        /// The revocation.
        revocation: JudgementId,
        /// The id it names.
        target: JudgementId,
    },
    /// A record the rules of who may decide do not admit, which is not used; for a
    /// revocation, of one record it names, which it does not withdraw.
    #[error("{0}")]
    NotAdmitted(NotAdmitted),
}

/// The records grouped by identity: those that stand alone, and the identities whose
/// records conflict.
struct Grouped {
    decisions: BTreeMap<JudgementId, Decision>,
    revocations: BTreeMap<JudgementId, Revocation>,
    same_meanings: BTreeMap<JudgementId, SameMeaning>,
    moveds: BTreeMap<JudgementId, Moved>,
    exempts: BTreeMap<JudgementId, Exempt>,
    conflicted: BTreeSet<JudgementId>,
    /// The book each record but a revocation is read with by who may decide.
    books: BTreeMap<JudgementId, RecordBook>,
    /// The records but revocations the rules do not admit.
    not_admitted: BTreeMap<JudgementId, NotAdmitted>,
    /// The conflicts.
    problems: Vec<LedgerProblem>,
}

impl Grouped {
    /// The same record twice is one record; two different ones with one id conflict,
    /// and neither is kept. Each record that stands alone, but a revocation, is read
    /// by `admit` with its book.
    fn of(records: Vec<Record>, admit: &Admit) -> Self {
        let mut by_id: BTreeMap<JudgementId, BTreeMap<RecordHash, Record>> = BTreeMap::new();
        for record in records {
            by_id
                .entry(record.id())
                .or_default()
                .insert(record.hash(), record);
        }

        let mut grouped = Self {
            decisions: BTreeMap::new(),
            revocations: BTreeMap::new(),
            same_meanings: BTreeMap::new(),
            moveds: BTreeMap::new(),
            exempts: BTreeMap::new(),
            conflicted: BTreeSet::new(),
            books: BTreeMap::new(),
            not_admitted: BTreeMap::new(),
            problems: Vec::new(),
        };
        for (id, group) in by_id {
            let record = Count::of(group.into_values());
            // Unruled, every record is admitted whatever its book: nothing to read.
            if !admit.is_unruled()
                && let Count::One(record) = &record
                && !matches!(record, Record::Revocation(_))
            {
                let book = admit.book_of(record);
                if let Err(not) = admit.admission(record, None, &book) {
                    grouped.not_admitted.insert(id, not);
                }
                grouped.books.insert(id, book);
            }
            match record {
                Count::None => {}
                Count::One(Record::Decision(d)) => {
                    grouped.decisions.insert(id, d);
                }
                Count::One(Record::Revocation(r)) => {
                    grouped.revocations.insert(id, r);
                }
                Count::One(Record::SameMeaning(s)) => {
                    grouped.same_meanings.insert(id, s);
                }
                Count::One(Record::Moved(m)) => {
                    grouped.moveds.insert(id, m);
                }
                Count::One(Record::Exempt(e)) => {
                    grouped.exempts.insert(id, e);
                }
                Count::Many(records) => {
                    grouped.conflicted.insert(id);
                    grouped
                        .problems
                        .push(LedgerProblem::Conflict { id, records });
                }
            }
        }
        grouped
    }
}

/// The book the record `target` a revocation names is read with, from `books` (the
/// book of each record but a revocation): `[decide]` when it has none. Read by
/// [`revoked_by`] and by [`Ledger::accept`].
fn target_book(books: &BTreeMap<JudgementId, RecordBook>, target: JudgementId) -> RecordBook {
    books.get(&target).cloned().unwrap_or(RecordBook::Shelf)
}

/// The decisions and declarations the revocations withdraw. A revocation of a
/// conflict, of a revocation or of what is not there withdraws nothing for that
/// target and is reported in `problems`, and so does one `admit` does not admit to
/// withdraw that target, read with the target's book, while no revocation it admits
/// withdraws that target (once one does, nothing is left to put right).
fn revoked_by(
    grouped: &Grouped,
    admit: &Admit,
    problems: &mut Vec<LedgerProblem>,
) -> BTreeSet<JudgementId> {
    let Grouped {
        decisions,
        revocations,
        same_meanings,
        moveds,
        exempts,
        conflicted,
        books,
        ..
    } = grouped;
    let mut revoked = BTreeSet::new();
    let mut refused = Vec::new();
    for (id, revocation) in revocations {
        for target in revocation.targets.iter().copied() {
            if conflicted.contains(&target) {
                problems.push(LedgerProblem::RevokesConflicted {
                    revocation: *id,
                    target,
                });
            } else if revocations.contains_key(&target) {
                problems.push(LedgerProblem::RevokesRevocation {
                    revocation: *id,
                    target,
                });
            } else if decisions.contains_key(&target)
                || same_meanings.contains_key(&target)
                || moveds.contains_key(&target)
                || exempts.contains_key(&target)
            {
                match admit.revocation(revocation, target, &target_book(books, target)) {
                    Ok(()) => {
                        revoked.insert(target);
                    }
                    Err(not) => refused.push(not),
                }
            } else {
                problems.push(LedgerProblem::RevokesUnknown {
                    revocation: *id,
                    target,
                });
            }
        }
    }
    problems.extend(
        refused
            .into_iter()
            .filter(|not| not.target.is_none_or(|target| !revoked.contains(&target)))
            .map(LedgerProblem::NotAdmitted),
    );
    revoked
}

/// The records of a project, with their contradictions set aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    decisions: BTreeMap<JudgementId, Decision>,
    /// The declarations of the same meaning that are not revoked.
    same_meanings: BTreeMap<JudgementId, SameMeaning>,
    /// The declarations of moves that are not revoked.
    moveds: BTreeMap<JudgementId, Moved>,
    /// The exemptions that are not revoked.
    exempts: BTreeMap<JudgementId, Exempt>,
    revoked: BTreeSet<JudgementId>,
    subjects: BTreeSet<DocId>,
    problems: Vec<LedgerProblem>,
    /// What the records were admitted by, and what a record to be written is.
    admit: Admit,
    /// The book each record but a revocation is read with, admitted or not.
    books: BTreeMap<JudgementId, RecordBook>,
    /// The records the rules do not admit, which are not used.
    set_aside: BTreeSet<JudgementId>,
}

impl Ledger {
    /// A ledger from records, in any order and with repeats, read with who may
    /// decide as `admit` says.
    ///
    /// The same record twice is one record. Two different records with one id are a
    /// [`Conflict`](LedgerProblem::Conflict) and neither is used (as two worktrees
    /// that picked one id for two decisions have no reason to be believed over one
    /// another). A revocation of something that is not there, of a revocation, or of
    /// a conflict is set aside for that one target only. A revocation of decisions of
    /// several subjects, or of someone else's, is allowed. A record the rules do not
    /// admit is not used, and is a [`NotAdmitted`](LedgerProblem::NotAdmitted) until a
    /// revocation the rules admit withdraws it (the rules are on [`Admit`]'s module
    /// page); a project that declares no rule admits every record.
    pub fn build(records: Vec<Record>, admit: &Admit) -> Self {
        let mut grouped = Grouped::of(records, admit);
        let mut problems = std::mem::take(&mut grouped.problems);
        let revoked = revoked_by(&grouped, admit, &mut problems);
        let Grouped {
            mut decisions,
            mut same_meanings,
            mut moveds,
            mut exempts,
            books,
            not_admitted,
            ..
        } = grouped;
        let set_aside: BTreeSet<JudgementId> = not_admitted.keys().copied().collect();
        problems.extend(
            not_admitted
                .into_values()
                .filter(|not| !revoked.contains(&not.record))
                .map(LedgerProblem::NotAdmitted),
        );
        decisions.retain(|id, _| !set_aside.contains(id));
        same_meanings.retain(|id, _| !revoked.contains(id) && !set_aside.contains(id));
        moveds.retain(|id, _| !revoked.contains(id) && !set_aside.contains(id));
        exempts.retain(|id, _| !revoked.contains(id) && !set_aside.contains(id));
        let subjects = decisions.values().map(|d| d.subject()).collect();
        Self {
            decisions,
            same_meanings,
            moveds,
            exempts,
            revoked,
            subjects,
            problems,
            admit: admit.clone(),
            books,
            set_aside,
        }
    }

    /// The declarations, not revoked, that make the hashes `a` and `b` of the
    /// document `id` mean the same, in the order of the way from `a` to `b`; `None`
    /// when none do. The one place two hashes of a document are said to mean the same.
    pub fn doc_same_meaning(
        &self,
        id: DocId,
        a: &DocHash,
        b: &DocHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        let edges = self.same_meanings.values().flat_map(|s| match &s.of {
            SameMeaningOf::Doc { id: of, from, to } if *of == id => {
                from.iter().map(|f| (s.id, *f, *to)).collect::<Vec<_>>()
            }
            _ => Vec::new(),
        });
        declared_way(edges, *a, *b)
    }

    /// As [`Ledger::doc_same_meaning`], for an artifact.
    pub fn artifact_same_meaning(
        &self,
        artifact: &ArtifactKey,
        a: &ArtifactHash,
        b: &ArtifactHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        let edges = self.same_meanings.values().flat_map(|s| match &s.of {
            SameMeaningOf::Artifact {
                artifact: of,
                from,
                to,
            } if of == artifact => from.iter().map(|f| (s.id, *f, *to)).collect::<Vec<_>>(),
            _ => Vec::new(),
        });
        declared_way(edges, *a, *b)
    }

    /// As [`Ledger::artifact_same_meaning`], for a pin as it is read now: the
    /// declarations about each name of its way count (the name it wrote, and each
    /// name a move took it to), since what moved is one thing under each of them.
    pub fn pin_same_meaning(
        &self,
        pin: &ReadPin,
        a: &ArtifactHash,
        b: &ArtifactHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        let mut names: BTreeSet<&ArtifactKey> =
            BTreeSet::from([pin.pin().artifact(), pin.read_as()]);
        names.extend(pin.moved().into_iter().flat_map(MovedTo::moved_from));
        let edges = self.same_meanings.values().flat_map(|s| match &s.of {
            SameMeaningOf::Artifact {
                artifact: of,
                from,
                to,
            } if names.contains(of) => from.iter().map(|f| (s.id, *f, *to)).collect::<Vec<_>>(),
            _ => Vec::new(),
        });
        declared_way(edges, *a, *b)
    }

    /// A declaration that is not revoked, by its identity.
    pub fn same_meaning(&self, id: JudgementId) -> Option<&SameMeaning> {
        self.same_meanings.get(&id)
    }

    /// The way the name `artifact`, pinned by an approval made at `made`, is read
    /// now: through the declarations of moves, not revoked, of its space, made at or
    /// after `made`, in the order of time, each from the name the ones before it led
    /// to; of two moves of one name at one time, the larger identity, as for a
    /// decision. None when no move applies. The one place a pin is read through moves
    /// (see [`Moved`]).
    pub fn moved_to(&self, artifact: &ArtifactKey, made: JudgedAt) -> Option<MovedTo> {
        moved_way(self.moveds.values(), artifact, made)
    }

    /// A declaration of a move that is not revoked, by its identity.
    pub fn moved(&self, id: JudgementId) -> Option<&Moved> {
        self.moveds.get(&id)
    }

    /// The exemptions that are not revoked, by identity: the one place the
    /// exemptions that stand are read, by
    /// [`exemption`](super::exemption)'s acceptance and reading.
    pub fn exemptions(&self) -> impl Iterator<Item = &Exempt> {
        self.exempts.values()
    }

    /// An exemption that is not revoked, by its identity.
    pub fn exemption(&self, id: JudgementId) -> Option<&Exempt> {
        self.exempts.get(&id)
    }

    /// The artifact pins of the approval that stands about `id`, each as it is read
    /// now ([`Ledger::moved_to`], at the time of that approval), in the order of the
    /// record; none when no approval stands. A move is of something that was there,
    /// so a pin that pinned its name as absent is read as written. The one maker of
    /// a [`ReadPin`]: what
    /// freshness, a declaration and a snapshot compare with what is there now.
    pub fn read_pins(&self, id: DocId) -> Vec<ReadPin> {
        let Some(decision) = self.standing_decision(id) else {
            return Vec::new();
        };
        let DecisionVerb::Approve { artifacts, .. } = decision.verb() else {
            return Vec::new();
        };
        artifacts
            .iter()
            .map(|pin| ReadPin {
                pin: pin.clone(),
                // A move is of something that was there: a pin of a name with nothing
                // behind it held no content to move, and is read as written.
                moved: match pin.seen {
                    PinnedArtifact::Present { .. } => self.moved_to(&pin.artifact, decision.at()),
                    PinnedArtifact::Absent => None,
                },
            })
            .collect()
    }

    /// Whether the record `id` can be withdrawn: it is a decision or a declaration of
    /// this ledger, used or not admitted by the rules of who may decide (withdrawing
    /// one is how its problem is put right), and no revocation withdraws it yet. The
    /// one place a withdrawal by identity is checked; whether the one who withdraws
    /// may is [`Ledger::accept`]'s.
    pub fn check_withdraw(&self, id: JudgementId) -> Result<(), WithdrawRefusal> {
        if self.revoked.contains(&id) {
            Err(WithdrawRefusal::AlreadyWithdrawn { id })
        } else if self.decisions.contains_key(&id)
            || self.same_meanings.contains_key(&id)
            || self.moveds.contains_key(&id)
            || self.exempts.contains_key(&id)
            || self.set_aside.contains(&id)
        {
            Ok(())
        } else {
            Err(WithdrawRefusal::Unknown { id })
        }
    }

    /// The declared status of a document.
    ///
    /// The one place it is read: from the decisions about the document that are not
    /// revoked, the latest, by time and then by the larger id. A revocation is never
    /// a candidate, because only decisions are kept as such.
    pub fn status_of(&self, id: DocId) -> DeclaredStatus {
        match self.standing_decision(id) {
            None => DeclaredStatus::Pending,
            Some(d) => match d.verb() {
                DecisionVerb::Approve {
                    seen,
                    frame,
                    artifacts,
                    expires,
                    ..
                } => DeclaredStatus::Approved {
                    judgement: d.id(),
                    seen: *seen,
                    frame: frame.clone(),
                    artifacts: artifacts.clone(),
                    expires: *expires,
                },
                DecisionVerb::Deny { .. } => DeclaredStatus::Denied { judgement: d.id() },
                DecisionVerb::Deprecate { .. } => DeclaredStatus::Deprecated { judgement: d.id() },
                DecisionVerb::Retire { .. } => DeclaredStatus::Retired { judgement: d.id() },
            },
        }
    }

    /// The decision that stands about a document: the latest of those about it that
    /// are not revoked, by time and then by the larger id. The one place it is
    /// chosen: what [`Ledger::status_of`] says is read from it, and so is every
    /// other answer that names the decision about a document.
    pub fn standing_decision(&self, id: DocId) -> Option<&Decision> {
        self.decisions
            .values()
            .filter(|d| d.subject() == id && !self.revoked.contains(&d.id()))
            .max_by_key(|d| (d.at(), d.id()))
    }

    /// The documents some decision is about, revoked or not. A declaration of the
    /// same meaning about a document is not counted here, and a revocation names no
    /// document.
    pub fn subjects(&self) -> &BTreeSet<DocId> {
        &self.subjects
    }

    /// Every decision of the ledger, revoked or not, by identity; those of an
    /// identity whose records conflict, and those the rules of who may decide do not
    /// admit, are not among them.
    pub fn decisions(&self) -> impl Iterator<Item = &Decision> {
        self.decisions.values()
    }

    /// Whether a revocation withdraws the record `id`.
    pub fn is_revoked(&self, id: JudgementId) -> bool {
        self.revoked.contains(&id)
    }

    /// The contradictions that were set aside.
    pub fn problems(&self) -> &[LedgerProblem] {
        &self.problems
    }

    /// The graph of the frames of the approvals that stand.
    ///
    /// The one place it is made: freshness reads its order and its cycles from it,
    /// and [`frame_approve`] asks it whether a new approval would close a cycle.
    pub(super) fn frame_graph(&self) -> FrameGraph {
        let mut edges: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
        for subject in &self.subjects {
            if let DeclaredStatus::Approved { frame, .. } = self.status_of(*subject) {
                edges
                    .entry(*subject)
                    .or_default()
                    .extend(frame.iter().map(FramePin::target));
            }
        }
        FrameGraph { edges }
    }
}

/// The graph of the frames of approved documents: an edge runs from an approved
/// document to each upstream its approval pinned.
///
/// This is the graph freshness travels, not the links the shelf holds now: an
/// approval stands on what it saw, and a link rewritten since is a change to tell
/// of, not a path to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FrameGraph {
    edges: BTreeMap<DocId, BTreeSet<DocId>>,
}

impl FrameGraph {
    /// The cycles among the frames, as [`CycleSource::Frames`].
    pub(super) fn cycles(&self) -> Vec<Cycle> {
        cycles_of(CycleSource::Frames, &self.edges)
    }

    /// The cycle that replacing the edges of `subject` with edges to `targets` would
    /// make, when it would make one that `subject` is in.
    ///
    /// An existing cycle that does not include `subject` is not its concern.
    pub(super) fn would_cycle(&self, subject: DocId, targets: &BTreeSet<DocId>) -> Option<Cycle> {
        let mut edges = self.edges.clone();
        edges.insert(subject, targets.clone());
        cycles_of(CycleSource::Frames, &edges)
            .into_iter()
            .find(|cycle| cycle.contains(subject))
    }

    /// The documents of the graph taken in components, upstream first: a component
    /// comes after every component it points at.
    pub(super) fn components(&self) -> Vec<Vec<DocId>> {
        components(&self.edges)
    }
}

/// Why a record cannot be withdrawn by its identity.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WithdrawRefusal {
    /// No decision or declaration of the ledger has the identity (a revocation, a
    /// record in conflict, or none at all).
    #[error("{id} is no decision or declaration of the ledger")]
    Unknown {
        /// The identity.
        id: JudgementId,
    },
    /// A revocation already withdraws it.
    #[error("{id} is already withdrawn")]
    AlreadyWithdrawn {
        /// The identity.
        id: JudgementId,
    },
}

mod admit;
mod canon;
mod decision;
mod exempt;
mod frame;
mod judgement;
mod moved;
mod pin;
mod readers;
mod record;
mod same_meaning;

pub use exempt::{Exempt, ExemptOf};
use moved::moved_way;
pub use moved::{
    FoundBy, GoneName, GoneNames, MoveCandidate, MoveNote, MoveSuggestion, MoveSuggestions, Moved,
    MovedOf, MovedProblem, MovedRefusal, MovedTo, NameSuggestions, NamedRefusal, NamedWhy,
    NoteKind, NothingWhy, PinnedNames, PinnedThere, PrefixPlan, PrefixRefusal, ReadPin, Similarity,
    SpaceSuggestions, frame_moved, frame_prefix_moves, moved_space, plan_prefix_moves,
};
use same_meaning::declared_way;
pub use same_meaning::{
    SameMeaning, SameMeaningOf, SameMeaningProblem, SameMeaningRefusal, SameMeaningTarget,
    frame_same_meaning,
};

pub use admit::{Admit, AdmittedRecord, NotAdmitted, RecordBook};
pub use canon::RecordProblem;
pub use decision::{Decision, DecisionProblem, DecisionVerb};
pub use frame::{
    ApproveFrame, FrameRefusal, NotStanding, PresentFrame, RetireFrame, frame_approve,
    frame_present, frame_retire,
};
pub use judgement::{JudgedAt, JudgementId};
pub use pin::{ArtifactPin, FramePin, PATH_ONLY_SPACE, PinnedArtifact, Seen};
use pin::{NameForm, StandsOn};
pub use readers::{ApprovalKind, Asked, Before, LeaveOut, Readers, Taking};
pub use record::{Record, RecordHash, Revocation};

#[cfg(test)]
use super::artifact::{ArtifactReadings, Comparison, ComparisonProblem, Space, Untold};
#[cfg(test)]
use super::build::{DocFailure, FailureTag, Shelf};
#[cfg(test)]
use super::schema::{BookName, EntryName};
#[cfg(test)]
use crate::domain::hash::{ContentHash, HashKind};
#[cfg(test)]
use crate::domain::location::RelPath;
#[cfg(test)]
use crate::domain::nonempty::{NonBlank, NonEmptySet};

#[cfg(test)]
mod admit_tests;
#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod exempt_tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod moved_prefix_tests;
#[cfg(test)]
mod moved_tests;
#[cfg(test)]
mod readers_tests;
#[cfg(test)]
mod revocation_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod tests;
