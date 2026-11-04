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
//! | [`RevisionPort`] | the current revision, whether the tree is clean, whether a revision reached the baseline, validation of a revision text |
//! | [`StorePort`] | the event log and the stored objects of the repo store and the private store, whether the private store is there, and the generated files |
//! | [`CurrentPort`] | the document files: a listing with stamps, and one file read and parsed (read only) |
//! | [`ScaffoldPort`] | creating a new document file from a template, never overwriting |
//! | [`CodeScanPort`] | the pins found in the scan directories at a given revision |
//! | [`TestRunPort`] | running the tests at a given revision |
//! | [`IndexCachePort`] | a cache of file stamps and searchable text (rebuildable, never a source of truth) |
//! | [`SecretScanPort`] | finding secrets in text before it is written |
//! | [`ClockPort`], [`IdPort`] | the time and new identifiers, which the domain never makes |

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::num::NonZeroU32;

use crate::domain::actor::Timestamp;
use crate::domain::approval::TestOutcome;
use crate::domain::error::ValueError;
use crate::domain::event::{Event, StoreKind};
use crate::domain::finding::FindingType;
use crate::domain::hash::ContentHash;
use crate::domain::id::EntityId;
use crate::domain::location::{CodeLocation, RelPath};
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::pin::PinId;
use crate::domain::reference::{Reach, Ref};
use crate::domain::revision::RevisionId;

use super::config::{PinPattern, TestCommand};
use super::projection::{ChangelogDoc, MapDoc};
use super::raw::{Candidate, NormalizedText, RawRevision};
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

    /// The revision of the working tree (the tip it stands on).
    fn current(&self) -> impl Future<Output = Result<RevisionId, Self::Error>> + Send;

    /// Whether the working tree has no uncommitted change.
    fn is_clean(&self) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    /// Whether the revision has reached the baseline.
    fn is_integrated(
        &self,
        revision: &RevisionId,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    /// A name for the current line of work (a branch, say). Display only.
    fn label(&self) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;

    /// The working directory, for the context line.
    fn worktree(&self) -> impl Future<Output = Result<WorktreeRoot, Self::Error>> + Send;
}

/// Whether an append wrote something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    /// The event or object was new.
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

    /// Append an event. Idempotent: an event with the same identifier is
    /// already there, which is success.
    fn append(
        &self,
        scope: StoreKind,
        event: &Event,
    ) -> impl Future<Output = Result<AppendOutcome, Self::Error>> + Send;

    /// Every event of the store.
    ///
    /// A store that is not there has no events: this cannot tell it from an empty
    /// one, which [`private_reach`](Self::private_reach) does.
    fn events(
        &self,
        scope: StoreKind,
    ) -> impl Future<Output = Result<Vec<Event>, Self::Error>> + Send;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mtime(u64);

impl Mtime {
    /// A stamp. The adapter decides the unit; the use cases never order it.
    pub fn new(stamp: u64) -> Self {
        Self(stamp)
    }

    /// The stamp.
    pub fn get(&self) -> u64 {
        self.0
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

    /// Whether another stat of the same path says the file is unchanged.
    pub fn same_as(&self, other: &FileStat) -> bool {
        self.mtime == other.mtime && self.size == other.size
    }
}

/// The document files. Read only: the tool never rewrites a user's file.
pub trait CurrentPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The document files under these paths (a path may be a directory or a
    /// file). A path that does not exist gives nothing.
    fn list(
        &self,
        roots: &[RelPath],
    ) -> impl Future<Output = Result<Vec<FileStat>, Self::Error>> + Send;

    /// Read and parse one file. `None` when it does not exist.
    fn read(
        &self,
        path: &RelPath,
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
}

/// Finding the pins declared in test files.
pub trait CodeScanPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// The pins in the files under `scan_dirs` at `revision`, with where
    /// each was found. Reads the revision, not the working tree. Which files in
    /// those directories are tests is not the port's business; it reports a pin
    /// wherever the pattern matches.
    fn pins_at(
        &self,
        revision: &RevisionId,
        scan_dirs: &[RelPath],
        pattern: &PinPattern,
    ) -> impl Future<Output = Result<BTreeMap<PinId, Vec<CodeLocation>>, Self::Error>> + Send;
}

/// Running the tests.
pub trait TestRunPort: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// Run the command at `revision` (in the working tree when it is the
    /// current clean one, in a temporary checkout otherwise).
    fn run(
        &self,
        revision: &RevisionId,
        command: &TestCommand,
    ) -> impl Future<Output = Result<TestOutcome, Self::Error>> + Send;
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
