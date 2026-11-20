//! A run of new files, written one after another and put on disk together: the one
//! place where the files a use case creates are made, and where they are put on disk.
//!
//! ```text
//!   RunWrites::run(ports, |w| ...)
//!      |  w.declaration(text) / w.file(path, bytes) / w.doc(new) / w.record(record)
//!      |        DeclarationWrite::create / FileWrite::create / DocWrite::create
//!      |        / LedgerAppend::append
//!      |        (create-only; a path that was made is kept, in order, inside `w`)
//!      |
//!      +-- the closure gave Ok(t) -----> FileSync::persist(every path kept) -> Ok(t)
//!      |                                 or, when that fails, NotDurable
//!      '-- the closure gave Err(Stop) -> FileSync::persist(every path kept), then
//!                                        PartWay { written, failed, cause, not_durable }
//!                                        (the cause as it is when nothing was written)
//! ```
//!
//! What a run promises, which `new`, `init` and every use case that writes records to
//! the ledger (judge, approve, exempt, moved, same meaning, withdraw) take over:
//!
//! - Each file is written whole, or removed (the adapters' promise: a write that
//!   fails after its file was made removes it). That is a promise about the
//!   process failing: a power loss before the file is on disk can leave one empty or
//!   partly written, and the next run takes a file whose front matter cannot be read
//!   as a name in use and moves on to another key. A record of the ledger is such a
//!   file, the one at `<whyshelf dir>/ledger/<id>.json`: it is created and never
//!   replaced, and one that cannot be read is told by every read of the ledger, not
//!   dropped.
//! - A run that answers success has every file it wrote on disk, so that it survives a
//!   power loss. That is done once for the run, at its end, and not once for each
//!   file.
//! - A run that stops part way puts the files it wrote on disk before it says which
//!   they were ([`ShelfError::PartWay`]); when that fails too, the answer carries both
//!   failures. A run that wrote every file and then failed to put them on disk is
//!   [`ShelfError::NotDurable`]: the files are whole and are left, and are named.
//!
//! Who decides what, each in one place:
//!
//! - which files the run wrote, in what order, and when they are put on disk: only
//!   [`RunWrites::run`] (its `finish` and `fail` are private), from the files the
//!   methods here made. A use case holds no list of what it wrote.
//! - which file a ledger record is written at: the adapter's, which says so when it
//!   writes the record ([`LedgerAppend::append`]) and before ([`LedgerAppend::path_of`],
//!   for the file a failed write was at). A use case never makes the path of a record.
//! - that a run stops: only a [`Stop`] returned by the closure. Every error the use
//!   case makes itself (a file found where one was to be made, what
//!   [`Shelf::replan_after_taken`] refuses) is returned as a [`Stop`] too.
//!
//! The one way left to write a file without the run knowing is to call a create
//! method of [`DocWrite`], [`DeclarationWrite`] or [`FileWrite`], or
//! [`LedgerAppend::append`], outside this module:
//! a trait's method cannot be hidden, so the calls are made here and nowhere else, and
//! the tests and the review look for one elsewhere.
//!
//! The closure of [`RunWrites::run`] gives its writes as a boxed future
//! ([`Writing`]), not as an async closure (`AsyncFnOnce`). Nothing in this crate
//! spawns the future of a use case, and the command line runs it on the thread it is
//! on, but the ports' futures are `Send` so that a caller may spawn one, and a bound
//! `Send` cannot be written on the future of an async closure: the compiler cannot
//! prove the future of a use case that holds one is `Send` (a test holds the use
//! cases that write to it). A boxed future carries the bound, for the price of one
//! allocation for a run.
//!
//! [`Shelf::replan_after_taken`]: crate::domain::shelf::build::Shelf::replan_after_taken

use std::future::Future;
use std::pin::Pin;

use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::NewDoc;
use crate::domain::shelf::ledger::AdmittedRecord;

use super::error::{PersistProblem, ShelfError};
use super::new_doc::Created;
use super::ports::{
    Creation, DeclarationText, DeclarationWrite, DocWrite, FileSync, FileWrite, LedgerAppend,
    ShelfPort, ShelfWrite, port,
};

/// What stops a run: the file it was at, and why. Returned by the closure of
/// [`RunWrites::run`], which makes the answer of it ([`ShelfError::PartWay`] when
/// files were written before, the cause as it is when none was).
#[derive(Debug)]
pub(super) struct Stop {
    /// The file that was not written.
    pub(super) failed: RelPath,
    /// Why.
    pub(super) cause: ShelfError,
}

/// What the closure of [`RunWrites::run`] gives: its writes, as a future that borrows
/// the run for as long as it takes (`Box::pin(async move { ... })`).
pub(super) type Writing<'a, T> = Pin<Box<dyn Future<Output = Result<T, Stop>> + Send + 'a>>;

/// What one write of a run came to.
#[derive(Debug)]
pub(super) enum Wrote<T> {
    /// The file was made, and is part of the run.
    Created(T),
    /// A file was already at the path, and nothing was written.
    AlreadyThere(RelPath),
}

/// The writes of one run, which keeps the files it made: [`RunWrites::run`] is the
/// only way to have one.
#[derive(Debug)]
pub(super) struct RunWrites<'p, P: ShelfWrite> {
    ports: &'p P,
    /// The files made so far, in order.
    written: Vec<RelPath>,
}

impl<'p, P: ShelfWrite> RunWrites<'p, P> {
    /// Run `f`, the writes of a use case, and put every file it made on disk once,
    /// at the end, whether `f` ends the run or stops it with a [`Stop`].
    ///
    /// `Ok(t)` of `f` is `Ok(t)` when the files are on disk (or none was made),
    /// else [`ShelfError::NotDurable`]. A [`Stop`] is [`ShelfError::PartWay`] with
    /// the files made, put on disk first, and the cause as it is when none was made.
    // The error is read once and ends the use case; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub(super) async fn run<T, F>(ports: &'p P, f: F) -> Result<T, ShelfError>
    where
        F: for<'a> FnOnce(&'a mut RunWrites<'p, P>) -> Writing<'a, T>,
    {
        let mut writes = RunWrites {
            ports,
            written: Vec::new(),
        };
        match f(&mut writes).await {
            Ok(done) => writes.finish().await.map(|()| done),
            Err(stop) => Err(writes.fail(stop).await),
        }
    }

    /// Put the files made on disk; nothing to do when none was made.
    // The error is read once and ends the use case; it is not a hot path.
    #[allow(clippy::result_large_err)]
    async fn finish(self) -> Result<(), ShelfError> {
        let Ok(files) = NonEmptyVec::from_vec(self.written) else {
            return Ok(());
        };
        match self.ports.file_syncer().persist(&files).await {
            Ok(()) => Ok(()),
            Err(source) => Err(ShelfError::NotDurable {
                files,
                cause: PersistProblem::of(source),
            }),
        }
    }

    /// The error of a run that stopped: the files made put on disk, then told
    /// ([`ShelfError::PartWay`], whose class is the cause's, or failed when they could
    /// not be put on disk either), or the cause as it is
    /// when none was made.
    async fn fail(self, stop: Stop) -> ShelfError {
        let Stop { failed, cause } = stop;
        let Ok(written) = NonEmptyVec::from_vec(self.written) else {
            return cause;
        };
        let not_durable = self
            .ports
            .file_syncer()
            .persist(&written)
            .await
            .err()
            .map(PersistProblem::of);
        ShelfError::PartWay {
            written,
            failed,
            cause: Box::new(cause),
            not_durable,
        }
    }

    /// Write the declaration, create-only. A file already at its path is
    /// [`Wrote::AlreadyThere`] with the path, and is the caller's to tell.
    pub(super) async fn declaration(&mut self, text: &DeclarationText) -> Result<Wrote<()>, Stop> {
        let writer = self.ports.declaration_writer();
        let file = writer.file();
        match writer.create(text).await {
            Ok(Creation::Created) => {
                self.written.push(file);
                Ok(Wrote::Created(()))
            }
            Ok(Creation::AlreadyThere) => Ok(Wrote::AlreadyThere(file)),
            Err(source) => Err(Stop {
                failed: file,
                cause: port(ShelfPort::DeclarationWrite)(source),
            }),
        }
    }

    /// Write a file that is neither a document nor the declaration, as it is,
    /// create-only. A file already at the path is [`Wrote::AlreadyThere`].
    pub(super) async fn file(&mut self, path: &RelPath, bytes: &[u8]) -> Result<Wrote<()>, Stop> {
        match self.ports.file_writer().create(path, bytes).await {
            Ok(Creation::Created) => {
                self.written.push(path.clone());
                Ok(Wrote::Created(()))
            }
            Ok(Creation::AlreadyThere) => Ok(Wrote::AlreadyThere(path.clone())),
            Err(source) => Err(Stop {
                failed: path.clone(),
                cause: port(ShelfPort::FileWrite)(source),
            }),
        }
    }

    /// Write a record of the ledger, create-only, at the path the ledger gives: a
    /// record already there is the ledger's error, not a [`Wrote::AlreadyThere`], and
    /// stops the run like any other failure of the write, at the file it was for
    /// ([`LedgerAppend::path_of`]). Only a record the rules of who may decide admit
    /// is written: the one way to one is [`Ledger::accept`].
    ///
    /// [`Ledger::accept`]: crate::domain::shelf::ledger::Ledger::accept
    pub(super) async fn record(&mut self, record: &AdmittedRecord) -> Result<(), Stop> {
        let ledger = self.ports.append();
        match ledger.append(record).await {
            Ok(file) => {
                self.written.push(file);
                Ok(())
            }
            Err(source) => Err(Stop {
                failed: ledger.path_of(record.record()),
                cause: port(ShelfPort::LedgerAppend)(source),
            }),
        }
    }

    /// Write a judged document's file, create-only: a file already at its path is
    /// [`Wrote::AlreadyThere`], and nothing is written. The one place a [`Created`]
    /// is made.
    pub(super) async fn doc(&mut self, new: &NewDoc) -> Result<Wrote<Created>, Stop> {
        let doc = new.doc();
        let origin = doc.origin();
        match self
            .ports
            .doc_writer()
            .create(origin, new.entries(), doc.body())
            .await
        {
            Ok(Creation::Created) => {
                self.written.push(origin.clone());
                Ok(Wrote::Created(Created {
                    id: doc.id(),
                    key: doc.key().clone(),
                    file: origin.clone(),
                    unresolved: new
                        .unresolved()
                        .into_iter()
                        .map(|(link, u)| (link.clone(), u.clone()))
                        .collect(),
                }))
            }
            Ok(Creation::AlreadyThere) => Ok(Wrote::AlreadyThere(origin.clone())),
            Err(source) => Err(Stop {
                failed: origin.clone(),
                cause: port(ShelfPort::DocWrite)(source),
            }),
        }
    }
}
