//! What the use cases of the shelf need from the outside world.
//!
//! Each port is a trait declared here, by its consumer, and implemented by the
//! `whyshelf-infra` crate (and by an in-memory fake in the tests). They keep to
//! these rules: a method returns
//! `impl Future<Output = Result<_, Self::Error>> + Send`, so this crate names no
//! runtime; there is no `dyn`; every port has a typed `type Error`, which reaches the
//! caller as [`ShelfError::Port`] with the cause kept; and what crosses the
//! boundary is a value of the domain or of this layer, never a file, a JSON text or
//! a TOML table.
//!
//! | port | gives the use cases |
//! |---|---|
//! | [`BookSchemaSource`] | the declarations of the books, as an adapter read them |
//! | [`DocSource`] | one [`DocReading`] per Markdown file under the directories of the books |
//! | [`LedgerRead`] | the records of the ledger, and the files of it that could not be read as one |
//! | [`LedgerAppend`] | one record added, never in place of another |
//! | [`Clock`] | now, as a [`JudgedAt`] |
//! | [`IdSource`] | the identity of a new record |
//! | [`ActorSource`] | who is acting |
//!
//! The ports are bundled in two traits, so that a use case that only reads is generic
//! over a type that has nothing to write with:
//!
//! ```text
//!   ShelfRead   BookSchemaSource  DocSource  LedgerRead  Clock
//!        ^
//!        |      what `books check`, `status`, `get`, `trace` and `stale` are generic over
//!   ShelfWrite  + LedgerAppend  IdSource  ActorSource
//!               what `judge` is generic over
//! ```

use std::error::Error;
use std::fmt;
use std::future::Future;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::location::RelPath;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::ledger::{JudgedAt, JudgementId, Record, RecordProblem};
use crate::domain::shelf::schema::{BookDir, BookSchemaDecl};

use super::error::ShelfError;

/// Which port failed, for [`ShelfError::Port`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShelfPort {
    /// [`BookSchemaSource`].
    Schema,
    /// [`DocSource`].
    Docs,
    /// [`LedgerRead`].
    LedgerRead,
    /// [`LedgerAppend`].
    LedgerAppend,
    /// [`Clock`].
    Clock,
    /// [`IdSource`].
    Ids,
    /// [`ActorSource`].
    Actor,
}

impl fmt::Display for ShelfPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Schema => "book schema",
            Self::Docs => "document",
            Self::LedgerRead => "ledger read",
            Self::LedgerAppend => "ledger append",
            Self::Clock => "clock",
            Self::Ids => "id",
            Self::Actor => "actor",
        })
    }
}

/// The declarations of the books of a project.
pub trait BookSchemaSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Every book as the project declares it: text and lists, nothing checked (that
    /// is [`Schema::validate`](crate::domain::shelf::schema::Schema::validate)'s
    /// work). A project with no declaration is an error, not an empty list: it is far
    /// more likely a wrong place than a shelf with no book.
    fn declarations(&self)
    -> impl Future<Output = Result<Vec<BookSchemaDecl>, Self::Error>> + Send;
}

/// The documents of a project, read.
pub trait DocSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// One reading for every Markdown file under one of these directories.
    ///
    /// A file whose front matter cannot be read is a
    /// [`DocReading::Unparsed`], not an error: the shelf makes it an invalid
    /// document that says why. A directory that is not there has no file. An error
    /// is for what stops the reading itself (a disk that cannot be read).
    fn read_docs(
        &self,
        dirs: &[BookDir],
    ) -> impl Future<Output = Result<Vec<DocReading>, Self::Error>> + Send;
}

/// Why a file of the ledger is not a record.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UnreadableWhy {
    /// The bytes are not UTF-8 text.
    #[error("the file is not UTF-8 text")]
    NotText,
    /// The text is not a JSON document the canonical form can be made from (it holds
    /// a number that is not a whole number of 64 bits, or a key written twice).
    #[error("the file is not a record in JSON: {detail}")]
    NotJson {
        /// What the parser said.
        detail: String,
    },
    /// The form is well made and is not a record.
    #[error("the file is not a record")]
    NotARecord(#[source] RecordProblem),
}

/// A file of the ledger that could not be read as a record.
///
/// Reading it must not be silent: a decision that cannot be read is a decision the
/// answers do not know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableRecord {
    /// The file, relative to the project root.
    pub file: RelPath,
    /// Why.
    pub why: UnreadableWhy,
}

/// The ledger as the adapter read it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LedgerContents {
    /// The records.
    pub records: Vec<Record>,
    /// The files that are none.
    pub unreadable: Vec<UnreadableRecord>,
}

/// Reading the ledger.
pub trait LedgerRead: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Every record of the ledger. A ledger that is not there reads as empty, and
    /// reading it creates nothing.
    fn read(&self) -> impl Future<Output = Result<LedgerContents, Self::Error>> + Send;
}

/// Adding to the ledger.
pub trait LedgerAppend: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Add a record. Create-only: a record is never put in the place of one that is
    /// already there, so an adapter that finds its place taken fails.
    fn append(&self, record: &Record) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// The time. The domain never reads a clock.
pub trait Clock: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Now, in unix milliseconds.
    fn now(&self) -> impl Future<Output = Result<JudgedAt, Self::Error>> + Send;
}

/// New identities of records. The domain never makes one.
pub trait IdSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The identity of a new record: a UUID v7.
    fn new_judgement_id(&self) -> impl Future<Output = Result<JudgementId, Self::Error>> + Send;
}

/// Who is acting.
pub trait ActorSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The name of the person (or tool) that judges, as it is declared.
    fn actor(&self) -> impl Future<Output = Result<Actor, Self::Error>> + Send;
}

/// The ports that a use case that only reads is generic over.
///
/// One trait with accessors, so that an adapter's types are bundled once.
pub trait ShelfRead: Send + Sync {
    /// The declarations of the books.
    type Schemas: BookSchemaSource;
    /// The document files.
    type Docs: DocSource;
    /// The ledger, read.
    type Ledger: LedgerRead;
    /// The clock.
    type Clock: Clock;

    /// The source of the declarations.
    fn schemas(&self) -> &Self::Schemas;
    /// The source of the documents.
    fn docs(&self) -> &Self::Docs;
    /// The ledger, to read.
    fn ledger(&self) -> &Self::Ledger;
    /// The clock.
    fn clock(&self) -> &Self::Clock;
}

/// The ports that `judge` is generic over: everything a read has, and the three
/// things only a judgement needs.
pub trait ShelfWrite: ShelfRead {
    /// The ledger, to append to.
    type Append: LedgerAppend;
    /// The identities of new records.
    type Ids: IdSource;
    /// The acting person.
    type Actor: ActorSource;

    /// The ledger, to write.
    fn append(&self) -> &Self::Append;
    /// The source of identities.
    fn ids(&self) -> &Self::Ids;
    /// The acting person.
    fn actor_source(&self) -> &Self::Actor;
}

/// Turn a port's failure into the error of a use case, keeping the cause.
pub(super) fn port<E: Error + Send + Sync + 'static>(
    which: ShelfPort,
) -> impl FnOnce(E) -> ShelfError {
    move |source| ShelfError::Port {
        port: which,
        source: Box::new(source),
    }
}
