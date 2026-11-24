//! What does not hold, divided into units of work that share nothing: each unit is a
//! set of documents that can be mended, and approved again, apart from every other
//! unit, so that several people (or agents) can take one each.
//!
//! The documents divided (the *undone*) are the approvals that no longer hold, and with
//! [`UnitsOf::DegradedAndPending`] the pending documents too. Two of them are in one
//! unit when they share something a change of one could touch for the other, and the
//! units are the connected parts of that relation:
//!
//! | rule | what | the one place |
//! |---|---|---|
//! | R1 | the approvals that no longer hold | the assessment's set of degraded approvals, which `--stale`, `stale`, the origins and `check` read too |
//! | R2 | with [`UnitsOf::DegradedAndPending`], every pending document as well | [`order::pending`] |
//! | R3 | two undone documents where one stands on the other (what its `Depends` links resolve to, and what its approval pinned: its footing), or one's `Depends` link names the other, which it cannot stand on (a document that is not valid, or one of the files that write a key two of them write) | the footing of each document, and the documents its links name |
//! | R4 | two undone documents that stand on artifacts that live in one file, whole or in part ([`Placement::files`]) | the adapter of each space ([`Placement`]) |
//! | R5 | two undone documents held back by the same chain of upstreams outside the undone: an upstream outside that holds an undone document back ([`order::held_back`], with every undone document in the run and none left out, for every link and target), and each upstream outside that holds one of the chain back in turn. The chain joins as the undone do (R3, R4, R6). An upstream a new approval may not stand on, and a document outside the undone that a link names and cannot stand on, join the undone and the chain documents whose links resolve to or name it, and are followed no further. Only what an undone document waits on is in its unit's `waits_on` | the order of a run; [`Ledger::may_stand_on`]; the documents a link names |
//! | R6 | an artifact whose place cannot be told joins only the documents that stand on that same name, and is listed in `unplaced`; a key that no document has joins the documents whose links write it, as such a name does, and is not listed; a text that is no key joins nothing | the adapter of its space; the keys a document's links write |
//! | R7 | the file of a document and the declaration are no key | this module |
//! | R8 | the documents of a unit come in the order a run takes them ([`upstream_first`]) | the order of a run |
//! | R9 | the units by the least identity of their documents; their files by path; their artifacts by space and name; what they wait on by identity | this module |
//!
//! [`plan`] reads the undone and the chain of what holds them back once, with what each
//! stands on, what its links name, and the artifacts whose place is to be asked
//! ([`UnitsPlan::asks`]); it counts what could join units without being seen
//! ([`Unjoined`]). The application asks the adapters where those artifacts live, once,
//! at the side after; [`divide`] makes the units of the answers. An artifact the answers
//! do not place is read as one whose place cannot be told, as a name not asked is read as
//! one that cannot be told.
//!
//! Why each rule is so:
//!
//! - R3 reads the targets of a document's `Depends` links as its footing has them, and
//!   the documents a link names as the document has them; this module says neither
//!   again. A link that names a document it cannot stand on joins the two all the same:
//!   the named one is to be mended before the other can be approved.
//! - R5 follows upstreams outside the undone only. The upstreams a run reads are what the
//!   `Depends` links resolve to now, which a footing holds among the documents it stands
//!   on, so an upstream among the undone already joins by R3, and a chain is joined to
//!   what it holds back by R3 too. The hold an upstream gives is read of its standing
//!   with every undone document in the run, so it is the same seen from every document
//!   it holds back. Only a pending upstream (when the pending are not divided) and an
//!   awaiting one can be outside: a stale or broken upstream is undone, and none is left
//!   out. An awaiting upstream holds nothing back in turn (it is a fresh approval, which
//!   stands on nothing pending or awaiting), so a chain grows through pending upstreams
//!   only. A chain is the work to be done before what it holds back can be approved, so
//!   what one of it shares with another document joins as the undone do; it stops at what
//!   holds nothing back: an approval that holds, an upstream a new approval may not stand
//!   on ([`Ledger::may_stand_on`]), and an undone document. A document of a chain is no
//!   document of a unit, its artifacts are not listed in one, and no order passes through
//!   it ([`upstream_first`] orders the undone of a unit alone).
//! - R5, an upstream a new approval may not stand on, and a document outside the undone
//!   that a link names and cannot stand on (not valid, or of a file that writes a key two
//!   files write): the frame of an approval refuses each document that stands on or names
//!   it, and one way to mend that (the decision looked at again or the successor
//!   approved, the file made valid, a key taken again) is work they all share. So each is
//!   joined to every undone document and document of a chain whose links resolve to it
//!   or name it, as its footing and its links have them, and to nothing else: not by what
//!   an approval only pinned, as a pin a link no longer resolves to is no work of the
//!   document. It is followed no further: mending an upstream decided against rewrites
//!   none of what it stands on, and the links of a document not valid cannot be read,
//!   which is counted (below), so joining it changes no count. Whether it is outside the
//!   undone is asked once, of what a document waits on; a join to one in the undone is
//!   one R3 makes too. One upstream is one entry of `waits_on`, of one kind ([`Waits`]):
//!   what a link resolves to is valid and what it cannot be followed to is not; what
//!   holds a document back has no decision against it, and that is all a refused one
//!   has; and the reasons a link cannot be followed to one document are joined, as one
//!   identity can have two files, each writing a key of its own.
//! - R6, a key no document has (no file writes it, or only files whose identity cannot be
//!   read or that have none): making that document is work the documents whose links
//!   write the key share, and two made apart would write one key twice; the file whose
//!   identity cannot be read is not guessed to be any document, and is counted (below). A text
//!   that is no key of its book is one no document can have, so each document mends its
//!   own link, and it joins nothing.
//! - R7: a document is the one document of its file, and a change of the declaration
//!   reaches every document, so as a key it would make one unit of all.
//! - R8: the order is [`upstream_first`]'s, and this module writes none of its own. A link
//!   that only names a document (R3's second kind) is no edge of that order, so the
//!   documents it joins come in no order of the link: the one it names is not always
//!   first. Where a unit holds no edge of what a document stands on, its documents come
//!   by identity.
//! - What could join units unseen is counted, not left out ([`Unjoined`]): an identity
//!   whose files are not valid that is undone, or that an undone document or a document
//!   of a chain stands on or names (its links cannot be read, so what it would join
//!   cannot be told), and a file whose identity cannot be read (it may be any document).
//!   An identity not valid that nothing divided stands on or names joins no unit.
//!
//! # Known limitations
//!
//! - A requirement that many decisions serve, all of them undone with it, is one unit:
//!   the rules allow nothing else.
//! - A chain stops at an approval that holds, and such a document is no key: as one, a
//!   root many documents stand on would make one unit of all. So the work of one unit can
//!   edit a file an artifact of a fresh document lives in that another unit stands on,
//!   which makes that document stale and the other unit's documents with it.
//! - A chain is not in the answer: `waits_on` names only what an undone document waits
//!   on, and what that waits on in turn joins units without being listed.
//! - A key that no document has joins the documents whose links write it and is listed
//!   nowhere (`check` tells of the link); a text that is no key of its book joins
//!   nothing.
//! - A unit is not what a run of `--stale` would approve again: one the frame of an
//!   approval refuses, and what stands on it, are in a unit too.
//! - An artifact that is partly in its files (a Rust item, whose docs come from the
//!   build) may share what is outside them with an artifact of another unit; and two
//!   units may write the same declaration.
//! - The division is of the shelf as it was read. An edit made after it can make it
//!   old; it is read again, and nothing is held.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptySet};

use super::artifact::ArtifactKey;
use super::build::Shelf;
use super::doc::{DocKey, Unfollowed};
use super::footing::Footing;
use super::freshness::{Assessment, StandingKind};
use super::graph::components;
use super::ledger::{Ledger, NotStanding};
use super::order::{self, Hold, upstream_first};
use super::placement::Placement;

/// Which documents are divided: the one choice of `units`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitsOf {
    /// The approvals that no longer hold: expired, stale or broken.
    Degraded,
    /// Those, and every pending document ([`order::pending`]).
    DegradedAndPending,
}

/// One document divided, with where it stands: the kind
/// [`Standing::kind`](super::freshness::Standing::kind) gives it, read once, when it is
/// taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitDoc {
    id: DocId,
    standing: StandingKind,
}

impl UnitDoc {
    /// The document.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// Where it stands ([`Standing::kind`](super::freshness::Standing::kind)): an
    /// approval that no longer holds, or pending.
    pub fn standing(&self) -> StandingKind {
        self.standing
    }
}

/// Why a unit waits on an upstream outside the undone: one upstream has one of these
/// (R5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waits {
    /// It holds a document of the unit back: pending or awaiting.
    Held(Hold),
    /// A new approval may not stand on it ([`Ledger::may_stand_on`]): the decision
    /// against it.
    Refused(NotStanding),
    /// A link of a document of the unit names it and cannot be followed to it: every
    /// reason, from every such link of the unit.
    Unfollowed(NonEmptySet<Unfollowed>),
}

impl Waits {
    /// The reasons one upstream holds documents for, from two of them (R5): the reasons
    /// a link cannot be followed to it are joined; any other pair is one value seen
    /// twice.
    fn merge(&mut self, other: Waits) {
        match (self, other) {
            (Waits::Unfollowed(whys), Waits::Unfollowed(more)) => {
                for why in more.iter() {
                    whys.insert(*why);
                }
            }
            (this, other) => debug_assert_eq!(*this, other, "one upstream, one arm (R5)"),
        }
    }
}

/// Puts `waits` for `upstream` into `into`, merged ([`Waits::merge`]) with what is there.
fn wait_on(into: &mut BTreeMap<DocId, Waits>, upstream: DocId, waits: Waits) {
    match into.entry(upstream) {
        Entry::Occupied(mut there) => there.get_mut().merge(waits),
        Entry::Vacant(slot) => {
            slot.insert(waits);
        }
    }
}

/// What [`divide`] reads of a document it joins, undone or of a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    /// The documents R3 joins it to when they are nodes too: what it stands on (its
    /// footing), and what its `Depends` links name, those it cannot stand on included.
    joins: BTreeSet<DocId>,
    /// The artifacts it stands on.
    artifacts: BTreeSet<ArtifactKey>,
    /// The upstreams its links resolve to that a new approval may not stand on, each
    /// with the decision against it (R5).
    refused: BTreeMap<DocId, NotStanding>,
    /// The documents its links name and cannot be followed to, each with every reason
    /// (R5).
    unfollowed: BTreeMap<DocId, NonEmptySet<Unfollowed>>,
    /// The keys its links write that are no document's (R6).
    unnamed: BTreeSet<DocKey>,
}

impl Node {
    /// What `id` stands on and names in `shelf` and `ledger`, read once.
    fn of(shelf: &Shelf, ledger: &Ledger, id: DocId) -> Self {
        let footing = Footing::of(shelf, id, ledger);
        let artifacts = footing.artifacts(shelf, ledger);
        let refused = order::refused_upstreams(ledger, &footing);
        let mut joins = footing.docs();
        let mut unfollowed: BTreeMap<DocId, NonEmptySet<Unfollowed>> = BTreeMap::new();
        let mut unnamed = BTreeSet::new();
        if let Some(doc) = shelf.doc(id) {
            joins.extend(doc.depends_names());
            for (named, why) in doc.unfollowed_names() {
                match unfollowed.entry(named) {
                    Entry::Occupied(mut whys) => whys.get_mut().insert(why),
                    Entry::Vacant(slot) => {
                        slot.insert(NonEmptySet::new(why, []));
                    }
                }
            }
            unnamed.extend(doc.unnamed_keys().cloned());
        }
        Self {
            joins,
            artifacts,
            refused,
            unfollowed,
            unnamed,
        }
    }
}

/// One undone document, with what [`divide`] reads of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Undone {
    doc: UnitDoc,
    node: Node,
    /// Each upstream outside the undone that it waits on, with why.
    waits: BTreeMap<DocId, Waits>,
}

/// The documents to divide, with what each stands on and what holds it back, and the
/// chain of upstreams outside that holds them back, read once ([`plan`]); [`divide`]
/// makes the units of it once the places of its artifacts ([`UnitsPlan::asks`]) are
/// answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitsPlan<'a> {
    shelf: &'a Shelf,
    ledger: &'a Ledger,
    undone: BTreeMap<DocId, Undone>,
    /// Every upstream outside the undone that holds an undone document back, and every
    /// one outside that holds one of these back in turn (R5).
    chain: BTreeMap<DocId, Node>,
    untold: Option<Unjoined>,
}

/// The documents `of` takes in `shelf`, `ledger` and `assessment` (R1, R2), each with
/// what it stands on, what its links name and what holds it back, and the chain of what
/// holds them back outside (R5), read once, with what could join units unseen counted
/// ([`Unjoined`]): the first of the two steps of the module page.
pub fn plan<'a>(
    shelf: &'a Shelf,
    ledger: &'a Ledger,
    assessment: &Assessment,
    of: UnitsOf,
) -> UnitsPlan<'a> {
    let mut ids: BTreeSet<DocId> = assessment.degraded().map(|(id, _)| id).collect();
    if of == UnitsOf::DegradedAndPending {
        ids.extend(order::pending(shelf, assessment));
    }
    let none = BTreeSet::new();
    // What holds `id` back from outside the undone, read with every undone document in
    // the run and none left out.
    let outside = |id: DocId| -> BTreeMap<DocId, Hold> {
        order::holds(shelf, assessment, id, &ids, &none)
            .into_iter()
            .filter(|held| !ids.contains(&held.upstream))
            .map(|held| (held.upstream, held.hold))
            .collect()
    };
    let mut undone: BTreeMap<DocId, Undone> = BTreeMap::new();
    // The chain starts at what holds an undone document back.
    let mut next: Vec<DocId> = Vec::new();
    for id in &ids {
        let doc = UnitDoc {
            id: *id,
            standing: assessment.standing_of(*id).kind(),
        };
        let node = Node::of(shelf, ledger, *id);
        let holds = outside(*id);
        next.extend(holds.keys().copied());
        // What it waits on: what holds it back, what it may not stand on and what its
        // links cannot be followed to, of those outside the undone. The one place an
        // upstream is told to be outside.
        let held = holds.into_iter().map(|(up, hold)| (up, Waits::Held(hold)));
        let refused = node
            .refused
            .iter()
            .map(|(up, status)| (*up, Waits::Refused(*status)));
        let unfollowed = node
            .unfollowed
            .iter()
            .map(|(up, whys)| (*up, Waits::Unfollowed(whys.clone())));
        let mut waits = BTreeMap::new();
        for (up, why) in held.chain(refused).chain(unfollowed) {
            if !ids.contains(&up) {
                wait_on(&mut waits, up, why);
            }
        }
        undone.insert(*id, Undone { doc, node, waits });
    }
    let mut chain: BTreeMap<DocId, Node> = BTreeMap::new();
    while let Some(id) = next.pop() {
        if chain.contains_key(&id) {
            continue;
        }
        next.extend(outside(id).into_keys().filter(|up| !chain.contains_key(up)));
        chain.insert(id, Node::of(shelf, ledger, id));
    }
    let stood_on_or_named: BTreeSet<DocId> = undone
        .values()
        .map(|undone| &undone.node)
        .chain(chain.values())
        .flat_map(|node| node.joins.iter().copied())
        .collect();
    let invalid = shelf
        .invalid()
        .keys()
        .filter(|id| ids.contains(id) || stood_on_or_named.contains(id))
        .count();
    let unreadable = shelf.unreadable().len();
    let untold = (invalid > 0 || unreadable > 0).then_some(Unjoined {
        invalid,
        unreadable,
    });
    UnitsPlan {
        shelf,
        ledger,
        undone,
        chain,
        untold,
    }
}

impl UnitsPlan<'_> {
    /// The artifacts whose place is to be asked of the adapters: every one an undone
    /// document or a document of the chain stands on.
    pub fn asks(&self) -> BTreeSet<ArtifactKey> {
        self.nodes()
            .flat_map(|(_, node)| node.artifacts.iter().cloned())
            .collect()
    }

    /// Every document divide joins: the undone, and the chain.
    fn nodes(&self) -> impl Iterator<Item = (DocId, &Node)> {
        self.undone
            .iter()
            .map(|(id, undone)| (*id, &undone.node))
            .chain(self.chain.iter().map(|(id, node)| (*id, node)))
    }
}

/// What can join two documents beside R3 (R4, R6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// A file an artifact lives in, whole or in part.
    File(RelPath),
    /// An artifact whose place cannot be told.
    Unplaced(ArtifactKey),
    /// A key a link writes that is no document's.
    Unnamed(DocKey),
}

/// The units of `plan`, given where each artifact it asks about lives (`placements`,
/// the adapters' answers): the second of the two steps of the module page, by its rules.
pub fn divide(plan: UnitsPlan<'_>, placements: &BTreeMap<ArtifactKey, Placement>) -> Units {
    // Each artifact stood on, with where it lives and who stands on it, of the chain too.
    let mut artifacts: BTreeMap<ArtifactKey, (Placement, BTreeSet<DocId>)> = BTreeMap::new();
    for (id, node) in plan.nodes() {
        for artifact in &node.artifacts {
            artifacts
                .entry(artifact.clone())
                .or_insert_with(|| {
                    let placement = placements
                        .get(artifact)
                        .cloned()
                        .unwrap_or_else(Placement::unanswered);
                    (placement, BTreeSet::new())
                })
                .1
                .insert(id);
        }
    }
    let nodes: BTreeSet<DocId> = plan.nodes().map(|(id, _)| id).collect();
    let mut joined: Vec<(DocId, DocId)> = Vec::new();
    let mut holders: BTreeMap<Key, BTreeSet<DocId>> = BTreeMap::new();
    for (id, node) in plan.nodes() {
        for other in &node.joins {
            if nodes.contains(other) {
                joined.push((id, *other));
            }
        }
        // An upstream decided against and a document a link cannot be followed to are
        // joined whether or not they are nodes: one that is, is R3's edge again.
        for stop in node.refused.keys().chain(node.unfollowed.keys()) {
            joined.push((id, *stop));
        }
        for key in &node.unnamed {
            holders
                .entry(Key::Unnamed(key.clone()))
                .or_default()
                .insert(id);
        }
    }
    let UnitsPlan {
        shelf,
        ledger,
        undone,
        untold,
        ..
    } = plan;
    for (artifact, (placement, by)) in &artifacts {
        match placement.files() {
            Some(files) => {
                for file in files {
                    holders
                        .entry(Key::File(file.clone()))
                        .or_default()
                        .extend(by);
                }
            }
            None => holders
                .entry(Key::Unplaced(artifact.clone()))
                .or_default()
                .extend(by),
        }
    }
    // Every undone document and every document of the chain is a node, joined both ways:
    // by R3 to the nodes it stands on or names (a document of the chain to what it holds
    // back among them), to the upstreams decided against and the documents its links
    // cannot be followed to (which are in the graph only, R5), and by each key to the
    // least node that holds it.
    let mut edges: BTreeMap<DocId, BTreeSet<DocId>> =
        nodes.iter().map(|id| (*id, BTreeSet::new())).collect();
    let mut join = |a: DocId, b: DocId| {
        if a != b {
            edges.entry(a).or_default().insert(b);
            edges.entry(b).or_default().insert(a);
        }
    };
    for (a, b) in joined {
        join(a, b);
    }
    for by in holders.values() {
        let mut by = by.iter();
        if let Some(first) = by.next() {
            for other in by {
                join(*first, *other);
            }
        }
    }
    let mut units: Vec<Unit> = components(&edges)
        .into_iter()
        .filter_map(|members| {
            // The undone of the part are the unit; its documents of the chain, and what it
            // waits on that is followed no further, only join. Every one of these is
            // reached from an undone document, so no part is of them alone.
            let members: BTreeSet<DocId> = members
                .into_iter()
                .filter(|id| undone.contains_key(id))
                .collect();
            let least = members.first().copied()?;
            let documents = upstream_first(shelf, ledger, &members)
                .into_iter()
                .filter_map(|id| undone.get(&id).map(|undone| undone.doc))
                .collect();
            // The artifacts its members stand on, read from the members rather than by a
            // pass over every artifact for each unit.
            let stood_on: BTreeSet<&ArtifactKey> = members
                .iter()
                .filter_map(|id| undone.get(id))
                .flat_map(|undone| &undone.node.artifacts)
                .collect();
            let unit_artifacts: Vec<UnitArtifact> = stood_on
                .into_iter()
                .filter_map(|artifact| {
                    let (placement, by) = artifacts.get(artifact)?;
                    Some(UnitArtifact {
                        artifact: artifact.clone(),
                        placement: placement.clone(),
                        documents: by.intersection(&members).copied().collect(),
                    })
                })
                .collect();
            let files = unit_artifacts
                .iter()
                .filter_map(|artifact| artifact.placement.files())
                .flatten()
                .cloned()
                .collect();
            let mut waits_on = BTreeMap::new();
            for undone in members.iter().filter_map(|id| undone.get(id)) {
                for (up, waits) in &undone.waits {
                    wait_on(&mut waits_on, *up, waits.clone());
                }
            }
            Some(Unit {
                least,
                documents,
                files,
                artifacts: unit_artifacts,
                waits_on,
            })
        })
        .collect();
    units.sort_by_key(|unit| unit.least);
    let unplaced = artifacts
        .into_iter()
        .filter_map(|(artifact, (placement, documents))| match placement {
            Placement::Unplaced(why) => Some(UnplacedArtifact {
                artifact,
                why,
                documents,
            }),
            Placement::Only(_) | Placement::Partly(_) => None,
        })
        .collect();
    Units {
        units,
        unplaced,
        untold,
    }
}

/// The undone documents divided into units that share nothing, the artifacts whose
/// place cannot be told, and what could join units unseen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Units {
    units: Vec<Unit>,
    unplaced: Vec<UnplacedArtifact>,
    untold: Option<Unjoined>,
}

impl Units {
    /// The units, by the least identity of their documents (R9).
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// Every artifact an undone document or a document of a chain stands on whose place
    /// cannot be told, by space and name: it joins only the documents that stand on the
    /// same name (R6), so that two units share no file is not said of it.
    pub fn unplaced(&self) -> &[UnplacedArtifact] {
        &self.unplaced
    }

    /// What could join units without being seen, when anything could: then two units
    /// may share something the division cannot tell.
    pub fn untold(&self) -> Option<&Unjoined> {
        self.untold.as_ref()
    }
}

/// What could join units without the division seeing it, counted ([`plan`] counts it);
/// at least one of the two counts is more than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unjoined {
    invalid: usize,
    unreadable: usize,
}

impl Unjoined {
    /// The identities whose files are not valid, so that their links cannot be read,
    /// that are undone or that an undone document or a document of a chain stands on or
    /// names.
    pub fn invalid(&self) -> usize {
        self.invalid
    }

    /// The files whose identity cannot be read: each may be any document.
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }
}

/// One unit: documents that can be mended and approved again apart from every other
/// unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The least identity of its documents, which orders the units.
    least: DocId,
    documents: Vec<UnitDoc>,
    files: BTreeSet<RelPath>,
    artifacts: Vec<UnitArtifact>,
    waits_on: BTreeMap<DocId, Waits>,
}

impl Unit {
    /// Its documents, in the order of [`upstream_first`] (R8): one stands after what it
    /// stands on of the unit; one a link only names comes in no order of that link, and
    /// no order passes through the chain the unit waits on.
    pub fn documents(&self) -> &[UnitDoc] {
        &self.documents
    }

    /// The files its artifacts live in, whole or in part, by path. The files of its
    /// documents and of the declaration are not among them (R7), nor those of the chain
    /// it waits on, which no other unit shares either. A file an artifact of a fresh
    /// document lives in is no key ([known limitations](self#known-limitations)).
    pub fn files(&self) -> &BTreeSet<RelPath> {
        &self.files
    }

    /// The artifacts its documents stand on, by space and name, each with where it
    /// lives.
    pub fn artifacts(&self) -> &[UnitArtifact] {
        &self.artifacts
    }

    /// Each upstream outside the undone that one of its documents waits on, by identity,
    /// once, with why (R5): it holds the document back (pending or awaiting), a new
    /// approval may not stand on it (the decision against it), or a link names it and
    /// cannot be followed to it (every reason). What those wait on in turn, and what the
    /// chain waits on, joins the unit too, and is not listed.
    pub fn waits_on(&self) -> &BTreeMap<DocId, Waits> {
        &self.waits_on
    }
}

/// An artifact the documents of a unit stand on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitArtifact {
    artifact: ArtifactKey,
    placement: Placement,
    documents: BTreeSet<DocId>,
}

impl UnitArtifact {
    /// Its space and name.
    pub fn artifact(&self) -> &ArtifactKey {
        &self.artifact
    }

    /// Where it lives, as the adapter of its space said.
    pub fn placement(&self) -> &Placement {
        &self.placement
    }

    /// The documents that stand on it.
    pub fn documents(&self) -> &BTreeSet<DocId> {
        &self.documents
    }
}

/// An artifact whose place cannot be told, with why and the documents that stand on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnplacedArtifact {
    artifact: ArtifactKey,
    why: NonBlank,
    documents: BTreeSet<DocId>,
}

impl UnplacedArtifact {
    /// Its space and name.
    pub fn artifact(&self) -> &ArtifactKey {
        &self.artifact
    }

    /// Why its place cannot be told.
    pub fn why(&self) -> &NonBlank {
        &self.why
    }

    /// The documents that stand on it.
    pub fn documents(&self) -> &BTreeSet<DocId> {
        &self.documents
    }
}

#[cfg(test)]
mod tests;
