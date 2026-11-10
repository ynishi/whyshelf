//! The judgements people made, as records, and what the records say.
//!
//! A [`Record`] is a [`Decision`] about one document (approve, deny, deprecate,
//! retire) or a [`Revocation`] of decisions. The domain holds a record as a value
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
//! # Two clocks
//!
//! [`JudgedAt`] is unix milliseconds and has an order, which "the latest decision"
//! and an expiry need. It is a number of milliseconds, not a text.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::canon::Canon;
use crate::domain::error::ValueError;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::id::{DocId, EntityId};
use crate::domain::nonempty::{NonBlank, NonEmptySet, TwoOrMore};

use super::build::{Cycle, CycleSource, DocStateKind, Shelf, components, cycles_of};
use super::doc::{DocHash, DocKey, FramePin, LinkState, UnresolvedLink};
use super::schema::{
    BookName, EntryName, KeyForm, KeyName, KeyPrefix, KeyProblem, KeyTail, NameProblem,
    Propagation, Slug,
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

/// What a retirement saw of the document it retires.
///
/// A document can be retired when its file is gone, or broken, or cannot be told
/// about; then there is no hash to see, and the record says which of these it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// The document was valid, with this hash.
    At(DocHash),
    /// The document's files were invalid.
    Invalid,
    /// There was no file for it.
    Gone,
    /// It could not be told whether there was a file (see
    /// [`DocState::Unknown`](super::build::DocState::Unknown)).
    Unknown,
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
        /// When the approval lapses, if it does.
        expires: Option<JudgedAt>,
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
}

/// The facts an approval stands on, taken by [`frame_approve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproveFrame {
    subject: DocId,
    seen: DocHash,
    frame: BTreeSet<FramePin>,
}

impl ApproveFrame {
    /// The document.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// Its hash.
    pub fn seen(&self) -> &DocHash {
        &self.seen
    }

    /// The upstreams it stands on.
    pub fn frame(&self) -> &BTreeSet<FramePin> {
        &self.frame
    }
}

/// The facts a denial or a deprecation stands on, taken by [`frame_present`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentFrame {
    subject: DocId,
    seen: DocHash,
}

impl PresentFrame {
    /// The document.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// Its hash.
    pub fn seen(&self) -> &DocHash {
        &self.seen
    }
}

/// The facts a retirement stands on, taken by [`frame_retire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetireFrame {
    subject: DocId,
    seen: Seen,
}

impl RetireFrame {
    /// The document.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// What was seen of it.
    pub fn seen(&self) -> &Seen {
        &self.seen
    }
}

/// Why a frame cannot be taken. It owns what it says, so it is a `'static` error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameRefusal {
    /// The document is not valid, so there is nothing sound to judge.
    #[error("the document is not valid: {0:?}")]
    NotValid(DocStateKind),
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
            expires,
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
        if let DecisionVerb::Approve { frame, expires, .. } = &verb {
            if let Some(expires) = *expires
                && expires <= at
            {
                return Err(DecisionProblem::ExpiresNotAfter { at, expires });
            }
            if frame.iter().any(|pin| pin.target() == subject) {
                return Err(DecisionProblem::FrameNamesSubject);
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

    /// What it does.
    pub fn verb(&self) -> &DecisionVerb {
        &self.verb
    }

    /// Who made it.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// When it was made.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// Why, when it says.
    pub fn reason(&self) -> Option<&NonBlank> {
        self.reason.as_ref()
    }
}

/// The withdrawal of earlier decisions, whoever made them and whichever documents
/// they are about. It has no subject and no frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    id: JudgementId,
    targets: NonEmptySet<JudgementId>,
    actor: Actor,
    at: JudgedAt,
    reason: NonBlank,
}

impl Revocation {
    /// A revocation of one or more decisions.
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

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The decisions it withdraws.
    pub fn targets(&self) -> &NonEmptySet<JudgementId> {
        &self.targets
    }

    /// Who made it.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// When it was made.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// Why.
    pub fn reason(&self) -> &NonBlank {
        &self.reason
    }
}

/// One record of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// A decision about a document.
    Decision(Decision),
    /// A withdrawal of decisions.
    Revocation(Revocation),
}

/// The hash of a record's canonical form, which tells two records of one id apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordHash(ContentHash);

impl RecordHash {
    /// The digest.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

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
    if let Some(key) = keys.iter().find(|k| !map.contains_key(**k)) {
        return Err(missing(key));
    }
    if let Some(key) = map.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(RecordProblem::ExtraKey { key: key.clone() });
    }
    Ok(())
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

fn key_form_canon(form: &KeyForm) -> Canon {
    match form {
        KeyForm::Prefixed { prefix, tail } => {
            let tail = match tail {
                KeyTail::Number(n) => Canon::map([
                    ("kind", Canon::str("number")),
                    // A `u64` may not fit the `i64` a canonical integer is, so the
                    // number is kept as its text.
                    ("value", Canon::str(n.to_string())),
                ]),
                KeyTail::Name(name) => Canon::map([
                    ("kind", Canon::str("name")),
                    ("value", Canon::str(name.as_str())),
                ]),
            };
            Canon::map([
                ("kind", Canon::str("prefixed")),
                ("prefix", Canon::str(prefix.as_str())),
                ("tail", tail),
            ])
        }
        KeyForm::Slug(slug) => Canon::map([
            ("kind", Canon::str("slug")),
            ("value", Canon::str(slug.as_str())),
        ]),
    }
}

fn key_form_from(map: &CanonMap) -> Result<KeyForm, RecordProblem> {
    match text(map, "kind")? {
        "slug" => {
            exact(map, &["kind", "value"])?;
            Slug::new(text(map, "value")?)
                .map(KeyForm::Slug)
                .map_err(RecordProblem::BadKey)
        }
        "prefixed" => {
            exact(map, &["kind", "prefix", "tail"])?;
            let prefix = KeyPrefix::new(text(map, "prefix")?).map_err(RecordProblem::BadKey)?;
            let tail = submap(map, "tail")?;
            exact(tail, &["kind", "value"])?;
            let value = text(tail, "value")?;
            let tail = match text(tail, "kind")? {
                "number" => value
                    .parse::<u64>()
                    .ok()
                    .filter(|n| n.to_string() == value)
                    .map(KeyTail::Number)
                    .ok_or_else(|| wrong("value", "a number written as a key writes it")),
                "name" => KeyName::new(value)
                    .map(KeyTail::Name)
                    .map_err(RecordProblem::BadKey),
                other => Err(RecordProblem::UnknownKind {
                    found: other.to_owned(),
                }),
            }?;
            Ok(KeyForm::Prefixed { prefix, tail })
        }
        other => Err(RecordProblem::UnknownKind {
            found: other.to_owned(),
        }),
    }
}

fn pin_canon(pin: &FramePin) -> Canon {
    Canon::map([
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
    ])
}

fn pin_from(canon: &Canon) -> Result<FramePin, RecordProblem> {
    let Canon::Map(map) = canon else {
        return Err(wrong("frame", "a list of maps"));
    };
    exact(map, &["link", "target", "key", "hash"])?;
    let link = EntryName::new(text(map, "link")?).map_err(RecordProblem::BadName)?;
    let target = doc_id(map, "target")?;
    let key = submap(map, "key")?;
    exact(key, &["book", "form"])?;
    let book = BookName::new(text(key, "book")?).map_err(RecordProblem::BadName)?;
    let form = key_form_from(submap(key, "form")?)?;
    let hash = doc_hash(map, "hash")?;
    Ok(FramePin::restore(link, target, DocKey { book, form }, hash))
}

fn seen_canon(seen: &Seen) -> Canon {
    let (state, hash) = match seen {
        Seen::At(hash) => ("at", Canon::str(hash.to_string())),
        Seen::Invalid => ("invalid", Canon::Null),
        Seen::Gone => ("gone", Canon::Null),
        Seen::Unknown => ("unknown", Canon::Null),
    };
    Canon::map([("state", Canon::str(state)), ("hash", hash)])
}

fn seen_from(map: &CanonMap) -> Result<Seen, RecordProblem> {
    exact(map, &["state", "hash"])?;
    match (text(map, "state")?, entry(map, "hash")?) {
        ("at", _) => doc_hash(map, "hash").map(Seen::At),
        ("invalid", Canon::Null) => Ok(Seen::Invalid),
        ("gone", Canon::Null) => Ok(Seen::Gone),
        ("unknown", Canon::Null) => Ok(Seen::Unknown),
        ("invalid" | "gone" | "unknown", _) => Err(wrong("hash", "null")),
        (other, _) => Err(RecordProblem::UnknownSeen {
            found: other.to_owned(),
        }),
    }
}

const DECISION_KEYS: [&str; 8] = [
    "v", "kind", "id", "subject", "verb", "actor", "at", "reason",
];

impl Record {
    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        match self {
            Self::Decision(d) => d.id,
            Self::Revocation(r) => r.id,
        }
    }

    /// The canonical form of the record: the one thing an adapter turns into a wire
    /// format.
    pub fn to_canon(&self) -> Canon {
        match self {
            Self::Decision(d) => decision_canon(d),
            Self::Revocation(r) => Canon::map([
                ("v", Canon::Int(VERSION)),
                ("kind", Canon::str("revocation")),
                ("id", Canon::str(r.id.to_string())),
                (
                    "targets",
                    Canon::list(r.targets.iter().map(|t| Canon::str(t.to_string()))),
                ),
                ("actor", Canon::str(r.actor.as_str())),
                ("at", Canon::Int(r.at.0)),
                ("reason", Canon::str(r.reason.as_str())),
            ]),
        }
    }

    /// The record a canonical form is the form of: the one reader of a record.
    ///
    /// Refuses a form that is not a map, a version it does not know, a key too many
    /// or too few for the kind (and the verb) of the record, a verb it does not
    /// know, an identifier that is not a UUID v7, and a decision that
    /// `Decision::assemble` refuses. The hashes and the frame it reads are restored
    /// with `DocHash::restore` and `FramePin::restore`.
    pub fn from_canon(canon: &Canon) -> Result<Self, RecordProblem> {
        let Canon::Map(map) = canon else {
            return Err(RecordProblem::NotAMap);
        };
        match int(map, "v")? {
            VERSION => {}
            found => return Err(RecordProblem::UnknownVersion { found }),
        }
        match text(map, "kind")? {
            "decision" => decision_from(map).map(Self::Decision),
            "revocation" => revocation_from(map).map(Self::Revocation),
            other => Err(RecordProblem::UnknownKind {
                found: other.to_owned(),
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
        ("kind", Canon::str("decision")),
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
            expires,
        } => {
            entries.push(("verb", Canon::str("approve")));
            entries.push(("seen", Canon::str(seen.to_string())));
            entries.push(("frame", Canon::list(frame.iter().map(pin_canon))));
            entries.push(("expires", expires.map_or(Canon::Null, |e| Canon::Int(e.0))));
        }
        DecisionVerb::Deny { seen } => {
            entries.push(("verb", Canon::str("deny")));
            entries.push(("seen", Canon::str(seen.to_string())));
        }
        DecisionVerb::Deprecate { seen } => {
            entries.push(("verb", Canon::str("deprecate")));
            entries.push(("seen", Canon::str(seen.to_string())));
        }
        DecisionVerb::Retire { seen } => {
            entries.push(("verb", Canon::str("retire")));
            entries.push(("seen", seen_canon(seen)));
        }
    }
    Canon::map(entries)
}

fn decision_from(map: &CanonMap) -> Result<Decision, RecordProblem> {
    let verb_name = text(map, "verb")?;
    let extra: &[&str] = match verb_name {
        "approve" => &["seen", "frame", "expires"],
        "deny" | "deprecate" | "retire" => &["seen"],
        other => {
            return Err(RecordProblem::UnknownVerb {
                found: other.to_owned(),
            });
        }
    };
    let keys: Vec<&str> = DECISION_KEYS.iter().chain(extra).copied().collect();
    exact(map, &keys)?;

    let id = judgement_id_of(text(map, "id")?, "id")?;
    let subject = doc_id(map, "subject")?;
    let actor = Actor::new(text(map, "actor")?).map_err(|e| bad("actor", e))?;
    let at = JudgedAt(int(map, "at")?);
    let reason = match entry(map, "reason")? {
        Canon::Null => None,
        Canon::Str(s) => Some(NonBlank::new(s.as_str()).map_err(|e| bad("reason", e))?),
        _ => return Err(wrong("reason", "a text or null")),
    };
    let verb = match verb_name {
        "approve" => {
            let frame = items(map, "frame")?
                .iter()
                .map(pin_from)
                .collect::<Result<BTreeSet<_>, _>>()?;
            let expires = match entry(map, "expires")? {
                Canon::Null => None,
                Canon::Int(n) => Some(JudgedAt(*n)),
                _ => return Err(wrong("expires", "a whole number or null")),
            };
            DecisionVerb::Approve {
                seen: doc_hash(map, "seen")?,
                frame,
                expires,
            }
        }
        "deny" => DecisionVerb::Deny {
            seen: doc_hash(map, "seen")?,
        },
        "deprecate" => DecisionVerb::Deprecate {
            seen: doc_hash(map, "seen")?,
        },
        _ => DecisionVerb::Retire {
            seen: seen_from(submap(map, "seen")?)?,
        },
    };
    Decision::assemble(id, subject, verb, actor, at, reason).map_err(RecordProblem::Invariant)
}

fn revocation_from(map: &CanonMap) -> Result<Revocation, RecordProblem> {
    exact(
        map,
        &["v", "kind", "id", "targets", "actor", "at", "reason"],
    )?;
    let id = judgement_id_of(text(map, "id")?, "id")?;
    let targets = items(map, "targets")?
        .iter()
        .map(|c| match c {
            Canon::Str(s) => judgement_id_of(s, "targets"),
            _ => Err(wrong("targets", "a list of texts")),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let targets = NonEmptySet::from_set(targets).map_err(|_| RecordProblem::EmptyTargets)?;
    let actor = Actor::new(text(map, "actor")?).map_err(|e| bad("actor", e))?;
    let at = JudgedAt(int(map, "at")?);
    let reason = NonBlank::new(text(map, "reason")?).map_err(|e| bad("reason", e))?;
    Ok(Revocation::new(id, targets, actor, at, reason))
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
// A problem is rare and read once; the variants are kept flat for the sake of the match.
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

/// The records of a project, with their contradictions set aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    decisions: BTreeMap<JudgementId, Decision>,
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
        let mut by_id: BTreeMap<JudgementId, BTreeMap<RecordHash, Record>> = BTreeMap::new();
        for record in records {
            by_id
                .entry(record.id())
                .or_default()
                .insert(record.hash(), record);
        }

        let mut problems = Vec::new();
        let mut conflicted = BTreeSet::new();
        let mut decisions = BTreeMap::new();
        let mut revocations = BTreeMap::new();
        for (id, group) in by_id {
            let mut group = group.into_values();
            match (group.next(), group.next()) {
                (Some(only), None) => match only {
                    Record::Decision(d) => {
                        decisions.insert(id, d);
                    }
                    Record::Revocation(r) => {
                        revocations.insert(id, r);
                    }
                },
                (Some(first), Some(second)) => {
                    conflicted.insert(id);
                    problems.push(LedgerProblem::Conflict {
                        id,
                        records: TwoOrMore::new(first, second, group.collect()),
                    });
                }
                (None, _) => {}
            }
        }

        let mut revoked = BTreeSet::new();
        for (id, revocation) in &revocations {
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
                } else if decisions.contains_key(&target) {
                    revoked.insert(target);
                } else {
                    problems.push(LedgerProblem::RevokesUnknown {
                        revocation: *id,
                        target,
                    });
                }
            }
        }

        let subjects = decisions.values().map(|d| d.subject).collect();
        Self {
            decisions,
            revoked,
            subjects,
            problems,
        }
    }

    /// The declared status of a document.
    ///
    /// The one place it is read: from the decisions about the document that are not
    /// revoked, the latest, by time and then by the larger id. A revocation is never
    /// a candidate, because only decisions are kept as such.
    pub fn status_of(&self, id: DocId) -> DeclaredStatus {
        let latest = self
            .decisions
            .values()
            .filter(|d| d.subject == id && !self.revoked.contains(&d.id))
            .max_by_key(|d| (d.at, d.id));
        match latest {
            None => DeclaredStatus::Pending,
            Some(d) => match &d.verb {
                DecisionVerb::Approve {
                    seen,
                    frame,
                    expires,
                } => DeclaredStatus::Approved {
                    judgement: d.id,
                    seen: *seen,
                    frame: frame.clone(),
                    expires: *expires,
                },
                DecisionVerb::Deny { .. } => DeclaredStatus::Denied { judgement: d.id },
                DecisionVerb::Deprecate { .. } => DeclaredStatus::Deprecated { judgement: d.id },
                DecisionVerb::Retire { .. } => DeclaredStatus::Retired { judgement: d.id },
            },
        }
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
    pub fn frame_graph(&self) -> FrameGraph {
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
pub struct FrameGraph {
    edges: BTreeMap<DocId, BTreeSet<DocId>>,
}

impl FrameGraph {
    /// The cycles among the frames, as [`CycleSource::Frames`].
    pub fn cycles(&self) -> Vec<Cycle> {
        cycles_of(CycleSource::Frames, &self.edges)
    }

    /// The cycle that replacing the edges of `subject` with edges to `targets` would
    /// make, when it would make one that `subject` is in.
    ///
    /// An existing cycle that does not include `subject` is not its concern.
    pub fn would_cycle(&self, subject: DocId, targets: &BTreeSet<DocId>) -> Option<Cycle> {
        let mut edges = self.edges.clone();
        edges.insert(subject, targets.clone());
        cycles_of(CycleSource::Frames, &edges)
            .into_iter()
            .find(|cycle| cycle.contains(subject))
    }

    /// The documents of the graph taken in components, upstream first: a component
    /// comes after every component it points at.
    pub fn components(&self) -> Vec<Vec<DocId>> {
        components(&self.edges)
    }
}

/// The frame of an approval of `id`: the facts it would stand on.
///
/// Refuses when the document is not valid, when a `Depends` link cannot be followed,
/// when the document is in a cycle of links, and when the approval would close a
/// cycle among the frames of approvals. An upstream that is itself only pending is
/// not a refusal: the approval is then stale from the start, which freshness says.
// The refusal is the answer and is read once; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_approve(
    shelf: &Shelf,
    ledger: &Ledger,
    id: DocId,
) -> Result<ApproveFrame, FrameRefusal> {
    let doc = shelf
        .state_of(id)
        .valid_or_kind()
        .map_err(FrameRefusal::NotValid)?;

    let mut pins = BTreeSet::new();
    for (link, states) in shelf.links_of(doc, Propagation::Depends) {
        for state in states {
            match state {
                LinkState::Unresolved(unresolved) => {
                    return Err(FrameRefusal::UnresolvedDepends {
                        link: link.clone(),
                        unresolved: unresolved.clone(),
                    });
                }
                LinkState::Resolved { target } => {
                    if let Some(upstream) = shelf.doc(*target) {
                        pins.insert(FramePin::taken(
                            link.clone(),
                            *target,
                            upstream.key().clone(),
                            *upstream.hash(),
                        ));
                    }
                }
            }
        }
    }

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
    })
}

/// The frame of a denial or a deprecation of `id`: the hash it would see.
///
/// Refuses when the document is not valid.
// The refusal is the answer and is read once; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_present(shelf: &Shelf, id: DocId) -> Result<PresentFrame, FrameRefusal> {
    let doc = shelf
        .state_of(id)
        .valid_or_kind()
        .map_err(FrameRefusal::NotValid)?;
    Ok(PresentFrame {
        subject: id,
        seen: *doc.hash(),
    })
}

/// The frame of a retirement of `id`: what is seen of the document now, whatever it
/// is. It does not fail: a document can be retired after its file is gone.
pub fn frame_retire(shelf: &Shelf, id: DocId) -> RetireFrame {
    let seen = match shelf.state_of(id).valid_or_kind() {
        Ok(doc) => Seen::At(*doc.hash()),
        Err(DocStateKind::Invalid(_)) => Seen::Invalid,
        Err(DocStateKind::Unknown(_)) => Seen::Unknown,
        Err(DocStateKind::Gone) => Seen::Gone,
    };
    RetireFrame { subject: id, seen }
}

#[cfg(test)]
mod tests;
