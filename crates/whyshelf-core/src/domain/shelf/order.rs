//! Approving a set of documents in one run: which are taken, by two rules, one owner
//! each ([`stale`] for `judge approve --stale`, [`pending`] and [`pending_upstream`]
//! for `--pending`, both read from one [`Assessment`]); in which order
//! ([`upstream_first`]); and which upstream holds one back ([`held_back`]). Each rule
//! is on its function. No order of books is read: the order comes from what each
//! document stands on.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::id::DocId;

use super::build::Shelf;
use super::doc::LinkState;
use super::freshness::{
    Assessment, Degradation, Freshness, FreshnessLevel, Severity, Standing, StandingKind,
};
use super::graph::components;
use super::ledger::{DeclaredStatus, FramePin, Ledger};
use super::schema::{EntryName, Propagation};

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
/// pending, decided against) is not looked at.
pub fn stale(assessment: &Assessment) -> StaleChoice {
    let mut choice = StaleChoice {
        again: BTreeSet::new(),
        broken: Vec::new(),
    };
    for (id, standing) in assessment.standings() {
        let Standing::Approved(Freshness::Degraded(degradation)) = standing else {
            continue;
        };
        match degradation.level() {
            Severity::Expired | Severity::Stale => {
                choice.again.insert(*id);
            }
            Severity::Broken => choice.broken.push((*id, degradation.clone())),
        }
    }
    choice
}

/// The approvals a run of `judge approve --stale` alone would make again, as far as
/// it can be told before the run: those [`stale`] takes again that no upstream holds
/// back ([`held_back`]), taken upstream first ([`upstream_first`]) as the run takes
/// them, so that one standing on a pending upstream, which `--stale` does not
/// approve, holds back what stands on it in turn. One the run leaves out for what
/// only the run finds (its frame refused, a repeat) is not foreseen.
pub fn stale_taken(shelf: &Shelf, ledger: &Ledger, assessment: &Assessment) -> BTreeSet<DocId> {
    let run = stale(assessment).again;
    let mut left_out = BTreeSet::new();
    let mut taken = BTreeSet::new();
    for id in upstream_first(shelf, ledger, &run) {
        if held_back(shelf, assessment, id, &run, &left_out).is_some() {
            left_out.insert(id);
        } else {
            taken.insert(id);
        }
    }
    taken
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
/// says nothing of it; nor is one approved, denied, deprecated or retired.
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

/// What the `Depends` links of a document resolve to now; nothing for an identity
/// with no valid document. A link that cannot be followed leads nowhere.
fn depends_targets(shelf: &Shelf, id: DocId) -> Vec<DocId> {
    shelf.doc(id).map_or_else(Vec::new, |doc| {
        doc.links_of(Propagation::Depends)
            .into_iter()
            .flat_map(|(_, states)| states.iter())
            .filter_map(|state| match state {
                LinkState::Resolved(target) => Some(target.id()),
                LinkState::Unresolved(_) => None,
            })
            .collect()
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
/// and not in `run`, stale and not in `run`, or in `left_out` (the documents of the
/// same run already left out), in that order of precedence, the links in the order
/// the document holds them. `run` is every document the run takes; one of it is
/// decided before what stands on it ([`upstream_first`]), approved or left out, so
/// one of it left out is told as `LeftOut`, not by its standing.
///
/// An approval made on such an upstream would be broken or stale from the start. An
/// upstream whose approval only expired holds nothing back: an approval on it is a
/// notice, not a staleness. What a decision says against an upstream (denied,
/// deprecated, retired), a link that cannot be followed and a cycle are refusals of
/// the frame ([`frame_approve`](super::ledger::frame_approve)), not this. Standings
/// are read from `assessment`, which the caller makes of the ledger as the approvals
/// already decided in the run left it: an upstream stale only through one the run
/// has approved since holds nothing back.
pub fn held_back(
    shelf: &Shelf,
    assessment: &Assessment,
    id: DocId,
    run: &BTreeSet<DocId>,
    left_out: &BTreeSet<DocId>,
) -> Option<HeldBack> {
    let doc = shelf.doc(id)?;
    doc.links_of(Propagation::Depends)
        .into_iter()
        .flat_map(|(link, states)| states.iter().map(move |state| (link, state)))
        .find_map(|(link, state)| {
            let LinkState::Resolved(target) = state else {
                return None;
            };
            let upstream = target.id();
            let in_run = run.contains(&upstream);
            let hold = match assessment.standing_of(upstream) {
                Standing::Approved(Freshness::Degraded(d)) if d.level() == Severity::Broken => {
                    Some(Hold::Broken)
                }
                Standing::Pending if !in_run => Some(Hold::Pending),
                Standing::Approved(Freshness::Degraded(d))
                    if !in_run && d.level() == Severity::Stale =>
                {
                    Some(Hold::Stale)
                }
                _ if left_out.contains(&upstream) => Some(Hold::LeftOut),
                _ => None,
            }?;
            Some(HeldBack {
                link: link.clone(),
                upstream,
                hold,
            })
        })
}

/// The identities of `among`, each after every one of `among` it stands on.
///
/// A document stands on what its `Depends` links resolve to now ([`Shelf::build`])
/// and on what the approval that stands about it pinned ([`Ledger::status_of`]).
/// Only the documents of `among` are ordered, and only the edges between two of
/// them count: a document outside the set is left out, and no order passes through
/// it. Otherwise the order is upstream first and deterministic: the same set, shelf
/// and ledger give the same order, and the documents with no edge to another of the
/// set come by identity among themselves.
///
/// Every document a `Depends` link of a member names, when it is in `among`, comes
/// before the member; so one pass in this order sees each upstream decided before
/// what stands on it.
///
/// A cycle cannot be judged: a document in a cycle of links or of frames is broken
/// ([`Cause::InCycle`]), and a broken approval is never in a set to approve again, so
/// no cycle of either graph reaches this order. The two graphs together can still
/// close a ring neither closes alone (a link now one way, a pin from before the link
/// was rewritten the other). There the links alone decide, since the approval made
/// again pins the links as they are now and its old pins are what it replaces.
///
/// [`Cause::InCycle`]: super::freshness::Cause::InCycle
pub fn upstream_first(shelf: &Shelf, ledger: &Ledger, among: &BTreeSet<DocId>) -> Vec<DocId> {
    let mut links: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
    let mut both: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
    for id in among {
        let linked: BTreeSet<DocId> = shelf
            .doc(*id)
            .map(|doc| {
                doc.links_of(Propagation::Depends)
                    .into_iter()
                    .flat_map(|(_, states)| states.iter())
                    .filter_map(|state| match state {
                        LinkState::Resolved(target) => Some(target.id()),
                        LinkState::Unresolved(_) => None,
                    })
                    .filter(|target| among.contains(target))
                    .collect()
            })
            .unwrap_or_default();
        let pinned: BTreeSet<DocId> = match ledger.status_of(*id) {
            DeclaredStatus::Approved { frame, .. } => frame
                .iter()
                .map(FramePin::target)
                .filter(|target| among.contains(target))
                .collect(),
            _ => BTreeSet::new(),
        };
        both.insert(*id, linked.union(&pinned).copied().collect());
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
        use crate::domain::shelf::freshness::assess;
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

    #[test]
    fn a_pending_document_is_one_valid_with_no_decision_standing_withdrawn_or_never_made() {
        use crate::domain::nonempty::{NonBlank, NonEmptySet};
        use crate::domain::shelf::fixtures_tests::{actor, deny, jid};
        use crate::domain::shelf::freshness::assess;
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
            actor(),
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
        use crate::domain::shelf::freshness::assess;
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
        use crate::domain::shelf::freshness::assess;
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
}
