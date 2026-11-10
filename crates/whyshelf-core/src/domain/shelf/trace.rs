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
//! the trail breaks. A link to an artifact is an edge upstream that ends there: the
//! artifact links to nothing, and nothing is walked down from it.
//!
//! [`linkage`] asks of the same edges, one step and for every document at once,
//! whether a document is linked at all, each way: whether a link reaches it, and
//! whether it holds one. It reads where each document stands from an
//! [`Assessment`], and the declaration from the shelf's schema. Its rules are on
//! [`Linkage`].

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;

use super::artifact::Artifact;
use super::build::Shelf;
use super::doc::{LinkState, UnresolvedLink};
use super::freshness::Assessment;
use super::schema::{BookName, EntryName, LinkKind, Propagation};

/// What the far end of a link comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEnd {
    /// A document.
    Doc(DocId),
    /// A key that cannot be followed.
    Unresolved(UnresolvedLink),
    /// An artifact, there, not there, or not told now. The walk ends at it: an
    /// artifact links to nothing.
    Artifact(Artifact),
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

/// Whether a valid document is linked at all, each way, and whether its book's
/// declaration lets it be: the one statement of what `unlinked` answers.
///
/// The two facts of the document are read from the links the documents hold now,
/// of every meaning (`depends`, `informs`, `replaces`), into any book, whatever the
/// declaration says about `cover`:
///
/// | a link that `holder` holds | `incoming` of what it names | `outgoing` of `holder` |
/// |---|---|---|
/// | resolves to another valid document, and `holder` [stands](super::freshness::Standing::stands) | yes | yes |
/// | resolves to another valid document, and `holder` is decided against (denied, deprecated, retired) | no | yes |
/// | resolves to `holder` itself | no | yes |
/// | cannot be followed (a key that names no valid document) | none is reached | yes |
/// | to artifacts, with a name written | none is reached | yes |
///
/// A link with nothing written in it is no link. As for coverage, a document
/// decided against reaches nothing, so it cannot hide a document that stands. A file
/// that is no valid document holds no link and is not asked about: the shelf keeps
/// it apart ([`Shelf::invalid`], [`Shelf::unreadable`]).
///
/// The two facts of the book are read from the checked schema alone, and count
/// every declared link but `replaces`: a successor's link is absent from every
/// original by nature.
///
/// - `reachable`: a link some book declares, its own included, not `replaces`,
///   points into it.
/// - `may_hold`: it declares a link that is not `replaces`, to artifacts or into a
///   book, its own included.
///
/// A book that is neither is cut off by its declaration, and what it holds is
/// inevitable. The answers are disjoint:
///
/// | answer | when |
/// |---|---|
/// | [`isolated`](Self::isolated) | neither `incoming` nor `outgoing`, and the book is `reachable` or `may_hold` |
/// | [`unreached`](Self::unreached) | not `incoming` but `outgoing`, and the book is `reachable` |
/// | [`holds_none`](Self::holds_none) | not `outgoing` but `incoming`, and the book `may_hold` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Linkage {
    /// A link held by another valid document that stands resolves to it.
    pub incoming: bool,
    /// It holds a link to anything.
    pub outgoing: bool,
    /// A declared link that is not `replaces`, of any book, points into its book.
    pub reachable: bool,
    /// Its book declares a link that is not `replaces`.
    pub may_hold: bool,
}

impl Linkage {
    /// It is linked neither way, though its declaration lets it be one way at least.
    pub fn isolated(&self) -> bool {
        (self.reachable || self.may_hold) && !self.incoming && !self.outgoing
    }

    /// It holds a link but no link reaches it, though the declaration lets one.
    pub fn unreached(&self) -> bool {
        self.reachable && !self.incoming && self.outgoing
    }

    /// A link reaches it but it holds none, though its book declares some.
    pub fn holds_none(&self) -> bool {
        self.may_hold && !self.outgoing && self.incoming
    }
}

/// The [`Linkage`] of every valid document of the shelf, with where each stands
/// from `assessment`.
pub fn linkage(shelf: &Shelf, assessment: &Assessment) -> BTreeMap<DocId, Linkage> {
    let schema = shelf.schema();
    let expected = |kind: &&LinkKind| kind.propagation != Propagation::Replaces;
    let reachable: BTreeSet<&BookName> = schema
        .books()
        .values()
        .flat_map(|book| {
            book.links()
                .into_iter()
                .map(|(_, kind)| kind)
                .filter(expected)
                .map(|kind| &kind.target)
        })
        .collect();
    let may_hold: BTreeSet<&BookName> = schema
        .books()
        .iter()
        .filter(|(_, book)| {
            book.artifact_links().into_iter().next().is_some()
                || book
                    .links()
                    .into_iter()
                    .map(|(_, kind)| kind)
                    .any(|k| expected(&k))
        })
        .map(|(name, _)| name)
        .collect();
    let mut all: BTreeMap<DocId, Linkage> = shelf
        .docs()
        .iter()
        .map(|(id, doc)| {
            let book = &doc.key().book;
            let linkage = Linkage {
                reachable: reachable.contains(book),
                may_hold: may_hold.contains(book),
                ..Linkage::default()
            };
            (*id, linkage)
        })
        .collect();
    for id in shelf.docs().keys() {
        let edges = out_edges(shelf, *id);
        if let Some(own) = all.get_mut(id) {
            own.outgoing = !edges.is_empty();
        }
        if !assessment.standing_of(*id).stands() {
            continue;
        }
        for edge in edges {
            if let LinkEnd::Doc(target) = edge.end
                && target != edge.holder
                && let Some(reached) = all.get_mut(&target)
            {
                reached.incoming = true;
            }
        }
    }
    all
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
    for (name, link) in doc.artifact_links() {
        for artifact in link.artifacts() {
            edges.push(LinkEdge {
                holder: id,
                link: name.clone(),
                propagation: link.kind().propagation.propagation(),
                end: LinkEnd::Artifact(artifact.clone()),
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
        LinkEnd::Unresolved(_) | LinkEnd::Artifact(_) => None,
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
mod linkage_tests;

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
    fn a_link_to_an_artifact_is_an_edge_upstream_that_ends_there() {
        use crate::domain::shelf::artifact::{ArtifactHash, ArtifactState};
        use crate::domain::shelf::fixtures_tests::{shelf_with, sp_testing};
        let hash = ArtifactHash::of_content(b"t");
        let s = shelf_with(
            vec![
                fr(1, &[]),
                sp_testing(2, &["FR-1"], &["tests/a.rs", "tests/b.rs"]),
            ],
            &[("tests/a.rs", Some(hash))],
        );
        let up = trace(&s, docid(2)).upstream;
        let artifacts: Vec<(&str, ArtifactState, Propagation)> = up
            .iter()
            .filter_map(|e| match &e.end {
                LinkEnd::Artifact(a) => Some((a.name().as_str(), a.state().clone(), e.propagation)),
                _ => None,
            })
            .collect();
        assert_eq!(
            artifacts,
            vec![
                (
                    "tests/a.rs",
                    ArtifactState::Present(hash),
                    Propagation::Depends
                ),
                ("tests/b.rs", ArtifactState::Absent, Propagation::Depends),
            ]
        );
        assert_eq!(up.len(), 3);
        assert!(
            trace(&s, docid(1))
                .downstream
                .iter()
                .all(|e| e.holder == docid(2))
        );
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
                LinkEnd::Unresolved(_) | LinkEnd::Artifact(_) => panic!("every link resolves"),
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
                LinkEnd::Unresolved(_) | LinkEnd::Artifact(_) => {
                    panic!("downstream ends are documents")
                }
            })
            .collect();
        assert_eq!(down.len(), 4);
        assert_eq!(down.iter().collect::<BTreeSet<_>>().len(), 4);
        assert_eq!(down.iter().filter(|(h, _)| *h == docid(4)).count(), 2);
    }
}
