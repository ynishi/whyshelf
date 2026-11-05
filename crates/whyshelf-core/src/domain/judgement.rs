//! A judgement: one decision a person made, as one immutable record.
//!
//! A judgement is not rebuilt from a file. It is typed at a command by a person (or
//! an agent), and from then on it only exists in a [`ledger`](super::ledger). That
//! is why it needs no hash of its content as an identity, no graph of heads and no
//! digest chain: it has an identifier the caller made ([`JudgementId`], a UUID v7;
//! the domain does not make one) and the record is never written again.
//!
//! # The record
//!
//! | field | meaning |
//! |---|---|
//! | `id` | the identity of the record |
//! | `at` | when it was written, **for display only**; it orders nothing, and neither does the time inside a UUID v7 |
//! | `judge` | who decided ([`Judge`]) |
//! | `origin` | the ledger the record was first written in; copying it to the other ledger byte for byte does not change it |
//! | `kind` | what was decided ([`JudgementKind`]) |
//!
//! Where order matters (what a revocation takes away) the record is named, never
//! placed in time: see [`ledger`](super::ledger).
//!
//! # What can be decided
//!
//! | kind | decides | revocable |
//! |---|---|---|
//! | [`Approve`](JudgementKind::Approve) | a gate let the target through and a person approved it; made from a [`Passed`] and nothing else | yes |
//! | [`Adopt`](JudgementKind::Adopt) | an earlier decision is taken in as an approval; made from an [`Adoptable`] and nothing else | yes |
//! | [`Revoke`](JudgementKind::Revoke) | the records named no longer stand | no |
//! | [`Retire`](JudgementKind::Retire) | a document or item whose file is gone is retired | yes |
//! | [`Published`](JudgementKind::Published) | the commit point of a publication: the records named now count in the public ledger | no |
//! | [`Purge`](JudgementKind::Purge) | the stored text of a version is deleted | no |
//!
//! An approval is made again, and an object is put again; a revocation, a purge and
//! a publication are not undone. A [`Revoke`](JudgementKind::Revoke) is built
//! through [`Ledgers::revoke`](super::ledger::Ledgers::revoke), which looks the
//! records up.
//!
//! # How records are made
//!
//! The fields of [`Judgement`] are private and each kind has a constructor that
//! checks what that kind must satisfy. [`JudgementKind`] itself can be built by
//! anyone, as a statement of intent: it is what [`JudgementKind::ledger`] is asked
//! to place before the record exists. It is not a record, and no [`Judgement`] can be made from one without the
//! checks.
//!
//! # The canonical form
//!
//! [`Judgement::canonical_bytes`] is the bytes of the record in the canonical
//! encoding ([`canon`](super::canon)). Two records of one identifier are the same
//! record exactly when these bytes are equal; otherwise they are
//! [`Corrupt`](super::ledger::Ledger::is_corrupt).

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use super::actor::Timestamp;
use super::adoption::Adoptable;
use super::approval::{Gate, GateBasis, UpstreamRecord};
use super::canon::{Canon, ToCanon};
use super::corpus::Visibility;
use super::error::{JudgementError, ValueError};
use super::event::EntityKind;
use super::gate::Passed;
use super::hash::{ContentHash, TextHash};
use super::id::{DocId, EntityId, ItemId};
use super::judge::Judge;
use super::ledger::Ledgers;
use super::location::SourceUri;
use super::nonempty::{NonBlank, NonEmptyVec};

/// The identity of a judgement: a UUID v7 the caller made.
///
/// Why given and not derived: a judgement is a decision somebody made, not a value
/// computed from other values. Two people who approve the same thing make two
/// records; a person who types a command twice makes two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudgementId(EntityId);

impl JudgementId {
    /// A record identity from an entity identifier (a UUID v7).
    pub fn new(id: EntityId) -> Self {
        Self(id)
    }

    /// The identifier inside.
    pub fn entity(&self) -> EntityId {
        self.0
    }
}

impl fmt::Display for JudgementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for JudgementId {
    type Err = ValueError;

    /// Only the form [`Display`](fmt::Display) writes, as for [`EntityId`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<EntityId>().map(Self)
    }
}

impl ToCanon for JudgementId {
    fn to_canon(&self) -> Canon {
        self.0.to_canon()
    }
}

/// What a judgement is about when it is about a thing with a file: a document, or
/// an item of a Design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// A document.
    Doc(DocId),
    /// An item of a Design.
    Item(ItemId),
}

impl Target {
    /// The identifier of the thing.
    pub fn entity(&self) -> EntityId {
        match self {
            Self::Doc(id) => id.entity(),
            Self::Item(id) => id.entity(),
        }
    }

    /// The target a gate's approval of `subject` is about: an item for CA, a
    /// document for every other gate.
    pub fn of_subject(gate: Gate, subject: EntityId) -> Self {
        match gate.target_kind() {
            EntityKind::Item => Self::Item(ItemId::new(subject)),
            EntityKind::Finding | EntityKind::Spec | EntityKind::Design => {
                Self::Doc(DocId::new(subject))
            }
        }
    }
}

/// The document a pinning record belongs to: the Design an item record pins (the
/// first upstream of CA), the subject itself for every other gate. The one place that
/// decides it, for approvals and adoptions alike.
fn home_of(gate: Gate, subject: EntityId, design: Option<EntityId>) -> Option<DocId> {
    match gate.target_kind() {
        EntityKind::Item => design.map(DocId::new),
        EntityKind::Finding | EntityKind::Spec | EntityKind::Design => Some(DocId::new(subject)),
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Doc(id) => write!(f, "document {id}"),
            Self::Item(id) => write!(f, "item {id}"),
        }
    }
}

impl ToCanon for Target {
    fn to_canon(&self) -> Canon {
        let (kind, id) = match self {
            Self::Doc(id) => ("doc", id.to_canon()),
            Self::Item(id) => ("item", id.to_canon()),
        };
        Canon::map([("kind", Canon::str(kind)), ("id", id)])
    }
}

/// Whether the file of a document (or of the Design holding an item) is there.
///
/// A fact the application reads and passes in; the domain never looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FilePresence {
    /// The file is there.
    Present,
    /// The file is not there.
    Absent,
}

/// The two hashes that name a version approved: what was judged, and the text kept
/// of it.
///
/// `content` is the hash the approval pins (the attestation's hash). `text` is the
/// hash of the normalised full text of that version, which is the key of the object
/// kept for it. Two texts that differ only in an alias or a tag have one `content`
/// and two `text`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey {
    content: ContentHash,
    text: TextHash,
}

impl ObjectKey {
    /// A key from the two hashes.
    pub fn new(content: ContentHash, text: TextHash) -> Self {
        Self { content, text }
    }

    /// The hash of what was judged.
    pub fn content(&self) -> &ContentHash {
        &self.content
    }

    /// The hash of the text kept.
    pub fn text(&self) -> &TextHash {
        &self.text
    }
}

impl ToCanon for ObjectKey {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("content", self.content.to_canon()),
            ("text", Canon::Str(self.text.to_string())),
        ])
    }
}

/// What an approval stands on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Basis {
    /// A gate's machine check passed first.
    ///
    /// For CS it carries the policy the check passed under, so a reader tells an
    /// approval made under a relaxed policy by it; the other gates demand the same of
    /// everyone.
    Gate(GateBasis),
    /// An earlier decision was taken in, from this source, without a check.
    Adopted {
        /// Where the decision came from.
        source: SourceUri,
    },
}

/// What was decided. See the module documentation for each kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgementKind {
    /// A gate passed and a person approved.
    Approve {
        /// The proof the gate passed, with what the approval says.
        passed: Passed,
        /// The version approved.
        object: ObjectKey,
    },
    /// An earlier decision is taken in as an approval.
    Adopt {
        /// The proof the adoption was checked, with what the approval says.
        adoptable: Adoptable,
        /// The version approved.
        object: ObjectKey,
    },
    /// The records named no longer stand.
    Revoke {
        /// The records.
        targets: NonEmptyVec<JudgementId>,
        /// Why.
        reason: NonBlank,
    },
    /// A document or item that is gone is retired.
    Retire {
        /// What is retired.
        target: Target,
        /// Why.
        reason: NonBlank,
    },
    /// The commit point of a publication.
    Published {
        /// The document published.
        doc: DocId,
        /// The records copied to the public ledger that now count.
        records: BTreeSet<JudgementId>,
    },
    /// The stored text of a version is deleted.
    Purge {
        /// The document the version belonged to.
        doc: DocId,
        /// The text deleted.
        object: TextHash,
        /// Why.
        reason: NonBlank,
    },
}

impl JudgementKind {
    /// The word for the kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Approve { .. } => "approve",
            Self::Adopt { .. } => "adopt",
            Self::Revoke { .. } => "revoke",
            Self::Retire { .. } => "retire",
            Self::Published { .. } => "published",
            Self::Purge { .. } => "purge",
        }
    }

    /// Whether a revocation may name a record of this kind: an approval, an
    /// adoption or a retirement.
    pub fn is_revocable(&self) -> bool {
        match self {
            Self::Approve { .. } | Self::Adopt { .. } | Self::Retire { .. } => true,
            Self::Revoke { .. } | Self::Published { .. } | Self::Purge { .. } => false,
        }
    }

    /// What the record pins, if it is an approval or an adoption.
    pub(super) fn pinning(&self) -> Option<Pinning<'_>> {
        match self {
            Self::Approve { passed, .. } => Some(Pinning {
                gate: passed.gate(),
                subject: passed.attestation().subject(),
                hash: passed.attestation().target_hash(),
                basis: Basis::Gate(passed.gate_basis()),
            }),
            Self::Adopt { adoptable, .. } => Some(Pinning {
                gate: adoptable.attestation().gate(),
                subject: adoptable.attestation().subject(),
                hash: adoptable.attestation().hash(),
                basis: Basis::Adopted {
                    source: adoptable.source().clone(),
                },
            }),
            Self::Revoke { .. }
            | Self::Retire { .. }
            | Self::Published { .. }
            | Self::Purge { .. } => None,
        }
    }

    /// The document an approval, an adoption or a retirement belongs to: the target
    /// itself for a document, and for an item the Design the record pins. `None` for
    /// a retirement of an item (it pins no Design) and for the kinds a publication
    /// does not carry.
    pub(super) fn home_doc(&self) -> Option<DocId> {
        match self {
            Self::Approve { passed, .. } => {
                let attestation = passed.attestation();
                let design = attestation.upstream().first().and_then(|up| match up {
                    UpstreamRecord::Entity { id, .. } => Some(*id),
                    UpstreamRecord::Event(_) => None,
                });
                home_of(attestation.gate(), attestation.subject(), design)
            }
            Self::Adopt { adoptable, .. } => {
                let attestation = adoptable.attestation();
                let design = attestation.upstream().first().map(|up| *up.id());
                home_of(attestation.gate(), attestation.subject(), design)
            }
            Self::Retire {
                target: Target::Doc(doc),
                ..
            } => Some(*doc),
            Self::Retire {
                target: Target::Item(_),
                ..
            }
            | Self::Revoke { .. }
            | Self::Published { .. }
            | Self::Purge { .. } => None,
        }
    }

    /// What the record is about: a thing, for an approval, an adoption, a retirement,
    /// a purge or a publication; the records it names, for a revocation, which are
    /// about what a ledger can say.
    pub(super) fn about(&self) -> About<'_> {
        match self {
            Self::Approve { passed, .. } => About::Target(Target::of_subject(
                passed.gate(),
                passed.attestation().subject(),
            )),
            Self::Adopt { adoptable, .. } => About::Target(Target::of_subject(
                adoptable.attestation().gate(),
                adoptable.attestation().subject(),
            )),
            Self::Retire { target, .. } => About::Target(*target),
            Self::Published { doc, .. } | Self::Purge { doc, .. } => {
                About::Target(Target::Doc(*doc))
            }
            Self::Revoke { targets, .. } => About::Records(targets),
        }
    }
}

/// What a record is about; see [`JudgementKind::about`].
pub(super) enum About<'a> {
    /// A document or an item.
    Target(Target),
    /// Other records.
    Records(&'a NonEmptyVec<JudgementId>),
}

impl ToCanon for JudgementKind {
    fn to_canon(&self) -> Canon {
        let kind = ("kind", Canon::str(self.name()));
        match self {
            Self::Approve { passed, object } => Canon::map([
                kind,
                ("attestation", passed.attestation().to_canon()),
                ("object", object.to_canon()),
            ]),
            Self::Adopt { adoptable, object } => Canon::map([
                kind,
                ("adoptable", adoptable.to_canon()),
                ("object", object.to_canon()),
            ]),
            Self::Revoke { targets, reason } => Canon::map([
                kind,
                ("targets", targets.to_canon()),
                ("reason", reason.to_canon()),
            ]),
            Self::Retire { target, reason } => Canon::map([
                kind,
                ("target", target.to_canon()),
                ("reason", reason.to_canon()),
            ]),
            Self::Published { doc, records } => Canon::map([
                kind,
                ("doc", doc.to_canon()),
                (
                    "records",
                    Canon::list(records.iter().map(ToCanon::to_canon)),
                ),
            ]),
            Self::Purge {
                doc,
                object,
                reason,
            } => Canon::map([
                kind,
                ("doc", doc.to_canon()),
                ("object", Canon::Str(object.to_string())),
                ("reason", reason.to_canon()),
            ]),
        }
    }
}

/// What an approval or an adoption pins, as the ledger reads it.
#[derive(Debug)]
pub(super) struct Pinning<'a> {
    pub(super) gate: Gate,
    pub(super) subject: EntityId,
    pub(super) hash: &'a ContentHash,
    pub(super) basis: Basis,
}

/// One decision, as an immutable record. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    id: JudgementId,
    at: Timestamp,
    judge: Judge,
    origin: Visibility,
    kind: JudgementKind,
}

impl Judgement {
    /// A record from its parts, with no check; for the constructors of the module
    /// family that make the check themselves.
    pub(super) fn assemble(
        id: JudgementId,
        at: Timestamp,
        judge: Judge,
        origin: Visibility,
        kind: JudgementKind,
    ) -> Self {
        Self {
            id,
            at,
            judge,
            origin,
            kind,
        }
    }

    /// An approval, from the proof that the gate passed.
    ///
    /// The object must be of the version the approval pins: its `content` is the
    /// attestation's hash.
    pub fn approve(
        id: JudgementId,
        at: Timestamp,
        judge: Judge,
        origin: Visibility,
        passed: Passed,
        object: ObjectKey,
    ) -> Result<Self, JudgementError> {
        let target = *passed.attestation().target_hash();
        if *object.content() != target {
            return Err(JudgementError::ObjectNotOfTarget {
                object: *object.content(),
                target,
            });
        }
        Ok(Self::assemble(
            id,
            at,
            judge,
            origin,
            JudgementKind::Approve { passed, object },
        ))
    }

    /// An adoption, from the proof that the adoption was checked.
    ///
    /// The object must be of the version the adoption pins.
    pub fn adopt(
        id: JudgementId,
        at: Timestamp,
        judge: Judge,
        origin: Visibility,
        adoptable: Adoptable,
        object: ObjectKey,
    ) -> Result<Self, JudgementError> {
        let target = *adoptable.attestation().hash();
        if *object.content() != target {
            return Err(JudgementError::ObjectNotOfTarget {
                object: *object.content(),
                target,
            });
        }
        Ok(Self::assemble(
            id,
            at,
            judge,
            origin,
            JudgementKind::Adopt { adoptable, object },
        ))
    }

    /// A retirement of something whose file is gone.
    ///
    /// `presence` is the fact the application read: a retirement of something whose
    /// file is there is refused.
    pub fn retire(
        id: JudgementId,
        at: Timestamp,
        judge: Judge,
        origin: Visibility,
        target: Target,
        presence: FilePresence,
        reason: NonBlank,
    ) -> Result<Self, JudgementError> {
        match presence {
            FilePresence::Present => Err(JudgementError::TargetPresent { target }),
            FilePresence::Absent => Ok(Self::assemble(
                id,
                at,
                judge,
                origin,
                JudgementKind::Retire { target, reason },
            )),
        }
    }

    /// A deletion of the stored text of a version.
    pub fn purge(
        id: JudgementId,
        at: Timestamp,
        judge: Judge,
        origin: Visibility,
        doc: DocId,
        object: TextHash,
        reason: NonBlank,
    ) -> Self {
        Self::assemble(
            id,
            at,
            judge,
            origin,
            JudgementKind::Purge {
                doc,
                object,
                reason,
            },
        )
    }

    /// The commit point of a publication, written in the public ledger.
    ///
    /// Every record named must be in the public ledger already (a record that is
    /// not sound there, because it is absent, corrupt, or differs from the private
    /// ledger's record of that identifier, is refused), so a commit point never
    /// points at something that was not copied. Every record named must also be an
    /// approval, an adoption or a retirement of `doc` or of one of its items, and not
    /// revoked: a publication of one document never makes a record about another
    /// count, nor one that was taken back. Its origin is the public ledger; there is
    /// no other place for it.
    pub fn published(
        id: JudgementId,
        at: Timestamp,
        judge: Judge,
        doc: DocId,
        records: BTreeSet<JudgementId>,
        ledgers: &Ledgers,
    ) -> Result<Self, JudgementError> {
        for record in &records {
            let held = ledgers
                .public()
                .get(record)
                .filter(|_| !ledgers.corrupt().contains(record))
                .ok_or(JudgementError::RecordNotInPublicLedger { record: *record })?;
            if !ledgers.is_of_doc(held, doc) {
                return Err(JudgementError::RecordNotOfDoc {
                    record: *record,
                    doc,
                });
            }
            if ledgers.is_revoked(record) {
                return Err(JudgementError::RecordRevoked { record: *record });
            }
        }
        Ok(Self::assemble(
            id,
            at,
            judge,
            Visibility::Public,
            JudgementKind::Published { doc, records },
        ))
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// When the record was written. For display: it orders nothing.
    pub fn at(&self) -> &Timestamp {
        &self.at
    }

    /// Who decided.
    pub fn judge(&self) -> &Judge {
        &self.judge
    }

    /// The ledger the record was first written in.
    pub fn origin(&self) -> Visibility {
        self.origin
    }

    /// What was decided.
    pub fn kind(&self) -> &JudgementKind {
        &self.kind
    }

    /// The version an approval or an adoption keeps an object for.
    pub fn object(&self) -> Option<&ObjectKey> {
        match &self.kind {
            JudgementKind::Approve { object, .. } | JudgementKind::Adopt { object, .. } => {
                Some(object)
            }
            JudgementKind::Revoke { .. }
            | JudgementKind::Retire { .. }
            | JudgementKind::Published { .. }
            | JudgementKind::Purge { .. } => None,
        }
    }

    /// The bytes of the record in the canonical encoding.
    ///
    /// Two records of one identifier are one record exactly when these are equal.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.to_canon().to_bytes()
    }
}

impl ToCanon for Judgement {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("id", self.id.to_canon()),
            ("at", self.at.to_canon()),
            ("judge", self.judge.to_canon()),
            (
                "origin",
                Canon::str(match self.origin {
                    Visibility::Public => "public",
                    Visibility::Private => "private",
                }),
            ),
            ("body", self.kind.to_canon()),
        ])
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::approval::{Attestation, CmResult, Pinned};
    use crate::domain::corpus::{
        Corpus, Document, Entity, FileFacts, GitPlace, Header, Location, Reading,
    };
    use crate::domain::event::EventId;
    use crate::domain::hash::BytesHash;
    use crate::domain::id::test_support::{eid, fid, sid};
    use crate::domain::knowable::Knowable;
    use crate::domain::policy::GatePolicy;

    pub(crate) fn jid(n: u128) -> JudgementId {
        JudgementId::new(eid(10_000 + n))
    }

    pub(crate) fn ts() -> Timestamp {
        Timestamp::from_validated("2026-11-05T09:00:00Z".into()).unwrap()
    }

    pub(crate) fn why() -> NonBlank {
        NonBlank::new("because").unwrap()
    }

    pub(crate) fn hash_of(tag: &str) -> ContentHash {
        ContentHash::raw(tag.as_bytes())
    }

    pub(crate) fn object_of(content: &ContentHash) -> ObjectKey {
        ObjectKey::new(*content, TextHash::of(&format!("text of {content}")))
    }

    /// A CR proof for Finding `n` at the hash named `tag`.
    pub(crate) fn cr_passed(n: u128, tag: &str) -> Passed {
        Passed::for_test(Attestation::Cr {
            finding: Pinned::new(fid(n), hash_of(tag)),
        })
    }

    /// A CS proof for Spec `n`, with `contract` and `hyp` named by tags.
    pub(crate) fn cs_passed(n: u128, contract: &str, hyp: &str) -> Passed {
        cs_passed_under(n, contract, hyp, GatePolicy::default())
    }

    /// A CS proof as [`cs_passed`] makes it, passed under `policy`.
    pub(crate) fn cs_passed_under(
        n: u128,
        contract: &str,
        hyp: &str,
        policy: GatePolicy,
    ) -> Passed {
        Passed::for_test(Attestation::Cs {
            spec: Pinned::new(sid(n), hash_of(contract)),
            hyp: hash_of(hyp),
            grounds: NonEmptyVec::new(Pinned::new(fid(1), hash_of("ground")), Vec::new()),
            policy,
        })
    }

    /// A CM proof for Spec `n` at the hypothesis hash named `hyp`, set against the
    /// CS event named `cs`.
    pub(crate) fn cm_passed(n: u128, hyp: &str, cs: &str) -> Passed {
        let cs: EventId = hash_of(cs).to_string().parse().unwrap();
        Passed::for_test(Attestation::Cm {
            spec: Pinned::new(sid(n), hash_of(hyp)),
            cs,
            result: CmResult::Pass,
        })
    }

    /// A CA proof for item `n`.
    pub(crate) fn ca_passed(n: u128, tag: &str) -> Passed {
        use crate::domain::approval::TestOutcome;
        use crate::domain::id::test_support::{did, iid};
        use crate::domain::revision::{Revision, RevisionId};
        Passed::for_test(Attestation::Ca {
            item: Pinned::new(iid(n), hash_of(tag)),
            design: Pinned::new(did(21), hash_of("design")),
            revision: Revision::Commit(RevisionId::from_validated("a1".into()).unwrap()),
            tests: TestOutcome::Passed,
        })
    }

    /// An approval record written in `origin`.
    pub(crate) fn approve(n: u128, judge: &Judge, origin: Visibility, passed: Passed) -> Judgement {
        let object = object_of(passed.attestation().target_hash());
        Judgement::approve(jid(n), ts(), judge.clone(), origin, passed, object).unwrap()
    }

    /// A file of the corpus under construction.
    pub(crate) struct Filed {
        entity: Entity,
        visibility: Visibility,
        path: String,
        git: Knowable<GitPlace>,
    }

    impl Filed {
        fn make(visibility: Visibility, path: Option<&str>, git: GitPlace, entity: Entity) -> Self {
            let path = path.map_or_else(
                || format!("{visibility:?}/{}.md", entity.id()).to_lowercase(),
                str::to_owned,
            );
            Self {
                entity,
                visibility,
                path,
                git: Knowable::Known(git),
            }
        }

        /// A file under the public root.
        pub(crate) fn public(entity: Entity) -> Self {
            Self::make(Visibility::Public, None, GitPlace::Tracked, entity)
        }

        /// A file under the public root, at a path of its own.
        pub(crate) fn public_at(path: &str, entity: Entity) -> Self {
            Self::make(Visibility::Public, Some(path), GitPlace::Tracked, entity)
        }

        /// A file under the private root, in a place git ignores.
        pub(crate) fn private(entity: Entity) -> Self {
            Self::make(Visibility::Private, None, GitPlace::Ignored, entity)
        }

        /// A file under the private root that git would publish.
        pub(crate) fn private_exposed(entity: Entity) -> Self {
            Self::make(Visibility::Private, None, GitPlace::Tracked, entity)
        }

        fn reading(self) -> (FileFacts, Reading) {
            let header = Header::new(
                self.entity.id(),
                self.entity.kind(),
                None,
                BTreeSet::new(),
                None,
            );
            // The bytes of two files of one entity are equal; that is what a copy is.
            let bytes = BytesHash::of(format!("{:?}", self.entity).as_bytes());
            let location = Location::new(self.visibility, self.path.parse().unwrap(), self.git);
            (
                FileFacts::new(location, bytes),
                Reading::Document(Document::new(header, self.entity).unwrap()),
            )
        }
    }

    pub(crate) fn corpus_of(files: Vec<Filed>) -> Corpus {
        corpus_with(files, &BTreeSet::new())
    }

    pub(crate) fn corpus_with(files: Vec<Filed>, published: &BTreeSet<DocId>) -> Corpus {
        Corpus::build(files.into_iter().map(Filed::reading), published)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::id::test_support::{docid, iid};
    use crate::domain::judge::test_support::judge;
    use crate::domain::ledger::Ledger;

    #[test]
    fn an_object_of_another_version_does_not_make_an_approval() {
        let passed = cr_passed(1, "v1");
        let other = object_of(&hash_of("v2"));
        let got = Judgement::approve(
            jid(1),
            ts(),
            judge("ada"),
            Visibility::Public,
            passed,
            other,
        );
        assert_eq!(
            got,
            Err(JudgementError::ObjectNotOfTarget {
                object: hash_of("v2"),
                target: hash_of("v1"),
            })
        );
    }

    #[test]
    fn a_retirement_needs_the_file_to_be_gone() {
        let target = Target::Doc(docid(1));
        let make = |presence| {
            Judgement::retire(
                jid(1),
                ts(),
                judge("ada"),
                Visibility::Public,
                target,
                presence,
                why(),
            )
        };
        assert_eq!(
            make(FilePresence::Present),
            Err(JudgementError::TargetPresent { target })
        );
        assert!(make(FilePresence::Absent).is_ok());
    }

    #[test]
    fn a_publication_names_only_records_the_public_ledger_holds() {
        let held = approve(1, &judge("ada"), Visibility::Private, cr_passed(1, "v1"));
        let ledgers = Ledgers::new(Ledger::from_judgements([held]), Ledger::new());
        let make = |records: &[u128]| {
            Judgement::published(
                jid(9),
                ts(),
                judge("ada"),
                docid(1),
                records.iter().map(|n| jid(*n)).collect(),
                &ledgers,
            )
        };
        assert_eq!(
            make(&[1, 2]),
            Err(JudgementError::RecordNotInPublicLedger { record: jid(2) })
        );
        let made = make(&[1]).unwrap();
        assert_eq!(made.origin(), Visibility::Public);
        assert!(make(&[]).is_ok());
    }

    #[test]
    fn a_record_in_the_private_ledger_only_is_not_one_the_public_ledger_holds() {
        let held = approve(1, &judge("ada"), Visibility::Private, cr_passed(1, "v1"));
        let ledgers = Ledgers::new(Ledger::new(), Ledger::from_judgements([held]));
        assert_eq!(
            Judgement::published(
                jid(9),
                ts(),
                judge("ada"),
                docid(1),
                BTreeSet::from([jid(1)]),
                &ledgers
            ),
            Err(JudgementError::RecordNotInPublicLedger { record: jid(1) })
        );
    }

    #[test]
    fn a_publication_names_only_records_of_its_document_and_its_items() {
        let ada = judge("ada");
        let retire_item = |n: u128, item: u128| {
            Judgement::retire(
                jid(n),
                ts(),
                ada.clone(),
                Visibility::Private,
                Target::Item(iid(item)),
                FilePresence::Absent,
                why(),
            )
            .unwrap()
        };
        let finding = approve(1, &ada, Visibility::Private, cr_passed(1, "v1"));
        // Item 31 of Design 21 (the Design `ca_passed` pins).
        let item = approve(2, &ada, Visibility::Private, ca_passed(31, "i1"));
        let retired_item = retire_item(3, 31);
        let unknown_item = retire_item(4, 32);
        let purge = Judgement::purge(
            jid(5),
            ts(),
            ada.clone(),
            Visibility::Private,
            docid(21),
            TextHash::of("t"),
            why(),
        );
        let ledgers = Ledgers::new(
            Ledger::from_judgements([finding, item, retired_item, unknown_item, purge]),
            Ledger::new(),
        );
        let make = |doc: u128, record: u128| {
            Judgement::published(
                jid(9),
                ts(),
                ada.clone(),
                docid(doc),
                BTreeSet::from([jid(record)]),
                &ledgers,
            )
        };
        assert!(make(1, 1).is_ok(), "the Finding's own approval");
        assert!(make(21, 2).is_ok(), "an item of the Design");
        assert!(make(21, 3).is_ok(), "a retirement of an item of the Design");
        for (doc, record) in [(21, 1), (1, 2), (1, 3), (21, 4), (21, 5)] {
            assert_eq!(
                make(doc, record),
                Err(JudgementError::RecordNotOfDoc {
                    record: jid(record),
                    doc: docid(doc),
                }),
                "publishing {doc} naming {record}"
            );
        }
    }

    #[test]
    fn a_publication_names_no_record_that_is_revoked_or_differs_between_the_ledgers() {
        let ada = judge("ada");
        let held = approve(1, &ada, Visibility::Private, cr_passed(1, "v1"));
        let revoke = Judgement::assemble(
            jid(2),
            ts(),
            ada.clone(),
            Visibility::Private,
            JudgementKind::Revoke {
                targets: NonEmptyVec::new(jid(1), Vec::new()),
                reason: why(),
            },
        );
        let make = |ledgers: &Ledgers| {
            Judgement::published(
                jid(9),
                ts(),
                ada.clone(),
                docid(1),
                BTreeSet::from([jid(1)]),
                ledgers,
            )
        };
        let revoked = Ledgers::new(
            Ledger::from_judgements([held.clone()]),
            Ledger::from_judgements([held.clone(), revoke]),
        );
        assert_eq!(
            make(&revoked),
            Err(JudgementError::RecordRevoked { record: jid(1) })
        );
        let altered = approve(1, &ada, Visibility::Private, cr_passed(1, "v2"));
        let differ = Ledgers::new(
            Ledger::from_judgements([altered]),
            Ledger::from_judgements([held]),
        );
        assert_eq!(
            make(&differ),
            Err(JudgementError::RecordNotInPublicLedger { record: jid(1) })
        );
    }

    #[test]
    fn only_approvals_adoptions_and_retirements_are_revocable() {
        let ada = judge("ada");
        let approve = approve(1, &ada, Visibility::Public, cr_passed(1, "v1"));
        let retire = Judgement::retire(
            jid(2),
            ts(),
            ada.clone(),
            Visibility::Public,
            Target::Item(iid(5)),
            FilePresence::Absent,
            why(),
        )
        .unwrap();
        let purge = Judgement::purge(
            jid(3),
            ts(),
            ada.clone(),
            Visibility::Public,
            docid(1),
            TextHash::of("t"),
            why(),
        );
        let revoke = Judgement::assemble(
            jid(4),
            ts(),
            ada,
            Visibility::Public,
            JudgementKind::Revoke {
                targets: NonEmptyVec::new(jid(1), Vec::new()),
                reason: why(),
            },
        );
        assert!(approve.kind().is_revocable());
        assert!(retire.kind().is_revocable());
        assert!(!purge.kind().is_revocable());
        assert!(!revoke.kind().is_revocable());
    }

    #[test]
    fn the_canonical_bytes_follow_every_field_including_the_time() {
        let ada = judge("ada");
        let a = approve(1, &ada, Visibility::Public, cr_passed(1, "v1"));
        assert_eq!(a.canonical_bytes(), a.clone().canonical_bytes());
        let later = Judgement::approve(
            jid(1),
            Timestamp::from_validated("2026-11-06T09:00:00Z".into()).unwrap(),
            ada.clone(),
            Visibility::Public,
            cr_passed(1, "v1"),
            object_of(&hash_of("v1")),
        )
        .unwrap();
        assert_ne!(a.canonical_bytes(), later.canonical_bytes());
        let private = approve(1, &ada, Visibility::Private, cr_passed(1, "v1"));
        assert_ne!(a.canonical_bytes(), private.canonical_bytes());
        let other = approve(1, &judge("grace"), Visibility::Public, cr_passed(1, "v1"));
        assert_ne!(a.canonical_bytes(), other.canonical_bytes());
    }

    #[test]
    fn a_target_is_named_by_the_kind_of_what_the_gate_approves() {
        let id = eid_of(5);
        assert_eq!(
            Target::of_subject(Gate::Ca, id),
            Target::Item(ItemId::new(id))
        );
        for gate in [Gate::Cr, Gate::Cs, Gate::Ci, Gate::Cm] {
            assert_eq!(Target::of_subject(gate, id), Target::Doc(DocId::new(id)));
        }
    }

    fn eid_of(n: u128) -> EntityId {
        crate::domain::id::test_support::eid(n)
    }
}
