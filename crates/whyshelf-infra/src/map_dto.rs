//! The shape of `map.json`, and its JSON Schema.
//!
//! The map answers, for a person or a tool that reads one file, what exists and
//! how it is related: for each entity its kind, path, alias, title, tags and
//! references. It carries no derived state (stale, approved), which changes at
//! almost every commit and would put noise in every diff.
//!
//! The types here are the version-1 shape. Every collection is a `BTreeMap` or a
//! sorted list, so one map always renders to the same bytes (see
//! [`crate::export`]). The JSON Schema is generated from these very types with
//! `schemars`, so the schema and the output cannot drift apart: it is written
//! next to the map as `map.schema.json` and printed by `whyshelf schema map`.
//! When the shape changes, `version` goes up and the schema of the old version is
//! kept.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use whyshelf_core::app::projection::{MAP_VERSION, MapDoc, MapEntity};

use crate::export::render_json;

/// The map of one store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "whyshelf map")]
pub struct MapV1 {
    /// The marker of a generated file; whyshelf writes it, and it is the first key
    /// of the file. It is in the schema so that a reader knows what it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<GeneratedV1>,
    /// The version of this shape.
    pub version: u32,
    /// The entities by identifier (UUID v7).
    pub entities: BTreeMap<String, MapEntityV1>,
}

/// The marker of a generated file (see [`crate::export`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedV1 {
    /// Says that the file is generated and must not be edited.
    pub note: String,
    /// The `export` hash of the file without this marker.
    pub content: String,
}

/// One entity of the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapEntityV1 {
    /// `finding`, `spec`, `design` or `item`.
    pub kind: String,
    /// The path of the document, relative to the project root; none for an item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The alias, if the entity has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// The title of a Finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The tags, by group.
    pub tags: MapTagsV1,
    /// What the entity refers to.
    pub refs: MapRefsV1,
}

/// The tags of an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapTagsV1 {
    /// `key:value` tags.
    pub official: Vec<String>,
    /// Free labels.
    pub extended: Vec<String>,
}

/// The references of an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapRefsV1 {
    /// The Findings a Spec stands on.
    pub grounds: Vec<String>,
    /// The Spec of a Design, or the Design of an item.
    pub upstream: Vec<String>,
    /// References to entities outside this store, as opaque `private:<id>`.
    pub external: Vec<String>,
}

fn sorted<T: ToString>(items: impl IntoIterator<Item = T>) -> Vec<String> {
    let mut texts: Vec<String> = items.into_iter().map(|item| item.to_string()).collect();
    texts.sort();
    texts
}

fn entity_of(entity: &MapEntity) -> MapEntityV1 {
    MapEntityV1 {
        kind: entity.kind().name().to_owned(),
        path: entity.path().map(ToString::to_string),
        alias: entity.alias().map(ToString::to_string),
        title: entity.title().map(str::to_owned),
        tags: MapTagsV1 {
            official: sorted(entity.tags().official()),
            extended: sorted(entity.tags().extended()),
        },
        refs: MapRefsV1 {
            grounds: sorted(entity.refs().grounds()),
            upstream: sorted(entity.refs().upstream()),
            external: sorted(entity.refs().external()),
        },
    }
}

impl MapV1 {
    /// The version-1 shape of a map.
    pub fn of(doc: &MapDoc) -> Self {
        debug_assert_eq!(doc.version(), MAP_VERSION);
        Self {
            generated: None,
            version: doc.version(),
            entities: doc
                .entities()
                .iter()
                .map(|(id, entity)| (id.to_string(), entity_of(entity)))
                .collect(),
        }
    }
}

/// The JSON Schema of the map as deterministic JSON text (the content of
/// `map.schema.json`).
pub fn schema_json() -> Result<String, serde_json::Error> {
    let schema = schemars::schema_for!(MapV1);
    Ok(render_json(&serde_json::to_value(&schema)?))
}
