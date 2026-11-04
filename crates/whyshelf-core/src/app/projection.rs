//! Projections: generated files as values.
//!
//! A projection is derived from the events and the files and is never a source of
//! truth: delete it and a rebuild writes it again. The use cases hand a projection
//! to the storage adapter as a value; turning it into JSON or Markdown, with the
//! generated-file marker, is the adapter's work (so this crate holds no serializer
//! and the layout of the bytes is tested where the bytes are made).
//!
//! # The map
//!
//! [`MapDoc`] answers, for a person or a tool reading one file, *what exists and
//! how it is related*: for each entity its kind, path, alias, title, tags (official
//! and extended) and references (the Findings a Spec stands on, the upstream of a
//! Design or an item, and references that point outside the store). It carries **no
//! derived state** (stale, approved): state changes at almost every commit and would
//! put noise in every diff. Every collection is ordered (`BTreeMap`/`BTreeSet`), so
//! the same model renders to the same bytes.
//!
//! The map of the repo store lists public entities only. A public entity that stands
//! on a private one shows it as the opaque `private:<id>` in `external`, with no
//! alias and no title. The private store has its own map, which may point at public
//! entities by their plain identifiers (private to public is free).
//!
//! # The status and the changelog
//!
//! The status Markdown is [`StatusReport`](super::view::StatusReport) (its public
//! part), the changelog Markdown is [`ChangelogDoc`]: one
//! [`Changelog`] per Finding and per Spec, from the events of
//! the store. Both are off unless the configuration asks for them.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::event::{EntityKind, StoreKind};
use crate::domain::id::{Alias, EntityId};
use crate::domain::location::RelPath;
use crate::domain::reference::Ref;
use crate::domain::tag::Tag;

use super::model::{Content, Model};
use super::view::Changelog;

/// The version of the map's shape.
pub const MAP_VERSION: u32 = 1;

/// The tags of an entity, by group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapTags {
    official: BTreeSet<Tag>,
    extended: BTreeSet<Tag>,
}

impl MapTags {
    /// The `key:value` tags.
    pub fn official(&self) -> &BTreeSet<Tag> {
        &self.official
    }

    /// The free labels.
    pub fn extended(&self) -> &BTreeSet<Tag> {
        &self.extended
    }
}

/// What an entity refers to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapRefs {
    grounds: BTreeSet<EntityId>,
    upstream: BTreeSet<EntityId>,
    external: BTreeSet<Ref>,
}

impl MapRefs {
    /// The Findings a Spec stands on.
    pub fn grounds(&self) -> &BTreeSet<EntityId> {
        &self.grounds
    }

    /// The Spec of a Design, or the Design of an item.
    pub fn upstream(&self) -> &BTreeSet<EntityId> {
        &self.upstream
    }

    /// References to entities outside this store, as opaque `private:<id>`.
    pub fn external(&self) -> &BTreeSet<Ref> {
        &self.external
    }
}

/// One entity in the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntity {
    kind: EntityKind,
    path: Option<RelPath>,
    alias: Option<Alias>,
    title: Option<String>,
    tags: MapTags,
    refs: MapRefs,
}

impl MapEntity {
    /// The kind.
    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    /// The path; none for an item.
    pub fn path(&self) -> Option<&RelPath> {
        self.path.as_ref()
    }

    /// The alias, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The title of a Finding (free text).
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The tags.
    pub fn tags(&self) -> &MapTags {
        &self.tags
    }

    /// The references.
    pub fn refs(&self) -> &MapRefs {
        &self.refs
    }
}

/// The map of one store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDoc {
    scope: StoreKind,
    entities: BTreeMap<EntityId, MapEntity>,
}

impl MapDoc {
    /// The version of the shape.
    pub fn version(&self) -> u32 {
        MAP_VERSION
    }

    /// The store the map is of.
    pub fn scope(&self) -> StoreKind {
        self.scope
    }

    /// The entities by identifier.
    pub fn entities(&self) -> &BTreeMap<EntityId, MapEntity> {
        &self.entities
    }

    /// Derive the map of the entities that live in `scope`.
    pub fn build(model: &Model, scope: StoreKind) -> MapDoc {
        // A reference stays plain if the target lives in this store, or if this is
        // the private store (private to public is free). Otherwise it is opaque.
        let plain = |target: EntityId| {
            scope == StoreKind::Private || model.home(target) == Some(StoreKind::Repo)
        };
        let mut entities = BTreeMap::new();
        for doc in model.docs().filter(|d| d.store() == scope) {
            let mut tags = MapTags::default();
            for tag in doc.tags() {
                if tag.is_official() {
                    tags.official.insert(tag.clone());
                } else {
                    tags.extended.insert(tag.clone());
                }
            }
            let mut refs = MapRefs::default();
            match doc.present().map(|p| p.content()) {
                Some(Content::Spec(spec)) => {
                    for ground in spec.grounds().iter().map(|g| g.entity()) {
                        if plain(ground) {
                            refs.grounds.insert(ground);
                        } else {
                            refs.external.insert(Ref::Private(ground));
                        }
                    }
                }
                Some(Content::Design(design)) => {
                    let spec = design.spec().entity();
                    if plain(spec) {
                        refs.upstream.insert(spec);
                    } else {
                        refs.external.insert(Ref::Private(spec));
                    }
                }
                _ => {}
            }
            entities.insert(
                doc.id(),
                MapEntity {
                    kind: doc.kind().entity_kind(),
                    path: Some(doc.path().clone()),
                    alias: doc.alias().cloned(),
                    title: doc.title().map(str::to_owned),
                    tags,
                    refs,
                },
            );
        }
        for item in model.items().filter(|i| i.store() == scope) {
            let mut refs = MapRefs::default();
            refs.upstream.insert(item.key().design.entity());
            entities.insert(
                item.key().id.entity(),
                MapEntity {
                    kind: EntityKind::Item,
                    path: None,
                    alias: None,
                    title: None,
                    tags: MapTags::default(),
                    refs,
                },
            );
        }
        MapDoc { scope, entities }
    }
}

/// The changelog of every Finding and Spec of one store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogDoc {
    scope: StoreKind,
    entries: BTreeMap<EntityId, Changelog>,
}

impl ChangelogDoc {
    /// The store the changelog is of.
    pub fn scope(&self) -> StoreKind {
        self.scope
    }

    /// One changelog per Finding and Spec.
    pub fn entries(&self) -> &BTreeMap<EntityId, Changelog> {
        &self.entries
    }

    /// Derive the changelog of the Findings and Specs that live in `scope`.
    pub fn build(model: &Model, scope: StoreKind) -> ChangelogDoc {
        let entries = model
            .docs()
            .filter(|d| d.store() == scope)
            .filter(|d| {
                matches!(
                    d.kind().entity_kind(),
                    EntityKind::Finding | EntityKind::Spec
                )
            })
            .map(|d| (d.id(), Changelog::of(model, d.id())))
            .collect();
        ChangelogDoc { scope, entries }
    }
}
