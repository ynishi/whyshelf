//! The classification fold: from events to the tags, kind, owner, path and alias
//! of each entity.
//!
//! [`fold`] is a pure function of a set of events. It does not depend on the
//! order of the slice, so every reader of the same events gets the same view.
//! "Saw" is always the ancestry relation of the event graph (see `event`).
//!
//! # Overlapping registrations
//!
//! Two processes, or two branches, may register the same file without seeing each
//! other. The `Registered` events then have different entity identifiers and all
//! survive in a merge. The fold treats registrations with the same kind, the same
//! path and the same first content hash as one entity: the smallest identifier is
//! the survivor and the others are aliases of it. Every event that targets an
//! alias belongs to the survivor's stream. Items fold the same way by
//! `(Design, number)`.
//!
//! Registrations at one path with *different* first content are different
//! entities. If two entities then sit at one current path, both are Invalid
//! (`PathCollision`) and the status points at it; moving one of them with `mv`
//! resolves it.
//!
//! # Per-value rules
//!
//! | value | rule | on a fork |
//! |---|---|---|
//! | tag | an observed-remove set | concurrent additions both stay |
//! | kind, owner, alias | the tips among the events that wrote the value | two tips with different values are a `Conflict` |
//! | path (`Registered`, `Moved`) | the same | two tips with different paths are a `PathConflict`, solved by `mv` |
//! | alias, across entities | two entities with one alias | both Invalid (`AliasCollision`) |
//!
//! **Tags.** Adding a tag gives that tag the mark of the adding event. Removing a
//! tag removes the marks of the adds that are ancestors of the removing event, and
//! only those. A tag is present when some mark of it is not removed by any
//! removing event. So an add made concurrently with a remove survives; a remove
//! made after seeing the add wins. Within one event, changes to one tag are
//! applied in order: the last one decides the mark, and any removal still clears
//! the marks the event saw.
//!
//! **Values.** The writers of a value are the events that set it. The tips are the
//! writers that are not an ancestor of another writer. One distinct value among
//! the tips is the value. Several are a conflict, listed in full. The next write
//! (for example `tag` or `mv`) lists every tip of the stream among its heads, so it
//! is a descendant of all the conflicting writers and the conflict is gone.
//!
//! # What a conflict does
//!
//! A conflict is reported and never resolved by choosing a side: no reader takes the
//! smallest value, so every reader sees the same thing.
//!
//! | value in conflict | the entity | shown as |
//! |---|---|---|
//! | kind | not made into an entity of either kind; held as a [`ConflictedDoc`], Invalid, its file unread | a row with the kinds in play; the entities that stand on it are Blocked |
//! | path | likewise a [`ConflictedDoc`]: which file to read is not known | the same, and the paths in the issue list |
//! | alias | the alias is not used to find it | an issue |
//! | owner | the entity has no owner for the evaluation | an issue |
//! | version | the recorded text is not known, so the entity is not "changed since ingest" | an issue |
//!
//! `publish` and `purge` need values they can rely on and are refused while one of
//! the kind, the path or the version is in conflict. What settles a conflict is a
//! write that sets that value again (`reclassify` for the kind, the alias or the
//! owner; `mv` for the path): it lists every tip among its heads, so it is a
//! descendant of all the writers in conflict.
//!
//! **A file that says something else.** The classification keys of a file's front
//! matter (`tags`, `kind`, `owner`, `alias`) are not part of the fold. They are taken
//! into the registration once ([`ClassificationImport`](super::front::ClassificationImport)); afterwards a file
//! that differs from the fold is shown as [`ClassificationIssue::FileDiffers`], and
//! nothing is changed on either side.
//!
//! **Roots from a publication.** A `Published` event carrying a snapshot is a
//! writer of every value and a mark of every tag in its snapshot. Folding the repo
//! store alone therefore gives the classification the private store had.
//!
//! **Absent ancestors.** A head that is not among the events is ignored.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::actor::Actor;
use super::event::{
    ClassChange, DocKind, Event, EventBody, EventGraph, EventId, Publication, Registration,
    Version, VersionKey,
};
use super::front::ClassDrift;
use super::id::{Alias, DesignId, EntityId, ItemId, ItemNumber};
use super::location::RelPath;
use super::nonempty::NonEmptyVec;
use super::state::{Approval, Completeness, EntityState, InvalidReason, Upstream};
use super::tag::{Tag, TagChange};

/// The value of a field after folding, which is either agreed or in conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged<T: Ord> {
    /// One value among the tips.
    Agreed(T),
    /// Two or more different values among the tips.
    Conflict(BTreeSet<T>),
}

impl<T: Ord> Merged<T> {
    /// The value when agreed.
    pub fn agreed(&self) -> Option<&T> {
        match self {
            Self::Agreed(v) => Some(v),
            Self::Conflict(_) => None,
        }
    }

    /// Whether the tips disagree.
    pub(crate) fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict(_))
    }
}

/// What the kind of a document is after folding: one kind, or the several that
/// two branches wrote.
///
/// The kind decides which map an entity is evaluated in and which file format its
/// file is read as. With two kinds there is no such map, so a document in conflict
/// is evaluated as a [`ConflictedDoc`] and is never made into an entity of either
/// kind: no guess, and no choice that depends on who reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindState {
    /// One kind among the tips.
    Known(DocKind),
    /// Two or more kinds among the tips.
    Conflict(BTreeSet<DocKind>),
}

impl KindState {
    /// The kind, when there is only one.
    pub fn known(&self) -> Option<DocKind> {
        match self {
            Self::Known(kind) => Some(*kind),
            Self::Conflict(_) => None,
        }
    }

    /// The kinds in play: the one, or all those in conflict.
    pub fn kinds(&self) -> BTreeSet<DocKind> {
        match self {
            Self::Known(kind) => BTreeSet::from([*kind]),
            Self::Conflict(kinds) => kinds.clone(),
        }
    }
}

impl From<Merged<DocKind>> for KindState {
    fn from(merged: Merged<DocKind>) -> Self {
        match merged {
            Merged::Agreed(kind) => Self::Known(kind),
            Merged::Conflict(kinds) => Self::Conflict(kinds),
        }
    }
}

/// The classification of an entity that has a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocClass {
    id: EntityId,
    kind: KindState,
    path: Merged<RelPath>,
    alias: Merged<Option<Alias>>,
    owner: Merged<Option<Actor>>,
    version: Merged<Version>,
    tags: BTreeSet<Tag>,
    tips: BTreeSet<EventId>,
    duplicates: BTreeSet<EntityId>,
}

impl DocClass {
    /// The surviving identifier.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// The kind.
    pub fn kind(&self) -> &KindState {
        &self.kind
    }

    /// The current path.
    pub fn path(&self) -> &Merged<RelPath> {
        &self.path
    }

    /// The alias, if any.
    pub fn alias(&self) -> &Merged<Option<Alias>> {
        &self.alias
    }

    /// The owner, if any.
    pub fn owner(&self) -> &Merged<Option<Actor>> {
        &self.owner
    }

    /// The latest recorded version.
    pub fn version(&self) -> &Merged<Version> {
        &self.version
    }

    /// The tags present.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The tips of the stream: what the next write lists as its heads.
    pub fn tips(&self) -> &BTreeSet<EventId> {
        &self.tips
    }

    /// The identifiers folded into this one.
    pub fn duplicates(&self) -> &BTreeSet<EntityId> {
        &self.duplicates
    }

    /// The first value in conflict, if any: the kind, the path, the version, the
    /// owner, the alias, in that order.
    ///
    /// An operation that relies on the classification (publishing copies it, purging
    /// reads the versions) is refused while this is `Some`.
    pub fn conflict(&self) -> Option<ConflictField> {
        [
            (self.kind.known().is_none(), ConflictField::Kind),
            (self.path.is_conflict(), ConflictField::Path),
            (self.version.is_conflict(), ConflictField::Version),
            (self.owner.is_conflict(), ConflictField::Owner),
            (self.alias.is_conflict(), ConflictField::Alias),
        ]
        .into_iter()
        .find_map(|(in_conflict, field)| in_conflict.then_some(field))
    }
}

/// The classification of a work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemClass {
    id: ItemId,
    design: DesignId,
    number: ItemNumber,
    tips: BTreeSet<EventId>,
    duplicates: BTreeSet<EntityId>,
}

impl ItemClass {
    /// The surviving identifier of the item.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The Design the item belongs to.
    pub fn design(&self) -> DesignId {
        self.design
    }

    /// The item's number in that Design.
    pub fn number(&self) -> ItemNumber {
        self.number
    }

    /// The tips of the item's stream.
    pub fn tips(&self) -> &BTreeSet<EventId> {
        &self.tips
    }

    /// The identifiers folded into this one.
    pub fn duplicates(&self) -> &BTreeSet<EntityId> {
        &self.duplicates
    }
}

/// A value that can be in conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictField {
    /// The kind.
    Kind,
    /// The owner.
    Owner,
    /// The alias.
    Alias,
    /// The recorded version of the text.
    Version,
    /// The path.
    Path,
}

impl fmt::Display for ConflictField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Kind => "kind",
            Self::Owner => "owner",
            Self::Alias => "alias",
            Self::Version => "version",
            Self::Path => "path",
        })
    }
}

/// A document whose kind or path two branches wrote differently.
///
/// It is held beside the typed maps of Findings, Specs and Designs, never in one of
/// them, because there is no kind (or no path to read) to make an entity of. It
/// has a state of its own, Invalid for the conflict, and an entity that stands on
/// it reads that state like any other upstream's: it is Blocked, and a gate
/// stops on it. Its file is not read: which file, or which format, is not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictedDoc {
    id: EntityId,
    field: ConflictField,
    kinds: BTreeSet<DocKind>,
    state: EntityState,
}

impl ConflictedDoc {
    /// The document, if its kind or its path is in conflict (the kind first).
    pub fn of(class: &DocClass) -> Option<Self> {
        let (field, kinds) = match (&class.kind, &class.path) {
            (KindState::Conflict(kinds), _) => (ConflictField::Kind, kinds.clone()),
            (KindState::Known(kind), Merged::Conflict(_)) => {
                (ConflictField::Path, BTreeSet::from([*kind]))
            }
            (KindState::Known(_), Merged::Agreed(_)) => return None,
        };
        Some(Self {
            id: class.id,
            field,
            kinds,
            state: EntityState::new(
                Approval::Invalid(NonEmptyVec::new(
                    InvalidReason::ClassificationConflict { field },
                    Vec::new(),
                )),
                Upstream::Ok,
                Completeness::Complete,
            ),
        })
    }

    /// The identifier.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// The value in conflict.
    pub fn field(&self) -> ConflictField {
        self.field
    }

    /// The kinds it might be: all those written for a kind conflict, the one kind
    /// for a path conflict.
    pub fn kinds(&self) -> &BTreeSet<DocKind> {
        &self.kinds
    }

    /// The state: Invalid, with the conflict as the reason.
    pub fn state(&self) -> &EntityState {
        &self.state
    }
}

/// A thing the status should point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassificationIssue {
    /// Two tips of the stream disagree about a value; `tag` settles it.
    Conflict {
        /// The entity.
        entity: EntityId,
        /// The value in conflict.
        field: ConflictField,
    },
    /// Two tips of the stream disagree about the path; `mv` settles it.
    PathConflict {
        /// The entity.
        entity: EntityId,
        /// The paths in conflict.
        paths: BTreeSet<RelPath>,
    },
    /// Two entities sit at one path. Both are Invalid.
    PathCollision {
        /// The shared path.
        path: RelPath,
        /// The entities.
        entities: BTreeSet<EntityId>,
    },
    /// Two entities have one alias. Both are Invalid.
    AliasCollision {
        /// The shared alias.
        alias: Alias,
        /// The entities.
        entities: BTreeSet<EntityId>,
    },
    /// Events target an entity that was never registered in this store.
    UnregisteredTarget {
        /// The entity.
        entity: EntityId,
    },
    /// A classification key in the file says something else than the store has.
    /// The store is the truth after registration; a person decides which is right.
    FileDiffers {
        /// The entity.
        entity: EntityId,
        /// The value, and what each side says.
        drift: ClassDrift,
    },
}

/// The result of [`fold`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassificationView {
    docs: BTreeMap<EntityId, DocClass>,
    items: BTreeMap<EntityId, ItemClass>,
    aliases: BTreeMap<EntityId, EntityId>,
    issues: Vec<ClassificationIssue>,
}

impl ClassificationView {
    /// The surviving identifier of an entity (itself unless it is an alias).
    pub fn canonical(&self, id: EntityId) -> EntityId {
        self.aliases.get(&id).copied().unwrap_or(id)
    }

    /// Every folded-away identifier and the survivor it maps to.
    pub fn alias_map(&self) -> &BTreeMap<EntityId, EntityId> {
        &self.aliases
    }

    /// The classification of a file entity, by any of its identifiers.
    pub fn doc(&self, id: EntityId) -> Option<&DocClass> {
        self.docs.get(&self.canonical(id))
    }

    /// Every file entity.
    pub fn docs(&self) -> impl Iterator<Item = &DocClass> {
        self.docs.values()
    }

    /// The classification of an item, by any of its identifiers.
    pub fn item(&self, id: EntityId) -> Option<&ItemClass> {
        self.items.get(&self.canonical(id))
    }

    /// Every item.
    pub fn items(&self) -> impl Iterator<Item = &ItemClass> {
        self.items.values()
    }

    /// The identifier of the item `(design, number)`, if one was registered.
    pub fn item_id(&self, design: DesignId, number: ItemNumber) -> Option<ItemId> {
        self.items
            .values()
            .find(|i| i.design == design && i.number == number)
            .map(|i| i.id)
    }

    /// The tips of an entity's stream, whatever its kind.
    pub fn tips(&self, id: EntityId) -> Option<&BTreeSet<EventId>> {
        let id = self.canonical(id);
        self.docs
            .get(&id)
            .map(|d| &d.tips)
            .or_else(|| self.items.get(&id).map(|i| &i.tips))
    }

    /// What the status should point at.
    pub fn issues(&self) -> &[ClassificationIssue] {
        &self.issues
    }

    /// The reasons the entity is Invalid because of how it is classified: a path
    /// or an alias shared with another entity.
    pub fn invalid_reasons(&self, id: EntityId) -> Vec<InvalidReason> {
        let id = self.canonical(id);
        self.issues
            .iter()
            .filter_map(|issue| match issue {
                ClassificationIssue::PathCollision { path, entities } if entities.contains(&id) => {
                    Some(InvalidReason::PathCollision { path: path.clone() })
                }
                ClassificationIssue::AliasCollision { alias, entities }
                    if entities.contains(&id) =>
                {
                    Some(InvalidReason::AliasCollision {
                        alias: alias.clone(),
                    })
                }
                _ => None,
            })
            .collect()
    }
}

fn merge<T: Ord + Clone>(graph: &EventGraph<'_>, writers: &[(EventId, T)]) -> Option<Merged<T>> {
    let ids: BTreeSet<EventId> = writers.iter().map(|(id, _)| *id).collect();
    let tips = graph.tips_of(&ids);
    let values: BTreeSet<T> = writers
        .iter()
        .filter(|(id, _)| tips.contains(id))
        .map(|(_, v)| v.clone())
        .collect();
    let mut iter = values.into_iter();
    let first = iter.next()?;
    match iter.next() {
        None => Some(Merged::Agreed(first)),
        Some(second) => {
            let mut set = BTreeSet::from([first, second]);
            set.extend(iter);
            Some(Merged::Conflict(set))
        }
    }
}

#[derive(Default)]
struct DocWriters {
    kind: Vec<(EventId, DocKind)>,
    path: Vec<(EventId, RelPath)>,
    alias: Vec<(EventId, Option<Alias>)>,
    owner: Vec<(EventId, Option<Actor>)>,
    version: Vec<(EventId, Version)>,
    marks: BTreeMap<Tag, Vec<EventId>>,
    removers: BTreeMap<Tag, Vec<EventId>>,
}

fn group_duplicates(graph: &EventGraph<'_>) -> BTreeMap<EntityId, EntityId> {
    let mut docs: BTreeMap<(DocKind, RelPath, VersionKey), BTreeSet<EntityId>> = BTreeMap::new();
    let mut items: BTreeMap<(DesignId, ItemNumber), BTreeSet<EntityId>> = BTreeMap::new();
    for event in graph.events() {
        match event.body() {
            EventBody::Registered(Registration::Doc(d)) => {
                docs.entry((d.kind(), d.path().clone(), d.first().key()))
                    .or_default()
                    .insert(event.target());
            }
            EventBody::Registered(Registration::Item { design, number }) => {
                items
                    .entry((*design, *number))
                    .or_default()
                    .insert(event.target());
            }
            _ => {}
        }
    }
    let mut aliases = BTreeMap::new();
    for ids in docs.values().chain(items.values()) {
        let mut iter = ids.iter();
        if let Some(survivor) = iter.next() {
            for other in iter {
                aliases.entry(*other).or_insert(*survivor);
            }
        }
    }
    aliases
}

fn tag_present(graph: &EventGraph<'_>, marks: &[EventId], removers: &[EventId]) -> bool {
    marks
        .iter()
        .any(|mark| removers.iter().all(|r| !graph.is_ancestor(mark, r)))
}

/// Fold a set of events into the classification of every entity.
///
/// Pure and order-independent: the same events in any order give an equal view.
pub fn fold(events: &[Event]) -> ClassificationView {
    let graph = EventGraph::new(events);
    let aliases = group_duplicates(&graph);
    let canonical = |id: EntityId| aliases.get(&id).copied().unwrap_or(id);

    let mut streams: BTreeMap<EntityId, Vec<&Event>> = BTreeMap::new();
    for event in graph.events() {
        streams
            .entry(canonical(event.target()))
            .or_default()
            .push(event);
    }
    let mut duplicates_of: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
    for (dup, survivor) in &aliases {
        duplicates_of.entry(*survivor).or_default().insert(*dup);
    }

    let mut view = ClassificationView {
        aliases: aliases.clone(),
        ..ClassificationView::default()
    };

    for (id, stream) in streams {
        let stream_ids: BTreeSet<EventId> = stream.iter().map(|e| e.id()).collect();
        let tips = graph.tips_of(&stream_ids);
        let duplicates = duplicates_of.remove(&id).unwrap_or_default();

        let item_registration = stream.iter().find_map(|e| match e.body() {
            EventBody::Registered(Registration::Item { design, number }) => {
                Some((*design, *number))
            }
            _ => None,
        });
        if let Some((design, number)) = item_registration {
            view.items.insert(
                id,
                ItemClass {
                    id: ItemId::new(id),
                    design,
                    number,
                    tips,
                    duplicates,
                },
            );
            continue;
        }

        let mut w = DocWriters::default();
        for event in &stream {
            collect_writers(&mut w, event);
        }
        let merged = (
            merge(&graph, &w.kind),
            merge(&graph, &w.path),
            merge(&graph, &w.alias),
            merge(&graph, &w.owner),
            merge(&graph, &w.version),
        );
        let (Some(kind), Some(path), Some(alias), Some(owner), Some(version)) = merged else {
            view.issues
                .push(ClassificationIssue::UnregisteredTarget { entity: id });
            continue;
        };
        let kind = KindState::from(kind);
        let tags = w
            .marks
            .iter()
            .filter(|(tag, marks)| {
                let removers = w.removers.get(*tag).map_or(&[][..], Vec::as_slice);
                tag_present(&graph, marks, removers)
            })
            .map(|(tag, _)| tag.clone())
            .collect();
        view.docs.insert(
            id,
            DocClass {
                id,
                kind,
                path,
                alias,
                owner,
                version,
                tags,
                tips,
                duplicates,
            },
        );
    }

    view.issues.extend(find_issues(&view.docs));
    view
}

fn collect_writers(w: &mut DocWriters, event: &Event) {
    let id = event.id();
    match event.body() {
        EventBody::Registered(Registration::Doc(d)) => {
            w.kind.push((id, d.kind()));
            w.path.push((id, d.path().clone()));
            w.alias.push((id, d.alias().cloned()));
            w.owner.push((id, d.owner().cloned()));
            w.version.push((id, d.first().clone()));
            for tag in d.tags() {
                w.marks.entry(tag.clone()).or_default().push(id);
            }
        }
        EventBody::Published(Publication::Incoming { snapshot, .. }) => {
            w.kind.push((id, snapshot.kind()));
            w.path.push((id, snapshot.path().clone()));
            w.alias.push((id, snapshot.alias().cloned()));
            w.owner.push((id, snapshot.owner().cloned()));
            w.version.push((id, snapshot.version().clone()));
            for tag in snapshot.tags() {
                w.marks.entry(tag.clone()).or_default().push(id);
            }
        }
        EventBody::Revised { new, .. } => w.version.push((id, new.clone())),
        EventBody::Moved { new, .. } => w.path.push((id, new.clone())),
        EventBody::Reclassified { changes } => {
            let mut kind = None;
            let mut owner = None;
            let mut alias = None;
            // Per tag: (any removal, the last change is an addition).
            let mut tag_ops: BTreeMap<Tag, (bool, bool)> = BTreeMap::new();
            for change in changes.iter() {
                match change {
                    ClassChange::Tag(TagChange::Add(tag)) => {
                        tag_ops.entry(tag.clone()).or_default().1 = true;
                    }
                    ClassChange::Tag(TagChange::Remove(tag)) => {
                        *tag_ops.entry(tag.clone()).or_default() = (true, false);
                    }
                    ClassChange::SetKind(k) => kind = Some(*k),
                    ClassChange::SetOwner(o) => owner = Some(Some(o.clone())),
                    ClassChange::ClearOwner => owner = Some(None),
                    ClassChange::SetAlias(a) => alias = Some(Some(a.clone())),
                    ClassChange::ClearAlias => alias = Some(None),
                }
            }
            if let Some(k) = kind {
                w.kind.push((id, k));
            }
            if let Some(o) = owner {
                w.owner.push((id, o));
            }
            if let Some(a) = alias {
                w.alias.push((id, a));
            }
            for (tag, (removed, ends_added)) in tag_ops {
                if removed {
                    w.removers.entry(tag.clone()).or_default().push(id);
                }
                if ends_added {
                    w.marks.entry(tag).or_default().push(id);
                }
            }
        }
        EventBody::Published(Publication::Outgoing { .. })
        | EventBody::Approved { .. }
        | EventBody::Revoked { .. }
        | EventBody::Purged { .. }
        | EventBody::Registered(Registration::Item { .. }) => {}
    }
}

fn find_issues(docs: &BTreeMap<EntityId, DocClass>) -> Vec<ClassificationIssue> {
    let mut issues = Vec::new();
    let mut by_path: BTreeMap<&RelPath, BTreeSet<EntityId>> = BTreeMap::new();
    let mut by_alias: BTreeMap<&Alias, BTreeSet<EntityId>> = BTreeMap::new();
    for (id, doc) in docs {
        if doc.kind.known().is_none() {
            issues.push(ClassificationIssue::Conflict {
                entity: *id,
                field: ConflictField::Kind,
            });
        }
        if doc.version.is_conflict() {
            issues.push(ClassificationIssue::Conflict {
                entity: *id,
                field: ConflictField::Version,
            });
        }
        if doc.owner.is_conflict() {
            issues.push(ClassificationIssue::Conflict {
                entity: *id,
                field: ConflictField::Owner,
            });
        }
        if doc.alias.is_conflict() {
            issues.push(ClassificationIssue::Conflict {
                entity: *id,
                field: ConflictField::Alias,
            });
        }
        match &doc.path {
            Merged::Agreed(path) => {
                by_path.entry(path).or_default().insert(*id);
            }
            Merged::Conflict(paths) => issues.push(ClassificationIssue::PathConflict {
                entity: *id,
                paths: paths.clone(),
            }),
        }
        if let Merged::Agreed(Some(alias)) = &doc.alias {
            by_alias.entry(alias).or_default().insert(*id);
        }
    }
    for (path, entities) in by_path {
        if entities.len() > 1 {
            issues.push(ClassificationIssue::PathCollision {
                path: path.clone(),
                entities,
            });
        }
    }
    for (alias, entities) in by_alias {
        if entities.len() > 1 {
            issues.push(ClassificationIssue::AliasCollision {
                alias: alias.clone(),
                entities,
            });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::test_support::*;
    use crate::domain::event::{ClassSnapshot, DocRegistration, StoreKind};
    use crate::domain::id::test_support::eid;
    use crate::domain::nonempty::NonEmptyVec;

    const P: &str = "docs/spec/a.md";

    fn root(entity: u128) -> Event {
        register_doc(entity, DocKind::Spec, P, None, 1)
    }

    fn tags_of(view: &ClassificationView, entity: u128) -> Vec<String> {
        view.doc(eid(entity))
            .unwrap()
            .tags()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn owner_event(entity: u128, hs: &[EventId], name: &str) -> Event {
        reclassify(
            entity,
            hs,
            vec![ClassChange::SetOwner(Actor::new(name).unwrap())],
        )
    }

    fn owner_of(view: &ClassificationView, entity: u128) -> Merged<Option<Actor>> {
        view.doc(eid(entity)).unwrap().owner().clone()
    }

    fn some(name: &str) -> Option<Actor> {
        Some(Actor::new(name).unwrap())
    }

    // --- tags ---------------------------------------------------------------

    #[test]
    fn a_tag_is_present_after_an_add_and_gone_after_a_remove_that_saw_it() {
        let r = root(1);
        let add = add_tag(1, &[r.id()], "x");
        let rm = remove_tag(1, &[add.id()], "x");
        assert_eq!(tags_of(&fold(&[r.clone(), add.clone()]), 1), ["x"]);
        assert_eq!(tags_of(&fold(&[r, add, rm]), 1), Vec::<String>::new());
    }

    #[test]
    fn plus_minus_plus_leaves_the_tag_present() {
        let r = root(1);
        let p1 = add_tag(1, &[r.id()], "x");
        let m = remove_tag(1, &[p1.id()], "x");
        let p2 = add_tag(1, &[m.id()], "x");
        assert_eq!(tags_of(&fold(&[r, p1, m, p2]), 1), ["x"]);
    }

    struct Case {
        name: &'static str,
        build: fn() -> Vec<Event>,
        expect: &'static [&'static str],
    }

    fn two_branch_union() -> Vec<Event> {
        let r = root(1);
        let a = add_tag(1, &[r.id()], "a");
        let b = add_tag(1, &[r.id()], "b");
        vec![r, a, b]
    }

    fn concurrent_add_survives_a_remove() -> Vec<Event> {
        let r = root(1);
        let add = add_tag(1, &[r.id()], "x");
        let rm = remove_tag(1, &[r.id()], "x"); // never saw the add
        vec![r, add, rm]
    }

    fn merge_then_remove_clears_both_marks() -> Vec<Event> {
        let r = root(1);
        let add_a = add_tag(1, &[r.id()], "x");
        let add_b = add_tag(1, &[r.id()], "x");
        let rm = remove_tag(1, &[add_a.id(), add_b.id()], "x");
        vec![r, add_a, add_b, rm]
    }

    fn remove_only_clears_what_it_saw() -> Vec<Event> {
        let r = root(1);
        let add_a = add_tag(1, &[r.id()], "x");
        let rm = remove_tag(1, &[add_a.id()], "x");
        let add_b = add_tag(1, &[r.id()], "x");
        // add_b is concurrent with the remove and has the same content as add_a:
        // the same heads give the same event, so it is the very same mark.
        vec![r, add_a, rm, add_b]
    }

    fn registration_tags_carry_a_mark() -> Vec<Event> {
        let r = Event::new(
            eid(1),
            BTreeSet::new(),
            ts(),
            EventBody::Registered(Registration::Doc(DocRegistration::new(
                DocKind::Spec,
                path(P),
                None,
                None,
                BTreeSet::from([tag("area:auth"), tag("wip")]),
                version(1),
            ))),
        )
        .unwrap();
        let rm = remove_tag(1, &[r.id()], "wip");
        vec![r, rm]
    }

    fn one_event_add_then_remove_nets_to_removed() -> Vec<Event> {
        let r = root(1);
        let both = reclassify(
            1,
            &[r.id()],
            vec![
                ClassChange::Tag(TagChange::Add(tag("x"))),
                ClassChange::Tag(TagChange::Remove(tag("x"))),
            ],
        );
        vec![r, both]
    }

    fn one_event_remove_then_add_nets_to_present() -> Vec<Event> {
        let r = root(1);
        let first = add_tag(1, &[r.id()], "x");
        let swap = reclassify(
            1,
            &[first.id()],
            vec![
                ClassChange::Tag(TagChange::Remove(tag("x"))),
                ClassChange::Tag(TagChange::Add(tag("x"))),
            ],
        );
        vec![r, first, swap]
    }

    #[test]
    fn tag_set_table() {
        let cases = [
            Case {
                name: "two branches add different tags",
                build: two_branch_union,
                expect: &["a", "b"],
            },
            Case {
                name: "an add concurrent with a remove survives",
                build: concurrent_add_survives_a_remove,
                expect: &["x"],
            },
            Case {
                name: "a remove after the merge clears every mark it saw",
                build: merge_then_remove_clears_both_marks,
                expect: &[],
            },
            Case {
                name: "the remove clears the add it saw",
                build: remove_only_clears_what_it_saw,
                expect: &[],
            },
            Case {
                name: "tags given at registration can be removed",
                build: registration_tags_carry_a_mark,
                expect: &["area:auth"],
            },
            Case {
                name: "add then remove in one event nets to removed",
                build: one_event_add_then_remove_nets_to_removed,
                expect: &[],
            },
            Case {
                name: "remove then add in one event nets to present",
                build: one_event_remove_then_add_nets_to_present,
                expect: &["x"],
            },
        ];
        for case in cases {
            let view = fold(&(case.build)());
            assert_eq!(tags_of(&view, 1), case.expect, "{}", case.name);
        }
    }

    // --- values -------------------------------------------------------------

    #[test]
    fn a_fork_with_two_owners_is_a_conflict_and_the_next_write_resolves_it() {
        let r = root(1);
        let a = owner_event(1, &[r.id()], "alice");
        let b = owner_event(1, &[r.id()], "bob");
        let view = fold(&[r.clone(), a.clone(), b.clone()]);
        assert_eq!(
            owner_of(&view, 1),
            Merged::Conflict(BTreeSet::from([some("alice"), some("bob")]))
        );
        assert_eq!(view.doc(eid(1)).unwrap().tips().len(), 2);
        assert!(view.issues().contains(&ClassificationIssue::Conflict {
            entity: eid(1),
            field: ConflictField::Owner
        }));

        // The next write lists every tip, so it descends from both.
        let tips: Vec<EventId> = view.doc(eid(1)).unwrap().tips().iter().copied().collect();
        let fix = owner_event(1, &tips, "alice");
        let healed = fold(&[r, a, b, fix.clone()]);
        assert_eq!(owner_of(&healed, 1), Merged::Agreed(some("alice")));
        assert_eq!(
            healed.doc(eid(1)).unwrap().tips(),
            &BTreeSet::from([fix.id()])
        );
        assert!(healed.issues().is_empty());
    }

    #[test]
    fn a_fork_with_the_same_value_on_both_sides_is_not_a_conflict() {
        let r = root(1);
        let a = owner_event(1, &[r.id()], "alice");
        let b = reclassify(
            1,
            &[r.id()],
            vec![
                ClassChange::SetOwner(Actor::new("alice").unwrap()),
                ClassChange::Tag(TagChange::Add(tag("x"))),
            ],
        );
        let view = fold(&[r, a, b]);
        assert_eq!(owner_of(&view, 1), Merged::Agreed(some("alice")));
    }

    #[test]
    fn a_later_write_overrides_an_earlier_one_on_the_same_line() {
        let r = root(1);
        let a = owner_event(1, &[r.id()], "alice");
        let b = owner_event(1, &[a.id()], "bob");
        let cleared = reclassify(1, &[b.id()], vec![ClassChange::ClearOwner]);
        assert_eq!(
            owner_of(&fold(&[r.clone(), a.clone(), b.clone()]), 1),
            Merged::Agreed(some("bob"))
        );
        assert_eq!(
            owner_of(&fold(&[r, a, b, cleared]), 1),
            Merged::Agreed(None)
        );
    }

    #[test]
    fn kind_and_alias_conflicts_are_reported_by_field() {
        let r = root(1);
        let k1 = reclassify(1, &[r.id()], vec![ClassChange::SetKind(DocKind::Design)]);
        let k2 = reclassify(1, &[r.id()], vec![ClassChange::SetKind(DocKind::Finding)]);
        let a1 = reclassify(
            1,
            &[r.id()],
            vec![ClassChange::SetAlias("spec/one".parse().unwrap())],
        );
        let a2 = reclassify(
            1,
            &[r.id()],
            vec![ClassChange::SetAlias("spec/two".parse().unwrap())],
        );
        let view = fold(&[r, k1, k2, a1, a2]);
        let issues = view.issues();
        assert!(issues.contains(&ClassificationIssue::Conflict {
            entity: eid(1),
            field: ConflictField::Kind
        }));
        assert!(issues.contains(&ClassificationIssue::Conflict {
            entity: eid(1),
            field: ConflictField::Alias
        }));
        assert!(view.doc(eid(1)).unwrap().kind().known().is_none());
    }

    #[test]
    fn a_kind_in_conflict_is_a_conflicted_doc_invalid_for_the_kind_and_of_no_kind() {
        let r = root(1);
        let k1 = reclassify(1, &[r.id()], vec![ClassChange::SetKind(DocKind::Finding)]);
        let k2 = reclassify(1, &[r.id()], vec![ClassChange::SetKind(DocKind::Design)]);
        let view = fold(&[r, k1, k2]);
        let class = view.doc(eid(1)).unwrap();
        assert_eq!(
            class.kind(),
            &KindState::Conflict(BTreeSet::from([DocKind::Finding, DocKind::Design]))
        );
        let conflicted = ConflictedDoc::of(class).expect("a conflict");
        assert_eq!(conflicted.field(), ConflictField::Kind);
        assert_eq!(
            conflicted.kinds(),
            &BTreeSet::from([DocKind::Finding, DocKind::Design]),
            "neither kind is chosen"
        );
        assert!(matches!(
            conflicted.state().approval(),
            Approval::Invalid(reasons) if reasons.first()
                == &InvalidReason::ClassificationConflict { field: ConflictField::Kind }
        ));
        assert!(matches!(conflicted.state().upstream(), Upstream::Ok));
    }

    #[test]
    fn a_path_in_conflict_is_a_conflicted_doc_of_the_one_kind_and_agreement_is_not() {
        let r = root(1);
        let m1 = moved(1, &[r.id()], P, "docs/spec/b.md");
        let m2 = moved(1, &[r.id()], P, "docs/spec/c.md");
        let view = fold(&[r.clone(), m1, m2]);
        let conflicted = ConflictedDoc::of(view.doc(eid(1)).unwrap()).expect("a conflict");
        assert_eq!(conflicted.field(), ConflictField::Path);
        assert_eq!(conflicted.kinds(), &BTreeSet::from([DocKind::Spec]));
        assert_eq!(ConflictedDoc::of(fold(&[r]).doc(eid(1)).unwrap()), None);
    }

    #[test]
    fn a_version_in_conflict_is_an_issue_and_not_a_conflicted_doc() {
        let r = root(1);
        let v1 = revised(1, &[r.id()], 1, 2);
        let v2 = revised(1, &[r.id()], 1, 3);
        let view = fold(&[r, v1, v2]);
        assert!(view.issues().contains(&ClassificationIssue::Conflict {
            entity: eid(1),
            field: ConflictField::Version
        }));
        assert_eq!(ConflictedDoc::of(view.doc(eid(1)).unwrap()), None);
    }

    #[test]
    fn path_conflict_is_solved_by_a_move_that_lists_every_tip() {
        let r = root(1);
        let m1 = moved(1, &[r.id()], P, "docs/spec/b.md");
        let m2 = moved(1, &[r.id()], P, "docs/spec/c.md");
        let view = fold(&[r.clone(), m1.clone(), m2.clone()]);
        assert_eq!(
            view.issues(),
            [ClassificationIssue::PathConflict {
                entity: eid(1),
                paths: BTreeSet::from([path("docs/spec/b.md"), path("docs/spec/c.md")]),
            }]
        );
        let tips: Vec<EventId> = view.doc(eid(1)).unwrap().tips().iter().copied().collect();
        let fix = moved(1, &tips, "docs/spec/b.md", "docs/spec/d.md");
        let healed = fold(&[r, m1, m2, fix]);
        assert_eq!(
            healed.doc(eid(1)).unwrap().path(),
            &Merged::Agreed(path("docs/spec/d.md"))
        );
        assert!(healed.issues().is_empty());
    }

    #[test]
    fn revision_chain_a_b_a_b_ends_at_b() {
        let r = root(1); // first version 1
        let to2 = revised(1, &[r.id()], 1, 2);
        let to1 = revised(1, &[to2.id()], 2, 1);
        let to2b = revised(1, &[to1.id()], 1, 2);
        let view = fold(&[r, to2, to1, to2b]);
        assert_eq!(
            view.doc(eid(1)).unwrap().version(),
            &Merged::Agreed(version(2))
        );
    }

    // --- streams, tips, absent ancestors ----------------------------------

    #[test]
    fn a_fork_heals_when_the_next_write_lists_all_tips() {
        let r = root(1);
        let a = add_tag(1, &[r.id()], "a");
        let b = add_tag(1, &[r.id()], "b");
        let forked = fold(&[r.clone(), a.clone(), b.clone()]);
        let tips = forked.doc(eid(1)).unwrap().tips().clone();
        assert_eq!(tips, BTreeSet::from([a.id(), b.id()]));
        let join = add_tag(1, &tips.iter().copied().collect::<Vec<_>>(), "c");
        let healed = fold(&[r, a, b, join.clone()]);
        assert_eq!(
            healed.doc(eid(1)).unwrap().tips(),
            &BTreeSet::from([join.id()])
        );
        assert_eq!(tags_of(&healed, 1), ["a", "b", "c"]);
    }

    #[test]
    fn an_absent_ancestor_is_not_an_error() {
        let ghost = register_doc(9, DocKind::Spec, "docs/spec/ghost.md", None, 3).id();
        let r = root(1);
        let a = add_tag(1, &[r.id(), ghost], "x");
        let view = fold(&[r, a]);
        assert_eq!(tags_of(&view, 1), ["x"]);
        assert!(view.issues().is_empty());
    }

    #[test]
    fn events_for_an_unregistered_entity_are_reported() {
        let stray = add_tag(5, &[], "x");
        let view = fold(&[stray]);
        assert_eq!(
            view.issues(),
            [ClassificationIssue::UnregisteredTarget { entity: eid(5) }]
        );
        assert!(view.doc(eid(5)).is_none());
    }

    #[test]
    fn the_fold_does_not_depend_on_the_order_or_repetition_of_events() {
        let r = root(1);
        let a = owner_event(1, &[r.id()], "alice");
        let b = add_tag(1, &[r.id()], "x");
        let c = moved(1, &[a.id(), b.id()], P, "docs/spec/z.md");
        let forward = vec![r.clone(), a.clone(), b.clone(), c.clone()];
        let backward = vec![c.clone(), b.clone(), a.clone(), r.clone()];
        let repeated = vec![r.clone(), r, a.clone(), a, b.clone(), c, b];
        assert_eq!(fold(&forward), fold(&backward));
        assert_eq!(fold(&forward), fold(&repeated));
    }

    // --- overlapping registrations ---------------------------------------

    #[test]
    fn overlapping_registrations_fold_to_the_smallest_entity_id() {
        let r2 = register_doc(2, DocKind::Spec, P, None, 1);
        let r1 = register_doc(1, DocKind::Spec, P, None, 1);
        let r3 = register_doc(3, DocKind::Spec, P, None, 1);
        // An event aimed at a duplicate lands in the survivor's stream.
        let tag_on_dup = add_tag(2, &[r2.id()], "x");
        let view = fold(&[r2.clone(), r1.clone(), r3, tag_on_dup]);
        assert_eq!(view.docs().count(), 1);
        let doc = view.doc(eid(3)).unwrap();
        assert_eq!(doc.id(), eid(1));
        assert_eq!(doc.duplicates(), &BTreeSet::from([eid(2), eid(3)]));
        assert_eq!(view.canonical(eid(2)), eid(1));
        assert_eq!(tags_of(&view, 1), ["x"]);
        assert!(view.issues().is_empty());
    }

    #[test]
    fn registrations_that_differ_in_kind_path_or_content_do_not_fold() {
        let a = register_doc(1, DocKind::Spec, P, None, 1);
        let other_content = register_doc(2, DocKind::Spec, P, None, 2);
        let other_path = register_doc(3, DocKind::Spec, "docs/spec/b.md", None, 1);
        let other_kind = register_doc(4, DocKind::Design, "docs/design/a.md", None, 1);
        let view = fold(&[a, other_content, other_path, other_kind]);
        assert_eq!(view.docs().count(), 4);
        assert!(view.alias_map().is_empty());
    }

    #[test]
    fn same_path_with_different_content_is_a_path_collision_until_one_moves() {
        let a = register_doc(1, DocKind::Spec, P, None, 1);
        let b = register_doc(2, DocKind::Spec, P, None, 2);
        let view = fold(&[a.clone(), b.clone()]);
        assert_eq!(
            view.issues(),
            [ClassificationIssue::PathCollision {
                path: path(P),
                entities: BTreeSet::from([eid(1), eid(2)]),
            }]
        );
        for n in [1, 2] {
            assert_eq!(
                view.invalid_reasons(eid(n)),
                vec![InvalidReason::PathCollision { path: path(P) }]
            );
        }
        let mv = moved(2, &[b.id()], P, "docs/spec/b.md");
        let solved = fold(&[a, b, mv]);
        assert!(solved.issues().is_empty());
        assert!(solved.invalid_reasons(eid(2)).is_empty());
    }

    #[test]
    fn two_entities_with_one_alias_are_both_invalid() {
        let a = register_doc(1, DocKind::Spec, "docs/spec/a.md", Some("spec/auth"), 1);
        let b = register_doc(2, DocKind::Spec, "docs/spec/b.md", Some("spec/auth"), 2);
        let c = register_doc(3, DocKind::Spec, "docs/spec/c.md", Some("spec/other"), 3);
        let view = fold(&[a, b, c]);
        let alias: Alias = "spec/auth".parse().unwrap();
        assert_eq!(
            view.issues(),
            [ClassificationIssue::AliasCollision {
                alias: alias.clone(),
                entities: BTreeSet::from([eid(1), eid(2)]),
            }]
        );
        assert_eq!(
            view.invalid_reasons(eid(1)),
            vec![InvalidReason::AliasCollision {
                alias: alias.clone()
            }]
        );
        assert_eq!(
            view.invalid_reasons(eid(2)),
            vec![InvalidReason::AliasCollision { alias }]
        );
        assert!(view.invalid_reasons(eid(3)).is_empty());
    }

    #[test]
    fn items_fold_by_design_and_number() {
        use crate::domain::id::test_support::did;
        let reg = |entity: u128, design: u128, n: u32| {
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
        };
        let view = fold(&[reg(7, 10, 1), reg(5, 10, 1), reg(6, 10, 2), reg(8, 11, 1)]);
        assert_eq!(view.items().count(), 3);
        assert_eq!(
            view.item_id(did(10), ItemNumber::new(1).unwrap()),
            Some(ItemId::new(eid(5)))
        );
        assert_eq!(view.canonical(eid(7)), eid(5));
        assert!(view.item(eid(7)).is_some());
        assert_eq!(
            view.item(eid(7)).unwrap().duplicates(),
            &BTreeSet::from([eid(7)])
        );
    }

    // --- publication ---------------------------------------------------------

    #[test]
    fn folding_the_repo_store_alone_gives_the_private_classification() {
        // Private store: register, tag, own, rename, revise, publish.
        let r = register_doc(1, DocKind::Spec, P, Some("spec/auth"), 1);
        let t = add_tag(1, &[r.id()], "area:auth");
        let t2 = add_tag(1, &[t.id()], "wip");
        let untag = remove_tag(1, &[t2.id()], "wip");
        let o = owner_event(1, &[untag.id()], "alice");
        let m = moved(1, &[o.id()], P, "docs/spec/auth.md");
        let rev = revised(1, &[m.id()], 1, 2);
        let private_events = vec![r, t, t2, untag, o, m, rev];
        let private = fold(&private_events);
        let p = private.doc(eid(1)).unwrap();

        let snapshot = ClassSnapshot::new(
            p.kind().known().unwrap(),
            p.path().agreed().cloned().unwrap(),
            p.alias().agreed().cloned().unwrap(),
            p.owner().agreed().cloned().unwrap(),
            p.tags().clone(),
            p.version().agreed().cloned().unwrap(),
        );
        let root_in_repo = Event::new(
            eid(1),
            BTreeSet::new(),
            ts(),
            EventBody::Published(Publication::Incoming {
                from: StoreKind::Private,
                snapshot,
            }),
        )
        .unwrap();
        let repo = fold(std::slice::from_ref(&root_in_repo));
        let q = repo.doc(eid(1)).unwrap();
        assert_eq!(q.kind(), p.kind());
        assert_eq!(q.path(), p.path());
        assert_eq!(q.alias(), p.alias());
        assert_eq!(q.owner(), p.owner());
        assert_eq!(q.tags(), p.tags());
        assert_eq!(q.version(), p.version());
        assert_eq!(q.tips(), &BTreeSet::from([root_in_repo.id()]));

        // Later events in the repo store descend from the root and keep working.
        let more = add_tag(1, &[root_in_repo.id()], "published");
        let both = fold(&[root_in_repo.clone(), more]);
        assert_eq!(tags_of(&both, 1), ["area:auth", "published"]);
        // A removal after publication removes a tag that came from the snapshot.
        let rm = remove_tag(1, &[root_in_repo.id()], "area:auth");
        assert_eq!(tags_of(&fold(&[root_in_repo, rm]), 1), Vec::<String>::new());
    }

    #[test]
    fn the_outgoing_half_of_a_publication_changes_no_value() {
        let r = root(1);
        let out = Event::new(
            eid(1),
            BTreeSet::from([r.id()]),
            ts(),
            EventBody::Published(Publication::Outgoing {
                to: StoreKind::Repo,
            }),
        )
        .unwrap();
        let before = fold(std::slice::from_ref(&r));
        let after = fold(&[r, out]);
        let (b, a) = (before.doc(eid(1)).unwrap(), after.doc(eid(1)).unwrap());
        assert_eq!(
            (b.kind(), b.path(), b.alias(), b.owner(), b.tags()),
            (a.kind(), a.path(), a.alias(), a.owner(), a.tags())
        );
    }

    #[test]
    fn a_non_empty_vec_of_changes_is_required_to_reclassify() {
        assert!(NonEmptyVec::<ClassChange>::from_vec(vec![]).is_err());
    }
}
