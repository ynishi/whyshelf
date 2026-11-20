//! The answers of `new` and `new --from`: the documents written, and the keys of
//! their links that cannot be followed yet.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{Created, NewMany};

use super::DocRef;

/// A key written in a link of a new document that no valid document answers to now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnresolvedOut {
    /// The link.
    pub link: String,
    /// The key as written.
    pub written: String,
    /// Why it cannot be followed.
    pub why: String,
}

/// The answer of `new`: the document written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedOut {
    /// The document.
    pub doc: DocRef,
    /// Its file, relative to the project root.
    pub file: String,
    /// The keys of its links that cannot be followed yet (the document is valid with
    /// them; each is a notice).
    pub unresolved_links: Vec<UnresolvedOut>,
}

/// The answer of `new --from`: the documents written, in the order they were
/// written, and the ones no other of them stands on.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NewManyOut {
    /// The documents, each as `new` tells one.
    pub docs: Vec<CreatedOut>,
    /// The keys (`book:KEY`) of those of `docs` that no other of them stands on
    /// through a `depends` link, in the same order: named to `judge approve
    /// --pending`, they take every document of the run and nothing else.
    pub heads: Vec<String>,
}

/// The answer of `new --from`.
pub fn new_many(many: &NewMany) -> NewManyOut {
    NewManyOut {
        docs: many.docs.iter().map(created).collect(),
        heads: many.heads.iter().map(ToString::to_string).collect(),
    }
}

/// The answer of `new`.
pub fn created(created: &Created) -> CreatedOut {
    CreatedOut {
        doc: DocRef {
            id: created.id.to_string(),
            key: Some(created.key.to_string()),
        },
        file: created.file.to_string(),
        unresolved_links: created
            .unresolved
            .iter()
            .map(|(link, unresolved)| UnresolvedOut {
                link: link.to_string(),
                written: unresolved.written().to_owned(),
                why: unresolved.to_string(),
            })
            .collect(),
    }
}
