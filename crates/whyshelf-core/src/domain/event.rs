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
//! `at` is for display and decides nothing.
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
//!                                   (not at, not by, not the file format version)
//! ```
//!
//! Nothing resolves the fork but the next write: it reads every tip, lists them
//! all as its heads and so becomes the single tip. Until then a value that two
//! branches set differently is reported as a conflict (see `classification`).
//!
//! # Identifiers
//!
//! An [`EventId`] is the hash of the kind, the target, the kind-specific content
//! and the heads. It excludes the time, the person (`by`) and any wire-format
//! version, so two writers who see the same state and find the same change
//! compute the same identifier and write one file, not two. Because the heads are
//! inside, the same change made again after something else happened has a
//! different identifier: `+x`, `-x`, `+x` are three events; revoke and approve
//! again are two approvals; a revision A, B, A, B is four events.
//!
//! The identifier of `Registered` also contains the new entity's identifier, so
//! two processes that register the same file independently never write the same
//! file name and a Git merge never collides on it. The overlapping registrations
//! are folded afterwards (see `classification`).
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

    /// The part of the body that enters the event identifier: everything but the
    /// person (`by`) and, for a registration, the starting classification.
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
            Self::Approved { attestation, .. } => attestation.to_canon(),
            Self::Revoked {
                revokes, reason, ..
            } => Canon::map([
                ("revokes", revokes.to_canon()),
                ("reason", reason.to_canon()),
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

/// One event of one entity's stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    id: EventId,
    target: EntityId,
    heads: BTreeSet<EventId>,
    at: Timestamp,
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
        at: Timestamp,
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
            at,
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

    /// When the event was recorded. Display only.
    pub fn at(&self) -> &Timestamp {
        &self.at
    }

    /// What the event records.
    pub fn body(&self) -> &EventBody {
        &self.body
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

    pub(crate) fn ts() -> Timestamp {
        Timestamp::from_validated("2026-11-04T21:40:00+09:00".into()).unwrap()
    }

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
            ts(),
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
            ts(),
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
            ts(),
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
            ts(),
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
    fn identifiers_exclude_time_and_person() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let b = Event::new(
            eid(1),
            BTreeSet::new(),
            Timestamp::from_validated("2030-01-01T00:00:00Z".into()).unwrap(),
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
                ts(),
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
            Event::new(eid(1), heads(&[some_head]), ts(), body()),
            Err(EventError::SnapshotHasHeads)
        );
        assert!(Event::new(eid(1), BTreeSet::new(), ts(), body()).is_ok());
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
}
