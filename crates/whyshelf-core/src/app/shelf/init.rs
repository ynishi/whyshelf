//! The use case that starts a project: `init`, a declaration of books and, beside
//! it, documents of them, each made as `new` makes one, and the other files they
//! may name.
//!
//! ```text
//!   declaration text -- BookSchemaSource::declarations_in --> ShelfDecl
//!   ShelfDecl -------- Schema::declared ----------------------> Schema, or every problem
//!   each request ----- the steps of `new` on the shelf -------> a NewDoc (key, id, judged)
//!                      of the documents before it
//!   in the way? ------ DeclarationWrite::in_the_way,
//!                      FileWrite::in_the_way of each other file,
//!                      DocWrite::in_the_way of each file,
//!                      DocWrite::dir_in_the_way of each book,
//!                      Shelf::files_writing each key ---------> refused, nothing written
//!   text ------------- DeclarationWrite::create (create-only) -> the declaration
//!   each InitFile ---- FileWrite::create (create-only) --------> the file, as it is
//!   each NewDoc ------ DocWrite::create (create-only) ---------> <book dir>/<key>.md
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
//! uses and checked by [`Schema::declared`], and each document is judged by the
//! rules `new` judges one by, on a shelf of the declaration that holds the documents
//! made before it (so a link to one of them resolves). Then everything in the way
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
//! the order given, and a write that fails removes the file it made: a run that
//! fails part way leaves, at worst, the declaration with fewer files, each whole,
//! and says which were written ([`ShelfError::PartWay`]).

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{DocReading, NewDoc, Shelf};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::order::heads;
use crate::domain::shelf::schema::Schema;

use super::ShelfApp;
use super::error::{Obstacle, ShelfError};
use super::new_doc::{Created, NewDocRequest, checked_new, write_new};
use super::ports::{
    ArtifactSource, BookSchemaSource, Creation, DeclarationText, DeclarationWrite, DocSource,
    DocWrite, FileWrite, ShelfPort, ShelfWrite, port,
};

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
    /// ([`heads`]), in the same order: named to `judge approve --pending`, they take
    /// every document it made and nothing else.
    ///
    /// [`heads`]: crate::domain::shelf::order::heads
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

        // Each document on the shelf of the ones before it, as `new` would make it
        // once they were written.
        let mut readings = Vec::new();
        let mut planned: Vec<NewDoc> = Vec::new();
        for doc in request.docs {
            let shelf = shelf_of(ports, &schema, readings.clone()).await?;
            let new = checked_new(ports, &shelf, doc).await?;
            readings.push(DocReading::Parsed {
                origin: new.doc().origin().clone(),
                entries: new.entries().clone(),
                body: new.doc().body().to_owned(),
            });
            planned.push(new);
        }
        let made: BTreeSet<DocId> = planned.iter().map(|new| new.doc().id()).collect();
        let heads = heads(&shelf_of(ports, &schema, readings).await?, &made);

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

        match ports
            .declaration_writer()
            .create(&request.declaration)
            .await
            .map_err(port(ShelfPort::DeclarationWrite))?
        {
            Creation::Created => {}
            Creation::AlreadyThere => {
                return Err(ShelfError::InTheWay {
                    obstacles: NonEmptyVec::new(Obstacle::There(declaration), Vec::new()),
                });
            }
        }
        let mut written = NonEmptyVec::new(declaration.clone(), Vec::new());
        let mut files = Vec::with_capacity(request.files.len());
        for file in &request.files {
            let made = ports
                .file_writer()
                .create(&file.path, &file.bytes)
                .await
                .map_err(port(ShelfPort::FileWrite));
            let cause = match made {
                Ok(Creation::Created) => {
                    written.push(file.path.clone());
                    files.push(file.path.clone());
                    continue;
                }
                Ok(Creation::AlreadyThere) => ShelfError::FileTaken {
                    path: file.path.clone(),
                },
                Err(cause) => cause,
            };
            return Err(ShelfError::PartWay {
                written,
                failed: file.path.clone(),
                cause: Box::new(cause),
            });
        }
        let mut docs = Vec::with_capacity(planned.len());
        for new in &planned {
            match write_new(ports, new).await {
                Ok(created) => {
                    written.push(created.file.clone());
                    docs.push(created);
                }
                Err(cause) => {
                    return Err(ShelfError::PartWay {
                        written,
                        failed: new.doc().origin().clone(),
                        cause: Box::new(cause),
                    });
                }
            }
        }
        let heads = docs
            .iter()
            .filter(|created| heads.contains(&created.id))
            .map(|created| created.key.clone())
            .collect();
        Ok(Initialized {
            declaration,
            files,
            docs,
            heads,
        })
    }
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
    planned: &[NewDoc],
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

/// The shelf of `schema` with these files, their links to artifacts answered by the
/// adapters, as the load builds one.
async fn shelf_of<P: ShelfWrite>(
    ports: &P,
    schema: &Schema,
    readings: Vec<DocReading>,
) -> Result<Shelf, ShelfError> {
    let asks = Shelf::artifact_asks(schema, &readings);
    let artifacts = if asks.is_empty() {
        ArtifactReadings::new()
    } else {
        ports
            .artifacts()
            .resolve(&asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?
    };
    Ok(Shelf::build(schema, readings, &artifacts))
}
