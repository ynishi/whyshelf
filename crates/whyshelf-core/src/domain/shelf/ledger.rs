//! The judgements people made, as records, and what the records say.
//!
//! A [`Record`] is a [`Decision`] about one document (approve, deny, deprecate,
//! retire), a [`SameMeaning`] (a declaration that an edit of a document or a file
//! keeps the meaning; see its page), or a [`Revocation`] of decisions and
//! declarations. The domain holds a record as a value
//! with no wire format: [`Record::to_canon`] and [`Record::from_canon`] are the two
//! ends of its canonical form, and an adapter turns that form into JSON and back.
//! [`Record::from_canon`] is strict (a version it does not know, a key too many or
//! too few, a verb it does not know, an identifier that is not a UUID v7, an
//! invariant that does not hold are all refused), so a record read from a file is as
//! sound as one made here.
//!
//! # From records to a status
//!
//! [`Ledger::build`] takes every record and sets aside what contradicts itself
//! (two records of one id that differ, a revocation of something that is not there,
//! of a revocation, or of a conflict) as [`LedgerProblem`]s. [`Ledger::status_of`]
//! then reads the *declared* status of a document from the latest decision about it
//! that is not revoked. The declared status says what people decided; whether the
//! decision still holds is [`freshness`](super::freshness)'s question.
//!
//! # Making a record
//!
//! A decision is made from a *frame*, the facts it stands on, which the shelf
//! supplies: [`frame_approve`], [`frame_present`] and [`frame_retire`] return an
//! [`ApproveFrame`], a [`PresentFrame`] or a [`RetireFrame`] (or a
//! [`FrameRefusal`]), and [`Decision::approve`] and its three siblings take no
//! other. So an approval cannot carry a `seen` hash nobody read, a retirement cannot
//! pretend to know the hash of a document that is gone, and a decision about a
//! document that is not valid is refused before any record exists. Every
//! constructor, and the reader of the canonical form, goes through one private
//! function, `Decision::assemble`, which holds the invariants of a decision.
//!
//! # The clock
//!
//! [`JudgedAt`] is unix milliseconds and has an order, which "the latest decision"
//! and an expiry need. It is a number of milliseconds, not a text. The domain has no
//! clock of its own: the caller passes in each time.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::canon::Canon;
use crate::domain::error::ValueError;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::id::{DocId, EntityId};
use crate::domain::location::RelPath;
use crate::domain::nonempty::{Count, NonBlank, NonEmptySet, NonEmptyVec, TwoOrMore};

use super::artifact::{
    ArtifactHash, ArtifactState, Comparison, ComparisonProblem, ContentAddress, OpaqueProblem,
    Revision,
};
use super::build::{DocFailure, DocState, FailureTag, Shelf};
use super::doc::{DocHash, DocKey, LinkState, UnresolvedLink};
use super::graph::{Cycle, CycleSource, components, cycles_of};
use super::schema::{
    ArtifactPropagation, BookName, EntryName, KeyForm, KeyPrefix, KeyProblem, KeyTail, NameProblem,
    Propagation, Slug, parse_tail,
};

/// The identity of a record: a UUID v7 the caller supplies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudgementId(EntityId);

impl JudgementId {
    /// An identity from an identifier.
    pub fn new(id: EntityId) -> Self {
        Self(id)
    }

    /// The identifier.
    pub fn entity(&self) -> EntityId {
        self.0
    }
}

impl fmt::Display for JudgementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// When a judgement was made, in unix milliseconds. The caller supplies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudgedAt(i64);

impl JudgedAt {
    /// A time from unix milliseconds.
    pub fn new(millis: i64) -> Self {
        Self(millis)
    }

    /// The unix milliseconds.
    pub fn millis(&self) -> i64 {
        self.0
    }
}

/// What an approval stood on for one link: the upstream it named, the key it had
/// then and the hash it had then.
///
/// The key is kept so that an upstream that has since gone can be told from a link
/// that is written wrong now: both are a miss in the shelf, and only the key says
/// whether they are the same one. It is not part of any hash.
///
/// Its constructor is private to this module: a pin is taken from a resolved link by
/// [`frame_approve`], or read back from a record by [`Record::from_canon`], and by
/// nothing else.
///
/// It may carry the [`ContentAddress`] of the upstream's file as it was (rule A5),
/// so that the content can be read back later; freshness never reads it (A4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FramePin {
    link: EntryName,
    target: DocId,
    key: DocKey,
    hash: DocHash,
    address: Option<ContentAddress>,
}

impl FramePin {
    fn new(
        link: EntryName,
        target: DocId,
        key: DocKey,
        hash: DocHash,
        address: Option<ContentAddress>,
    ) -> Self {
        Self {
            link,
            target,
            key,
            hash,
            address,
        }
    }

    /// Where the upstream's content, as it was pinned, can be read back; none when
    /// the adapter could not say, or the record is of a version that kept none.
    pub fn address(&self) -> Option<&ContentAddress> {
        self.address.as_ref()
    }

    /// The link of the approved document the pin was taken for.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// The upstream.
    pub fn target(&self) -> DocId {
        self.target
    }

    /// The key the upstream had when it was pinned.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The hash the upstream had when it was pinned.
    pub fn hash(&self) -> &DocHash {
        &self.hash
    }
}

/// What an approval saw at the path of an artifact it stands on: a file with this
/// hash (and where its content can be read back, when that could be said), or no
/// file.
///
/// An absent file has no address by the type: there is nothing to read back.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PinnedArtifact {
    /// A file, with the hash of its content.
    Present {
        /// The hash.
        hash: ArtifactHash,
        /// Where the content can be read back, when it could be said.
        address: Option<ContentAddress>,
    },
    /// No file was there (rule A2: an approval pins an absence too).
    Absent,
}

impl PinnedArtifact {
    /// What was there, without the address: what freshness compares (rule A4).
    pub fn state(&self) -> ArtifactState {
        match self {
            Self::Present { hash, .. } => ArtifactState::Present(*hash),
            Self::Absent => ArtifactState::Absent,
        }
    }

    /// Where the content can be read back.
    pub fn address(&self) -> Option<&ContentAddress> {
        match self {
            Self::Present { address, .. } => address.as_ref(),
            Self::Absent => None,
        }
    }
}

/// What an approval stood on for one path of a `Depends` link to artifacts: the
/// link, the path, and what was there (rule A2).
///
/// Its constructor is private to this module, like a [`FramePin`]'s: it is taken from
/// an [`Artifact`](super::artifact::Artifact) of the document by [`frame_approve`],
/// or read back from a record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactPin {
    link: EntryName,
    path: RelPath,
    seen: PinnedArtifact,
    /// How the hash in `seen` was made. A record written before comparisons existed
    /// has none, and is [`Comparison::Bytes`], as it was made.
    comparison: Comparison,
}

impl ArtifactPin {
    /// How the hash of what was there was made.
    pub fn comparison(&self) -> &Comparison {
        &self.comparison
    }

    /// The link of the approved document the pin was taken for.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// The path of the artifact.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// What was there when it was pinned.
    pub fn seen(&self) -> &PinnedArtifact {
        &self.seen
    }
}

/// What a retirement saw of the document it retires.
///
/// A document can be retired when its file is gone, or broken, or cannot be told
/// about; then there is no hash to see, and the record says which of these it was
/// (the [`FailureTag`] of the [`DocFailure`], without
/// what it carries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// The document was valid, with this hash.
    At(DocHash),
    /// The document was not valid, in this way.
    Not(FailureTag),
}

/// What a decision does.
///
/// Each verb holds what only it can have: an approval stands on a frame and may
/// expire, a retirement may have seen a document that is no longer valid, the others
/// saw one hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionVerb {
    /// The document is sound as it was seen.
    Approve {
        /// The hash that was read and approved.
        seen: DocHash,
        /// The upstreams the approval stands on.
        frame: BTreeSet<FramePin>,
        /// The artifacts the approval stands on (rule A2).
        artifacts: BTreeSet<ArtifactPin>,
        /// When the approval lapses, if it does.
        expires: Option<JudgedAt>,
        /// Where the content of the approved document's file, as it was approved,
        /// can be read back (rule A5).
        seen_address: Option<ContentAddress>,
        /// The point of the project's history the approval was made at, as a hint
        /// (rule A5).
        revision: Option<Revision>,
    },
    /// The document is refused as it was seen.
    Deny {
        /// The hash that was read.
        seen: DocHash,
    },
    /// The document is to be replaced.
    Deprecate {
        /// The hash that was read.
        seen: DocHash,
    },
    /// The document is no longer kept.
    Retire {
        /// What was seen of it.
        seen: Seen,
    },
}

/// Why a decision cannot be made.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecisionProblem {
    /// An approval that lapses at or before the moment it is made.
    #[error("an approval that expires must expire after it is made")]
    ExpiresNotAfter {
        /// When it is made.
        at: JudgedAt,
        /// When it would lapse.
        expires: JudgedAt,
    },
    /// An approval that stands on the document it approves.
    #[error("an approval's frame does not name the approved document")]
    FrameNamesSubject,
    /// An approval whose frame pins one link to one document twice (with two
    /// hashes): it cannot say which it saw.
    #[error("an approval's frame pins `{link}` to {target} twice")]
    FramePinsTwice {
        /// The link.
        link: EntryName,
        /// The document.
        target: DocId,
    },
    /// An approval whose frame pins one path of one link to artifacts twice (with
    /// two states): it cannot say which it saw.
    #[error("an approval's frame pins `{link}` to `{path}` twice")]
    ArtifactPinsTwice {
        /// The link.
        link: EntryName,
        /// The path.
        path: RelPath,
    },
}

/// The facts an approval stands on, taken by [`frame_approve`].
///
/// It knows the file of the approved document and of each pinned upstream, so that
/// [`ApproveFrame::addressed`] can put the [`ContentAddress`] of each on the right
/// item; the files are not kept in the record (a file that moves keeps its
/// approval).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproveFrame {
    subject: DocId,
    seen: DocHash,
    frame: BTreeSet<FramePin>,
    artifacts: BTreeSet<ArtifactPin>,
    seen_address: Option<ContentAddress>,
    revision: Option<Revision>,
    subject_file: RelPath,
    upstream_files: BTreeMap<DocId, RelPath>,
}

impl ApproveFrame {
    /// The files whose [`ContentAddress`] the record can keep: the approved
    /// document's, each pinned upstream's, and each pinned artifact that is there.
    pub fn files(&self) -> BTreeSet<RelPath> {
        let mut files = BTreeSet::from([self.subject_file.clone()]);
        files.extend(self.upstream_files.values().cloned());
        files.extend(
            self.artifacts
                .iter()
                .filter(|pin| matches!(pin.seen, PinnedArtifact::Present { .. }))
                .map(|pin| pin.path.clone()),
        );
        files
    }

    /// The frame with the addresses of its files, by path, and the revision it is
    /// taken at (rule A5): the one place an address is put on an item of an
    /// approval. A file with no address in `addresses` keeps none; an artifact that
    /// is absent never has one. Nothing a freshness reads changes (A4).
    pub fn addressed(
        mut self,
        addresses: &BTreeMap<RelPath, ContentAddress>,
        revision: Option<Revision>,
    ) -> Self {
        self.seen_address = addresses.get(&self.subject_file).cloned();
        self.frame = std::mem::take(&mut self.frame)
            .into_iter()
            .map(|mut pin| {
                pin.address = self
                    .upstream_files
                    .get(&pin.target)
                    .and_then(|file| addresses.get(file))
                    .cloned();
                pin
            })
            .collect();
        self.artifacts = std::mem::take(&mut self.artifacts)
            .into_iter()
            .map(|mut pin| {
                if let PinnedArtifact::Present { address, .. } = &mut pin.seen {
                    *address = addresses.get(&pin.path).cloned();
                }
                pin
            })
            .collect();
        self.revision = revision;
        self
    }

    /// The pins of artifacts the approval would stand on.
    pub fn artifacts(&self) -> &BTreeSet<ArtifactPin> {
        &self.artifacts
    }
}

/// The facts a denial or a deprecation stands on, taken by [`frame_present`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentFrame {
    subject: DocId,
    seen: DocHash,
}

/// The facts a retirement stands on, taken by [`frame_retire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetireFrame {
    subject: DocId,
    seen: Seen,
}

/// Why a frame cannot be taken. It owns what it says, so it is a `'static` error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameRefusal {
    /// The document is not valid, so there is nothing sound to judge.
    #[error("the document is not valid: {0:?}")]
    NotValid(DocFailure),
    /// A `Depends` link of the document cannot be followed, so an approval would
    /// stand on nothing.
    #[error("the link `{link}` cannot be followed: {unresolved:?}")]
    UnresolvedDepends {
        /// The link.
        link: EntryName,
        /// What was written and why it fails.
        unresolved: UnresolvedLink,
    },
    /// The document is in a cycle: of `Depends` links, or of the frames of
    /// approvals, which this approval would close. Which one the cycle's id says.
    #[error("the document is in a cycle: {0:?}")]
    InCycle(Cycle),
    /// A `Depends` link names a document people decided does not stand. An approval
    /// pinned to it would not hold from the start; a deprecated one is to be
    /// replaced, so pin its successor.
    #[error("the link `{link}` names {target}, which is {status}: an approval cannot stand on it")]
    UpstreamNotStanding {
        /// The link.
        link: EntryName,
        /// The document it names.
        target: DocId,
        /// What was decided about it.
        status: NotStanding,
    },
}

/// A decision that leaves a document not standing, so nothing can be approved on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NotStanding {
    /// Refused.
    #[error("denied")]
    Denied,
    /// To be replaced.
    #[error("deprecated")]
    Deprecated,
    /// No longer kept.
    #[error("retired")]
    Retired,
}

/// A decision about a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    id: JudgementId,
    subject: DocId,
    verb: DecisionVerb,
    actor: Actor,
    at: JudgedAt,
    reason: Option<NonBlank>,
}

impl Decision {
    /// An approval.
    ///
    /// `expires` must be after `at`.
    pub fn approve(
        id: JudgementId,
        frame: ApproveFrame,
        expires: Option<JudgedAt>,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Approve {
            seen: frame.seen,
            frame: frame.frame,
            artifacts: frame.artifacts,
            expires,
            seen_address: frame.seen_address,
            revision: frame.revision,
        };
        Self::assemble(id, frame.subject, verb, actor, at, reason)
    }

    /// A denial.
    pub fn deny(
        id: JudgementId,
        frame: PresentFrame,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Deny { seen: frame.seen };
        Self::assemble(id, frame.subject, verb, actor, at, reason)
    }

    /// A deprecation.
    pub fn deprecate(
        id: JudgementId,
        frame: PresentFrame,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Deprecate { seen: frame.seen };
        Self::assemble(id, frame.subject, verb, actor, at, reason)
    }

    /// A retirement.
    pub fn retire(
        id: JudgementId,
        frame: RetireFrame,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        let verb = DecisionVerb::Retire { seen: frame.seen };
        Self::assemble(id, frame.subject, verb, actor, at, reason)
    }

    /// The one place the invariants of a decision are checked: every constructor and
    /// the reader of the canonical form go through it.
    fn assemble(
        id: JudgementId,
        subject: DocId,
        verb: DecisionVerb,
        actor: Actor,
        at: JudgedAt,
        reason: Option<NonBlank>,
    ) -> Result<Self, DecisionProblem> {
        if let DecisionVerb::Approve {
            frame,
            artifacts,
            expires,
            ..
        } = &verb
        {
            let mut paths = BTreeSet::new();
            for pin in artifacts {
                if !paths.insert((pin.link(), pin.path())) {
                    return Err(DecisionProblem::ArtifactPinsTwice {
                        link: pin.link().clone(),
                        path: pin.path().clone(),
                    });
                }
            }
            if let Some(expires) = *expires
                && expires <= at
            {
                return Err(DecisionProblem::ExpiresNotAfter { at, expires });
            }
            if frame.iter().any(|pin| pin.target() == subject) {
                return Err(DecisionProblem::FrameNamesSubject);
            }
            let mut seen = BTreeSet::new();
            for pin in frame {
                if !seen.insert((pin.link(), pin.target())) {
                    return Err(DecisionProblem::FramePinsTwice {
                        link: pin.link().clone(),
                        target: pin.target(),
                    });
                }
            }
        }
        Ok(Self {
            id,
            subject,
            verb,
            actor,
            at,
            reason,
        })
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The document it is about.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// What it does, with what only that verb has.
    pub fn verb(&self) -> &DecisionVerb {
        &self.verb
    }

    /// Who decided.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// When.
    pub fn at(&self) -> JudgedAt {
        self.at
    }
}

/// The withdrawal of earlier decisions and declarations of the same meaning,
/// whoever made them and whatever they are about. It has no subject and no frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    id: JudgementId,
    targets: NonEmptySet<JudgementId>,
    actor: Actor,
    at: JudgedAt,
    reason: NonBlank,
}

impl Revocation {
    /// A revocation of one or more decisions or declarations.
    pub fn new(
        id: JudgementId,
        targets: NonEmptySet<JudgementId>,
        actor: Actor,
        at: JudgedAt,
        reason: NonBlank,
    ) -> Self {
        Self {
            id,
            targets,
            actor,
            at,
            reason,
        }
    }
}

/// One record of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// A decision about a document.
    Decision(Decision),
    /// A withdrawal of decisions or declarations.
    Revocation(Revocation),
    /// A declaration that an edit keeps the meaning.
    SameMeaning(SameMeaning),
}

/// The hash of a record's canonical form, which tells two records of one id apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordHash(ContentHash);

/// The version of the canonical form of a record.
const VERSION: i64 = 1;

/// Why a canonical form is not a record.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecordProblem {
    /// The value is not a map.
    #[error("a record is a map")]
    NotAMap,
    /// A key every record of its kind has is missing.
    #[error("the record has no `{key}`")]
    MissingKey {
        /// The key.
        key: String,
    },
    /// A key no record of its kind has.
    #[error("the record has `{key}`, which no record of its kind has")]
    ExtraKey {
        /// The key.
        key: String,
    },
    /// A value of the wrong shape.
    #[error("`{key}` is not {expected}")]
    WrongType {
        /// The key.
        key: String,
        /// What it should be.
        expected: &'static str,
    },
    /// A version this reader does not know.
    #[error("version {found} of a record is not known")]
    UnknownVersion {
        /// The version found.
        found: i64,
    },
    /// A `kind` that is neither `decision` nor `revocation`.
    #[error("`{found}` is not a kind of record")]
    UnknownKind {
        /// What was found.
        found: String,
    },
    /// A `kind` of a key in a frame that this reader does not know: of the key, or of
    /// its tail.
    #[error("`{found}` is not a kind of key")]
    UnknownKeyKind {
        /// What was found.
        found: String,
    },
    /// A `verb` this reader does not know.
    #[error("`{found}` is not a verb of a decision")]
    UnknownVerb {
        /// What was found.
        found: String,
    },
    /// A retirement's `seen` that is not one of the four.
    #[error("`{found}` is not something a retirement saw")]
    UnknownSeen {
        /// What was found.
        found: String,
    },
    /// A text that is not a valid identifier, hash, actor or reason.
    #[error("`{key}` is not valid")]
    BadValue {
        /// The key.
        key: String,
        /// Why.
        #[source]
        source: ValueError,
    },
    /// A content address or a revision that is no opaque value.
    #[error("`{key}` is not an opaque value")]
    BadOpaque {
        /// The key.
        key: String,
        /// Why.
        #[source]
        source: OpaqueProblem,
    },
    /// The `state` of a pinned artifact that is neither `present` nor `absent`.
    #[error("`{found}` is not something an approval saw of an artifact")]
    UnknownArtifactState {
        /// What was found.
        found: String,
    },
    /// A key of a document in a frame that breaks the key grammar.
    #[error("a key in the record is not valid")]
    BadKey(#[source] KeyProblem),
    /// A name in a frame that breaks the name grammar.
    #[error("a name in the record is not valid")]
    BadName(#[source] NameProblem),
    /// A revocation that names no decision.
    #[error("a revocation names no decision")]
    EmptyTargets,
    /// The record is well formed but breaks an invariant of a decision.
    #[error("the record breaks an invariant of a decision")]
    Invariant(#[source] DecisionProblem),
    /// A declaration that an edit keeps the meaning names no earlier hash.
    #[error("a declaration of the same meaning names no earlier hash")]
    EmptyFrom,
    /// The record is well formed but breaks an invariant of a declaration.
    #[error("the record breaks an invariant of a declaration of the same meaning")]
    SameMeaning(#[source] SameMeaningProblem),
    /// The comparison of an artifact pin is not one.
    #[error("the comparison of a pinned artifact is not one")]
    BadComparison(#[source] ComparisonProblem),
    /// The `kind` of what a declaration is about is neither `doc` nor `artifact`.
    #[error("`{found}` is not something a declaration of the same meaning is about")]
    UnknownSubjectKind {
        /// What was found.
        found: String,
    },
}

type CanonMap = BTreeMap<String, Canon>;

fn missing(key: &str) -> RecordProblem {
    RecordProblem::MissingKey {
        key: key.to_owned(),
    }
}

fn wrong(key: &str, expected: &'static str) -> RecordProblem {
    RecordProblem::WrongType {
        key: key.to_owned(),
        expected,
    }
}

fn bad(key: &str, source: ValueError) -> RecordProblem {
    RecordProblem::BadValue {
        key: key.to_owned(),
        source,
    }
}

fn entry<'a>(map: &'a CanonMap, key: &str) -> Result<&'a Canon, RecordProblem> {
    map.get(key).ok_or_else(|| missing(key))
}

fn text<'a>(map: &'a CanonMap, key: &str) -> Result<&'a str, RecordProblem> {
    match entry(map, key)? {
        Canon::Str(s) => Ok(s),
        _ => Err(wrong(key, "a text")),
    }
}

fn int(map: &CanonMap, key: &str) -> Result<i64, RecordProblem> {
    match entry(map, key)? {
        Canon::Int(n) => Ok(*n),
        _ => Err(wrong(key, "a whole number")),
    }
}

fn items<'a>(map: &'a CanonMap, key: &str) -> Result<&'a [Canon], RecordProblem> {
    match entry(map, key)? {
        Canon::List(items) => Ok(items),
        _ => Err(wrong(key, "a list")),
    }
}

fn submap<'a>(map: &'a CanonMap, key: &str) -> Result<&'a CanonMap, RecordProblem> {
    match entry(map, key)? {
        Canon::Map(m) => Ok(m),
        _ => Err(wrong(key, "a map")),
    }
}

/// Checks that a map has exactly these keys.
fn exact(map: &CanonMap, keys: &[&str]) -> Result<(), RecordProblem> {
    exact_with(map, keys, &[])
}

/// Checks that a map has every key of `required`, and no key beyond those and
/// `optional`: the keys added after the first version of a record are optional, so
/// a record written before them reads back as it was (rule A5).
fn exact_with(map: &CanonMap, required: &[&str], optional: &[&str]) -> Result<(), RecordProblem> {
    if let Some(key) = required.iter().find(|k| !map.contains_key(**k)) {
        return Err(missing(key));
    }
    if let Some(key) = map
        .keys()
        .find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(RecordProblem::ExtraKey { key: key.clone() });
    }
    Ok(())
}

/// An optional opaque value: absent when the key is not there.
fn opaque_of<T>(
    map: &CanonMap,
    key: &str,
    make: fn(String) -> Result<T, OpaqueProblem>,
) -> Result<Option<T>, RecordProblem> {
    match map.get(key) {
        None => Ok(None),
        Some(Canon::Str(s)) => {
            make(s.clone())
                .map(Some)
                .map_err(|source| RecordProblem::BadOpaque {
                    key: key.to_owned(),
                    source,
                })
        }
        Some(_) => Err(wrong(key, "a text")),
    }
}

/// Adds an optional text to the entries of a canonical form, only when there is one:
/// a record without it has the form a record of the first version had.
fn push_opaque<'a>(entries: &mut Vec<(&'a str, Canon)>, key: &'a str, value: Option<&str>) {
    if let Some(value) = value {
        entries.push((key, Canon::str(value)));
    }
}

fn doc_hash(map: &CanonMap, key: &str) -> Result<DocHash, RecordProblem> {
    text(map, key)?
        .parse::<ContentHash>()
        .map(DocHash::restore)
        .map_err(|e| bad(key, e))
}

fn doc_id(map: &CanonMap, key: &str) -> Result<DocId, RecordProblem> {
    text(map, key)?.parse::<DocId>().map_err(|e| bad(key, e))
}

fn judgement_id_of(raw: &str, key: &str) -> Result<JudgementId, RecordProblem> {
    raw.parse::<EntityId>()
        .map(JudgementId::new)
        .map_err(|e| bad(key, e))
}

/// A closed set of words of the canonical form, each with its one spelling. `as_str`
/// writes it and `parse` reads it, both from the one table, so the two ends of the
/// form cannot disagree about a word and no code compares a text with a literal.
macro_rules! tags {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }

        impl $name {
            const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

tags!(
    /// The `kind` of a record.
    RecordKind {
        Decision => "decision",
        Revocation => "revocation",
        SameMeaning => "same_meaning",
    }
);

tags!(
    /// The `kind` of what a declaration of the same meaning is about.
    SubjectKind {
        Doc => "doc",
        Artifact => "artifact",
    }
);

tags!(
    /// The `verb` of a decision.
    Verb {
        Approve => "approve",
        Deny => "deny",
        Deprecate => "deprecate",
        Retire => "retire",
    }
);

tags!(
    /// The `state` of what a retirement saw.
    SeenState {
        At => "at",
        Invalid => "invalid",
        Gone => "gone",
        Unknown => "unknown",
    }
);

tags!(
    /// The `kind` of a key in a frame.
    KeyFormKind {
        Prefixed => "prefixed",
        Slug => "slug",
    }
);

tags!(
    /// The `kind` of the tail of a prefixed key.
    TailKind {
        Number => "number",
        Name => "name",
    }
);

impl Verb {
    fn of(verb: &DecisionVerb) -> Self {
        match verb {
            DecisionVerb::Approve { .. } => Self::Approve,
            DecisionVerb::Deny { .. } => Self::Deny,
            DecisionVerb::Deprecate { .. } => Self::Deprecate,
            DecisionVerb::Retire { .. } => Self::Retire,
        }
    }
}

fn key_form_canon(form: &KeyForm) -> Canon {
    match form {
        KeyForm::Prefixed { prefix, tail } => {
            let tail = match tail {
                KeyTail::Number(n) => Canon::map([
                    ("kind", Canon::str(TailKind::Number.as_str())),
                    // A `u64` may not fit the `i64` a canonical integer is, so the
                    // number is kept as its text.
                    ("value", Canon::str(n.to_string())),
                ]),
                KeyTail::Name(name) => Canon::map([
                    ("kind", Canon::str(TailKind::Name.as_str())),
                    ("value", Canon::str(name.as_str())),
                ]),
            };
            Canon::map([
                ("kind", Canon::str(KeyFormKind::Prefixed.as_str())),
                ("prefix", Canon::str(prefix.as_str())),
                ("tail", tail),
            ])
        }
        KeyForm::Slug(slug) => Canon::map([
            ("kind", Canon::str(KeyFormKind::Slug.as_str())),
            ("value", Canon::str(slug.as_str())),
        ]),
    }
}

fn key_form_from(map: &CanonMap) -> Result<KeyForm, RecordProblem> {
    let kind = text(map, "kind")?;
    match KeyFormKind::parse(kind) {
        Some(KeyFormKind::Slug) => {
            exact(map, &["kind", "value"])?;
            Slug::new(text(map, "value")?)
                .map(KeyForm::Slug)
                .map_err(RecordProblem::BadKey)
        }
        Some(KeyFormKind::Prefixed) => {
            exact(map, &["kind", "prefix", "tail"])?;
            let prefix = KeyPrefix::new(text(map, "prefix")?).map_err(RecordProblem::BadKey)?;
            let tail = submap(map, "tail")?;
            exact(tail, &["kind", "value"])?;
            let value = text(tail, "value")?;
            // The tail is read by the grammar's own reader, so a record holds no key
            // a document could not have (a number with a leading zero, or too long).
            let read = parse_tail(value).map_err(RecordProblem::BadKey)?;
            let tail_kind = text(tail, "kind")?;
            let tail = match (TailKind::parse(tail_kind), read) {
                (Some(TailKind::Number), tail @ KeyTail::Number(_))
                | (Some(TailKind::Name), tail @ KeyTail::Name(_)) => tail,
                (Some(_), _) => {
                    return Err(wrong(
                        "value",
                        "the kind of the tail is the kind its text reads as",
                    ));
                }
                (None, _) => {
                    return Err(RecordProblem::UnknownKeyKind {
                        found: tail_kind.to_owned(),
                    });
                }
            };
            Ok(KeyForm::Prefixed { prefix, tail })
        }
        None => Err(RecordProblem::UnknownKeyKind {
            found: kind.to_owned(),
        }),
    }
}

/// The optional key of a pin, and of what an approval saw of its subject and of an
/// artifact, that holds a [`ContentAddress`] (rule A5).
const ADDRESS: &str = "address";
/// The optional key of an approval that holds the [`ContentAddress`] of the approved
/// document's file.
const SEEN_ADDRESS: &str = "seen_address";
/// The optional key of an approval that holds its [`Revision`].
const REVISION: &str = "revision";
/// The optional key of an approval that holds its pins of artifacts (left out when
/// there is none, so an approval that stands on no artifact has the first form).
const ARTIFACTS: &str = "artifacts";

fn pin_canon(pin: &FramePin) -> Canon {
    let mut entries = vec![
        ("link", Canon::str(pin.link().as_str())),
        ("target", Canon::str(pin.target().to_string())),
        (
            "key",
            Canon::map([
                ("book", Canon::str(pin.key().book.as_str())),
                ("form", key_form_canon(&pin.key().form)),
            ]),
        ),
        ("hash", Canon::str(pin.hash().to_string())),
    ];
    push_opaque(
        &mut entries,
        ADDRESS,
        pin.address().map(ContentAddress::as_str),
    );
    Canon::map(entries)
}

fn pin_from(canon: &Canon) -> Result<FramePin, RecordProblem> {
    let Canon::Map(map) = canon else {
        return Err(wrong("frame", "a list of maps"));
    };
    exact_with(map, &["link", "target", "key", "hash"], &[ADDRESS])?;
    let link = EntryName::new(text(map, "link")?).map_err(RecordProblem::BadName)?;
    let target = doc_id(map, "target")?;
    let key = submap(map, "key")?;
    exact(key, &["book", "form"])?;
    let book = BookName::new(text(key, "book")?).map_err(RecordProblem::BadName)?;
    let form = key_form_from(submap(key, "form")?)?;
    let hash = doc_hash(map, "hash")?;
    let address = opaque_of(map, ADDRESS, ContentAddress::from_validated)?;
    Ok(FramePin::new(
        link,
        target,
        DocKey { book, form },
        hash,
        address,
    ))
}

tags!(
    /// The `state` of what an approval saw of an artifact.
    ArtifactSeenState {
        Present => "present",
        Absent => "absent",
    }
);

fn artifact_pin_canon(pin: &ArtifactPin) -> Canon {
    let (state, hash) = match &pin.seen {
        PinnedArtifact::Present { hash, .. } => {
            (ArtifactSeenState::Present, Canon::str(hash.to_string()))
        }
        PinnedArtifact::Absent => (ArtifactSeenState::Absent, Canon::Null),
    };
    let mut entries = vec![
        ("link", Canon::str(pin.link.as_str())),
        ("path", Canon::str(pin.path.as_str())),
        ("state", Canon::str(state.as_str())),
        ("hash", hash),
    ];
    push_opaque(
        &mut entries,
        ADDRESS,
        pin.seen.address().map(ContentAddress::as_str),
    );
    // `bytes` is written as no key, so a pin made the way every pin was made before
    // comparisons existed has the same form, and the same record hash, as before.
    if pin.comparison != Comparison::Bytes {
        entries.push((
            COMPARE,
            Canon::map([
                ("kind", Canon::str(pin.comparison.kind_word())),
                (
                    "ignore",
                    Canon::list(pin.comparison.ignored().map(|p| Canon::str(p.as_str()))),
                ),
            ]),
        ));
    }
    Canon::map(entries)
}

/// The optional key of an artifact pin that holds the comparison its hash was made
/// with, when it is not `bytes`.
const COMPARE: &str = "compare";

fn comparison_from(map: &CanonMap) -> Result<Comparison, RecordProblem> {
    let Some(entry) = map.get(COMPARE) else {
        return Ok(Comparison::Bytes);
    };
    let Canon::Map(compare) = entry else {
        return Err(wrong(COMPARE, "a map"));
    };
    exact(compare, &["kind", "ignore"])?;
    let ignore = items(compare, "ignore")?
        .iter()
        .map(|c| match c {
            Canon::Str(s) => Ok(s.clone()),
            _ => Err(wrong("ignore", "a list of texts")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    // As this code writes it: sorted, each once. Another order is no form made here.
    if ignore.windows(2).any(|w| w[0] >= w[1]) {
        return Err(wrong("ignore", "a sorted list of texts, each once"));
    }
    match Comparison::parse(text(compare, "kind")?, &ignore) {
        // `bytes` is never written as a key, so a record that writes it was not made
        // here.
        Ok(Comparison::Bytes) => Err(wrong(COMPARE, "a comparison other than bytes")),
        Ok(comparison) => Ok(comparison),
        Err(why) => Err(RecordProblem::BadComparison(why)),
    }
}

fn artifact_pin_from(canon: &Canon) -> Result<ArtifactPin, RecordProblem> {
    let Canon::Map(map) = canon else {
        return Err(wrong(ARTIFACTS, "a list of maps"));
    };
    exact_with(map, &["link", "path", "state", "hash"], &[ADDRESS, COMPARE])?;
    let comparison = comparison_from(map)?;
    let link = EntryName::new(text(map, "link")?).map_err(RecordProblem::BadName)?;
    let path = text(map, "path")?
        .parse::<RelPath>()
        .map_err(|e| bad("path", e))?;
    let state = text(map, "state")?;
    let address = opaque_of(map, ADDRESS, ContentAddress::from_validated)?;
    let seen = match (ArtifactSeenState::parse(state), entry(map, "hash")?) {
        (Some(ArtifactSeenState::Present), _) => PinnedArtifact::Present {
            hash: text(map, "hash")?
                .parse::<ContentHash>()
                .map(ArtifactHash::restore)
                .map_err(|e| bad("hash", e))?,
            address,
        },
        (Some(ArtifactSeenState::Absent), Canon::Null) if address.is_none() => {
            PinnedArtifact::Absent
        }
        (Some(ArtifactSeenState::Absent), Canon::Null) => {
            return Err(RecordProblem::ExtraKey {
                key: ADDRESS.to_owned(),
            });
        }
        (Some(ArtifactSeenState::Absent), _) => return Err(wrong("hash", "null")),
        (None, _) => {
            return Err(RecordProblem::UnknownArtifactState {
                found: state.to_owned(),
            });
        }
    };
    Ok(ArtifactPin {
        link,
        path,
        seen,
        comparison,
    })
}

fn seen_canon(seen: &Seen) -> Canon {
    let (state, hash) = match seen {
        Seen::At(hash) => (SeenState::At, Canon::str(hash.to_string())),
        Seen::Not(FailureTag::Invalid) => (SeenState::Invalid, Canon::Null),
        Seen::Not(FailureTag::Gone) => (SeenState::Gone, Canon::Null),
        Seen::Not(FailureTag::Unknown) => (SeenState::Unknown, Canon::Null),
    };
    Canon::map([("state", Canon::str(state.as_str())), ("hash", hash)])
}

fn seen_from(map: &CanonMap) -> Result<Seen, RecordProblem> {
    exact(map, &["state", "hash"])?;
    let state = text(map, "state")?;
    match (SeenState::parse(state), entry(map, "hash")?) {
        (Some(SeenState::At), _) => doc_hash(map, "hash").map(Seen::At),
        (Some(SeenState::Invalid), Canon::Null) => Ok(Seen::Not(FailureTag::Invalid)),
        (Some(SeenState::Gone), Canon::Null) => Ok(Seen::Not(FailureTag::Gone)),
        (Some(SeenState::Unknown), Canon::Null) => Ok(Seen::Not(FailureTag::Unknown)),
        (Some(_), _) => Err(wrong("hash", "null")),
        (None, _) => Err(RecordProblem::UnknownSeen {
            found: state.to_owned(),
        }),
    }
}

/// The keys every decision has, besides the ones its verb adds.
const DECISION_KEYS: [&str; 8] = [
    "v", "kind", "id", "subject", "verb", "actor", "at", "reason",
];

/// The keys a revocation has, and no more.
const REVOCATION_KEYS: [&str; 7] = ["v", "kind", "id", "targets", "actor", "at", "reason"];

/// The keys a declaration of the same meaning has, and no more.
const SAME_MEANING_KEYS: [&str; 9] = [
    "v", "kind", "id", "subject", "from", "to", "actor", "at", "reason",
];

impl Record {
    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        match self {
            Self::Decision(d) => d.id,
            Self::Revocation(r) => r.id,
            Self::SameMeaning(s) => s.id,
        }
    }

    /// The canonical form of the record: the one thing an adapter turns into a wire
    /// format.
    pub fn to_canon(&self) -> Canon {
        match self {
            Self::Decision(d) => decision_canon(d),
            Self::Revocation(r) => Canon::map([
                ("v", Canon::Int(VERSION)),
                ("kind", Canon::str(RecordKind::Revocation.as_str())),
                ("id", Canon::str(r.id.to_string())),
                (
                    "targets",
                    Canon::list(r.targets.iter().map(|t| Canon::str(t.to_string()))),
                ),
                ("actor", Canon::str(r.actor.as_str())),
                ("at", Canon::Int(r.at.0)),
                ("reason", Canon::str(r.reason.as_str())),
            ]),
            Self::SameMeaning(s) => same_meaning_canon(s),
        }
    }

    /// The record a canonical form is the form of: the one reader of a record.
    ///
    /// Refuses a form that is not a map, a version it does not know, a key too many
    /// or too few for the kind (and the verb) of the record, a verb it does not
    /// know, an identifier that is not a UUID v7, and a decision that
    /// `Decision::assemble` refuses. The hashes it reads are restored with
    /// `DocHash::restore`, the one place a hash is made from a record.
    pub fn from_canon(canon: &Canon) -> Result<Self, RecordProblem> {
        let Canon::Map(map) = canon else {
            return Err(RecordProblem::NotAMap);
        };
        match int(map, "v")? {
            VERSION => {}
            found => return Err(RecordProblem::UnknownVersion { found }),
        }
        let kind = text(map, "kind")?;
        match RecordKind::parse(kind) {
            Some(RecordKind::Decision) => decision_from(map).map(Self::Decision),
            Some(RecordKind::Revocation) => revocation_from(map).map(Self::Revocation),
            Some(RecordKind::SameMeaning) => same_meaning_from(map).map(Self::SameMeaning),
            None => Err(RecordProblem::UnknownKind {
                found: kind.to_owned(),
            }),
        }
    }

    /// The hash of the canonical form.
    pub fn hash(&self) -> RecordHash {
        RecordHash(ContentHash::of(HashKind::ShelfJudgement, &self.to_canon()))
    }
}

fn decision_canon(d: &Decision) -> Canon {
    let mut entries: Vec<(&str, Canon)> = vec![
        ("v", Canon::Int(VERSION)),
        ("kind", Canon::str(RecordKind::Decision.as_str())),
        ("verb", Canon::str(Verb::of(&d.verb).as_str())),
        ("id", Canon::str(d.id.to_string())),
        ("subject", Canon::str(d.subject.to_string())),
        ("actor", Canon::str(d.actor.as_str())),
        ("at", Canon::Int(d.at.0)),
        (
            "reason",
            d.reason
                .as_ref()
                .map_or(Canon::Null, |r| Canon::str(r.as_str())),
        ),
    ];
    match &d.verb {
        DecisionVerb::Approve {
            seen,
            frame,
            artifacts,
            expires,
            seen_address,
            revision,
        } => {
            entries.push(("seen", Canon::str(seen.to_string())));
            entries.push(("frame", Canon::list(frame.iter().map(pin_canon))));
            entries.push(("expires", expires.map_or(Canon::Null, |e| Canon::Int(e.0))));
            if !artifacts.is_empty() {
                entries.push((
                    ARTIFACTS,
                    Canon::list(artifacts.iter().map(artifact_pin_canon)),
                ));
            }
            push_opaque(
                &mut entries,
                SEEN_ADDRESS,
                seen_address.as_ref().map(ContentAddress::as_str),
            );
            push_opaque(
                &mut entries,
                REVISION,
                revision.as_ref().map(Revision::as_str),
            );
        }
        DecisionVerb::Deny { seen } | DecisionVerb::Deprecate { seen } => {
            entries.push(("seen", Canon::str(seen.to_string())));
        }
        DecisionVerb::Retire { seen } => entries.push(("seen", seen_canon(seen))),
    }
    Canon::map(entries)
}

/// The fields a decision and a revocation have in the same way, read in this order.
struct Common<R> {
    id: JudgementId,
    actor: Actor,
    at: JudgedAt,
    reason: R,
}

/// The `id`, `actor` and `at` of a record, and its `reason` as `reason` reads it (a
/// decision may have none, a revocation must give one).
fn read_common<R>(
    map: &CanonMap,
    reason: impl FnOnce(&CanonMap) -> Result<R, RecordProblem>,
) -> Result<Common<R>, RecordProblem> {
    Ok(Common {
        id: judgement_id_of(text(map, "id")?, "id")?,
        actor: Actor::new(text(map, "actor")?).map_err(|e| bad("actor", e))?,
        at: JudgedAt(int(map, "at")?),
        reason: reason(map)?,
    })
}

/// The `reason` of a decision: a text, or null for none.
fn optional_reason(map: &CanonMap) -> Result<Option<NonBlank>, RecordProblem> {
    match entry(map, "reason")? {
        Canon::Null => Ok(None),
        Canon::Str(s) => NonBlank::new(s.as_str())
            .map(Some)
            .map_err(|e| bad("reason", e)),
        _ => Err(wrong("reason", "a text or null")),
    }
}

/// The `reason` of a revocation: a text, always.
fn required_reason(map: &CanonMap) -> Result<NonBlank, RecordProblem> {
    NonBlank::new(text(map, "reason")?).map_err(|e| bad("reason", e))
}

fn decision_from(map: &CanonMap) -> Result<Decision, RecordProblem> {
    let verb_name = text(map, "verb")?;
    let Some(verb) = Verb::parse(verb_name) else {
        return Err(RecordProblem::UnknownVerb {
            found: verb_name.to_owned(),
        });
    };
    let (extra, optional): (&[&str], &[&str]) = match verb {
        Verb::Approve => (
            &["seen", "frame", "expires"],
            &[ARTIFACTS, SEEN_ADDRESS, REVISION],
        ),
        Verb::Deny | Verb::Deprecate | Verb::Retire => (&["seen"], &[]),
    };
    let keys: Vec<&str> = DECISION_KEYS.iter().chain(extra).copied().collect();
    exact_with(map, &keys, optional)?;

    let Common {
        id,
        actor,
        at,
        reason,
    } = read_common(map, optional_reason)?;
    let subject = doc_id(map, "subject")?;
    let verb = match verb {
        Verb::Approve => {
            let frame = items(map, "frame")?
                .iter()
                .map(pin_from)
                .collect::<Result<BTreeSet<_>, _>>()?;
            let expires = match entry(map, "expires")? {
                Canon::Null => None,
                Canon::Int(n) => Some(JudgedAt(*n)),
                _ => return Err(wrong("expires", "a whole number or null")),
            };
            let artifacts = match map.get(ARTIFACTS) {
                None => BTreeSet::new(),
                Some(_) => items(map, ARTIFACTS)?
                    .iter()
                    .map(artifact_pin_from)
                    .collect::<Result<BTreeSet<_>, _>>()?,
            };
            DecisionVerb::Approve {
                seen: doc_hash(map, "seen")?,
                frame,
                artifacts,
                expires,
                seen_address: opaque_of(map, SEEN_ADDRESS, ContentAddress::from_validated)?,
                revision: opaque_of(map, REVISION, Revision::from_validated)?,
            }
        }
        Verb::Deny => DecisionVerb::Deny {
            seen: doc_hash(map, "seen")?,
        },
        Verb::Deprecate => DecisionVerb::Deprecate {
            seen: doc_hash(map, "seen")?,
        },
        Verb::Retire => DecisionVerb::Retire {
            seen: seen_from(submap(map, "seen")?)?,
        },
    };
    Decision::assemble(id, subject, verb, actor, at, reason).map_err(RecordProblem::Invariant)
}

fn revocation_from(map: &CanonMap) -> Result<Revocation, RecordProblem> {
    exact(map, &REVOCATION_KEYS)?;
    let Common {
        id,
        actor,
        at,
        reason,
    } = read_common(map, required_reason)?;
    let targets = items(map, "targets")?
        .iter()
        .map(|c| match c {
            Canon::Str(s) => judgement_id_of(s, "targets"),
            _ => Err(wrong("targets", "a list of texts")),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let targets = NonEmptySet::from_set(targets).map_err(|_| RecordProblem::EmptyTargets)?;
    Ok(Revocation::new(id, targets, actor, at, reason))
}

fn same_meaning_canon(s: &SameMeaning) -> Canon {
    let (subject, from, to) = match &s.of {
        SameMeaningOf::Doc { id, from, to } => (
            Canon::map([
                ("kind", Canon::str(SubjectKind::Doc.as_str())),
                ("id", Canon::str(id.to_string())),
            ]),
            Canon::list(from.iter().map(|h| Canon::str(h.to_string()))),
            Canon::str(to.to_string()),
        ),
        SameMeaningOf::Artifact { path, from, to } => (
            Canon::map([
                ("kind", Canon::str(SubjectKind::Artifact.as_str())),
                ("path", Canon::str(path.as_str())),
            ]),
            Canon::list(from.iter().map(|h| Canon::str(h.to_string()))),
            Canon::str(to.to_string()),
        ),
    };
    Canon::map([
        ("v", Canon::Int(VERSION)),
        ("kind", Canon::str(RecordKind::SameMeaning.as_str())),
        ("id", Canon::str(s.id.to_string())),
        ("subject", subject),
        ("from", from),
        ("to", to),
        ("actor", Canon::str(s.actor.as_str())),
        ("at", Canon::Int(s.at.0)),
        ("reason", Canon::str(s.reason.as_str())),
    ])
}

/// The hashes of `from`: a list of one or more texts, each read by `read`.
fn hashes_of<H: Ord>(
    map: &CanonMap,
    read: impl Fn(&str) -> Result<H, RecordProblem>,
) -> Result<NonEmptySet<H>, RecordProblem> {
    let hashes = items(map, "from")?
        .iter()
        .map(|c| match c {
            Canon::Str(s) => read(s),
            _ => Err(wrong("from", "a list of texts")),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    NonEmptySet::from_set(hashes).map_err(|_| RecordProblem::EmptyFrom)
}

fn artifact_hash_of(raw: &str, key: &str) -> Result<ArtifactHash, RecordProblem> {
    raw.parse::<ContentHash>()
        .map(ArtifactHash::restore)
        .map_err(|e| bad(key, e))
}

fn same_meaning_from(map: &CanonMap) -> Result<SameMeaning, RecordProblem> {
    exact(map, &SAME_MEANING_KEYS)?;
    let Common {
        id,
        actor,
        at,
        reason,
    } = read_common(map, required_reason)?;
    let subject = submap(map, "subject")?;
    let kind = text(subject, "kind")?;
    let of = match SubjectKind::parse(kind) {
        Some(SubjectKind::Doc) => {
            exact(subject, &["kind", "id"])?;
            SameMeaningOf::Doc {
                id: doc_id(subject, "id")?,
                from: hashes_of(map, |s| {
                    s.parse::<ContentHash>()
                        .map(DocHash::restore)
                        .map_err(|e| bad("from", e))
                })?,
                to: doc_hash(map, "to")?,
            }
        }
        Some(SubjectKind::Artifact) => {
            exact(subject, &["kind", "path"])?;
            SameMeaningOf::Artifact {
                path: text(subject, "path")?
                    .parse::<RelPath>()
                    .map_err(|e| bad("path", e))?,
                from: hashes_of(map, |s| artifact_hash_of(s, "from"))?,
                to: artifact_hash_of(text(map, "to")?, "to")?,
            }
        }
        None => {
            return Err(RecordProblem::UnknownSubjectKind {
                found: kind.to_owned(),
            });
        }
    };
    SameMeaning::new(id, of, actor, at, reason).map_err(RecordProblem::SameMeaning)
}

/// What the records of a ledger say about one document: the declared status.
///
/// It says what people decided, not whether it still holds: that is what
/// [`assess`](super::freshness::assess) adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredStatus {
    /// No decision stands about the document.
    Pending,
    /// The latest decision approves it.
    Approved {
        /// The decision.
        judgement: JudgementId,
        /// The hash that was approved.
        seen: DocHash,
        /// What the approval stands on.
        frame: BTreeSet<FramePin>,
        /// The artifacts the approval stands on.
        artifacts: BTreeSet<ArtifactPin>,
        /// When it lapses.
        expires: Option<JudgedAt>,
    },
    /// The latest decision denies it.
    Denied {
        /// The decision.
        judgement: JudgementId,
    },
    /// The latest decision deprecates it.
    Deprecated {
        /// The decision.
        judgement: JudgementId,
    },
    /// The latest decision retires it.
    Retired {
        /// The decision.
        judgement: JudgementId,
    },
}

/// A contradiction among the records, which the ledger sets aside.
// `Conflict` holds two or more whole records inline (the enum is as large as that), and
// the other variants hold two identifiers. A ledger has few problems, so the size costs
// little, and boxing the records would only add an allocation per conflict and a deref
// in each match.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LedgerProblem {
    /// Two or more different records with one id: none of them is used.
    #[error("{} different records have the id {id}", records.len())]
    Conflict {
        /// The id.
        id: JudgementId,
        /// The records, which differ.
        records: TwoOrMore<Record>,
    },
    /// A revocation of an id no record has.
    #[error("{revocation} revokes {target}, which is not in the ledger")]
    RevokesUnknown {
        /// The revocation.
        revocation: JudgementId,
        /// The id it names.
        target: JudgementId,
    },
    /// A revocation of a revocation.
    #[error("{revocation} revokes {target}, which is itself a revocation")]
    RevokesRevocation {
        /// The revocation.
        revocation: JudgementId,
        /// The id it names.
        target: JudgementId,
    },
    /// A revocation of an id whose records conflict.
    #[error("{revocation} revokes {target}, whose records conflict")]
    RevokesConflicted {
        /// The revocation.
        revocation: JudgementId,
        /// The id it names.
        target: JudgementId,
    },
}

/// The records grouped by identity: those that stand alone, and the identities whose
/// records conflict.
struct Grouped {
    decisions: BTreeMap<JudgementId, Decision>,
    revocations: BTreeMap<JudgementId, Revocation>,
    same_meanings: BTreeMap<JudgementId, SameMeaning>,
    conflicted: BTreeSet<JudgementId>,
    /// The conflicts.
    problems: Vec<LedgerProblem>,
}

impl Grouped {
    /// The same record twice is one record; two different ones with one id conflict,
    /// and neither is kept.
    fn of(records: Vec<Record>) -> Self {
        let mut by_id: BTreeMap<JudgementId, BTreeMap<RecordHash, Record>> = BTreeMap::new();
        for record in records {
            by_id
                .entry(record.id())
                .or_default()
                .insert(record.hash(), record);
        }

        let mut grouped = Self {
            decisions: BTreeMap::new(),
            revocations: BTreeMap::new(),
            same_meanings: BTreeMap::new(),
            conflicted: BTreeSet::new(),
            problems: Vec::new(),
        };
        for (id, group) in by_id {
            match Count::of(group.into_values()) {
                Count::None => {}
                Count::One(Record::Decision(d)) => {
                    grouped.decisions.insert(id, d);
                }
                Count::One(Record::Revocation(r)) => {
                    grouped.revocations.insert(id, r);
                }
                Count::One(Record::SameMeaning(s)) => {
                    grouped.same_meanings.insert(id, s);
                }
                Count::Many(records) => {
                    grouped.conflicted.insert(id);
                    grouped
                        .problems
                        .push(LedgerProblem::Conflict { id, records });
                }
            }
        }
        grouped
    }
}

/// The decisions and declarations the revocations withdraw. A revocation of a
/// conflict, of a revocation or of what is not there withdraws nothing for that
/// target and is reported in `problems`.
fn revoked_by(grouped: &Grouped, problems: &mut Vec<LedgerProblem>) -> BTreeSet<JudgementId> {
    let Grouped {
        decisions,
        revocations,
        same_meanings,
        conflicted,
        ..
    } = grouped;
    let mut revoked = BTreeSet::new();
    for (id, revocation) in revocations {
        for target in revocation.targets.iter().copied() {
            if conflicted.contains(&target) {
                problems.push(LedgerProblem::RevokesConflicted {
                    revocation: *id,
                    target,
                });
            } else if revocations.contains_key(&target) {
                problems.push(LedgerProblem::RevokesRevocation {
                    revocation: *id,
                    target,
                });
            } else if decisions.contains_key(&target) || same_meanings.contains_key(&target) {
                revoked.insert(target);
            } else {
                problems.push(LedgerProblem::RevokesUnknown {
                    revocation: *id,
                    target,
                });
            }
        }
    }
    revoked
}

/// The records of a project, with their contradictions set aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    decisions: BTreeMap<JudgementId, Decision>,
    /// The declarations of the same meaning that are not revoked.
    same_meanings: BTreeMap<JudgementId, SameMeaning>,
    revoked: BTreeSet<JudgementId>,
    subjects: BTreeSet<DocId>,
    problems: Vec<LedgerProblem>,
}

impl Ledger {
    /// A ledger from records, in any order and with repeats.
    ///
    /// The same record twice is one record. Two different records with one id are a
    /// [`Conflict`](LedgerProblem::Conflict) and neither is used (as two worktrees
    /// that picked one id for two decisions have no reason to be believed over one
    /// another). A revocation of something that is not there, of a revocation, or of
    /// a conflict is set aside for that one target only. A revocation of decisions of
    /// several subjects, or of someone else's, is allowed.
    pub fn build(records: Vec<Record>) -> Self {
        let mut grouped = Grouped::of(records);
        let mut problems = std::mem::take(&mut grouped.problems);
        let revoked = revoked_by(&grouped, &mut problems);
        let Grouped {
            decisions,
            mut same_meanings,
            ..
        } = grouped;
        same_meanings.retain(|id, _| !revoked.contains(id));
        let subjects = decisions.values().map(|d| d.subject).collect();
        Self {
            decisions,
            same_meanings,
            revoked,
            subjects,
            problems,
        }
    }

    /// The declarations, not revoked, that make the hashes `a` and `b` of the
    /// document `id` mean the same, in the order of the way from `a` to `b`; `None`
    /// when none do. The one place two hashes of a document are said to mean the same.
    pub fn doc_same_meaning(
        &self,
        id: DocId,
        a: &DocHash,
        b: &DocHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        let edges = self.same_meanings.values().flat_map(|s| match &s.of {
            SameMeaningOf::Doc { id: of, from, to } if *of == id => {
                from.iter().map(|f| (s.id, *f, *to)).collect::<Vec<_>>()
            }
            _ => Vec::new(),
        });
        declared_way(edges, *a, *b)
    }

    /// As [`Ledger::doc_same_meaning`], for the file at `path`.
    pub fn artifact_same_meaning(
        &self,
        path: &RelPath,
        a: &ArtifactHash,
        b: &ArtifactHash,
    ) -> Option<NonEmptyVec<JudgementId>> {
        let edges = self.same_meanings.values().flat_map(|s| match &s.of {
            SameMeaningOf::Artifact { path: of, from, to } if of == path => {
                from.iter().map(|f| (s.id, *f, *to)).collect::<Vec<_>>()
            }
            _ => Vec::new(),
        });
        declared_way(edges, *a, *b)
    }

    /// A declaration that is not revoked, by its identity.
    pub fn same_meaning(&self, id: JudgementId) -> Option<&SameMeaning> {
        self.same_meanings.get(&id)
    }

    /// Whether the record `id` can be withdrawn: it is a decision or a declaration of
    /// this ledger, and no revocation withdraws it yet. The one place a withdrawal
    /// by identity is checked.
    pub fn check_withdraw(&self, id: JudgementId) -> Result<(), WithdrawRefusal> {
        if self.revoked.contains(&id) {
            Err(WithdrawRefusal::AlreadyWithdrawn { id })
        } else if self.decisions.contains_key(&id) || self.same_meanings.contains_key(&id) {
            Ok(())
        } else {
            Err(WithdrawRefusal::Unknown { id })
        }
    }

    /// The declared status of a document.
    ///
    /// The one place it is read: from the decisions about the document that are not
    /// revoked, the latest, by time and then by the larger id. A revocation is never
    /// a candidate, because only decisions are kept as such.
    pub fn status_of(&self, id: DocId) -> DeclaredStatus {
        match self.standing_decision(id) {
            None => DeclaredStatus::Pending,
            Some(d) => match &d.verb {
                DecisionVerb::Approve {
                    seen,
                    frame,
                    artifacts,
                    expires,
                    ..
                } => DeclaredStatus::Approved {
                    judgement: d.id,
                    seen: *seen,
                    frame: frame.clone(),
                    artifacts: artifacts.clone(),
                    expires: *expires,
                },
                DecisionVerb::Deny { .. } => DeclaredStatus::Denied { judgement: d.id },
                DecisionVerb::Deprecate { .. } => DeclaredStatus::Deprecated { judgement: d.id },
                DecisionVerb::Retire { .. } => DeclaredStatus::Retired { judgement: d.id },
            },
        }
    }

    /// The decision that stands about a document: the latest of those about it that
    /// are not revoked, by time and then by the larger id. The one place it is
    /// chosen; [`Ledger::status_of`] and the snapshot of an approval read it.
    pub fn standing_decision(&self, id: DocId) -> Option<&Decision> {
        self.decisions
            .values()
            .filter(|d| d.subject == id && !self.revoked.contains(&d.id))
            .max_by_key(|d| (d.at, d.id))
    }

    /// The documents some record is about, revoked or not.
    pub fn subjects(&self) -> &BTreeSet<DocId> {
        &self.subjects
    }

    /// The contradictions that were set aside.
    pub fn problems(&self) -> &[LedgerProblem] {
        &self.problems
    }

    /// The graph of the frames of the approvals that stand.
    ///
    /// The one place it is made: freshness reads its order and its cycles from it,
    /// and [`frame_approve`] asks it whether a new approval would close a cycle.
    pub(super) fn frame_graph(&self) -> FrameGraph {
        let mut edges: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
        for subject in &self.subjects {
            if let DeclaredStatus::Approved { frame, .. } = self.status_of(*subject) {
                edges
                    .entry(*subject)
                    .or_default()
                    .extend(frame.iter().map(FramePin::target));
            }
        }
        FrameGraph { edges }
    }
}

/// The graph of the frames of approved documents: an edge runs from an approved
/// document to each upstream its approval pinned.
///
/// This is the graph freshness travels, not the links the shelf holds now: an
/// approval stands on what it saw, and a link rewritten since is a change to tell
/// of, not a path to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FrameGraph {
    edges: BTreeMap<DocId, BTreeSet<DocId>>,
}

impl FrameGraph {
    /// The cycles among the frames, as [`CycleSource::Frames`].
    pub(super) fn cycles(&self) -> Vec<Cycle> {
        cycles_of(CycleSource::Frames, &self.edges)
    }

    /// The cycle that replacing the edges of `subject` with edges to `targets` would
    /// make, when it would make one that `subject` is in.
    ///
    /// An existing cycle that does not include `subject` is not its concern.
    pub(super) fn would_cycle(&self, subject: DocId, targets: &BTreeSet<DocId>) -> Option<Cycle> {
        let mut edges = self.edges.clone();
        edges.insert(subject, targets.clone());
        cycles_of(CycleSource::Frames, &edges)
            .into_iter()
            .find(|cycle| cycle.contains(subject))
    }

    /// The documents of the graph taken in components, upstream first: a component
    /// comes after every component it points at.
    pub(super) fn components(&self) -> Vec<Vec<DocId>> {
        components(&self.edges)
    }
}

/// The frame of an approval of `id`: the facts it would stand on.
///
/// Refuses when the document is not valid, when a `Depends` link cannot be followed
/// (a link to a document that is not valid is one: the build leaves it unresolved),
/// when it names a document people decided does not stand (denied, deprecated or
/// retired: an approval on it would not hold from the start), when the document is
/// in a cycle of links, and when the approval would close a cycle among the frames
/// of approvals. A resolved link carries the key and hash of what it names, and the
/// pin is taken from it as it is.
///
/// One rule decides what is refused here and what is left to freshness: what a
/// decision already says about an upstream refuses; how the upstream stands *now*
/// (an upstream that is only pending, or approved and itself degraded) does not,
/// because it can change without anyone deciding anything about this document. Such
/// an approval is stale (or worse) from the start, and freshness says so.
// The refusal is the answer and is asked once per judgement; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_approve(
    shelf: &Shelf,
    ledger: &Ledger,
    id: DocId,
) -> Result<ApproveFrame, FrameRefusal> {
    let doc = shelf
        .state_of(id)
        .valid_or_failure()
        .map_err(FrameRefusal::NotValid)?;

    let mut pins = BTreeSet::new();
    let mut upstream_files = BTreeMap::new();
    for (link, states) in doc.links_of(Propagation::Depends) {
        for state in states {
            match state {
                LinkState::Unresolved(unresolved) => {
                    return Err(FrameRefusal::UnresolvedDepends {
                        link: link.clone(),
                        unresolved: unresolved.clone(),
                    });
                }
                LinkState::Resolved(upstream) => {
                    let target = upstream.id();
                    let not_standing = match ledger.status_of(target) {
                        DeclaredStatus::Denied { .. } => Some(NotStanding::Denied),
                        DeclaredStatus::Deprecated { .. } => Some(NotStanding::Deprecated),
                        DeclaredStatus::Retired { .. } => Some(NotStanding::Retired),
                        DeclaredStatus::Pending | DeclaredStatus::Approved { .. } => None,
                    };
                    if let Some(status) = not_standing {
                        return Err(FrameRefusal::UpstreamNotStanding {
                            link: link.clone(),
                            target,
                            status,
                        });
                    }
                    if let Some(file) = shelf.doc(target).map(|d| d.origin().clone()) {
                        upstream_files.insert(target, file);
                    }
                    pins.insert(FramePin::new(
                        link.clone(),
                        target,
                        upstream.key().clone(),
                        *upstream.hash(),
                        None,
                    ));
                }
            }
        }
    }

    // Rule A2: every artifact a `Depends` link names is pinned as it is, a file or
    // none. An artifact has no declared status, so nothing here refuses it.
    // Each pin keeps the comparison its hash was made with, the link's.
    let artifacts: BTreeSet<ArtifactPin> = doc
        .artifact_links()
        .iter()
        .filter(|(_, link)| link.kind().propagation == ArtifactPropagation::Depends)
        .flat_map(|(name, link)| {
            let comparison = &link.kind().comparison;
            link.artifacts().iter().map(move |artifact| ArtifactPin {
                link: name.clone(),
                path: artifact.path().clone(),
                seen: match artifact.state() {
                    ArtifactState::Present(hash) => PinnedArtifact::Present {
                        hash: *hash,
                        address: None,
                    },
                    ArtifactState::Absent => PinnedArtifact::Absent,
                },
                comparison: comparison.clone(),
            })
        })
        .collect();

    if let Some(cycle) = shelf.cycles().iter().find(|c| c.contains(id)) {
        return Err(FrameRefusal::InCycle(cycle.clone()));
    }
    let targets: BTreeSet<DocId> = pins.iter().map(FramePin::target).collect();
    if let Some(cycle) = ledger.frame_graph().would_cycle(id, &targets) {
        return Err(FrameRefusal::InCycle(cycle));
    }
    Ok(ApproveFrame {
        subject: id,
        seen: *doc.hash(),
        frame: pins,
        artifacts,
        seen_address: None,
        revision: None,
        subject_file: doc.origin().clone(),
        upstream_files,
    })
}

/// The frame of a denial or a deprecation of `id`: the hash it would see.
///
/// Refuses when the document is not valid.
// The refusal is the answer and is asked once per judgement; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_present(shelf: &Shelf, id: DocId) -> Result<PresentFrame, FrameRefusal> {
    let doc = shelf
        .state_of(id)
        .valid_or_failure()
        .map_err(FrameRefusal::NotValid)?;
    Ok(PresentFrame {
        subject: id,
        seen: *doc.hash(),
    })
}

/// Why a record cannot be withdrawn by its identity.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WithdrawRefusal {
    /// No decision or declaration of the ledger has the identity (a revocation, a
    /// record in conflict, or none at all).
    #[error("{id} is no decision or declaration of the ledger")]
    Unknown {
        /// The identity.
        id: JudgementId,
    },
    /// A revocation already withdraws it.
    #[error("{id} is already withdrawn")]
    AlreadyWithdrawn {
        /// The identity.
        id: JudgementId,
    },
}

/// The frame of a retirement of `id`: what is seen of the document now, whatever it
/// is. It does not fail: a document can be retired after its file is gone.
pub fn frame_retire(shelf: &Shelf, id: DocId) -> RetireFrame {
    let seen = match shelf.state_of(id) {
        DocState::Valid(doc) => Seen::At(*doc.hash()),
        DocState::Invalid(_) => Seen::Not(FailureTag::Invalid),
        DocState::Gone => Seen::Not(FailureTag::Gone),
        DocState::Unknown { .. } => Seen::Not(FailureTag::Unknown),
    };
    RetireFrame { subject: id, seen }
}

mod same_meaning;

use same_meaning::declared_way;
pub use same_meaning::{
    SameMeaning, SameMeaningOf, SameMeaningProblem, SameMeaningRefusal, SameMeaningTarget,
    frame_same_meaning,
};

#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod revocation_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod tests;
