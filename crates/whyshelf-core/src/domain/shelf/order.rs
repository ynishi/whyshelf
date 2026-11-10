//! Approving a set of approvals again: in which order ([`upstream_first`]), and which
//! upstream holds one back ([`held_back`]). Each rule is on its function. No order of
//! books is read: the order comes from what each document stands on.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;

use super::build::Shelf;
use super::doc::LinkState;
use super::freshness::{Assessment, Freshness, Severity, Standing};
use super::graph::components;
use super::ledger::{DeclaredStatus, FramePin, Ledger};
use super::schema::{EntryName, Propagation};

/// Why an upstream holds an approval back from being made again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// The upstream's approval is broken.
    Broken,
    /// The upstream was never approved: approve it first.
    Pending,
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

/// The first upstream that keeps `id` from being approved again so that it holds, if
/// one does: a document a `Depends` link of `id` resolves to now that is broken,
/// pending, or in `left_out` (the documents of the same run already left out), in
/// that order of precedence, the links in the order the document holds them.
///
/// An approval made on such an upstream would be broken or stale from the start.
/// What a decision says against an upstream (denied, deprecated, retired), a link
/// that cannot be followed and a cycle are refusals of the frame
/// ([`frame_approve`](super::ledger::frame_approve)), not this. Standings are read
/// from `assessment`, as the project was read.
pub fn held_back(
    shelf: &Shelf,
    assessment: &Assessment,
    id: DocId,
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
            let hold = match assessment.standing_of(upstream) {
                Standing::Approved(Freshness::Degraded(d)) if d.level() == Severity::Broken => {
                    Some(Hold::Broken)
                }
                Standing::Pending => Some(Hold::Pending),
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
        let held = held_back(&now, &assessment, docid(2), &none).unwrap();
        assert_eq!((held.upstream, held.hold), (docid(3), Hold::Pending));
        assert_eq!(held.link.as_str(), "base");
        assert_eq!(held_back(&now, &assessment, docid(1), &none), None);

        let without_pending = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
        let assessment = assess(&without_pending, &ledger, JudgedAt::new(100));
        assert_eq!(
            held_back(&without_pending, &assessment, docid(2), &none),
            None
        );
        let held = held_back(&without_pending, &assessment, docid(2), &ids(&[1])).unwrap();
        assert_eq!((held.upstream, held.hold), (docid(1), Hold::LeftOut));
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
