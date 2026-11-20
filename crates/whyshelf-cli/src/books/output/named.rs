//! How an answer names a document and an artifact: a document is `{id, key}`
//! ([`DocRef`]), written with the keys of one report, and a document or an artifact an
//! origin names is wrapped as [`DocNamed`] or [`ArtifactNamed`].

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::shelf::artifact::ArtifactKey;

use super::Namer;

/// A document named in an answer: its identity, and its key when it is valid.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocRef {
    /// The identity (UUID v7).
    pub id: String,
    /// The key (`book:KEY`), when the document is valid.
    pub key: Option<String>,
}

/// A document an origin names, in `stale` and in `stats`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocNamed {
    /// The document.
    pub doc: DocRef,
}

/// An artifact an origin names, in `stale` and in `stats`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ArtifactNamed {
    /// Its name (kept from when every name was a path; the same as `name`).
    pub path: String,
    /// Its name.
    pub name: String,
    /// Its space.
    pub space: String,
}

impl ArtifactNamed {
    pub(super) fn of(artifact: &ArtifactKey) -> Self {
        Self {
            path: artifact.name().to_string(),
            name: artifact.name().to_string(),
            space: artifact.space().to_string(),
        }
    }
}

impl Namer<'_> {
    pub(super) fn doc(&self, id: DocId) -> DocRef {
        DocRef {
            id: id.to_string(),
            key: self.0.key_of(id).map(ToString::to_string),
        }
    }
}
