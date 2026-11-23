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
//! admit ([`Admit`]), as [`LedgerProblem`]s. [`Ledger::declared`]
//! then reads what the records say of a document from the latest decision about it
//! that is not revoked, an approval as a view of its record tied to the ledger
//! ([`ApprovalView`]), and [`Ledger::status_of`] is its owned copy, the *declared*
//! status. The declared status says what people decided; whether the decision still
//! holds is [`freshness`](super::freshness)'s question.
//!
//! ```text
//!   records ──► RecordOrder (time, then identity) ──► the latest decision of a document
//!                                                        │ ApprovalView::of: the one
//!                                                        │ reading of an approval's record
//!   Ledger::declared ──► DeclaredNow: Pending | Approved(StandingApproval) | Against
//!                          │                     │ reading, quorum: its readers
//!                          │                     │ what its pins come to now
//!                          │                     └ view(): ApprovalView ──► read_pins
//!                          └ Ledger::status_of: the owned copy, DeclaredStatus
//! ```
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

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::id::DocId;
use crate::domain::nonempty::{Count, NonEmptyVec, TwoOrMore};

use super::artifact::{ArtifactHash, ArtifactKey, ContentAddress, Revision};
use super::build::Shelf;
use super::doc::DocHash;
use super::graph::{Cycle, CycleSource, components, cycles_of};

/// What the records of a ledger say about one document: the declared status, an owned
/// copy of what [`Ledger::declared`] says.
///
/// It says what people decided, not whether it still holds: that is what
/// [`assess`](super::freshness::assess) adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredStatus {
    /// No decision stands about the document.
    Pending,
    /// The latest decision approves it.
    Approved(Approval),
    /// The latest decision is against it: it denies, deprecates or retires it.
    ///
    /// The one place a record is read as going against a document: the verb of the
    /// decision is read once, by the view of an approval ([`ApprovalView`]), and what
    /// each reader does with a document decided against is read from here, not decided
    /// again.
    Against {
        /// The decision.
        judgement: JudgementId,
        /// Which of the decisions against it.
        status: NotStanding,
    },
}

/// A decision against a document: it denies, deprecates or retires it. Nothing can
/// be approved on such a document, and it counts for nothing: no covering link
/// expects it, and none of what it writes reaches.
///
/// The one statement of which decisions go against a document: a decision of another
/// kind does not, and a new kind of decision against one is a variant here (and of
/// [`DecisionVerb`]), which every match over it then has to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NotStanding {
    /// Refused.
    #[error("denied")]
    Denied,
    /// To be replaced.
    #[error("deprecated")]
    Deprecated,
    /// No longer kept.
    #[error("retired")]
    Retired,
}

/// The approval that stands about a document, as an owned copy of its view
/// ([`ApprovalView`]): what a reader that keeps the declared status reads of it, and no
/// more.
///
/// It is a value of the record alone: what the approval's pins are read as now, and who
/// ran the command, are not in it, so two copies are equal when the approval is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    judgement: JudgementId,
    frame: BTreeSet<FramePin>,
    expires: Option<JudgedAt>,
}

impl Approval {
    /// The decision.
    pub fn judgement(&self) -> JudgementId {
        self.judgement
    }

    /// The upstreams the approval stands on.
    pub fn frame(&self) -> &BTreeSet<FramePin> {
        &self.frame
    }

    /// When it lapses.
    pub fn expires(&self) -> Option<JudgedAt> {
        self.expires
    }
}

/// An approval of the ledger `'l`, read from its record: what it stands on, when it
/// lapses and who made it, tied to the ledger it is read in, so that what its pins are
/// read as now is that ledger's.
///
/// What an approval is, is read from its record once, by the view: the one match of the
/// verb of a decision for what it stands on, lapses at and was made by, for every reader
/// of the ledger. The approval that stands ([`StandingApproval`]), each of the readers
/// of a content ([`Readers::approvals`]), an approval stats sets beside an earlier one,
/// and a recorded approval asked about, are each one. A decision of another verb is no
/// approval: it goes against its document ([`NotStanding`]).
///
/// Nothing of it takes a ledger, so it cannot be read with another, and a view of a
/// ledger dropped or replaced cannot be kept. It has no equality: two views of one
/// record in two ledgers may read its pins differently. Its `Debug` shows the record.
#[derive(Clone, Copy)]
pub struct ApprovalView<'l> {
    ledger: &'l Ledger,
    decision: &'l Decision,
    seen: &'l DocHash,
    seen_address: Option<&'l ContentAddress>,
    frame: &'l BTreeSet<FramePin>,
    artifacts: &'l BTreeSet<ArtifactPin>,
    expires: Option<JudgedAt>,
    revision: Option<&'l Revision>,
}

impl std::fmt::Debug for ApprovalView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalView")
            .field("decision", self.decision)
            .finish_non_exhaustive()
    }
}

impl<'l> ApprovalView<'l> {
    /// The view of `decision`, of `ledger`, when it approves; else which decision
    /// against its document it is. The one match of the verb of a decision for what an
    /// approval is: a new verb is an arm here, and a new decision against a document a
    /// variant of [`NotStanding`] too.
    fn of(ledger: &'l Ledger, decision: &'l Decision) -> Result<Self, NotStanding> {
        match decision.verb() {
            DecisionVerb::Approve {
                seen,
                frame,
                artifacts,
                expires,
                seen_address,
                revision,
            } => Ok(Self {
                ledger,
                decision,
                seen,
                seen_address: seen_address.as_ref(),
                frame,
                artifacts,
                expires: *expires,
                revision: revision.as_ref(),
            }),
            DecisionVerb::Deny { .. } => Err(NotStanding::Denied),
            DecisionVerb::Deprecate { .. } => Err(NotStanding::Deprecated),
            DecisionVerb::Retire { .. } => Err(NotStanding::Retired),
        }
    }

    /// The approved document.
    pub fn subject(&self) -> DocId {
        self.decision.subject()
    }

    /// The decision.
    pub fn judgement(&self) -> JudgementId {
        self.decision.id()
    }

    /// When it was made.
    pub fn at(&self) -> JudgedAt {
        self.decision.at()
    }

    /// Where it is in the order of the records.
    pub(in crate::domain::shelf) fn order(&self) -> RecordOrder {
        self.decision.order()
    }

    /// Who approved.
    pub fn actor(&self) -> &'l Actor {
        self.decision.actor()
    }

    /// Who approved, and who ran the command when it was another. No judgement reads
    /// who ran it.
    pub fn attribution(&self) -> Attribution {
        self.decision.attribution().clone()
    }

    /// The hash that was approved.
    pub fn seen(&self) -> &'l DocHash {
        self.seen
    }

    /// Where the content of the approved document's file, as it was approved, can be
    /// read back.
    pub fn seen_address(&self) -> Option<&'l ContentAddress> {
        self.seen_address
    }

    /// The upstreams it stands on.
    pub fn frame(&self) -> &'l BTreeSet<FramePin> {
        self.frame
    }

    /// The artifacts it stands on, as the record keeps them.
    pub fn artifacts(&self) -> &'l BTreeSet<ArtifactPin> {
        self.artifacts
    }

    /// When it lapses.
    pub fn expires(&self) -> Option<JudgedAt> {
        self.expires
    }

    /// The point of the history it was made at, when the record keeps one.
    pub fn revision(&self) -> Option<&'l Revision> {
        self.revision
    }

    /// Its artifact pins, each as it is read now, in the order of the record: a pin of
    /// something that was there through the moves declared at or after the approval
    /// was made ([`Ledger::moved_to`]), a pin of a name as absent as written, since a
    /// move is of something that was there. Derived from the ledger each time it is
    /// asked, never kept. The one rule of what a pin is read as now, and the one maker
    /// of a [`ReadPin`].
    pub fn read_pins(&self) -> Vec<ReadPin> {
        self.ledger
            .read_pins_under(self.artifacts, self.at(), Before::Now)
    }

    /// What it stands on, as two approvals are compared.
    fn stands_on(&self) -> StandsOn<'l> {
        StandsOn::of(self.seen, self.frame, self.artifacts)
    }

    /// Whether a pin of it is read through a move the cut `before` keeps.
    fn read_through_a_move(&self, before: Before) -> bool {
        self.ledger
            .read_pins_under(self.artifacts, self.at(), before)
            .iter()
            .any(|read| read.moved().is_some())
    }

    /// The owned copy of what a declared status keeps of it.
    fn to_approval(self) -> Approval {
        Approval {
            judgement: self.judgement(),
            frame: self.frame.clone(),
            expires: self.expires,
        }
    }
}

/// The approval that stands about a document ([`Ledger::declared`]): its view, and the
/// questions only the approval that stands answers, who reads its content now and
/// whether enough do ([`StandingApproval::reading`], [`StandingApproval::quorum`]),
/// and what its pins come to now.
///
/// A reader of a content that is not the one that stands, and an approval before a
/// cut, are views ([`ApprovalView`]) and answer none of these. A standing approval is
/// tied to its ledger as its view is, so it is not kept past it:
///
/// ```compile_fail
/// use whyshelf_core::domain::id::DocId;
/// use whyshelf_core::domain::shelf::ledger::{DeclaredNow, JudgedAt, Ledger};
///
/// fn f(ledger: Ledger, id: DocId, now: JudgedAt) {
///     let standing = match ledger.declared(id) {
///         DeclaredNow::Approved(s) => s,
///         _ => return,
///     };
///     drop(ledger);
///     let _ = standing.reading(now);
/// }
/// ```
///
/// The same without the `drop` is read:
///
/// ```
/// use whyshelf_core::domain::id::DocId;
/// use whyshelf_core::domain::shelf::ledger::{DeclaredNow, JudgedAt, Ledger};
///
/// fn f(ledger: Ledger, id: DocId, now: JudgedAt) {
///     let standing = match ledger.declared(id) {
///         DeclaredNow::Approved(s) => s,
///         _ => return,
///     };
///     let _ = standing.reading(now);
/// }
/// ```
///
/// A reader of a content is a view, which asks no reading:
///
/// ```compile_fail
/// use whyshelf_core::domain::shelf::ledger::{JudgedAt, Readers};
///
/// fn g(readers: Readers<'_>, now: JudgedAt) {
///     let _ = readers.approvals()[0].reading(now);
/// }
/// ```
///
/// while what a view reads is read:
///
/// ```
/// use whyshelf_core::domain::shelf::ledger::{JudgedAt, Readers};
///
/// fn g(readers: Readers<'_>, _now: JudgedAt) {
///     let _ = readers.approvals()[0].read_pins();
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct StandingApproval<'l>(ApprovalView<'l>);

impl<'l> StandingApproval<'l> {
    /// Its view: what it stands on, when it lapses and who made it.
    pub fn view(&self) -> ApprovalView<'l> {
        self.0
    }
}

/// What the records of a ledger say about one document now, tied to the ledger
/// ([`Ledger::declared`]): pending, the approval that stands, or the decision against it.
/// [`DeclaredStatus`] is its owned copy.
#[derive(Debug, Clone, Copy)]
pub enum DeclaredNow<'l> {
    /// No decision stands about the document.
    Pending,
    /// The latest decision approves it.
    Approved(StandingApproval<'l>),
    /// The latest decision is against it ([`DeclaredStatus::Against`]).
    Against {
        /// The decision.
        judgement: JudgementId,
        /// Which of the decisions against it.
        status: NotStanding,
    },
}

impl DeclaredNow<'_> {
    /// The owned copy: each variant as it is, the approval as what a declared status
    /// keeps of it ([`Approval`]).
    pub(in crate::domain::shelf) fn to_status(self) -> DeclaredStatus {
        match self {
            Self::Pending => DeclaredStatus::Pending,
            Self::Approved(standing) => DeclaredStatus::Approved(standing.view().to_approval()),
            Self::Against { judgement, status } => DeclaredStatus::Against { judgement, status },
        }
    }
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

/// The names of a pin's way, as it is read now: one thing under each of them. Made only
/// by [`way_names`] (and, in tests, `WayNames::unmoved`), so that what is declared of
/// a pin's artifact is read under the names of one pin's way, never of several pins
/// together.
struct WayNames<'p>(BTreeSet<&'p ArtifactKey>);

impl<'p> WayNames<'p> {
    /// Whether `name` is one of them.
    fn contains(&self, name: &ArtifactKey) -> bool {
        self.0.contains(name)
    }

    /// The way of a name no move took: the name alone.
    #[cfg(test)]
    fn unmoved(name: &'p ArtifactKey) -> Self {
        Self(BTreeSet::from([name]))
    }
}

/// The names of the way of `pin`, as it is read now: the name it wrote, the name it is
/// read as, and each name a move took it from. The one place they are gathered, for
/// what is declared of the pin's artifact ([`Ledger::pin_joins`]).
fn way_names(pin: &ReadPin) -> WayNames<'_> {
    let mut names = BTreeSet::from([pin.pin().artifact(), pin.read_as()]);
    names.extend(pin.moved().into_iter().flat_map(MovedTo::moved_from));
    WayNames(names)
}

/// The declarations of the ledger `'l` that are read for one content, each with one
/// earlier hash and its hash then: of a document ([`DocJoins`], [`Ledger::doc_joins`]),
/// or of the names of one pin's way ([`ArtifactJoins`], [`Ledger::pin_joins`]). What
/// joins two hashes, which edges are short of names, and which edges a name declared
/// are read from it alone, so the three are read from the same declarations.
struct Joins<'l, H> {
    ledger: &'l Ledger,
    declared: Vec<(&'l SameMeaning, H, H)>,
}

/// The declarations read for a document.
type DocJoins<'l> = Joins<'l, DocHash>;

/// The declarations read for an artifact under the names of one pin's way.
type ArtifactJoins<'l> = Joins<'l, ArtifactHash>;

impl<H: std::fmt::Debug> std::fmt::Debug for Joins<'_, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joins")
            .field("declared", &self.declared)
            .finish_non_exhaustive()
    }
}

impl<H: Ord + Copy> Joins<'_, H> {
    /// Whether the declarations join `a` to `b`, and by which ([`Ledger::joined`]).
    fn joins(&self, a: &H, b: &H) -> Option<NonEmptyVec<JudgementId>> {
        self.ledger.joined(self.declared.iter().copied(), *a, *b)
    }

    /// The edges of the declarations that `by` declared.
    fn declared_by(&self, by: &Actor) -> BTreeSet<(H, H)> {
        same_meaning::declared_by(self.declared.iter().copied(), by)
    }

    /// The edges short of names that a pin of `from` meets, each told by `of`.
    fn short_from(
        &self,
        from: H,
        of: impl Fn(&SameMeaning, H, H) -> SameMeaningEdge,
    ) -> Vec<ShortSameMeaning> {
        same_meaning::short_from(
            &self.ledger.counted_edges(self.declared.iter().copied()),
            from,
            of,
        )
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
    /// An edit is joined only by as many different names as its declarations need
    /// ([`SameMeaning`]'s page has the rule).
    pub fn doc_same_meaning(
        &self,
        id: DocId,
        a: &DocHash,
        b: &DocHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        self.doc_joins(id).joins(a, b)
    }

    /// As [`Ledger::doc_same_meaning`], for a pin of an artifact as it is read now: the
    /// declarations about each name of its way count (the name it wrote, and each name
    /// a move took it to), since what moved is one thing under each of them.
    pub(in crate::domain::shelf::ledger) fn pin_same_meaning(
        &self,
        pin: &ReadPin,
        a: &ArtifactHash,
        b: &ArtifactHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        self.pin_joins(pin).joins(a, b)
    }

    /// The declarations read for the document `id`.
    fn doc_joins(&self, id: DocId) -> DocJoins<'_> {
        Joins {
            ledger: self,
            declared: self.doc_declared(id),
        }
    }

    /// The declarations read for an artifact under the names `ways` of one pin's way.
    fn artifact_joins(&self, ways: &WayNames<'_>) -> ArtifactJoins<'_> {
        Joins {
            ledger: self,
            declared: self.artifact_declared(|of| ways.contains(of)),
        }
    }

    /// The declarations read for the artifact pin `read`: those about each name of its
    /// way ([`way_names`]). The one selection for a pin, which freshness reads through
    /// [`Ledger::pin_same_meaning`], and which what `judge same` offers, its second
    /// step and the edges short of names read too.
    fn pin_joins(&self, read: &ReadPin) -> ArtifactJoins<'_> {
        self.artifact_joins(&way_names(read))
    }

    /// The edits declared to keep the meaning by fewer different names than they
    /// need, which join nothing until more names declare them, that a pin that stale
    /// reads now meets: the edges among the declarations freshness reads for that pin
    /// (of its document, or of every name of its artifact's way, as freshness reads
    /// each pin along its own moves) of which one end is the hash pinned, or is joined
    /// to it by edges that count. A pin stale reads now is one of an approval whose
    /// document is valid in `shelf`; for an artifact, one that pinned a hash, under a
    /// link that still writes the name it is read as and declares the comparison it was
    /// pinned with. Those of the approvals by the identity of their document, each
    /// pin's in the order of the record (the document's own hash first, then its
    /// upstreams, then its artifacts), each edge once. An edge no such pin meets is not
    /// told, and none is in a project that declares no rule, where every declaration
    /// needs one name.
    pub fn same_meaning_short(&self, shelf: &Shelf) -> Vec<ShortSameMeaning> {
        let mut out: Vec<ShortSameMeaning> = Vec::new();
        if self.admit.is_unruled() {
            return out;
        }
        let mut add = |found: Vec<ShortSameMeaning>| {
            for short in found {
                if !out.contains(&short) {
                    out.push(short);
                }
            }
        };
        let pins = PinsNow::of(self, shelf);
        let now = ComparedNow::of(&pins);
        for then in now.thens() {
            match then {
                Then::Doc(id, hash) => add(self
                    .doc_joins(id)
                    .short_from(hash, |_, a, b| SameMeaningEdge::Doc { id, a, b })),
                Then::Artifact(read, hash) => {
                    add(self.pin_joins(read).short_from(hash, |first, a, b| {
                        SameMeaningEdge::Artifact {
                            artifact: match &first.of {
                                SameMeaningOf::Artifact { artifact, .. } => artifact.clone(),
                                SameMeaningOf::Doc { .. } => read.read_as().clone(),
                            },
                            a,
                            b,
                        }
                    }));
                }
            }
        }
        out
    }

    /// The declarations about the document `id`, each with one earlier hash and its
    /// hash then.
    fn doc_declared(&self, id: DocId) -> Vec<(&SameMeaning, DocHash, DocHash)> {
        self.same_meanings
            .values()
            .flat_map(|s| match &s.of {
                SameMeaningOf::Doc { id: of, from, to } if *of == id => {
                    from.iter().map(|f| (s, *f, *to)).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    /// The declarations about the artifacts `of` takes, each with one earlier hash and
    /// its hash then.
    fn artifact_declared(
        &self,
        of: impl Fn(&ArtifactKey) -> bool,
    ) -> Vec<(&SameMeaning, ArtifactHash, ArtifactHash)> {
        self.same_meanings
            .values()
            .flat_map(|s| match &s.of {
                SameMeaningOf::Artifact { artifact, from, to } if of(artifact) => {
                    from.iter().map(|f| (s, *f, *to)).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    /// Whether `declared` joins `a` to `b`, and by which declarations. A ledger no rule
    /// reads takes each declaration as an edge of its own, as every one needs one
    /// name, and counts nothing.
    fn joined<'a, H: Ord + Copy>(
        &self,
        declared: impl IntoIterator<Item = (&'a SameMeaning, H, H)>,
        a: H,
        b: H,
    ) -> Option<NonEmptyVec<JudgementId>> {
        if self.admit.is_unruled() {
            return same_meaning::each_way(declared, a, b);
        }
        same_meaning::way(&self.counted_edges(declared), a, b)
    }

    /// The edges `declared` makes, each declaration with its need.
    fn counted_edges<'a, H: Ord + Copy>(
        &self,
        declared: impl IntoIterator<Item = (&'a SameMeaning, H, H)>,
    ) -> Vec<same_meaning::Edge<'a, H>> {
        same_meaning::edges(
            declared
                .into_iter()
                .map(|(s, from, to)| (s, self.need(s.id, s.at), from, to)),
        )
    }

    /// A declaration that is not revoked, by its identity.
    pub fn same_meaning(&self, id: JudgementId) -> Option<&SameMeaning> {
        self.same_meanings.get(&id)
    }

    /// The way the name `artifact`, pinned by an approval made at `made`, is read
    /// now: through the declarations of moves, not revoked, of its space, made at or
    /// after `made`, in the order of time, each from the name the ones before it led
    /// to; of two moves of one name at one time, the larger identity, as for a
    /// decision. None when no move applies. The way one name is followed through the
    /// moves (see [`Moved`]); which moves are read, and which pins, is the rule of the
    /// view of an approval ([`ApprovalView::read_pins`]).
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

    /// The pins `artifacts` of an approval made at `at`, each as it is read now: a pin
    /// of something that was there, through the moves the cut `before` keeps (in the
    /// order of the records) that reach it (by time, [`moved_way`]); a pin of a name as
    /// absent as written. The one rule of a pin read through moves, which the view of
    /// an approval reads.
    fn read_pins_under(
        &self,
        artifacts: &BTreeSet<ArtifactPin>,
        at: JudgedAt,
        before: Before,
    ) -> Vec<ReadPin> {
        artifacts
            .iter()
            .map(|pin| ReadPin {
                pin: pin.clone(),
                // A move is of something that was there: a pin of a name with nothing
                // behind it held no content to move, and is read as written.
                moved: match pin.seen {
                    PinnedArtifact::Present { .. } => moved_way(
                        self.moveds.values().filter(|m| before.keeps_move(m)),
                        &pin.artifact,
                        at,
                    ),
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

    /// What the records say about the document `id` now, tied to this ledger: pending
    /// when no decision stands about it, else what the decision that stands is
    /// ([`Ledger::standing_decision`]): the approval that stands, read from its record
    /// once ([`ApprovalView`]), or the decision against it.
    ///
    /// The one place it is read. Who reads the approval that stands, whether enough
    /// do, and what its pins come to now are asked of it ([`StandingApproval`]).
    pub fn declared(&self, id: DocId) -> DeclaredNow<'_> {
        match self.standing_decision(id) {
            None => DeclaredNow::Pending,
            Some(decision) => match ApprovalView::of(self, decision) {
                Ok(view) => DeclaredNow::Approved(StandingApproval(view)),
                Err(status) => DeclaredNow::Against {
                    judgement: decision.id(),
                    status,
                },
            },
        }
    }

    /// The declared status of a document: an owned copy of [`Ledger::declared`].
    pub fn status_of(&self, id: DocId) -> DeclaredStatus {
        self.declared(id).to_status()
    }

    /// The decision that stands about a document: the latest of those about it that
    /// are not revoked, by time and then by the larger id (the order of the records). A
    /// revocation is never a candidate, because only decisions are kept as such. The
    /// one place it is chosen: what [`Ledger::declared`] says is read from it, and so
    /// is every other answer that names the decision about a document.
    pub fn standing_decision(&self, id: DocId) -> Option<&Decision> {
        // The first of `decisions_latest_first`, taken without sorting the rest.
        self.decisions_of(id, Before::Now).max_by_key(|d| d.order())
    }

    /// The decisions about `id` that are not revoked and that `before` keeps, the
    /// latest first in the order of the records: the one list the decision that stands
    /// and the readers of a content are read from.
    fn decisions_latest_first(&self, id: DocId, before: Before) -> impl Iterator<Item = &Decision> {
        let mut decisions: Vec<&Decision> = self.decisions_of(id, before).collect();
        decisions.sort_by_key(|d| Reverse(d.order()));
        decisions.into_iter()
    }

    /// The decisions about `id` that are not revoked and that `before` keeps, by
    /// identity.
    fn decisions_of(&self, id: DocId, before: Before) -> impl Iterator<Item = &Decision> {
        self.decisions.values().filter(move |d| {
            d.subject() == id && !self.revoked.contains(&d.id()) && before.keeps(d)
        })
    }

    /// The approval of the identity `id`, of this ledger, revoked or not; none for a
    /// decision of another verb, or no decision of the ledger.
    #[cfg(test)]
    pub(in crate::domain::shelf) fn approval(&self, id: JudgementId) -> Option<ApprovalView<'_>> {
        self.decisions
            .get(&id)
            .and_then(|decision| ApprovalView::of(self, decision).ok())
    }

    /// Every approval of the ledger, revoked or not, by identity: each decision that
    /// approves, as its view.
    pub(in crate::domain::shelf) fn approvals(&self) -> impl Iterator<Item = ApprovalView<'_>> {
        self.decisions()
            .filter_map(|decision| ApprovalView::of(self, decision).ok())
    }

    /// The upstreams every approval of the ledger pinned, revoked or not, each once.
    pub(crate) fn upstreams_pinned(&self) -> BTreeSet<DocId> {
        self.approvals()
            .flat_map(|view| view.frame().iter().map(FramePin::target))
            .collect()
    }

    /// The approvals that stand, by the identity of their documents.
    fn approvals_that_stand(&self) -> impl Iterator<Item = StandingApproval<'_>> {
        self.subjects
            .iter()
            .filter_map(|subject| match self.declared(*subject) {
                DeclaredNow::Approved(standing) => Some(standing),
                DeclaredNow::Pending | DeclaredNow::Against { .. } => None,
            })
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
        for standing in self.approvals_that_stand() {
            let view = standing.view();
            edges
                .entry(view.subject())
                .or_default()
                .extend(view.frame().iter().map(FramePin::target));
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
mod compared;
mod decision;
mod exempt;
mod frame;
mod judgement;
mod moved;
mod pin;
mod readers;
mod record;
mod refusal;
mod same_meaning;

use compared::{ComparedNow, PinsNow, Then};
pub use exempt::{Exempt, ExemptOf, ExemptSubject};
use moved::moved_way;
pub use moved::{
    FoundBy, GoneName, GoneNames, MoveCandidate, MoveNote, MoveSuggestion, MoveSuggestions, Moved,
    MovedOf, MovedProblem, MovedRefusal, MovedTo, NameSuggestions, NamedRefusal, NamedWhy,
    NoteKind, NothingWhy, PinnedNames, PinnedThere, PrefixPlan, PrefixRefusal, PrefixSuggestion,
    ReadPin, RefusedPrefix, Similarity, SpaceSuggestions, frame_moved, frame_prefix_moves,
    moved_space, plan_prefix_moves,
};
pub use refusal::{NotComparedWhy, SameMeaningRefusal};
pub use same_meaning::{
    Framed, SameMeaning, SameMeaningEdge, SameMeaningOf, SameMeaningProblem, SameMeaningTarget,
    ShortSameMeaning, frame_same_meaning,
};

pub use admit::{Admit, AdmittedRecord, NotAdmitted, RecordBook};
pub use canon::RecordProblem;
pub(in crate::domain::shelf) use compared::{ApprovalPins, PinNow, SubjectPins};
pub use decision::{Decision, DecisionProblem, DecisionVerb, RecordOrder};
pub use frame::{
    ApproveFrame, FrameRefusal, PresentFrame, RetireFrame, frame_approve, frame_present,
    frame_retire,
};
pub use judgement::{JudgedAt, JudgementId};
pub use pin::{ArtifactPin, FramePin, PATH_ONLY_SPACE, PinName, PinnedArtifact, Seen};
use pin::{NameForm, StandsOn};
pub use readers::{
    ApprovalKind, Asked, Before, LeaveOut, Quorum, ReadBy, ReaderCount, Readers, Reading, Taking,
};
pub use record::{Record, RecordHash, Revocation};

#[cfg(test)]
use super::artifact::{ArtifactReadings, Comparison, ComparisonProblem, Space, Untold};
#[cfg(test)]
use super::build::{DocFailure, FailureTag};
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
mod attribution_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod exempt_tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod judge_same_tests;
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod moved_prefix_tests;
#[cfg(test)]
mod moved_tests;
#[cfg(test)]
mod order_tests;
#[cfg(test)]
mod quorum_tests;
#[cfg(test)]
mod readers_tests;
#[cfg(test)]
mod revocation_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod tests;
