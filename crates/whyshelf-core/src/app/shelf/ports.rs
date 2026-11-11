//! What the use cases of the shelf need from the outside world.
//!
//! Each port is a trait declared here, by its consumer, and implemented by the
//! `whyshelf-infra` crate (and by an in-memory fake in the tests). They keep to
//! these rules: a method that does I/O returns
//! `impl Future<Output = Result<_, Self::Error>> + Send`, so this crate names no
//! runtime; a method that does none is a plain function, and returns a `Result` only
//! when it can fail ([`ArtifactSource::spaces`] and [`ArtifactSource::presets`]
//! answer from what the adapter is; [`BookSchemaSource::declarations_in`] parses a
//! text in memory; [`DeclarationWrite::file`] names a file it was made with), so a
//! caller is not made to wait on what does not wait; there is no `dyn`; every port
//! has a typed `type Error`, which reaches the caller as [`ShelfError::Port`] with
//! the cause kept; and what crosses the boundary is a value of the domain or of this
//! layer, never a file, a JSON text or a TOML table, with one exception: a text or
//! bytes in an adapter's own format that the core carries from one port to another
//! and never reads ([`DeclarationText`], the `read` and `text` of a [`FileForm`]).
//!
//! | port | gives the use cases |
//! |---|---|
//! | [`BookSchemaSource`] | the declaration of the project, as an adapter read it: the books, the presets and the defaults; and the declaration a text of its format holds, read by the same reader |
//! | [`DocSource`] | one [`DocReading`] per Markdown file under the directories of the books, the bytes and the [`ContentAddress`] of a document's file now, and how each file stands to the one form documents are written in ([`FileForm`]) |
//! | [`LedgerRead`] | the records of the ledger, and the files of it that could not be read as one |
//! | [`LedgerAppend`] | one record added, never in place of another |
//! | [`Clock`] | now, as a [`JudgedAt`] |
//! | [`IdSource`] | the identity of a new record, and of a new document |
//! | [`DocWrite`] | a new document's file, from its entries and body, never in place of a file that is there; and what is in the way of one at a path |
//! | [`DeclarationWrite`] | the file of the declaration, from a text in its format, never in place of a file that is there; and what is in the way of it |
//! | [`DocReplace`] | a document's file that is there, replaced whole by its text in the form, after a comparison with what was read (an edit saved just after it is overwritten) |
//! | [`ActorSource`] | who is acting |
//! | [`ArtifactSource`] | the spaces it offers, and the presets of links to artifacts each adapter offers for its own; for each name a document links to in one: there (hashes, an address?), absent, cannot tell (why), or not a name (why); and the bytes of an artifact now |
//! | [`ContentStore`] | content read back by its [`ContentAddress`], by the adapter of the document or the space it is of, or "not readable" |
//! | [`RevisionSource`] | the point of the project's history now, if there is one |
//! | [`MoveSource`] | for the names of a space with nothing behind them now, the names each may have moved to, each with how it was found, or that the space cannot suggest |
//!
//! The ports are bundled in two traits, so that a use case that only reads is generic
//! over a type that has nothing to write with:
//!
//! ```text
//!   ShelfRead   BookSchemaSource  DocSource  LedgerRead  Clock  ArtifactSource  ContentStore
//!        ^      MoveSource
//!        |      what `check`, `status`, `get`, `trace`, `stale`, `get --approved`,
//!        |      `diff`, `moves` and `fmt --check` are generic over
//!   ShelfWrite  + LedgerAppend  IdSource  ActorSource  RevisionSource  DocWrite  DocReplace
//!               DeclarationWrite
//!               what `judge`, `new_doc`, `init` and `fmt` are generic over
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::future::Future;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::shelf::artifact::{
    ArtifactAsks, ArtifactKey, ArtifactReadings, ContentAddress, Revision, Space,
};
use crate::domain::shelf::build::{DocReading, RawValue};
#[cfg(doc)]
use crate::domain::shelf::ledger::GoneNames;
use crate::domain::shelf::ledger::{
    GoneName, JudgedAt, JudgementId, Record, RecordProblem, SpaceSuggestions,
};
use crate::domain::shelf::schema::{BookRange, BuiltinPresets, ShelfDecl};

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
    /// [`DocWrite`].
    DocWrite,
    /// [`DocReplace`].
    DocReplace,
    /// [`MoveSource`].
    Moves,
    /// [`DeclarationWrite`].
    DeclarationWrite,
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
            Self::DocWrite => "document write",
            Self::DocReplace => "document replace",
            Self::Moves => "move suggestions",
            Self::DeclarationWrite => "declaration write",
        })
    }
}

/// The declarations of the books of a project.
pub trait BookSchemaSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Every book, preset and default as the project declares them: text and
    /// lists, nothing checked (that is
    /// [`Schema::declared`](crate::domain::shelf::schema::Schema::declared)'s work).
    /// A project with no declaration is an error, not an empty one: it is far more
    /// likely a wrong place than a shelf with no book.
    fn declarations(&self) -> impl Future<Output = Result<ShelfDecl, Self::Error>> + Send;

    /// The declaration `text` holds, read by the reader
    /// [`declarations`](Self::declarations) reads the project's file with, and
    /// nothing checked: what `init` holds a declaration against before it writes
    /// it. A text that is not of the adapter's format is an error.
    fn declarations_in(&self, text: &DeclarationText) -> Result<ShelfDecl, Self::Error>;
}

/// The text of a declaration of books, in the format of the adapter that reads and
/// writes the project's declaration.
///
/// The use cases carry it and never read it: what it declares is
/// [`BookSchemaSource::declarations_in`]'s answer, and it is written as it is, by
/// [`DeclarationWrite::create`], so that what is written in it beside the
/// declaration (comments) stays for a person to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationText(String);

impl DeclarationText {
    /// A text, as written.
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The documents of a project, read.
pub trait DocSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// One reading for every Markdown file the range of one of these books takes
    /// ([`BookRange::takes`]: under the directory, matching its `include` and none
    /// of its `exclude`). A file the range does not take is not read at all.
    ///
    /// A file whose front matter cannot be read is a
    /// [`DocReading::Unparsed`], not an error: the shelf makes it an invalid
    /// document that says why (or leaves it out, when its book skips a file with no
    /// front matter). A directory that is not there has no file. An error
    /// is for what stops the reading itself (a disk that cannot be read).
    fn read_docs(
        &self,
        ranges: &[BookRange],
    ) -> impl Future<Output = Result<Vec<DocReading>, Self::Error>> + Send;

    /// The bytes of the file of a document (relative to the project root) as they
    /// are now, or none when no file is there: what `diff` sets beside the content
    /// that was approved.
    fn current(
        &self,
        file: &RelPath,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;

    /// Where the content of each of these files of documents, as it is now, can be
    /// read back later ([`ContentStore::read_back`] of [`ContentOf::Doc`]), for those
    /// it can say (outside version control it cannot, and that is no error).
    fn addresses(
        &self,
        files: &BTreeSet<RelPath>,
    ) -> impl Future<Output = Result<BTreeMap<RelPath, ContentAddress>, Self::Error>> + Send;

    /// For every Markdown file the ranges take, as
    /// [`read_docs`](Self::read_docs) finds them: its reading, its bytes, and how it
    /// stands to the one form the adapter writes a document in (the form
    /// [`DocWrite::create`] writes). Reading creates nothing.
    fn forms(
        &self,
        ranges: &[BookRange],
    ) -> impl Future<Output = Result<Vec<FileForm>, Self::Error>> + Send;
}

/// A document's file beside the one form its adapter writes ([`DocSource::forms`]).
///
/// The bytes are the adapter's text, carried by the use case and never read by it:
/// [`DocReplace::replace_read`] is given them back, so that it replaces only the file
/// as it was read, and with the text that was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileForm {
    /// The file's reading now, as [`DocSource::read_docs`] gives it.
    pub reading: DocReading,
    /// The file's bytes now.
    pub read: Vec<u8>,
    /// How the file stands to the form.
    pub form: Form,
}

/// How a file stands to the one form its adapter writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// Its bytes are already the form's.
    InForm,
    /// No text in the form can be made from it: its front matter cannot be read.
    Unwritable,
    /// Its front matter holds a comment, which the form has no place for; `line` is
    /// the line of the file the first one is on.
    Comment {
        /// The line, counted from 1.
        line: u32,
    },
    /// It is not in the form, and the user cannot write it.
    ReadOnly,
    /// It is not in the form, and it has more than one link (a hard link), which a
    /// file written in its place would not keep.
    Linked,
    /// It is not in the form: the text the form gives it, and what that text reads
    /// as. The use case writes the text only when that reading is the file's own.
    Other {
        /// The text in the form.
        text: Vec<u8>,
        /// The reading of that text.
        reads_as: DocReading,
    },
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

/// New identities of records and of documents. The domain never makes one.
pub trait IdSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The identity of a new record: a UUID v7.
    fn new_judgement_id(&self) -> impl Future<Output = Result<JudgementId, Self::Error>> + Send;

    /// The identity of a new document: a UUID v7, which the document keeps for good.
    fn new_doc_id(&self) -> impl Future<Output = Result<DocId, Self::Error>> + Send;
}

/// What [`DocWrite::create`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Creation {
    /// The file was written.
    Created,
    /// A file is already at the path; nothing was written.
    AlreadyThere,
}

/// Writing a new document's file.
pub trait DocWrite: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// What is in the way of a new file at `path` (relative to the project root)
    /// now, if anything, by its path: a parent inside the root that is there and is
    /// no directory (a file, a link to nothing, a link to what is no directory),
    /// else anything at the path itself (a file of any content, a directory, a
    /// link, even one that leads nowhere). A parent that is a link to a directory
    /// is none: [`create`](Self::create) makes the file through it. What `create`
    /// would find in its way, asked before it is called.
    fn in_the_way(
        &self,
        path: &RelPath,
    ) -> impl Future<Output = Result<Option<RelPath>, Self::Error>> + Send;

    /// What is in the way of a directory at `dir` (a book's, relative to the project
    /// root) now, if anything, by its path: `dir` itself or a parent inside the root
    /// that is there and is no directory, by the rule [`in_the_way`](Self::in_the_way)
    /// gives a parent. A directory there, or a link to one, is none: documents are
    /// read from it and made in it.
    fn dir_in_the_way(
        &self,
        dir: &RelPath,
    ) -> impl Future<Output = Result<Option<RelPath>, Self::Error>> + Send;

    /// Write the file at `path` (relative to the project root) with this front matter
    /// and body. The entries are values; their text (YAML) is the adapter's, and must
    /// read back through [`DocSource`] into the same entries.
    ///
    /// Create-only: a file already at the path is never replaced, and is
    /// [`Creation::AlreadyThere`], not an error. Missing directories are made. Writing
    /// over a file is another port, [`DocReplace`], which only `fmt` calls. A write
    /// that fails after the file was made removes that file (and no other), so a
    /// failure leaves no part of a document behind.
    fn create(
        &self,
        path: &RelPath,
        entries: &BTreeMap<String, RawValue>,
        body: &str,
    ) -> impl Future<Output = Result<Creation, Self::Error>> + Send;
}

/// Writing the file of a project's declaration, the file [`BookSchemaSource`] reads.
pub trait DeclarationWrite: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The file, relative to the project root.
    fn file(&self) -> RelPath;

    /// What is in the way of the file now, if anything, by the rule of
    /// [`DocWrite::in_the_way`]: a parent that is no directory, or anything at the
    /// file itself.
    fn in_the_way(&self) -> impl Future<Output = Result<Option<RelPath>, Self::Error>> + Send;

    /// Write the file with `text`, as it is. Missing directories are made.
    ///
    /// Create-only, as [`DocWrite::create`]: something already at the file is never
    /// replaced, and is [`Creation::AlreadyThere`], not an error. A write that fails
    /// after the file was made removes that file, as `DocWrite::create` does.
    fn create(
        &self,
        text: &DeclarationText,
    ) -> impl Future<Output = Result<Creation, Self::Error>> + Send;
}

/// What [`DocReplace::replace_read`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replacement {
    /// The file was replaced.
    Replaced,
    /// The file is no longer what was read (changed, or gone); nothing was written.
    Changed,
}

/// Writing over a document's file that is there: the one write that replaces a
/// file, kept apart from [`DocWrite::create`], which never does (so `new` cannot
/// overwrite).
pub trait DocReplace: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Replace the file at `path` (relative to the project root), which a reading
    /// just read as `read`, by `text`, the text [`DocSource::forms`] gave for it in
    /// the form.
    ///
    /// The file is compared with `read` just before it is replaced: one that changed
    /// or is gone by then is [`Replacement::Changed`], not an error, and is left as
    /// it is. The comparison and the replacement are two steps, so an edit saved in
    /// the short time between them is overwritten, and a file removed in it is
    /// written again. The file is replaced whole: it holds `read` or `text`, never a
    /// part of either, so a run that fails part way leaves each file it wrote a
    /// sound file of its own.
    fn replace_read(
        &self,
        path: &RelPath,
        read: &[u8],
        text: &[u8],
    ) -> impl Future<Output = Result<Replacement, Self::Error>> + Send;
}

/// Who is acting.
pub trait ActorSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The name of the person (or tool) that judges, as it is declared.
    fn actor(&self) -> impl Future<Output = Result<Actor, Self::Error>> + Send;
}

/// The artifacts of a project, things outside the documents, by their names in the
/// spaces the adapters offer: what is there now.
///
/// The domain knows a space only by its name; what the names of a space are, and
/// what each comes to, is this port's answer. Every space goes through the same
/// methods, values and rules. The adapter reads the bytes and what a
/// [`ContentAddress`] is; what of the bytes a hash covers is the domain's
/// [`Comparison`], which the adapter applies to them and does not choose.
///
/// [`Comparison`]: crate::domain::shelf::artifact::Comparison
pub trait ArtifactSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The spaces it can resolve names in: what
    /// [`Schema::validate`](crate::domain::shelf::schema::Schema::validate) holds the
    /// declaration's spaces against.
    fn spaces(&self) -> BTreeSet<Space>;

    /// The presets of links to artifacts it offers, each by the adapter of a space
    /// for its own, as values: what
    /// [`expand`](crate::domain::shelf::schema::expand) takes as the built-in ones.
    /// The domain knows no preset of its own.
    fn presets(&self) -> BuiltinPresets;

    /// For each name asked, in its space, one answer of the four of an
    /// [`ArtifactReading`](crate::domain::shelf::artifact::ArtifactReading): there
    /// (its hash under each comparison asked, made with
    /// [`ArtifactReading::of_bytes`](crate::domain::shelf::artifact::ArtifactReading::of_bytes)
    /// from the bytes read, and where its content can be read back, when the adapter
    /// can say), absent, cannot tell now (with why), or not a name of the space (with
    /// why). Every name asked about has an answer; a name of a space it does not
    /// offer cannot be told. An error is for what stops the reading itself (a thing
    /// that is there and cannot be read); what a space's names are is each adapter's
    /// own rule, on its page.
    fn resolve(
        &self,
        asks: &ArtifactAsks,
    ) -> impl Future<Output = Result<ArtifactReadings, Self::Error>> + Send;

    /// The bytes of the artifact as they are now, or none when nothing is there or
    /// they cannot be read now: what `diff` sets beside the content that was
    /// approved.
    fn current(
        &self,
        artifact: &ArtifactKey,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;
}

/// Where the artifacts that are gone may have moved to, as each space's adapter can
/// see: what `moves` asks, and nothing else does.
///
/// A port of its own, beside [`ArtifactSource`] and not a method of it: that port
/// answers what is at a name now, in four ways, for every reading of the project,
/// and a space answers it whether or not it has any way to suggest; this one
/// searches (the content of what is there, the space's history since an approval)
/// and may say it has no way to. The domain decides which names are asked about
/// ([`PinnedNames`](crate::domain::shelf::ledger::PinnedNames)) and orders what comes
/// back ([`GoneNames::suggest`]); the adapter only looks.
pub trait MoveSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// For each of `gone`, names of `space` with nothing behind them now, the names
    /// it may have moved to, found by what each approval pinned of it (the hash
    /// under its comparison, the address, the revision the approval was made at),
    /// each with how it was found and whether something is there now; or that the
    /// space cannot suggest. What could not be looked at (a history the adapter
    /// cannot read) is a note of the name, never an error: an error is for what
    /// stops the looking itself.
    fn suggest(
        &self,
        space: &Space,
        gone: &[GoneName],
    ) -> impl Future<Output = Result<SpaceSuggestions, Self::Error>> + Send;
}

/// Whose content an address is of: a document's file, or an artifact of a space. The
/// address is read back by the adapter that named it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentOf {
    /// A document's file: the adapter of the documents reads it back.
    Doc,
    /// An artifact of this space: the adapter of the space reads it back.
    Artifact(Space),
}

/// Content read back by its address, as it was when the address was taken, by the
/// adapter that took it.
pub trait ContentStore: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The content at `address`, an address of `of`, or none when nothing can be
    /// read through it: it was never kept, is kept no longer, or the adapter of `of`
    /// cannot read back (a space with no such adapter is one). An error is for a
    /// store that cannot be asked at all.
    fn read_back(
        &self,
        of: &ContentOf,
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
    /// The artifacts, by space.
    type Artifacts: ArtifactSource;
    /// The content that was approved, read back.
    type Store: ContentStore;
    /// Where the artifacts that are gone may have moved to.
    type Moves: MoveSource;

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
    /// The source of suggested moves.
    fn moves(&self) -> &Self::Moves;
}

/// The ports that `judge`, `new_doc`, `init` and `fmt` are generic over: everything a
/// read has, and the things only a write needs.
pub trait ShelfWrite: ShelfRead {
    /// The ledger, to append to.
    type Append: LedgerAppend;
    /// The identities of new records and documents.
    type Ids: IdSource;
    /// The acting person.
    type Actor: ActorSource;
    /// The revision an approval is made at.
    type Revisions: RevisionSource;
    /// The files of new documents.
    type DocWriter: DocWrite;
    /// The files of documents that are there, written over in the form.
    type DocReplacer: DocReplace;
    /// The file of the declaration, written when a project is started.
    type DeclarationWriter: DeclarationWrite;

    /// The ledger, to write.
    fn append(&self) -> &Self::Append;
    /// The writer of new documents' files.
    fn doc_writer(&self) -> &Self::DocWriter;
    /// The writer over documents' files that are there.
    fn doc_replacer(&self) -> &Self::DocReplacer;
    /// The writer of the declaration's file.
    fn declaration_writer(&self) -> &Self::DeclarationWriter;
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
