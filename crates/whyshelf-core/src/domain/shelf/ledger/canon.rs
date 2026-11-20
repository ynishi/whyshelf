//! The canonical form of a record, its two ends ([`Record::to_canon`] and
//! [`Record::from_canon`]) and why a form is no record ([`RecordProblem`]): the one
//! version, the keys each kind of record has and the words of the form, for every
//! kind of record at once.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::canon::Canon;
use crate::domain::error::ValueError;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::id::{DocId, EntityId};
use crate::domain::nonempty::{NonBlank, NonEmptySet};

use super::super::artifact::{
    ArtifactHash, ArtifactKey, ArtifactName, ArtifactNameProblem, Comparison, ComparisonProblem,
    ContentAddress, OpaqueProblem, Revision, Space, SpaceProblem,
};
use super::super::build::FailureTag;
use super::super::doc::{DocHash, DocKey};
use super::super::schema::{
    BookName, EntryName, KeyForm, KeyPrefix, KeyProblem, KeyTail, NameProblem, Slug, parse_tail,
};
use super::{
    ArtifactPin, Decision, DecisionProblem, DecisionVerb, Exempt, ExemptOf, FramePin, JudgedAt,
    JudgementId, Moved, MovedOf, MovedProblem, NameForm, PATH_ONLY_SPACE, PinnedArtifact, Record,
    RecordHash, Revocation, SameMeaning, SameMeaningOf, SameMeaningProblem, Seen,
};

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
    /// The space of an artifact is no space.
    #[error("the space of an artifact in the record is not one")]
    BadSpace(#[source] SpaceProblem),
    /// The name of an artifact is no name.
    #[error("the name of an artifact in the record is not one")]
    BadArtifactName(#[source] ArtifactNameProblem),
    /// The record is well formed but breaks an invariant of a declaration of a move.
    #[error("the record breaks an invariant of a declaration of a move")]
    Moved(#[source] MovedProblem),
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
        Moved => "moved",
        Exempt => "exempt",
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
        ("state", Canon::str(state.as_str())),
        ("hash", hash),
    ];
    push_artifact(&mut entries, &pin.artifact, pin.form);
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

/// The keys that name an artifact in a record made now.
const SPACED_KEYS: [&str; 2] = ["space", "name"];
/// The key that names an artifact in a record written before spaces existed.
const PATH_KEY: &str = "path";

/// Adds the artifact to the entries of a canonical form, in the form the record
/// had: `space` and `name`, or, for a record read back that was written before
/// spaces existed, `path` alone, so that it keeps its form and its hash.
fn push_artifact(entries: &mut Vec<(&str, Canon)>, artifact: &ArtifactKey, form: NameForm) {
    match form {
        NameForm::Spaced => {
            entries.push(("space", Canon::str(artifact.space().as_str())));
            entries.push(("name", Canon::str(artifact.name().as_str())));
        }
        NameForm::PathOnly => entries.push((PATH_KEY, Canon::str(artifact.name().as_str()))),
    }
}

/// The artifact a map names, and the form it is written in: `space` and `name`, or
/// `path` and no `space` (a record written before spaces existed, whose path is a
/// name in [`PATH_ONLY_SPACE`]). `others` are the keys the map has beside these,
/// `optional` those it may have; both forms are checked to have no key beyond them.
fn artifact_of(
    map: &CanonMap,
    others: &[&str],
    optional: &[&str],
) -> Result<(ArtifactKey, NameForm), RecordProblem> {
    let name_of =
        |key: &str| ArtifactName::new(text(map, key)?).map_err(RecordProblem::BadArtifactName);
    if map.contains_key("space") || !map.contains_key(PATH_KEY) {
        let required: Vec<&str> = others.iter().copied().chain(SPACED_KEYS).collect();
        exact_with(map, &required, optional)?;
        let space = Space::new(text(map, "space")?).map_err(RecordProblem::BadSpace)?;
        Ok((ArtifactKey::new(space, name_of("name")?), NameForm::Spaced))
    } else {
        let required: Vec<&str> = others.iter().copied().chain([PATH_KEY]).collect();
        exact_with(map, &required, optional)?;
        let space = Space::new(PATH_ONLY_SPACE).map_err(RecordProblem::BadSpace)?;
        Ok((
            ArtifactKey::new(space, name_of(PATH_KEY)?),
            NameForm::PathOnly,
        ))
    }
}

fn artifact_pin_from(canon: &Canon) -> Result<ArtifactPin, RecordProblem> {
    let Canon::Map(map) = canon else {
        return Err(wrong(ARTIFACTS, "a list of maps"));
    };
    let (artifact, form) = artifact_of(map, &["link", "state", "hash"], &[ADDRESS, COMPARE])?;
    let comparison = comparison_from(map)?;
    let link = EntryName::new(text(map, "link")?).map_err(RecordProblem::BadName)?;
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
        artifact,
        seen,
        comparison,
        form,
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

/// The keys a declaration of a move has, and no more.
const MOVED_KEYS: [&str; 9] = [
    "v", "kind", "id", "space", "from", "to", "actor", "at", "reason",
];

/// The keys an exemption has, and no more.
const EXEMPT_KEYS: [&str; 9] = [
    "v", "kind", "id", "subject", "book", "link", "actor", "at", "reason",
];

impl Record {
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
                ("at", Canon::Int(r.at.millis())),
                ("reason", Canon::str(r.reason.as_str())),
            ]),
            Self::SameMeaning(s) => same_meaning_canon(s),
            Self::Moved(m) => moved_canon(m),
            Self::Exempt(e) => exempt_canon(e),
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
            Some(RecordKind::Moved) => moved_from(map).map(Self::Moved),
            Some(RecordKind::Exempt) => exempt_from(map).map(Self::Exempt),
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
        ("verb", Canon::str(Verb::of(d.verb()).as_str())),
        ("id", Canon::str(d.id().to_string())),
        ("subject", Canon::str(d.subject().to_string())),
        ("actor", Canon::str(d.actor().as_str())),
        ("at", Canon::Int(d.at().millis())),
        (
            "reason",
            d.reason().map_or(Canon::Null, |r| Canon::str(r.as_str())),
        ),
    ];
    match d.verb() {
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
            entries.push((
                "expires",
                expires.map_or(Canon::Null, |e| Canon::Int(e.millis())),
            ));
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
        at: JudgedAt::new(int(map, "at")?),
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
                Canon::Int(n) => Some(JudgedAt::new(*n)),
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
        SameMeaningOf::Artifact { artifact, from, to } => {
            let mut subject = vec![("kind", Canon::str(SubjectKind::Artifact.as_str()))];
            push_artifact(&mut subject, artifact, s.form);
            (
                Canon::map(subject),
                Canon::list(from.iter().map(|h| Canon::str(h.to_string()))),
                Canon::str(to.to_string()),
            )
        }
    };
    Canon::map([
        ("v", Canon::Int(VERSION)),
        ("kind", Canon::str(RecordKind::SameMeaning.as_str())),
        ("id", Canon::str(s.id.to_string())),
        ("subject", subject),
        ("from", from),
        ("to", to),
        ("actor", Canon::str(s.actor.as_str())),
        ("at", Canon::Int(s.at.millis())),
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
    let (of, form) = match SubjectKind::parse(kind) {
        Some(SubjectKind::Doc) => {
            exact(subject, &["kind", "id"])?;
            let of = SameMeaningOf::Doc {
                id: doc_id(subject, "id")?,
                from: hashes_of(map, |s| {
                    s.parse::<ContentHash>()
                        .map(DocHash::restore)
                        .map_err(|e| bad("from", e))
                })?,
                to: doc_hash(map, "to")?,
            };
            (of, NameForm::Spaced)
        }
        Some(SubjectKind::Artifact) => {
            let (artifact, form) = artifact_of(subject, &["kind"], &[])?;
            let of = SameMeaningOf::Artifact {
                artifact,
                from: hashes_of(map, |s| artifact_hash_of(s, "from"))?,
                to: artifact_hash_of(text(map, "to")?, "to")?,
            };
            (of, form)
        }
        None => {
            return Err(RecordProblem::UnknownSubjectKind {
                found: kind.to_owned(),
            });
        }
    };
    let mut declared =
        SameMeaning::new(id, of, actor, at, reason).map_err(RecordProblem::SameMeaning)?;
    declared.form = form;
    Ok(declared)
}

fn moved_canon(m: &Moved) -> Canon {
    Canon::map([
        ("v", Canon::Int(VERSION)),
        ("kind", Canon::str(RecordKind::Moved.as_str())),
        ("id", Canon::str(m.id.to_string())),
        ("space", Canon::str(m.of.space().as_str())),
        ("from", Canon::str(m.of.from_name().as_str())),
        ("to", Canon::str(m.of.to_name().as_str())),
        ("actor", Canon::str(m.actor.as_str())),
        ("at", Canon::Int(m.at.millis())),
        ("reason", Canon::str(m.reason.as_str())),
    ])
}

fn moved_from(map: &CanonMap) -> Result<Moved, RecordProblem> {
    exact(map, &MOVED_KEYS)?;
    let Common {
        id,
        actor,
        at,
        reason,
    } = read_common(map, required_reason)?;
    let space = Space::new(text(map, "space")?).map_err(RecordProblem::BadSpace)?;
    let name_of =
        |key: &str| ArtifactName::new(text(map, key)?).map_err(RecordProblem::BadArtifactName);
    let of = MovedOf::new(space, name_of("from")?, name_of("to")?).map_err(RecordProblem::Moved)?;
    Ok(Moved::new(id, of, actor, at, reason))
}

fn exempt_canon(e: &Exempt) -> Canon {
    Canon::map([
        ("v", Canon::Int(VERSION)),
        ("kind", Canon::str(RecordKind::Exempt.as_str())),
        ("id", Canon::str(e.id.to_string())),
        ("subject", Canon::str(e.of.doc().to_string())),
        ("book", Canon::str(e.of.book().as_str())),
        ("link", Canon::str(e.of.link().as_str())),
        ("actor", Canon::str(e.actor.as_str())),
        ("at", Canon::Int(e.at.millis())),
        ("reason", Canon::str(e.reason.as_str())),
    ])
}

fn exempt_from(map: &CanonMap) -> Result<Exempt, RecordProblem> {
    exact(map, &EXEMPT_KEYS)?;
    let Common {
        id,
        actor,
        at,
        reason,
    } = read_common(map, required_reason)?;
    let book = BookName::new(text(map, "book")?).map_err(RecordProblem::BadName)?;
    let link = EntryName::new(text(map, "link")?).map_err(RecordProblem::BadName)?;
    let of = ExemptOf::new(doc_id(map, "subject")?, book, link);
    Ok(Exempt::new(id, of, actor, at, reason))
}
