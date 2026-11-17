//! A document that is not written yet: its key, and whether it would be valid.
//!
//! Four questions, each asked of a built [`Shelf`] and answered here only (a run of
//! new documents is planned in the module `plan`, which asks the same ones):
//!
//! ```text
//!   Shelf::next_key(book)          ──► NewKey     the highest number any file of the book
//!                                                 writes, plus one (a name tail is not a
//!                                                 number); none for a book of slugs
//!   Shelf::claim_key(book, text)   ──► NewKey     the text read in the book's grammar,
//!                                                 and written by no file of the book
//!   key_after_taken(search)        ──► AfterTaken what to do when the file of the key was
//!                                                 there at the write: the next number, a
//!                                                 refusal, or giving up (read by
//!                                                 Shelf::replan_after_taken only)
//!   Shelf::check_new(id, key, ..)  ──► NewDoc     the would-be file read by the build's own
//!                                                 reader (Candidate::read, judge) and its
//!                                                 links resolved in an index of keys
//! ```
//!
//! A key is taken when any file of the book writes it in the book's grammar, a valid
//! document or not, its identity readable or not: a key that an invalid file holds is
//! never handed out again, so fixing that file later cannot make two documents of one
//! key. A file whose front matter cannot be read at all shows no key, and is not
//! counted (see the limits in the module page of [`domain::shelf`]). Such a file, or
//! a file another run wrote after the shelf was read, shows only when the new file is
//! written and is found to be there already: [`Shelf::replan_after_taken`] says what
//! comes of that for a key that was chosen (the next number) and for a key that was
//! asked for (nothing: it is refused).
//!
//! A document is made from its row by one function, which [`Shelf::check_new`] and
//! the plan of a run both call and which writes no second set of rules: the entries
//! go through the same steps a file on disk goes through in [`Shelf::build`], so what
//! it accepts is exactly what the build will accept when the file is read back. A
//! link to a key no document has is accepted as it is by the build (an unresolved
//! link is a state of a valid document), and [`NewDoc::unresolved`] lists it. An
//! entry the build reads as not written (`source=` for an optional field, a link
//! given no key) is not written: [`NewDoc::entries`], made by
//! [`Shelf::form_entries`], leaves it out, so that the file holds what the document
//! comes to. `fmt` asks the same entry point, so the two write one form.
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
    BookName, BookSchema, BookScope, EntryName, ID_ENTRY, KEY_ENTRY, KeyForm, KeyPattern,
    KeyProblem, KeyTail,
};

use super::plan::{NewRequest, PlannedValue};
use super::{
    Candidate, DocProblem, DocReading, FormEntries, Judged, KeyIndex, RawValue, Shelf, ask_of,
    resolve_artifacts, resolve_links,
};

/// A key a new document may take: read in its book's grammar and written by no file
/// of the book.
///
/// Only [`Shelf::next_key`], [`Shelf::claim_key`] and [`Shelf::plan_new`] (which
/// chooses a key and moves it on) make one, so [`Shelf::check_new`] cannot be handed
/// a key that is taken or malformed.
///
/// It keeps how it was got (chosen by the shelf as a number, or asked for), which only
/// [`Shelf::replan_after_taken`] reads: a key that was asked for is not chosen again
/// when its file turns out to be there, and one that was chosen is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKey {
    pub(super) key: DocKey,
    pub(super) origin: KeyOrigin,
}

/// How a [`NewKey`] was got. Private to the shelf's build: nothing outside it decides
/// by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum KeyOrigin {
    /// Chosen by the shelf as this number of a prefixed book.
    Next { number: u64 },
    /// Given by a person.
    Claimed,
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
    /// The file of the next key is one the range of the book does not take.
    #[error(transparent)]
    OutOfRange(Box<OutOfRange>),
}

/// A key whose file, `<book dir>/<key>.md`, the range of its book does not take
/// (`include`, `exclude`): the book would never read the document, so it is not
/// made. The answer names the patterns.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "the book `{book}` does not take `{path}`, the file of the key `{}`: {scope}",
    .key.form
)]
pub struct OutOfRange {
    /// The book.
    pub book: BookName,
    /// The key.
    pub key: DocKey,
    /// The file the document would be.
    pub path: RelPath,
    /// The range of the book.
    pub scope: BookScope,
}

/// The file a document of `key` would be, `<book dir>/<key>.md`: the one place it is
/// made, for the key steps and for [`Shelf::check_new`]. An error when that is no
/// path under the root (a slug with a `.` or `..` segment).
fn file_of(book: &BookSchema, key: &DocKey) -> Result<RelPath, ValueError> {
    format!("{}/{}.md", book.dir(), key.form).parse()
}

/// The key, refused when its file is a path the book's range does not take. A key
/// whose file is no path is let through: [`Shelf::check_new`] refuses it with why.
fn in_range(book: &BookSchema, key: DocKey) -> Result<DocKey, Box<OutOfRange>> {
    match file_of(book, &key) {
        Ok(path) if !book.takes(&path) => Err(Box::new(OutOfRange {
            book: book.name().clone(),
            key,
            path,
            scope: book.scope().clone(),
        })),
        _ => Ok(key),
    }
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
    /// The file of the key is one the range of the book does not take.
    #[error(transparent)]
    OutOfRange(Box<OutOfRange>),
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
    /// `<book dir>/<key>.md` is a path the range of the book does not take. The key
    /// steps refuse such a key first; this is the same rule, asked again of the file
    /// that is judged.
    #[error(transparent)]
    OutOfRange(Box<OutOfRange>),
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
    entries: FormEntries,
}

impl NewDoc {
    /// The document as the build makes it from the file (its file is
    /// [`Doc::origin`]).
    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    /// Every entry of the front matter to write, `id` and `key` included: the ones the
    /// request gave, but those the build read as not written and the file need not
    /// hold (a field written empty, a link with no key), which [`Shelf::form_entries`]
    /// leaves out and the document is the same without.
    pub fn entries(&self) -> &FormEntries {
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

/// How many keys one row tries, its first included, when the file of each key chosen
/// for it is there at the write; each row of a run counts its own, as one `new` does.
/// [`Shelf::replan_after_taken`] is the only reader that decides by it; a surface may
/// say it (the help of `new`).
pub const NEXT_KEY_TRIES: u32 = 64;

/// The key of the number `n` of a prefixed book, read in the book's grammar and taken
/// by its range: the one place a numbered key is made, for [`Shelf::next_key`], the
/// plan of a run and [`Shelf::replan_after_taken`]. The text goes through the
/// grammar, the one reader of a key, so a number too long for a key is refused by the
/// rule that refuses it anywhere.
pub(super) fn numbered(
    schema: &BookSchema,
    book: &BookName,
    n: u64,
) -> Result<NewKey, NextKeyProblem> {
    let KeyPattern::Prefixed(prefix) = schema.key() else {
        return Err(NextKeyProblem::SlugBook { book: book.clone() });
    };
    let form = schema
        .key()
        .parse(&format!("{prefix}-{n}"))
        .map_err(|problem| NextKeyProblem::Exhausted {
            book: book.clone(),
            highest: n.saturating_sub(1),
            problem,
        })?;
    let key = in_range(
        schema,
        DocKey {
            book: book.clone(),
            form,
        },
    )
    .map_err(NextKeyProblem::OutOfRange)?;
    Ok(NewKey {
        key,
        origin: KeyOrigin::Next { number: n },
    })
}

/// The number of a key of a prefixed book, when its tail is one (a name tail is no
/// number): what the highest key of a book is counted by, for the files of the shelf
/// and for the keys of a run.
pub(super) fn key_number(key: &DocKey) -> Option<u64> {
    match &key.form {
        KeyForm::Prefixed {
            tail: KeyTail::Number(n),
            ..
        } => Some(*n),
        KeyForm::Prefixed { .. } | KeyForm::Slug(_) => None,
    }
}

/// The search for a free key of one row of a run: the key the row began with, the one
/// it is at, how many keys the row has tried, and the highest number any key of the
/// run's book holds.
///
/// Made by [`Shelf::replan_after_taken`] from its plan, and moved on by
/// `key_after_taken` each time the file of the key is found to be there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KeySearch {
    pub(super) first: DocKey,
    pub(super) current: NewKey,
    pub(super) tries: u32,
    /// The highest number of a key of the plan in the book of `current`: the next key
    /// is counted from it, so a key that another row holds is not chosen again.
    pub(super) highest: u64,
}

/// What to do when the file of the key being written was there (`key_after_taken`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AfterTaken {
    /// Write the next key of the search.
    Next(KeySearch),
    /// The key was asked for, so it is refused as it is.
    Asked,
    /// Every key tried had a file: give up.
    NoFreeKey(NoFreeKey),
    /// The key chosen has no next one.
    NoNextKey(NextKeyProblem),
}

/// A search that tried every key it could and found a file at each: the keys from the
/// first one the row was planned at to the last.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "the book `{book}` had a file at every key from `{}` to `{}`",
    .first.form, .last.form
)]
pub struct NoFreeKey {
    /// The book.
    pub book: BookName,
    /// The first key tried.
    pub first: DocKey,
    /// The last key tried.
    pub last: DocKey,
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
        // A key's number has fewer digits than `u64::MAX`, so saturating is safe.
        numbered(schema, book, self.highest_written(book).saturating_add(1))
    }

    /// The highest number any file of the book writes, valid or not (0 when none): a
    /// name tail is no number. What [`Shelf::next_key`] and the plan of a run count
    /// from.
    pub(super) fn highest_written(&self, book: &BookName) -> u64 {
        self.written_keys
            .keys()
            .filter(|key| key.book == *book)
            .filter_map(key_number)
            .max()
            .unwrap_or(0)
    }

    /// What comes of a key whose file was there when it was written: a fact learnt
    /// after the shelf was read (a run beside this one wrote the file, or its front
    /// matter cannot be read, so the shelf shows no key for it).
    ///
    /// - A key that was asked for ([`Shelf::claim_key`]) is not chosen again:
    ///   [`AfterTaken::Asked`].
    /// - A key that was chosen ([`Shelf::next_key`]) is followed by the number after
    ///   the highest the run's keys of the book hold, read in the book's grammar and
    ///   range: [`AfterTaken::Next`], to be written in its turn, or
    ///   [`AfterTaken::NoNextKey`] when there is none.
    /// - The row gives up after [`NEXT_KEY_TRIES`] keys, its first included:
    ///   [`AfterTaken::NoFreeKey`], from the first key it was planned at to the last it
    ///   tried. The limit keeps a row from going on without end; as long as no more
    ///   runs than that write at one time, each of them finds a key.
    ///
    /// Read by [`Shelf::replan_after_taken`] only, which adds what a run of many rows
    /// asks of it. The shelf is not read again: the next number is counted from the
    /// keys of the plan, so a key it makes is free as the shelf knows it, as
    /// [`Shelf::next_key`]'s is.
    pub(super) fn key_after_taken(&self, search: KeySearch) -> AfterTaken {
        let KeySearch {
            first,
            current,
            tries,
            highest,
        } = search;
        let KeyOrigin::Next { number } = current.origin else {
            return AfterTaken::Asked;
        };
        if tries >= NEXT_KEY_TRIES {
            return AfterTaken::NoFreeKey(NoFreeKey {
                book: first.book.clone(),
                first,
                last: current.key,
            });
        }
        let book = current.key.book;
        let Some(schema) = self.schema.book(&book) else {
            return AfterTaken::NoNextKey(NextKeyProblem::UnknownBook { book });
        };
        match numbered(schema, &book, number.max(highest).saturating_add(1)) {
            Ok(next) => AfterTaken::Next(KeySearch {
                first,
                current: next,
                tries: tries + 1,
                highest: highest.max(number),
            }),
            Err(problem) => AfterTaken::NoNextKey(problem),
        }
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
        if self.written_keys.contains_key(&key) {
            return Err(ClaimKeyProblem::Taken { key });
        }
        let key = in_range(schema, key).map_err(ClaimKeyProblem::OutOfRange)?;
        Ok(NewKey {
            key,
            origin: KeyOrigin::Claimed,
        })
    }

    /// The files that write `key` in its book's grammar, valid or not, in the order
    /// of their paths: what makes the key taken ([`Shelf::claim_key`]). None for a
    /// key no file writes.
    pub fn files_writing(&self, key: &DocKey) -> Vec<RelPath> {
        self.written_keys
            .get(key)
            .map(|files| files.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// What the links to artifacts of the new documents of a run ask of the adapters,
    /// all rows together: what [`Shelf::plan_new`] is to be given the answers for, as
    /// [`Shelf::artifact_asks`] is for the build. A book the shelf does not declare
    /// asks nothing (the plan refuses it).
    pub fn new_artifact_asks(&self, requests: &[NewRequest]) -> ArtifactAsks {
        let mut asks = ArtifactAsks::new();
        for request in requests {
            let Some(book) = self.schema.book(&request.book) else {
                continue;
            };
            // The names of a link to artifacts are texts, never a name of a row.
            let entries: BTreeMap<String, RawValue> = request
                .entries
                .iter()
                .filter_map(|(name, value)| match value {
                    PlannedValue::Raw(raw) => Some((name.clone(), raw.clone())),
                    PlannedValue::Link(_) => None,
                })
                .collect();
            ask_of(book, &entries, &mut asks);
        }
        asks
    }

    /// Whether a document with this identity, key, entries and body would be valid,
    /// as the file `<book dir>/<key>.md`, with the answers `artifacts` of the adapters
    /// for what [`Shelf::new_artifact_asks`] asked.
    ///
    /// A document judged alone, against the shelf's own index of keys. A run of many
    /// is planned by [`Shelf::plan_new`], which makes each document by the same
    /// function (no second set of rules) against an index that holds the run too.
    /// The file it would be is read by the build's own reader and judged as the build
    /// judges it (fields and their types, required fields, enum words, links and
    /// their counts, names of links to artifacts), and its links are resolved in the
    /// index. `entries` are every entry but `id` and `key`, which come from `id` and
    /// `key`.
    // The problems are the answer, read once per request; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn check_new(
        &self,
        id: DocId,
        key: &NewKey,
        entries: BTreeMap<String, RawValue>,
        body: String,
        artifacts: &ArtifactReadings,
    ) -> Result<NewDoc, NewDocProblem> {
        self.new_doc_in(&self.index, id, key, entries, body, artifacts)
    }

    /// The document of a row, judged as [`Shelf::check_new`] says, its links resolved
    /// in `index`: the one place a [`NewDoc`] is made, for `check_new` (the shelf's
    /// own index) and for the plan of a run (the shelf's with the run's documents
    /// added, and again when a key is chosen again).
    // The problems are the answer, read once per request; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub(super) fn new_doc_in(
        &self,
        index: &KeyIndex,
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
        let origin = file_of(book, &key.key).map_err(|source| NewDocProblem::PathInvalid {
            key: key.key.clone(),
            source,
        })?;
        // The rule the key steps asked, asked again of the file judged, so that the
        // answer names the range and not "under no book".
        in_range(book, key.key.clone()).map_err(NewDocProblem::OutOfRange)?;
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
                let links = resolve_links(index, links);
                let artifact_links = resolve_artifacts(artifacts, artifact_links);
                Ok(NewDoc {
                    doc: Doc::new(draft, links, artifact_links),
                    entries: Self::form_entries(&self.schema, &key.key.book, &entries),
                })
            }
            Judged::Invalid { file, .. } => Err(NewDocProblem::Invalid(file.problems)),
        }
    }
}
