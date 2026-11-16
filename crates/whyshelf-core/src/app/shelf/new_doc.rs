//! The use case that writes a new document: `new_doc`, one new file (the other
//! writes: [`judge`](super::ShelfApp::judge) adds to the ledger, and
//! [`fmt`](super::ShelfApp::fmt) writes documents' files that are there over in the
//! form, through a port of its own, [`DocReplace`](super::ports::DocReplace)).
//!
//! ```text
//!   request ---- load -------------------------------> the shelf as it is
//!   book, key? -- Shelf::claim_key / next_key -------> a NewKey (free, in the grammar)
//!   texts  ---- the book's declared types ----------> RawValues (a translation only)
//!   entries -- Shelf::new_artifact_asks ------------> ArtifactSource::resolve
//!   new id ---- Shelf::check_new -------------------> a NewDoc, or every DocProblem
//!   NewDoc ---- DocWrite::create (create-only) -----> <book dir>/<key>.md
//!      |
//!      '-- a file was there (a run beside this one, or a file whose front matter
//!          cannot be read): Shelf::key_after_taken -> the next key, same shelf,
//!          same id, same answers of the adapters, judged again -> written again
//!          (a key that was asked for is refused; after NEXT_KEY_TRIES keys,
//!          NoFreeKey)
//! ```
//!
//! Nothing here decides a key or the validity of a document, nor whether a key that
//! was found taken is chosen again: [`Shelf::key_after_taken`] says, and this use case
//! only does what it says. The one thing done here is the translation of what a
//! person typed (`name=value`, all text) into the
//! values a front matter holds, by what the book declares for the name: a list field
//! or a link takes `a,b` as a list (each item trimmed, empty items dropped), a truth
//! value `true` / `false`, a whole number its digits. A text that does not fit stays
//! a text and the domain says it does not fit; a name the book does not declare is a
//! text too, and the domain says it is no entry of the book.
//!
//! The identity is fetched only after the key and the entries are known, and the
//! write is the last step, so a refused document uses no identity and writes nothing.
//! The identity, the entries and the answers of the adapters do not depend on the key
//! of the book, so a key chosen again is judged on what was already made: nothing is
//! read or asked again, and an identity that no file holds is used once.
//!
//! [`Shelf::key_after_taken`]: crate::domain::shelf::build::Shelf::key_after_taken

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{AfterTaken, KeySearch, NewDoc, NewKey, RawValue, Shelf};
use crate::domain::shelf::doc::{DocKey, FieldValue, UnresolvedLink};
use crate::domain::shelf::schema::{BookName, BookSchema, EntryName, FieldType, ScalarType};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::load;
use super::ports::{ArtifactSource, Creation, DocWrite, IdSource, ShelfPort, ShelfWrite, port};

/// A request for a new document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDocRequest {
    /// The book.
    pub book: BookName,
    /// The key, as typed; `None` is the book's next key.
    pub key: Option<String>,
    /// The entries of the front matter as typed, `name` and text, in order (`id` and
    /// `key` are whyshelf's). A list field or a link reads its text as items split
    /// at commas.
    pub entries: Vec<(String, String)>,
    /// Items of a list field or a link, `name` and one item each, as typed (commas
    /// included, white space around it left out), in order: each is appended after
    /// the items of the entry of that name, if any. An item for a field that is no
    /// list, and an item of nothing, are refused.
    pub items: Vec<(String, String)>,
    /// The body.
    pub body: String,
}

/// The document that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// Its identity.
    pub id: DocId,
    /// Its key.
    pub key: DocKey,
    /// Its file, relative to the project root.
    pub file: RelPath,
    /// The keys written in its links that no valid document answers to now, with the
    /// link: accepted, as the build accepts them.
    pub unresolved: Vec<(EntryName, UnresolvedLink)>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Write one new document, valid for its book, at `<book dir>/<key>.md`.
    ///
    /// A key that is taken or malformed, a book with no next key, entries the book
    /// refuses and a file already at the path of a key that was asked for are each
    /// an error, and nothing is written. A file is never replaced.
    ///
    /// When no key is given, a file found at the key chosen, when it is written
    /// (another run wrote it after the shelf was read, or its front matter cannot be
    /// read), is not an error: the next key is written instead
    /// ([`Shelf::key_after_taken`]), so runs at once in one working tree each write
    /// a document of their own. When every key tried had a file the error is
    /// [`ShelfError::NoFreeKey`].
    pub async fn new_doc(&mut self, request: NewDocRequest) -> Result<Created, ShelfError> {
        let loaded = load(&self.ports).await?;
        let shelf = &loaded.shelf;
        let (key, parts) = parts_of(&self.ports, shelf, request).await?;
        let mut search = KeySearch::start(key);
        loop {
            let new = checked_with_key(shelf, search.key(), &parts)?;
            let path = match write_new(&self.ports, &new).await? {
                Written::Created(created) => return Ok(created),
                Written::AlreadyThere(path) => path,
            };
            match shelf.key_after_taken(search) {
                AfterTaken::Next(next) => search = next,
                AfterTaken::Asked => return Err(ShelfError::FileTaken { path }),
                AfterTaken::NoFreeKey(problem) => return Err(ShelfError::NoFreeKey(problem)),
                AfterTaken::NoNextKey(problem) => return Err(ShelfError::NextKey(problem)),
            }
        }
    }
}

/// What a request makes that does not depend on the key of its book: the entries
/// read by the book, a new identity, the body, and the answers of the adapters for the
/// names its links to artifacts write.
struct Parts {
    id: DocId,
    entries: BTreeMap<String, RawValue>,
    body: String,
    artifacts: ArtifactReadings,
}

/// The document a request makes on `shelf`, judged and not written: its key, its
/// entries read by its book, a new identity, and the answers of the adapters for
/// the names its links to artifacts write. The steps of `new`, which `init` takes
/// too, for each document it makes.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
pub(super) async fn checked_new<P: ShelfWrite>(
    ports: &P,
    shelf: &Shelf,
    request: NewDocRequest,
) -> Result<NewDoc, ShelfError> {
    let (key, parts) = parts_of(ports, shelf, request).await?;
    checked_with_key(shelf, &key, &parts)
}

/// The key of a request and the rest of what its document is made of, whatever the
/// key ends up as: the key (asked for, or the next one), the entries, a new identity
/// (taken once), and the one answer of the adapters.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
async fn parts_of<P: ShelfWrite>(
    ports: &P,
    shelf: &Shelf,
    request: NewDocRequest,
) -> Result<(NewKey, Parts), ShelfError> {
    let key = match &request.key {
        Some(text) => shelf.claim_key(&request.book, text)?,
        None => shelf.next_key(&request.book)?,
    };
    let entries = typed_entries(shelf, &request.book, request.entries, request.items)?;
    let id = ports
        .ids()
        .new_doc_id()
        .await
        .map_err(port(ShelfPort::Ids))?;
    // The names its links to artifacts write are asked of their adapters as the
    // load asks those of every document, so a text that is no name of its space
    // is refused here as the build would refuse it.
    let asks = shelf.new_artifact_asks(&key, &entries);
    let artifacts = if asks.is_empty() {
        ArtifactReadings::new()
    } else {
        ports
            .artifacts()
            .resolve(&asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?
    };
    Ok((
        key,
        Parts {
            id,
            entries,
            body: request.body,
            artifacts,
        },
    ))
}

/// The document of `parts` at `key`, judged and not written. Asks no port: the one
/// place a document at a key is judged, for the first key and each one after it.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
fn checked_with_key(shelf: &Shelf, key: &NewKey, parts: &Parts) -> Result<NewDoc, ShelfError> {
    Ok(shelf.check_new(
        parts.id,
        key,
        parts.entries.clone(),
        parts.body.clone(),
        &parts.artifacts,
    )?)
}

/// What the write of a judged document came to.
pub(super) enum Written {
    /// The file was made.
    Created(Created),
    /// A file was already at the path, and nothing was written.
    AlreadyThere(RelPath),
}

/// Write a judged document's file, create-only: a file already at its path is
/// [`Written::AlreadyThere`], and nothing is written. The one place a [`Created`]
/// is made.
pub(super) async fn write_new<P: ShelfWrite>(
    ports: &P,
    new: &NewDoc,
) -> Result<Written, ShelfError> {
    let doc = new.doc();
    let written = ports
        .doc_writer()
        .create(doc.origin(), new.entries(), doc.body())
        .await
        .map_err(port(ShelfPort::DocWrite))?;
    Ok(match written {
        Creation::Created => Written::Created(Created {
            id: doc.id(),
            key: doc.key().clone(),
            file: doc.origin().clone(),
            unresolved: new
                .unresolved()
                .into_iter()
                .map(|(link, u)| (link.clone(), u.clone()))
                .collect(),
        }),
        Creation::AlreadyThere => Written::AlreadyThere(doc.origin().clone()),
    })
}

/// The entries as values, by what the book declares for each name (the translation
/// of the module page). A book the shelf does not know translates every entry as a
/// text; the key step has refused it before.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
fn typed_entries(
    shelf: &Shelf,
    book: &BookName,
    texts: Vec<(String, String)>,
    items: Vec<(String, String)>,
) -> Result<BTreeMap<String, RawValue>, ShelfError> {
    let schema = shelf.schema().book(book);
    let mut entries = BTreeMap::new();
    for (name, text) in texts {
        let value = schema.map_or_else(
            || RawValue::Text(text.clone()),
            |schema| typed(schema, &name, &text),
        );
        if entries.insert(name.clone(), value).is_some() {
            return Err(ShelfError::EntryTwice { name });
        }
    }
    for (name, item) in items {
        if !schema.is_some_and(|schema| is_list(schema, &name)) {
            return Err(ShelfError::ItemOfNoList { name });
        }
        // As an item split from `a, b` is: white space around it does not count,
        // and an item of nothing is no item.
        let item = item.trim().to_owned();
        if item.is_empty() {
            return Err(ShelfError::EmptyItem { name });
        }
        match entries
            .entry(name)
            .or_insert_with(|| RawValue::List(Vec::new()))
        {
            RawValue::List(list) => list.push(item),
            // `typed` reads every entry of a list field or a link as a list.
            other => *other = RawValue::List(vec![item]),
        }
    }
    Ok(entries)
}

/// Whether the book reads the entry `name` as a list: a list field or a link.
fn is_list(book: &BookSchema, name: &str) -> bool {
    book.links().contains_key(name)
        || book.artifact_links().contains_key(name)
        || matches!(
            book.fields().get(name).map(|decl| &decl.ty),
            Some(FieldType::Scalar(ScalarType::TextList))
        )
}

/// One text as the value its declaration asks for, or as the text it is when it does
/// not fit.
fn typed(book: &BookSchema, name: &str, text: &str) -> RawValue {
    let as_text = || RawValue::Text(text.to_owned());
    let is_link = book.links().contains_key(name) || book.artifact_links().contains_key(name);
    if is_link {
        return list(text);
    }
    match book.fields().get(name).map(|decl| &decl.ty) {
        Some(FieldType::Scalar(ScalarType::TextList)) => list(text),
        Some(ty @ FieldType::Scalar(ScalarType::Bool | ScalarType::Integer)) => {
            match FieldValue::read(ty, text) {
                Some(FieldValue::Bool(b)) => RawValue::Bool(b),
                Some(FieldValue::Integer(n)) => RawValue::Integer(n),
                _ => as_text(),
            }
        }
        Some(FieldType::Scalar(ScalarType::Text) | FieldType::Enum(_)) | None => as_text(),
    }
}

/// `a, b,` as `[a, b]`.
fn list(text: &str) -> RawValue {
    RawValue::List(
        text.split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}
