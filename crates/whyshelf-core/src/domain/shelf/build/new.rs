//! A document that is not written yet: its key, and whether it would be valid.
//!
//! Three questions, each asked of a built [`Shelf`] and answered here only:
//!
//! ```text
//!   Shelf::next_key(book)          ──► NewKey   the highest number any file of the book
//!                                               writes, plus one (a name tail is not a
//!                                               number); none for a book of slugs
//!   Shelf::claim_key(book, text)   ──► NewKey   the text read in the book's grammar,
//!                                               and written by no file of the book
//!   Shelf::check_new(id, key, ..)  ──► NewDoc   the would-be file read by the build's own
//!                                               reader (Candidate::read, judge) and its
//!                                               links resolved in the shelf's index
//! ```
//!
//! A key is taken when any file of the book writes it in the book's grammar, a valid
//! document or not, its identity readable or not: a key that an invalid file holds is
//! never handed out again, so fixing that file later cannot make two documents of one
//! key. A file whose front matter cannot be read at all shows no key, and is not
//! counted (see the limits in the module page of [`domain::shelf`]).
//!
//! [`Shelf::check_new`] writes no second set of rules: the entries go through the
//! same steps a file on disk goes through in [`Shelf::build`], so what it accepts is
//! exactly what the build will accept when the file is read back. A link to a key no
//! document has is accepted as it is by the build (an unresolved link is a state of a
//! valid document), and [`NewDoc::unresolved`] lists it.
//!
//! [`domain::shelf`]: crate::domain::shelf

use std::collections::BTreeMap;

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{ArtifactAsks, ArtifactReadings};
use crate::domain::shelf::doc::{Doc, DocKey, LinkState, UnresolvedLink};
use crate::domain::shelf::schema::{
    BookName, EntryName, ID_ENTRY, KEY_ENTRY, KeyForm, KeyPattern, KeyProblem, KeyTail,
};

use super::{
    Candidate, DocProblem, DocReading, Judged, RawValue, Shelf, ask_of, resolve_artifacts,
    resolve_links,
};

/// A key a new document may take: read in its book's grammar and written by no file
/// of the book.
///
/// Only [`Shelf::next_key`] and [`Shelf::claim_key`] make one, so
/// [`Shelf::check_new`] cannot be handed a key that is taken or malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKey {
    key: DocKey,
}

impl NewKey {
    /// The key, with its book.
    pub fn key(&self) -> &DocKey {
        &self.key
    }
}

/// Why a book has no next key.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NextKeyProblem {
    /// The book is not declared.
    #[error("there is no book `{book}`")]
    UnknownBook {
        /// The book named.
        book: BookName,
    },
    /// The keys of the book are slugs, which have no order: a key must be given.
    #[error("the keys of `{book}` are slugs, so there is no next one: give the key")]
    SlugBook {
        /// The book.
        book: BookName,
    },
    /// The number after the highest one is longer than a key's number may be.
    #[error("the book `{book}` has no number left after {highest}")]
    Exhausted {
        /// The book.
        book: BookName,
        /// The highest number written.
        highest: u64,
        /// Why the grammar refuses the next one.
        #[source]
        problem: KeyProblem,
    },
}

/// Why a key given for a new document cannot be its key.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaimKeyProblem {
    /// The book is not declared.
    #[error("there is no book `{book}`")]
    UnknownBook {
        /// The book named.
        book: BookName,
    },
    /// The text does not fit the grammar of the book's keys.
    #[error("`{written}` is not a key of the book `{book}`")]
    Malformed {
        /// The book.
        book: BookName,
        /// The text given.
        written: String,
        /// Why the grammar refuses it.
        #[source]
        problem: KeyProblem,
    },
    /// A file of the book already writes the key (valid or not).
    #[error("the key `{key}` is taken")]
    Taken {
        /// The key.
        key: DocKey,
    },
}

/// Why a new document cannot be made of what was given.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NewDocProblem {
    /// The book of the key is not declared in this shelf's schema.
    #[error("there is no book `{book}`")]
    UnknownBook {
        /// The book.
        book: BookName,
    },
    /// An entry the front matter keeps for itself (`id`, `key`) was given: the new
    /// document's identity and key are not the caller's to write.
    #[error("`{name}` is set by whyshelf and cannot be given")]
    ReservedEntry {
        /// The entry.
        name: &'static str,
    },
    /// `<book dir>/<key>.md` is not a path under the root (a slug with a `.` or `..`
    /// segment).
    #[error("`{key}` gives no file path under the book")]
    PathInvalid {
        /// The key.
        key: DocKey,
        /// Why the path is refused.
        #[source]
        source: ValueError,
    },
    /// The document would be invalid: every problem the build would find.
    #[error(
        "the new document would be invalid: {}",
        .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    )]
    Invalid(NonEmptyVec<DocProblem>),
}

/// A new document that would be valid: what to write, and the document the build
/// will make of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDoc {
    doc: Doc,
    entries: BTreeMap<String, RawValue>,
}

impl NewDoc {
    /// The document as the build makes it from the file (its file is
    /// [`Doc::origin`]).
    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    /// Every entry of the front matter to write, `id` and `key` included.
    pub fn entries(&self) -> &BTreeMap<String, RawValue> {
        &self.entries
    }

    /// The keys written in its links that no valid document answers to now, by link:
    /// accepted, as the build accepts them, and worth telling.
    pub fn unresolved(&self) -> Vec<(&EntryName, &UnresolvedLink)> {
        self.doc
            .links()
            .iter()
            .flat_map(|(name, link)| {
                link.states().iter().filter_map(move |state| match state {
                    LinkState::Unresolved(unresolved) => Some((name, unresolved)),
                    LinkState::Resolved(_) => None,
                })
            })
            .collect()
    }
}

impl Shelf {
    /// The next key of a prefixed book: one more than the highest number any file of
    /// the book writes, valid or not, so that a key a file writes is not handed out
    /// again (a file whose front matter cannot be read writes no key that is seen);
    /// `<PREFIX>-1` when there is none. A key with a name tail (`FR-LATENCY`) is no
    /// number and is not counted. A book of slugs has no next key.
    ///
    /// The one place the next key is decided.
    pub fn next_key(&self, book: &BookName) -> Result<NewKey, NextKeyProblem> {
        let schema = self
            .schema
            .book(book)
            .ok_or_else(|| NextKeyProblem::UnknownBook { book: book.clone() })?;
        let KeyPattern::Prefixed(prefix) = schema.key() else {
            return Err(NextKeyProblem::SlugBook { book: book.clone() });
        };
        let highest = self
            .written_keys
            .iter()
            .filter(|key| key.book == *book)
            .filter_map(|key| match &key.form {
                KeyForm::Prefixed {
                    tail: KeyTail::Number(n),
                    ..
                } => Some(*n),
                KeyForm::Prefixed { .. } | KeyForm::Slug(_) => None,
            })
            .max()
            .unwrap_or(0);
        // The text goes through the book's grammar, the one reader of a key, so a
        // number too long for a key is refused by the rule that refuses it anywhere
        // (a key's number has fewer digits than `u64::MAX`, so saturating is safe).
        let next = highest.saturating_add(1);
        let form = schema
            .key()
            .parse(&format!("{prefix}-{next}"))
            .map_err(|problem| NextKeyProblem::Exhausted {
                book: book.clone(),
                highest,
                problem,
            })?;
        Ok(NewKey {
            key: DocKey {
                book: book.clone(),
                form,
            },
        })
    }

    /// A key a person gave for a new document: read in the book's grammar, and
    /// refused when a file of the book already writes it, valid or not.
    ///
    /// The one place a given key is accepted for a new document.
    pub fn claim_key(&self, book: &BookName, written: &str) -> Result<NewKey, ClaimKeyProblem> {
        let schema = self
            .schema
            .book(book)
            .ok_or_else(|| ClaimKeyProblem::UnknownBook { book: book.clone() })?;
        let form = schema
            .key()
            .parse(written)
            .map_err(|problem| ClaimKeyProblem::Malformed {
                book: book.clone(),
                written: written.to_owned(),
                problem,
            })?;
        let key = DocKey {
            book: book.clone(),
            form,
        };
        if self.written_keys.contains(&key) {
            return Err(ClaimKeyProblem::Taken { key });
        }
        Ok(NewKey { key })
    }

    /// What the links to artifacts of a new document of `key`'s book with these
    /// entries ask of the adapters: what [`Shelf::check_new`] is to be given the
    /// answers for, as [`Shelf::artifact_asks`] is for the build. A book the shelf
    /// does not declare asks nothing (`check_new` refuses it).
    pub fn new_artifact_asks(
        &self,
        key: &NewKey,
        entries: &BTreeMap<String, RawValue>,
    ) -> ArtifactAsks {
        let mut asks = ArtifactAsks::new();
        if let Some(book) = self.schema.book(&key.key.book) {
            ask_of(book, entries, &mut asks);
        }
        asks
    }

    /// Whether a document with this identity, key, entries and body would be valid,
    /// as the file `<book dir>/<key>.md`, with the answers `artifacts` of the adapters
    /// for what [`Shelf::new_artifact_asks`] asked.
    ///
    /// The one place a new document is judged, and no second set of rules: the file
    /// it would be is read by the build's own reader and judged as the build judges
    /// it (fields and their types, required fields, enum words, links and their
    /// counts, names of links to artifacts), and its links are resolved in this
    /// shelf's index. `entries` are every entry but `id` and `key`, which come from
    /// `id` and `key`.
    // The problems are the answer, read once per request; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn check_new(
        &self,
        id: DocId,
        key: &NewKey,
        mut entries: BTreeMap<String, RawValue>,
        body: String,
        artifacts: &ArtifactReadings,
    ) -> Result<NewDoc, NewDocProblem> {
        for name in [ID_ENTRY, KEY_ENTRY] {
            if entries.contains_key(name) {
                return Err(NewDocProblem::ReservedEntry { name });
            }
        }
        let book = self
            .schema
            .book(&key.key.book)
            .ok_or_else(|| NewDocProblem::UnknownBook {
                book: key.key.book.clone(),
            })?;
        let origin: RelPath = format!("{}/{}.md", book.dir(), key.key.form)
            .parse()
            .map_err(|source| NewDocProblem::PathInvalid {
                key: key.key.clone(),
                source,
            })?;
        entries.insert(ID_ENTRY.to_owned(), RawValue::Text(id.to_string()));
        entries.insert(
            KEY_ENTRY.to_owned(),
            RawValue::Text(key.key.form.to_string()),
        );
        let reading = DocReading::Parsed {
            origin,
            entries: entries.clone(),
            body,
        };
        match Candidate::read(&self.schema, reading, artifacts).judge() {
            Judged::Valid {
                draft,
                links,
                artifact_links,
            } => {
                let links = resolve_links(&self.index, links);
                let artifact_links = resolve_artifacts(artifacts, artifact_links);
                Ok(NewDoc {
                    doc: Doc::new(draft, links, artifact_links),
                    entries,
                })
            }
            Judged::Invalid { file, .. } => Err(NewDocProblem::Invalid(file.problems)),
        }
    }
}
