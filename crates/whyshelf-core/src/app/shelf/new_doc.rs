//! The use case that writes new documents: `new_docs`, a run of new files, and
//! `new_doc`, the run of one (the other writes: [`judge`](super::ShelfApp::judge)
//! adds to the ledger, and [`fmt`](super::ShelfApp::fmt) writes documents' files that
//! are there over in the form, through a port of its own,
//! [`DocReplace`](super::ports::DocReplace)).
//!
//! ```text
//!   requests --- load, once ----------------------------> the shelf as it is
//!   entries ---- the book's declared types --------------> PlannedValues (a translation only)
//!   a new id each, then every name of a link to artifacts of every row
//!            --- Shelf::new_artifact_asks ----------------> ArtifactSource::resolve, once
//!   all rows --- Shelf::plan_new -------------------------> a NewPlan: every key, name and
//!                                                            judgement, or every PlanProblem
//!   NewPlan ---- RunWrites::doc (DocWrite::create, create-only), in order
//!      |                                                   -> <book dir>/<key>.md, each
//!      |-- a file was there (a run beside this one, or a file whose front matter
//!      |   cannot be read): Shelf::replan_after_taken -> the same place written again
//!      |   with another key, or a refusal, or giving up
//!      '-- the end of the run (RunWrites::run) -> FileSync::persist, once, every file
//! ```
//!
//! Nothing here decides a key, the validity of a document, what a name stands for, or
//! whether a key that was found taken is chosen again: [`Shelf::plan_new`] and
//! [`Shelf::replan_after_taken`] say, and this use case only does what they say. The
//! one thing done here is the translation of what a person typed into the values a
//! front matter holds, by what the book declares for the name ([`planned_value`]):
//! a text (`name=value`) is read as a list field or a link reads it, `a,b` as a list
//! (each item trimmed, empty items dropped), a truth value `true` / `false`, a whole
//! number its digits; a truth value or a whole number given as such is kept; a list of
//! items is the items of a link to documents (each a key, or the name of a row of the
//! run) or of a list field. A text that does not fit stays a text and the domain says
//! it does not fit; a name the book does not declare is a text too, and the domain
//! says it is no entry of the book.
//!
//! An identity is taken for each row after its entries are read and before the plan,
//! because a key is decided in the plan: a run that is refused writes nothing and
//! shows no identity it took. The identities, the entries and the answers of the
//! adapters do not depend on the keys, so a key chosen again is judged on what was
//! already made: nothing is read or asked again, and an identity that no file holds
//! is used once.
//!
//! Every write of the run is made inside [`RunWrites::run`], which keeps the files
//! written and puts them on disk together, once, when the run ends: an answer of
//! success has every file on disk. A write that fails after a file of the run was
//! written stops the run, and the answer says what was written
//! ([`ShelfError::PartWay`]; the files written stay, each whole, and were put on disk
//! before it was given). Every way the run stops is a [`Stop`](super::run_writes::Stop).
//! When only the putting on disk fails, the answer is [`ShelfError::NotDurable`].

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptyVec, TwoOrMore};
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{
    LinkItem, NewPlan, NewRequest, PlanAfterTaken, PlanProblem, PlannedValue, RawValue, RunName,
    Shelf,
};
use crate::domain::shelf::doc::{DocKey, FieldValue, UnresolvedLink};
use crate::domain::shelf::schema::{BookName, BookSchema, EntryName, FieldType, ScalarType};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::load;
use super::ports::{ArtifactSource, IdSource, ShelfPort, ShelfWrite, port};
use super::run_writes::{RunWrites, Stop, Wrote};

/// What a request gives for one entry of a new document, before the book's
/// declaration says what it is: a surface that reads text (the command line) gives a
/// [`Text`](Self::Text), one that reads typed values (a JSON line) the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryInput {
    /// No value, as a reader of a document gave it (`key:`, `null`): given as it was
    /// read, with no meaning of its own, so that the domain decides what it comes to
    /// (an entry that says nothing, which `new` does not write).
    Null,
    /// A text, read by the type the book declares for the entry: as typed on a
    /// command line (`a,b` is a list for a list field or a link, `true` a truth
    /// value, `3` a whole number).
    Text(String),
    /// A truth value, as it is.
    Bool(bool),
    /// A whole number, as it is.
    Integer(i64),
    /// A list of items: of a link to documents, each a key or the name of a row of
    /// the run; of a list field or a link to artifacts, texts only.
    Items(Vec<ItemInput>),
}

/// One item of a list given for an entry ([`EntryInput::Items`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemInput {
    /// A text: a key in a link to documents, as it is in a list field or a link to
    /// artifacts.
    Text(String),
    /// The name of a row of the run: only an item of a link to documents can be one.
    Named(String),
}

/// A request for a new document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDocRequest {
    /// The number the caller gives the row, which every problem of it carries: the
    /// line of the file for a run read from one (blank lines counted), 1 for a single
    /// document.
    pub row: usize,
    /// The book.
    pub book: BookName,
    /// The key, as typed; `None` is the book's next key.
    pub key: Option<String>,
    /// The name the other rows of the run link to this one by; not written to the
    /// file. `None` is no name.
    pub name: Option<String>,
    /// The entries of the front matter as given, in order (`id` and `key` are
    /// whyshelf's). A list field or a link reads a text as items split at commas.
    pub entries: Vec<(String, EntryInput)>,
    /// Items of a list field or a link, `name` and one item each, as typed (commas
    /// included, white space around it left out), in order: each is appended after
    /// the items of the entry of that name, if any. An item for a field that is no
    /// list is refused; an item of nothing is given to the domain like any other,
    /// which refuses it as an item that says nothing.
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

/// What a run of new documents wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMany {
    /// The documents, in the order they were written.
    pub docs: Vec<Created>,
    /// Those of `docs` that no other of them stands on through a `Depends` link
    /// ([`NewPlan::heads`]), in the same order: named to `judge approve --pending`,
    /// they take every document of the run and nothing else.
    pub heads: Vec<DocKey>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Write a run of new documents, each valid for its book, at
    /// `<book dir>/<key>.md`, the shelf read once and the artifacts asked once.
    ///
    /// All of them, or none: a key that is taken or malformed, a book with no next
    /// key, entries a book refuses, a name that stands for no row, and every other
    /// refusal of [`Shelf::plan_new`] is an error that lists every row refused
    /// ([`ShelfError::Plan`]), and nothing is written. A fault of the request itself
    /// (an entry given twice, an item for no list, an empty name, a name given
    /// where no document is linked) is told for every row that has one
    /// ([`ShelfError::Rows`] for two or more, else the row's own error), and the run
    /// is not planned. A file is never replaced.
    ///
    /// A row with no key whose key, when it is written, is found to have a file
    /// (another run wrote it after the shelf was read, or its front matter cannot be
    /// read) is not an error: [`Shelf::replan_after_taken`] gives it the next key, and
    /// runs at once in one working tree each write documents of their own. What it
    /// cannot mend is told as [`ShelfError::FileTaken`] (a key that was asked for),
    /// [`ShelfError::NoFreeKey`], [`ShelfError::NextKey`] and [`ShelfError::Stuck`],
    /// each as [`ShelfError::PartWay`] when a file of the run was written before it.
    pub async fn new_docs(&mut self, requests: Vec<NewDocRequest>) -> Result<NewMany, ShelfError> {
        let loaded = load(&self.ports).await?;
        let shelf = &loaded.shelf;
        let plan = plan_of(&self.ports, shelf, requests).await?;
        RunWrites::run(&self.ports, |writes| {
            Box::pin(write_plan(writes, shelf, plan))
        })
        .await
    }

    /// Write one new document, valid for its book, at `<book dir>/<key>.md`: the run
    /// of one row ([`ShelfApp::new_docs`]), told as it always was.
    ///
    /// A key that is taken or malformed, a book with no next key, entries the book
    /// refuses and a file already at the path of a key that was asked for are each
    /// an error, and nothing is written. A file is never replaced.
    ///
    /// When no key is given, a file found at the key chosen, when it is written
    /// (another run wrote it after the shelf was read, or its front matter cannot be
    /// read), is not an error: the next key is written instead
    /// ([`Shelf::replan_after_taken`]), so runs at once in one working tree each write
    /// a document of their own. When every key tried had a file the error is
    /// [`ShelfError::NoFreeKey`].
    pub async fn new_doc(&mut self, request: NewDocRequest) -> Result<Created, ShelfError> {
        let many = self.new_docs(vec![request]).await.map_err(single_refusal)?;
        many.docs
            .into_iter()
            .next()
            .ok_or(ShelfError::NothingRequested)
    }
}

/// The writes of a plan, in order, inside the run `writes` is of: a document whose
/// path has a file is planned again by [`Shelf::replan_after_taken`], and what it
/// cannot mend stops the run ([`Stop`]), as every other thing that does.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
async fn write_plan<P: ShelfWrite>(
    writes: &mut RunWrites<'_, P>,
    shelf: &Shelf,
    mut plan: NewPlan,
) -> Result<NewMany, Stop> {
    let mut docs: Vec<Created> = Vec::with_capacity(plan.len());
    let mut at = 0;
    while let Some(new) = plan.doc_at(at) {
        let taken = match writes.doc(new).await? {
            Wrote::Created(created) => {
                docs.push(created);
                at += 1;
                continue;
            }
            Wrote::AlreadyThere(path) => path,
        };
        let cause = match shelf.replan_after_taken(plan, at) {
            PlanAfterTaken::Replanned(next) => {
                plan = next;
                continue;
            }
            PlanAfterTaken::Asked => ShelfError::FileTaken {
                path: taken.clone(),
            },
            PlanAfterTaken::Stuck { row, written_row } => ShelfError::Stuck { row, written_row },
            PlanAfterTaken::Refused { row, problem } => ShelfError::Plan(NonEmptyVec::new(
                PlanProblem::Doc { row, problem },
                Vec::new(),
            )),
            PlanAfterTaken::NoFreeKey(problem) => ShelfError::NoFreeKey(problem),
            PlanAfterTaken::NoNextKey(problem) => ShelfError::NextKey(problem),
        };
        return Err(Stop {
            failed: taken,
            cause,
        });
    }
    let heads = plan.heads();
    Ok(NewMany { docs, heads })
}

/// The plan of a run of requests on `shelf`: the entries of each row read by its
/// book ([`planned_value`]), an identity for each, one answer of the adapters for the
/// names every row's links to artifacts write, and then [`Shelf::plan_new`]. The steps
/// of `new`, which `init` takes too.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
pub(super) async fn plan_of<P: ShelfWrite>(
    ports: &P,
    shelf: &Shelf,
    requests: Vec<NewDocRequest>,
) -> Result<NewPlan, ShelfError> {
    let mut rows = Vec::with_capacity(requests.len());
    let mut faults = Vec::new();
    for request in requests {
        let row = request.row;
        match request_of(shelf, request) {
            Ok(request) => rows.push(request),
            Err(cause) => faults.push(in_row(row, cause)),
        }
    }
    // A fault of the request is told for every row that has one, and the run is not
    // planned: what the plan would find in the rows that are whole waits for the
    // request to be mended.
    let mut faults = faults.into_iter();
    if let Some(first) = faults.next() {
        return Err(match faults.next() {
            None => first,
            Some(second) => {
                ShelfError::Rows(Box::new(TwoOrMore::new(first, second, faults.collect())))
            }
        });
    }
    let mut ids = Vec::with_capacity(rows.len());
    for _ in &rows {
        ids.push(
            ports
                .ids()
                .new_doc_id()
                .await
                .map_err(port(ShelfPort::Ids))?,
        );
    }
    // The names its links to artifacts write are asked of their adapters as the
    // load asks those of every document, so a text that is no name of its space
    // is refused here as the build would refuse it.
    let asks = shelf.new_artifact_asks(&rows);
    let artifacts = if asks.is_empty() {
        ArtifactReadings::new()
    } else {
        ports
            .artifacts()
            .resolve(&asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?
    };
    shelf
        .plan_new(rows.into_iter().zip(ids).collect(), &artifacts)
        .map_err(ShelfError::Plan)
}

/// A fault of a row that has no number of its own, told with the row it is of. One
/// that carries its row already is told as it is.
fn in_row(row: usize, cause: ShelfError) -> ShelfError {
    match cause {
        ShelfError::NameOutsideLink { .. } => cause,
        cause => ShelfError::InRow {
            row,
            cause: Box::new(cause),
        },
    }
}

/// What a run of one row is refused for, told as `new` told it before runs of many
/// were written: the problem of the row, with no row. A run refused for more than one
/// thing, or for a name, is told as the run it is.
pub(super) fn single_refusal(error: ShelfError) -> ShelfError {
    match error {
        ShelfError::InRow { cause, .. } => *cause,
        ShelfError::Plan(problems) if problems.len() == 1 => match problems.first() {
            PlanProblem::Doc { problem, .. } => ShelfError::NewDoc(problem.clone()),
            PlanProblem::ClaimKey { problem, .. } => ShelfError::ClaimKey(problem.clone()),
            PlanProblem::NextKey { problem, .. } => ShelfError::NextKey(problem.clone()),
            PlanProblem::KeyTwice { .. }
            | PlanProblem::NameTwice { .. }
            | PlanProblem::UnknownName { .. }
            | PlanProblem::NameOtherBook { .. } => ShelfError::Plan(problems),
        },
        other => other,
    }
}

/// The request of a row for the plan: its entries read by its book, and its name.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
fn request_of(shelf: &Shelf, request: NewDocRequest) -> Result<NewRequest, ShelfError> {
    let name = request
        .name
        .map(RunName::new)
        .transpose()
        .map_err(|_| ShelfError::EmptyName)?;
    let entries = typed_entries(
        shelf.schema().book(&request.book),
        request.row,
        request.entries,
        request.items,
    )?;
    Ok(NewRequest {
        row: request.row,
        book: request.book,
        key: request.key,
        name,
        entries,
        body: request.body,
    })
}

/// The entries as values, by what the book declares for each name (the translation
/// of the module page). A book the shelf does not know translates every entry as a
/// text; the key step refuses it later.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
fn typed_entries(
    schema: Option<&BookSchema>,
    row: usize,
    inputs: Vec<(String, EntryInput)>,
    items: Vec<(String, String)>,
) -> Result<BTreeMap<String, PlannedValue>, ShelfError> {
    let mut entries = BTreeMap::new();
    for (name, input) in inputs {
        let value = planned_value(schema, row, &name, input)?;
        if entries.insert(name.clone(), value).is_some() {
            return Err(ShelfError::EntryTwice { name });
        }
    }
    for (name, item) in items {
        let kind = entry_kind(schema, &name);
        if !matches!(
            kind,
            EntryKind::DocLink | EntryKind::ArtifactLink | EntryKind::TextList
        ) {
            return Err(ShelfError::ItemOfNoList { name });
        }
        // As an item split from `a, b` is: white space around it does not count. An
        // item of nothing is not judged here: it goes on to the domain, which says
        // an item that says nothing is a problem of the document.
        let item = item.trim().to_owned();
        let first = if kind == EntryKind::DocLink {
            PlannedValue::Link(vec![LinkItem::Key(item.clone())])
        } else {
            PlannedValue::Raw(RawValue::List(vec![item.clone()]))
        };
        match entries.entry(name) {
            Entry::Vacant(vacant) => {
                vacant.insert(first);
            }
            Entry::Occupied(mut occupied) => match occupied.get_mut() {
                PlannedValue::Raw(RawValue::List(list)) => list.push(item),
                PlannedValue::Link(links) => links.push(LinkItem::Key(item)),
                // An entry given as a value that is no list (a truth value, a whole
                // number; `planned_value` reads a text of a list field or a link as a
                // list) and again as items is given twice: neither replaces the other.
                PlannedValue::Raw(_) => {
                    return Err(ShelfError::EntryTwice {
                        name: occupied.key().clone(),
                    });
                }
            },
        }
    }
    Ok(entries)
}

/// How a book reads one entry: the one judgement of what its declaration makes of a
/// name, made in [`entry_kind`] and nowhere else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    /// A link to documents.
    DocLink,
    /// A link to artifacts.
    ArtifactLink,
    /// A list of texts.
    TextList,
    /// A truth value.
    Bool,
    /// A whole number.
    Integer,
    /// A text, a line or an enum, a name the book does not declare, or a book the shelf does
    /// not know (`book` is `None`): all read as a text.
    Text,
}

/// How `book` reads the entry `name`.
fn entry_kind(book: Option<&BookSchema>, name: &str) -> EntryKind {
    let Some(book) = book else {
        return EntryKind::Text;
    };
    if book.links().contains_key(name) {
        return EntryKind::DocLink;
    }
    if book.artifact_links().contains_key(name) {
        return EntryKind::ArtifactLink;
    }
    match book.fields().get(name).map(|decl| &decl.ty) {
        Some(FieldType::Scalar(ScalarType::TextList)) => EntryKind::TextList,
        Some(FieldType::Scalar(ScalarType::Bool)) => EntryKind::Bool,
        Some(FieldType::Scalar(ScalarType::Integer)) => EntryKind::Integer,
        Some(FieldType::Scalar(ScalarType::Text | ScalarType::Line) | FieldType::Enum(_))
        | None => EntryKind::Text,
    }
}

/// What a value given for the entry `name` of `row` is, by the type its book declares
/// (`book` is `None` for a book the shelf does not know, which reads every text as a
/// text): the one reading of an input.
///
/// - [`EntryInput::Text`] is read as a person typed it ([`typed`]).
/// - [`EntryInput::Null`] is kept as it is ([`RawValue::Null`]): what it says is the
///   domain's to tell.
/// - [`EntryInput::Bool`] and [`EntryInput::Integer`] are kept as they are.
/// - [`EntryInput::Items`] are the items of a link to documents, each a key or the
///   name of a row; for any other entry they are a list of texts, and a name among
///   them is [`ShelfError::NameOutsideLink`]. A text is trimmed as an item of
///   `name+=text` is; one of nothing goes on to the domain (an item that says
///   nothing is its problem), and a name of nothing is [`ShelfError::EmptyItem`].
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
pub(super) fn planned_value(
    book: Option<&BookSchema>,
    row: usize,
    name: &str,
    input: EntryInput,
) -> Result<PlannedValue, ShelfError> {
    match input {
        EntryInput::Null => Ok(PlannedValue::Raw(RawValue::Null)),
        EntryInput::Bool(b) => Ok(PlannedValue::Raw(RawValue::Bool(b))),
        EntryInput::Integer(n) => Ok(PlannedValue::Raw(RawValue::Integer(n))),
        EntryInput::Text(text) => Ok(typed(entry_kind(book, name), &text)),
        EntryInput::Items(items) => {
            // As an item of `name+=text` is: white space around a text does not
            // count.
            let text_of = |text: String| text.trim().to_owned();
            if entry_kind(book, name) == EntryKind::DocLink {
                let mut linked = Vec::with_capacity(items.len());
                for item in items {
                    linked.push(match item {
                        ItemInput::Text(key) => LinkItem::Key(text_of(key)),
                        ItemInput::Named(named) => {
                            LinkItem::Named(RunName::new(named).map_err(|_| {
                                ShelfError::EmptyItem {
                                    name: name.to_owned(),
                                }
                            })?)
                        }
                    });
                }
                return Ok(PlannedValue::Link(linked));
            }
            let mut texts = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    ItemInput::Text(text) => texts.push(text_of(text)),
                    ItemInput::Named(_) => {
                        return Err(ShelfError::NameOutsideLink {
                            row,
                            entry: name.to_owned(),
                        });
                    }
                }
            }
            Ok(PlannedValue::Raw(RawValue::List(texts)))
        }
    }
}

/// One text as the value its declaration asks for, or as the text it is when it does
/// not fit.
fn typed(kind: EntryKind, text: &str) -> PlannedValue {
    let as_text = || PlannedValue::Raw(RawValue::Text(text.to_owned()));
    // A text of nothing is no truth value or number: it stays a text, and the domain
    // tells that it says nothing.
    let read = |scalar| match NonBlank::new(text)
        .ok()
        .and_then(|said| FieldValue::read(&FieldType::Scalar(scalar), &said))
    {
        Some(FieldValue::Bool(b)) => PlannedValue::Raw(RawValue::Bool(b)),
        Some(FieldValue::Integer(n)) => PlannedValue::Raw(RawValue::Integer(n)),
        _ => as_text(),
    };
    match kind {
        EntryKind::DocLink => {
            PlannedValue::Link(list(text).into_iter().map(LinkItem::Key).collect())
        }
        EntryKind::ArtifactLink | EntryKind::TextList => {
            PlannedValue::Raw(RawValue::List(list(text)))
        }
        EntryKind::Bool => read(ScalarType::Bool),
        EntryKind::Integer => read(ScalarType::Integer),
        EntryKind::Text => as_text(),
    }
}

/// `a, b,` as `[a, b]`.
fn list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}
