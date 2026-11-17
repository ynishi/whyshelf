//! The use cases that put documents' files in the one form documents are written
//! in: [`fmt`](ShelfApp::fmt), which rewrites them, and
//! [`fmt_check`](ShelfApp::fmt_check), which only says which are not in it.
//!
//! ```text
//!   BookSchemaSource -> the dirs of the books
//!   DocSource::forms -> per file: its reading, its bytes, the line of a comment
//!        |
//!        v  plan (here, per file), in this order
//!   unparsed, unheld ......................................... left alone
//!   Shelf::form_entries -> the entries the form holds        (the same as `new`'s)
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
//! with no key). Which entries those are is decided by the entry point `new`
//! asks too ([`Shelf::form_entries`]), so what `new` writes is in the form, and `fmt`
//! rewrites a file that holds such an entry without it. An entry that says nothing
//! and that the build tells a problem for (a required field, a link whose count asks
//! for a key, an undeclared entry, the `id` and the `key`) stays in the form, so the
//! rewrite does not change what `check` says.
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

use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::{DocReading, RawProblem, Shelf, UnheldValue};
use crate::domain::shelf::schema::Schema;

use super::ShelfApp;
use super::error::ShelfError;
use super::load::schema_of;
use super::ports::{
    DocReplace, DocSource, FileRead, Replaceable, Replacement, ShelfPort, ShelfRead, ShelfWrite,
    port,
};

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

/// What to do with one file.
enum Plan {
    InForm(RelPath),
    Rewrite {
        file: RelPath,
        read: Vec<u8>,
        text: Vec<u8>,
    },
    LeaveAlone(LeftAlone),
}

/// A file left as it is, and why.
fn leave(file: &RelPath, why: LeftAloneWhy) -> Plan {
    Plan::LeaveAlone(LeftAlone {
        file: file.clone(),
        why,
    })
}

/// The one rule of which file is written: the plan for one file, decided in this
/// order (the module page).
async fn plan<D: DocSource>(
    schema: &Schema,
    docs: &D,
    found: FileRead,
) -> Result<Plan, ShelfError> {
    let FileRead {
        reading,
        read,
        comment,
    } = found;
    let (origin, entries, body) = match reading {
        DocReading::Parsed {
            origin,
            entries,
            body,
        } => (origin, entries, body),
        DocReading::Unparsed { origin, why, .. } => {
            let why = LeftAloneWhy::Unparsed(Shelf::told(schema, &origin, why));
            return Ok(leave(&origin, why));
        }
        DocReading::Unheld { origin, values, .. } => {
            return Ok(leave(&origin, LeftAloneWhy::Unheld(values)));
        }
    };
    // A file the books do not take is not read for `fmt`; were one handed over, it is
    // not written.
    let Some(book) = schema.book_of(&origin) else {
        return Ok(leave(&origin, LeftAloneWhy::ReadsOtherwise));
    };
    let form = Shelf::form_entries(schema, book.name(), &entries);
    let rendered = docs.render(&origin, &form, &body);
    if rendered.text == read {
        return Ok(Plan::InForm(origin));
    }
    if let Some(line) = comment {
        return Ok(leave(&origin, LeftAloneWhy::Comment { line }));
    }
    match docs
        .replaceable(&origin)
        .await
        .map_err(port(ShelfPort::Docs))?
    {
        Replaceable::Yes => {}
        Replaceable::ReadOnly => return Ok(leave(&origin, LeftAloneWhy::ReadOnly)),
        Replaceable::Linked => return Ok(leave(&origin, LeftAloneWhy::Linked)),
    }
    let wanted = DocReading::Parsed {
        origin: origin.clone(),
        entries: form.as_map().clone(),
        body,
    };
    if rendered.reads_as == wanted {
        Ok(Plan::Rewrite {
            file: origin,
            read,
            text: rendered.text,
        })
    } else {
        Ok(leave(&origin, LeftAloneWhy::ReadsOtherwise))
    }
}

/// The plan of every file of the books.
async fn plans<P: ShelfRead>(ports: &P) -> Result<Vec<Plan>, ShelfError> {
    let schema = schema_of(ports).await?.map_err(ShelfError::Schema)?;
    let found = ports
        .docs()
        .forms(&schema.ranges())
        .await
        .map_err(port(ShelfPort::Docs))?;
    let mut plans = Vec::with_capacity(found.len());
    for file in found {
        // A file its book skips (no front matter at all) is no document: `fmt` has
        // nothing to say about it, as `check` has not.
        if Shelf::skip_of(&schema, &file.reading).is_some() {
            continue;
        }
        plans.push(plan(&schema, ports.docs(), file).await?);
    }
    Ok(plans)
}

impl<P: ShelfRead> ShelfApp<P> {
    /// Which documents' files are not in the one form, and which `fmt` would leave
    /// as they are, each with why. Writes nothing.
    pub async fn fmt_check(&self) -> Result<FmtReport, ShelfError> {
        let mut report = FmtReport::default();
        for plan in plans(&self.ports).await? {
            match plan {
                Plan::InForm(file) => report.in_form.push(file),
                Plan::Rewrite { file, .. } => report.not_in_form.push(file),
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
    pub async fn fmt(&mut self) -> Result<FmtReport, ShelfError> {
        let mut report = FmtReport::default();
        for plan in plans(&self.ports).await? {
            match plan {
                Plan::InForm(file) => report.in_form.push(file),
                Plan::LeaveAlone(left) => report.left_alone.push(left),
                Plan::Rewrite { file, read, text } => {
                    let done = self
                        .ports
                        .doc_replacer()
                        .replace_read(&file, &read, &text)
                        .await
                        .map_err(port(ShelfPort::DocReplace))?;
                    match done {
                        Replacement::Replaced => report.rewritten.push(file),
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
