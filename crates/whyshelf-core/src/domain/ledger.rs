//! The ledger: the judgements people made, as a set that merges by union.
//!
//! A project has two ledgers, the public one (committed with the project) and the
//! private one (for one person). Each is a set of [`Judgement`] records that is only
//! ever added to. What a document or an item *is* comes from its file; the ledger
//! holds what people *decided* about it. State is computed from the files and the
//! ledgers whenever it is asked for, and never written down.
//!
//! # The merge declaration
//!
//! 1. Merging the documents (content, classification, identity, place) is Git's
//!    work. The domain reads the files after a merge and has no rule for combining
//!    two versions of one.
//! 2. **A ledger is an append-only set. Merging two ledgers is their union, and
//!    union is commutative, associative and idempotent**: it does not matter in
//!    which order branches are merged or how often. ([`Ledger::union`])
//!    - Two records of one [`JudgementId`] must be equal in the canonical bytes
//!      ([`Judgement::canonical_bytes`]). If two differ (copied by hand, damaged),
//!      **both** are excluded and the identifier is listed as corrupt, for `fsck`
//!      ([`Ledger::corrupt_ids`], [`Ledgers::corrupt`]). Corrupt absorbs: it stays
//!      corrupt in any union, so the result does not depend on the order either.
//!    - The same record in the public and the private ledger (a publication copies
//!      it byte for byte) reads as one.
//! 3. **The objects form a set that can also delete (a 2P-set).** A text hash with a
//!    [`Purge`](JudgementKind::Purge) record reads as gone even if the object file is
//!    there ([`Ledgers::object_reading`]). Whoever writes an object does not put a
//!    purged hash back; a branch that diverged before the purge and put the same hash
//!    is read as gone after the merge. A purge does not remove the text from Git's
//!    history.
//! 4. What disagrees after a merge is not a merge problem. It is a fact about the
//!    present state (duplicate ids, a changed upstream) and is reported there.
//!
//! The ledgers in a [`Ledgers`] are two such sets; [`Ledgers::union`] merges both.
//!
//! # Revocation, and why concurrent approvals stand
//!
//! A [`Revoke`](JudgementKind::Revoke) names records by identifier. It takes away
//! exactly the records it names, and so:
//!
//! - It is built by [`Ledgers::revoke`], which names **the visible, unrevoked
//!   records** of the attestation (or retirement) asked for, by the judge asked for
//!   (the one revoking, if none is named). Visible means counting in the ledger the
//!   revocation is written to, so that it takes a record away for everyone who reads
//!   that record, not only for the one revoking.
//! - A record somebody wrote on another branch, which the revoker could not see, is
//!   not named. After the merge it **stands** (add-wins), as a remove from an
//!   observed-remove set removes only what it observed.
//! - Only an approval, an adoption or a retirement can be named. A revocation, a
//!   purge and a publication stand; a request to name one is refused
//!   ([`RevokeError::NotRevocable`]), and a revocation that does so anyway (a
//!   ledger written by hand) is read as naming nothing.
//! - A record of another judge needs `force` ([`RevokeError::RevokeOthers`]).
//!
//! Time orders nothing here. A revoke is not "later" than an approval; it is a
//! record that names it.
//!
//! # What is approved
//!
//! "Is `(gate, subject, hash)` approved?" is asked of the whole set: an unrevoked
//! approval or adoption exists for it ([`Ledgers::is_approved`]). The
//! [`Basis`] says which: a gate's machine check passed, or
//! an earlier decision was taken in.
//!
//! A measurement (gate CM) binds to the Spec and the hash of its hypotheses, not to
//! the record of the CS approval that fixed them ([`Ledgers::is_measured`]): with
//! two CS approvals of one version there is no saying which one is meant.
//!
//! # Retire
//!
//! A [`Retire`](JudgementKind::Retire) says that a document or item that has
//! judgements, and whose file is gone, is retired. Its constructor needs the file to
//! be missing ([`Judgement::retire`]). The retirement counts only while the file is
//! absent ([`Retirement::Retired`]). If the file comes back, the retirement does not
//! apply and the document is present again, and a notice that a retired document has
//! returned is given for as long as the retirement is not revoked
//! ([`Retirement::Returned`]): the state is not recorded, so "once" cannot be
//! kept. If the file goes again, the unrevoked retirement is alive again.
//!
//! # Two ledgers, and what counts
//!
//! The ledger a judgement is written to is decided in one place,
//! [`JudgementKind::ledger`]:
//!
//! | judgement about | goes to |
//! |---|---|
//! | a document | the ledger of the document's visibility |
//! | an item | the ledger of its Design's visibility |
//! | a purge | the ledger of the visibility of `doc` |
//! | a revoke | the ledger of what the records it names are about |
//! | a publication | the public ledger |
//! | a target whose file is gone | the ledger that already holds judgements about it (the public one if both), else it cannot be made ([`JudgementError::UnknownTarget`]) |
//!
//! **A publication is committed by one record**, [`Published`](JudgementKind::Published),
//! written in the public ledger once the copied records are there; each record it
//! names is an unrevoked approval, adoption or retirement of that document or of one
//! of its items ([`Judgement::published`]). A record in the
//! public ledger whose `origin` is private is **inert** until a `Published` of its own
//! document names it (a record in the public ledger counts if its origin is Public, or
//! a Published of its own document names it; a `Published` of document A activates
//! nothing about B): it does not count for approval, for presence or for revocation. So a
//! publication that stopped before the commit point has done nothing as far as the
//! public side can see, and abandoning it needs no clean-up in the ledger. A record
//! the owner holds in the private ledger counts there whether or not it was copied.
//!
//! **Presence.** A document with no file under either root is *missing* if the
//! public ledger holds a record that counts about it (`origin` public, or named by a
//! `Published`), whatever the private root is. If the only records are inert, the
//! public side does not know the document; the owner's private ledger decides
//! ([`Ledgers::judged_in`]). The rule needs only whether a `Published` exists and
//! the `origin` of each record; no order between them.
//!
//! # Where to look
//!
//! | item | holds |
//! |---|---|
//! | [`Ledger`] | one ledger as a set of records, with [`union`](Ledger::union) |
//! | [`Ledgers`] | the public and the private ledger read together; every query |
//! | [`RevokeRequest`] | what [`Ledgers::revoke`] is asked |
//! | [`Retirement`], [`ObjectReading`] | the answers about a retirement and an object |

use std::collections::{BTreeMap, BTreeSet};

use super::actor::Timestamp;
use super::adoption;
use super::approval::Gate;
use super::corpus::{Corpus, Visibility};
use super::error::{JudgementError, RevokeError};
use super::hash::{ContentHash, TextHash};
use super::id::{DocId, EntityId, SpecId};
use super::judge::Judge;
use super::judgement::{
    About, Basis, FilePresence, Judgement, JudgementId, JudgementKind, Pinning, Target,
};
use super::nonempty::{NonBlank, NonEmptyVec};

/// What a ledger holds under one identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    /// One record, with its canonical bytes.
    Sound {
        judgement: Box<Judgement>,
        bytes: Vec<u8>,
    },
    /// Two records of one identifier that differ in the bytes: neither is kept.
    Corrupt,
}

impl Slot {
    fn of(judgement: Judgement) -> Self {
        let bytes = judgement.canonical_bytes();
        Self::Sound {
            judgement: Box::new(judgement),
            bytes,
        }
    }

    /// The slot of two records of one identifier. Equal records are one; any other
    /// pair, and anything met with a corrupt slot, is corrupt.
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (
                Self::Sound {
                    judgement,
                    bytes: left,
                },
                Self::Sound { bytes: right, .. },
            ) if left == right => Self::Sound {
                judgement,
                bytes: left,
            },
            _ => Self::Corrupt,
        }
    }

    fn judgement(&self) -> Option<&Judgement> {
        match self {
            Self::Sound { judgement, .. } => Some(judgement),
            Self::Corrupt => None,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Sound { bytes, .. } => Some(bytes),
            Self::Corrupt => None,
        }
    }
}

/// One ledger: a set of judgement records, only ever added to.
///
/// Built from records, and merged with another ledger by [`union`](Self::union).
/// Which ledger it is (public or private) is not part of the value: that is
/// [`Ledgers`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    slots: BTreeMap<JudgementId, Slot>,
}

impl Ledger {
    /// An empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// A ledger of the records given, in any order and with repeats. The decoding of
    /// stored records into [`Judgement`]s is an adapter's work; the canonical bytes
    /// that tell two records of one identifier apart are computed here from the
    /// decoded value.
    pub fn from_judgements(judgements: impl IntoIterator<Item = Judgement>) -> Self {
        let mut ledger = Self::new();
        for judgement in judgements {
            ledger.insert(judgement);
        }
        ledger
    }

    /// Add a record. A record of an identifier the ledger holds is merged with it:
    /// equal records are one, different records make the identifier corrupt.
    pub fn insert(&mut self, judgement: Judgement) {
        self.absorb(judgement.id(), Slot::of(judgement));
    }

    fn absorb(&mut self, id: JudgementId, slot: Slot) {
        let merged = match self.slots.remove(&id) {
            Some(held) => held.merge(slot),
            None => slot,
        };
        self.slots.insert(id, merged);
    }

    /// The union of two ledgers: what merging two branches gives.
    ///
    /// Commutative, associative and idempotent.
    #[must_use]
    pub fn union(&self, other: &Ledger) -> Ledger {
        let mut merged = self.clone();
        for (id, slot) in &other.slots {
            merged.absorb(*id, slot.clone());
        }
        merged
    }

    /// The record of an identifier, unless there is none or it is corrupt.
    pub fn get(&self, id: &JudgementId) -> Option<&Judgement> {
        self.slots.get(id).and_then(Slot::judgement)
    }

    /// Whether the ledger holds a sound record of this identifier.
    pub fn contains(&self, id: &JudgementId) -> bool {
        self.get(id).is_some()
    }

    /// Whether two records of this identifier were found that differ.
    pub fn is_corrupt(&self, id: &JudgementId) -> bool {
        matches!(self.slots.get(id), Some(Slot::Corrupt))
    }

    /// The identifiers with two records that differ, in order.
    pub fn corrupt_ids(&self) -> impl Iterator<Item = JudgementId> + '_ {
        self.slots
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Corrupt))
            .map(|(id, _)| *id)
    }

    /// The sound records, ordered by identifier.
    pub fn judgements(&self) -> impl Iterator<Item = &Judgement> {
        self.slots.values().filter_map(Slot::judgement)
    }

    /// Whether the ledger holds nothing, not even a corrupt identifier.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn bytes_of(&self, id: &JudgementId) -> Option<&[u8]> {
        self.slots.get(id).and_then(Slot::bytes)
    }
}

/// On which side of the project a record counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sides {
    public: bool,
    private: bool,
}

impl Sides {
    fn on(self, side: Visibility) -> bool {
        match side {
            Visibility::Public => self.public,
            Visibility::Private => self.private,
        }
    }
}

/// What a retired target is, given whether its file is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retirement {
    /// No retirement stands for it.
    NotRetired,
    /// A retirement stands and the file is gone: the target is retired, and nothing is
    /// said about it.
    Retired,
    /// A retirement stands and the file is back: the target is present again, with the
    /// notice that a retired document returned, until the retirement is revoked.
    Returned,
}

/// What an object reads as, given whether its file is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectReading {
    /// A purge record names its hash: gone, whether or not the file is there.
    Purged,
    /// No purge, and the file is there.
    Present,
    /// No purge, and no file.
    Missing,
}

/// An approval or adoption that stands, with what it pins.
#[derive(Debug)]
pub struct StandingApproval<'a> {
    judgement: &'a Judgement,
    pinning: Pinning<'a>,
}

impl<'a> StandingApproval<'a> {
    /// The record.
    pub fn judgement(&self) -> &'a Judgement {
        self.judgement
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.judgement.id()
    }

    /// Who approved or adopted.
    pub fn judge(&self) -> &'a Judge {
        self.judgement.judge()
    }

    /// The gate.
    pub fn gate(&self) -> Gate {
        self.pinning.gate
    }

    /// The document or item.
    pub fn subject(&self) -> EntityId {
        self.pinning.subject
    }

    /// The hash pinned.
    pub fn hash(&self) -> &'a ContentHash {
        self.pinning.hash
    }

    /// What it stands on.
    pub fn basis(&self) -> &Basis {
        &self.pinning.basis
    }
}

/// What to revoke.
#[derive(Debug, Clone)]
pub enum RevokeWhat {
    /// The approvals and adoptions of `(gate, subject, hash)` that are visible and
    /// unrevoked, by `of` (the one revoking, if none).
    Attestation {
        /// The gate.
        gate: Gate,
        /// The document or item.
        subject: EntityId,
        /// The hash approved.
        hash: ContentHash,
        /// Whose records; the one revoking if `None`.
        of: Option<Judge>,
    },
    /// The retirements of the target that are visible and unrevoked, by `of` (the one
    /// revoking, if none).
    Retire {
        /// What was retired.
        target: Target,
        /// Whose records; the one revoking if `None`.
        of: Option<Judge>,
    },
    /// These records, named one by one.
    Records(NonEmptyVec<JudgementId>),
}

/// What [`Ledgers::revoke`] is asked: an input, not a record.
#[derive(Debug, Clone)]
pub struct RevokeRequest {
    /// The identity of the revocation (made by the caller).
    pub id: JudgementId,
    /// When it is written, for display.
    pub at: Timestamp,
    /// Who revokes.
    pub by: Judge,
    /// The ledger to write it in; see [`JudgementKind::ledger`].
    pub origin: Visibility,
    /// Whether a record of another judge may be revoked.
    pub force: bool,
    /// Why.
    pub reason: NonBlank,
    /// What to revoke.
    pub what: RevokeWhat,
}

/// The public and the private ledger read together: every question the rules ask.
///
/// Built once from the two ledgers ([`new`](Self::new)); the answers are derived and
/// never stored anywhere. `Ledgers` for a machine that cannot see the private ledger
/// (a CI run, a clone) is built from the public ledger and an empty private one.
#[derive(Debug, Clone, Default)]
pub struct Ledgers {
    public: Ledger,
    private: Ledger,
    sides: BTreeMap<JudgementId, Sides>,
    corrupt: BTreeSet<JudgementId>,
    published: BTreeMap<DocId, BTreeSet<JudgementId>>,
    revoked: BTreeSet<JudgementId>,
    purged: BTreeSet<TextHash>,
}

impl Ledgers {
    /// Read two ledgers together.
    pub fn new(public: Ledger, private: Ledger) -> Self {
        let mut corrupt: BTreeSet<JudgementId> =
            public.corrupt_ids().chain(private.corrupt_ids()).collect();
        for id in public.slots.keys() {
            if let (Some(left), Some(right)) = (public.bytes_of(id), private.bytes_of(id))
                && left != right
            {
                corrupt.insert(*id);
            }
        }

        let mut published: BTreeMap<DocId, BTreeSet<JudgementId>> = BTreeMap::new();
        for judgement in public.judgements() {
            if corrupt.contains(&judgement.id()) || judgement.origin() != Visibility::Public {
                continue;
            }
            if let JudgementKind::Published { doc, records } = judgement.kind() {
                published
                    .entry(*doc)
                    .or_default()
                    .extend(records.iter().copied());
            }
        }

        let mut ledgers = Self {
            public,
            private,
            sides: BTreeMap::new(),
            corrupt,
            published,
            revoked: BTreeSet::new(),
            purged: BTreeSet::new(),
        };
        // A `Published` activates only the records of its own document; the home of a
        // record is read by the same rule the constructor of `Published` uses.
        let named: BTreeSet<JudgementId> = ledgers
            .published
            .iter()
            .flat_map(|(doc, ids)| {
                ids.iter().filter(|id| {
                    ledgers
                        .public
                        .get(id)
                        .is_some_and(|record| ledgers.is_of_doc(record, *doc))
                })
            })
            .copied()
            .collect();

        let mut sides = BTreeMap::new();
        for id in ledgers
            .public
            .slots
            .keys()
            .chain(ledgers.private.slots.keys())
        {
            if ledgers.corrupt.contains(id) || sides.contains_key(id) {
                continue;
            }
            let on_public = ledgers.public.get(id).is_some_and(|judgement| {
                judgement.origin() == Visibility::Public || named.contains(id)
            });
            let on_private = ledgers.private.contains(id);
            if on_public || on_private {
                sides.insert(
                    *id,
                    Sides {
                        public: on_public,
                        private: on_private,
                    },
                );
            }
        }

        ledgers.sides = sides;
        ledgers.revoked = ledgers.revocations();
        ledgers.purged = ledgers
            .judgements()
            .filter_map(|judgement| match judgement.kind() {
                JudgementKind::Purge { object, .. } => Some(*object),
                _ => None,
            })
            .collect();
        ledgers
    }

    /// The records that standing revocations take away: of the records they name,
    /// the ones a revocation may name.
    fn revocations(&self) -> BTreeSet<JudgementId> {
        let mut revoked = BTreeSet::new();
        for judgement in self.judgements() {
            if let JudgementKind::Revoke { targets, .. } = judgement.kind() {
                for target in targets.iter() {
                    if self
                        .get(target)
                        .is_some_and(|record| record.kind().is_revocable())
                    {
                        revoked.insert(*target);
                    }
                }
            }
        }
        revoked
    }

    /// The merge of two readings: the union of each of the ledgers.
    #[must_use]
    pub fn union(&self, other: &Ledgers) -> Ledgers {
        Ledgers::new(
            self.public.union(&other.public),
            self.private.union(&other.private),
        )
    }

    /// The public ledger as it was given.
    pub fn public(&self) -> &Ledger {
        &self.public
    }

    /// The private ledger as it was given.
    pub fn private(&self) -> &Ledger {
        &self.private
    }

    /// The identifiers excluded because two records of one differ, in either ledger
    /// or between them. For `fsck`.
    pub fn corrupt(&self) -> &BTreeSet<JudgementId> {
        &self.corrupt
    }

    /// A sound record of either ledger, whether or not it counts.
    pub(super) fn record(&self, id: &JudgementId) -> Option<&Judgement> {
        if self.corrupt.contains(id) {
            return None;
        }
        self.public.get(id).or_else(|| self.private.get(id))
    }

    /// The records that count, ordered by identifier: sound, and either in the
    /// private ledger or in the public one with a public `origin` or named by a
    /// `Published`. Revoked records are among them.
    pub fn judgements(&self) -> impl Iterator<Item = &Judgement> {
        self.sides.keys().filter_map(|id| self.record(id))
    }

    /// The record of an identifier, if it counts.
    pub fn get(&self, id: &JudgementId) -> Option<&Judgement> {
        if self.sides.contains_key(id) {
            self.record(id)
        } else {
            None
        }
    }

    /// Whether the record is in the public ledger, is not corrupt, and does not count
    /// yet: its `origin` is private and no `Published` names it. It is not in the
    /// private ledger either (there it would count).
    pub fn is_inert(&self, id: &JudgementId) -> bool {
        self.public.contains(id) && !self.corrupt.contains(id) && !self.sides.contains_key(id)
    }

    /// Whether a standing revocation names the record.
    pub fn is_revoked(&self, id: &JudgementId) -> bool {
        self.revoked.contains(id)
    }

    /// The documents a `Published` record names, for the corpus to tell a
    /// publication cut short from a duplicate.
    pub fn published_docs(&self) -> BTreeSet<DocId> {
        self.published.keys().copied().collect()
    }

    /// The approvals and adoptions that count and are not revoked, ordered by
    /// identifier.
    pub fn approvals(&self) -> impl Iterator<Item = StandingApproval<'_>> {
        self.judgements().filter_map(|judgement| {
            if self.revoked.contains(&judgement.id()) {
                return None;
            }
            judgement
                .kind()
                .pinning()
                .map(|pinning| StandingApproval { judgement, pinning })
        })
    }

    /// The approvals and adoptions of exactly `(gate, subject, hash)` that stand.
    pub fn approvals_at<'a>(
        &'a self,
        gate: Gate,
        subject: EntityId,
        hash: &'a ContentHash,
    ) -> impl Iterator<Item = StandingApproval<'a>> + 'a {
        self.approvals().filter(move |approval| {
            approval.pinning.gate == gate
                && approval.pinning.subject == subject
                && approval.pinning.hash == hash
        })
    }

    /// Whether `(gate, subject, hash)` is approved: an unrevoked approval or adoption
    /// exists for it.
    ///
    /// It is never "does it match the latest": two branches that approve two versions
    /// of one thing do not undo each other.
    pub fn is_approved(&self, gate: Gate, subject: EntityId, hash: &ContentHash) -> bool {
        self.approvals_at(gate, subject, hash).next().is_some()
    }

    /// Whether the hypotheses of a Spec, at `hyp`, have a measurement recorded.
    ///
    /// The measurement binds to `(Spec, hyp_hash)`, not to a CS record: whichever of
    /// the CS approvals of that version is revoked, the measurement stands.
    pub fn is_measured(&self, spec: SpecId, hyp: &ContentHash) -> bool {
        self.is_approved(Gate::Cm, spec.entity(), hyp)
    }

    /// What the retirements of `target` come to, given whether its file is there.
    pub fn retirement(&self, target: &Target, file: FilePresence) -> Retirement {
        let retired = self.judgements().any(|judgement| {
            !self.revoked.contains(&judgement.id())
                && matches!(judgement.kind(), JudgementKind::Retire { target: t, .. } if t == target)
        });
        match (retired, file) {
            (false, _) => Retirement::NotRetired,
            (true, FilePresence::Absent) => Retirement::Retired,
            (true, FilePresence::Present) => Retirement::Returned,
        }
    }

    /// Whether a purge record that counts names this text.
    pub fn is_purged(&self, text: &TextHash) -> bool {
        self.purged.contains(text)
    }

    /// What an object reads as. A purged hash is gone even if the file is there.
    pub fn object_reading(&self, text: &TextHash, file_present: bool) -> ObjectReading {
        if self.is_purged(text) {
            ObjectReading::Purged
        } else if file_present {
            ObjectReading::Present
        } else {
            ObjectReading::Missing
        }
    }

    /// Whether a record that counts on this side is about the target.
    ///
    /// On the public side only a record that counts in the public ledger is looked
    /// at, so an inert one is not; on the private side, only the private ledger. A
    /// revocation is about what the records it names are about.
    pub fn judged_in(&self, target: &Target, side: Visibility) -> bool {
        self.sides.iter().any(|(id, sides)| {
            sides.on(side)
                && self
                    .record(id)
                    .and_then(|record| self.subject_of(record, side))
                    .as_ref()
                    == Some(target)
        })
    }

    /// What a record is about; for a revocation, what the first record it names that
    /// counts on `side` is about.
    fn subject_of(&self, record: &Judgement, side: Visibility) -> Option<Target> {
        let direct = |kind: &JudgementKind| match kind.about() {
            About::Target(target) => Some(target),
            About::Records(_) => None,
        };
        match record.kind().about() {
            About::Target(target) => Some(target),
            About::Records(named) => named.iter().find_map(|id| {
                self.sides
                    .get(id)
                    .filter(|sides| sides.on(side))
                    .and_then(|_| self.record(id))
                    .and_then(|found| direct(found.kind()))
            }),
        }
    }

    /// Whether the record is an approval, an adoption or a retirement of `doc` or of
    /// one of its items: what a publication of `doc` may name.
    ///
    /// A retirement of an item pins no Design, so the item is taken to be `doc`'s when
    /// a sound approval or adoption of it, in either ledger, pins `doc` as its Design.
    pub(super) fn is_of_doc(&self, record: &Judgement, doc: DocId) -> bool {
        match record.kind() {
            JudgementKind::Retire {
                target: Target::Item(item),
                ..
            } => self
                .public
                .judgements()
                .chain(self.private.judgements())
                .filter(|other| !self.corrupt.contains(&other.id()))
                .any(|other| {
                    matches!(other.kind().about(), About::Target(Target::Item(i)) if i == *item)
                        && other.kind().home_doc() == Some(doc)
                }),
            kind => kind.home_doc() == Some(doc),
        }
    }

    /// Build a revocation, or say why not.
    ///
    /// The records revoked are those that are visible on the side the revocation is
    /// written to (they count in that ledger), are not revoked, are approvals,
    /// adoptions or retirements, and are the named judge's. A record that counts
    /// only in the other ledger is not visible there: a revocation written beside
    /// it would take it away for the one revoking and for nobody else. A record
    /// written elsewhere and not merged yet is not among them and will stand.
    /// The checks come in this order: what is named exists and may be revoked, then
    /// whose it is.
    pub fn revoke(&self, request: RevokeRequest) -> Result<Judgement, RevokeError> {
        let RevokeRequest {
            id,
            at,
            by,
            origin,
            force,
            reason,
            what,
        } = request;
        let visible = |id: &JudgementId| self.sides.get(id).is_some_and(|s| s.on(origin));

        let chosen: Vec<&Judgement> = match &what {
            RevokeWhat::Attestation {
                gate,
                subject,
                hash,
                of,
            } => {
                let owner = of.as_ref().unwrap_or(&by);
                self.approvals_at(*gate, *subject, hash)
                    .map(|approval| approval.judgement)
                    .filter(|judgement| {
                        visible(&judgement.id()) && judgement.judge().same_person(owner)
                    })
                    .collect()
            }
            RevokeWhat::Retire { target, of } => {
                let owner = of.as_ref().unwrap_or(&by);
                self.judgements()
                    .filter(|judgement| {
                        visible(&judgement.id())
                            && !self.revoked.contains(&judgement.id())
                            && judgement.judge().same_person(owner)
                            && matches!(judgement.kind(),
                                JudgementKind::Retire { target: t, .. } if t == target)
                    })
                    .collect()
            }
            RevokeWhat::Records(ids) => {
                let mut chosen = Vec::new();
                let mut seen = BTreeSet::new();
                for named in ids.iter() {
                    let record = self
                        .get(named)
                        .filter(|_| visible(named))
                        .ok_or(RevokeError::UnknownRecord { record: *named })?;
                    if !record.kind().is_revocable() {
                        return Err(RevokeError::NotRevocable { record: *named });
                    }
                    if self.revoked.contains(named) {
                        return Err(RevokeError::AlreadyRevoked { record: *named });
                    }
                    if seen.insert(*named) {
                        chosen.push(record);
                    }
                }
                chosen
            }
        };

        let targets =
            NonEmptyVec::from_vec(chosen.iter().map(|judgement| judgement.id()).collect())
                .map_err(|_| RevokeError::NothingToRevoke)?;
        if !force
            && let Some(other) = chosen
                .iter()
                .find(|judgement| !judgement.judge().same_person(&by))
        {
            return Err(RevokeError::RevokeOthers {
                owner: other.judge().clone(),
            });
        }
        Ok(Judgement::assemble(
            id,
            at,
            by,
            origin,
            JudgementKind::Revoke { targets, reason },
        ))
    }
}

/// The visibility of the files that carry the target, if any do.
fn corpus_visibility(
    corpus: &Corpus,
    target: Target,
) -> Result<Option<Visibility>, JudgementError> {
    let seen: BTreeSet<Visibility> = match target {
        Target::Doc(id) => corpus
            .by_id(&id)
            .iter()
            .map(|file| file.location().visibility())
            .collect(),
        Target::Item(item) => adoption::designs_holding(corpus, item)
            .into_iter()
            .map(|(file, _, _)| file.location().visibility())
            .collect(),
    };
    let mut seen = seen.into_iter();
    match (seen.next(), seen.next()) {
        (None, _) => Ok(None),
        (Some(only), None) => Ok(Some(only)),
        (Some(_), Some(_)) => Err(JudgementError::AmbiguousTarget { target }),
    }
}

/// The ledger for a judgement about `target`: the visibility of its file; if there
/// is none, the ledger already holding judgements about it, the public one first.
fn place_of(
    corpus: &Corpus,
    ledgers: &Ledgers,
    target: Target,
) -> Result<Visibility, JudgementError> {
    if let Some(visibility) = corpus_visibility(corpus, target)? {
        return Ok(visibility);
    }
    [Visibility::Public, Visibility::Private]
        .into_iter()
        .find(|side| ledgers.judged_in(&target, *side))
        .ok_or(JudgementError::UnknownTarget { target })
}

impl JudgementKind {
    /// The ledger a judgement of this kind is written in. The one place that rule
    /// is held (see the module documentation).
    ///
    /// Asked before the record exists: the answer is the `origin` the record is
    /// built with.
    pub fn ledger(&self, corpus: &Corpus, ledgers: &Ledgers) -> Result<Visibility, JudgementError> {
        if matches!(self, Self::Published { .. }) {
            return Ok(Visibility::Public);
        }
        match self.about() {
            About::Target(target) => place_of(corpus, ledgers, target),
            About::Records(named) => {
                let subject = |id: &JudgementId| -> Result<Target, JudgementError> {
                    let record = ledgers
                        .record(id)
                        .ok_or(JudgementError::UnknownRecord { record: *id })?;
                    match record.kind().about() {
                        About::Target(target) if record.kind().is_revocable() => Ok(target),
                        About::Target(_) | About::Records(_) => {
                            Err(JudgementError::NotRevocable { record: *id })
                        }
                    }
                };
                let first = subject(named.first())?;
                for id in named.iter() {
                    if subject(id)? != first {
                        return Err(JudgementError::RevokeSpansTargets);
                    }
                }
                place_of(corpus, ledgers, first)
            }
        }
    }
}

impl Judgement {
    /// The ledger this judgement's kind is written in; see [`JudgementKind::ledger`].
    pub fn ledger(&self, corpus: &Corpus, ledgers: &Ledgers) -> Result<Visibility, JudgementError> {
        self.kind().ledger(corpus, ledgers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::adoption::{self, Adoptable, AdoptionInput};
    use crate::domain::design::test_support::{design, identified};
    use crate::domain::finding::test_support::finding;
    use crate::domain::gate::Passed;
    use crate::domain::id::test_support::{docid, eid, iid, sid};
    use crate::domain::judge::test_support::judge;
    use crate::domain::judgement::test_support::*;
    use crate::domain::spec::test_support::spec;

    fn ada() -> Judge {
        judge("ada")
    }

    fn grace() -> Judge {
        judge("grace")
    }

    fn pair(public: Vec<Judgement>, private: Vec<Judgement>) -> Ledgers {
        Ledgers::new(
            Ledger::from_judgements(public),
            Ledger::from_judgements(private),
        )
    }

    fn public_only(records: Vec<Judgement>) -> Ledgers {
        pair(records, Vec::new())
    }

    /// An approval of Finding `finding` at the version `tag`, by `who`, in `origin`.
    fn cr(n: u128, who: &Judge, origin: Visibility, finding: u128, tag: &str) -> Judgement {
        approve(n, who, origin, cr_passed(finding, tag))
    }

    fn revoke_ids(
        ledgers: &Ledgers,
        n: u128,
        by: &Judge,
        ids: &[u128],
        force: bool,
    ) -> Result<Judgement, RevokeError> {
        ledgers.revoke(RevokeRequest {
            id: jid(n),
            at: ts(),
            by: by.clone(),
            origin: Visibility::Public,
            force,
            reason: why(),
            what: RevokeWhat::Records(
                NonEmptyVec::from_vec(ids.iter().map(|id| jid(*id)).collect()).unwrap(),
            ),
        })
    }

    fn revoke_attestation(
        ledgers: &Ledgers,
        n: u128,
        by: &Judge,
        of: Option<&Judge>,
        force: bool,
        (gate, subject, tag): (Gate, EntityId, &str),
    ) -> Result<Judgement, RevokeError> {
        ledgers.revoke(RevokeRequest {
            id: jid(n),
            at: ts(),
            by: by.clone(),
            origin: Visibility::Public,
            force,
            reason: why(),
            what: RevokeWhat::Attestation {
                gate,
                subject,
                hash: hash_of(tag),
                of: of.cloned(),
            },
        })
    }

    fn cr_subject(finding: u128, tag: &str) -> (Gate, EntityId, &str) {
        (Gate::Cr, eid(finding), tag)
    }

    fn approved(ledgers: &Ledgers, finding: u128, tag: &str) -> bool {
        ledgers.is_approved(Gate::Cr, eid(finding), &hash_of(tag))
    }

    fn ids_of<'a>(approvals: impl Iterator<Item = StandingApproval<'a>>) -> Vec<JudgementId> {
        approvals.map(|approval| approval.id()).collect()
    }

    // --- a set that merges by union --------------------------------------

    #[test]
    fn two_records_of_one_identifier_that_differ_are_both_excluded() {
        let one = cr(1, &ada(), Visibility::Public, 1, "v1");
        let other = cr(1, &ada(), Visibility::Public, 1, "v2");
        let ledger = Ledger::from_judgements([one.clone(), other.clone()]);
        assert!(ledger.is_corrupt(&jid(1)));
        assert!(!ledger.contains(&jid(1)));
        assert_eq!(ledger.corrupt_ids().collect::<Vec<_>>(), [jid(1)]);
        assert_eq!(ledger.judgements().count(), 0);
        // The order the records arrive in does not matter.
        assert_eq!(
            ledger,
            Ledger::from_judgements([other.clone(), one.clone()])
        );

        let ledgers = Ledgers::new(ledger, Ledger::new());
        assert!(!approved(&ledgers, 1, "v1"));
        assert!(!approved(&ledgers, 1, "v2"));
        assert_eq!(ledgers.corrupt(), &BTreeSet::from([jid(1)]));
    }

    #[test]
    fn corrupt_absorbs_in_a_union_and_in_a_repeat() {
        let one = cr(1, &ada(), Visibility::Public, 1, "v1");
        let other = cr(1, &ada(), Visibility::Public, 1, "v2");
        let a = Ledger::from_judgements([one.clone()]);
        let b = Ledger::from_judgements([other]);
        let both = a.union(&b);
        assert!(both.is_corrupt(&jid(1)));
        assert_eq!(both, b.union(&a));
        assert!(both.union(&a).is_corrupt(&jid(1)));
        let mut again = both.clone();
        again.insert(one);
        assert_eq!(again, both);
    }

    #[test]
    fn one_identifier_that_differs_between_the_two_ledgers_is_corrupt() {
        let public = cr(1, &ada(), Visibility::Public, 1, "v1");
        let private = cr(1, &ada(), Visibility::Public, 1, "v2");
        let ledgers = pair(vec![public], vec![private]);
        assert_eq!(ledgers.corrupt(), &BTreeSet::from([jid(1)]));
        assert_eq!(ledgers.judgements().count(), 0);
        assert!(!approved(&ledgers, 1, "v1"));
        assert!(!approved(&ledgers, 1, "v2"));
    }

    #[test]
    fn the_same_record_in_both_ledgers_reads_as_one() {
        let record = cr(1, &ada(), Visibility::Public, 1, "v1");
        let ledgers = pair(vec![record.clone()], vec![record]);
        assert!(ledgers.corrupt().is_empty());
        assert_eq!(ledgers.judgements().count(), 1);
        assert_eq!(ledgers.approvals().count(), 1);
        assert!(approved(&ledgers, 1, "v1"));
    }

    /// The records of the tests below, by name.
    struct Pool {
        approve: Judgement,
        conflicting: Judgement,
        other_judge: Judgement,
        revoke: Judgement,
        private: Judgement,
        publish: Judgement,
        purge: Judgement,
    }

    fn pool() -> Pool {
        let approve = cr(1, &ada(), Visibility::Public, 1, "v1");
        let conflicting = cr(1, &ada(), Visibility::Public, 1, "v2");
        let other_judge = cr(2, &grace(), Visibility::Public, 1, "v1");
        let revoke = revoke_ids(&public_only(vec![approve.clone()]), 5, &ada(), &[1], false)
            .expect("an approval can be revoked");
        let private = cr(7, &ada(), Visibility::Private, 2, "w1");
        let publish = Judgement::published(
            jid(8),
            ts(),
            ada(),
            docid(2),
            BTreeSet::from([jid(7)]),
            &public_only(vec![private.clone()]),
        )
        .expect("the record is in the public ledger");
        let purge = Judgement::purge(
            jid(9),
            ts(),
            ada(),
            Visibility::Public,
            docid(1),
            TextHash::of("gone"),
            why(),
        );
        Pool {
            approve,
            conflicting,
            other_judge,
            revoke,
            private,
            publish,
            purge,
        }
    }

    /// A small deterministic generator: the tests do not depend on a library for
    /// random values.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// Ledgers split in three at random, each record in any of the branches (or in
    /// none), and some in several.
    fn branches(records: &[Judgement], rng: &mut Lcg) -> [Ledger; 3] {
        let masks: Vec<u64> = records.iter().map(|_| rng.next() % 8).collect();
        let mut out = [Ledger::new(), Ledger::new(), Ledger::new()];
        for (slot, branch) in out.iter_mut().enumerate() {
            *branch = Ledger::from_judgements(
                records
                    .iter()
                    .zip(&masks)
                    .filter(|(_, mask)| *mask & (1 << slot) != 0)
                    .map(|(record, _)| record.clone()),
            );
        }
        out
    }

    fn all_records(pool: &Pool) -> Vec<Judgement> {
        vec![
            pool.approve.clone(),
            pool.conflicting.clone(),
            pool.other_judge.clone(),
            pool.revoke.clone(),
            pool.private.clone(),
            pool.publish.clone(),
            pool.purge.clone(),
        ]
    }

    #[test]
    fn union_is_commutative_associative_and_idempotent() {
        let records = all_records(&pool());
        let mut rng = Lcg(0x5eed);
        for _ in 0..3000 {
            let [a, b, c] = branches(&records, &mut rng);
            assert_eq!(a.union(&b), b.union(&a));
            assert_eq!(a.union(&b).union(&c), a.union(&b.union(&c)));
            assert_eq!(a.union(&a), a);
            let all = a.union(&b).union(&c);
            assert_eq!(all.union(&all), all);
            assert_eq!(all.union(&a), all);
        }
    }

    /// Everything the rules read from a pair of ledgers.
    fn reading(
        ledgers: &Ledgers,
    ) -> (
        Vec<JudgementId>,
        Vec<JudgementId>,
        Vec<JudgementId>,
        BTreeSet<DocId>,
        bool,
        bool,
    ) {
        (
            ledgers.judgements().map(Judgement::id).collect(),
            ids_of(ledgers.approvals()),
            ledgers.corrupt().iter().copied().collect(),
            ledgers.published_docs(),
            ledgers.is_purged(&TextHash::of("gone")),
            approved(ledgers, 1, "v1"),
        )
    }

    #[test]
    fn what_is_read_is_the_same_in_any_order_of_merging() {
        let records = all_records(&pool());
        let mut rng = Lcg(0xfeed);
        for _ in 0..1500 {
            let [a, b, c] = branches(&records, &mut rng);
            let orders = [
                a.union(&b).union(&c),
                a.union(&c).union(&b),
                b.union(&a).union(&c),
                b.union(&c).union(&a),
                c.union(&a).union(&b),
                c.union(&b).union(&a),
                a.union(&b.union(&c)),
            ];
            let expected = reading(&Ledgers::new(orders[0].clone(), Ledger::new()));
            for merged in &orders[1..] {
                assert_eq!(
                    reading(&Ledgers::new(merged.clone(), Ledger::new())),
                    expected
                );
            }
        }
    }

    #[test]
    fn merging_both_ledgers_is_the_union_of_each() {
        let records = all_records(&pool());
        let mut rng = Lcg(0xbead);
        for _ in 0..1000 {
            let [a, b, c] = branches(&records, &mut rng);
            let [d, e, f] = branches(&records, &mut rng);
            let x = Ledgers::new(a, d);
            let y = Ledgers::new(b, e);
            let z = Ledgers::new(c, f);
            let left = x.union(&y).union(&z);
            let right = z.union(&y).union(&x);
            assert_eq!(left.public(), right.public());
            assert_eq!(left.private(), right.private());
            assert_eq!(reading(&left), reading(&right));
            assert_eq!(reading(&x.union(&x)), reading(&x));
        }
    }

    // --- revocation: what it names, who may, and what survives ------------

    #[test]
    fn a_revocation_names_what_the_judge_can_see_and_nothing_else() {
        // Two records of one attestation by the same judge: both are named.
        let ledgers = public_only(vec![
            cr(1, &ada(), Visibility::Public, 1, "v1"),
            cr(2, &ada(), Visibility::Public, 1, "v1"),
            cr(3, &grace(), Visibility::Public, 1, "v1"),
            cr(4, &ada(), Visibility::Public, 1, "other-version"),
        ]);
        let revoke = revoke_attestation(&ledgers, 10, &ada(), None, false, cr_subject(1, "v1"))
            .expect("ada has records of it");
        let JudgementKind::Revoke { targets, .. } = revoke.kind() else {
            panic!("expected a revoke");
        };
        assert_eq!(
            targets.iter().copied().collect::<Vec<_>>(),
            [jid(1), jid(2)]
        );

        let after = public_only(vec![
            cr(1, &ada(), Visibility::Public, 1, "v1"),
            cr(2, &ada(), Visibility::Public, 1, "v1"),
            cr(3, &grace(), Visibility::Public, 1, "v1"),
            cr(4, &ada(), Visibility::Public, 1, "other-version"),
            revoke,
        ]);
        // Grace's record and the other version stand.
        assert_eq!(
            ids_of(after.approvals_at(Gate::Cr, eid(1), &hash_of("v1"))),
            [jid(3)]
        );
        assert!(approved(&after, 1, "other-version"));
        // Nothing of ada's is left to revoke.
        assert_eq!(
            revoke_attestation(&after, 11, &ada(), None, false, cr_subject(1, "v1")),
            Err(RevokeError::NothingToRevoke)
        );
    }

    #[test]
    fn a_concurrent_approval_survives_a_revocation_that_did_not_see_it() {
        let seen = cr(1, &ada(), Visibility::Public, 1, "v1");
        // Branch one: ada revokes her approval. Branch two, written without seeing
        // that: grace approves the same version.
        let one = public_only(vec![seen.clone()]);
        let revoke = revoke_attestation(&one, 5, &ada(), None, false, cr_subject(1, "v1"))
            .expect("visible on branch one");
        let concurrent = cr(2, &grace(), Visibility::Public, 1, "v1");

        let branch_one = Ledger::from_judgements([seen.clone(), revoke]);
        let branch_two = Ledger::from_judgements([seen, concurrent]);
        for merged in [branch_one.union(&branch_two), branch_two.union(&branch_one)] {
            let ledgers = Ledgers::new(merged, Ledger::new());
            assert!(ledgers.is_revoked(&jid(1)));
            assert!(!ledgers.is_revoked(&jid(2)));
            assert!(approved(&ledgers, 1, "v1"), "the unseen approval stands");
            assert_eq!(
                ids_of(ledgers.approvals_at(Gate::Cr, eid(1), &hash_of("v1"))),
                [jid(2)]
            );
        }
        // The same judge's concurrent approval of the same version stands too.
        let twice = cr(3, &ada(), Visibility::Public, 1, "v1");
        let merged = Ledger::from_judgements([twice]).union(&branch_one);
        assert!(approved(&Ledgers::new(merged, Ledger::new()), 1, "v1"));
    }

    #[test]
    fn only_approvals_adoptions_and_retirements_can_be_named() {
        let approve = cr(1, &ada(), Visibility::Public, 1, "v1");
        let purge = pool().purge;
        let ledgers_without = public_only(vec![approve.clone()]);
        let revoke = revoke_ids(&ledgers_without, 4, &ada(), &[1], false).unwrap();
        let publish = Judgement::published(
            jid(3),
            ts(),
            ada(),
            docid(1),
            BTreeSet::from([jid(1)]),
            &ledgers_without,
        )
        .unwrap();
        let ledgers = public_only(vec![approve, purge, publish, revoke]);
        // jid(9) is the purge of the pool.
        for (named, expected) in [
            (9, RevokeError::NotRevocable { record: jid(9) }),
            (3, RevokeError::NotRevocable { record: jid(3) }),
            (4, RevokeError::NotRevocable { record: jid(4) }),
            (1, RevokeError::AlreadyRevoked { record: jid(1) }),
            (77, RevokeError::UnknownRecord { record: jid(77) }),
        ] {
            assert_eq!(
                revoke_ids(&ledgers, 20, &ada(), &[named], true),
                Err(expected),
                "naming {named}"
            );
        }
    }

    #[test]
    fn a_revocation_written_by_hand_that_names_a_publication_names_nothing() {
        let held = cr(1, &ada(), Visibility::Private, 1, "v1");
        let public = public_only(vec![held.clone()]);
        let publish = Judgement::published(
            jid(3),
            ts(),
            ada(),
            docid(1),
            BTreeSet::from([jid(1)]),
            &public,
        )
        .unwrap();
        let forged = Judgement::assemble(
            jid(4),
            ts(),
            ada(),
            Visibility::Public,
            JudgementKind::Revoke {
                targets: NonEmptyVec::new(jid(3), vec![jid(9)]),
                reason: why(),
            },
        );
        let ledgers = public_only(vec![held, publish, forged]);
        assert!(!ledgers.is_revoked(&jid(3)));
        assert!(ledgers.published_docs().contains(&docid(1)));
        assert!(approved(&ledgers, 1, "v1"), "the commit point still stands");
    }

    #[test]
    fn a_revocation_names_only_what_counts_in_the_ledger_it_is_written_to() {
        let private = cr(1, &ada(), Visibility::Private, 1, "v1");
        let public = cr(2, &ada(), Visibility::Public, 2, "v1");
        let ledgers = pair(vec![public], vec![private]);
        let request = |origin, what| RevokeRequest {
            id: jid(5),
            at: ts(),
            by: ada(),
            origin,
            force: false,
            reason: why(),
            what,
        };
        let records = |n| RevokeWhat::Records(NonEmptyVec::new(jid(n), Vec::new()));
        // A public revocation of a record only the private ledger holds would take
        // it away for its owner alone.
        assert_eq!(
            ledgers.revoke(request(Visibility::Public, records(1))),
            Err(RevokeError::UnknownRecord { record: jid(1) })
        );
        assert_eq!(
            ledgers.revoke(request(
                Visibility::Public,
                RevokeWhat::Attestation {
                    gate: Gate::Cr,
                    subject: eid(1),
                    hash: hash_of("v1"),
                    of: None,
                }
            )),
            Err(RevokeError::NothingToRevoke)
        );
        // And the other way round.
        assert_eq!(
            ledgers.revoke(request(Visibility::Private, records(2))),
            Err(RevokeError::UnknownRecord { record: jid(2) })
        );
        assert!(
            ledgers
                .revoke(request(Visibility::Private, records(1)))
                .is_ok()
        );
        assert!(
            ledgers
                .revoke(request(Visibility::Public, records(2)))
                .is_ok()
        );
    }

    #[test]
    fn revoking_another_judges_record_needs_force() {
        let ledgers = public_only(vec![cr(1, &ada(), Visibility::Public, 1, "v1")]);
        let expected = RevokeError::RevokeOthers { owner: ada() };
        // By attestation, naming ada.
        assert_eq!(
            revoke_attestation(
                &ledgers,
                5,
                &grace(),
                Some(&ada()),
                false,
                cr_subject(1, "v1")
            ),
            Err(expected.clone())
        );
        // By record.
        assert_eq!(
            revoke_ids(&ledgers, 5, &grace(), &[1], false),
            Err(expected)
        );
        // With force, either way.
        assert!(
            revoke_attestation(
                &ledgers,
                5,
                &grace(),
                Some(&ada()),
                true,
                cr_subject(1, "v1")
            )
            .is_ok()
        );
        assert!(revoke_ids(&ledgers, 5, &grace(), &[1], true).is_ok());
        // Without naming anyone, grace names her own records, and has none.
        assert_eq!(
            revoke_attestation(&ledgers, 5, &grace(), None, false, cr_subject(1, "v1")),
            Err(RevokeError::NothingToRevoke)
        );
    }

    #[test]
    fn the_same_person_through_an_agent_is_not_another_judge() {
        let through_agent = crate::domain::judge::test_support::agent("ada", "some-agent");
        let ledgers = public_only(vec![approve(
            1,
            &through_agent,
            Visibility::Public,
            cr_passed(1, "v1"),
        )]);
        assert!(revoke_ids(&ledgers, 5, &ada(), &[1], false).is_ok());
    }

    // --- what is approved -------------------------------------------------

    #[test]
    fn approved_is_a_question_about_the_set_not_about_the_latest() {
        let ledgers = public_only(vec![
            cr(1, &ada(), Visibility::Public, 1, "v1"),
            cr(2, &grace(), Visibility::Public, 1, "v2"),
        ]);
        assert!(approved(&ledgers, 1, "v1"));
        assert!(approved(&ledgers, 1, "v2"));
        assert!(!approved(&ledgers, 1, "v3"));
        assert!(!ledgers.is_approved(Gate::Cs, eid(1), &hash_of("v1")));
        assert!(!approved(&ledgers, 2, "v1"));
    }

    #[test]
    fn a_measurement_binds_to_the_spec_and_the_hypothesis_hash() {
        let cs_by_ada = approve(
            1,
            &ada(),
            Visibility::Public,
            cs_passed(11, "contract", "hyp"),
        );
        let cs_by_grace = approve(
            2,
            &grace(),
            Visibility::Public,
            cs_passed(11, "contract", "hyp"),
        );
        let measurement = approve(
            3,
            &ada(),
            Visibility::Public,
            cm_passed(11, "hyp", "cs-event"),
        );
        let ledgers = public_only(vec![cs_by_ada, cs_by_grace, measurement.clone()]);
        let hyp = hash_of("hyp");
        assert_eq!(
            ledgers
                .approvals_at(Gate::Cs, eid(11), &hash_of("contract"))
                .count(),
            2,
            "two CS approvals of one version"
        );
        assert!(ledgers.is_measured(sid(11), &hyp));
        assert!(!ledgers.is_measured(sid(11), &hash_of("another hypothesis")));
        assert!(!ledgers.is_measured(sid(12), &hyp));

        // Revoking either, or both, of the CS approvals leaves the measurement.
        let mut records = vec![
            approve(
                1,
                &ada(),
                Visibility::Public,
                cs_passed(11, "contract", "hyp"),
            ),
            approve(
                2,
                &grace(),
                Visibility::Public,
                cs_passed(11, "contract", "hyp"),
            ),
            measurement,
        ];
        let revoke = revoke_ids(&public_only(records.clone()), 8, &ada(), &[1], false).unwrap();
        records.push(revoke);
        let after_one = public_only(records.clone());
        assert!(after_one.is_measured(sid(11), &hyp));
        let revoke = revoke_ids(&after_one, 9, &grace(), &[2], false).unwrap();
        records.push(revoke);
        let after_both = public_only(records);
        assert_eq!(
            after_both
                .approvals_at(Gate::Cs, eid(11), &hash_of("contract"))
                .count(),
            0
        );
        assert!(after_both.is_measured(sid(11), &hyp));
    }

    fn adoptable_finding(source: &str) -> Adoptable {
        let corpus = corpus_of(vec![Filed::public(crate::domain::corpus::Entity::Finding(
            finding(1, "Login takes 8 s."),
        ))]);
        adoption::check(AdoptionInput {
            corpus: &corpus,
            gate: Gate::Cr,
            target: Target::Doc(docid(1)),
            source,
        })
        .unwrap()
    }

    #[test]
    fn an_adoption_stands_as_an_approval_with_its_own_basis() {
        let adoptable = adoptable_finding("https://tracker.example/issues/7");
        let hash = *adoptable.attestation().hash();
        let adopted = Judgement::adopt(
            jid(1),
            ts(),
            ada(),
            Visibility::Public,
            adoptable,
            object_of(&hash),
        )
        .unwrap();
        let gated = approve(2, &grace(), Visibility::Public, {
            // The same version as the Finding of the adoption, through a gate.
            Passed::for_test(crate::domain::approval::Attestation::Cr {
                finding: crate::domain::approval::Pinned::new(
                    crate::domain::id::test_support::fid(1),
                    hash,
                ),
            })
        });
        let ledgers = public_only(vec![adopted, gated]);
        assert!(ledgers.is_approved(Gate::Cr, eid(1), &hash));
        let bases: Vec<Basis> = ledgers
            .approvals_at(Gate::Cr, eid(1), &hash)
            .map(|approval| approval.basis().clone())
            .collect();
        assert_eq!(
            bases,
            [
                Basis::Adopted {
                    source: "https://tracker.example/issues/7".parse().unwrap()
                },
                Basis::Gate
            ]
        );
        // One revocation by attestation takes the adoption of its judge, and only that.
        let revoke = ledgers
            .revoke(RevokeRequest {
                id: jid(5),
                at: ts(),
                by: ada(),
                origin: Visibility::Public,
                force: false,
                reason: why(),
                what: RevokeWhat::Attestation {
                    gate: Gate::Cr,
                    subject: eid(1),
                    hash,
                    of: None,
                },
            })
            .unwrap();
        let JudgementKind::Revoke { targets, .. } = revoke.kind() else {
            panic!("expected a revoke");
        };
        assert_eq!(targets.iter().copied().collect::<Vec<_>>(), [jid(1)]);
    }

    // --- retire -------------------------------------------------------------

    fn retire(n: u128, target: Target) -> Judgement {
        Judgement::retire(
            jid(n),
            ts(),
            ada(),
            Visibility::Public,
            target,
            FilePresence::Absent,
            why(),
        )
        .unwrap()
    }

    #[test]
    fn a_retirement_counts_only_while_the_file_is_gone_and_a_return_is_noticed_until_it_is_revoked()
    {
        let target = Target::Doc(docid(1));
        let other = Target::Doc(docid(2));
        let ledgers = public_only(vec![retire(1, target)]);
        assert_eq!(
            ledgers.retirement(&target, FilePresence::Absent),
            Retirement::Retired
        );
        assert_eq!(
            ledgers.retirement(&target, FilePresence::Present),
            Retirement::Returned,
            "the file is back: present, with the notice"
        );
        assert_eq!(
            ledgers.retirement(&other, FilePresence::Absent),
            Retirement::NotRetired
        );

        // Revoking the retirement ends the notice.
        let revoke = ledgers
            .revoke(RevokeRequest {
                id: jid(2),
                at: ts(),
                by: ada(),
                origin: Visibility::Public,
                force: false,
                reason: why(),
                what: RevokeWhat::Retire { target, of: None },
            })
            .unwrap();
        let revoked = public_only(vec![retire(1, target), revoke]);
        for file in [FilePresence::Present, FilePresence::Absent] {
            assert_eq!(revoked.retirement(&target, file), Retirement::NotRetired);
        }

        // The file goes again and a retirement is written again: it is alive.
        let again = public_only(vec![
            retire(1, target),
            revoked.get(&jid(2)).cloned().unwrap(),
            retire(3, target),
        ]);
        assert_eq!(
            again.retirement(&target, FilePresence::Absent),
            Retirement::Retired
        );
    }

    // --- two ledgers: what counts -----------------------------------------

    #[test]
    fn a_private_record_in_the_public_ledger_is_inert_until_a_publication_names_it() {
        let copied = cr(1, &ada(), Visibility::Private, 1, "v1");
        let ledgers = public_only(vec![copied.clone()]);
        assert!(ledgers.is_inert(&jid(1)));
        assert!(ledgers.get(&jid(1)).is_none());
        assert!(!approved(&ledgers, 1, "v1"));
        assert!(!ledgers.judged_in(&Target::Doc(docid(1)), Visibility::Public));

        let publish = Judgement::published(
            jid(2),
            ts(),
            ada(),
            docid(1),
            BTreeSet::from([jid(1)]),
            &ledgers,
        )
        .unwrap();
        let committed = public_only(vec![copied.clone(), publish]);
        assert!(!committed.is_inert(&jid(1)));
        assert!(approved(&committed, 1, "v1"));
        assert!(committed.judged_in(&Target::Doc(docid(1)), Visibility::Public));
        assert_eq!(committed.published_docs(), BTreeSet::from([docid(1)]));

        // A publication that did not name it leaves it inert.
        let elsewhere =
            Judgement::published(jid(3), ts(), ada(), docid(2), BTreeSet::new(), &ledgers).unwrap();
        assert!(public_only(vec![copied, elsewhere]).is_inert(&jid(1)));
    }

    #[test]
    fn a_publication_of_one_document_activates_nothing_about_another() {
        // Written by hand: `Judgement::published` would refuse this.
        let about_b = cr(1, &ada(), Visibility::Private, 1, "v1");
        let publish_a = Judgement::assemble(
            jid(2),
            ts(),
            ada(),
            Visibility::Public,
            JudgementKind::Published {
                doc: docid(2),
                records: BTreeSet::from([jid(1)]),
            },
        );
        let ledgers = public_only(vec![about_b, publish_a]);
        assert!(ledgers.is_inert(&jid(1)));
        assert!(!approved(&ledgers, 1, "v1"));
        assert!(!ledgers.judged_in(&Target::Doc(docid(1)), Visibility::Public));
    }

    #[test]
    fn a_public_revocation_naming_only_an_inert_record_judges_nothing() {
        let inert = cr(1, &ada(), Visibility::Private, 1, "v1");
        let forged = Judgement::assemble(
            jid(2),
            ts(),
            ada(),
            Visibility::Public,
            JudgementKind::Revoke {
                targets: NonEmptyVec::new(jid(1), Vec::new()),
                reason: why(),
            },
        );
        let ledgers = public_only(vec![inert, forged]);
        assert!(
            ledgers.get(&jid(2)).is_some(),
            "the revocation itself counts"
        );
        assert!(ledgers.is_inert(&jid(1)));
        assert!(!ledgers.is_revoked(&jid(1)));
        assert!(!ledgers.judged_in(&Target::Doc(docid(1)), Visibility::Public));
    }

    #[test]
    fn the_owner_reads_a_private_record_in_the_private_ledger_whether_or_not_it_was_copied() {
        let record = cr(1, &ada(), Visibility::Private, 1, "v1");
        let ledgers = pair(vec![record.clone()], vec![record]);
        assert!(approved(&ledgers, 1, "v1"));
        assert!(
            !ledgers.is_inert(&jid(1)),
            "it counts through the private ledger"
        );
        assert!(ledgers.judged_in(&Target::Doc(docid(1)), Visibility::Private));
        assert!(
            !ledgers.judged_in(&Target::Doc(docid(1)), Visibility::Public),
            "and the public side still sees nothing"
        );
    }

    #[test]
    fn a_revocation_of_an_inert_record_does_not_count_either() {
        let copied = cr(1, &ada(), Visibility::Private, 1, "v1");
        let revoke = pair(vec![], vec![copied.clone()])
            .revoke(RevokeRequest {
                id: jid(2),
                at: ts(),
                by: ada(),
                origin: Visibility::Private,
                force: false,
                reason: why(),
                what: RevokeWhat::Records(NonEmptyVec::new(jid(1), Vec::new())),
            })
            .unwrap();
        // The copied record is in the public ledger, and the private revocation in the
        // private one: the owner reads it revoked, the public side reads neither.
        let owner = pair(vec![copied.clone()], vec![copied.clone(), revoke.clone()]);
        assert!(!approved(&owner, 1, "v1"));
        let public = public_only(vec![copied]);
        assert!(!approved(&public, 1, "v1"));
        assert!(public.get(&revoke.id()).is_none());
    }

    #[test]
    fn a_purged_text_reads_as_gone_even_when_its_file_is_there() {
        let text = TextHash::of("gone");
        let kept = TextHash::of("kept");
        let ledgers = public_only(vec![pool().purge]);
        assert_eq!(ledgers.object_reading(&text, true), ObjectReading::Purged);
        assert_eq!(ledgers.object_reading(&text, false), ObjectReading::Purged);
        assert_eq!(ledgers.object_reading(&kept, true), ObjectReading::Present);
        assert_eq!(ledgers.object_reading(&kept, false), ObjectReading::Missing);

        // A branch that diverged before the purge and put the object back: the merge
        // reads it gone, whichever side of the merge is asked.
        let diverged = Ledger::new();
        let merged = Ledger::from_judgements([pool().purge]).union(&diverged);
        assert!(Ledgers::new(merged, Ledger::new()).is_purged(&text));
        // An inert purge (private origin, copied, not named) does not delete.
        let inert = Judgement::purge(
            jid(2),
            ts(),
            ada(),
            Visibility::Private,
            docid(1),
            kept,
            why(),
        );
        assert!(!public_only(vec![inert.clone()]).is_purged(&kept));
        assert!(pair(vec![], vec![inert]).is_purged(&kept));
    }

    // --- which ledger a judgement is written in -------------------------------

    fn spec_entity(n: u128) -> crate::domain::corpus::Entity {
        crate::domain::corpus::Entity::Spec(spec(n, &[1], &["PIN-A"]))
    }

    fn design_entity() -> crate::domain::corpus::Entity {
        crate::domain::corpus::Entity::Design(design(
            21,
            11,
            vec![(1, identified(31, "cache", &["PIN-A"]))],
        ))
    }

    fn approve_kind(passed: Passed) -> JudgementKind {
        let object = object_of(passed.attestation().target_hash());
        JudgementKind::Approve { passed, object }
    }

    #[test]
    fn a_document_goes_where_the_document_is() {
        let none = Ledgers::default();
        let kind = approve_kind(cs_passed(11, "c", "h"));
        let public = corpus_of(vec![Filed::public(spec_entity(11))]);
        let private = corpus_of(vec![Filed::private(spec_entity(11))]);
        assert_eq!(kind.ledger(&public, &none), Ok(Visibility::Public));
        assert_eq!(kind.ledger(&private, &none), Ok(Visibility::Private));
        // The record asks the same.
        let record = approve(1, &ada(), Visibility::Private, cs_passed(11, "c", "h"));
        assert_eq!(record.ledger(&private, &none), Ok(Visibility::Private));
    }

    #[test]
    fn an_item_goes_where_its_design_is() {
        let none = Ledgers::default();
        let kind = approve_kind(ca_passed(31, "item"));
        let public = corpus_of(vec![Filed::public(design_entity())]);
        let private = corpus_of(vec![Filed::private(design_entity())]);
        assert_eq!(kind.ledger(&public, &none), Ok(Visibility::Public));
        assert_eq!(kind.ledger(&private, &none), Ok(Visibility::Private));
        let retire = JudgementKind::Retire {
            target: Target::Item(iid(31)),
            reason: why(),
        };
        assert_eq!(retire.ledger(&private, &none), Ok(Visibility::Private));
    }

    #[test]
    fn a_purge_goes_where_its_document_is() {
        let none = Ledgers::default();
        let kind = JudgementKind::Purge {
            doc: docid(11),
            object: TextHash::of("t"),
            reason: why(),
        };
        let public = corpus_of(vec![Filed::public(spec_entity(11))]);
        let private = corpus_of(vec![Filed::private(spec_entity(11))]);
        assert_eq!(kind.ledger(&public, &none), Ok(Visibility::Public));
        assert_eq!(kind.ledger(&private, &none), Ok(Visibility::Private));
    }

    #[test]
    fn a_publication_goes_to_the_public_ledger_whatever_the_files_say() {
        let kind = JudgementKind::Published {
            doc: docid(11),
            records: BTreeSet::new(),
        };
        let private = corpus_of(vec![Filed::private(spec_entity(11))]);
        assert_eq!(
            kind.ledger(&private, &Ledgers::default()),
            Ok(Visibility::Public)
        );
        assert_eq!(
            kind.ledger(&corpus_of(vec![]), &Ledgers::default()),
            Ok(Visibility::Public)
        );
    }

    #[test]
    fn something_whose_file_is_gone_goes_where_its_judgements_already_are() {
        let empty = corpus_of(vec![]);
        let target = Target::Doc(docid(1));
        let retire = JudgementKind::Retire {
            target,
            reason: why(),
        };
        let public = cr(1, &ada(), Visibility::Public, 1, "v1");
        let private = cr(2, &ada(), Visibility::Private, 1, "v1");

        assert_eq!(
            retire.ledger(&empty, &public_only(vec![public.clone()])),
            Ok(Visibility::Public)
        );
        assert_eq!(
            retire.ledger(&empty, &pair(vec![], vec![private.clone()])),
            Ok(Visibility::Private)
        );
        assert_eq!(
            retire.ledger(&empty, &pair(vec![public.clone()], vec![private.clone()])),
            Ok(Visibility::Public),
            "both: the public ledger"
        );
        assert_eq!(
            retire.ledger(&empty, &Ledgers::default()),
            Err(JudgementError::UnknownTarget { target })
        );
        // A record that is inert says nothing to the public side.
        assert_eq!(
            retire.ledger(&empty, &public_only(vec![private])),
            Err(JudgementError::UnknownTarget { target })
        );
    }

    #[test]
    fn a_purge_of_a_vanished_document_goes_where_the_document_was_judged() {
        let kind = JudgementKind::Purge {
            doc: docid(1),
            object: TextHash::of("t"),
            reason: why(),
        };
        let private = cr(2, &ada(), Visibility::Private, 1, "v1");
        assert_eq!(
            kind.ledger(&corpus_of(vec![]), &pair(vec![], vec![private])),
            Ok(Visibility::Private)
        );
    }

    #[test]
    fn a_revocation_goes_where_what_it_names_is_about() {
        let record = cr(1, &ada(), Visibility::Public, 1, "v1");
        let ledgers = public_only(vec![record]);
        let kind = JudgementKind::Revoke {
            targets: NonEmptyVec::new(jid(1), Vec::new()),
            reason: why(),
        };
        let finding_doc = |filed: fn(crate::domain::corpus::Entity) -> Filed| {
            corpus_of(vec![filed(crate::domain::corpus::Entity::Finding(
                finding(1, "b"),
            ))])
        };
        assert_eq!(
            kind.ledger(&finding_doc(Filed::public), &ledgers),
            Ok(Visibility::Public)
        );
        assert_eq!(
            kind.ledger(&finding_doc(Filed::private), &ledgers),
            Ok(Visibility::Private),
            "before the commit point the document is private, and so is the revocation"
        );
        // And with the file gone, where the record is.
        assert_eq!(
            kind.ledger(&corpus_of(vec![]), &ledgers),
            Ok(Visibility::Public)
        );
    }

    #[test]
    fn a_revocation_of_what_is_unknown_or_cannot_be_named_has_no_ledger() {
        let corpus = corpus_of(vec![]);
        let revoke_of = |ids: Vec<JudgementId>| JudgementKind::Revoke {
            targets: NonEmptyVec::from_vec(ids).unwrap(),
            reason: why(),
        };
        let pool = pool();
        let ledgers = public_only(vec![
            pool.approve.clone(),
            pool.purge.clone(),
            cr(3, &ada(), Visibility::Public, 5, "x"),
        ]);
        assert_eq!(
            revoke_of(vec![jid(99)]).ledger(&corpus, &ledgers),
            Err(JudgementError::UnknownRecord { record: jid(99) })
        );
        assert_eq!(
            revoke_of(vec![jid(9)]).ledger(&corpus, &ledgers),
            Err(JudgementError::NotRevocable { record: jid(9) })
        );
        assert_eq!(
            revoke_of(vec![jid(1), jid(3)]).ledger(&corpus, &ledgers),
            Err(JudgementError::RevokeSpansTargets)
        );
    }

    #[test]
    fn a_document_in_both_roots_with_no_publication_between_them_has_no_one_ledger() {
        // Two files of one id that differ: a duplicate, not a publication cut short.
        let public = Filed::public(crate::domain::corpus::Entity::Finding(finding(1, "one")));
        let private = Filed::private(crate::domain::corpus::Entity::Finding(finding(1, "two")));
        let corpus = corpus_of(vec![public, private]);
        let kind = approve_kind(cr_passed(1, "v1"));
        assert_eq!(
            kind.ledger(&corpus, &Ledgers::default()),
            Err(JudgementError::AmbiguousTarget {
                target: Target::Doc(docid(1))
            })
        );
    }

    #[test]
    fn a_publication_cut_short_is_one_document_in_the_public_root() {
        let entity = crate::domain::corpus::Entity::Finding(finding(1, "one"));
        let files = vec![Filed::public(entity.clone()), Filed::private(entity)];
        let corpus = corpus_with(files, &BTreeSet::from([docid(1)]));
        let kind = approve_kind(cr_passed(1, "v1"));
        assert_eq!(
            kind.ledger(&corpus, &Ledgers::default()),
            Ok(Visibility::Public)
        );
    }

    #[test]
    fn the_documents_published_are_what_the_corpus_is_built_with() {
        let copied = cr(1, &ada(), Visibility::Private, 1, "v1");
        let ledgers = public_only(vec![copied.clone()]);
        let publish = Judgement::published(
            jid(2),
            ts(),
            ada(),
            docid(1),
            BTreeSet::from([jid(1)]),
            &ledgers,
        )
        .unwrap();
        let after = public_only(vec![copied, publish]);
        assert_eq!(after.published_docs(), BTreeSet::from([docid(1)]));
        assert!(ledgers.published_docs().is_empty());
    }
}
