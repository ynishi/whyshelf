//! A judgement read back from a ledger, and the one decoder of its canonical bytes.
//!
//! A ledger keeps each judgement as its canonical bytes
//! ([`Judgement::canonical_bytes`]). A [`StoredJudgement`] is what those bytes give
//! back: [`StoredJudgement::decode`] reads them with [`Canon::from_bytes`], builds
//! every part through the checking constructor of its type, and accepts the record
//! only if encoding it again gives the bytes it came from. A record that is not
//! exactly what this build writes (an extra key, a name that is not normalised, a
//! default written out, a kind this build does not know) is a [`DecodeError`], not a
//! record.
//!
//! ```text
//!   new judgement                         a record read back
//!   -------------                         ------------------
//!   gate / adoption check                 ledger file (canonical bytes)
//!        |  Passed / Adoptable                    |  StoredJudgement::decode
//!        v                                        v
//!   Judgement::approve ...                StoredJudgement
//!        |  split()                               |  read it, keep it in a Ledger,
//!        v                                        |  copy it to the public ledger
//!   ObjectJudgement | PlainJudgement              v
//!        |                                  Recordable::copied   (the only write)
//!        v
//!   Recordable (the write chain)
//! ```
//!
//! # Why a separate type
//!
//! A [`Passed`] and an [`Adoptable`]
//! can be made only by a check, so that a new approval stands on a check made for it.
//! A record read back has to carry them, and no check was made. Two things keep that
//! from opening the door:
//!
//! - the proofs of a decoded record are made by `pub(in crate::domain)` constructors
//!   that nothing outside the domain can call, and they are marked as restored:
//!   [`Judgement::approve`] and [`Judgement::adopt`] refuse them
//!   ([`JudgementError::ProofRestored`]), so a proof taken out of a stored record
//!   cannot start another approval;
//! - a `StoredJudgement` has no way back to a [`Judgement`]: it cannot be
//!   [`split`](Judgement::split) into the views the write chain takes, so it is not an
//!   approval or an adoption that is about to be recorded. The only write a stored
//!   record takes part in is a publication's copy of a record of the private ledger
//!   (`Recordable::copied`), which compares it byte for byte with the one the ledger
//!   holds.
//!
//! ```compile_fail,E0599
//! use whyshelf_core::domain::stored::StoredJudgement;
//!
//! // A record read back has no `split`: it cannot enter the write chain.
//! fn split(stored: StoredJudgement) {
//!     let _ = stored.split();
//! }
//! ```
//!
//! ```compile_fail,E0599
//! use whyshelf_core::domain::stored::StoredJudgement;
//!
//! // Nor can it be given up as a new judgement.
//! fn fresh(stored: StoredJudgement) {
//!     let _ = stored.into_judgement();
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use whyshelf_core::domain::judgement::Judgement;
//! use whyshelf_core::domain::stored::StoredJudgement;
//!
//! fn fresh(stored: StoredJudgement) -> Judgement {
//!     stored.into()
//! }
//! ```
//!
//! ```compile_fail,E0451
//! use whyshelf_core::domain::judgement::Judgement;
//! use whyshelf_core::domain::stored::StoredJudgement;
//!
//! // The wrapped judgement is private: only `decode` and `From<Judgement>` make one.
//! fn forged(judgement: Judgement) -> StoredJudgement {
//!     StoredJudgement { judgement }
//! }
//! ```
//!
//! ```compile_fail,E0624
//! use whyshelf_core::domain::approval::Attestation;
//! use whyshelf_core::domain::gate::Passed;
//! use whyshelf_core::domain::hash::TextHash;
//!
//! // The constructor of a restored proof is private to the domain.
//! fn forged(attestation: Attestation, text: TextHash) -> Passed {
//!     Passed::restore(attestation, text)
//! }
//! ```
//!
//! # What is read, what is rebuilt
//!
//! | of the record | rebuilt as |
//! |---|---|
//! | `id`, `at`, `judge`, `origin` | the value, through the constructor of its type |
//! | `kind` | the [`JudgementKind`], each part through its constructor |
//! | the proof of an approval or an adoption | a restored [`Passed`] or [`Adoptable`] |
//! | the text a proof read (evidence, not in the bytes) | the `text` of the record's [`ObjectKey`], which the record already keeps equal to it |
//!
//! The checks that hold for every record that was written hold for one read back: the
//! object is of the version the proof pins, and an adoption is not of a measurement
//! (CM). What cannot be checked from the bytes alone (that the file of a retired
//! target is gone, that a publication names records the public ledger holds) is
//! asked of the ledger, as it is for a record just made.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use super::actor::Timestamp;
use super::adoption::{Adoptable, AdoptedAttestation};
use super::approval::{Attestation, CmResult, Gate, Pinned, TestOutcome};
use super::boundary::{Crosses, CrossingText};
use super::canon::{Canon, CanonError, ToCanon};
use super::corpus::Visibility;
use super::error::JudgementError;
use super::event::EventId;
use super::gate::Passed;
use super::hash::{ContentHash, HunkId, PinMac, TextHash};
use super::id::{DesignId, DocId, EntityId, FindingId, ItemId, SpecId};
use super::integration::{ChangeFingerprint, HunkScheme};
use super::judge::{Email, Judge, PersonName, Principal, ToolName};
use super::judgement::{
    Judgement, JudgementId, JudgementKind, ObjectKey, Target, object_of_version,
};
use super::keyring::{KeyedPin, Kid, UpstreamHash};
use super::location::{SourceUri, Uri};
use super::nonempty::{NonBlank, NonEmptySet, NonEmptyVec};
use super::policy::GatePolicy;
use super::revision::{Revision, RevisionId};

/// Why bytes are not a record this build writes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// The bytes are not the canonical encoding of a value.
    #[error("not the canonical encoding of a value: {0}")]
    Bytes(#[from] CanonError),
    /// A part is of another shape than a record has there.
    #[error("{at}: expected {expected}")]
    Shape {
        /// Where in the record.
        at: String,
        /// What a record has there.
        expected: &'static str,
    },
    /// A map lacks a key the record needs.
    #[error("{at}: no `{key}`")]
    Missing {
        /// Where in the record.
        at: String,
        /// The key.
        key: String,
    },
    /// The constructor of a part refused its value.
    #[error("{at}: {reason}")]
    Refused {
        /// Where in the record.
        at: String,
        /// What the constructor said.
        reason: String,
    },
    /// The parts agree with the type they were read into and not with each other.
    #[error(transparent)]
    Inconsistent(#[from] JudgementError),
    /// The record is well formed and is not what this build writes for those parts
    /// (a key it does not know, a value it would have normalised or left out).
    #[error("the record is not in the form this build writes")]
    NotRecanonical,
}

/// A judgement read back from a ledger. See the module documentation.
///
/// It reads like a [`Judgement`] and can be held in a [`Ledger`](super::ledger::Ledger);
/// it cannot start a new record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJudgement {
    judgement: Judgement,
}

impl StoredJudgement {
    /// The record whose canonical bytes these are.
    ///
    /// Exactly the bytes [`canonical_bytes`](Self::canonical_bytes) of the result gives
    /// back, or an error: see the module documentation.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let canon = Canon::from_bytes(bytes)?;
        let root = Node::new(&canon, "record");
        let id = root.field("id")?.parse::<JudgementId>()?;
        let at = root
            .field("at")?
            .build(|text| Timestamp::from_validated(text.to_owned()))?;
        let judge = judge(&root.field("judge")?)?;
        let origin = origin(&root.field("origin")?)?;
        let kind = kind(&root.field("body")?)?;
        let judgement = Judgement::assemble(id, at, judge, origin, kind);
        if judgement.canonical_bytes() != bytes {
            return Err(DecodeError::NotRecanonical);
        }
        Ok(Self { judgement })
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.judgement.id()
    }

    /// When the record was written. For display: it orders nothing.
    pub fn at(&self) -> &Timestamp {
        self.judgement.at()
    }

    /// Who decided.
    pub fn judge(&self) -> &Judge {
        self.judgement.judge()
    }

    /// The ledger the record was first written in.
    pub fn origin(&self) -> Visibility {
        self.judgement.origin()
    }

    /// What was decided.
    pub fn kind(&self) -> &JudgementKind {
        self.judgement.kind()
    }

    /// The version an approval or an adoption keeps an object for.
    pub fn object(&self) -> Option<&ObjectKey> {
        self.judgement.object()
    }

    /// The bytes of the record in the canonical encoding: the ones it was read from,
    /// or, for a record just written, the ones that were written.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.judgement.canonical_bytes()
    }
}

impl From<Judgement> for StoredJudgement {
    /// A record just made, as it will be read back from the ledger it is written to.
    ///
    /// One way: a stored record does not become a [`Judgement`] again.
    fn from(judgement: Judgement) -> Self {
        Self { judgement }
    }
}

impl ToCanon for StoredJudgement {
    fn to_canon(&self) -> Canon {
        self.judgement.to_canon()
    }
}

impl Crosses for StoredJudgement {
    fn cross_into(&self, out: &mut CrossingText) {
        self.judgement.cross_into(out);
    }
}

// --- reading a Canon ----------------------------------------------------------------

/// A place in the value being read, with the path that leads to it for the error.
struct Node<'a> {
    value: &'a Canon,
    at: String,
}

impl<'a> Node<'a> {
    fn new(value: &'a Canon, at: impl Into<String>) -> Self {
        Self {
            value,
            at: at.into(),
        }
    }

    fn shape(&self, expected: &'static str) -> DecodeError {
        DecodeError::Shape {
            at: self.at.clone(),
            expected,
        }
    }

    fn refused(&self, reason: impl fmt::Display) -> DecodeError {
        DecodeError::Refused {
            at: self.at.clone(),
            reason: reason.to_string(),
        }
    }

    fn entries(&self) -> Result<&'a BTreeMap<String, Canon>, DecodeError> {
        match self.value {
            Canon::Map(entries) => Ok(entries),
            _ => Err(self.shape("a map")),
        }
    }

    fn field_opt(&self, key: &str) -> Result<Option<Node<'a>>, DecodeError> {
        Ok(self
            .entries()?
            .get(key)
            .map(|value| Node::new(value, format!("{}.{key}", self.at))))
    }

    fn field(&self, key: &str) -> Result<Node<'a>, DecodeError> {
        self.field_opt(key)?.ok_or_else(|| DecodeError::Missing {
            at: self.at.clone(),
            key: key.to_owned(),
        })
    }

    fn text(&self) -> Result<&'a str, DecodeError> {
        match self.value {
            Canon::Str(text) => Ok(text),
            _ => Err(self.shape("a string")),
        }
    }

    fn flag(&self) -> Result<bool, DecodeError> {
        match self.value {
            Canon::Bool(flag) => Ok(*flag),
            _ => Err(self.shape("a boolean")),
        }
    }

    fn items(&self) -> Result<Vec<Node<'a>>, DecodeError> {
        match self.value {
            Canon::List(items) => Ok(items
                .iter()
                .enumerate()
                .map(|(i, value)| Node::new(value, format!("{}[{i}]", self.at)))
                .collect()),
            _ => Err(self.shape("a list")),
        }
    }

    /// `None` for the absence of a value (`null`), the node itself otherwise.
    fn present(self) -> Option<Self> {
        (self.value != &Canon::Null).then_some(self)
    }

    /// The string, through a constructor that may refuse it.
    fn build<T, E: fmt::Display>(
        &self,
        make: impl FnOnce(&'a str) -> Result<T, E>,
    ) -> Result<T, DecodeError> {
        make(self.text()?).map_err(|error| self.refused(error))
    }

    /// The string, as a value of a type that reads the form it writes.
    fn parse<T: FromStr>(&self) -> Result<T, DecodeError>
    where
        T::Err: fmt::Display,
    {
        self.build(str::parse)
    }

    /// One of the words, by the string.
    fn word<T: Copy>(&self, words: &[(&str, T)]) -> Result<T, DecodeError> {
        let text = self.text()?;
        words
            .iter()
            .find(|(word, _)| *word == text)
            .map(|(_, value)| *value)
            .ok_or_else(|| self.refused(format_args!("`{text}` is not one of the words")))
    }
}

// --- the parts of a record ----------------------------------------------------------

fn origin(node: &Node<'_>) -> Result<Visibility, DecodeError> {
    node.word(&[
        ("public", Visibility::Public),
        ("private", Visibility::Private),
    ])
}

fn judge(node: &Node<'_>) -> Result<Judge, DecodeError> {
    let name = node.field("name")?.build(PersonName::new)?;
    let email = node
        .field("email")?
        .present()
        .map(|email| email.build(Email::new))
        .transpose()?;
    let principal = node.field("principal")?;
    let principal = match principal.field("principal")?.text()? {
        "human" => Principal::Human,
        "undeclared" => Principal::Undeclared,
        "agent" => Principal::Agent {
            tool: principal.field("tool")?.build(ToolName::new)?,
        },
        other => {
            return Err(principal.refused(format_args!("`{other}` is not a principal")));
        }
    };
    Ok(Judge::new(name, email, principal))
}

fn gate(node: &Node<'_>) -> Result<Gate, DecodeError> {
    let words: Vec<(&str, Gate)> = Gate::ALL.iter().map(|gate| (gate.name(), *gate)).collect();
    node.word(&words)
}

fn pinned<I: FromStr>(node: &Node<'_>) -> Result<Pinned<I>, DecodeError>
where
    I::Err: fmt::Display,
{
    Ok(Pinned::new(
        node.field("id")?.parse()?,
        node.field("hash")?.parse()?,
    ))
}

fn pinned_upstream<I: FromStr>(node: &Node<'_>) -> Result<Pinned<I, UpstreamHash>, DecodeError>
where
    I::Err: fmt::Display,
{
    Ok(Pinned::new(
        node.field("id")?.parse()?,
        upstream_hash(&node.field("hash")?)?,
    ))
}

/// A plain pin is a string, a keyed one a map of the key's name and the MAC.
fn upstream_hash(node: &Node<'_>) -> Result<UpstreamHash, DecodeError> {
    match node.value {
        Canon::Str(_) => Ok(UpstreamHash::Plain(node.parse::<ContentHash>()?)),
        Canon::Map(_) => Ok(UpstreamHash::Keyed(KeyedPin::new(
            node.field("kid")?.parse::<Kid>()?,
            node.field("mac")?.parse::<PinMac>()?,
        ))),
        _ => Err(node.shape("a hash or a keyed pin")),
    }
}

fn revision(node: &Node<'_>) -> Result<Revision, DecodeError> {
    let id = |node: Node<'_>| node.build(|text| RevisionId::from_validated(text.to_owned()));
    match node.field("form")?.text()? {
        "commit" => Ok(Revision::Commit(id(node.field("id")?)?)),
        "patch" => Ok(Revision::Patch {
            base: id(node.field("base")?)?,
            patch: node.field("patch")?.parse()?,
        }),
        other => Err(node.refused(format_args!("`{other}` is not a form of revision"))),
    }
}

fn fingerprint(node: &Node<'_>) -> Result<ChangeFingerprint, DecodeError> {
    let listed = node.field("hunks")?;
    let hunks = listed
        .items()?
        .iter()
        .map(|hunk| hunk.parse::<HunkId>())
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(ChangeFingerprint::new(
        revision(&node.field("base")?)?,
        revision(&node.field("baseline")?)?,
        NonEmptySet::from_set(hunks).map_err(|error| listed.refused(error))?,
        node.field("strong")?.flag()?,
        node.field("scheme")?.build(HunkScheme::from_name)?,
    ))
}

fn cm_result(node: &Node<'_>) -> Result<CmResult, DecodeError> {
    match node.field("result")?.text()? {
        "pass" => Ok(CmResult::Pass),
        "fail-wiring" => Ok(CmResult::FailWiring {
            item_hint: node.field("item_hint")?.build(NonBlank::new)?,
        }),
        "fail-hyp" => Ok(CmResult::FailHyp {
            finding: node.field("finding")?.parse::<FindingId>()?,
        }),
        other => Err(node.refused(format_args!("`{other}` is not a result"))),
    }
}

fn policy(node: &Node<'_>) -> Result<GatePolicy, DecodeError> {
    Ok(GatePolicy::new(
        node.field("cs_requires_pins")?.flag()?,
        node.field("cs_requires_hypotheses")?.flag()?,
    ))
}

fn nonempty<T>(node: &Node<'_>, items: Vec<T>) -> Result<NonEmptyVec<T>, DecodeError> {
    NonEmptyVec::from_vec(items).map_err(|error| node.refused(error))
}

fn attestation(node: &Node<'_>) -> Result<Attestation, DecodeError> {
    let target = || node.field("target");
    Ok(match gate(&node.field("gate")?)? {
        Gate::Cr => Attestation::Cr {
            finding: pinned(&target()?)?,
        },
        Gate::Cs => {
            let grounds = node.field("grounds")?;
            let list = grounds
                .items()?
                .iter()
                .map(pinned_upstream::<FindingId>)
                .collect::<Result<Vec<_>, _>>()?;
            Attestation::Cs {
                spec: pinned(&target()?)?,
                hyp: node.field("hyp")?.parse()?,
                grounds: nonempty(&grounds, list)?,
                // The default policy is not written; any other is.
                policy: node
                    .field_opt("policy")?
                    .map(|policy_node| policy(&policy_node))
                    .transpose()?
                    .unwrap_or_default(),
            }
        }
        Gate::Ci => Attestation::Ci {
            design: pinned::<DesignId>(&target()?)?,
            spec: pinned_upstream::<SpecId>(&node.field("spec")?)?,
        },
        Gate::Ca => Attestation::Ca {
            item: pinned::<ItemId>(&target()?)?,
            design: pinned_upstream::<DesignId>(&node.field("design")?)?,
            revision: revision(&node.field("revision")?)?,
            tests: node.field("tests")?.word(&[
                ("passed", TestOutcome::Passed),
                ("failed", TestOutcome::Failed),
            ])?,
            // Left out when there is none.
            fingerprint: node
                .field_opt("change")?
                .map(|change| fingerprint(&change))
                .transpose()?,
        },
        Gate::Cm => Attestation::Cm {
            spec: pinned::<SpecId>(&target()?)?,
            cs: node.field("cs")?.parse::<EventId>()?,
            result: cm_result(&node.field("result")?)?,
        },
    })
}

fn adopted_attestation(node: &Node<'_>) -> Result<AdoptedAttestation, DecodeError> {
    let gate_node = node.field("gate")?;
    let gate = gate(&gate_node)?;
    if gate == Gate::Cm {
        // A measurement is not adopted; `adoption::check` never makes one.
        return Err(gate_node.refused("a measurement (CM) is not adopted"));
    }
    let target = pinned::<EntityId>(&node.field("target")?)?;
    let upstream = node
        .field("upstream")?
        .items()?
        .iter()
        .map(pinned_upstream::<EntityId>)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AdoptedAttestation::restore(
        gate,
        *target.id(),
        *target.hash(),
        upstream,
    ))
}

fn source(node: &Node<'_>) -> Result<SourceUri, DecodeError> {
    if let Some(uri) = node.field_opt("uri")? {
        Ok(SourceUri::Uri(uri.parse::<Uri>()?))
    } else {
        Ok(SourceUri::Citation(
            node.field("citation")?.build(NonBlank::new)?,
        ))
    }
}

fn object_key(node: &Node<'_>) -> Result<ObjectKey, DecodeError> {
    Ok(ObjectKey::new(
        node.field("content")?.parse::<ContentHash>()?,
        node.field("text")?.parse::<TextHash>()?,
    ))
}

fn target(node: &Node<'_>) -> Result<Target, DecodeError> {
    let id = node.field("id")?;
    match node.field("kind")?.text()? {
        "doc" => Ok(Target::Doc(id.parse::<DocId>()?)),
        "item" => Ok(Target::Item(id.parse::<ItemId>()?)),
        other => Err(node.refused(format_args!("`{other}` is not a kind of target"))),
    }
}

fn kind(body: &Node<'_>) -> Result<JudgementKind, DecodeError> {
    let reason = || body.field("reason")?.build(NonBlank::new);
    match body.field("kind")?.text()? {
        "approve" => {
            let attestation = attestation(&body.field("attestation")?)?;
            let object = object_key(&body.field("object")?)?;
            // The text the gate read is not in the bytes: the record keeps it equal to
            // the object's, which is what the proof gets back.
            let passed = Passed::restore(attestation, *object.text());
            object_of_version(&object, passed.attestation().target_hash(), passed.text())?;
            Ok(JudgementKind::Approve { passed, object })
        }
        "adopt" => {
            let adoptable = body.field("adoptable")?;
            let object = object_key(&body.field("object")?)?;
            let attestation = adopted_attestation(&adoptable.field("attestation")?)?;
            let source = source(&adoptable.field("source")?)?;
            let adoptable = Adoptable::restore(attestation, source, *object.text());
            object_of_version(&object, adoptable.attestation().hash(), adoptable.text())?;
            Ok(JudgementKind::Adopt { adoptable, object })
        }
        "revoke" => {
            let targets = body.field("targets")?;
            let list = targets
                .items()?
                .iter()
                .map(|id| id.parse::<JudgementId>())
                .collect::<Result<Vec<_>, _>>()?;
            Ok(JudgementKind::Revoke {
                targets: nonempty(&targets, list)?,
                reason: reason()?,
            })
        }
        "retire" => Ok(JudgementKind::Retire {
            target: target(&body.field("target")?)?,
            reason: reason()?,
        }),
        "published" => Ok(JudgementKind::Published {
            doc: body.field("doc")?.parse()?,
            records: body
                .field("records")?
                .items()?
                .iter()
                .map(|id| id.parse::<JudgementId>())
                .collect::<Result<_, _>>()?,
        }),
        "purge" => Ok(JudgementKind::Purge {
            doc: body.field("doc")?.parse()?,
            object: body.field("object")?.parse()?,
            reason: reason()?,
        }),
        other => Err(body.refused(format_args!("`{other}` is not a kind of judgement"))),
    }
}

#[cfg(test)]
mod tests;
