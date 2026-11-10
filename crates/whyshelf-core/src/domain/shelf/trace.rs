//! What a document links to and what links to it, followed both ways.
//!
//! [`trace`] is the one entry. It follows the links the shelf holds now (not the
//! frames of approvals: that is freshness), every kind of link, transitively, each
//! document expanded once so a cycle ends.
//!
//! ```text
//!   upstream:    start ──link──► target ──link──► ...      (the links each holds)
//!   downstream:  start ◄──link── holder ◄──link── ...      (an index of who points at whom,
//!                                                          made once for the walk)
//! ```
//!
//! A link that cannot be followed is an edge too, upstream, so an answer shows where
//! the trail breaks.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;

use super::build::Shelf;
use super::doc::{LinkState, UnresolvedLink};
use super::schema::{EntryName, Propagation};

/// What the far end of a link comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEnd {
    /// A document.
    Doc(DocId),
    /// A key that cannot be followed.
    Unresolved(UnresolvedLink),
}

/// One link: `holder` holds `link`, which names `end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    /// The document that holds the link.
    pub holder: DocId,
    /// The name of the link.
    pub link: EntryName,
    /// What the link means for freshness.
    pub propagation: Propagation,
    /// What it names.
    pub end: LinkEnd,
    /// How many links from the document the walk started at (1 is a link of its own,
    /// or a link to it).
    pub depth: usize,
}

/// The links around one document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Trace {
    /// What it links to, and what those link to: each edge once.
    pub upstream: Vec<LinkEdge>,
    /// What links to it, and what links to those: each edge once. The `end` of each
    /// is a [`LinkEnd::Doc`], the document linked to.
    pub downstream: Vec<LinkEdge>,
}

/// The links of `start`, up and down. A document that is not valid holds no links,
/// so its trace upstream is empty; it can still be linked to.
pub fn trace(shelf: &Shelf, start: DocId) -> Trace {
    Trace {
        upstream: walk(start, |id| out_edges(shelf, id), far_up),
        downstream: {
            let index = in_index(shelf);
            walk(
                start,
                |id| index.get(&id).cloned().unwrap_or_default(),
                far_down,
            )
        },
    }
}

/// The links a valid document holds, as edges of depth 0, in the order of their
/// names (each link carries its kind, so its meaning is read from it).
fn out_edges(shelf: &Shelf, id: DocId) -> Vec<LinkEdge> {
    let Some(doc) = shelf.doc(id) else {
        return Vec::new();
    };
    let mut edges = Vec::new();
    for (name, link) in doc.links() {
        for state in link.states() {
            edges.push(LinkEdge {
                holder: id,
                link: name.clone(),
                propagation: link.kind().propagation,
                end: match state {
                    LinkState::Resolved(target) => LinkEnd::Doc(target.id()),
                    LinkState::Unresolved(u) => LinkEnd::Unresolved(u.clone()),
                },
                depth: 0,
            });
        }
    }
    edges
}

/// The far end of an edge walked upstream: the document it names, if it names one.
fn far_up(edge: &LinkEdge) -> Option<DocId> {
    match edge.end {
        LinkEnd::Doc(target) => Some(target),
        LinkEnd::Unresolved(_) => None,
    }
}

/// The far end of an edge walked downstream: the document that holds it.
fn far_down(edge: &LinkEdge) -> Option<DocId> {
    Some(edge.holder)
}

/// For every document, the resolved links that name it: made once, so a walk down is
/// as cheap as a walk up.
fn in_index(shelf: &Shelf) -> BTreeMap<DocId, Vec<LinkEdge>> {
    let mut index: BTreeMap<DocId, Vec<LinkEdge>> = BTreeMap::new();
    for id in shelf.docs().keys() {
        for edge in out_edges(shelf, *id) {
            if let LinkEnd::Doc(target) = edge.end {
                index.entry(target).or_default().push(edge);
            }
        }
    }
    index
}

/// A breadth-first walk from `start` over the edges `next` gives for a document,
/// each document expanded once. `far` says which end of an edge the walk goes on
/// to: [`far_up`] or [`far_down`].
fn walk(
    start: DocId,
    next: impl Fn(DocId) -> Vec<LinkEdge>,
    far: fn(&LinkEdge) -> Option<DocId>,
) -> Vec<LinkEdge> {
    let mut edges = Vec::new();
    let mut expanded = BTreeSet::from([start]);
    let mut frontier = vec![start];
    let mut depth = 0;
    while !frontier.is_empty() {
        depth += 1;
        let mut onward = Vec::new();
        for id in frontier {
            for mut edge in next(id) {
                if let Some(far) = far(&edge)
                    && expanded.insert(far)
                {
                    onward.push(far);
                }
                edge.depth = depth;
                edges.push(edge);
            }
        }
        frontier = onward;
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::fixtures_tests::{docid, fr, shelf};

    #[test]
    fn a_chain_is_followed_both_ways_with_depths() {
        let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);

        let from_3 = trace(&s, docid(3));
        let up: Vec<(DocId, usize)> = from_3
            .upstream
            .iter()
            .map(|e| (e.holder, e.depth))
            .collect();
        assert_eq!(up, vec![(docid(3), 1), (docid(2), 2)]);
        assert!(from_3.downstream.is_empty());

        let from_1 = trace(&s, docid(1));
        let down: Vec<(DocId, usize)> = from_1
            .downstream
            .iter()
            .map(|e| (e.holder, e.depth))
            .collect();
        assert_eq!(down, vec![(docid(2), 1), (docid(3), 2)]);
        assert!(from_1.upstream.is_empty());
    }

    #[test]
    fn a_link_that_cannot_be_followed_is_an_edge_upstream() {
        let s = shelf(vec![fr(2, &["FR-9"])]);
        let t = trace(&s, docid(2));
        assert_eq!(t.upstream.len(), 1);
        assert!(matches!(t.upstream[0].end, LinkEnd::Unresolved(_)));
    }

    #[test]
    fn a_diamond_expands_each_document_once_and_lists_each_edge_once() {
        // 4 -> 2, 4 -> 3, 2 -> 1, 3 -> 1
        let s = shelf(vec![
            fr(1, &[]),
            fr(2, &["FR-1"]),
            fr(3, &["FR-1"]),
            fr(4, &["FR-2", "FR-3"]),
        ]);

        let up: Vec<(DocId, DocId, usize)> = trace(&s, docid(4))
            .upstream
            .iter()
            .map(|e| match e.end {
                LinkEnd::Doc(t) => (e.holder, t, e.depth),
                LinkEnd::Unresolved(_) => panic!("every link resolves"),
            })
            .collect();
        assert_eq!(up.len(), 4);
        let unique: BTreeSet<(DocId, DocId)> = up.iter().map(|(h, t, _)| (*h, *t)).collect();
        assert_eq!(unique.len(), 4);
        // FR-1 is reached by two paths, yet FR-2 and FR-3 each hold their one link.
        assert_eq!(up.iter().filter(|(h, _, _)| *h == docid(2)).count(), 1);
        assert_eq!(up.iter().filter(|(h, _, _)| *h == docid(3)).count(), 1);
        assert_eq!(up.iter().filter(|(_, _, d)| *d == 2).count(), 2);

        let down: Vec<(DocId, DocId)> = trace(&s, docid(1))
            .downstream
            .iter()
            .map(|e| match e.end {
                LinkEnd::Doc(t) => (e.holder, t),
                LinkEnd::Unresolved(_) => panic!("downstream ends are documents"),
            })
            .collect();
        assert_eq!(down.len(), 4);
        assert_eq!(down.iter().collect::<BTreeSet<_>>().len(), 4);
        assert_eq!(down.iter().filter(|(h, _)| *h == docid(4)).count(), 2);
    }
}
