//! The use case that starts a project: `init`, a declaration of books and, beside
//! it, documents of them, each made as `new` makes one, and the other files they
//! may name.
//!
//! ```text
//!   declaration text -- BookSchemaSource::declarations_in --> ShelfDecl
//!   ShelfDecl -------- Schema::declared ----------------------> Schema, or every problem
//!   all requests ----- the steps of `new`, as one run ---------> a NewPlan (every key, id,
//!                      on the shelf of the declaration alone      judgement and link)
//!   in the way? ------ DeclarationWrite::in_the_way,
//!                      FileWrite::in_the_way of each other file,
//!                      DocWrite::in_the_way of each file,
//!                      DocWrite::dir_in_the_way of each book,
//!                      Shelf::files_writing each key ---------> refused, nothing written
//!   all of them, as one run (RunWrites::run):
//!   text ------------- RunWrites::declaration (create-only) ---> the declaration
//!   each InitFile ---- RunWrites::file (create-only) -----------> the file, as it is
//!   each NewDoc ------ RunWrites::doc (create-only) ------------> <book dir>/<key>.md
//!   the end of the run - FileSync::persist, once -----------------> every file on disk
//! ```
//!
//! The other files ([`InitFile`]) are what a template's documents may link to as
//! artifacts (a table a plan pins, say), so that a project starts with no name that
//! has nothing behind it. They are written as they are and judged by nothing but
//! what is in their way: what they are is the caller's, as the declaration's text
//! is. Two writes at one path, or one under another (a file at a document's path,
//! say), are a fault of the request ([`ShelfError::Overlapping`]), refused before
//! anything is looked at on disk. A document is judged before they are written, so a link of it to one of them
//! is judged as a name with nothing behind it yet, which is valid.
//!
//! What the declaration and the documents are is the caller's: this use case knows
//! no kind of document, and is given them as a text in the format of the adapter of
//! the declaration and as the requests `new` takes. Everything is judged before
//! anything is written: the text is read by the reader every read of the project
//! uses and checked by [`Schema::declared`], and the documents are planned as one run
//! by the plan `new` plans a run by ([`Shelf::plan_new`]), on a shelf of the
//! declaration alone, so that a link to any document of the run resolves, whichever
//! comes first. Then everything in the way
//! is looked for, all of it, by one rule for the declaration and the documents:
//! anything at a path (a file, a directory, a link even to nothing) is in the way,
//! and so is a parent of it that is there and is no directory (a file, a link to
//! nothing or to what is no directory; a link to a directory is a way through).
//! A book's directory that is there and is no directory, or has such a parent, is
//! in the way too, for every book declared, whether or not a document is made in
//! it: the books are read from their directories.
//! When no path is in the way, each file of a book that already writes a key a
//! document would take is. Any of it is a refusal that names each.
//!
//! The declaration is written first, then the other files, then the documents, in
//! the order given, all inside one [`RunWrites::run`], and a write that fails removes
//! the file it made: a run that fails part way leaves, at worst, the declaration with
//! fewer files, each whole, and says which were written ([`ShelfError::PartWay`]).
//! The files written are put on disk together, once, when the run ends (a run that
//! stops part way puts those it wrote on disk before it says so): an answer of
//! success has every file on disk, and one whose files cannot be put on disk is
//! [`ShelfError::NotDurable`].

use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{NewDoc, Shelf};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::schema::Schema;

use super::ShelfApp;
use super::error::{Obstacle, ShelfError};
use super::new_doc::{Created, NewDocRequest, plan_of, single_refusal};
use super::ports::{
    ArtifactSource, BookSchemaSource, DeclarationText, DeclarationWrite, DocSource, DocWrite,
    FileWrite, ShelfPort, ShelfWrite, port,
};
use super::run_writes::{RunWrites, Stop, Wrote};

/// A request to start a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRequest {
    /// The declaration, as it is to be written.
    pub declaration: DeclarationText,
    /// The other files to write beside it, in order: neither a document nor the
    /// declaration, each as it is (a table a document links to, say).
    pub files: Vec<InitFile>,
    /// The documents to make beside it, in order, each as `new` would be asked for
    /// it; a document may link to one before it.
    pub docs: Vec<NewDocRequest>,
}

/// A file `init` writes that is neither a document nor the declaration: its path
/// and its bytes, written as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitFile {
    /// Where, relative to the project root.
    pub path: RelPath,
    /// What it holds.
    pub bytes: Vec<u8>,
}

/// What `init` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initialized {
    /// The file of the declaration, relative to the project root.
    pub declaration: RelPath,
    /// The other files, relative to the project root, in the order they were
    /// written.
    pub files: Vec<RelPath>,
    /// The documents, in the order they were written.
    pub docs: Vec<Created>,
    /// Those of `docs` that no other of them stands on through a `Depends` link
    /// ([`NewPlan::heads`]), in the same order: named to `judge approve --pending`,
    /// they take every document it made and nothing else.
    ///
    /// [`NewPlan::heads`]: crate::domain::shelf::build::NewPlan::heads
    pub heads: Vec<DocKey>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Write a declaration of books, the other files given, and these documents of
    /// the books, where none is.
    ///
    /// A text that is no declaration is an error of the port that reads it
    /// ([`ShelfError::Port`], a failed run), a declaration that is not sound is
    /// [`ShelfError::Schema`], and a document its book refuses is the error `new`
    /// gives for it; in each case nothing is written. Anything at the declaration's
    /// path, another file's or a document's, a parent of one that is no directory, and a file of a book that already writes a key a
    /// document would take, is [`ShelfError::InTheWay`], with every such thing;
    /// nothing is written then either. No file is ever replaced. A write that fails
    /// after the first is [`ShelfError::PartWay`], with the files written.
    pub async fn init(&mut self, request: InitRequest) -> Result<Initialized, ShelfError> {
        let ports = &self.ports;
        let decl = ports
            .schemas()
            .declarations_in(&request.declaration)
            .map_err(port(ShelfPort::Schema))?;
        let artifacts = ports.artifacts();
        let schema = Schema::declared(decl, &artifacts.presets(), &artifacts.spaces())
            .map_err(ShelfError::Schema)?;

        // The documents as one run, on the shelf of the declaration alone: what `new`
        // would make of them once they were written, every link among them resolved.
        let shelf = Shelf::build(&schema, Vec::new(), &ArtifactReadings::new());
        let plan = plan_of(ports, &shelf, request.docs)
            .await
            .map_err(single_refusal)?;
        let planned: Vec<&NewDoc> = plan.docs().collect();
        let heads = plan.heads();

        let declaration = ports.declaration_writer().file();
        let paths: Vec<&RelPath> = std::iter::once(&declaration)
            .chain(request.files.iter().map(|file| &file.path))
            .chain(planned.iter().map(|new| new.doc().origin()))
            .collect();
        if let Some((path, with)) = overlapping(&paths) {
            return Err(ShelfError::Overlapping { path, with });
        }
        let obstacles = in_the_way(ports, &schema, &request.files, &planned).await?;
        if let Ok(obstacles) = NonEmptyVec::from_vec(obstacles) {
            return Err(ShelfError::InTheWay { obstacles });
        }

        // Every write is made inside one run, which keeps what was written and puts it
        // on disk together at the end, so what failed after the declaration says what
        // was written.
        let (files, docs) = RunWrites::run(ports, |writes| {
            Box::pin(write_all(
                writes,
                &request.declaration,
                &request.files,
                &planned,
            ))
        })
        .await?;
        Ok(Initialized {
            declaration,
            files,
            docs,
            heads,
        })
    }
}

/// The writes of `init`, in order, inside the run `writes` is of: the declaration
/// (its text; the run knows its file), then the other files, then the documents. The
/// files and the documents it made, in order. Anything found at a path where a file
/// was to be made stops the run ([`Stop`]), as every other thing that does.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
async fn write_all<P: ShelfWrite>(
    writes: &mut RunWrites<'_, P>,
    text: &DeclarationText,
    other_files: &[InitFile],
    planned: &[&NewDoc],
) -> Result<(Vec<RelPath>, Vec<Created>), Stop> {
    if let Wrote::AlreadyThere(at) = writes.declaration(text).await? {
        return Err(Stop {
            failed: at.clone(),
            cause: ShelfError::InTheWay {
                obstacles: NonEmptyVec::new(Obstacle::There(at), Vec::new()),
            },
        });
    }
    let mut files = Vec::with_capacity(other_files.len());
    for file in other_files {
        match writes.file(&file.path, &file.bytes).await? {
            Wrote::Created(()) => files.push(file.path.clone()),
            Wrote::AlreadyThere(path) => {
                return Err(Stop {
                    failed: file.path.clone(),
                    cause: ShelfError::FileTaken { path },
                });
            }
        }
    }
    let mut docs = Vec::with_capacity(planned.len());
    for new in planned {
        match writes.doc(new).await? {
            Wrote::Created(created) => docs.push(created),
            Wrote::AlreadyThere(path) => {
                return Err(Stop {
                    failed: new.doc().origin().clone(),
                    cause: ShelfError::FileTaken { path },
                });
            }
        }
    }
    Ok((files, docs))
}

/// The first path of `paths`, in order, that is an earlier one or lies under an
/// earlier one, or has an earlier one under it, with that earlier one: two writes
/// that cannot both be made.
fn overlapping(paths: &[&RelPath]) -> Option<(RelPath, RelPath)> {
    let under = |inner: &str, outer: &str| {
        inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    paths.iter().enumerate().find_map(|(at, path)| {
        paths[..at]
            .iter()
            .find(|with| {
                let (a, b) = (path.as_str(), with.as_str());
                a == b || under(a, b) || under(b, a)
            })
            .map(|with| ((*path).clone(), (*with).clone()))
    })
}

/// Everything in the way of the writes: what is at the declaration's path, another
/// file's or a document's, or at a parent of one that is no directory, then each
/// book's directory, or a parent of it, that is no directory, whether or not a
/// document is made in it (each path once, in the order they are written); when
/// none is, each file of a book that writes a key a
/// document would take (one at the document's own path is told as a path).
async fn in_the_way<P: ShelfWrite>(
    ports: &P,
    schema: &Schema,
    files: &[InitFile],
    planned: &[&NewDoc],
) -> Result<Vec<Obstacle>, ShelfError> {
    let mut paths: Vec<RelPath> = Vec::new();
    paths.extend(
        ports
            .declaration_writer()
            .in_the_way()
            .await
            .map_err(port(ShelfPort::DeclarationWrite))?,
    );
    for file in files {
        if let Some(path) = ports
            .file_writer()
            .in_the_way(&file.path)
            .await
            .map_err(port(ShelfPort::FileWrite))?
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    for new in planned {
        if let Some(path) = ports
            .doc_writer()
            .in_the_way(new.doc().origin())
            .await
            .map_err(port(ShelfPort::DocWrite))?
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    // Every book's directory is read, whether or not a document is made in it.
    for book in schema.books().values() {
        if let Some(path) = ports
            .doc_writer()
            .dir_in_the_way(book.dir().as_rel())
            .await
            .map_err(port(ShelfPort::DocWrite))?
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    // A directory of a book that is no directory cannot be read for its keys; what
    // is in its way is the answer.
    if !paths.is_empty() || planned.is_empty() {
        return Ok(paths.into_iter().map(Obstacle::There).collect());
    }
    let mut obstacles = Vec::new();
    let there = ports
        .docs()
        .read_docs(&schema.ranges())
        .await
        .map_err(port(ShelfPort::Docs))?;
    let shelf = Shelf::build(schema, there, &ArtifactReadings::new());
    for new in planned {
        let key = new.doc().key();
        for file in shelf.files_writing(key) {
            if file != *new.doc().origin() {
                obstacles.push(Obstacle::KeyTaken {
                    key: key.clone(),
                    file,
                });
            }
        }
    }
    Ok(obstacles)
}
