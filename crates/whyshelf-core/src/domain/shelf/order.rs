//! Approving a set of documents in one run: which are taken, by two rules, one owner
//! each ([`stale`] for `judge approve --stale`, [`pending`] and [`pending_upstream`]
//! for `--pending`, both read from one [`Assessment`]); in which order
//! ([`upstream_first`]); which upstream holds one back ([`held_back`]); and, for an
//! approval made by naming one document, a run of one, what it would meet at the
//! frame, foreseen with what `--stale` would take ([`Foreseen`]). Each rule is on its
//! function. No order of books is read: the order comes from what each document
//! stands on.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::id::DocId;

use super::build::Shelf;
use super::footing::Footing;
use super::freshness::{
    Assessment, Carried, Degradation, FreshnessLevel, OriginGroup, Severity, Standing,
    StandingKind, Unapproved, UpstreamReading,
};
use super::graph::components;
use super::ledger::{ApproveFrame, FrameRefusal, Ledger, NotStanding, frame_approve};
use super::schema::EntryName;

/// The rule that took a document into a run of approvals. The two take apart sets: a
/// stale one has an approval that stands, a pending one has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// Its approval is expired or stale ([`stale`]).
    Stale,
    /// No decision stands about it ([`pending`]).
    Pending,
}

/// What [`stale`] takes: the approvals to make again, and the broken ones, left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleChoice {
    /// The documents whose approval is expired or stale.
    pub again: BTreeSet<DocId>,
    /// The documents whose approval is broken, by identity, each with why.
    pub broken: Vec<(DocId, Degradation)>,
}

/// The rule of `--stale`: an approval that is expired or stale is taken to be made
/// again; one that is broken is not, and is told with its degradation, since what it
/// stood on is gone, refused or cannot be trusted. Every other standing (fresh,
/// pending, decided against) is not looked at, and neither is a quorum: a fresh
/// approval that awaits other names is not degraded, and another name settles it,
/// not a run.
pub fn stale(assessment: &Assessment) -> StaleChoice {
    let mut choice = StaleChoice {
        again: BTreeSet::new(),
        broken: Vec::new(),
    };
    for (id, degradation) in assessment.degraded() {
        match degradation.level() {
            Severity::Expired | Severity::Stale => {
                choice.again.insert(id);
            }
            Severity::Broken => choice.broken.push((id, degradation.clone())),
        }
    }
    choice
}

/// The approvals a run of `judge approve --stale` alone would make again, as far as
/// it can be told before the run: those [`stale`] takes again that no upstream holds
/// back ([`held_back`]), taken upstream first ([`upstream_first`]) as the run takes
/// them, so that one standing on a pending upstream, which `--stale` does not
/// approve, holds back what stands on it in turn.
///
/// One whose frame is refused as the shelf and the ledger tell it before the run
/// ([`ApproveFrame::foresee`]) is left out, as the run leaves it out, and so is what
/// stands on it: a run writes only approvals, of documents nothing was decided
/// against, which change no answer of it. What only the records the run itself
/// writes, or who runs it, decide is not foreseen, such as a cycle among the frames
/// of approvals, which the run's own approvals rewrite, a repeat or a second reading,
/// a name the rules do not admit, or a hold its own approvals make (an upstream it
/// approves again by fewer names than its rule asks for then awaits others,
/// [`Hold::Awaiting`]).
pub fn stale_taken(shelf: &Shelf, ledger: &Ledger, assessment: &Assessment) -> BTreeSet<DocId> {
    let run = stale(assessment).again;
    let mut left_out = BTreeSet::new();
    let mut taken = BTreeSet::new();
    for id in upstream_first(shelf, ledger, &run) {
        if held_back(shelf, assessment, id, &run, &left_out).is_some()
            || ApproveFrame::foresee(shelf, ledger, id).is_err()
        {
            left_out.insert(id);
        } else {
            taken.insert(id);
        }
    }
    taken
}

/// The origins of a shelf to be settled, with the documents their settlements
/// approve by naming them: what [`Foreseen::of`] asks the frame about. Which
/// documents those are is what settles the origins to say
/// ([`origins_asked`](super::settle::origins_asked)), so only the modules of the
/// shelf make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginsAsked {
    groups: Vec<OriginGroup>,
    named: BTreeSet<DocId>,
}

impl OriginsAsked {
    /// The origins `groups`, with `named`, the documents their settlements approve
    /// by naming them.
    pub(super) fn new(groups: Vec<OriginGroup>, named: BTreeSet<DocId>) -> Self {
        Self { groups, named }
    }
}

/// What the approving commands `stale` can offer would approve, foreseen once for
/// the shelf before any of them is run, with the origins it was asked about: what
/// `--stale` would approve again ([`stale_taken`]), and, for each document the
/// settlements of those origins approve by name ([`OriginsAsked`]), whether the frame
/// takes an approval of it.
///
/// An approval by name is a run of one, which writes nothing before it, so the frame
/// it meets is [`frame_approve`] on the ledger as it is, the cycle among the frames
/// of approvals included; why `--stale` is foreseen otherwise is [`stale_taken`]'s.
/// A document it was not asked about is not taken: an approval of it is offered only
/// once the frame was asked. The origins are settled with it by
/// [`settle_all`](super::settle::settle_all), so the ones it was asked about are the
/// ones settled. What only the person who runs it decides is not foreseen: whether
/// the rules admit the name, a name that already approved the content (which repeats
/// its reading), the time it is to lapse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreseen {
    groups: Vec<OriginGroup>,
    again: BTreeSet<DocId>,
    framed: BTreeMap<DocId, Result<(), FrameRefusal>>,
}

impl Foreseen {
    /// What is foreseen of the origins `asked` holds, on this shelf and ledger, as
    /// [`Foreseen`] says.
    pub fn of(
        shelf: &Shelf,
        ledger: &Ledger,
        assessment: &Assessment,
        asked: OriginsAsked,
    ) -> Self {
        let framed = asked
            .named
            .into_iter()
            .map(|doc| (doc, frame_approve(shelf, ledger, doc).map(|_| ())))
            .collect();
        Self {
            groups: asked.groups,
            again: stale_taken(shelf, ledger, assessment),
            framed,
        }
    }

    /// A foreseen value of no origin, from its parts, for the tests of what reads it.
    #[cfg(test)]
    pub(crate) fn of_parts(
        again: BTreeSet<DocId>,
        framed: BTreeMap<DocId, Result<(), FrameRefusal>>,
    ) -> Self {
        Self {
            groups: Vec::new(),
            again,
            framed,
        }
    }

    /// The approvals `--stale` would make again ([`stale_taken`]).
    pub(super) fn again(&self) -> &BTreeSet<DocId> {
        &self.again
    }

    /// Whether the frame takes an approval of `doc`; see [`Foreseen`] for one it was
    /// not asked about.
    pub(super) fn frame_takes(&self, doc: DocId) -> bool {
        matches!(self.framed.get(&doc), Some(Ok(())))
    }

    /// The origins it was asked about, in the order they were given.
    pub(super) fn groups(&self) -> &[OriginGroup] {
        &self.groups
    }

    /// The origins it was asked about, given back to be settled.
    pub(super) fn into_groups(self) -> Vec<OriginGroup> {
        self.groups
    }
}

/// What stands about a document that is not pending: a decision. There is no
/// pending kind, so a [`NotPending`] cannot say "pending".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecidedKind {
    /// Approved, at this level.
    Approved(FreshnessLevel),
    /// Denied.
    Denied,
    /// Deprecated.
    Deprecated,
    /// Retired.
    Retired,
}

impl DecidedKind {
    /// The decided kind of a standing; `None` for [`Standing::Pending`].
    fn of(standing: &Standing) -> Option<Self> {
        match standing.kind() {
            StandingKind::Pending => None,
            StandingKind::Approved(level) => Some(Self::Approved(level)),
            StandingKind::Denied => Some(Self::Denied),
            StandingKind::Deprecated => Some(Self::Deprecated),
            StandingKind::Retired => Some(Self::Retired),
        }
    }
}

/// Why a document is not pending for [`pending_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotPending {
    /// No valid document has the identity now (its files are invalid, or none has it),
    /// whatever the ledger says: an approval of it would be refused.
    NotValid,
    /// A decision stands about it: an approval (at any freshness), a denial, a
    /// deprecation or a retirement.
    Decided(DecidedKind),
}

impl fmt::Display for NotPending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = match self {
            Self::NotValid => "no valid document now",
            Self::Decided(DecidedKind::Denied) => "denied",
            Self::Decided(DecidedKind::Deprecated) => "deprecated",
            Self::Decided(DecidedKind::Retired) => "retired",
            Self::Decided(DecidedKind::Approved(FreshnessLevel::Fresh)) => "approved, and fresh",
            Self::Decided(DecidedKind::Approved(FreshnessLevel::Awaiting)) => {
                "approved, and awaiting the approvals of other names"
            }
            Self::Decided(DecidedKind::Approved(FreshnessLevel::Expired)) => {
                "approved, and expired"
            }
            Self::Decided(DecidedKind::Approved(FreshnessLevel::Stale)) => "approved, and stale",
            Self::Decided(DecidedKind::Approved(FreshnessLevel::Broken)) => "approved, and broken",
        };
        f.write_str(word)
    }
}

/// The rule of `--pending`, for one document: it is pending when it is a valid
/// document now and no decision stands about it ([`Standing::Pending`], which is read
/// from [`Ledger::status_of`]: never decided, or every decision about it withdrawn).
/// A document whose files are invalid, or that has none, is not, though the ledger
/// says nothing of it; nor is one approved, or decided against
/// ([`NotStanding`]).
///
/// The ledger knows no standing "superseded": a pending document another replaces
/// (a `Replaces` link to it) is pending, and taken as any other.
pub fn pending_of(shelf: &Shelf, assessment: &Assessment, id: DocId) -> Result<(), NotPending> {
    if shelf.doc(id).is_none() {
        return Err(NotPending::NotValid);
    }
    match DecidedKind::of(&assessment.standing_of(id)) {
        None => Ok(()),
        Some(decided) => Err(NotPending::Decided(decided)),
    }
}

/// What the `Depends` links of a document resolve to now (`Doc::depends_links`);
/// nothing for an identity with no valid document.
fn depends_targets(shelf: &Shelf, id: DocId) -> Vec<DocId> {
    shelf.doc(id).map_or_else(Vec::new, |doc| {
        doc.depends_links().map(|(_, target)| target).collect()
    })
}

/// The members of `among` that no other member names through a `Depends` link, as
/// the documents link now: the fewest of them that [`pending_upstream`] takes the
/// whole set from, when every member is pending. `init` names these to approve
/// exactly the documents it made.
pub fn heads(shelf: &Shelf, among: &BTreeSet<DocId>) -> BTreeSet<DocId> {
    let stood_on: BTreeSet<DocId> = among
        .iter()
        .flat_map(|id| depends_targets(shelf, *id))
        .collect();
    among.difference(&stood_on).copied().collect()
}

/// Every document [`pending_of`] says is pending.
pub fn pending(shelf: &Shelf, assessment: &Assessment) -> BTreeSet<DocId> {
    shelf
        .docs()
        .keys()
        .copied()
        .filter(|id| pending_of(shelf, assessment, *id).is_ok())
        .collect()
}

/// `named`, and every pending document ([`pending_of`]) reached from one of them
/// through `Depends` links, transitively, as the documents link now.
///
/// The links are followed through every document reached, pending or not, so a
/// pending document stood on through an approved one is taken too: a run that also
/// approves again what is stale can then approve that one in between. A link that
/// cannot be followed leads nowhere. `named` is taken as it is: whether each named
/// one is pending is the caller's to ask first.
pub fn pending_upstream(
    shelf: &Shelf,
    assessment: &Assessment,
    named: &BTreeSet<DocId>,
) -> BTreeSet<DocId> {
    let mut seen: BTreeSet<DocId> = named.clone();
    let mut taken: BTreeSet<DocId> = named.clone();
    let mut next: Vec<DocId> = named.iter().copied().collect();
    while let Some(id) = next.pop() {
        for upstream in depends_targets(shelf, id) {
            if !seen.insert(upstream) {
                continue;
            }
            if pending_of(shelf, assessment, upstream).is_ok() {
                taken.insert(upstream);
            }
            next.push(upstream);
        }
    }
    taken
}

/// Why an upstream holds an approval back from being made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// The upstream's approval is broken.
    Broken,
    /// The upstream is pending and not in the run: approve it first.
    Pending,
    /// The upstream's approval is stale and not in the run: approve it again first.
    Stale,
    /// The upstream's approval awaits the approvals of other names
    /// ([`Quorum::Short`](super::ledger::Quorum::Short)),
    /// at any freshness, an expired one included, in the run or not: an approval on
    /// it would be stale from the start, as on a pending one, until another name
    /// approves it.
    Awaiting,
    /// The upstream is itself held back, or refused, in the same run.
    LeftOut,
}

/// The upstream that holds an approval back, through which link, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldBack {
    /// The `Depends` link that names it now.
    pub link: EntryName,
    /// The upstream.
    pub upstream: DocId,
    /// Why it holds the approval back.
    pub hold: Hold,
}

/// The first upstream that keeps `id` from being approved so that it holds, if one
/// does: a document a `Depends` link of `id` resolves to now that is broken, pending
/// and not in `run`, stale and not in `run`, approved with a quorum short of names
/// ([`Quorum::Short`](super::ledger::Quorum::Short), at any freshness), or in
/// `left_out` (the documents of the same run already left out), in that order of
/// precedence, the links in the order the document holds them. `run` is every document the run takes; one of it is decided
/// before what stands on it ([`upstream_first`]), approved or left out, so one of it
/// left out is told as `LeftOut`, not by its standing.
///
/// An approval made on such an upstream would be broken or stale from the start. An
/// upstream whose approval only expired holds nothing back, unless its quorum is
/// short: an approval on it is a notice, not a staleness. A short quorum is read
/// before that, as it is no level of the freshness, and an expired approval that
/// awaits other names would otherwise let what stands on it through. An upstream a
/// new approval may not stand on ([`Ledger::may_stand_on`]), a link that cannot be
/// followed and a cycle are refusals of the frame
/// ([`frame_approve`]), not this. What an upstream is to
/// the approval on it is read from its standing once (`UpstreamReading` of the
/// module `freshness`), and a reading that holds nothing back is then asked only
/// whether the upstream was left out. Standings are read from `assessment`, which the
/// caller makes of the ledger as the approvals already decided in the run left it: an
/// upstream stale only through one the run has approved since holds nothing back.
///
/// Each (link, target) of the links is told on its own, by what its upstream is; the
/// first that holds the approval back is the answer.
pub fn held_back(
    shelf: &Shelf,
    assessment: &Assessment,
    id: DocId,
    run: &BTreeSet<DocId>,
    left_out: &BTreeSet<DocId>,
) -> Option<HeldBack> {
    let doc = shelf.doc(id)?;
    doc.depends_links()
        .find_map(|(link, upstream)| hold_of(assessment, link, upstream, run, left_out))
}

/// The upstreams a new approval of the document of `footing` may not stand on
/// ([`Ledger::may_stand_on`]), among what its `Depends` links resolve to now (the
/// footing's linked, so one two links name is there once), each with the decision
/// against it. What its approval only pinned is not among them: the frame of a new
/// approval pins the links as they are.
pub(super) fn refused_upstreams(
    ledger: &Ledger,
    footing: &Footing,
) -> BTreeMap<DocId, NotStanding> {
    footing
        .linked()
        .iter()
        .filter_map(|upstream| {
            ledger
                .may_stand_on(*upstream)
                .err()
                .map(|status| (*upstream, status))
        })
        .collect()
}

/// For each (link, target) of `id`'s Depends links, the hold [`held_back`] tells of
/// it, in link order then target order; [`held_back`] is the first.
pub(super) fn holds(
    shelf: &Shelf,
    assessment: &Assessment,
    id: DocId,
    run: &BTreeSet<DocId>,
    left_out: &BTreeSet<DocId>,
) -> Vec<HeldBack> {
    shelf.doc(id).map_or_else(Vec::new, |doc| {
        doc.depends_links()
            .filter_map(|(link, upstream)| hold_of(assessment, link, upstream, run, left_out))
            .collect()
    })
}

/// What one (link, target) holds back, by the rule of [`held_back`].
fn hold_of(
    assessment: &Assessment,
    link: &EntryName,
    upstream: DocId,
    run: &BTreeSet<DocId>,
    left_out: &BTreeSet<DocId>,
) -> Option<HeldBack> {
    let in_run = run.contains(&upstream);
    // What the upstream is to an approval on it is read once, from its
    // standing; here only what holds a run back of that.
    let hold = match UpstreamReading::of(&assessment.standing_of(upstream)) {
        UpstreamReading::Approval { awaiting, carried } => match carried {
            Some(Carried::Broken) => Some(Hold::Broken),
            Some(Carried::Stale) if !in_run => Some(Hold::Stale),
            Some(Carried::Stale) | None => awaiting.then_some(Hold::Awaiting),
        },
        UpstreamReading::Unapproved(Unapproved::Pending) if !in_run => Some(Hold::Pending),
        // Holds nothing back: a pending upstream in the run is decided
        // before this one, and refusing an approval on an upstream decided
        // against is the frame's (`frame_approve`), not the run's.
        UpstreamReading::Unapproved(Unapproved::Pending) => None,
        UpstreamReading::Unapproved(Unapproved::Superseded { .. } | Unapproved::Refused(_)) => None,
    };
    // The same after every arm that holds nothing back: the upstream was
    // itself left out of the run.
    let hold = hold.or_else(|| left_out.contains(&upstream).then_some(Hold::LeftOut))?;
    Some(HeldBack {
        link: link.clone(),
        upstream,
        hold,
    })
}

/// The identities of `among`, each after every one of `among` it stands on.
///
/// A document stands on what its `Depends` links resolve to now ([`Shelf::build`])
/// and on what the approval that stands about it pinned ([`Ledger::declared`]).
/// Only the documents of `among` are ordered, and only the edges between two of
/// them count: a document outside the set is left out, and no order passes through
/// it. A link that names a document it cannot stand on (one that is not valid, or a
/// key two files write) is no edge, and gives no order. Otherwise the order is
/// upstream first and deterministic: the same set, shelf and ledger give the same
/// order, and the documents with no edge to another of the set come by identity
/// among themselves.
///
/// Every document a `Depends` link of a member resolves to, when it is in `among`,
/// comes before the member; so one pass in this order sees each upstream decided
/// before what stands on it.
///
/// A set may hold a cycle: a broken approval may be one of a cycle of links or of
/// frames ([`Cause::InCycle`]), and a pending document's links may close one, so a
/// set that takes broken or pending documents (the pending ones `--pending` takes, a
/// unit of [`units`](super::units)) can bring it here. The
/// members of a cycle the links alone close come by identity among themselves. The
/// two graphs together can also close a ring neither closes alone (a link now one
/// way, a pin from before the link was rewritten the other). There the links alone
/// decide, since the approval made again pins the links as they are now and its old
/// pins are what it replaces.
///
/// [`Cause::InCycle`]: super::freshness::Cause::InCycle
pub fn upstream_first(shelf: &Shelf, ledger: &Ledger, among: &BTreeSet<DocId>) -> Vec<DocId> {
    let mut links: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
    let mut both: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
    for id in among {
        let footing = Footing::of(shelf, *id, ledger);
        let linked: BTreeSet<DocId> = footing.linked().intersection(among).copied().collect();
        let stood_on: BTreeSet<DocId> = footing.docs().intersection(among).copied().collect();
        both.insert(*id, stood_on);
        links.insert(*id, linked);
    }
    components(&both)
        .into_iter()
        .flat_map(|component| {
            if component.len() == 1 {
                return component;
            }
            let members: BTreeSet<DocId> = component.iter().copied().collect();
            let within: BTreeMap<DocId, BTreeSet<DocId>> = members
                .iter()
                .map(|id| {
                    let targets = links
                        .get(id)
                        .map(|t| t.intersection(&members).copied().collect())
                        .unwrap_or_default();
                    (*id, targets)
                })
                .collect();
            components(&within).into_iter().flatten().collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::fixtures_tests::{
        approve, approved, docid, fr, fr_titled, ledger, shelf,
    };

    fn ids(ns: &[u128]) -> BTreeSet<DocId> {
        ns.iter().map(|n| docid(*n)).collect()
    }

    fn position(order: &[DocId], n: u128) -> usize {
        order.iter().position(|id| *id == docid(n)).unwrap()
    }

    #[test]
    fn a_chain_comes_upstream_first_whatever_the_identities() {
        // FR-3 is the most upstream, FR-1 the most downstream.
        let shelf = shelf(vec![fr(3, &[]), fr(2, &["FR-3"]), fr(1, &["FR-2"])]);
        let ledger = approved(&shelf, &[3, 2, 1]);
        let order = upstream_first(&shelf, &ledger, &ids(&[1, 2, 3]));
        assert_eq!(order, vec![docid(3), docid(2), docid(1)]);
    }

    #[test]
    fn a_diamond_comes_after_both_of_its_sides() {
        let shelf = shelf(vec![
            fr(4, &["FR-2", "FR-3"]),
            fr(2, &["FR-1"]),
            fr(3, &["FR-1"]),
            fr(1, &[]),
        ]);
        let ledger = ledger(Vec::new());
        let order = upstream_first(&shelf, &ledger, &ids(&[1, 2, 3, 4]));
        assert_eq!(order.len(), 4);
        assert!(position(&order, 1) < position(&order, 2));
        assert!(position(&order, 1) < position(&order, 3));
        assert!(position(&order, 2) < position(&order, 4));
        assert!(position(&order, 3) < position(&order, 4));
    }

    #[test]
    fn documents_outside_the_set_are_left_out() {
        let shelf = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
        let ledger = approved(&shelf, &[1, 2, 3]);
        let order = upstream_first(&shelf, &ledger, &ids(&[3, 1]));
        assert_eq!(order, vec![docid(1), docid(3)]);
        assert!(upstream_first(&shelf, &ledger, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn the_pins_of_an_approval_count_as_what_it_stands_on() {
        // FR-2 stood on FR-5 when it was approved, and links to nothing now.
        let then = shelf(vec![fr(5, &[]), fr(2, &["FR-5"])]);
        let ledger = approved(&then, &[5, 2]);
        let now = shelf(vec![fr_titled(5, "edited", &[]), fr(2, &[])]);
        let order = upstream_first(&now, &ledger, &ids(&[2, 5]));
        assert_eq!(order, vec![docid(5), docid(2)]);
    }

    #[test]
    fn a_pending_or_left_out_upstream_holds_an_approval_back_and_a_stale_one_does_not() {
        use crate::domain::shelf::freshness::assess_unasked as assess;
        use crate::domain::shelf::ledger::JudgedAt;
        let then = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &[])]);
        let ledger = approved(&then, &[1, 2]);
        // FR-1 is edited (stale); FR-2 now also stands on FR-3, which is pending.
        let now = shelf(vec![
            fr_titled(1, "edited", &[]),
            fr(2, &["FR-1", "FR-3"]),
            fr(3, &[]),
        ]);
        let assessment = assess(&now, &ledger, JudgedAt::new(100));
        let none = BTreeSet::new();
        // The run of `--stale`: the two stale ones.
        let run = ids(&[1, 2]);
        let held = held_back(&now, &assessment, docid(2), &run, &none).unwrap();
        assert_eq!((held.upstream, held.hold), (docid(3), Hold::Pending));
        assert_eq!(held.link.as_str(), "base");
        assert_eq!(held_back(&now, &assessment, docid(1), &run, &none), None);
        // FR-3 pending in the run holds nothing back; FR-1 stale outside it does.
        let with_pending = ids(&[2, 3]);
        let held = held_back(&now, &assessment, docid(2), &with_pending, &none).unwrap();
        assert_eq!((held.upstream, held.hold), (docid(1), Hold::Stale));
        let all = ids(&[1, 2, 3]);
        assert_eq!(held_back(&now, &assessment, docid(2), &all, &none), None);

        let without_pending = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
        let assessment = assess(&without_pending, &ledger, JudgedAt::new(100));
        assert_eq!(
            held_back(&without_pending, &assessment, docid(2), &run, &none),
            None
        );
        let held = held_back(&without_pending, &assessment, docid(2), &run, &ids(&[1])).unwrap();
        assert_eq!((held.upstream, held.hold), (docid(1), Hold::LeftOut));
    }

    /// FR-4, whose links `base` and `also` name these, in the schema of two links. A
    /// document holds its links by name, so `also` is read before `base`.
    fn on_two_links(base: &[&str], also: &[&str]) -> crate::domain::shelf::build::DocReading {
        use crate::domain::shelf::fixtures_tests::{list, reading, text};
        reading(
            "findings/fr-4.md",
            4,
            "FR-4",
            &[
                ("title", text("t")),
                ("base", list(base)),
                ("also", list(also)),
            ],
        )
    }

    #[test]
    fn holds_tells_of_every_link_and_target_in_order_and_held_back_is_the_first() {
        use crate::domain::shelf::fixtures_tests::{schema_of_two_links, shelf_of};
        use crate::domain::shelf::freshness::assess_unasked as assess;
        use crate::domain::shelf::ledger::JudgedAt;
        // FR-1 approved; FR-2 and FR-3 pending, outside the run: the second target of
        // `base` and the one target of `also` each hold FR-4 back.
        let s = shelf_of(
            &schema_of_two_links(),
            vec![
                fr(1, &[]),
                fr(2, &[]),
                fr(3, &[]),
                on_two_links(&["FR-1", "FR-2"], &["FR-3"]),
            ],
        );
        let approvals = approved(&s, &[1]);
        let assessment = assess(&s, &approvals, JudgedAt::new(100));
        let run = ids(&[4]);
        let none = BTreeSet::new();
        let all = holds(&s, &assessment, docid(4), &run, &none);
        let told: Vec<(&str, DocId, Hold)> = all
            .iter()
            .map(|held| (held.link.as_str(), held.upstream, held.hold))
            .collect();
        assert_eq!(
            told,
            [
                ("also", docid(3), Hold::Pending),
                ("base", docid(2), Hold::Pending),
            ]
        );
        assert_eq!(
            held_back(&s, &assessment, docid(4), &run, &none).as_ref(),
            all.first()
        );
        // In the run, neither holds anything back; and no valid document, no hold.
        assert!(holds(&s, &assessment, docid(4), &ids(&[2, 3, 4]), &none).is_empty());
        assert!(holds(&s, &assessment, docid(77), &run, &none).is_empty());
    }

    #[test]
    fn a_refused_upstream_on_one_link_does_not_hide_a_pending_one_on_the_next() {
        use crate::domain::shelf::fixtures_tests::{deny, schema_of_two_links, shelf_of};
        use crate::domain::shelf::freshness::assess_unasked as assess;
        use crate::domain::shelf::ledger::JudgedAt;
        // `also` (read first, by name) names FR-1, denied; `base` names FR-2, pending.
        let s = shelf_of(
            &schema_of_two_links(),
            vec![fr(1, &[]), fr(2, &[]), on_two_links(&["FR-2"], &["FR-1"])],
        );
        let decided = ledger(vec![deny(&s, 1, 9, 20)]);
        let assessment = assess(&s, &decided, JudgedAt::new(100));
        let run = ids(&[4]);
        let none = BTreeSet::new();
        let held = held_back(&s, &assessment, docid(4), &run, &none).unwrap();
        assert_eq!(
            (held.link.as_str(), held.upstream, held.hold),
            ("base", docid(2), Hold::Pending)
        );
        assert_eq!(holds(&s, &assessment, docid(4), &run, &none), vec![held]);
    }

    #[test]
    fn a_pending_document_is_one_valid_with_no_decision_standing_withdrawn_or_never_made() {
        use crate::domain::nonempty::{NonBlank, NonEmptySet};
        use crate::domain::shelf::fixtures_tests::{by, deny, jid};
        use crate::domain::shelf::freshness::assess_unasked as assess;
        use crate::domain::shelf::ledger::{JudgedAt, Record, Revocation};
        // FR-1 never decided; FR-2 approved then withdrawn; FR-3 approved; FR-4
        // denied; FR-5 has no valid file.
        let docs = shelf(vec![
            fr(1, &[]),
            fr(2, &[]),
            fr(3, &[]),
            fr(4, &[]),
            fr(5, &[]),
        ]);
        let mut records: Vec<Record> = Vec::new();
        for n in [2, 3] {
            records.push(approve(
                &docs,
                &ledger(records.clone()),
                n,
                1000 + n,
                10,
                None,
            ));
        }
        records.push(Record::Revocation(Revocation::new(
            jid(1),
            NonEmptySet::from_set(BTreeSet::from([jid(1002)])).unwrap(),
            by(),
            JudgedAt::new(20),
            NonBlank::new("not yet").unwrap(),
        )));
        records.push(deny(&docs, 4, 2, 30));
        let ledger = ledger(records);
        let mut untitled =
            crate::domain::shelf::fixtures_tests::reading("findings/fr-5.md", 5, "FR-5", &[]);
        if let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &mut untitled {
            entries.remove("title");
        }
        let now = shelf(vec![
            fr(1, &[]),
            fr(2, &[]),
            fr(3, &[]),
            fr(4, &[]),
            untitled,
        ]);
        let assessment = assess(&now, &ledger, JudgedAt::new(100));

        assert_eq!(pending_of(&now, &assessment, docid(1)), Ok(()));
        assert_eq!(pending_of(&now, &assessment, docid(2)), Ok(()));
        assert_eq!(
            pending_of(&now, &assessment, docid(3)),
            Err(NotPending::Decided(DecidedKind::Approved(
                FreshnessLevel::Fresh
            )))
        );
        assert_eq!(
            pending_of(&now, &assessment, docid(4)),
            Err(NotPending::Decided(DecidedKind::Denied))
        );
        assert_eq!(assessment.standing_of(docid(5)), Standing::Pending);
        assert_eq!(
            pending_of(&now, &assessment, docid(5)),
            Err(NotPending::NotValid)
        );
        assert_eq!(pending(&now, &assessment), ids(&[1, 2]));
    }

    #[test]
    fn the_pending_upstream_of_a_document_is_reached_through_depends_links_only() {
        use crate::domain::shelf::freshness::assess_unasked as assess;
        use crate::domain::shelf::ledger::JudgedAt;
        // FR-4 -> FR-3 (approved) -> FR-2 -> FR-1, and FR-6 apart; all pending but FR-3.
        let docs = shelf(vec![
            fr(1, &[]),
            fr(2, &["FR-1"]),
            fr(3, &["FR-2"]),
            fr(4, &["FR-3"]),
            fr(6, &[]),
        ]);
        let ledger = approved(&docs, &[3]);
        let assessment = assess(&docs, &ledger, JudgedAt::new(100));
        assert_eq!(
            pending_upstream(&docs, &assessment, &ids(&[4])),
            ids(&[1, 2, 4])
        );
        assert_eq!(
            pending_upstream(&docs, &assessment, &ids(&[2])),
            ids(&[1, 2])
        );
        assert_eq!(pending(&docs, &assessment), ids(&[1, 2, 4, 6]));
        // FR-4 and FR-6 are stood on by no other of the set; FR-1, FR-2 and FR-3 are.
        assert_eq!(heads(&docs, &ids(&[1, 2, 3, 4, 6])), ids(&[4, 6]));
        assert_eq!(heads(&docs, &ids(&[1, 2])), ids(&[2]));
    }

    #[test]
    fn the_stale_rule_takes_the_expired_and_the_stale_and_tells_the_broken() {
        use crate::domain::shelf::fixtures_tests::deny;
        use crate::domain::shelf::freshness::assess_unasked as assess;
        use crate::domain::shelf::ledger::JudgedAt;
        // FR-1 edited (FR-1 and FR-2 stale); FR-3 expired; FR-5 on FR-4, which is
        // denied after both were approved (FR-5 broken); FR-6 fresh; FR-7 pending.
        let then = shelf(vec![
            fr(1, &[]),
            fr(2, &["FR-1"]),
            fr(3, &[]),
            fr(4, &[]),
            fr(5, &["FR-4"]),
            fr(6, &[]),
            fr(7, &[]),
        ]);
        let mut records = Vec::new();
        for (n, expires) in [
            (1, None),
            (2, None),
            (3, Some(50)),
            (4, None),
            (5, None),
            (6, None),
        ] {
            records.push(approve(
                &then,
                &ledger(records.clone()),
                n,
                1000 + n,
                10,
                expires,
            ));
        }
        records.push(deny(&then, 4, 8, 20));
        let now = shelf(vec![
            fr_titled(1, "edited", &[]),
            fr(2, &["FR-1"]),
            fr(3, &[]),
            fr(4, &[]),
            fr(5, &["FR-4"]),
            fr(6, &[]),
            fr(7, &[]),
        ]);
        let ledger = ledger(records);
        let assessment = assess(&now, &ledger, JudgedAt::new(100));
        let choice = stale(&assessment);
        assert_eq!(choice.again, ids(&[1, 2, 3]));
        let broken: Vec<DocId> = choice.broken.iter().map(|(id, _)| *id).collect();
        assert_eq!(broken, vec![docid(5)]);
    }

    #[test]
    fn a_ring_only_links_and_pins_together_close_is_ordered_by_the_links() {
        // FR-2 was approved on FR-1; now FR-1 links to FR-2 and FR-2 to nothing.
        let then = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
        let first = approve(&then, &ledger(Vec::new()), 1, 101, 10, None);
        let second = approve(&then, &ledger(vec![first.clone()]), 2, 102, 10, None);
        let ledger = ledger(vec![first, second]);
        let now = shelf(vec![fr(1, &["FR-2"]), fr(2, &[])]);
        let order = upstream_first(&now, &ledger, &ids(&[1, 2]));
        assert_eq!(order, vec![docid(2), docid(1)]);
    }

    #[test]
    fn the_refused_upstreams_are_those_linked_a_decision_against_stands_about() {
        use crate::domain::nonempty::{NonBlank, NonEmptySet};
        use crate::domain::shelf::build::DocReading;
        use crate::domain::shelf::fixtures_tests::{
            by, deny, deprecate, jid, list, reading, retire, schema_of_two_links, shelf_of, text,
        };
        use crate::domain::shelf::ledger::{JudgedAt, Record, Revocation};
        // FR-1 pending, FR-2 approved, FR-3 denied, FR-4 deprecated, FR-5 retired, FR-6
        // denied and the denial revoked; FR-7 denied, and only pinned by FR-10's
        // approval. FR-10 links to all but FR-7, and to FR-3 on both of its links.
        let ups: Vec<DocReading> = (1..=7).map(|n| fr(n, &[])).collect();
        let on = |base: &[&str]| {
            reading(
                "findings/fr-10.md",
                10,
                "FR-10",
                &[
                    ("title", text("t")),
                    ("base", list(base)),
                    ("also", list(&["FR-3"])),
                ],
            )
        };
        let schema = schema_of_two_links();
        let mut then_docs = ups.clone();
        then_docs.push(on(&["FR-7"]));
        let then = shelf_of(&schema, then_docs);
        let mut records: Vec<Record> = Vec::new();
        for n in [2, 7, 10] {
            records.push(approve(
                &then,
                &ledger(records.clone()),
                n,
                1000 + n,
                10,
                None,
            ));
        }
        records.push(deny(&then, 3, 3, 20));
        records.push(deprecate(&then, 4, 4, 20));
        records.push(retire(&then, 5, 5, 20));
        records.push(deny(&then, 6, 6, 20));
        records.push(Record::Revocation(Revocation::new(
            jid(16),
            NonEmptySet::from_set(BTreeSet::from([jid(6)])).unwrap(),
            by(),
            JudgedAt::new(30),
            NonBlank::new("not so").unwrap(),
        )));
        records.push(deny(&then, 7, 7, 20));
        let mut now_docs = ups;
        now_docs.push(on(&["FR-1", "FR-2", "FR-3", "FR-4", "FR-5", "FR-6"]));
        let now = shelf_of(&schema, now_docs);
        let ledger = ledger(records);
        let footing = Footing::of(&now, docid(10), &ledger);
        assert!(footing.docs().contains(&docid(7)), "FR-7 is pinned");
        assert_eq!(
            refused_upstreams(&ledger, &footing),
            BTreeMap::from([
                (docid(3), NotStanding::Denied),
                (docid(4), NotStanding::Deprecated),
                (docid(5), NotStanding::Retired),
            ])
        );
        // No valid document, no footing linked: nothing refused.
        let none = Footing::of(&now, docid(77), &ledger);
        assert!(refused_upstreams(&ledger, &none).is_empty());
    }
}
