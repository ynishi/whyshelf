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
//! | [`ArtifactSource`] | for each path a document links to: the hash of the file and where its content can be read back, or no file; and the bytes of a file now |
//! | [`ContentStore`] | content read back by its [`ContentAddress`], or "not readable" |
//! | [`RevisionSource`] | the point of the project's history now, if there is one |
//!
//! The ports are bundled in two traits, so that a use case that only reads is generic
//! over a type that has nothing to write with:
//!
//! ```text
//!   ShelfRead   BookSchemaSource  DocSource  LedgerRead  Clock  ArtifactSource  ContentStore
//!        ^
//!        |      what `books check`, `status`, `get`, `trace`, `stale`, `show --approved`
//!        |      and `diff` are generic over
//!   ShelfWrite  + LedgerAppend  IdSource  ActorSource  RevisionSource
//!               what `judge` is generic over
//! ```

use std::error::Error;
use std::fmt;
use std::future::Future;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::location::RelPath;
use crate::domain::shelf::artifact::{ArtifactReadings, ContentAddress, Revision};
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
    /// [`ArtifactSource`].
    Artifacts,
    /// [`ContentStore`].
    Store,
    /// [`RevisionSource`].
    Revision,
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
            Self::Artifacts => "artifact",
            Self::Store => "content store",
            Self::Revision => "revision",
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
    /// The text is not JSON.
    #[error("the file is not a record in JSON: {detail}")]
    NotJson {
        /// What the parser said (its own wording, for a person to read).
        detail: String,
    },
    /// The JSON holds a number that is not a whole number of 64 bits, which the
    /// canonical form has no place for.
    #[error("the file holds the number {number}, which is not a whole number of 64 bits")]
    NotWhole {
        /// The number as written.
        number: String,
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

/// The files of a project that are no document, read by path: what is there now.
///
/// The adapter decides what of a file the [`ArtifactHash`] covers (it hands the bytes
/// it chose to [`ArtifactHash::of_content`]) and what a [`ContentAddress`] is; the
/// domain only compares them.
///
/// [`ArtifactHash`]: crate::domain::shelf::artifact::ArtifactHash
/// [`ArtifactHash::of_content`]: crate::domain::shelf::artifact::ArtifactHash::of_content
pub trait ArtifactSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// For each path: the hash of the file and where its content can be read back
    /// (when the adapter can say: outside version control it cannot, and that is no
    /// error), or no file. Every path asked about has an answer. An error is for what
    /// stops the reading itself (a file that is there and cannot be read).
    fn read_artifacts(
        &self,
        paths: &[RelPath],
    ) -> impl Future<Output = Result<ArtifactReadings, Self::Error>> + Send;

    /// The bytes of the file at `path` as they are now, or none when no file is
    /// there: what `diff` sets beside the content that was approved.
    fn current(
        &self,
        path: &RelPath,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;
}

/// Content read back by its address, as it was when the address was taken.
pub trait ContentStore: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The content at `address`, or none when nothing can be read through it (it was
    /// never kept, or is kept no longer). An error is for a store that cannot be
    /// asked at all.
    fn read_back(
        &self,
        address: &ContentAddress,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;
}

/// The point of the project's history the work is at, as a hint for a record.
pub trait RevisionSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The revision now, or none when the project has no history the adapter can
    /// name (outside version control, or before it has any history).
    fn revision(&self) -> impl Future<Output = Result<Option<Revision>, Self::Error>> + Send;
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
    /// The files that are no document.
    type Artifacts: ArtifactSource;
    /// The content that was approved, read back.
    type Store: ContentStore;

    /// The source of the declarations.
    fn schemas(&self) -> &Self::Schemas;
    /// The source of the documents.
    fn docs(&self) -> &Self::Docs;
    /// The ledger, to read.
    fn ledger(&self) -> &Self::Ledger;
    /// The clock.
    fn clock(&self) -> &Self::Clock;
    /// The source of the artifacts.
    fn artifacts(&self) -> &Self::Artifacts;
    /// The store content is read back from.
    fn store(&self) -> &Self::Store;
}

/// The ports that `judge` is generic over: everything a read has, and the four
/// things only a judgement needs.
pub trait ShelfWrite: ShelfRead {
    /// The ledger, to append to.
    type Append: LedgerAppend;
    /// The identities of new records.
    type Ids: IdSource;
    /// The acting person.
    type Actor: ActorSource;
    /// The revision an approval is made at.
    type Revisions: RevisionSource;

    /// The ledger, to write.
    fn append(&self) -> &Self::Append;
    /// The source of identities.
    fn ids(&self) -> &Self::Ids;
    /// The acting person.
    fn actor_source(&self) -> &Self::Actor;
    /// The source of the revision.
    fn revisions(&self) -> &Self::Revisions;
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
