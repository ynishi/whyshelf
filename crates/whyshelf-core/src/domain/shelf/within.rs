//! Whether a change stays inside the documents it was meant for: for every document,
//! whether the change reaches it ([`Witness`], or what cannot be told of it,
//! [`ClaimUntold`]), and where it stands against the reach of the named documents as
//! the shelf stood before the change; one [`Verdict`] each, and whether all of them
//! pass ([`Within::passes`]).
//!
//! The gate decides none of what it reads. Each way a change is shown to reach a
//! document is the answer of the rule that owns it:
//!
//! | # | witness | read from |
//! |---|---|---|
//! | W1 | [`Witness::AddsCause`] | [`impact`](super::impact::impact) of the change: [`Made::Yes`] |
//! | W2 | [`Witness::RemovesCause`] | the same `impact`, the two ends swapped: [`Made::Yes`] |
//! | W3 | [`Witness::Decided`] | [`Ledger::status_of`] differs between the two ends |
//! | W4 | [`Witness::OnShelf`] | [`Shelf::state_of`] is `Valid` or `Invalid` at one end, `Gone` or `Unknown` at the other |
//! | W5 | [`Witness::Edited`] | while valid at both ends, [`Doc::hash`](super::doc::Doc::hash) differs (its book, fields and body), or what its links name ([`Doc::links_named`](super::doc::Doc::links_named)), which the hash leaves out; or it is valid at one end and invalid at the other |
//! | W6 | [`Witness::Recorded`] | a record about it ([`Record::about`]) is at one end only, by its identity, or not the same at both; a revocation is about what the records it withdraws are about |
//!
//! When none stands, what cannot be told is carried as the rules said it
//! ([`ClaimUntold`]): either direction of `impact` that answered
//! [`Made::CannotTell`], a document invalid at both ends (its content cannot be
//! compared), or one on the shelf at neither end and `Unknown` at one or both. The
//! holdings of the two ends ([`ImpactEntry::before`], [`ImpactEntry::after`]) and
//! whether they differ ([`ImpactEntry::changed`]) are not read: where one end alone
//! cannot tell a name, the two holdings differ by the reading, not by the change. W1
//! and W2 carry no causes: the report the gate is part of carries what each end held,
//! with its causes.
//!
//! Where a document stands, before the change (`Place`, in this order):
//!
//! | place | when |
//! |---|---|
//! | named | it is one of the named |
//! | reached | the walk down from the named on the shelf before reached it ([`reach`](super::reach)) |
//! | not before | it is `Gone` before the change: the reach before cannot hold it |
//! | reach untold | the walk before could not read everything ([`Unwalked`]) |
//! | outside | otherwise |
//!
//! A link the change writes is not counted: the reach is the shelf's before the
//! change, so a change cannot write itself inside, whether it adds a document or
//! gives one it edits a link to the named. A document the change adds, or puts back,
//! is inside only when it is named.
//!
//! # The verdict, the gate and the exit
//!
//! This is the one statement of the rule; the command line, the README and the
//! manual point here.
//!
//! | | named, reached | not before | reach untold | outside |
//! |---|---|---|---|---|
//! | reaches it | inside | unproven | unproven | outside |
//! | cannot tell | inside | undetermined | undetermined | undetermined |
//! | does not reach it | not claimed | not claimed | not claimed | not claimed |
//!
//! A verdict passes when it is not claimed or inside ([`Verdict::passes`]). Besides
//! the verdicts, the gate carries what cannot be read at either end ([`Unreadable`]):
//! a file of the shelf whose identity cannot be read, a file of the shelf left in a
//! merge conflict whose sides show different identities, and a file of the ledger
//! that is no record. None has an identity, and so a row or a verdict, and each may be
//! a document, or a record about one, the change touched; so the gate does
//! not pass while one is there, whichever end it is at. The gate passes when every
//! verdict passes and nothing is unreadable ([`Within::passes`]). `impact --within
//! --exit-code` exits 1 when it does not pass and 0 when it does; without
//! `--exit-code`, 0.
//!
//! The rows of the two directions are built from one set of identities, each paired
//! with its other (`impact::both_ways`, visible to the crate only), so neither
//! direction can lack a row the other has.
//!
//! Known limits:
//!
//! - Claimed more than the change wrote: a link names a document by its identity
//!   while it resolves and by its text when it no longer does, so when the document a
//!   link names is gone or no longer valid, the document that links to it is `edited`
//!   though its own file is untouched. A document that only informs one the change
//!   removes is so outside unless it is named or reached. (A retired document stays on
//!   the shelf, so a link to it still resolves.)
//! - Not claimed: a record about an artifact alone (a move, a declaration of the same
//!   meaning or an exemption of an artifact) that changes no approval; a file with no
//!   `id:` (it is no document); and a rename of a document's file that keeps its
//!   identity, its key and its content (nothing it is read as changes).
//! - Read through Git, nothing is built, so an approval on a `rust-item` cannot be
//!   told: one outside the reach is undetermined. Build the docs in two directories to
//!   tell it.
//! - A file invalid before the change makes a document reached outside unproven, not
//!   outside; a document invalid at both ends cannot be told.
//! - A key only the ledger knows before the change names the identity it had then
//!   (a document that took the key since is named by its identifier), so the other is
//!   unproven or outside, never passed.
//!
//! A verdict is made here and nowhere else, so it cannot be built from its parts
//! outside this crate:
//!
//! ```compile_fail
//! use whyshelf_core::domain::shelf::within::{Verdict, VerdictKind};
//!
//! fn make(kind: VerdictKind) -> Verdict {
//!     Verdict(kind)
//! }
//! ```
//!
//! and it is read through [`Verdict::kind`]:
//!
//! ```
//! use whyshelf_core::domain::shelf::within::{Verdict, VerdictKind};
//!
//! fn word(verdict: &Verdict) -> &'static str {
//!     match verdict.kind() {
//!         VerdictKind::NotClaimed { .. } => "not_claimed",
//!         VerdictKind::Inside { .. } => "inside",
//!         VerdictKind::Outside { .. } => "outside",
//!         VerdictKind::Unproven { .. } => "unproven",
//!         VerdictKind::Undetermined { .. } => "undetermined",
//!     }
//! }
//! ```
//!
//! [`Made::Yes`]: super::impact::Made::Yes
//! [`Made::CannotTell`]: super::impact::Made::CannotTell
//! [`Ledger::status_of`]: super::ledger::Ledger::status_of
//! [`Shelf::state_of`]: super::build::Shelf::state_of
//! [`ImpactEntry::before`]: super::impact::ImpactEntry::before
//! [`ImpactEntry::after`]: super::impact::ImpactEntry::after
//! [`ImpactEntry::changed`]: super::impact::ImpactEntry::changed
//! [`Record::about`]: super::ledger::Record::about

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonEmptySet, NonEmptyVec};

use super::build::{DocState, InvalidDoc, MayHold, Shelf};
use super::freshness::{RootCause, UntoldPin};
use super::impact::{ImpactEntry, Made, MadeCannotTell, Sides};
use super::ledger::{JudgementId, Ledger, Record, RecordAbout};
use super::reach::{Unwalked, Walked};

/// One end of a change, as the gate reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct End<'a> {
    /// The shelf.
    pub(crate) shelf: &'a Shelf,
    /// The ledger.
    pub(crate) ledger: &'a Ledger,
    /// The records the ledger was built from, every one that could be read.
    pub(crate) records: &'a [Record],
    /// The files of the ledger that are no record that could be read.
    pub(crate) unreadable_records: &'a [RelPath],
}

impl<'a> End<'a> {
    fn parts(&self) -> (&'a Shelf, &'a Ledger) {
        (self.shelf, self.ledger)
    }
}

/// The documents a record the change adds, takes away or rewrites is about (W6): each
/// record that is not the same at both ends, by its identity, and what it is about
/// ([`Record::about`]); a revocation is about what the records it withdraws are about,
/// at either end.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Recorded(BTreeSet<DocId>);

impl Recorded {
    pub(crate) fn of(before: &[Record], after: &[Record]) -> Self {
        fn index(records: &[Record]) -> BTreeMap<JudgementId, Vec<&Record>> {
            let mut by_id: BTreeMap<JudgementId, Vec<&Record>> = BTreeMap::new();
            for record in records {
                by_id.entry(record.id()).or_default().push(record);
            }
            by_id
        }
        let (was, is) = (index(before), index(after));
        let mut docs = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut next: Vec<JudgementId> = was
            .keys()
            .chain(is.keys())
            .filter(|id| was.get(id) != is.get(id))
            .copied()
            .collect();
        while let Some(id) = next.pop() {
            if !seen.insert(id) {
                continue;
            }
            for record in was.get(&id).into_iter().chain(is.get(&id)).flatten() {
                match record.about() {
                    RecordAbout::Doc(doc) => {
                        docs.insert(doc);
                    }
                    RecordAbout::Records(targets) => next.extend(targets.iter().copied()),
                    RecordAbout::Artifact => {}
                }
            }
        }
        Self(docs)
    }
}

/// One way the change is shown to reach a document, each read from the rule that
/// owns it; the gate decides none of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Witness {
    /// It adds a cause that makes the approval not hold ([`Made::Yes`] of the change).
    ///
    /// [`Made::Yes`]: super::impact::Made::Yes
    AddsCause,
    /// It takes away such a cause ([`Made::Yes`] of the change read backwards).
    ///
    /// [`Made::Yes`]: super::impact::Made::Yes
    RemovesCause,
    /// What was decided about it differs ([`Ledger::status_of`] of the two ends).
    Decided,
    /// It is on the shelf at one end only: [`Shelf::state_of`] is `Valid` or `Invalid`
    /// at one end and `Gone` or `Unknown` at the other.
    OnShelf,
    /// Its content differs: while it is valid at both ends, [`Doc::hash`] differs, or
    /// what its links name does ([`Doc::links_named`]); or it is valid at one end and
    /// invalid at the other ([`Shelf::state_of`]). A link whose target stops resolving
    /// (gone, or no longer valid) names it by text, not identity, so the linking
    /// document is edited too.
    ///
    /// [`Doc::hash`]: super::doc::Doc::hash
    /// [`Doc::links_named`]: super::doc::Doc::links_named
    Edited,
    /// A record about it is at one end only, or not the same at both: one the change
    /// adds, withdraws or rewrites ([`Record::about`]), whether or not it moves what
    /// was decided or whether an approval holds.
    Recorded,
}

/// What could not be told of whether the change reaches a document, each direction
/// kept apart and as `impact` answered it. At least one is there: only the gate
/// makes one.
// Each answer of `impact` is boxed: what cannot be told is the rarer verdict, and held
// inline it would make every claim and verdict as large as two of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimUntold {
    adds: Option<Box<MadeCannotTell>>,
    removes: Option<Backward>,
    edit: Option<InvalidBoth>,
    shelf: Option<UnknownAt>,
}

impl ClaimUntold {
    /// What `impact` of the change could not tell.
    pub fn adds(&self) -> Option<&MadeCannotTell> {
        self.adds.as_deref()
    }

    /// What `impact` of the change read backwards could not tell.
    pub fn removes(&self) -> Option<&Backward> {
        self.removes.as_ref()
    }

    /// The document is invalid at both ends, so its content cannot be compared.
    pub fn edit(&self) -> Option<&InvalidBoth> {
        self.edit.as_ref()
    }

    /// The document is on the shelf at neither end, and a file may hold it at one or
    /// both.
    pub fn shelf(&self) -> Option<&UnknownAt> {
        self.shelf.as_ref()
    }
}

/// What `impact` could not tell of the change read backwards (after to before), said
/// in the change's own words: the one place the sides of the two readings are turned.
/// Only the gate makes one, from the backward reading's answer, kept as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backward(Box<MadeCannotTell>);

impl Backward {
    /// The side of the change that could not tell: the backward reading's side
    /// before is the change's side after, and its side after the change's side
    /// before; both stays both.
    pub fn side(&self) -> Sides {
        match self.0.side() {
            Sides::Before => Sides::After,
            Sides::After => Sides::Before,
            Sides::Both => Sides::Both,
        }
    }

    /// The names that could not be told.
    pub fn untold(&self) -> &NonEmptyVec<UntoldPin> {
        self.0.untold()
    }

    /// The causes of the side before the change (the backward reading's side after)
    /// that stand on a name its other side could not tell.
    pub fn uncertain(&self) -> &[RootCause] {
        self.0.uncertain()
    }
}

/// The document is invalid at both ends, so whether its content changed cannot be
/// told: an invalid document has no hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidBoth;

/// The document is on the shelf at neither end (`Gone` or `Unknown`), and `Unknown`
/// at one or both: a file may hold it, so whether the change put it back or rewrote
/// it cannot be told. Each end's files that may hold it, as [`DocState::Unknown`]
/// gives them; at least one end has them, and only the gate makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAt {
    before: Option<MayHold>,
    after: Option<MayHold>,
}

impl UnknownAt {
    /// The files that may hold the document before the change, when it is `Unknown`
    /// there.
    pub fn before(&self) -> Option<&MayHold> {
        self.before.as_ref()
    }

    /// The files that may hold the document after the change, when it is `Unknown`
    /// there.
    pub fn after(&self) -> Option<&MayHold> {
        self.after.as_ref()
    }
}

/// Whether the change reaches a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Claim {
    /// It does, shown by these.
    Claimed(NonEmptySet<Witness>),
    /// It cannot be told.
    CannotTell(ClaimUntold),
    /// It does not.
    NotClaimed,
}

/// Whether a document is on the shelf at an end: valid or invalid.
fn on_shelf(state: &DocState<'_>) -> bool {
    matches!(state, DocState::Valid(_) | DocState::Invalid(_))
}

/// The files that may hold the document, when it is `Unknown`.
fn unknown_in(state: &DocState<'_>) -> Option<MayHold> {
    match state {
        DocState::Unknown(may) => Some(may.clone()),
        DocState::Valid(_) | DocState::Invalid(_) | DocState::Gone => None,
    }
}

impl Claim {
    /// Whether the change reaches the document of `forward` (the rules W1–W6 on the
    /// page of the module). `forward` and `backward` are the rows of one identity, of
    /// `impact` of the change and of the change read backwards, paired as
    /// [`both_ways`](super::impact::both_ways) builds them;
    /// `before` and `after` are each end's shelf and ledger.
    pub(crate) fn of(
        forward: &ImpactEntry,
        backward: &ImpactEntry,
        before: (&Shelf, &Ledger),
        after: (&Shelf, &Ledger),
        recorded: &Recorded,
    ) -> Self {
        let id = forward.id();
        let (was, is) = (before.0.state_of(id), after.0.state_of(id));
        let mut witnesses = BTreeSet::new();
        if matches!(forward.made(), Made::Yes(_)) {
            witnesses.insert(Witness::AddsCause);
        }
        if matches!(backward.made(), Made::Yes(_)) {
            witnesses.insert(Witness::RemovesCause);
        }
        if before.1.status_of(id) != after.1.status_of(id) {
            witnesses.insert(Witness::Decided);
        }
        if on_shelf(&was) != on_shelf(&is) {
            witnesses.insert(Witness::OnShelf);
        }
        let edited = match (&was, &is) {
            (DocState::Valid(a), DocState::Valid(b)) => {
                a.hash() != b.hash() || a.links_named() != b.links_named()
            }
            (DocState::Valid(_), DocState::Invalid(_))
            | (DocState::Invalid(_), DocState::Valid(_)) => true,
            _ => false,
        };
        if edited {
            witnesses.insert(Witness::Edited);
        }
        if recorded.0.contains(&id) {
            witnesses.insert(Witness::Recorded);
        }
        if let Ok(witnesses) = NonEmptySet::from_set(witnesses) {
            return Self::Claimed(witnesses);
        }
        let untold_of = |entry: &ImpactEntry| match entry.made() {
            Made::CannotTell(why) => Some(why.clone()),
            Made::Yes(_) | Made::No => None,
        };
        let shelf = (!on_shelf(&was) && !on_shelf(&is))
            .then(|| UnknownAt {
                before: unknown_in(&was),
                after: unknown_in(&is),
            })
            .filter(|at| at.before.is_some() || at.after.is_some());
        let untold = ClaimUntold {
            adds: untold_of(forward).map(Box::new),
            removes: untold_of(backward).map(|why| Backward(Box::new(why))),
            edit: matches!((&was, &is), (DocState::Invalid(_), DocState::Invalid(_)))
                .then_some(InvalidBoth),
            shelf,
        };
        if untold.adds.is_some()
            || untold.removes.is_some()
            || untold.edit.is_some()
            || untold.shelf.is_some()
        {
            Self::CannotTell(untold)
        } else {
            Self::NotClaimed
        }
    }
}

/// Where a document stands against the reach of the named documents before the
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// It is one of the named.
    Named,
    /// The walk down from the named before the change reached it.
    Reached,
    /// It is `Gone` before the change: the reach before cannot hold it.
    NotBefore,
    /// Not reached, and the walk before could not read everything.
    Untold(Unwalked),
    /// Outside the reach.
    Outside,
}

impl Place {
    /// Where `id` stands (the table of places on the page of the module), given the
    /// named, the walk down from them on the shelf before the change, and that shelf.
    pub(crate) fn of(
        id: DocId,
        named: &NonEmptySet<DocId>,
        walked: &Walked,
        before: &Shelf,
    ) -> Self {
        if named.contains(&id) {
            return Self::Named;
        }
        if walked.reached().contains(&id) {
            return Self::Reached;
        }
        if matches!(before.state_of(id), DocState::Gone) {
            return Self::NotBefore;
        }
        // A document `Unknown` before falls here: a file whose identity cannot be read
        // makes the walk's untold.
        match walked.untold() {
            Some(untold) => Self::Untold(*untold),
            None => Self::Outside,
        }
    }
}

/// How a document is inside the reach before the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsideBy {
    /// It is named.
    Named,
    /// It depends on one named, transitively.
    Reached,
}

/// What the change does to a document inside the reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsideClaim {
    /// It reaches it, shown by these.
    Witnessed(NonEmptySet<Witness>),
    /// Whether it does cannot be told, with what cannot be.
    CannotTell(ClaimUntold),
}

/// Why a document the change reaches cannot be shown outside the reach before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnprovenWhy {
    /// It was not on the shelf before the change: the change adds or puts it back.
    NotBefore,
    /// The walk before could not read everything, so it may be inside.
    ReachUntold(Unwalked),
}

/// Where a document stands that is not inside the reach before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Away {
    /// It was not on the shelf before the change.
    NotBefore,
    /// The walk before could not read everything, so it may be inside.
    ReachUntold(Unwalked),
    /// Outside the reach.
    Outside,
}

/// What the gate answers of one document (the table of verdicts on the page of the
/// module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictKind {
    /// The change does not reach it.
    #[non_exhaustive]
    NotClaimed,
    /// It is inside the reach before the change, whatever the change does to it.
    #[non_exhaustive]
    Inside {
        /// How it is inside.
        by: InsideBy,
        /// What the change does to it.
        claim: InsideClaim,
    },
    /// The change reaches it, and it is outside the reach.
    #[non_exhaustive]
    Outside {
        /// How the change is shown to reach it.
        witnesses: NonEmptySet<Witness>,
    },
    /// The change reaches it, and it cannot be shown outside the reach.
    #[non_exhaustive]
    Unproven {
        /// Why.
        why: UnprovenWhy,
        /// How the change is shown to reach it.
        witnesses: NonEmptySet<Witness>,
    },
    /// Whether the change reaches it cannot be told, and it is not inside the reach.
    #[non_exhaustive]
    Undetermined {
        /// Where it stands.
        place: Away,
        /// What cannot be told.
        untold: ClaimUntold,
    },
}

/// What the gate answers for one document. Only the gate makes one: the field is
/// private, so neither this crate's other modules nor another crate can build one
/// from its parts (the parts are public).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict(VerdictKind);

impl Verdict {
    /// The verdict (the table on the page of the module).
    pub fn kind(&self) -> &VerdictKind {
        &self.0
    }

    /// Whether it passes: the change does not reach the document, or the document is
    /// inside the reach. The one place one verdict is read as passing.
    pub fn passes(&self) -> bool {
        matches!(self.0, VerdictKind::NotClaimed | VerdictKind::Inside { .. })
    }

    /// The verdict of a document the change does `claim` to, standing at `place`:
    /// the table of verdicts.
    fn of(claim: Claim, place: Place) -> Self {
        let inside = |by, claim| VerdictKind::Inside { by, claim };
        Self(match claim {
            Claim::NotClaimed => VerdictKind::NotClaimed,
            Claim::Claimed(witnesses) => match place {
                Place::Named => inside(InsideBy::Named, InsideClaim::Witnessed(witnesses)),
                Place::Reached => inside(InsideBy::Reached, InsideClaim::Witnessed(witnesses)),
                Place::NotBefore => VerdictKind::Unproven {
                    why: UnprovenWhy::NotBefore,
                    witnesses,
                },
                Place::Untold(walk) => VerdictKind::Unproven {
                    why: UnprovenWhy::ReachUntold(walk),
                    witnesses,
                },
                Place::Outside => VerdictKind::Outside { witnesses },
            },
            Claim::CannotTell(untold) => match place {
                Place::Named => inside(InsideBy::Named, InsideClaim::CannotTell(untold)),
                Place::Reached => inside(InsideBy::Reached, InsideClaim::CannotTell(untold)),
                Place::NotBefore => VerdictKind::Undetermined {
                    place: Away::NotBefore,
                    untold,
                },
                Place::Untold(walk) => VerdictKind::Undetermined {
                    place: Away::ReachUntold(walk),
                    untold,
                },
                Place::Outside => VerdictKind::Undetermined {
                    place: Away::Outside,
                    untold,
                },
            },
        })
    }
}

/// One document's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithinEntry {
    id: DocId,
    verdict: Verdict,
}

impl WithinEntry {
    /// The document.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// What the gate answers of it.
    pub fn verdict(&self) -> &Verdict {
        &self.verdict
    }
}

/// What the gate cannot read at either end: the files of the shelf whose identity
/// cannot be read, those left in a merge conflict whose sides show different
/// identities ([`InvalidDoc::sides`]), and the files
/// of the ledger that are no record that can be read. Each may be a document, or a
/// record about one, that the change adds, takes away or rewrites, and none has an
/// identity to give a verdict of its own (a file left in a merge conflict holds one of
/// the documents its sides show, and a document of a side that has no record has no
/// verdict); so whether the change reaches outside the reach cannot be told while any
/// is there, and the gate does not guess which. At least one list holds a file; only
/// the gate makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    files_before: Vec<RelPath>,
    files_after: Vec<RelPath>,
    sided_before: Vec<RelPath>,
    sided_after: Vec<RelPath>,
    records_before: Vec<RelPath>,
    records_after: Vec<RelPath>,
}

impl Unreadable {
    /// The files of the shelf before the change whose identity cannot be read.
    pub fn files_before(&self) -> &[RelPath] {
        &self.files_before
    }

    /// The files of the shelf after the change whose identity cannot be read.
    pub fn files_after(&self) -> &[RelPath] {
        &self.files_after
    }

    /// The files of the shelf before the change left in a merge conflict whose sides
    /// show different identities.
    pub fn sided_before(&self) -> &[RelPath] {
        &self.sided_before
    }

    /// The files of the shelf after the change left in a merge conflict whose sides
    /// show different identities.
    pub fn sided_after(&self) -> &[RelPath] {
        &self.sided_after
    }

    /// The files of the ledger before the change that are no record.
    pub fn records_before(&self) -> &[RelPath] {
        &self.records_before
    }

    /// The files of the ledger after the change that are no record.
    pub fn records_after(&self) -> &[RelPath] {
        &self.records_after
    }

    /// What cannot be read at the two ends, if anything.
    fn of(before: &End<'_>, after: &End<'_>) -> Option<Self> {
        let origins = |files: &[InvalidDoc]| -> Vec<RelPath> {
            files.iter().map(|doc| doc.origin().clone()).collect()
        };
        let found = Self {
            files_before: origins(before.shelf.unreadable()),
            files_after: origins(after.shelf.unreadable()),
            sided_before: origins(before.shelf.sided()),
            sided_after: origins(after.shelf.sided()),
            records_before: before.unreadable_records.to_vec(),
            records_after: after.unreadable_records.to_vec(),
        };
        let any = !(found.files_before.is_empty()
            && found.files_after.is_empty()
            && found.sided_before.is_empty()
            && found.sided_after.is_empty()
            && found.records_before.is_empty()
            && found.records_after.is_empty());
        any.then_some(found)
    }
}

/// The gate's answer: the named, the reach before the change, a verdict for every
/// document `impact` answers and for every other document a record the change touched
/// is about, and what cannot be read at either end. Only the gate
/// makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Within {
    named: NonEmptySet<DocId>,
    reach_before: Walked,
    entries: Vec<WithinEntry>,
    unreadable: Option<Unreadable>,
}

impl Within {
    /// The documents named.
    pub fn named(&self) -> &NonEmptySet<DocId> {
        &self.named
    }

    /// The reach before the change: the named, and every document that depends on one
    /// of them on the shelf before it, transitively.
    pub fn reached_before(&self) -> &BTreeSet<DocId> {
        self.reach_before.reached()
    }

    /// What may depend on the named before the change and could not be read, when
    /// anything may.
    pub fn untold_before(&self) -> Option<&Unwalked> {
        self.reach_before.untold()
    }

    /// Every document, by identity, with its verdict: each `impact` answers, and each
    /// other a record the change touched is about.
    pub fn entries(&self) -> &[WithinEntry] {
        &self.entries
    }

    /// What cannot be read at either end, when anything cannot: while it is there,
    /// whether the change reaches outside the reach cannot be told.
    pub fn unreadable(&self) -> Option<&Unreadable> {
        self.unreadable.as_ref()
    }

    /// Whether the gate passes: every verdict passes ([`Verdict::passes`]), and nothing
    /// at either end cannot be read ([`Within::unreadable`]). The one place the gate is
    /// folded.
    pub fn passes(&self) -> bool {
        self.unreadable.is_none() && self.entries.iter().all(|entry| entry.verdict.passes())
    }
}

/// The gate: for each identity's pair of rows (`impact` of the change, and of the
/// change read backwards, `impact::both_ways`), whether the change reaches the
/// document ([`Claim::of`], with the records the change adds, withdraws or rewrites,
/// [`Recorded`]) and where it stands ([`Place::of`]), folded into its verdict; and what
/// cannot be read at either end ([`Unreadable`]). `walked` is the walk down from
/// `named` on the shelf before.
pub(crate) fn within(
    named: NonEmptySet<DocId>,
    walked: Walked,
    before: End<'_>,
    after: End<'_>,
    rows: &[(ImpactEntry, ImpactEntry)],
) -> Within {
    let recorded = Recorded::of(before.records, after.records);
    let place = |id: DocId| Place::of(id, &named, &walked, before.shelf);
    let mut entries: Vec<WithinEntry> = rows
        .iter()
        .map(|(forward, backward)| WithinEntry {
            id: forward.id(),
            verdict: Verdict::of(
                Claim::of(forward, backward, before.parts(), after.parts(), &recorded),
                place(forward.id()),
            ),
        })
        .collect();
    // A record the change touched about a document `impact` has no row for (one on the
    // shelf at neither end that no decision is about) reaches it all the same: it gets
    // its verdict here too, by the one table, with that record its only witness.
    let rowed: BTreeSet<DocId> = rows.iter().map(|(forward, _)| forward.id()).collect();
    entries.extend(recorded.0.difference(&rowed).map(|id| WithinEntry {
        id: *id,
        verdict: Verdict::of(
            Claim::Claimed(NonEmptySet::new(Witness::Recorded, [])),
            place(*id),
        ),
    }));
    entries.sort_by_key(|entry| entry.id);
    Within {
        named,
        reach_before: walked,
        entries,
        unreadable: Unreadable::of(&before, &after),
    }
}

#[cfg(test)]
#[path = "within/tests.rs"]
mod tests;
