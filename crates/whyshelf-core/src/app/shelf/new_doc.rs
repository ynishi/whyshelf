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
//! ```
//!
//! Nothing here decides a key or the validity of a document. The one thing done here
//! is the translation of what a person typed (`name=value`, all text) into the
//! values a front matter holds, by what the book declares for the name: a list field
//! or a link takes `a,b` as a list (each item trimmed, empty items dropped), a truth
//! value `true` / `false`, a whole number its digits. A text that does not fit stays
//! a text and the domain says it does not fit; a name the book does not declare is a
//! text too, and the domain says it is no entry of the book.
//!
//! The identity is fetched only after the key and the entries are known, and the
//! write is the last step, so a refused document uses no identity and writes nothing.

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{NewDoc, RawValue, Shelf};
use crate::domain::shelf::doc::{DocKey, UnresolvedLink};
use crate::domain::shelf::schema::{BookName, BookSchema, EntryName, FieldType};

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
    /// refuses and a file already at the path are each an error, and nothing is
    /// written. A file is never replaced.
    pub async fn new_doc(&mut self, request: NewDocRequest) -> Result<Created, ShelfError> {
        let loaded = load(&self.ports).await?;
        let new = checked_new(&self.ports, &loaded.shelf, request).await?;
        write_new(&self.ports, &new).await
    }
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
    Ok(shelf.check_new(id, &key, entries, request.body, &artifacts)?)
}

/// Write a judged document's file, create-only: a file already at its path is
/// [`ShelfError::FileTaken`], and nothing is written.
pub(super) async fn write_new<P: ShelfWrite>(
    ports: &P,
    new: &NewDoc,
) -> Result<Created, ShelfError> {
    let doc = new.doc();
    let written = ports
        .doc_writer()
        .create(doc.origin(), new.entries(), doc.body())
        .await
        .map_err(port(ShelfPort::DocWrite))?;
    match written {
        Creation::Created => Ok(Created {
            id: doc.id(),
            key: doc.key().clone(),
            file: doc.origin().clone(),
            unresolved: new
                .unresolved()
                .into_iter()
                .map(|(link, u)| (link.clone(), u.clone()))
                .collect(),
        }),
        Creation::AlreadyThere => Err(ShelfError::FileTaken {
            path: doc.origin().clone(),
        }),
    }
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
            Some(FieldType::TextList)
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
        Some(FieldType::TextList) => list(text),
        Some(FieldType::Bool) => match text {
            "true" => RawValue::Bool(true),
            "false" => RawValue::Bool(false),
            _ => as_text(),
        },
        Some(FieldType::Integer) => text.parse().map_or_else(|_| as_text(), RawValue::Integer),
        Some(FieldType::Text | FieldType::Enum(_)) | None => as_text(),
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
