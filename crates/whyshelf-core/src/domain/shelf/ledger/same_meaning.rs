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
//! [`frame_same_meaning`] gathers what to declare from the pins stale compares now, by
//! the document or the name asked about: the edit from one earlier hash (the one the
//! person names, else the one the document's own approval saw, else the only one stale
//! tells as changed) to the hash now, with every other hash stale tells as changed that
//! the declarations along the moves of every pin that holds it join to that earlier
//! one. An older pin from before a change no one declared is left a change. A
//! declaration about a document that is not valid, of an earlier hash whose pin has
//! nothing behind it now or cannot be told now, of a pin stale does not compare now, or
//! with nothing to declare (no pin of it holds a hash stale tells as changed) is refused
//! before any record exists, by one refusal in one order; one by a name that declared
//! the same edge already is refused by its second step ([`Framed::not_declared_by`]).
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

use super::super::artifact::{Artifact, ArtifactHash, ArtifactKey, ArtifactName, Space};
use super::super::build::Shelf;
use super::super::doc::DocHash;
use super::super::schema::{Approvals, BookName};
use super::compared::{ComparedNow, PinsNow};
use super::refusal::SameMeaningRefusal;
use super::{ArtifactJoins, DocJoins, JudgedAt, JudgementId, Ledger, NameForm};

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
/// nothing; one a pin that stale reads now meets is told by
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

/// What a declaration about `target` would say, framed on `ledger` ([`Framed`]): the
/// edit from one earlier hash to the hash now, and every other hash stale tells as
/// changed that is joined to that earlier one, read from the pins of the approvals that
/// stand as stale compares them on `shelf`.
///
/// What is offered is exactly what stale tells as a change: an edit of a pin whose
/// content it compares now, of an approval whose document is valid, and for an artifact
/// under a link that still writes the name it is read as and declares the comparison it
/// was pinned with; an edit a declaration already keeps, along the moves of the pin, is
/// no change. The earlier hash is the one the person read the edit from, and is never
/// guessed across a change no one declared. It is, in this order: the one `from` names
/// (a hash a pin holds as changed); for a document, the hash its own approval saw, when
/// stale tells it as changed; the only earlier hash, or, of several, all of them when
/// one of them is joined by every other, whatever their order. Another hash is joined to
/// the earlier one only when the declarations along the moves of every pin that holds
/// it join the two (of a document, those about it), as freshness reads each pin.
/// Otherwise the declaration is refused as [`SameMeaningRefusal::SeveralEarlier`]. So an
/// older downstream pin, from before a rewrite the document was approved again after,
/// stays a change.
///
/// For an artifact, hashes are taken under one
/// [`Comparison`](super::super::artifact::Comparison) at a time, since two links may
/// compare it differently: the one whose pins hold the hash named (the first in the
/// order of comparisons when two do), else the only one with an edit to declare (two
/// such, and none named, is [`SameMeaningRefusal::SeveralEarlier`]), with the hash now
/// under that comparison.
///
/// When nothing is offered, or the hash named is no edit offered, one refusal decides,
/// over the pins of the document or of the name that pinned a hash (the hash named, or
/// any), in this order: a pin that holds (at the hash now, or kept in its meaning) is
/// [`SameMeaningRefusal::NotPinned`] when a hash was named, else
/// [`SameMeaningRefusal::NothingToDeclare`]; a pin not told now is
/// [`SameMeaningRefusal::ArtifactUnknown`]; a pin gone is
/// [`SameMeaningRefusal::ArtifactAbsent`]; a pin stale does not compare now is
/// [`SameMeaningRefusal::NotComparedNow`], with the first such pin's reason; else
/// [`SameMeaningRefusal::NotPinned`] when a hash was named, and
/// [`SameMeaningRefusal::NothingToDeclare`] when none was. So a name no pin holds a hash
/// of has nothing to declare, whatever is there now. A document that is not valid is
/// [`SameMeaningRefusal::DocNotValid`] before any pin is read, and so is a name no
/// document links, or that documents link in two spaces.
///
/// A second declaration of the same edit is refused: here as
/// [`SameMeaningRefusal::NothingToDeclare`] when the ledger joins the edit already, and
/// by [`Framed::not_declared_by`] as [`SameMeaningRefusal::AlreadyDeclaredBy`] when the
/// one who declares it declared it before, which adds no name.
///
/// The one place the earlier hashes are chosen.
pub fn frame_same_meaning<'l>(
    shelf: &Shelf,
    ledger: &'l Ledger,
    target: SameMeaningTarget,
    from: Option<ContentHash>,
) -> Result<Framed<'l>, SameMeaningRefusal> {
    // Every approval's pins are compared only once the target is told, so a name no
    // document links, or links in two spaces, is refused before that work.
    match target {
        SameMeaningTarget::Doc(id) => {
            let pins = PinsNow::of(ledger, shelf);
            let now = ComparedNow::of(&pins);
            let group = now
                .doc(id)
                .map_err(|failure| SameMeaningRefusal::DocNotValid {
                    id,
                    tag: failure.tag(),
                })?;
            let named = from.map(DocHash::restore);
            let Some(open) = group
                .open()
                .filter(|open| named.is_none_or(|hash| open.from.contains(&hash)))
            else {
                return Err(group.refusal(named));
            };
            let joins = ledger.doc_joins(id);
            let from = earlier(&open.from, named, group.own_edited(), |hash, base| {
                joins.joins(hash, base).is_some()
            })?;
            Ok(Framed(FramedKind::Doc {
                id,
                to: open.now,
                from,
                joins,
            }))
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
            // A pin is taken by the name it is read as, so an edit made with a move is
            // declared at the new name; an artifact's hash is made by a comparison, so
            // the edits are taken under one comparison at a time.
            let pins = PinsNow::of(ledger, shelf);
            let now = ComparedNow::of(&pins);
            let group = now.artifact(&artifact);
            let named = from.map(ArtifactHash::restore);
            let opens: Vec<_> = group
                .comparisons()
                .into_iter()
                .filter_map(|comparison| group.open(comparison))
                .collect();
            let open = match named {
                Some(hash) => opens.into_iter().find(|open| open.from.contains_key(&hash)),
                None if opens.len() > 1 => {
                    return Err(SameMeaningRefusal::SeveralEarlier {
                        hashes: opens
                            .iter()
                            .flat_map(|open| open.from.keys())
                            .map(ToString::to_string)
                            .collect(),
                    });
                }
                None => opens.into_iter().next(),
            };
            let Some(open) = open else {
                return Err(group.refusal(named));
            };
            // The declarations each pin of each earlier hash is read with.
            let mut joins: BTreeMap<ArtifactHash, NonEmptyVec<ArtifactJoins<'l>>> = open
                .from
                .iter()
                .map(|(hash, reads)| (*hash, reads.map(|read| ledger.pin_joins(read))))
                .collect();
            let hashes: BTreeSet<ArtifactHash> = open.from.keys().copied().collect();
            let from = earlier(&hashes, named, None, |hash, base| {
                joins
                    .get(hash)
                    .is_some_and(|each| each.iter().all(|j| j.joins(hash, base).is_some()))
            })?;
            let from: Vec<(ArtifactHash, NonEmptyVec<ArtifactJoins<'l>>)> = from
                .iter()
                .filter_map(|hash| joins.remove(hash).map(|each| (*hash, each)))
                .collect();
            let from =
                NonEmptyVec::from_vec(from).map_err(|_| SameMeaningRefusal::NothingToDeclare)?;
            Ok(Framed(FramedKind::Artifact {
                artifact,
                to: open.now,
                from,
            }))
        }
    }
}

/// What [`frame_same_meaning`] offers to declare, framed on the ledger `'l`: the edit,
/// with the declarations that ledger reads for the content of each of its earlier
/// hashes (of the document, or along the moves of each pin that holds the hash), which
/// its second step reads ([`Framed::not_declared_by`]). Only `frame_same_meaning` makes
/// one, and it holds the hashes, the name and the declarations of that ledger: no pin
/// and no shelf.
///
/// It is tied to the ledger it was framed from, so its second step reads the
/// declarations of that ledger and of no other, and it is not kept past it:
///
/// ```compile_fail
/// use whyshelf_core::domain::actor::Actor;
/// use whyshelf_core::domain::shelf::build::Shelf;
/// use whyshelf_core::domain::shelf::ledger::{Ledger, SameMeaningTarget, frame_same_meaning};
///
/// fn f(ledger: Ledger, shelf: &Shelf, target: SameMeaningTarget, by: &Actor) {
///     let framed = frame_same_meaning(shelf, &ledger, target, None).unwrap();
///     drop(ledger);
///     let _ = framed.not_declared_by(by);
/// }
/// ```
///
/// The same without the `drop` is read:
///
/// ```
/// use whyshelf_core::domain::actor::Actor;
/// use whyshelf_core::domain::shelf::build::Shelf;
/// use whyshelf_core::domain::shelf::ledger::{Ledger, SameMeaningTarget, frame_same_meaning};
///
/// fn f(ledger: Ledger, shelf: &Shelf, target: SameMeaningTarget, by: &Actor) {
///     let framed = frame_same_meaning(shelf, &ledger, target, None).unwrap();
///     let _ = framed.not_declared_by(by);
/// }
/// ```
#[derive(Debug)]
pub struct Framed<'l>(FramedKind<'l>);

/// What is framed, by the kind of its content: the edit, and the declarations its second
/// step reads, side by side.
#[derive(Debug)]
enum FramedKind<'l> {
    /// A document: one content, read with the declarations about it.
    Doc {
        id: DocId,
        to: DocHash,
        from: NonEmptySet<DocHash>,
        joins: DocJoins<'l>,
    },
    /// An artifact: each earlier hash, in their order, with the declarations each pin
    /// that holds it is read with.
    Artifact {
        artifact: ArtifactKey,
        to: ArtifactHash,
        from: NonEmptyVec<(ArtifactHash, NonEmptyVec<ArtifactJoins<'l>>)>,
    },
}

impl Framed<'_> {
    /// The edit it would declare.
    pub fn of(&self) -> SameMeaningOf {
        match &self.0 {
            FramedKind::Doc { id, to, from, .. } => SameMeaningOf::Doc {
                id: *id,
                from: from.clone(),
                to: *to,
            },
            FramedKind::Artifact { artifact, to, from } => SameMeaningOf::Artifact {
                artifact: artifact.clone(),
                from: NonEmptySet::new(from.first().0, from.iter().map(|(hash, _)| *hash)),
                to: *to,
            },
        }
    }

    /// The edit, without the earlier hashes that `by` already declared to mean what its
    /// hash means, in a declaration not revoked among those the ledger it was framed
    /// from reads for it: of the document, or along the moves of every pin that holds
    /// the hash, as freshness reads each pin. A hash is left out only when `by` declared
    /// its edge along the way of every pin that holds it. That edge has `by`'s name
    /// already, and one name counts once, so a second declaration of it adds nothing
    /// (as an approval that repeats its actor's own reading writes nothing). Where the
    /// ledger joins the edit already, the frame refuses it first; this is for one whose
    /// edge does not count yet and awaits another name, so it happens only where a rule
    /// asks for more than one name (in a project that declares no rule, the first
    /// declaration joins the edge). With no earlier hash left, it is refused as
    /// [`SameMeaningRefusal::AlreadyDeclaredBy`].
    pub fn not_declared_by(self, by: &Actor) -> Result<SameMeaningOf, SameMeaningRefusal> {
        match self.0 {
            FramedKind::Doc {
                id,
                to,
                from,
                joins,
            } => {
                let from = unrepeated(from, to, &joins.declared_by(by), by)?;
                Ok(SameMeaningOf::Doc { id, from, to })
            }
            FramedKind::Artifact { artifact, to, from } => {
                let left: BTreeSet<ArtifactHash> = from
                    .iter()
                    .filter(|(hash, each)| {
                        !each
                            .iter()
                            .all(|joins| joins.declared_by(by).contains(&ends(*hash, to)))
                    })
                    .map(|(hash, _)| *hash)
                    .collect();
                let from = NonEmptySet::from_set(left)
                    .map_err(|_| SameMeaningRefusal::AlreadyDeclaredBy { actor: by.clone() })?;
                Ok(SameMeaningOf::Artifact { artifact, from, to })
            }
        }
    }
}

/// The edges of `declared` that `by` declared.
pub(super) fn declared_by<'a, H: Ord>(
    declared: impl IntoIterator<Item = (&'a SameMeaning, H, H)>,
    by: &Actor,
) -> BTreeSet<(H, H)> {
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

/// The earlier hashes a declaration covers, out of the hashes `open` stale tells as
/// edited (none of them the hash now): the rule of [`frame_same_meaning`], for both kinds
/// of subject.
///
/// `named` is the hash the person named, one of `open`; `own` the hash the document's
/// own approval saw, when stale tells it as edited; and `joined(h, b)` whether the
/// declarations read for every pin that holds `h` join it to `b`. With a hash named, or
/// else the own one, the earlier hashes are it and those joined to it. With neither,
/// all of `open` are when one of them is joined by every other, which asks of no order
/// of the hashes; else they are several earlier hashes.
fn earlier<H: Ord + Copy + std::fmt::Display>(
    open: &BTreeSet<H>,
    named: Option<H>,
    own: Option<H>,
    joined: impl Fn(&H, &H) -> bool,
) -> Result<NonEmptySet<H>, SameMeaningRefusal> {
    let from: BTreeSet<H> = match named.or(own.filter(|own| open.contains(own))) {
        Some(base) => open
            .iter()
            .copied()
            .filter(|hash| *hash == base || joined(hash, &base))
            .collect(),
        None if open.len() <= 1
            || open
                .iter()
                .any(|base| open.iter().all(|hash| hash == base || joined(hash, base))) =>
        {
            open.clone()
        }
        None => {
            return Err(SameMeaningRefusal::SeveralEarlier {
                hashes: open.iter().map(ToString::to_string).collect(),
            });
        }
    };
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
