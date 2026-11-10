//! The graphs of a shelf taken apart: their components, and the cycles in them.
//!
//! Two graphs of documents are searched for cycles: the `Depends` links the shelf
//! holds now ([`Shelf::build`]) and the frames of the approvals that stand
//! (`Ledger::frame_graph`). Both are a map from a document to what it stands on,
//! and both are taken apart here, once: `cycles_of` is the one place a cycle is
//! found, and `components` the one place the order upstream first is made (both
//! visible to the shelf's modules only).
//!
//! ```text
//!   Shelf::build ─────────┐                      ┌──► Cycle (CycleSource::Links)
//!                         ├──► components ──► cycles_of
//!   Ledger::frame_graph ──┘          │           └──► Cycle (CycleSource::Frames)
//!                                    └──► the order freshness travels (frames)
//! ```
//!
//! [`Shelf::build`]: super::build::Shelf::build

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::nonempty::TwoOrMore;

/// Which graph a cycle was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CycleSource {
    /// The `Depends` links the shelf holds now.
    Links,
    /// The frames of the approvals in the ledger (the `FrameGraph` of the ledger
    /// module).
    Frames,
}

/// The identity of a cycle: where it was found and the smallest identifier in it.
///
/// The source is part of it, so that two cycles of different graphs that share a
/// smallest document are two ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CycleId {
    /// The graph.
    pub source: CycleSource,
    /// The smallest identifier among the members.
    pub min: DocId,
}

/// The documents of a cycle: one that stands on itself, or two or more that stand
/// on one another, smallest first. Private, so a cycle cannot be made with members
/// its graph does not have; [`Cycle::ids`] is how they are read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CycleMembers {
    SelfLoop(DocId),
    Ring(TwoOrMore<DocId>),
}

/// A cycle among documents, made by `cycles_of` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    id: CycleId,
    members: CycleMembers,
}

impl Cycle {
    /// The identity of the cycle.
    pub fn id(&self) -> CycleId {
        self.id
    }

    /// Whether `id` is in the cycle.
    pub fn contains(&self, id: DocId) -> bool {
        self.ids().contains(&id)
    }

    /// The members, smallest first.
    pub fn ids(&self) -> Vec<DocId> {
        match &self.members {
            CycleMembers::SelfLoop(m) => vec![*m],
            CycleMembers::Ring(ms) => ms.iter().copied().collect(),
        }
    }
}

/// The strongly connected components of a directed graph, in an order where a
/// component comes after every component its members point at.
///
/// An edge points from a document to what it stands on, so the order is upstream
/// first. The walk keeps its own stack, so a long chain of documents cannot overflow
/// the call stack.
pub(super) fn components(edges: &BTreeMap<DocId, BTreeSet<DocId>>) -> Vec<Vec<DocId>> {
    let mut nodes: BTreeSet<DocId> = edges.keys().copied().collect();
    for targets in edges.values() {
        nodes.extend(targets.iter().copied());
    }
    let ids: Vec<DocId> = nodes.into_iter().collect();
    let index_of: BTreeMap<DocId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let adjacent: Vec<Vec<usize>> = ids
        .iter()
        .map(|id| {
            edges
                .get(id)
                .map(|targets| {
                    targets
                        .iter()
                        .filter_map(|t| index_of.get(t).copied())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();

    let n = ids.len();
    let unvisited = usize::MAX;
    let mut order = vec![unvisited; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0usize;
    let mut out: Vec<Vec<DocId>> = Vec::new();

    for root in 0..n {
        if order[root] != unvisited {
            continue;
        }
        order[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        let mut calls: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(v, i)) = calls.last() {
            if let Some(&w) = adjacent[v].get(i) {
                if let Some(top) = calls.last_mut() {
                    top.1 += 1;
                }
                if order[w] == unvisited {
                    order[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    calls.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(order[w]);
                }
            } else {
                calls.pop();
                if low[v] == order[v] {
                    let mut component = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        component.push(ids[w]);
                        if w == v {
                            break;
                        }
                    }
                    component.sort();
                    out.push(component);
                }
                if let Some(&(parent, _)) = calls.last() {
                    low[parent] = low[parent].min(low[v]);
                }
            }
        }
    }
    out
}

/// Every cycle of a graph: each component of two or more, and each document that
/// points at itself. Ordered by identity.
pub(super) fn cycles_of(
    source: CycleSource,
    edges: &BTreeMap<DocId, BTreeSet<DocId>>,
) -> Vec<Cycle> {
    let mut cycles: Vec<Cycle> = components(edges)
        .into_iter()
        .filter_map(|component| {
            let mut ids = component.into_iter();
            let first = ids.next()?;
            let members = match ids.next() {
                Some(second) => CycleMembers::Ring(TwoOrMore::new(first, second, ids.collect())),
                None if edges.get(&first).is_some_and(|t| t.contains(&first)) => {
                    CycleMembers::SelfLoop(first)
                }
                None => return None,
            };
            Some(Cycle {
                id: CycleId { source, min: first },
                members,
            })
        })
        .collect();
    cycles.sort_by_key(Cycle::id);
    cycles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::fixtures_tests::docid;

    fn edges(pairs: &[(u128, &[u128])]) -> BTreeMap<DocId, BTreeSet<DocId>> {
        pairs
            .iter()
            .map(|(from, to)| (docid(*from), to.iter().map(|t| docid(*t)).collect()))
            .collect()
    }

    #[test]
    fn components_come_upstream_first() {
        // 3 stands on 2, 2 stands on 1; 4 on 3 and 1.
        let graph = edges(&[(3, &[2]), (2, &[1]), (4, &[3, 1])]);
        let order: Vec<DocId> = components(&graph).into_iter().flatten().collect();
        let position = |n: u128| order.iter().position(|id| *id == docid(n)).unwrap();
        assert_eq!(order.len(), 4);
        assert!(position(1) < position(2));
        assert!(position(2) < position(3));
        assert!(position(3) < position(4));
    }

    #[test]
    fn a_cycle_is_one_component_and_its_upstream_comes_before_it() {
        let graph = edges(&[(1, &[2]), (2, &[1, 3]), (3, &[])]);
        let all = components(&graph);
        assert_eq!(all, vec![vec![docid(3)], vec![docid(1), docid(2)]]);
    }

    #[test]
    fn cycles_of_finds_rings_and_self_loops_and_names_them_by_source() {
        let graph = edges(&[(1, &[2]), (2, &[3]), (3, &[1]), (4, &[4]), (5, &[1])]);
        let cycles = cycles_of(CycleSource::Frames, &graph);
        assert_eq!(cycles.len(), 2);
        assert_eq!(
            cycles[0].id(),
            CycleId {
                source: CycleSource::Frames,
                min: docid(1)
            }
        );
        assert_eq!(cycles[0].ids(), vec![docid(1), docid(2), docid(3)]);
        assert!(matches!(cycles[0].members, CycleMembers::Ring(_)));
        assert_eq!(cycles[1].ids(), vec![docid(4)]);
        assert!(matches!(cycles[1].members, CycleMembers::SelfLoop(_)));
        // Two cycles of different graphs on one document are two ids.
        let links = cycles_of(CycleSource::Links, &graph);
        assert_ne!(links[0].id(), cycles[0].id());
    }

    #[test]
    fn a_very_long_chain_does_not_overflow_the_stack() {
        let n = 20_000u128;
        let graph: BTreeMap<DocId, BTreeSet<DocId>> = (2..=n)
            .map(|i| (docid(i), BTreeSet::from([docid(i - 1)])))
            .collect();
        assert!(cycles_of(CycleSource::Links, &graph).is_empty());
        assert_eq!(components(&graph).len(), usize::try_from(n).unwrap());
    }
}
