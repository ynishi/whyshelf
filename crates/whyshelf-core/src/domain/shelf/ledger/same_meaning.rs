//! Declarations that an edit keeps the meaning: [`SameMeaning`].
//!
//! A person says that what a document or a file is now means what it meant at one or
//! more earlier hashes (a typo fixed, a sentence reworded, a comment of the code
//! changed). Freshness then takes a pin of one of those hashes as a pin of the hash
//! now, and tells it as a notice instead of a cause: the approval holds, and who said
//! the edit keeps the meaning, and why, stays readable.
//!
//! Nothing is guessed. Two hashes mean the same only through declarations, read on
//! every assessment like every other record ([`Ledger::doc_same_meaning`],
//! [`Ledger::artifact_same_meaning`]); a declaration is withdrawn by a
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
//! [`Ledger::artifact_same_meaning`]: super::Ledger::artifact_same_meaning

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::hash::ContentHash;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptySet, NonEmptyVec};

use super::super::artifact::{ArtifactHash, ArtifactState};
use super::super::build::{FailureTag, Shelf};
use super::super::doc::DocHash;
use super::super::schema::ArtifactPropagation;
use super::{DeclaredStatus, JudgedAt, JudgementId, Ledger, PinnedArtifact};

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
    /// A file that is no document, named by its path.
    Artifact {
        /// The path.
        path: RelPath,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameMeaning {
    pub(super) id: JudgementId,
    pub(super) of: SameMeaningOf,
    pub(super) actor: Actor,
    pub(super) at: JudgedAt,
    pub(super) reason: NonBlank,
}

impl SameMeaning {
    /// A declaration. Refuses one whose hash now is among the earlier hashes. The
    /// reason is required: a declaration is a claim a reader must be able to check.
    pub fn new(
        id: JudgementId,
        of: SameMeaningOf,
        actor: Actor,
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
            actor,
            at,
            reason,
        })
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
        &self.actor
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
    /// A file that is no document, as a link writes its path.
    Artifact(RelPath),
}

/// Why a declaration is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SameMeaningRefusal {
    /// The document is not a valid document now, so it has no hash to declare.
    #[error("{id} is not a valid document now ({tag:?})")]
    DocNotValid {
        /// The document.
        id: DocId,
        /// How it is not.
        tag: FailureTag,
    },
    /// No document links the path, so what is there was never read.
    #[error("no document links `{path}`")]
    ArtifactNotLinked {
        /// The path.
        path: RelPath,
    },
    /// The path has no file now.
    #[error("`{path}` has no file now")]
    ArtifactAbsent {
        /// The path.
        path: RelPath,
    },
    /// Every approval that stands pinned the hash now, or one that already means the
    /// same as it.
    #[error("no approval that stands pinned another hash of it: there is nothing to declare")]
    NothingToDeclare,
    /// The hash named as the earlier one is no hash an approval that stands pinned
    /// for it (or it is the hash now, or already means the same as it).
    #[error("no approval that stands pinned {hash} for it, apart from the hash now")]
    NotPinned {
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
/// declaration of the same edit is refused as [`SameMeaningRefusal::NothingToDeclare`].
///
/// The one place the earlier hashes are chosen.
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
            for subject in ledger.subjects() {
                let DeclaredStatus::Approved { seen, frame, .. } = ledger.status_of(*subject)
                else {
                    continue;
                };
                if *subject == id {
                    pinned.insert(seen);
                    own = Some(seen);
                }
                pinned.extend(
                    frame
                        .iter()
                        .filter(|pin| pin.target() == id)
                        .map(|pin| *pin.hash()),
                );
            }
            let same = |a: &DocHash, b: &DocHash| ledger.doc_same_meaning(id, a, b).is_some();
            let from = earlier(pinned, now, from.map(DocHash::restore), own, same)?;
            Ok(SameMeaningOf::Doc { id, from, to: now })
        }
        SameMeaningTarget::Artifact(path) => {
            let state = shelf
                .docs()
                .values()
                .flat_map(|doc| {
                    [ArtifactPropagation::Depends, ArtifactPropagation::Informs]
                        .into_iter()
                        .flat_map(|p| doc.artifact_links_of(p))
                        .flat_map(|(_, artifacts)| artifacts.iter())
                        .filter(|a| *a.path() == path)
                        .map(|a| *a.state())
                        .collect::<Vec<_>>()
                })
                .next()
                .ok_or_else(|| SameMeaningRefusal::ArtifactNotLinked { path: path.clone() })?;
            let ArtifactState::Present(now) = state else {
                return Err(SameMeaningRefusal::ArtifactAbsent { path });
            };
            let mut pinned = BTreeSet::new();
            for subject in ledger.subjects() {
                let DeclaredStatus::Approved { artifacts, .. } = ledger.status_of(*subject) else {
                    continue;
                };
                pinned.extend(artifacts.iter().filter_map(|pin| match pin.seen() {
                    PinnedArtifact::Present { hash, .. } if *pin.path() == path => Some(*hash),
                    _ => None,
                }));
            }
            let same = |a: &ArtifactHash, b: &ArtifactHash| {
                ledger.artifact_same_meaning(&path, a, b).is_some()
            };
            let from = earlier(pinned, now, from.map(ArtifactHash::restore), None, same)?;
            Ok(SameMeaningOf::Artifact {
                path,
                from,
                to: now,
            })
        }
    }
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

/// The declarations that join `a` to `b`, in the order of the way from one to the
/// other, or `None` when no way of declarations joins them.
///
/// Each declaration joins each of its earlier hashes to its hash then, both ways: a
/// meaning kept from A to B is kept from B to A. The way is a shortest one, so the
/// answer is the same for the same records in any order.
pub(super) fn declared_way<H: Ord + Copy>(
    edges: impl IntoIterator<Item = (JudgementId, H, H)>,
    a: H,
    b: H,
) -> Option<NonEmptyVec<JudgementId>> {
    let mut next: BTreeMap<H, Vec<(H, JudgementId)>> = BTreeMap::new();
    for (id, x, y) in edges {
        next.entry(x).or_default().push((y, id));
        next.entry(y).or_default().push((x, id));
    }
    let mut came: BTreeMap<H, (H, JudgementId)> = BTreeMap::new();
    let mut queue = VecDeque::from([a]);
    let mut seen = BTreeSet::from([a]);
    while let Some(at) = queue.pop_front() {
        if at == b {
            let mut way = Vec::new();
            let mut here = b;
            while let Some((before, id)) = came.get(&here) {
                way.push(*id);
                here = *before;
            }
            way.reverse();
            return NonEmptyVec::from_vec(way).ok();
        }
        for (onward, id) in next.get(&at).into_iter().flatten() {
            if seen.insert(*onward) {
                came.insert(*onward, (at, *id));
                queue.push_back(*onward);
            }
        }
    }
    None
}
