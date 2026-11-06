//! What the use cases need from the outside world, declared by their consumer.
//!
//! Each port is a trait implemented by the `whyshelf-infra` crate (and by a fake
//! in tests). Three rules shape them all:
//!
//! - **Async without a runtime.** A method returns
//!   `impl Future<Output = Result<_, Self::Error>> + Send`, so an adapter may await
//!   a process or a database, and this crate names no runtime. There is no `dyn`:
//!   the use cases are generic over the [`Ports`] bundle.
//! - **A typed error per port.** Every port has `type Error`, and a failure
//!   reaches the caller as [`AppError::Port`](super::error::AppError::Port) with the
//!   cause kept.
//! - **Typed values across the boundary.** Ports take and return values of the
//!   domain or of this layer, never a SQL string, a command line or a JSON
//!   document. Parsing a file, rendering a generated file and talking to a
//!   version-control tool are the adapters' work.
//!
//! | port | gives the use cases |
//! |---|---|
//! | [`RevisionPort`] | the current revision, whether the tree is clean, whether a revision reached the baseline (or that this clone cannot tell), validation of a revision text |
//! | [`StorePort`] | the event log and the stored objects of the repo store and the private store, whether the private store is there, and the generated files |
//! | [`CurrentPort`] | the document files: a listing with stamps, and one file read and parsed (read only) |
//! | [`ScaffoldPort`] | creating a new document file from a template, never overwriting |
//! | [`CodeScanPort`] | the pins found in the test files at a given revision, and the tags that are none |
//! | [`TestRunPort`] | running the tests at a given revision, for a time |
//! | [`IndexCachePort`] | a cache of file stamps and searchable text (rebuildable, never a source of truth) |
//! | [`SecretScanPort`] | finding secrets in text before it is written |
//! | [`ClockPort`], [`IdPort`] | the time and new identifiers, which the domain never makes |
//!
//! # The ports of the corpus and the ledgers
//!
//! The model that reads the documents from their files and keeps judgements in two
//! ledgers (see [`domain::corpus`](crate::domain::corpus) and
//! [`domain::ledger`](crate::domain::ledger)) has ports of its own, bundled apart as
//! [`CorpusPorts`] so that the ports above, and the use cases that stand on them, are
//! not touched while the new ones are built beside them. They keep the three rules
//! above. Writing is by evidence: what can be written to the ledger or the object
//! store is made only by the commit step
//! ([`commit::record`](super::commit::record)), never by an adapter.
//!
//! | port | gives the use cases |
//! |---|---|
//! | [`DocumentSource`] | every document file of both roots, with its place, the hash of its bytes, its reading and its normalized text |
//! | [`LedgerStore`] | a ledger read whole (its records and the ones it could not decode), and a [`Recordable`] appended |
//! | [`ObjectStore`] | the text of a version put under its [`TextHash`] (unless purged) and read back |
//! | [`ActorSource`] | who is acting: a [`Judge`], with what stands behind it |
//! | [`KeySource`] | the keys that pin private upstreams, or that there are none to be had here |
//! | [`ConfirmPort`] | asking a person to confirm what cannot be undone |
//! | [`FileWriter`] | creating, replacing and removing a file in a root, checked against the bytes the caller saw |
//! | [`RevisionFactsPort`] | what version control says about the code: whether a change landed, the fingerprint of a change, who wrote a revision, what Git makes of a path |

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::num::NonZeroU32;

use crate::domain::actor::Timestamp;
use crate::domain::approval::TestsRun;
use crate::domain::corpus::{FileFacts, GitPlace, Location, Visibility};
use crate::domain::error::ValueError;
use crate::domain::event::{DocKind, Event, Recorded, StoreKind};
use crate::domain::finding::FindingType;
use crate::domain::hash::{BytesHash, ContentHash, TextHash};
use crate::domain::id::EntityId;
use crate::domain::integration::{BaselineFacts, ChangeFingerprint, UnknownWhy};
use crate::domain::judge::{Email, Judge, PersonName};
use crate::domain::judgement::JudgementId;
use crate::domain::keyring::PinKeyring;
use crate::domain::knowable::Knowable;
use crate::domain::ledger::Ledger;
use crate::domain::location::{CodeLocation, RelPath, TestFileSet};
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::pin::{MalformedPin, PinId};
use crate::domain::publication::{PlanDigest, PublicationPlan};
use crate::domain::reference::{Reach, Ref};
use crate::domain::revision::{IntegrationFact, Revision, RevisionId};
use crate::domain::stored::StoredJudgement;

use super::commit::record::{Recordable, Scanned};
use super::config::{PinSearch, TestCommand, TestTimeout};
use super::outcome::Refusal;
use super::projection::{ChangelogDoc, MapDoc};
use super::raw::{Candidate, NormalizedText, RawDocument, RawRevision};
use super::view::StatusReport;

/// Which port failed, for [`AppError::Port`](super::error::AppError::Port).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PortName {
    /// [`RevisionPort`].
    Revision,
    /// [`StorePort`].
    Store,
    /// [`CurrentPort`].
    Current,
    /// [`ScaffoldPort`].
    Scaffold,
    /// [`CodeScanPort`].
    CodeScan,
    /// [`TestRunPort`].
    TestRun,
    /// [`IndexCachePort`].
    IndexCache,
    /// [`SecretScanPort`].
    SecretScan,
    /// [`ClockPort`].
    Clock,
    /// [`IdPort`].
    Id,
    /// [`DocumentSource`].
    Documents,
    /// [`LedgerStore`].
    Ledger,
    /// [`ObjectStore`].
    Objects,
    /// [`ActorSource`].
    Actor,
    /// [`KeySource`].
    Keys,
    /// [`ConfirmPort`].
    Confirm,
    /// [`FileWriter`].
    Files,
    /// [`RevisionFactsPort`].
    RevisionFacts,
}

impl fmt::Display for PortName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Revision => "revision",
            Self::Store => "store",
            Self::Current => "current",
            Self::Scaffold => "scaffold",
            Self::CodeScan => "code-scan",
            Self::TestRun => "test-run",
            Self::IndexCache => "index-cache",
            Self::SecretScan => "secret-scan",
            Self::Clock => "clock",
            Self::Id => "id",
            Self::Documents => "documents",
            Self::Ledger => "ledger",
            Self::Objects => "objects",
            Self::Actor => "actor",
            Self::Keys => "keys",
            Self::Confirm => "confirm",
            Self::Files => "files",
            Self::RevisionFacts => "revision-facts",
        })
    }
}

/// The directory a command runs in, for the context line. Display only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRoot(NonBlank);

impl WorktreeRoot {
    /// A name for the working directory; blank is refused.
    pub fn new(text: impl Into<String>) -> Result<Self, ValueError> {
        NonBlank::new(text).map(Self)
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for WorktreeRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// The version-control side: revisions of the code.
///
/// The model speaks only of a revision, of being integrated into a baseline and of
/// a clean tree; branch names and hash lengths stay behind this port.
pub trait RevisionPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Check a revision text. `None` when it names no revision; the adapter
    /// builds the [`RevisionId`] only after its own check.
    fn validate(
        &self,
        text: &RawRevision,
    ) -> impl Future<Output = Result<Option<RevisionId>, Self::Error>> + Send;

    /// The revision of the working tree (the tip it stands on); `None` before the
    /// first commit, when there is no tip yet. That is a state of the repository, not
    /// a failure of the port: the use cases say a commit is needed
    /// ([`AppError::NoCommit`](super::error::AppError::NoCommit)).
    fn current(&self) -> impl Future<Output = Result<Option<RevisionId>, Self::Error>> + Send;

    /// Whether the working tree has no uncommitted change.
    fn is_clean(&self) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    /// Whether the revision has reached the baseline: yes, no, or that it cannot be
    /// told here ([`IntegrationFact::Unknown`]: the baseline or the revision is not
    /// in this clone). A question the clone cannot answer is an answer, not an
    /// error, so that a shallow or single-branch checkout does not stop every
    /// command; an error is for a failure of the tool itself.
    fn integration(
        &self,
        revision: &RevisionId,
    ) -> impl Future<Output = Result<IntegrationFact, Self::Error>> + Send;

    /// A name for the current line of work (a branch, say). Display only.
    fn label(&self) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;

    /// The working directory, for the context line.
    fn worktree(&self) -> impl Future<Output = Result<WorktreeRoot, Self::Error>> + Send;
}

/// Whether an append wrote something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    /// The record of an event, or the object, was new.
    Written,
    /// It was there already; an append is idempotent, so this is success.
    AlreadyPresent,
}

/// The generated files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExportKind {
    /// The map of identifiers, relations and tags.
    Map,
    /// The status as Markdown.
    Status,
    /// The changelog as Markdown.
    Changelog,
}

/// A generated file as a value, for the adapter to render.
///
/// Rendering is deterministic (ordered keys, fixed indentation, a marker with
/// the `export` hash of the content) and belongs to the adapter; the use cases
/// only hand over what the file says.
#[derive(Debug, Clone, Copy)]
pub enum ExportDoc<'a> {
    /// The map.
    Map(&'a MapDoc),
    /// The status.
    Status(&'a StatusReport),
    /// The changelog.
    Changelog(&'a ChangelogDoc),
}

impl ExportDoc<'_> {
    /// Which file this is.
    pub fn kind(&self) -> ExportKind {
        match self {
            Self::Map(_) => ExportKind::Map,
            Self::Status(_) => ExportKind::Status,
            Self::Changelog(_) => ExportKind::Changelog,
        }
    }
}

/// What the adapter found when it looked at a generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFile {
    /// There is no such file.
    Absent,
    /// The file exists but carries no marker, or one that cannot be read.
    Unmarked,
    /// The file has a marker.
    Marked(ExportParts),
}

/// A generated file taken apart for the drift check
/// ([`export_drift`](crate::domain::lint::export_drift)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportParts {
    marker: ContentHash,
    body: Vec<u8>,
    rebuilt: Vec<u8>,
}

impl ExportParts {
    /// The marker found, the file without its marker, and what rendering the
    /// current sources gives, also without a marker.
    pub fn new(marker: ContentHash, body: Vec<u8>, rebuilt: Vec<u8>) -> Self {
        Self {
            marker,
            body,
            rebuilt,
        }
    }

    /// The marker's `export` hash.
    pub fn marker(&self) -> &ContentHash {
        &self.marker
    }

    /// The file without its marker.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// A fresh render without a marker.
    pub fn rebuilt(&self) -> &[u8] {
        &self.rebuilt
    }
}

/// The two stores: the event log, the stored objects and the generated files.
///
/// `scope` is the domain's [`StoreKind`]: the repo store is committed with the
/// project, the private store lives outside it.
pub trait StorePort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Append a record of an event: the event and the time it was written.
    ///
    /// Idempotent per record. A record is the pair of the event's identifier and the
    /// time (see [`RecordDigest`](crate::domain::event::RecordDigest)): the same
    /// pair again is already there, [`AppendOutcome::AlreadyPresent`]; the same
    /// event at another time is a further record, kept beside the first and never
    /// replacing it, [`AppendOutcome::Written`].
    ///
    /// The contract every implementation keeps is
    /// [`store_contract`](super::store_contract), and a test runs it on each.
    fn append(
        &self,
        scope: StoreKind,
        event: &Event,
        at: &Timestamp,
    ) -> impl Future<Output = Result<AppendOutcome, Self::Error>> + Send;

    /// Every event of the store, once, with the time of each of its records
    /// ([`Recorded`]).
    ///
    /// A store that is not there has no events: this cannot tell it from an empty
    /// one, which [`private_reach`](Self::private_reach) does.
    fn events(
        &self,
        scope: StoreKind,
    ) -> impl Future<Output = Result<Vec<Recorded>, Self::Error>> + Send;

    /// Whether the private store is there to be read: a store that is there and has
    /// no event yet is [`Reach::Available`], a store that is not there (a public
    /// clone, a CI machine) is [`Reach::Absent`].
    ///
    /// A reference into the private store that names nothing is a typing mistake
    /// in the first case and a thing this environment cannot see in the second.
    fn private_reach(&self) -> impl Future<Output = Result<Reach, Self::Error>> + Send;

    /// Store the text of a version under its `raw` hash. Idempotent.
    fn put_object(
        &self,
        scope: StoreKind,
        raw: &ContentHash,
        text: &NormalizedText,
    ) -> impl Future<Output = Result<AppendOutcome, Self::Error>> + Send;

    /// The text stored under a `raw` hash, if there is one.
    fn get_object(
        &self,
        scope: StoreKind,
        raw: &ContentHash,
    ) -> impl Future<Output = Result<Option<NormalizedText>, Self::Error>> + Send;

    /// Delete a stored object (after a secret slipped through). `false` when
    /// there was none.
    fn purge_object(
        &self,
        scope: StoreKind,
        raw: &ContentHash,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    /// Render a generated file and write it.
    fn write_export(
        &self,
        scope: StoreKind,
        doc: &ExportDoc<'_>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Look at the generated file as it is on disk against a fresh render of
    /// `doc`.
    fn inspect_export(
        &self,
        scope: StoreKind,
        doc: &ExportDoc<'_>,
    ) -> impl Future<Output = Result<ExportFile, Self::Error>> + Send;
}

/// An opaque stamp of a file's modification time, compared for equality only.
///
/// The time of a file can fail to be read. That is a stamp of its own,
/// [`Mtime::Unknown`], which [`Mtime::matches`] nothing, not even itself: a file
/// whose time is not known is read again each time, never taken as unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mtime {
    /// The time was read; the adapter decides the unit.
    Known(u64),
    /// The time could not be read.
    Unknown,
}

impl Mtime {
    /// A stamp that was read. The adapter decides the unit; the use cases never
    /// order it.
    pub fn new(stamp: u64) -> Self {
        Self::Known(stamp)
    }

    /// The stamp, when it was read.
    pub fn known(&self) -> Option<u64> {
        match self {
            Self::Known(stamp) => Some(*stamp),
            Self::Unknown => None,
        }
    }

    /// Whether this stamp says the file is the same as when `other` was taken:
    /// both were read, and they are equal.
    pub fn matches(&self, other: &Mtime) -> bool {
        matches!((self, other), (Self::Known(a), Self::Known(b)) if a == b)
    }
}

/// What a listing says of a file: enough to see that it changed without
/// reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    path: RelPath,
    mtime: Mtime,
    size: u64,
}

impl FileStat {
    /// A stat.
    pub fn new(path: RelPath, mtime: Mtime, size: u64) -> Self {
        Self { path, mtime, size }
    }

    /// The path.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// The modification stamp.
    pub fn mtime(&self) -> Mtime {
        self.mtime
    }

    /// The size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Whether another stat of the same path says the file is unchanged. A stat
    /// whose time is unknown says nothing of the kind.
    pub fn same_as(&self, other: &FileStat) -> bool {
        self.mtime.matches(&other.mtime) && self.size == other.size
    }
}

/// Why a path that was found cannot be the path of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathProblem {
    /// The name is not valid UTF-8.
    NotUtf8,
    /// The text is not a path a document can have, for this reason.
    Invalid(String),
}

impl fmt::Display for PathProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => f.write_str("its name is not valid UTF-8"),
            Self::Invalid(reason) => write!(f, "it is not a path a document can have: {reason}"),
        }
    }
}

/// A file or directory the listing found and could not name: it is not in the
/// listing, and not being there is not the same as not having been seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadablePath {
    shown: String,
    problem: PathProblem,
}

impl UnreadablePath {
    /// A path that cannot be named, as it can be shown (lossily, for a name that is
    /// not UTF-8), and why.
    pub fn new(shown: String, problem: PathProblem) -> Self {
        Self { shown, problem }
    }

    /// The path, as it can be shown.
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// Why it cannot be named.
    pub fn problem(&self) -> &PathProblem {
        &self.problem
    }
}

impl fmt::Display for UnreadablePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.shown, self.problem)
    }
}

/// What a listing found: the files, and the paths it could not name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    files: Vec<FileStat>,
    unreadable: Vec<UnreadablePath>,
}

impl Listing {
    /// A listing.
    pub fn new(files: Vec<FileStat>, unreadable: Vec<UnreadablePath>) -> Self {
        Self { files, unreadable }
    }

    /// The files, by path.
    pub fn files(&self) -> &[FileStat] {
        &self.files
    }

    /// The paths that were found and could not be named: reported, never dropped.
    pub fn unreadable(&self) -> &[UnreadablePath] {
        &self.unreadable
    }

    /// The files and the unreadable paths apart.
    pub fn into_parts(self) -> (Vec<FileStat>, Vec<UnreadablePath>) {
        (self.files, self.unreadable)
    }
}

/// The document files. Read only: the tool never rewrites a user's file.
pub trait CurrentPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The document files under these paths (a path may be a directory or a
    /// file). A path that does not exist gives nothing; a file whose path cannot be
    /// named is reported in the listing, not left out of it.
    fn list(&self, roots: &[RelPath]) -> impl Future<Output = Result<Listing, Self::Error>> + Send;

    /// Read and parse one file. `None` when it does not exist.
    ///
    /// `kind` is the kind the file is registered as, or that the caller asks it to be
    /// read as: the file is read as that kind, and a key of another kind is reported
    /// ([`ForeignSection`](crate::domain::front::ForeignSection)). `None` is for a
    /// file nobody registered, whose kind is [`DocKind::guess`]ed from its keys and
    /// left to the caller when the keys do not say.
    fn read(
        &self,
        path: &RelPath,
        kind: Option<DocKind>,
    ) -> impl Future<Output = Result<Option<Candidate>, Self::Error>> + Send;
}

/// What a new document file starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
    /// A Finding of this type.
    Finding {
        /// The type, which decides the metadata the file asks for.
        finding_type: FindingType,
        /// The title.
        title: NonBlank,
    },
    /// A Spec standing on these Findings.
    Spec {
        /// The grounds, written as the file will write them (an alias, or
        /// `private:<id>` for a public file that stands on a private Finding).
        grounds: NonEmptyVec<Ref>,
    },
    /// A Design for this Spec.
    Design {
        /// The Spec, written as the file will write it.
        spec: Ref,
    },
}

/// Whether a scaffold created the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateOutcome {
    /// The file was created.
    Created,
    /// A file was there already and was left alone.
    AlreadyExists,
}

/// Creating a new document file. Only the scaffold use case holds it, so every
/// other use case stays read only on files.
pub trait ScaffoldPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Create the file from the template, never overwriting.
    fn create_new(
        &self,
        path: &RelPath,
        template: &Template,
    ) -> impl Future<Output = Result<CreateOutcome, Self::Error>> + Send;

    /// Take away a file this process created from `template`, if it is still exactly
    /// what was written; a file that is gone or has other content is left alone.
    /// Says whether the file was removed.
    ///
    /// The undo of [`create_new`](Self::create_new) for a use case that fails after
    /// it, so a failed scaffold leaves no file the next attempt would stop on.
    fn remove_created(
        &self,
        path: &RelPath,
        template: &Template,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}

/// What a scan of the test files found: the pins, and the tags that are none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinScan {
    /// Each pin, with where it was found.
    pub pins: BTreeMap<PinId, Vec<CodeLocation>>,
    /// The tags that were found and whose name is no pin's (`@pin: PIN-foo`), in the
    /// order found.
    pub malformed: Vec<MalformedPin>,
}

/// Finding the pins declared in test files.
pub trait CodeScanPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The pins in the test files at `revision`, with where each was found. Reads
    /// the revision, not the working tree.
    ///
    /// A file is a test file when it is under one of `scan_dirs` and `files` names it
    /// (see [`TestFileSet`]). A mark `search` cuts out of a line is read as a pin in
    /// its vocabulary ([`PinId::parse_in`]); one that is none is reported in
    /// [`PinScan::malformed`], never dropped.
    fn pins_at(
        &self,
        revision: &RevisionId,
        scan_dirs: &[RelPath],
        files: &TestFileSet,
        search: &PinSearch,
    ) -> impl Future<Output = Result<PinScan, Self::Error>> + Send;
}

/// Running the tests.
pub trait TestRunPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Run the command at `revision` (in the working tree when it is the
    /// current clean one, in a temporary checkout otherwise), for at most `timeout`.
    ///
    /// A command that is still running when the time is up is stopped, and the
    /// answer is [`TestsRun::TimedOut`], not an error: no result came out, which is a
    /// fact about the run that the gate reports.
    fn run(
        &self,
        revision: &RevisionId,
        command: &TestCommand,
        timeout: TestTimeout,
    ) -> impl Future<Output = Result<TestsRun, Self::Error>> + Send;
}

/// One file in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    stat: FileStat,
    raw: ContentHash,
    body: String,
}

impl CacheEntry {
    /// An entry: the stamp the file had, the `raw` hash of its text and the
    /// text to search (free text).
    pub fn new(stat: FileStat, raw: ContentHash, body: String) -> Self {
        Self { stat, raw, body }
    }

    /// The stamp.
    pub fn stat(&self) -> &FileStat {
        &self.stat
    }

    /// The path.
    pub fn path(&self) -> &RelPath {
        self.stat.path()
    }

    /// The `raw` hash of the text when it was cached.
    pub fn raw(&self) -> &ContentHash {
        &self.raw
    }

    /// The searchable text.
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// The cache: stamps (to see what changed without reading) and searchable text.
///
/// It holds typed values only; there is no way to hand the use cases a SQL
/// string, so the table layout is free to change. It can be thrown away and
/// rebuilt at any time.
pub trait IndexCachePort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Every entry.
    fn load(&self) -> impl Future<Output = Result<Vec<CacheEntry>, Self::Error>> + Send;

    /// Insert or replace entries.
    fn upsert(
        &self,
        entries: &[CacheEntry],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Remove the entries of these paths.
    fn remove(&self, paths: &[RelPath]) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Replace the whole cache.
    fn rebuild(
        &self,
        entries: &[CacheEntry],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// The paths whose text contains the words, best first, at most `limit`.
    fn search_fts(
        &self,
        text: &NonBlank,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<RelPath>, Self::Error>> + Send;
}

/// A secret found in a text.
///
/// It names the rule that matched and the line, never the secret, so the report
/// is safe to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretHit {
    rule: NonBlank,
    line: NonZeroU32,
}

impl SecretHit {
    /// A hit of the named rule (free text) at a line from 1.
    pub fn new(rule: NonBlank, line: NonZeroU32) -> Self {
        Self { rule, line }
    }

    /// The rule that matched.
    pub fn rule(&self) -> &str {
        self.rule.as_str()
    }

    /// The line from 1.
    pub fn line(&self) -> u32 {
        self.line.get()
    }
}

/// Looking for secrets before text is written.
pub trait SecretScanPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The secrets in the text.
    fn scan(
        &self,
        text: &NormalizedText,
    ) -> impl Future<Output = Result<Vec<SecretHit>, Self::Error>> + Send;
}

/// The time. The domain never reads a clock.
pub trait ClockPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Now, validated as RFC 3339 by the adapter.
    fn now(&self) -> impl Future<Output = Result<Timestamp, Self::Error>> + Send;
}

/// New identifiers. The domain never makes one.
pub trait IdPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// A new UUID v7.
    fn new_entity_id(&self) -> impl Future<Output = Result<EntityId, Self::Error>> + Send;
}

/// All the ports of a workspace, bundled.
///
/// One trait with associated types instead of ten type parameters: a use case
/// is a method of `Workspace<P: Ports>`, and the adapter crate implements `Ports`
/// once.
pub trait Ports: Send + Sync {
    /// The revision port.
    type Revision: RevisionPort;
    /// The store port.
    type Store: StorePort;
    /// The current-files port.
    type Current: CurrentPort;
    /// The scaffold port.
    type Scaffold: ScaffoldPort;
    /// The code scan port.
    type CodeScan: CodeScanPort;
    /// The test-run port.
    type TestRun: TestRunPort;
    /// The cache port.
    type Cache: IndexCachePort;
    /// The secret scan port.
    type Secret: SecretScanPort;
    /// The clock.
    type Clock: ClockPort;
    /// The identifier source.
    type Id: IdPort;

    /// The revision port.
    fn revision(&self) -> &Self::Revision;
    /// The store port.
    fn store(&self) -> &Self::Store;
    /// The current-files port.
    fn current(&self) -> &Self::Current;
    /// The scaffold port.
    fn scaffold(&self) -> &Self::Scaffold;
    /// The code scan port.
    fn code_scan(&self) -> &Self::CodeScan;
    /// The test-run port.
    fn test_run(&self) -> &Self::TestRun;
    /// The cache port.
    fn cache(&self) -> &Self::Cache;
    /// The secret scan port.
    fn secret(&self) -> &Self::Secret;
    /// The clock.
    fn clock(&self) -> &Self::Clock;
    /// The identifier source.
    fn id(&self) -> &Self::Id;
}

// --- the ports of the corpus and the ledgers --------------------------------------

/// The document files of both roots.
///
/// Read only. An adapter that is built for the corpus lists every document file of
/// the public root and of the private one, whatever it holds.
pub trait DocumentSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Every document file, once: where it sits, the hash of its bytes
    /// ([`FileFacts`]), and what reading it gave with its normalized text
    /// ([`RawDocument`]).
    ///
    /// The adapter knows the root a file is under and its path, not what Git makes of
    /// it: it states [`Location::git`] as [`Knowable::Unknown`]. The caller asks
    /// [`RevisionFactsPort::git_place`] and builds the [`Location`] again with the
    /// answer before it builds the corpus, so that the one place Git is asked is the
    /// revision port.
    ///
    /// A file that cannot be read as a document is listed with the
    /// [`Reading`](crate::domain::corpus::Reading) that says so; it is never left
    /// out.
    fn list(
        &self,
    ) -> impl Future<Output = Result<Vec<(FileFacts, RawDocument)>, Self::Error>> + Send;
}

/// Why a record in a ledger could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordProblem {
    /// The record's bytes are not a record this build can decode.
    Undecodable,
    /// The record is stored under the identifier of another.
    NameDisagrees,
}

impl fmt::Display for RecordProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Undecodable => "it cannot be decoded",
            Self::NameDisagrees => "it is stored under the identifier of another record",
        })
    }
}

/// A record the ledger holds and the adapter could not read: reported, never dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorruptRecord {
    id: JudgementId,
    problem: RecordProblem,
}

impl CorruptRecord {
    /// A record that could not be read, by the identifier it is stored under.
    pub fn new(id: JudgementId, problem: RecordProblem) -> Self {
        Self { id, problem }
    }

    /// The identifier it is stored under.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// Why it could not be read.
    pub fn problem(&self) -> RecordProblem {
        self.problem
    }
}

impl fmt::Display for CorruptRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the record {}: {}", self.id, self.problem)
    }
}

/// What reading one ledger gave: the records, and the ones it could not read.
///
/// Two records of one identifier that differ are both in `records`; telling them
/// apart is the domain's ([`Ledger::from_stored`]), not the adapter's. The records are
/// [`StoredJudgement`]s, which the adapter makes with
/// [`StoredJudgement::decode`] and nothing else: the domain owns the reading of the
/// bytes, and a record read back cannot start a new one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerRead {
    records: Vec<StoredJudgement>,
    corrupt: Vec<CorruptRecord>,
}

impl LedgerRead {
    /// What was read.
    pub fn new(records: Vec<StoredJudgement>, corrupt: Vec<CorruptRecord>) -> Self {
        Self { records, corrupt }
    }

    /// The records that were decoded, every one found.
    pub fn records(&self) -> &[StoredJudgement] {
        &self.records
    }

    /// The records that could not be read, for `fsck`.
    pub fn corrupt(&self) -> &[CorruptRecord] {
        &self.corrupt
    }

    /// The records as a ledger (a set, merged by union).
    pub fn ledger(&self) -> Ledger {
        Ledger::from_stored(self.records.iter().cloned())
    }
}

/// The two ledgers, public and private.
///
/// A ledger is an append-only set of judgements. The only thing that can be written
/// is a [`Recordable`], which can be made only by the commit step: the adapter cannot
/// write an approval whose object was not stored, nor a record to a ledger other than
/// the one the rule chose.
pub trait LedgerStore: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The records of one ledger, and the ones that could not be read. A ledger that
    /// is not there (a clone without the private ledger) reads as empty.
    fn read(
        &self,
        side: Visibility,
    ) -> impl Future<Output = Result<LedgerRead, Self::Error>> + Send;

    /// Write a record to the ledger the [`Recordable`] names.
    ///
    /// Idempotent per record: the same record (the same canonical bytes) again is
    /// already there, and that is success. A record of the same identifier with other
    /// bytes is kept beside the first, never in its place, so the ledger reads it as
    /// corrupt.
    ///
    /// A [`Purge`](crate::domain::judgement::JudgementKind::Purge) record also takes
    /// the text it names out of the [`ObjectStore`] of the same side, and the store
    /// remembers the hash as purged (see there).
    fn append(&self, record: &Recordable) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Whether a text was stored.
///
/// A value-free enum: the port reports what happened and the commit step makes the
/// evidence ([`StoredObject`](super::commit::record::StoredObject)); an adapter has no
/// way to make one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Put {
    /// The text is stored (it was, or it already was).
    Stored,
    /// The text was purged. It is not put back, and nothing was stored.
    Purged,
}

/// The texts of the versions that were judged.
///
/// The objects form a set that can also delete (a 2P-set): a hash that is purged is
/// gone for good. A store that has seen the purge answers [`Put::Purged`] to a put of
/// that hash, and gives nothing for it; a branch that diverged before the purge and
/// stored the hash is read as gone after the merge.
pub trait ObjectStore: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Store the text of a [`Scanned`] under its key, in the store of its ledger.
    /// Idempotent.
    fn put(&self, text: &Scanned<'_>) -> impl Future<Output = Result<Put, Self::Error>> + Send;

    /// The text under `key` in one side's store; `None` when there is none, and when
    /// the hash was purged.
    fn get(
        &self,
        side: Visibility,
        key: TextHash,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;
}

/// Who is acting.
pub trait ActorSource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The judge of this run: the name, the mail if the person chose to record it,
    /// and what stands behind it. Where each comes from, and why a process's own
    /// environment is not trusted to say "human", is the adapter's contract.
    fn actor(&self) -> impl Future<Output = Result<Judge, Self::Error>> + Send;
}

/// The keys that pin private upstreams.
pub trait KeySource: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The keyring of this person, or [`Knowable::Unknown`] when this environment has
    /// no place to look for one. A person who has no key has an empty keyring
    /// ([`PinKeyring::empty`]), which is a known thing.
    fn keys(&self) -> impl Future<Output = Result<Knowable<PinKeyring>, Self::Error>> + Send;
}

/// What a person answered.
///
/// A value-free enum, like [`Put`]: only the commit step turns a
/// [`Yes`](Self::Yes) into a
/// [`HumanConfirmation`](super::commit::confirm::HumanConfirmation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmed {
    /// The person typed back what was shown.
    Yes,
    /// The person was asked, and did not confirm.
    No,
    /// There is nobody to ask here (no terminal, a tool server).
    Unavailable,
}

/// What a confirmation is about, for the person to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfirmAction {
    /// Publish a document: its records and versions go where Git keeps them.
    Publish,
    /// Remove a private copy whose bytes differ from the public file.
    RemoveDifferingCopy,
    /// Take earlier decisions in as approvals.
    Adopt,
}

/// What the person is shown beside the digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSummary {
    action: ConfirmAction,
    count: usize,
}

impl PlanSummary {
    /// A summary: what is to be done, and how many things it is done to.
    pub fn new(action: ConfirmAction, count: usize) -> Self {
        Self { action, count }
    }

    /// What is to be done.
    pub fn action(&self) -> ConfirmAction {
        self.action
    }

    /// How many things (records, files) it is done to.
    pub fn count(&self) -> usize {
        self.count
    }
}

/// What a person is asked: what to type back, and what to read first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmPrompt {
    plan: PlanDigest,
    summary: PlanSummary,
}

impl ConfirmPrompt {
    /// A prompt for the plan with this digest.
    pub fn new(plan: PlanDigest, summary: PlanSummary) -> Self {
        Self { plan, summary }
    }

    /// The prompt for a publication plan, or `None` when the plan needs no
    /// confirmation (it has nothing that cannot be undone).
    pub fn of_publication(plan: &PublicationPlan) -> Option<Self> {
        if !plan.needs_human_confirmation() {
            return None;
        }
        let summary = if plan.irreversible().is_some() {
            PlanSummary::new(ConfirmAction::Publish, plan.records().len())
        } else {
            PlanSummary::new(ConfirmAction::RemoveDifferingCopy, 1)
        };
        Some(Self::new(plan.digest(), summary))
    }

    /// The digest of the plan: what the person types back, in its short form
    /// ([`PlanDigest::short`]).
    pub fn plan(&self) -> PlanDigest {
        self.plan
    }

    /// What the person is shown.
    pub fn summary(&self) -> PlanSummary {
        self.summary
    }
}

/// A person's confirmation of what cannot be undone.
///
/// The command line's adapter answers from a terminal, by the person typing back the
/// short digest, and says [`Confirmed::Unavailable`] when there is no terminal. The
/// tool server's adapter always says [`Confirmed::Unavailable`]. A flag or an argument
/// is never an answer: whoever follows the next step can set one.
///
/// Nothing is run on a [`Confirmed::Yes`] alone: the commit step makes a
/// [`HumanConfirmation`](super::commit::confirm::HumanConfirmation) of it, and the run
/// checks it against the plan made again just before.
pub trait ConfirmPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Ask.
    fn ask(
        &self,
        prompt: &ConfirmPrompt,
    ) -> impl Future<Output = Result<Confirmed, Self::Error>> + Send;
}

/// What writing a file came to.
///
/// Everything but [`Done`](Self::Done) and [`Exists`](Self::Exists) is the file not
/// having been written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    /// The file was written.
    Done,
    /// `create_new` found a file there already.
    Exists,
    /// The file's bytes are not the bytes the caller compared with, or changed
    /// before the replacement.
    ChangedUnderfoot,
    /// The file has more than one hard link; replacing it would cut them.
    MultipleLinks,
    /// The path is a symbolic link, or not a regular file.
    NotRegularFile,
    /// The file's group cannot be kept on the replacement, and the group has access.
    CannotKeepGroup,
}

impl Write {
    /// The refusal this stands for, for the file at `at`; `None` for
    /// [`Done`](Self::Done), and for [`Exists`](Self::Exists), which only the caller
    /// can read (a retry is success; a publication's destination is a refusal).
    pub fn refusal(self, at: &RelPath) -> Option<Refusal> {
        match self {
            Self::Done | Self::Exists => None,
            Self::ChangedUnderfoot => Some(Refusal::ChangedUnderfoot(at.clone())),
            Self::MultipleLinks => Some(Refusal::MultipleLinks(at.clone())),
            Self::NotRegularFile => Some(Refusal::NotRegularFile(at.clone())),
            Self::CannotKeepGroup => Some(Refusal::CannotKeepGroup(at.clone())),
        }
    }
}

/// Writing a document file: the one way the tool changes a user's file, for an import
/// and a publication.
///
/// The contract is the adapter's to keep, and it checks and writes in one motion, not
/// "check, then write":
///
/// - The root's directory is opened and the path is walked from it. The last element
///   is not followed: a symbolic link, or anything but a regular file, is
///   [`Write::NotRegularFile`].
/// - [`create_new`](Self::create_new) creates the file and fails if it exists
///   ([`Write::Exists`]).
/// - [`rewrite`](Self::rewrite) reads the file it opened and compares its bytes with
///   `expect` ([`Write::ChangedUnderfoot`] if they differ), refuses a file with
///   more than one hard link ([`Write::MultipleLinks`]), writes a temporary file in
///   the same directory with the mode and owner of the original (`0600` first), keeps
///   the group or refuses ([`Write::CannotKeepGroup`]), compares the file once more
///   just before the replacement, and replaces it.
/// - [`remove`](Self::remove) compares and removes.
///
/// A write by another editor between the last comparison and the replacement cannot
/// be shut out against an editor that takes no lock, and extended attributes are not
/// copied.
///
/// `expect` is a [`BytesHash`], the hash of the bytes on disk: it tells a changed
/// alias or tag, and is right for a file with CRLF line ends.
pub trait FileWriter: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Create the file with this content.
    fn create_new(
        &self,
        at: &Location,
        text: &[u8],
    ) -> impl Future<Output = Result<Write, Self::Error>> + Send;

    /// Replace the file, whose bytes are `expect`, with this content.
    fn rewrite(
        &self,
        at: &Location,
        expect: BytesHash,
        text: &[u8],
    ) -> impl Future<Output = Result<Write, Self::Error>> + Send;

    /// Remove the file, whose bytes are `expect`.
    fn remove(
        &self,
        at: &Location,
        expect: BytesHash,
    ) -> impl Future<Output = Result<Write, Self::Error>> + Send;
}

/// The person who wrote a revision: a name, and a mail if there is one.
///
/// What the revision port knows of the author of a commit. It is compared with a
/// [`Judge`] by name and, when both have one, by mail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Author {
    name: PersonName,
    email: Option<Email>,
}

impl Author {
    /// An author from the values the adapter read.
    pub fn new(name: PersonName, email: Option<Email>) -> Self {
        Self { name, email }
    }

    /// The name.
    pub fn name(&self) -> &PersonName {
        &self.name
    }

    /// The mail, if the revision has one.
    pub fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }

    /// Whether this author and the judge are one person: the same name, and the same
    /// mail when both have one (the judge's principal does not count).
    pub fn is_judge(&self, judge: &Judge) -> bool {
        self.name == *judge.name()
            && match (&self.email, judge.email()) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

/// The question put to the baseline about one approved change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineQuery<'a> {
    approved: &'a Revision,
    fingerprint: Option<&'a ChangeFingerprint>,
}

impl<'a> BaselineQuery<'a> {
    /// The change approved at `approved`, with the fingerprint the approval recorded,
    /// if it recorded one.
    pub fn new(approved: &'a Revision, fingerprint: Option<&'a ChangeFingerprint>) -> Self {
        Self {
            approved,
            fingerprint,
        }
    }

    /// The revision the change was approved at.
    pub fn approved(&self) -> &'a Revision {
        self.approved
    }

    /// The fingerprint the approval recorded.
    pub fn fingerprint(&self) -> Option<&'a ChangeFingerprint> {
        self.fingerprint
    }
}

/// What the baseline says of a change, and why it cannot say more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineAnswer {
    facts: BaselineFacts,
    cannot_tell: UnknownWhy,
}

impl BaselineAnswer {
    /// The facts, and the reason for whichever of them is unknown (used only when the
    /// verdict turns on an unknown fact).
    pub fn new(facts: BaselineFacts, cannot_tell: UnknownWhy) -> Self {
        Self { facts, cannot_tell }
    }

    /// The facts.
    pub fn facts(&self) -> BaselineFacts {
        self.facts
    }

    /// The reason a fact is unknown.
    pub fn cannot_tell(&self) -> UnknownWhy {
        self.cannot_tell
    }
}

/// What version control says about the code, in the terms of the corpus model.
///
/// The ports of the model above ([`RevisionPort`]) speak of a revision and a clean
/// tree and are kept as they are; this one adds the facts the new rules take as
/// values. The domain knows no Git.
pub trait RevisionFactsPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Whether the approved change reached the baseline, by ancestry and by its
    /// hunks. A fact this clone cannot tell is [`Knowable::Unknown`], with the
    /// reason; it is an answer, not an error.
    fn baseline_facts(
        &self,
        query: &BaselineQuery<'_>,
    ) -> impl Future<Output = Result<BaselineAnswer, Self::Error>> + Send;

    /// The fingerprint of the change approved at `approved`, recorded by the approval;
    /// `None` when there is nothing to fingerprint (the revision is the baseline, or
    /// the change has no hunk) or the clone cannot tell.
    fn change_fingerprint(
        &self,
        approved: &Revision,
    ) -> impl Future<Output = Result<Option<ChangeFingerprint>, Self::Error>> + Send;

    /// Who wrote the revision; unknown for a private root outside Git or a patch.
    fn author(
        &self,
        at: &Revision,
    ) -> impl Future<Output = Result<Knowable<Author>, Self::Error>> + Send;

    /// What Git makes of a path under a root. Asked before the
    /// [`Location`] of the file is built.
    fn git_place(
        &self,
        root: Visibility,
        path: &RelPath,
    ) -> impl Future<Output = Result<Knowable<GitPlace>, Self::Error>> + Send;
}

/// The ports of the corpus and the ledgers, bundled.
///
/// A separate bundle from [`Ports`] so that adding it changes nothing that stands on
/// [`Ports`]; a workspace for the new use cases is generic over both
/// (`P: Ports + CorpusPorts`) and takes the clock, the identifiers and the secret scan
/// from [`Ports`].
pub trait CorpusPorts: Send + Sync {
    /// The document files.
    type Documents: DocumentSource;
    /// The ledgers.
    type Ledger: LedgerStore;
    /// The stored texts.
    type Objects: ObjectStore;
    /// Who is acting.
    type Actor: ActorSource;
    /// The pin keys.
    type Keys: KeySource;
    /// The confirmation of what cannot be undone.
    type Confirm: ConfirmPort;
    /// Writing document files.
    type Files: FileWriter;
    /// Facts from version control.
    type Facts: RevisionFactsPort;

    /// The document files.
    fn documents(&self) -> &Self::Documents;
    /// The ledgers.
    fn ledger(&self) -> &Self::Ledger;
    /// The stored texts.
    fn objects(&self) -> &Self::Objects;
    /// Who is acting. (Not `actor`: that is the method of the port itself, and a
    /// type that is both would answer to two.)
    fn actor_source(&self) -> &Self::Actor;
    /// The pin keys.
    fn key_source(&self) -> &Self::Keys;
    /// The confirmation.
    fn confirm(&self) -> &Self::Confirm;
    /// Writing document files.
    fn files(&self) -> &Self::Files;
    /// Facts from version control.
    fn facts(&self) -> &Self::Facts;
}
