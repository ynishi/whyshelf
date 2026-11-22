//! Declarations that an edit keeps the meaning: [`SameMeaning`].
//!
//! A person says that what a document or a file is now means what it meant at one or
//! more earlier hashes (a typo fixed, a sentence reworded, a comment of the code
//! changed). Freshness then takes a pin of one of those hashes as a pin of the hash
//! now, and tells it as a notice instead of a cause: the approval holds, and who said
//! the edit keeps the meaning, and why, stays readable.
//!
//! Nothing is guessed. Two hashes mean the same only through declarations, read on
//! every assessment like every other record ([`Ledger::doc_same_meaning`], and for a
//! pin of an artifact as it is read now the declarations of every name of its way,
//! `Ledger::pin_same_meaning`); a declaration is withdrawn by a
//! [`Revocation`](super::Revocation), as a decision is.
//!
//! ```text
//!   pinned A ── declared ── B ── declared ── C (now)    A, B and C mean the same
//!   pinned D                                            D does not: a change
//! ```
//!
//! [`frame_same_meaning`] gathers what to declare: the edit from one earlier hash (the
//! one the person names, else the one the document's own approval saw, else the only
//! one pinned) to the hash now, with every pinned hash that already means the same as
//! that earlier one. An older pin from before a change no one declared is left a
//! change. A declaration about a document that is not valid, about a file that is not
//! there, with nothing to declare, or whose earlier hash cannot be told is refused
//! before any record exists.
//!
//! [`Ledger::doc_same_meaning`]: super::Ledger::doc_same_meaning
//!
//! How many different names an edit needs before it is joined is on [`SameMeaning`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use thiserror::Error;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::hash::ContentHash;
use crate::domain::id::DocId;
use crate::domain::nonempty::{NonBlank, NonEmptySet, NonEmptyVec};
use crate::domain::words::{Join, Mark, listed};

use super::super::artifact::{
    Artifact, ArtifactHash, ArtifactKey, ArtifactName, ArtifactSeen, ArtifactState, Comparison,
    Space, Untold,
};
use super::super::build::{FailureTag, Shelf};
use super::super::doc::DocHash;
use super::super::schema::{Approvals, BookName};
use super::{JudgedAt, JudgementId, Ledger, NameForm, PinnedArtifact, ReadPin, way_names};

/// What a declaration is about, and the hashes it says mean the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SameMeaningOf {
    /// A document: each hash of `from` means what `to` means.
    Doc {
        /// The document.
        id: DocId,
        /// The earlier hashes.
        from: NonEmptySet<DocHash>,
        /// The hash it had when the declaration was made.
        to: DocHash,
    },
    /// An artifact, by its space and its name.
    Artifact {
        /// The artifact.
        artifact: ArtifactKey,
        /// The earlier hashes.
        from: NonEmptySet<ArtifactHash>,
        /// The hash it had when the declaration was made.
        to: ArtifactHash,
    },
}

/// Why a declaration cannot be made as given.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SameMeaningProblem {
    /// The hash it had is among the earlier hashes: a declaration of nothing.
    #[error("the hash now, {to}, is among the hashes said to mean the same as it")]
    ToAmongFrom {
        /// The hash, as text.
        to: String,
    },
}

/// A person's declaration that an edit keeps the meaning.
///
/// # How many names
///
/// Each declaration joins each of its earlier hashes to its hash then: an *edge*, a
/// pair of hashes, either way round. An edge is used for a way only once, for some
/// declaration of it, the different names that declared the edge are at least as
/// many as that declaration needs (the `approvals` of the rule that reads it; one
/// when no rule does): so where a book asks for two names to approve, one name cannot
/// keep an approval of it fresh by declaring the edit alone, and a declaration added
/// never takes back an edge that counted (one made after a rule's `since` beside one
/// made before it). The names are counted per edge, never per way: a way is made of
/// edges, and counting it whole would let one name join two hashes by declaring a way
/// of its own around the edge it was short on. An edge with fewer names joins
/// nothing; one a pin of an approval that stands meets is told by
/// [`Ledger::same_meaning_short`](super::Ledger::same_meaning_short) (in `check`,
/// `same_meaning_short`) until more names declare it. A ledger no rule reads counts
/// nothing: every declaration needs one name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameMeaning {
    pub(super) id: JudgementId,
    pub(super) of: SameMeaningOf,
    pub(super) by: Attribution,
    pub(super) at: JudgedAt,
    pub(super) reason: NonBlank,
    /// How the record names its artifact: kept from a record read back, so it is
    /// written as it was stored.
    pub(super) form: NameForm,
    /// The book of the document it is about, as the record names it (its key `book`).
    pub(super) book: Option<BookName>,
}

impl SameMeaning {
    /// A declaration. Refuses one whose hash now is among the earlier hashes. The
    /// reason is required: a declaration is a claim a reader must be able to check.
    pub fn new(
        id: JudgementId,
        of: SameMeaningOf,
        by: Attribution,
        at: JudgedAt,
        reason: NonBlank,
    ) -> Result<Self, SameMeaningProblem> {
        let to_among_from = match &of {
            SameMeaningOf::Doc { from, to, .. } => from.contains(to).then(|| to.to_string()),
            SameMeaningOf::Artifact { from, to, .. } => from.contains(to).then(|| to.to_string()),
        };
        if let Some(to) = to_among_from {
            return Err(SameMeaningProblem::ToAmongFrom { to });
        }
        Ok(Self {
            id,
            of,
            by,
            at,
            reason,
            form: NameForm::Spaced,
            book: None,
        })
    }

    /// The declaration, naming the book its document is in now as `shelf` tells it
    /// ([`Shelf::book_to_record`]), in its key `book`; a declaration about an
    /// artifact, about a document whose book cannot be told, or in a project that
    /// declares no rule of who may decide, names none.
    pub fn in_book(mut self, shelf: &Shelf) -> Self {
        self.book = match &self.of {
            SameMeaningOf::Doc { id, .. } => shelf.book_to_record(*id).cloned(),
            SameMeaningOf::Artifact { .. } => None,
        };
        self
    }

    /// The book of the document it is about, as the record names it; none for an
    /// artifact, and for a record made before records named one. Only who may
    /// decide reads it.
    pub fn book(&self) -> Option<&BookName> {
        self.book.as_ref()
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// What it is about, and the hashes.
    pub fn of(&self) -> &SameMeaningOf {
        &self.of
    }

    /// Who declared it.
    pub fn actor(&self) -> &Actor {
        self.by.actor()
    }

    /// Who declared it, and who ran the command when it was another.
    pub fn attribution(&self) -> &Attribution {
        &self.by
    }

    /// When.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// Why.
    pub fn reason(&self) -> &NonBlank {
        &self.reason
    }
}

/// What a declaration may be about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SameMeaningTarget {
    /// A document.
    Doc(DocId),
    /// An artifact, by its name as a link writes it, in the one space the documents
    /// link it in.
    Artifact(ArtifactName),
}

/// Why a declaration is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SameMeaningRefusal {
    /// The document is not a valid document now, so it has no hash to declare.
    #[error("{}", tag.said_of(id))]
    DocNotValid {
        /// The document.
        id: DocId,
        /// How it is not.
        tag: FailureTag,
    },
    /// No document links the name, so what is there was never read.
    #[error("no document links `{name}`")]
    ArtifactNotLinked {
        /// The name.
        name: ArtifactName,
    },
    /// Documents link the name in more than one space, so which artifact is meant
    /// cannot be told.
    #[error("documents link `{name}` in more than one space: {}", listed(.spaces.iter(), Mark::Backtick, Join::And))]
    ArtifactInSpaces {
        /// The name.
        name: ArtifactName,
        /// The spaces.
        spaces: Vec<Space>,
    },
    /// Nothing is there now.
    #[error("{artifact} has nothing there now")]
    ArtifactAbsent {
        /// The artifact.
        artifact: ArtifactKey,
    },
    /// Its adapter cannot tell about it now.
    #[error("{artifact} cannot be told about now: {why}")]
    ArtifactUnknown {
        /// The artifact.
        artifact: ArtifactKey,
        /// Why it cannot be told.
        why: Untold,
    },
    /// Every approval that stands pinned the hash now, or one that already means the
    /// same as it.
    #[error("no approval that stands pinned another hash of it: there is nothing to declare")]
    NothingToDeclare,
    /// The one who declares it declared the same edit already, in a declaration not
    /// revoked ([`Ledger::not_declared_by`]): one name counts once, so it would add
    /// nothing, and the edit awaits the declaration of another name.
    #[error("`{actor}` already declared this edit: it awaits the declaration of another name")]
    AlreadyDeclaredBy {
        /// The one who declares it.
        actor: Actor,
    },
    /// The hash named as the earlier one is no hash an approval that stands pinned
    /// for it (or it is the hash now, or already means the same as it).
    #[error("no approval that stands pinned {hash} for it, apart from the hash now")]
    NotPinned {
        /// The hash named, as text.
        hash: String,
    },
    /// The hash named was pinned under a comparison no link declares for the
    /// artifact now: the approval is to be looked at again (the comparison changed), and an
    /// edit across two comparisons cannot be declared.
    #[error("{hash} was pinned under a comparison no link declares now: approve again instead")]
    PinnedUnderOtherComparison {
        /// The hash named, as text.
        hash: String,
    },
    /// Approvals pinned earlier hashes that do not mean the same as one another, and
    /// none was named: which edit was read cannot be told.
    #[error("approvals pinned {} earlier hashes that do not mean the same: name the one the edit was read from", hashes.len())]
    SeveralEarlier {
        /// The hashes, as text.
        hashes: Vec<String>,
    },
}

/// What a declaration about `target` would say: the edit from one earlier hash to
/// the hash now, and every hash an approval that stands pinned that already means
/// the same as that earlier one.
///
/// The earlier hash is the one the person read the edit from, and is never guessed
/// across a change no one declared. It is, in this order: the one `from` names (a
/// hash some approval that stands pinned); for a document, the hash its own approval
/// saw, when that is not the hash now; the only earlier hash, when every pinned one
/// means the same. Otherwise the declaration is refused as
/// [`SameMeaningRefusal::SeveralEarlier`]. So an older downstream pin, from before a
/// rewrite the document was approved again after, stays a change. A second
/// declaration of the same edit is refused: here as
/// [`SameMeaningRefusal::NothingToDeclare`] when the ledger joins the edit already, and
/// by [`Ledger::not_declared_by`] as [`SameMeaningRefusal::AlreadyDeclaredBy`] when the
/// one who declares it declared it before, which adds no name.
///
/// For an artifact, hashes are taken under one [`Comparison`] at a time, since two links
/// may compare it differently: the one whose pins hold the hash named, else the only
/// one with an earlier hash to declare (two such, and none named, is
/// [`SameMeaningRefusal::SeveralEarlier`]), with the hash now under that comparison.
///
/// The one place the earlier hashes are chosen.
///
/// Known limitation: it reads what is there now and which declarations join on its
/// own, apart from the pins stale compares (F-63): it may offer an edit that stale
/// already tells as kept, or one of a pin stale does not compare. Making it read the
/// pins stale compares is left for #175.
pub fn frame_same_meaning(
    shelf: &Shelf,
    ledger: &Ledger,
    target: SameMeaningTarget,
    from: Option<ContentHash>,
) -> Result<SameMeaningOf, SameMeaningRefusal> {
    match target {
        SameMeaningTarget::Doc(id) => {
            let doc = shelf.state_of(id).valid_or_failure().map_err(|failure| {
                SameMeaningRefusal::DocNotValid {
                    id,
                    tag: failure.tag(),
                }
            })?;
            let now = *doc.hash();
            let mut pinned = BTreeSet::new();
            let mut own = None;
            for standing in ledger.approvals_that_stand() {
                let view = standing.view();
                let seen = *view.seen();
                if view.subject() == id {
                    pinned.insert(seen);
                    own = Some(seen);
                }
                pinned.extend(
                    view.frame()
                        .iter()
                        .filter(|pin| pin.target() == id)
                        .map(|pin| *pin.hash()),
                );
            }
            let same = |a: &DocHash, b: &DocHash| ledger.doc_same_meaning(id, a, b).is_some();
            let from = earlier(pinned, now, from.map(DocHash::restore), own, same)?;
            Ok(SameMeaningOf::Doc { id, from, to: now })
        }
        SameMeaningTarget::Artifact(name) => {
            // The artifact is the name in the one space the documents link it in.
            let spaces: BTreeSet<&Space> = shelf
                .docs()
                .values()
                .flat_map(|doc| doc.artifact_links().values())
                .flat_map(|link| link.artifacts())
                .filter(|a| *a.name() == name)
                .map(Artifact::space)
                .collect();
            let artifact = match spaces.into_iter().collect::<Vec<_>>().as_slice() {
                [] => return Err(SameMeaningRefusal::ArtifactNotLinked { name }),
                [space] => ArtifactKey::new((*space).clone(), name),
                many => {
                    return Err(SameMeaningRefusal::ArtifactInSpaces {
                        name,
                        spaces: many.iter().map(|s| (*s).clone()).collect(),
                    });
                }
            };
            // An artifact's hash is made by a comparison, so the hash now and the
            // earlier hashes are taken under one comparison at a time: the hash now of
            // each comparison a link declares for it now.
            let mut now_of: BTreeMap<Comparison, &ArtifactState> = BTreeMap::new();
            for doc in shelf.docs().values() {
                for link in doc.artifact_links().values() {
                    for found in link.artifacts().iter().filter(|a| *a.key() == artifact) {
                        now_of.insert(link.kind().comparison.clone(), found.state());
                    }
                }
            }
            let unknown = now_of
                .values()
                .find_map(|state| state.told().err().cloned());
            let now_of: BTreeMap<Comparison, ArtifactHash> = now_of
                .into_iter()
                .filter_map(|(c, state)| match state.told() {
                    Ok(ArtifactSeen::Present(hash)) => Some((c, hash)),
                    Ok(ArtifactSeen::Absent) | Err(_) => None,
                })
                .collect();
            if now_of.is_empty() {
                return Err(match unknown {
                    Some(why) => SameMeaningRefusal::ArtifactUnknown { artifact, why },
                    None => SameMeaningRefusal::ArtifactAbsent { artifact },
                });
            }
            // The earlier hashes, by the comparison they were made with; a pin of a
            // comparison no link declares now is told by freshness as such, and has
            // nothing to declare.
            let mut pinned: BTreeMap<Comparison, BTreeSet<ArtifactHash>> = BTreeMap::new();
            let mut under_other: BTreeSet<ArtifactHash> = BTreeSet::new();
            for standing in ledger.approvals_that_stand() {
                // A pin is taken by the name it is read as, so an edit made with a
                // move is declared at the new name.
                let read = standing.view().read_pins();
                for pin in read
                    .iter()
                    .filter(|p| *p.read_as() == artifact)
                    .map(ReadPin::pin)
                {
                    let PinnedArtifact::Present { hash, .. } = pin.seen() else {
                        continue;
                    };
                    if now_of.contains_key(pin.comparison()) {
                        pinned
                            .entry(pin.comparison().clone())
                            .or_default()
                            .insert(*hash);
                    } else {
                        under_other.insert(*hash);
                    }
                }
            }
            let same = |a: &ArtifactHash, b: &ArtifactHash| {
                ledger.artifact_same_meaning(&artifact, a, b).is_some()
            };
            let named = from.map(ArtifactHash::restore);
            // The comparison to declare under: the one whose pins hold the hash named,
            // else the only one with an earlier hash; two with earlier hashes and none
            // named cannot be told apart.
            let open: Vec<&Comparison> = pinned
                .iter()
                .filter(|(c, hashes)| {
                    let now = now_of[*c];
                    hashes.iter().any(|h| *h != now && !same(h, &now))
                })
                .map(|(c, _)| c)
                .collect();
            let comparison = match named {
                Some(hash) => match pinned.iter().find(|(_, hashes)| hashes.contains(&hash)) {
                    Some((c, _)) => c.clone(),
                    None if under_other.contains(&hash) => {
                        return Err(SameMeaningRefusal::PinnedUnderOtherComparison {
                            hash: hash.to_string(),
                        });
                    }
                    None => {
                        return Err(SameMeaningRefusal::NotPinned {
                            hash: hash.to_string(),
                        });
                    }
                },
                None => match open.as_slice() {
                    [] => return Err(SameMeaningRefusal::NothingToDeclare),
                    [one] => (*one).clone(),
                    _ => {
                        return Err(SameMeaningRefusal::SeveralEarlier {
                            hashes: open
                                .iter()
                                .flat_map(|c| {
                                    let now = now_of[*c];
                                    let same = &same;
                                    pinned[*c]
                                        .iter()
                                        .filter(move |h| **h != now && !same(h, &now))
                                })
                                .map(ToString::to_string)
                                .collect(),
                        });
                    }
                },
            };
            let now = now_of[&comparison];
            let hashes = pinned.remove(&comparison).unwrap_or_default();
            let from = earlier(hashes, now, named, None, same)?;
            Ok(SameMeaningOf::Artifact {
                artifact,
                from,
                to: now,
            })
        }
    }
}

impl Ledger {
    /// The edit `of`, as [`frame_same_meaning`] framed it, without the earlier
    /// hashes that `by` already declared to mean what its hash means, in a
    /// declaration not revoked among those counting reads for it: of the document, or
    /// of every name of the artifact's way through its moves, as for each pin read as
    /// that artifact, as the declarations of a pin read now are read. That edge has `by`'s name
    /// already, and one name counts once, so a second declaration of it adds nothing
    /// (as an approval that repeats its actor's own reading writes nothing). Where the
    /// ledger joins the edit already, the frame refuses it first; this is for one
    /// whose edge does not count yet and awaits another name. With no earlier hash
    /// left, it is refused as [`SameMeaningRefusal::AlreadyDeclaredBy`].
    pub fn not_declared_by(
        &self,
        of: SameMeaningOf,
        by: &Actor,
    ) -> Result<SameMeaningOf, SameMeaningRefusal> {
        match of {
            SameMeaningOf::Doc { id, from, to } => {
                let declared = declared_by(self.doc_declared(id), by);
                let from = unrepeated(from, to, &declared, by)?;
                Ok(SameMeaningOf::Doc { id, from, to })
            }
            SameMeaningOf::Artifact { artifact, from, to } => {
                let pins: Vec<ReadPin> = self
                    .approvals_that_stand()
                    .flat_map(|standing| standing.view().read_pins())
                    .filter(|pin| *pin.read_as() == artifact)
                    .collect();
                let mut names = BTreeSet::from([&artifact]);
                names.extend(pins.iter().flat_map(way_names));
                let declared = declared_by(self.artifact_declared(|of| names.contains(of)), by);
                let from = unrepeated(from, to, &declared, by)?;
                Ok(SameMeaningOf::Artifact { artifact, from, to })
            }
        }
    }
}

/// The edges of `declared` that `by` declared.
fn declared_by<H: Ord>(declared: Vec<(&SameMeaning, H, H)>, by: &Actor) -> BTreeSet<(H, H)> {
    declared
        .into_iter()
        .filter(|(s, _, _)| s.actor() == by)
        .map(|(_, from, to)| ends(from, to))
        .collect()
}

/// The two hashes of an edge, the smaller first.
fn ends<H: Ord>(x: H, y: H) -> (H, H) {
    if x <= y { (x, y) } else { (y, x) }
}

/// The earlier hashes of `from` whose edge to `to` is not among `declared`, those
/// `by` declared; none left is [`SameMeaningRefusal::AlreadyDeclaredBy`].
fn unrepeated<H: Ord + Copy>(
    from: NonEmptySet<H>,
    to: H,
    declared: &BTreeSet<(H, H)>,
    by: &Actor,
) -> Result<NonEmptySet<H>, SameMeaningRefusal> {
    let left: BTreeSet<H> = from
        .iter()
        .filter(|f| !declared.contains(&ends(**f, to)))
        .copied()
        .collect();
    NonEmptySet::from_set(left)
        .map_err(|_| SameMeaningRefusal::AlreadyDeclaredBy { actor: by.clone() })
}

/// The earlier hashes a declaration covers, out of every hash an approval that
/// stands pinned: the rule of [`frame_same_meaning`], for both kinds of subject.
///
/// `named` is the hash the person named, `own` the hash the subject's own approval
/// saw, and `same` whether the ledger already joins two hashes.
fn earlier<H: Ord + Copy + std::fmt::Display>(
    mut pinned: BTreeSet<H>,
    now: H,
    named: Option<H>,
    own: Option<H>,
    same: impl Fn(&H, &H) -> bool,
) -> Result<NonEmptySet<H>, SameMeaningRefusal> {
    pinned.retain(|h| *h != now && !same(h, &now));
    let base = match (named, own) {
        (Some(hash), _) if pinned.contains(&hash) => hash,
        (Some(hash), _) => {
            return Err(SameMeaningRefusal::NotPinned {
                hash: hash.to_string(),
            });
        }
        (None, Some(seen)) if pinned.contains(&seen) => seen,
        (None, _) => {
            let Some(first) = pinned.iter().next().copied() else {
                return Err(SameMeaningRefusal::NothingToDeclare);
            };
            if pinned.iter().all(|h| *h == first || same(h, &first)) {
                first
            } else {
                return Err(SameMeaningRefusal::SeveralEarlier {
                    hashes: pinned.iter().map(ToString::to_string).collect(),
                });
            }
        }
    };
    let from: BTreeSet<H> = pinned
        .into_iter()
        .filter(|h| *h == base || same(h, &base))
        .collect();
    NonEmptySet::from_set(from).map_err(|_| SameMeaningRefusal::NothingToDeclare)
}

/// One pair of hashes said to mean the same, with the declarations that say it and
/// how many different names it needs (the rule is on [`SameMeaning`]).
pub(super) struct Edge<'a, H> {
    /// The two hashes, the smaller first.
    ends: (H, H),
    /// The declarations of it, in the order of identity.
    declarations: Vec<&'a SameMeaning>,
    /// The least the declarations need.
    need: Approvals,
}

impl<H> Edge<'_, H> {
    /// The declarations that make the edge count: the first of each name, in the
    /// order of identity, as many as it needs; none while fewer names declared it.
    fn counted(&self) -> Option<NonEmptyVec<JudgementId>> {
        let firsts = self.firsts();
        if firsts.len() < self.need.count() {
            return None;
        }
        NonEmptyVec::from_vec(firsts.into_iter().take(self.need.count()).collect()).ok()
    }

    /// The first declaration of each name, in the order of identity.
    fn firsts(&self) -> Vec<JudgementId> {
        let mut names = BTreeSet::new();
        self.declarations
            .iter()
            .filter(|s| names.insert(s.actor()))
            .map(|s| s.id)
            .collect()
    }
}

/// The edges `declared` makes: each declaration with the need of its record, one of
/// its earlier hashes, and its hash then. Each pair of hashes is one edge, in the
/// order the declarations first name it.
pub(super) fn edges<'a, H: Ord + Copy>(
    declared: impl IntoIterator<Item = (&'a SameMeaning, Approvals, H, H)>,
) -> Vec<Edge<'a, H>> {
    let mut out: Vec<Edge<'a, H>> = Vec::new();
    let mut at: BTreeMap<(H, H), usize> = BTreeMap::new();
    for (record, need, from, to) in declared {
        let ends = ends(from, to);
        match at.get(&ends) {
            Some(i) => {
                let edge = &mut out[*i];
                edge.declarations.push(record);
                edge.need = edge.need.min(need);
            }
            None => {
                at.insert(ends, out.len());
                out.push(Edge {
                    ends,
                    declarations: vec![record],
                    need,
                });
            }
        }
    }
    out
}

/// The declarations that join `a` to `b` over the edges that count, in the order of
/// the way from one to the other (each edge's as [`Edge::counted`] gives them), or
/// `None` when no way of them joins the two.
pub(super) fn way<H: Ord + Copy>(
    edges: &[Edge<'_, H>],
    a: H,
    b: H,
) -> Option<NonEmptyVec<JudgementId>> {
    let hops = declared_way(
        edges
            .iter()
            .filter_map(|edge| Some((edge.counted()?, edge.ends.0, edge.ends.1))),
        a,
        b,
    )?;
    let mut ids = hops.iter().flat_map(NonEmptyVec::iter).copied();
    let first = ids.next()?;
    Some(NonEmptyVec::new(first, ids.collect()))
}

/// The declarations of `declared` that join `a` to `b`, each its own edge, in the
/// order of the way: how a ledger no rule reads joins them, where each declaration
/// needs one name and counting the names would change nothing.
pub(super) fn each_way<'a, H: Ord + Copy>(
    declared: impl IntoIterator<Item = (&'a SameMeaning, H, H)>,
    a: H,
    b: H,
) -> Option<NonEmptyVec<JudgementId>> {
    declared_way(declared.into_iter().map(|(s, x, y)| (s.id, x, y)), a, b)
}

/// The edges of `edges` declared by fewer different names than they need that a pin
/// of `from` meets: one of their ends is `from`, or is joined to it by edges that
/// count. Each is told with what it is about (`of`, given its first declaration and
/// its two hashes).
pub(super) fn short_from<H: Ord + Copy>(
    edges: &[Edge<'_, H>],
    from: H,
    of: impl Fn(&SameMeaning, H, H) -> SameMeaningEdge,
) -> Vec<ShortSameMeaning> {
    let mut reached = BTreeSet::from([from]);
    let mut next = vec![from];
    while let Some(at) = next.pop() {
        for edge in edges.iter().filter(|edge| edge.counted().is_some()) {
            let (x, y) = edge.ends;
            let onward = if x == at {
                y
            } else if y == at {
                x
            } else {
                continue;
            };
            if reached.insert(onward) {
                next.push(onward);
            }
        }
    }
    edges
        .iter()
        .filter(|edge| edge.counted().is_none())
        .filter(|edge| reached.contains(&edge.ends.0) || reached.contains(&edge.ends.1))
        .filter_map(|edge| {
            let first = *edge.declarations.first()?;
            let declared = NonEmptyVec::new(
                first.id,
                edge.declarations[1..].iter().map(|s| s.id).collect(),
            );
            let names = NonEmptySet::new(
                first.actor().clone(),
                edge.declarations[1..].iter().map(|s| s.actor().clone()),
            );
            Some(ShortSameMeaning {
                of: of(first, edge.ends.0, edge.ends.1),
                declared,
                names,
                need: edge.need,
            })
        })
        .collect()
}

/// What an edge of declarations of the same meaning is about: a document or an
/// artifact, with the two hashes it joins (the smaller first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SameMeaningEdge {
    /// Two hashes of a document.
    Doc {
        /// The document.
        id: DocId,
        /// One hash.
        a: DocHash,
        /// The other.
        b: DocHash,
    },
    /// Two hashes of an artifact.
    Artifact {
        /// The artifact, by its space and its name.
        artifact: ArtifactKey,
        /// One hash.
        a: ArtifactHash,
        /// The other.
        b: ArtifactHash,
    },
}

/// An edge declared by fewer different names than its declarations need: it joins
/// nothing until more names declare it ([`Ledger::same_meaning_short`]; the rule is
/// on [`SameMeaning`]).
///
/// [`Ledger::same_meaning_short`]: super::Ledger::same_meaning_short
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortSameMeaning {
    /// What it is about.
    pub of: SameMeaningEdge,
    /// The declarations of it, in the order of identity.
    pub declared: NonEmptyVec<JudgementId>,
    /// The names that declared it, each once.
    pub names: NonEmptySet<Actor>,
    /// How many different names it needs: the least of its declarations'.
    pub need: Approvals,
}

/// The labels of the edges that join `a` to `b`, in the order of the way from one
/// to the other, or `None` when no way of them joins them.
///
/// Each edge joins its two hashes both ways: a meaning kept from A to B is kept
/// from B to A. The way is a shortest one, so the answer is the same for the same
/// records in any order.
fn declared_way<H: Ord + Copy, L: Clone>(
    edges: impl IntoIterator<Item = (L, H, H)>,
    a: H,
    b: H,
) -> Option<NonEmptyVec<L>> {
    let mut next: BTreeMap<H, Vec<(H, L)>> = BTreeMap::new();
    for (label, x, y) in edges {
        next.entry(x).or_default().push((y, label.clone()));
        next.entry(y).or_default().push((x, label));
    }
    let mut came: BTreeMap<H, (H, L)> = BTreeMap::new();
    let mut queue = VecDeque::from([a]);
    let mut seen = BTreeSet::from([a]);
    while let Some(at) = queue.pop_front() {
        if at == b {
            let mut way = Vec::new();
            let mut here = b;
            while let Some((before, label)) = came.get(&here) {
                way.push(label.clone());
                here = *before;
            }
            way.reverse();
            return NonEmptyVec::from_vec(way).ok();
        }
        for (onward, label) in next.get(&at).into_iter().flatten() {
            if seen.insert(*onward) {
                came.insert(*onward, (at, label.clone()));
                queue.push_back(*onward);
            }
        }
    }
    None
}
