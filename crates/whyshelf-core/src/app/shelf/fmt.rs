//! The use cases that put documents' files in the one form documents are written
//! in: [`fmt`](ShelfApp::fmt), which rewrites them, and
//! [`fmt_check`](ShelfApp::fmt_check), which only says which are not in it.
//!
//! ```text
//!   BookSchemaSource -> the dirs of the books
//!   DropAsk -> DropNames::new(&Schema) ........ the names to drop, checked once
//!   DocSource::forms -> per file: its reading, its bytes, the line of a comment
//!        |
//!        v  plan (here, per file), in this order
//!   unparsed, unheld ......................................... left alone
//!   Shelf::form_entries -> the entries the form holds        (the same as `new`'s),
//!                          and the names it dropped
//!   DocSource::render   -> the text in the form, and what it reads as
//!   the bytes are the text ................................. counted, not written
//!   a comment in the front matter ............................ left alone
//!   DocSource::replaceable: read-only, or a hard link ........ left alone
//!   the text reads as the entries the form holds ......... DocReplace::replace_read
//!   the text reads as anything else .......................... left alone
//! ```
//!
//! The promise: `fmt` writes a file only when the text in the form reads back as the
//! file does without the entries that say nothing and that the build reads as not
//! written (an optional field written empty, a link whose count admits none written
//! with no key), and without the entries it was asked to drop (see below). Which
//! entries those are is decided by the entry point `new`
//! asks too ([`Shelf::form_entries`]), so what `new` writes is in the form, and `fmt`
//! rewrites a file that holds such an entry without it. An entry that says nothing
//! and that the build tells a problem for (a required field, a link whose count asks
//! for a key, an undeclared entry, the `id` and the `key`) stays in the form, so the
//! rewrite does not change what `check` says. The one thing a rewrite changes in it is
//! the unknown entry of each name `fmt` was asked to drop, which goes; a `fmt` asked to
//! drop nothing changes none.
//!
//! # Dropping a name
//!
//! `fmt --drop NAME`, once for each name, is for a field, a link or a link to artifacts
//! taken out of a book's declaration: every document that held it tells an unknown
//! entry in `check`, and nothing else takes it out of them. [`DropNames::new`] checks
//! the names against the declaration once, after it is read: `id`, `key` and a name
//! every book declares are a [`ShelfError::Drop`], and nothing is written. Then
//! `form_entries` leaves out an entry of such a name from a file of a book that
//! declares nothing under it, whatever the entry holds; an entry of that name in a book
//! that declares it stays. Every other safeguard is as it was: a file with a comment, a
//! read-only file, a hard link and a file whose text would not read back as the entries
//! the form holds are left alone, so the entry stays in them and `check` still tells
//! it. What a file reads back as is compared with the entries the form holds, so an
//! entry dropped is not among them and is checked like the rest. `fmt` changes no hash:
//! what a document's hash covers is what the declaration reads, and it changed when the
//! declaration stopped reading the field, not when the entry goes.
//! [`FmtReport::dropped`] gives, for each file listed as rewritten or as not in the form,
//! the names dropped from it, so `fmt_check` shows the files and the names a rewrite
//! would take out without writing anything.
//!
//! The form is the adapter's (its text is never read here): the whole text after
//! the normalization rule, [`normalize`](crate::domain::shelf::doc::normalize), so a
//! body's line endings and trailing white space are normalized too, as every reading
//! already normalizes them. What is decided here is whether a file may be written in
//! it: only when the text in the form reads back as exactly the entries the form
//! holds and the body of the file now. A document's hash is taken over its book,
//! fields and body as read, which an entry the build reads as not written is not
//! part of, so such a rewrite changes no hash and no approval. The check is made on
//! every file, on every run; a file that fails it is left as it is and listed with
//! why, and the others are still written. A file left alone is not in the form
//! either: the answer lists it apart from the files `fmt` would write, and
//! `fmt --check` fails on both.
//!
//! The files are written one at a time, each replaced whole. A port that fails part
//! way leaves the first of them written, each a sound file of its own in the form,
//! and the rest as they were.
//!
//! Whether a file may be written in the form is decided in one place, [`form_write`],
//! which takes the entries to write: `fmt` gives the entries a file has, and
//! [`rekey`](super::rekey) the entries it has with a new key, so a file `rekey` writes
//! is held to the same rule and is refused for the same reasons.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::shelf::build::{
    DocReading, DropAsk, DropNames, FormEntries, RawProblem, RawValue, Shelf, UnheldValue,
};
use crate::domain::shelf::schema::Schema;

use super::ShelfApp;
use super::error::ShelfError;
use super::load::schema_of;
use super::ports::{
    DocReplace, DocSource, FileRead, Replaceable, Replacement, ShelfPort, ShelfRead, ShelfWrite,
    port,
};
use super::state::Side;

/// The answer of [`fmt`](ShelfApp::fmt) and [`fmt_check`](ShelfApp::fmt_check), the
/// files in the order the documents are read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FmtReport {
    /// The files written in the form; none for `fmt_check`.
    pub rewritten: Vec<RelPath>,
    /// The files that were in the form already, not written.
    pub in_form: Vec<RelPath>,
    /// The files not in the form that would be written in it; for `fmt`, none
    /// (they are `rewritten`).
    pub not_in_form: Vec<RelPath>,
    /// The files left as they are, each with why.
    pub left_alone: Vec<LeftAlone>,
    /// For each file in `rewritten` (or, for `fmt_check`, in `not_in_form`) that loses
    /// an entry it was asked to drop, the names it loses; a file that loses none is not
    /// here. Empty when no name was asked.
    pub dropped: Vec<Dropped>,
}

/// The names of the entries a rewritten file loses because `fmt` was asked to drop
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// The file.
    pub file: RelPath,
    /// The names, each once, in the order of the names.
    pub names: BTreeSet<NonBlank>,
}

impl FmtReport {
    /// Whether every file is in the form: none `fmt` would write, and none it leaves
    /// alone (such a file is not in the form either). What `fmt --check` exits 1 on
    /// when false.
    pub fn all_in_form(&self) -> bool {
        self.not_in_form.is_empty() && self.left_alone.is_empty()
    }
}

/// A file `fmt` leaves as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftAlone {
    /// The file.
    pub file: RelPath,
    /// Why.
    pub why: LeftAloneWhy,
}

/// Why a file is left as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftAloneWhy {
    /// Its front matter cannot be read: there is nothing to write it from.
    Unparsed(RawProblem),
    /// Its front matter is a mapping, and holds values of kinds a front matter does
    /// not hold: there is nothing to write them from.
    Unheld(NonEmptyVec<UnheldValue>),
    /// Its front matter holds a comment, which the form would lose; at this line.
    Comment {
        /// The line of the file, counted from 1.
        line: u32,
    },
    /// Its text in the form would not read back as the entries the form holds.
    ReadsOtherwise,
    /// The user cannot write the file.
    ReadOnly,
    /// The file has more than one link (a hard link), which a file written in its
    /// place would not keep.
    Linked,
    /// The file changed, or went, between the reading and the write.
    Changed,
}

/// The reason as a sentence, for an error that names a file that cannot be written in
/// the form (`rekey`).
impl fmt::Display for LeftAloneWhy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparsed(problem) => write!(f, "its front matter cannot be read: {problem}"),
            Self::Unheld(values) => write!(
                f,
                "its front matter cannot be read: {}",
                UnheldValue::told_all(values)
            ),
            Self::Comment { line } => write!(
                f,
                "its front matter holds a comment on line {line}, which the form would lose"
            ),
            Self::ReadsOtherwise => {
                f.write_str("its text in the form would not read as the file does")
            }
            Self::ReadOnly => f.write_str("it cannot be written (read-only)"),
            Self::Linked => f.write_str(
                "it has more than one link (a hard link), which a file written in its place would not keep",
            ),
            Self::Changed => f.write_str("it changed after it was read"),
        }
    }
}

/// What to do with one file.
enum Plan {
    InForm(RelPath),
    Rewrite {
        file: RelPath,
        read: Vec<u8>,
        text: Vec<u8>,
        /// The names the rewrite takes out (none unless `--drop`).
        dropped: BTreeSet<NonBlank>,
    },
    LeaveAlone(LeftAlone),
}

impl Dropped {
    /// The names a rewrite takes out of `file`, as the report says them: none for a
    /// file that loses none.
    fn of(file: &RelPath, names: BTreeSet<NonBlank>) -> Option<Self> {
        (!names.is_empty()).then(|| Dropped {
            file: file.clone(),
            names,
        })
    }
}

/// A file left as it is, and why.
fn leave(file: &RelPath, why: LeftAloneWhy) -> Plan {
    Plan::LeaveAlone(LeftAlone {
        file: file.clone(),
        why,
    })
}

/// A file's bytes as they were read, and the line of a comment in its front matter:
/// what [`form_write`] holds the text in the form against.
#[derive(Debug, Clone, Copy)]
pub(super) struct Held<'a> {
    /// The file the bytes are of.
    pub(super) file: &'a RelPath,
    /// The bytes now.
    pub(super) read: &'a [u8],
    /// The line of the first comment of its front matter, when it holds one.
    pub(super) comment: Option<u32>,
}

/// The text of a document in the form, as the adapter renders it, and what it reads as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Written {
    /// The text in the form.
    pub(super) text: Vec<u8>,
    /// What the text reads as, read as the file it is written as.
    pub(super) reads_as: DocReading,
    /// The entries the text holds, the ones `DocWrite::create` writes it from, with
    /// the names it takes out ([`FormEntries::dropped`]).
    pub(super) form: FormEntries,
}

/// What the rule of the form says of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FormWrite {
    /// The text in the form is the file as it is.
    InForm(Written),
    /// The file may be written as this text.
    Write(Written),
    /// The file is to be left as it is, and why.
    Leave(LeftAloneWhy),
}

/// The one rule of whether a document may be written in the form, for `fmt`, which
/// writes the entries a file has, and for `rekey`, which writes others (module page).
///
/// `held` is the file the bytes are of; `origin` the file the text is written as (the
/// same file for `fmt`, the new file of a document `rekey` moves). The decisions, in
/// this order: the file's book takes `origin`; the text in the form of `entries` and
/// `body`; it is the bytes now ([`FormWrite::InForm`]); the front matter holds a
/// comment; the file may be written ([`DocSource::replaceable`]); the text reads back
/// as exactly the entries the form holds and `body`.
pub(super) async fn form_write<D: DocSource>(
    schema: &Schema,
    drop: &DropNames,
    docs: &D,
    held: Held<'_>,
    origin: &RelPath,
    entries: &BTreeMap<String, RawValue>,
    body: &str,
) -> Result<FormWrite, ShelfError> {
    // A file the books do not take is not read for `fmt`; were one handed over, it is
    // not written.
    let Some(book) = schema.book_of(origin) else {
        return Ok(FormWrite::Leave(LeftAloneWhy::ReadsOtherwise));
    };
    let form = Shelf::form_entries(schema, book.name(), entries, drop);
    let rendered = docs.render(origin, &form, body);
    let written = Written {
        text: rendered.text,
        reads_as: rendered.reads_as,
        form,
    };
    if written.text == held.read {
        return Ok(FormWrite::InForm(written));
    }
    if let Some(line) = held.comment {
        return Ok(FormWrite::Leave(LeftAloneWhy::Comment { line }));
    }
    match docs
        .replaceable(held.file)
        .await
        .map_err(port(ShelfPort::Docs))?
    {
        Replaceable::Yes => {}
        Replaceable::ReadOnly => return Ok(FormWrite::Leave(LeftAloneWhy::ReadOnly)),
        Replaceable::Linked => return Ok(FormWrite::Leave(LeftAloneWhy::Linked)),
    }
    let wanted = DocReading::Parsed {
        origin: origin.clone(),
        entries: written.form.as_map().clone(),
        body: body.to_owned(),
    };
    Ok(if written.reads_as == wanted {
        FormWrite::Write(written)
    } else {
        FormWrite::Leave(LeftAloneWhy::ReadsOtherwise)
    })
}

/// A file whose front matter was read: its file, its entries and its body.
pub(super) type Parsed = (RelPath, BTreeMap<String, RawValue>, String);

/// The file, entries and body of a reading whose front matter was read; else the file
/// as it is left alone, and why: there is nothing to write it from.
pub(super) fn parsed(schema: &Schema, reading: DocReading) -> Result<Parsed, LeftAlone> {
    match reading {
        DocReading::Parsed {
            origin,
            entries,
            body,
        } => Ok((origin, entries, body)),
        DocReading::Unparsed { origin, why, .. } => {
            let why = LeftAloneWhy::Unparsed(Shelf::told(schema, &origin, why));
            Err(LeftAlone { file: origin, why })
        }
        DocReading::Unheld { origin, values, .. } => Err(LeftAlone {
            file: origin,
            why: LeftAloneWhy::Unheld(values),
        }),
    }
}

/// The plan for one file, decided in this order (the module page): a file whose front
/// matter cannot be read is left alone ([`parsed`]), any other by [`form_write`].
async fn plan<D: DocSource>(
    schema: &Schema,
    drop: &DropNames,
    docs: &D,
    found: FileRead,
) -> Result<Plan, ShelfError> {
    let FileRead {
        reading,
        read,
        comment,
    } = found;
    let (origin, entries, body) = match parsed(schema, reading) {
        Ok(parsed) => parsed,
        Err(left) => return Ok(Plan::LeaveAlone(left)),
    };
    let held = Held {
        file: &origin,
        read: &read,
        comment,
    };
    Ok(
        match form_write(schema, drop, docs, held, &origin, &entries, &body).await? {
            FormWrite::InForm(_) => Plan::InForm(origin),
            FormWrite::Leave(why) => leave(&origin, why),
            FormWrite::Write(written) => Plan::Rewrite {
                file: origin,
                read,
                text: written.text,
                dropped: written.form.dropped().clone(),
            },
        },
    )
}

/// The plan of every file of the books.
async fn plans<P: ShelfRead>(ports: &P, ask: &DropAsk) -> Result<Vec<Plan>, ShelfError> {
    let schema = schema_of(ports, Side::After)
        .await?
        .map_err(ShelfError::Schema)?;
    // The names to drop fit the declaration, or nothing is read or written.
    let drop = DropNames::new(&schema, ask.clone()).map_err(ShelfError::Drop)?;
    let found = ports
        .docs()
        .forms(Side::After, &schema.ranges())
        .await
        .map_err(port(ShelfPort::Docs))?;
    let mut plans = Vec::with_capacity(found.len());
    for file in found {
        // A file its book skips (no front matter at all) is no document: `fmt` has
        // nothing to say about it, as `check` has not.
        if Shelf::skip_of(&schema, &file.reading).is_some() {
            continue;
        }
        plans.push(plan(&schema, &drop, ports.docs(), file).await?);
    }
    Ok(plans)
}

impl<P: ShelfRead> ShelfApp<P> {
    /// Which documents' files are not in the one form, and which `fmt` would leave
    /// as they are, each with why, and for each file it would write, the names it would
    /// drop (`ask`). Writes nothing. Names that do not fit the declaration are
    /// [`ShelfError::Drop`].
    pub async fn fmt_check(&self, ask: &DropAsk) -> Result<FmtReport, ShelfError> {
        let mut report = FmtReport::default();
        for plan in plans(&self.ports, ask).await? {
            match plan {
                Plan::InForm(file) => report.in_form.push(file),
                Plan::Rewrite { file, dropped, .. } => {
                    report.dropped.extend(Dropped::of(&file, dropped));
                    report.not_in_form.push(file);
                }
                Plan::LeaveAlone(left) => report.left_alone.push(left),
            }
        }
        Ok(report)
    }
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Write every document's file that is not in the one form in it, when the text
    /// in the form reads back as the entries the form holds and the body of the file
    /// now (the module page); every other
    /// file is left as it is, and listed with why. A file in the form is not written.
    /// The names in `ask` are dropped from the files of the books that do not declare
    /// them (the module page); names that do not fit the declaration are
    /// [`ShelfError::Drop`], and nothing is written.
    pub async fn fmt(&mut self, ask: &DropAsk) -> Result<FmtReport, ShelfError> {
        let mut report = FmtReport::default();
        for plan in plans(&self.ports, ask).await? {
            match plan {
                Plan::InForm(file) => report.in_form.push(file),
                Plan::LeaveAlone(left) => report.left_alone.push(left),
                Plan::Rewrite {
                    file,
                    read,
                    text,
                    dropped,
                } => {
                    let done = self
                        .ports
                        .doc_replacer()
                        .replace_read(&file, &read, &text)
                        .await
                        .map_err(port(ShelfPort::DocReplace))?;
                    match done {
                        Replacement::Replaced => {
                            report.dropped.extend(Dropped::of(&file, dropped));
                            report.rewritten.push(file);
                        }
                        Replacement::Changed => report.left_alone.push(LeftAlone {
                            file,
                            why: LeftAloneWhy::Changed,
                        }),
                    }
                }
            }
        }
        Ok(report)
    }
}
