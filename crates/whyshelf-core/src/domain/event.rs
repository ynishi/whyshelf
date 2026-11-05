//! Events: the append-only record of everything that happens to an entity.
//!
//! The event log is the source of truth for history. Content lives in files and
//! may be edited; every other change (registration, revision, reclassification,
//! move, approval, revocation, purge, publication) is an event, written once and
//! never rewritten. A correction is a new event.
//!
//! # Kinds
//!
//! | event | records |
//! |---|---|
//! | `Registered` | an entity comes into being: kind, path, alias, owner, tags and first version; or an item of a Design by number |
//! | `Revised` | the content changed from one version to another (each version is a content hash and a raw hash) |
//! | `Reclassified` | tags added or removed, kind, owner or alias changed; never affects approvals |
//! | `Moved` | the path changed; the identifier did not |
//! | `Approved` | a gate's attestation: what was approved, at which hashes, against which upstream hashes, by whom |
//! | `Revoked` | an earlier `Approved` is withdrawn, with a reason |
//! | `Purged` | the stored object of a raw hash was deleted (the history stays) |
//! | `Published` | an entity moved from the private store to the repo store |
//!
//! # The stream is a DAG
//!
//! The events of one entity form a directed acyclic graph. Each event lists
//! `heads`: the tips of that entity's stream as the writer saw them (a tip is an
//! event that no other event lists in its heads). The first event has no heads. If
//! two branches each add an event and are merged, the stream has two tips; the
//! next write lists both and the stream is one again. Every writing use case lists
//! all current tips, so a fork heals by itself.
//!
//! Event A is an **ancestor** of event B when B reaches A by following heads.
//! Whether one event "saw" another is defined by ancestry, never by time. The time
//! of a record (see [`Recorded`]) is for display and decides nothing.
//!
//! ```text
//!  one stream (all events are about one entity; an arrow is "lists as head")
//!
//!   R  Registered          heads = {}
//!   ^
//!   |
//!   E1 Reclassified        heads = {R}       <- tips seen by E1: {R}
//!   ^  ^
//!   |  +-----------------+     two branches each wrote one event after E1
//!   |                    |
//!   E2 Revised           E3 Reclassified     <- the merge brings both files in;
//!   heads = {E1}         heads = {E1}           tips are now {E2, E3}: a fork
//!   ^                    ^
//!   +----------+---------+
//!              |
//!              J  any next write   heads = {E2, E3}   <- the join: one tip {J}
//!
//!   event id = hash( kind, target, kind-specific content, heads )
//!                  (not the time, not the file format version; the person `by`
//!                   only inside the content of Approved and Revoked)
//! ```
//!
//! Nothing resolves the fork but the next write: it reads every tip, lists them
//! all as its heads and so becomes the single tip. Until then a value that two
//! branches set differently is reported as a conflict (see `classification`).
//!
//! # Identifiers
//!
//! An [`EventId`] is the hash of the kind, the target, the kind-specific content
//! and the heads. It excludes the time and any wire-format version, so two
//! writers who see the same state and find the same change compute the same
//! identifier: they write records of one event (see below). Because the heads are
//! inside, the same change made again after something else happened has a
//! different identifier: `+x`, `-x`, `+x` are three events; revoke and approve
//! again are two approvals; a revision A, B, A, B is four events.
//!
//! The person (`by`) is inside the identifier of `Approved` and `Revoked` and of
//! no other kind. A judgement is a fact about the person who made it: two people
//! who approve the same attestation make two events, and a revocation withdraws
//! the approval of one of them and leaves the other standing. For the other kinds
//! the person is not recorded, and a change found by two writers is one event
//! whoever they are.
//!
//! The identifier of `Registered` also contains the new entity's identifier, so
//! two processes that register the same file independently write two events, not
//! two records of one. The overlapping registrations are folded afterwards (see
//! `classification`).
//!
//! # Records: one event, written more than once
//!
//! An [`Event`] has no time. The same change, found by two writers who saw the
//! same state, is one event that was recorded twice, at two times. A [`Recorded`]
//! is the event with the time of each of its records. A record is told from
//! another of the same event by its [`RecordDigest`], the hash of the event's
//! identifier and the time of that record, so a record written again (same
//! identifier, same time) is the same record, and one written at another time is
//! a further record, never a replacement.
//!
//! A [`Timestamp`] is compared for equality only (RFC 3339 texts of different UTC
//! offsets do not sort as time), so the times of an event are not put in time
//! order and none is called the earliest. They are kept in the order of their
//! digests, which every reader computes alike, and the one shown is the first of
//! them: *a* time of one record of the event.
//!
//! # Crossing stores
//!
//! There are two stores, the repo store (committed with the project) and the
//! private store (outside the repo). `publish` writes `Published { to }` into
//! the private store, as an ordinary event with heads, and `Published { from,
//! snapshot }` into the repo store. The latter is a **root** of the repo store's
//! stream: it has no heads and carries a snapshot of the classification (tags,
//! kind, owner, path, alias) and the latest version, so the repo store can be
//! folded alone and give the same classification as the private store did. A head
//! that points at an event the store does not have is treated as absent, not as
//! an error. Older events and objects of the private store are not copied.
//!
//! The wire form of an event (JSON with a format version) belongs to the storage
//! adapter. The domain's event knows no format version, so migrating the file
//! format cannot change an identifier.
//!
//! # Compatibility
//!
//! The identifier of `Approved` and `Revoked` contains `by`, and a record has a
//! digest, a hash kind of its own. The hash domain tag of events stays `v1`: the
//! formula of those two identifiers changed before any release, so there are no
//! files written under the earlier formula to keep readable. Once a release has
//! been made, a change to the formula of an identifier is a new tag, and every
//! identifier and approval already written is then migrated (see the compatibility
//! table of the wire format in the storage adapter).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use super::actor::{Actor, Timestamp};
use super::approval::Attestation;
use super::canon::{Canon, ToCanon};
use super::error::{EventError, ValueError};
use super::hash::{ContentHash, HashKind};
use super::id::{Alias, DesignId, EntityId, ItemNumber};
use super::location::RelPath;
use super::nonempty::{NonBlank, NonEmptyVec};
use super::tag::{Tag, TagChange};

/// The kind of an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityKind {
    /// A Finding.
    Finding,
    /// A Spec.
    Spec,
    /// A Design.
    Design,
    /// A work item of a Design.
    Item,
}

impl EntityKind {
    /// The lower-case name used in queries and files.
    pub fn name(self) -> &'static str {
        match self {
            Self::Finding => "finding",
            Self::Spec => "spec",
            Self::Design => "design",
            Self::Item => "item",
        }
    }
}

/// The kind of an entity that has its own file.
///
/// An item has no file of its own, so it is not a `DocKind`. This is the value
/// the classification meta `kind` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocKind {
    /// A Finding file.
    Finding,
    /// A Spec file.
    Spec,
    /// A Design file.
    Design,
}

impl DocKind {
    /// The same kind as an [`EntityKind`].
    pub fn entity_kind(self) -> EntityKind {
        match self {
            Self::Finding => EntityKind::Finding,
            Self::Spec => EntityKind::Spec,
            Self::Design => EntityKind::Design,
        }
    }
}

impl DocKind {
    /// The kind named `finding`, `spec` or `design`; `None` for any other text.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "finding" => Some(Self::Finding),
            "spec" => Some(Self::Spec),
            "design" => Some(Self::Design),
            _ => None,
        }
    }
}

impl fmt::Display for DocKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.entity_kind().name())
    }
}

impl ToCanon for DocKind {
    fn to_canon(&self) -> Canon {
        Canon::str(self.entity_kind().name())
    }
}

/// Which store an event lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoreKind {
    /// The store committed with the repository.
    Repo,
    /// The store outside the repository, never published.
    Private,
}

impl StoreKind {
    /// The name a person reads and types: `repo` or `private`.
    ///
    /// This is the name of the display and of the input (settings, the command
    /// line, the tool server). The hash (the canonical form) and the wire format
    /// keep their own literals, so renaming a display cannot change an identifier
    /// or break a stored event.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Private => "private",
        }
    }

    /// Whether a document in this store can see an entity in `target`: the repo
    /// store cannot see the private store, and the private store sees both.
    ///
    /// The one statement of which store sees which; the index of references, the
    /// map and the scaffold ask this and keep no copy.
    pub fn sees(self, target: StoreKind) -> bool {
        !matches!((self, target), (Self::Repo, Self::Private))
    }

    /// The mark a reference from a document in this store to an entity in `target`
    /// carries: the repo store reaches the private store only by the opaque
    /// `private:` identifier, and everything else is written plain.
    pub fn mark_for(self, target: StoreKind) -> RefMark {
        if self.sees(target) {
            RefMark::Unmarked
        } else {
            RefMark::Private
        }
    }
}

/// The mark of a reference to an entity of the project (see
/// [`StoreKind::mark_for`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefMark {
    /// Written by alias or identifier, no mark.
    Unmarked,
    /// Written `private:<id>`, by the opaque identifier alone.
    Private,
}

impl FromStr for StoreKind {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "repo" => Ok(Self::Repo),
            "private" => Ok(Self::Private),
            _ => Err(ValueError::StoreKind),
        }
    }
}

impl ToCanon for StoreKind {
    fn to_canon(&self) -> Canon {
        // The hash has its own literals: see `as_str`.
        Canon::str(match self {
            Self::Repo => "repo",
            Self::Private => "private",
        })
    }
}

/// The identifier of an event: a hash, see the module documentation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(ContentHash);

impl EventId {
    /// The hash inside.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EventId({})", self.0)
    }
}

impl FromStr for EventId {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl ToCanon for EventId {
    fn to_canon(&self) -> Canon {
        self.0.to_canon()
    }
}

/// One version of an entity's text.
///
/// A file that parses has a `content` hash (what approvals use) and a `raw` hash
/// (the key of the stored object). A file that does not parse has only the raw
/// hash. Editing only a supplementary section changes `raw` and leaves `content`
/// alone: a `Revised` is still written, so the history keeps the version, and
/// approvals are unaffected because they use `content`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Version {
    /// A version that parsed.
    Parsed {
        /// The hash approvals refer to.
        content: ContentHash,
        /// The hash of the normalized text; the key of the stored object.
        raw: ContentHash,
    },
    /// A version that did not parse.
    Unparsed {
        /// The hash of the normalized text; the key of the stored object.
        raw: ContentHash,
    },
}

/// What identifies a version for the purpose of folding duplicate registrations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VersionKey {
    /// The content hash of a version that parsed.
    Content(ContentHash),
    /// The raw hash of a version that did not parse.
    Raw(ContentHash),
}

impl Version {
    /// The raw hash.
    pub fn raw(&self) -> &ContentHash {
        match self {
            Self::Parsed { raw, .. } | Self::Unparsed { raw } => raw,
        }
    }

    /// The content hash, if the version parsed.
    pub fn content(&self) -> Option<&ContentHash> {
        match self {
            Self::Parsed { content, .. } => Some(content),
            Self::Unparsed { .. } => None,
        }
    }

    /// The key two versions are equal by: content when it parsed, raw if not.
    pub fn key(&self) -> VersionKey {
        match self {
            Self::Parsed { content, .. } => VersionKey::Content(*content),
            Self::Unparsed { raw } => VersionKey::Raw(*raw),
        }
    }
}

impl ToCanon for VersionKey {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Content(h) => Canon::map([("content", h.to_canon())]),
            Self::Raw(h) => Canon::map([("raw", h.to_canon())]),
        }
    }
}

impl ToCanon for Version {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Parsed { content, raw } => {
                Canon::map([("content", content.to_canon()), ("raw", raw.to_canon())])
            }
            Self::Unparsed { raw } => Canon::map([("raw", raw.to_canon())]),
        }
    }
}

/// The registration of an entity that has a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRegistration {
    kind: DocKind,
    path: RelPath,
    alias: Option<Alias>,
    owner: Option<Actor>,
    tags: BTreeSet<Tag>,
    first: Version,
}

impl DocRegistration {
    /// A registration. Alias, owner and tags are the classification the
    /// entity starts with.
    pub fn new(
        kind: DocKind,
        path: RelPath,
        alias: Option<Alias>,
        owner: Option<Actor>,
        tags: BTreeSet<Tag>,
        first: Version,
    ) -> Self {
        Self {
            kind,
            path,
            alias,
            owner,
            tags,
            first,
        }
    }

    /// The kind.
    pub fn kind(&self) -> DocKind {
        self.kind
    }

    /// The path at registration.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// The alias it starts with, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The owner it starts with, if any.
    pub fn owner(&self) -> Option<&Actor> {
        self.owner.as_ref()
    }

    /// The tags it starts with.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The first version.
    pub fn first(&self) -> &Version {
        &self.first
    }
}

/// What a `Registered` event registers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registration {
    /// A Finding, Spec or Design file.
    Doc(DocRegistration),
    /// A work item, identified by its Design and number. The item's identifier
    /// is the event's target.
    Item {
        /// The Design the item belongs to.
        design: DesignId,
        /// The number inside that Design.
        number: ItemNumber,
    },
}

/// One edit of an entity's classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassChange {
    /// Add or remove a tag.
    Tag(TagChange),
    /// Set the kind.
    SetKind(DocKind),
    /// Set the owner.
    SetOwner(Actor),
    /// Remove the owner.
    ClearOwner,
    /// Set the alias.
    SetAlias(Alias),
    /// Remove the alias.
    ClearAlias,
}

impl ToCanon for ClassChange {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Tag(change) => Canon::map([("tag", change.to_canon())]),
            Self::SetKind(kind) => Canon::map([("kind", kind.to_canon())]),
            Self::SetOwner(owner) => Canon::map([("owner", owner.to_canon())]),
            Self::ClearOwner => Canon::map([("owner", Canon::Null)]),
            Self::SetAlias(alias) => Canon::map([("alias", alias.to_canon())]),
            Self::ClearAlias => Canon::map([("alias", Canon::Null)]),
        }
    }
}

/// The classification and latest version of an entity at the moment it was
/// published, carried by the root event of the repo store's stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSnapshot {
    kind: DocKind,
    path: RelPath,
    alias: Option<Alias>,
    owner: Option<Actor>,
    tags: BTreeSet<Tag>,
    version: Version,
}

impl ClassSnapshot {
    /// A snapshot of the values the fold gave in the source store.
    pub fn new(
        kind: DocKind,
        path: RelPath,
        alias: Option<Alias>,
        owner: Option<Actor>,
        tags: BTreeSet<Tag>,
        version: Version,
    ) -> Self {
        Self {
            kind,
            path,
            alias,
            owner,
            tags,
            version,
        }
    }

    /// The kind.
    pub fn kind(&self) -> DocKind {
        self.kind
    }

    /// The path.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// The alias, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The owner, if any.
    pub fn owner(&self) -> Option<&Actor> {
        self.owner.as_ref()
    }

    /// The tags. The mark of each tag is the publishing event itself.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The latest version.
    pub fn version(&self) -> &Version {
        &self.version
    }
}

impl ToCanon for ClassSnapshot {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("kind", self.kind.to_canon()),
            ("path", self.path.to_canon()),
            ("alias", self.alias.to_canon()),
            ("owner", self.owner.to_canon()),
            ("tags", Canon::list(self.tags.iter().map(ToCanon::to_canon))),
            ("version", self.version.to_canon()),
        ])
    }
}

/// The two halves of a publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publication {
    /// Written in the source store, as an ordinary event with heads.
    Outgoing {
        /// The store the entity moves to.
        to: StoreKind,
    },
    /// Written in the destination store as the root of its stream. Has no heads.
    Incoming {
        /// The store the entity came from.
        from: StoreKind,
        /// The classification and version to start from.
        snapshot: ClassSnapshot,
    },
}

/// What an event records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventBody {
    /// An entity comes into being.
    Registered(Registration),
    /// The content changed.
    Revised {
        /// The version before.
        old: Version,
        /// The version after.
        new: Version,
    },
    /// The classification changed.
    Reclassified {
        /// The edits, applied in order.
        changes: NonEmptyVec<ClassChange>,
    },
    /// The path changed.
    Moved {
        /// The path before.
        old: RelPath,
        /// The path after.
        new: RelPath,
    },
    /// A gate approved something.
    Approved {
        /// Who approved, as declared.
        by: Actor,
        /// What was approved.
        attestation: Attestation,
    },
    /// An earlier approval is withdrawn.
    Revoked {
        /// The `Approved` event withdrawn.
        revokes: EventId,
        /// Why, free text.
        reason: NonBlank,
        /// Who withdrew it, as declared.
        by: Actor,
    },
    /// A stored object was deleted.
    Purged {
        /// The raw hash whose object was deleted.
        raw: ContentHash,
        /// Why, free text.
        reason: NonBlank,
    },
    /// An entity crossed from one store to the other.
    Published(Publication),
}

impl EventBody {
    /// The name of the kind of event, as written in files.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Registered(_) => "Registered",
            Self::Revised { .. } => "Revised",
            Self::Reclassified { .. } => "Reclassified",
            Self::Moved { .. } => "Moved",
            Self::Approved { .. } => "Approved",
            Self::Revoked { .. } => "Revoked",
            Self::Purged { .. } => "Purged",
            Self::Published(_) => "Published",
        }
    }

    /// The part of the body that enters the event identifier: everything but, for
    /// a registration, the starting classification. The person (`by`) is in it for
    /// `Approved` and `Revoked`, the judgements of a person, and for no other kind
    /// (no other has one).
    fn id_content(&self) -> Canon {
        match self {
            Self::Registered(Registration::Doc(doc)) => Canon::map([
                ("entity", doc.kind.to_canon()),
                ("path", doc.path.to_canon()),
                ("first", doc.first.key().to_canon()),
            ]),
            Self::Registered(Registration::Item { design, number }) => Canon::map([
                ("entity", Canon::str("item")),
                ("design", design.to_canon()),
                ("number", number.to_canon()),
            ]),
            Self::Revised { old, new } => {
                Canon::map([("old", old.to_canon()), ("new", new.to_canon())])
            }
            Self::Reclassified { changes } => Canon::map([("changes", changes.to_canon())]),
            Self::Moved { old, new } => {
                Canon::map([("old", old.to_canon()), ("new", new.to_canon())])
            }
            Self::Approved { by, attestation } => Canon::map([
                ("by", by.to_canon()),
                ("attestation", attestation.to_canon()),
            ]),
            Self::Revoked {
                revokes,
                reason,
                by,
            } => Canon::map([
                ("revokes", revokes.to_canon()),
                ("reason", reason.to_canon()),
                ("by", by.to_canon()),
            ]),
            Self::Purged { raw, reason } => {
                Canon::map([("raw", raw.to_canon()), ("reason", reason.to_canon())])
            }
            Self::Published(Publication::Outgoing { to }) => Canon::map([("to", to.to_canon())]),
            Self::Published(Publication::Incoming { from, snapshot }) => {
                Canon::map([("from", from.to_canon()), ("snapshot", snapshot.to_canon())])
            }
        }
    }
}

/// What a writer may record: every [`EventBody`] but `Approved`.
///
/// The reading side (the storage adapter that rebuilds events from files) needs
/// `Approved` and keeps using [`EventBody`]. The writing side does not: an approval
/// is built from a [`Passed`](super::gate::Passed) and from nothing else, so the
/// type a use case hands to the draft has no variant for it, and no use case can
/// record an approval that a gate did not let through.
///
/// ```compile_fail
/// use whyshelf_core::domain::event::RecordBody;
///
/// // There is no such variant: an approval is recorded from a gate's `Passed`.
/// fn forged(by: whyshelf_core::domain::actor::Actor, attestation: whyshelf_core::domain::approval::Attestation) -> RecordBody {
///     RecordBody::Approved { by, attestation }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordBody {
    /// An entity comes into being.
    Registered(Registration),
    /// The content changed.
    Revised {
        /// The version before.
        old: Version,
        /// The version after.
        new: Version,
    },
    /// The classification changed.
    Reclassified {
        /// The edits, applied in order.
        changes: NonEmptyVec<ClassChange>,
    },
    /// The path changed.
    Moved {
        /// The path before.
        old: RelPath,
        /// The path after.
        new: RelPath,
    },
    /// An earlier approval is withdrawn.
    Revoked {
        /// The `Approved` event withdrawn.
        revokes: EventId,
        /// Why, free text.
        reason: NonBlank,
        /// Who withdrew it, as declared.
        by: Actor,
    },
    /// A stored object was deleted.
    Purged {
        /// The raw hash whose object was deleted.
        raw: ContentHash,
        /// Why, free text.
        reason: NonBlank,
    },
    /// An entity crossed from one store to the other.
    Published(Publication),
}

impl From<RecordBody> for EventBody {
    fn from(body: RecordBody) -> Self {
        match body {
            RecordBody::Registered(registration) => Self::Registered(registration),
            RecordBody::Revised { old, new } => Self::Revised { old, new },
            RecordBody::Reclassified { changes } => Self::Reclassified { changes },
            RecordBody::Moved { old, new } => Self::Moved { old, new },
            RecordBody::Revoked {
                revokes,
                reason,
                by,
            } => Self::Revoked {
                revokes,
                reason,
                by,
            },
            RecordBody::Purged { raw, reason } => Self::Purged { raw, reason },
            RecordBody::Published(publication) => Self::Published(publication),
        }
    }
}

/// One event of one entity's stream.
///
/// It has no time: the time belongs to a record of the event, see [`Recorded`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    id: EventId,
    target: EntityId,
    heads: BTreeSet<EventId>,
    body: EventBody,
}

impl Event {
    /// Build an event and compute its identifier.
    ///
    /// `heads` are all the tips of the target's stream that the caller saw.
    /// Refused: an approval whose attestation is about another entity than
    /// `target`, and a snapshot publication (a root) that has heads.
    pub fn new(
        target: EntityId,
        heads: BTreeSet<EventId>,
        body: EventBody,
    ) -> Result<Self, EventError> {
        if let EventBody::Approved { attestation, .. } = &body {
            let subject = attestation.subject();
            if subject != target {
                return Err(EventError::TargetMismatch { target, subject });
            }
        }
        if matches!(body, EventBody::Published(Publication::Incoming { .. })) && !heads.is_empty() {
            return Err(EventError::SnapshotHasHeads);
        }
        let id = EventId(ContentHash::of(
            HashKind::Event,
            &Canon::map([
                ("event", Canon::str(body.kind_name())),
                ("target", target.to_canon()),
                ("content", body.id_content()),
                ("heads", Canon::list(heads.iter().map(ToCanon::to_canon))),
            ]),
        ));
        Ok(Self {
            id,
            target,
            heads,
            body,
        })
    }

    /// The identifier.
    pub fn id(&self) -> EventId {
        self.id
    }

    /// The entity the event is about.
    pub fn target(&self) -> EntityId {
        self.target
    }

    /// The tips of the stream the writer saw.
    pub fn heads(&self) -> &BTreeSet<EventId> {
        &self.heads
    }

    /// What the event records.
    pub fn body(&self) -> &EventBody {
        &self.body
    }
}

/// The digest of one record of an event: 16 lowercase hex characters, the first
/// 64 bits of the hash ([`HashKind::EventRecord`]) of the event's identifier and
/// the time of the record.
///
/// It is what tells the records of one event apart, and the order of the times of
/// an event. It is computed from domain values, not from the bytes of a file, so
/// that whether two records are the same is decided here and not by how a file
/// happens to be written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordDigest(String);

/// The number of hex characters of a [`RecordDigest`].
const RECORD_DIGEST_LEN: usize = 16;

impl RecordDigest {
    /// The digest of the record of `event` at `at`.
    pub fn of(event: &EventId, at: &Timestamp) -> Self {
        let hash = ContentHash::of(
            HashKind::EventRecord,
            &Canon::map([("event", event.to_canon()), ("at", at.to_canon())]),
        );
        Self(hash.hex().chars().take(RECORD_DIGEST_LEN).collect())
    }

    /// The 16 hex characters.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RecordDigest {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let shaped = s.len() == RECORD_DIGEST_LEN
            && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if shaped {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::RecordDigestShape)
        }
    }
}

/// The times of an event: those of `first` and of `more`, each record once, in the
/// order of their digests.
fn ordered_times(
    event: &EventId,
    first: &Timestamp,
    more: impl IntoIterator<Item = Timestamp>,
) -> NonEmptyVec<Timestamp> {
    let mut by_digest: BTreeMap<RecordDigest, Timestamp> = BTreeMap::new();
    for at in std::iter::once(first.clone()).chain(more) {
        by_digest.entry(RecordDigest::of(event, &at)).or_insert(at);
    }
    let mut times = by_digest.into_values();
    // `first` is in the map, so the list is never empty; the head is taken from it.
    match times.next() {
        Some(head) => NonEmptyVec::new(head, times.collect()),
        None => NonEmptyVec::new(first.clone(), Vec::new()),
    }
}

/// The times of two sets of records of one event, each record once, in the order of
/// their digests.
///
/// For a reader that has the event in two stores: it is one event, with the records
/// of both.
pub fn union_times(
    event: &EventId,
    a: &NonEmptyVec<Timestamp>,
    b: &NonEmptyVec<Timestamp>,
) -> NonEmptyVec<Timestamp> {
    ordered_times(event, a.first(), a.iter().skip(1).chain(b.iter()).cloned())
}

/// An event with the time of each of its records.
///
/// Reading a store gives these: the writers who recorded the same change at
/// different times left a record each, and the event is read once with all their
/// times. The times are in the order of the digests of their records, not in
/// time order (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    event: Event,
    at: NonEmptyVec<Timestamp>,
}

impl Recorded {
    /// An event with the time of its one record.
    pub fn new(event: Event, at: Timestamp) -> Self {
        Self {
            event,
            at: NonEmptyVec::new(at, Vec::new()),
        }
    }

    /// The event.
    pub fn event(&self) -> &Event {
        &self.event
    }

    /// The event, without its times.
    pub fn into_event(self) -> Event {
        self.event
    }

    /// The time of each record of the event, in the order of the digests of the
    /// records.
    pub fn at(&self) -> &NonEmptyVec<Timestamp> {
        &self.at
    }

    /// The time to show: the time of one record of the event, the first in the order
    /// of the digests. It is not the earliest, and decides nothing.
    pub fn shown_at(&self) -> &Timestamp {
        self.at.first()
    }

    /// Add the records of `other`, which must be records of the same event.
    ///
    /// A record that is already there (same time) stays one. Refused when the
    /// other is not the same event, whole: the identifier does not cover everything
    /// (a registration's starting classification is outside it), so two records that
    /// agree on the identifier and disagree on the rest are not records of one event.
    pub fn merge(&mut self, other: Recorded) -> Result<(), EventError> {
        if self.event != other.event {
            return Err(EventError::RecordsDiffer {
                id: self.event.id(),
            });
        }
        self.at = union_times(&self.event.id(), &self.at, &other.at);
        Ok(())
    }
}

/// The ancestry relation of a set of events.
///
/// Built once from a slice of events, it answers "is A an ancestor of B" for the
/// whole set. A head that is not in the set is skipped: a store may lack an
/// ancestor (after a publication, or after a partial merge) and that is not an
/// error. Duplicate events with one identifier count once.
pub(crate) struct EventGraph<'a> {
    by_id: BTreeMap<EventId, &'a Event>,
    ancestors: BTreeMap<EventId, BTreeSet<EventId>>,
}

impl<'a> EventGraph<'a> {
    pub(crate) fn new(events: &'a [Event]) -> Self {
        let mut by_id: BTreeMap<EventId, &Event> = BTreeMap::new();
        for event in events {
            by_id.entry(event.id).or_insert(event);
        }
        let mut ancestors: BTreeMap<EventId, BTreeSet<EventId>> = BTreeMap::new();
        for start in by_id.keys().copied().collect::<Vec<_>>() {
            if ancestors.contains_key(&start) {
                continue;
            }
            let mut stack = vec![start];
            let mut on_stack: BTreeSet<EventId> = BTreeSet::from([start]);
            while let Some(&top) = stack.last() {
                let pending: Vec<EventId> = by_id[&top]
                    .heads
                    .iter()
                    .copied()
                    .filter(|h| {
                        by_id.contains_key(h) && !ancestors.contains_key(h) && !on_stack.contains(h)
                    })
                    .collect();
                if pending.is_empty() {
                    let mut set = BTreeSet::new();
                    for head in &by_id[&top].heads {
                        if by_id.contains_key(head) && *head != top {
                            set.insert(*head);
                            if let Some(inherited) = ancestors.get(head) {
                                set.extend(inherited.iter().copied());
                            }
                        }
                    }
                    ancestors.insert(top, set);
                    on_stack.remove(&top);
                    stack.pop();
                } else {
                    // One at a time, so the stack is the current path of a
                    // depth-first walk and a head found on it is a real cycle.
                    for head in pending.into_iter().take(1) {
                        on_stack.insert(head);
                        stack.push(head);
                    }
                }
            }
        }
        Self { by_id, ancestors }
    }

    /// Every event once, in identifier order.
    pub(crate) fn events(&self) -> impl Iterator<Item = &'a Event> + '_ {
        self.by_id.values().copied()
    }

    /// Whether `ancestor` is a strict ancestor of `of`.
    pub(crate) fn is_ancestor(&self, ancestor: &EventId, of: &EventId) -> bool {
        self.ancestors
            .get(of)
            .is_some_and(|set| set.contains(ancestor))
    }

    /// The strict ancestors of an event.
    #[cfg(test)]
    pub(crate) fn ancestors_of(&self, id: &EventId) -> Option<&BTreeSet<EventId>> {
        self.ancestors.get(id)
    }

    /// The members of `ids` that are not an ancestor of another member.
    pub(crate) fn tips_of(&self, ids: &BTreeSet<EventId>) -> BTreeSet<EventId> {
        ids.iter()
            .filter(|candidate| !ids.iter().any(|other| self.is_ancestor(candidate, other)))
            .copied()
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::id::test_support::eid;

    pub(crate) fn path(p: &str) -> RelPath {
        p.parse().unwrap()
    }

    pub(crate) fn tag(t: &str) -> Tag {
        t.parse().unwrap()
    }

    pub(crate) fn version(n: u8) -> Version {
        Version::Parsed {
            content: ContentHash::of_bytes(HashKind::Finding, &[n]),
            raw: ContentHash::raw(&[n]),
        }
    }

    pub(crate) fn heads(ids: &[EventId]) -> BTreeSet<EventId> {
        ids.iter().copied().collect()
    }

    pub(crate) fn register_doc(
        entity: u128,
        kind: DocKind,
        p: &str,
        alias: Option<&str>,
        v: u8,
    ) -> Event {
        Event::new(
            eid(entity),
            BTreeSet::new(),
            EventBody::Registered(Registration::Doc(DocRegistration::new(
                kind,
                path(p),
                alias.map(|a| a.parse().unwrap()),
                None,
                BTreeSet::new(),
                version(v),
            ))),
        )
        .unwrap()
    }

    pub(crate) fn reclassify(entity: u128, hs: &[EventId], changes: Vec<ClassChange>) -> Event {
        Event::new(
            eid(entity),
            heads(hs),
            EventBody::Reclassified {
                changes: NonEmptyVec::from_vec(changes).unwrap(),
            },
        )
        .unwrap()
    }

    pub(crate) fn add_tag(entity: u128, hs: &[EventId], t: &str) -> Event {
        reclassify(entity, hs, vec![ClassChange::Tag(TagChange::Add(tag(t)))])
    }

    pub(crate) fn remove_tag(entity: u128, hs: &[EventId], t: &str) -> Event {
        reclassify(
            entity,
            hs,
            vec![ClassChange::Tag(TagChange::Remove(tag(t)))],
        )
    }

    pub(crate) fn moved(entity: u128, hs: &[EventId], old: &str, new: &str) -> Event {
        Event::new(
            eid(entity),
            heads(hs),
            EventBody::Moved {
                old: path(old),
                new: path(new),
            },
        )
        .unwrap()
    }

    pub(crate) fn revised(entity: u128, hs: &[EventId], old: u8, new: u8) -> Event {
        Event::new(
            eid(entity),
            heads(hs),
            EventBody::Revised {
                old: version(old),
                new: version(new),
            },
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::id::test_support::{did, eid};

    #[test]
    fn a_registration_identifier_excludes_the_starting_classification() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let b = Event::new(
            eid(1),
            BTreeSet::new(),
            EventBody::Registered(Registration::Doc(DocRegistration::new(
                DocKind::Spec,
                path("docs/spec/a.md"),
                Some("spec/a".parse().unwrap()),
                Some(Actor::new("alice").unwrap()),
                BTreeSet::from([tag("area:auth")]),
                version(1),
            ))),
        )
        .unwrap();
        // Registration formula: kind, entity id, path, first content hash.
        assert_eq!(a.id(), b.id());
    }

    #[test]
    fn registered_identifier_contains_the_entity_identifier() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let b = register_doc(2, DocKind::Spec, "docs/spec/a.md", None, 1);
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn registered_identifier_follows_kind_path_and_first_content() {
        let base = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        assert_ne!(
            base.id(),
            register_doc(1, DocKind::Design, "docs/spec/a.md", None, 1).id()
        );
        assert_ne!(
            base.id(),
            register_doc(1, DocKind::Spec, "docs/spec/b.md", None, 1).id()
        );
        assert_ne!(
            base.id(),
            register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 2).id()
        );
    }

    #[test]
    fn item_registration_formula_is_entity_design_and_number() {
        let item = |entity: u128, design: u128, n: u32| {
            Event::new(
                eid(entity),
                BTreeSet::new(),
                EventBody::Registered(Registration::Item {
                    design: did(design),
                    number: ItemNumber::new(n).unwrap(),
                }),
            )
            .unwrap()
            .id()
        };
        assert_eq!(item(1, 10, 1), item(1, 10, 1));
        assert_ne!(item(1, 10, 1), item(2, 10, 1));
        assert_ne!(item(1, 10, 1), item(1, 11, 1));
        assert_ne!(item(1, 10, 1), item(1, 10, 2));
    }

    #[test]
    fn the_same_change_seen_from_the_same_heads_is_one_event() {
        let root = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let from_cli = add_tag(1, &[root.id()], "wip");
        let from_mcp = add_tag(1, &[root.id()], "wip");
        assert_eq!(from_cli.id(), from_mcp.id());
    }

    #[test]
    fn tag_plus_minus_plus_are_three_distinct_events() {
        let root = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let plus1 = add_tag(1, &[root.id()], "x");
        let minus = remove_tag(1, &[plus1.id()], "x");
        let plus2 = add_tag(1, &[minus.id()], "x");
        let ids = BTreeSet::from([plus1.id(), minus.id(), plus2.id()]);
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn revision_a_b_a_b_gives_four_distinct_events() {
        let root = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let ab = revised(1, &[root.id()], 1, 2);
        let ba = revised(1, &[ab.id()], 2, 1);
        let ab2 = revised(1, &[ba.id()], 1, 2);
        let ids = BTreeSet::from([ab.id(), ba.id(), ab2.id()]);
        assert_eq!(ids.len(), 3);
        assert_ne!(ab.id(), ab2.id());
    }

    #[test]
    fn a_snapshot_publication_is_a_root() {
        let snapshot = ClassSnapshot::new(
            DocKind::Spec,
            path("docs/spec/a.md"),
            None,
            None,
            BTreeSet::new(),
            version(1),
        );
        let body = || {
            EventBody::Published(Publication::Incoming {
                from: StoreKind::Private,
                snapshot: snapshot.clone(),
            })
        };
        let some_head = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1).id();
        assert_eq!(
            Event::new(eid(1), heads(&[some_head]), body()),
            Err(EventError::SnapshotHasHeads)
        );
        assert!(Event::new(eid(1), BTreeSet::new(), body()).is_ok());
    }

    #[test]
    fn graph_ancestry_follows_heads_transitively() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let b = add_tag(1, &[a.id()], "x");
        let c = add_tag(1, &[b.id()], "y");
        let d = add_tag(1, &[a.id()], "z");
        let events = [a.clone(), b.clone(), c.clone(), d.clone()];
        let g = EventGraph::new(&events);
        assert!(g.is_ancestor(&a.id(), &c.id()));
        assert!(g.is_ancestor(&b.id(), &c.id()));
        assert!(!g.is_ancestor(&c.id(), &a.id()));
        assert!(!g.is_ancestor(&b.id(), &d.id()));
        assert!(!g.is_ancestor(&c.id(), &c.id()), "ancestry is strict");
        let all: BTreeSet<_> = events.iter().map(Event::id).collect();
        assert_eq!(g.tips_of(&all), BTreeSet::from([c.id(), d.id()]));
    }

    #[test]
    fn a_missing_ancestor_is_absent_not_an_error() {
        let ghost = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1).id();
        let orphan = add_tag(1, &[ghost], "x");
        let events = [orphan.clone()];
        let g = EventGraph::new(&events);
        assert_eq!(g.ancestors_of(&orphan.id()), Some(&BTreeSet::new()));
        assert!(!g.is_ancestor(&ghost, &orphan.id()));
    }

    #[test]
    fn duplicate_events_count_once() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let events = [a.clone(), a];
        assert_eq!(EventGraph::new(&events).events().count(), 1);
    }

    #[test]
    fn version_key_prefers_content_over_raw() {
        let content = ContentHash::of_bytes(HashKind::Finding, b"c");
        let raw = ContentHash::raw(b"r");
        let parsed = Version::Parsed { content, raw };
        let other_raw = Version::Parsed {
            content,
            raw: ContentHash::raw(b"r2"),
        };
        assert_eq!(parsed.key(), other_raw.key());
        assert_ne!(parsed.key(), Version::Unparsed { raw }.key());
        assert_eq!(parsed.content(), Some(&content));
        assert_eq!(Version::Unparsed { raw }.content(), None);
    }

    #[test]
    fn a_store_is_named_for_display_and_input_and_read_back() {
        for (store, name) in [(StoreKind::Repo, "repo"), (StoreKind::Private, "private")] {
            assert_eq!(store.as_str(), name);
            assert_eq!(name.parse::<StoreKind>(), Ok(store));
        }
        for bad in ["", "Repo", "public", " private"] {
            assert_eq!(bad.parse::<StoreKind>(), Err(ValueError::StoreKind));
        }
    }

    /// Which store sees which, and the mark a reference between them carries: the
    /// one table every reader of it asks.
    #[test]
    fn what_a_store_sees_and_the_mark_a_reference_carries() {
        use StoreKind::{Private, Repo};
        for (from, to, sees, mark) in [
            (Repo, Repo, true, RefMark::Unmarked),
            (Repo, Private, false, RefMark::Private),
            (Private, Repo, true, RefMark::Unmarked),
            (Private, Private, true, RefMark::Unmarked),
        ] {
            assert_eq!(from.sees(to), sees, "{from:?} sees {to:?}");
            assert_eq!(from.mark_for(to), mark, "{from:?} to {to:?}");
        }
    }

    /// A body that can be recorded becomes the body of an event with the same
    /// content, and the event it makes is the one that the same `EventBody` makes.
    #[test]
    fn a_recorded_body_becomes_the_event_body_of_the_same_content() {
        let moved = RecordBody::Moved {
            old: path("docs/spec/a.md"),
            new: path("docs/spec/b.md"),
        };
        assert_eq!(
            EventBody::from(moved),
            EventBody::Moved {
                old: path("docs/spec/a.md"),
                new: path("docs/spec/b.md"),
            }
        );
        let purged = RecordBody::Purged {
            raw: ContentHash::raw(b"x"),
            reason: NonBlank::new("a secret").unwrap(),
        };
        assert_eq!(EventBody::from(purged).kind_name(), "Purged");
    }

    // Golden vector: the canonical bytes of a store, which the event hash covers.
    // They are written out here, not derived from `as_str`, so that renaming the
    // display cannot move an identifier.
    #[test]
    fn golden_store_kind_canon() {
        assert_eq!(
            StoreKind::Repo.to_canon().to_bytes(),
            [&[0x03, 0, 0, 0, 0, 0, 0, 0, 4][..], b"repo"].concat()
        );
        assert_eq!(
            StoreKind::Private.to_canon().to_bytes(),
            [&[0x03, 0, 0, 0, 0, 0, 0, 0, 7][..], b"private"].concat()
        );
    }

    // Golden vector: fixed event, fixed identifier.
    #[test]
    fn golden_event_id() {
        let root = register_doc(1, DocKind::Spec, "docs/spec/cache-timeout.md", None, 1);
        assert_eq!(
            root.id().to_string(),
            "sha256:53449694df6a1cb247050ff1db465d288e8496be16ac63038a4e59c4279a7f68"
        );
    }

    fn at(text: &str) -> Timestamp {
        Timestamp::from_validated(text.to_owned()).unwrap()
    }

    // Golden vector: fixed identifier and time, fixed digest. The digest names a file
    // of a store, so if this fails a stored record would no longer be found by name.
    #[test]
    fn golden_record_digest() {
        let root = register_doc(1, DocKind::Spec, "docs/spec/cache-timeout.md", None, 1);
        let digest = RecordDigest::of(&root.id(), &at("2026-11-04T21:40:00+09:00"));
        assert_eq!(digest.as_str(), "28ddceaf478b5198");
        assert_eq!(
            digest.to_string().parse::<RecordDigest>(),
            Ok(digest.clone())
        );

        // The same digest from the bytes written out by hand: a map of two entries,
        // `at` then `event` (keys in order), each a string; under the tag of the kind.
        let text = |s: &str| [&[0x03][..], &(s.len() as u64).to_be_bytes(), s.as_bytes()].concat();
        let canon = [
            &[0x05][..],
            &2u64.to_be_bytes(),
            &text("at"),
            &text("2026-11-04T21:40:00+09:00"),
            &text("event"),
            &text(&root.id().to_string()),
        ]
        .concat();
        let by_hand = ContentHash::of_bytes(HashKind::EventRecord, &canon);
        assert_eq!(by_hand.hex()[..16], *digest.as_str());
    }

    #[test]
    fn a_record_digest_is_16_lowercase_hex_characters() {
        for bad in [
            "",
            "0123456789abcde",
            "0123456789abcdef0",
            "0123456789ABCDEF",
            "0123456789abcdeg",
        ] {
            assert_eq!(
                bad.parse::<RecordDigest>(),
                Err(ValueError::RecordDigestShape),
                "{bad}"
            );
        }
        assert!("0123456789abcdef".parse::<RecordDigest>().is_ok());
    }

    #[test]
    fn the_digest_follows_the_identifier_and_the_time() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let b = register_doc(2, DocKind::Spec, "docs/spec/a.md", None, 1);
        let t = at("2026-11-04T21:40:00+09:00");
        let u = at("2026-11-04T21:41:00+09:00");
        assert_eq!(RecordDigest::of(&a.id(), &t), RecordDigest::of(&a.id(), &t));
        assert_ne!(RecordDigest::of(&a.id(), &t), RecordDigest::of(&b.id(), &t));
        assert_ne!(RecordDigest::of(&a.id(), &t), RecordDigest::of(&a.id(), &u));
    }

    #[test]
    fn records_of_one_event_at_two_times_are_one_event_with_two_times() {
        let event = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let (t, u) = (
            at("2026-11-04T21:40:00+09:00"),
            at("2026-11-04T21:41:00+09:00"),
        );
        let mut recorded = Recorded::new(event.clone(), t.clone());
        recorded
            .merge(Recorded::new(event.clone(), u.clone()))
            .unwrap();
        assert_eq!(recorded.event(), &event);
        assert_eq!(recorded.at().len(), 2);
        let times: BTreeSet<_> = recorded.at().iter().map(Timestamp::as_str).collect();
        assert_eq!(times, BTreeSet::from([t.as_str(), u.as_str()]));
        // The same record again is the same record.
        recorded.merge(Recorded::new(event, t)).unwrap();
        assert_eq!(recorded.at().len(), 2);
    }

    /// The times are kept in the order of the digests of their records, whichever
    /// order they arrive in, and are not read as instants: the same instant written
    /// with two offsets is two times.
    #[test]
    fn times_are_in_digest_order_and_are_not_interpreted() {
        let event = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let utc = at("2026-11-04T12:00:00Z");
        let tokyo = at("2026-11-04T21:00:00+09:00");
        let mut one = Recorded::new(event.clone(), utc.clone());
        one.merge(Recorded::new(event.clone(), tokyo.clone()))
            .unwrap();
        let mut other = Recorded::new(event.clone(), tokyo.clone());
        other
            .merge(Recorded::new(event.clone(), utc.clone()))
            .unwrap();
        assert_eq!(one, other, "the order of arrival does not matter");
        assert_eq!(one.at().len(), 2, "one instant, two texts, two records");

        let mut by_digest = [utc, tokyo];
        by_digest.sort_by_key(|t| RecordDigest::of(&event.id(), t));
        assert_eq!(
            one.at().iter().collect::<Vec<_>>(),
            by_digest.iter().collect::<Vec<_>>()
        );
        assert_eq!(one.shown_at(), &by_digest[0]);
    }

    #[test]
    fn the_times_of_two_stores_are_one_set() {
        let event = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let (t, u, v) = (
            at("2026-11-04T21:40:00+09:00"),
            at("2026-11-04T21:41:00+09:00"),
            at("2026-11-04T21:42:00+09:00"),
        );
        let repo = Recorded::new(event.clone(), t.clone());
        let mut private = Recorded::new(event.clone(), t.clone());
        private
            .merge(Recorded::new(event.clone(), u.clone()))
            .unwrap();
        private
            .merge(Recorded::new(event.clone(), v.clone()))
            .unwrap();
        let union = union_times(&event.id(), repo.at(), private.at());
        assert_eq!(&union, private.at());
        assert_eq!(union_times(&event.id(), private.at(), repo.at()), union);
    }

    #[test]
    fn records_that_do_not_agree_on_the_whole_event_are_not_merged() {
        // The starting classification is outside the identifier of a registration.
        let plain = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let tagged = Event::new(
            eid(1),
            BTreeSet::new(),
            EventBody::Registered(Registration::Doc(DocRegistration::new(
                DocKind::Spec,
                path("docs/spec/a.md"),
                None,
                None,
                BTreeSet::from([tag("area:auth")]),
                version(1),
            ))),
        )
        .unwrap();
        assert_eq!(plain.id(), tagged.id());
        let t = at("2026-11-04T21:40:00+09:00");
        let mut recorded = Recorded::new(plain.clone(), t.clone());
        assert_eq!(
            recorded.merge(Recorded::new(tagged, t)),
            Err(EventError::RecordsDiffer { id: plain.id() })
        );
    }
}
