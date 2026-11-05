//! The project's settings, as values.
//!
//! Reading `.whyshelf.toml` is the adapter's work (a file format); the use cases
//! see only this value. Everything here is a *rule of the project* that a use
//! case enforces: where new documents go, which official tags exist, which
//! directories are scanned for tests, how many changed files make a full rebuild
//! cheaper than a diff, who approves by default.

use std::time::Duration;

use crate::domain::actor::Actor;
use crate::domain::error::ValueError;
use crate::domain::event::StoreKind;
use crate::domain::location::{RelPath, TestFileSet};
use crate::domain::nonempty::NonBlank;
use crate::domain::policy::{GatePolicy, PinVocabulary};
use crate::domain::reference::ProjectName;
use crate::domain::tag::TagVocabulary;

/// The regular expression that delimits a pin tag in a test file, when the project
/// writes its own instead of the scanner's.
///
/// Its first group is the whole mark; whether the mark is a pin is decided by the
/// [`PinVocabulary`] it is read in, not by the expression. Why a type: it is a rule
/// of the project that the code scanner needs, and a blank pattern would match
/// nothing and hide every pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinPattern(NonBlank);

impl PinPattern {
    /// A pattern; blank is refused. Whether it compiles is the scanner's check.
    pub fn new(pattern: impl Into<String>) -> Result<Self, ValueError> {
        NonBlank::new(pattern).map(Self)
    }

    /// The pattern text.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// What the scan of the tests needs to find pins and to tell which marks are pins:
/// the project's [`PinVocabulary`], and optionally its own [`PinPattern`].
///
/// The scanner builds the regular expression from this value: the project's pattern
/// if it gave one, else the scanner's own, which delimits a mark after `@pin:`. The
/// expression only cuts a mark out of a line; the vocabulary alone decides whether
/// it is a pin (see [`PinId::parse_in`](crate::domain::pin::PinId::parse_in)), so a mark that is none is reported, never
/// dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinSearch {
    vocabulary: PinVocabulary,
    pattern: Option<PinPattern>,
}

impl PinSearch {
    /// A search in this vocabulary, with the scanner's own pattern.
    pub fn new(vocabulary: PinVocabulary) -> Self {
        Self {
            vocabulary,
            pattern: None,
        }
    }

    /// The project's own pattern for cutting a mark out of a line
    /// (`test.pin_pattern`).
    pub fn with_pattern(mut self, pattern: PinPattern) -> Self {
        self.pattern = Some(pattern);
        self
    }

    /// The vocabulary the marks are read in.
    pub fn vocabulary(&self) -> &PinVocabulary {
        &self.vocabulary
    }

    /// The project's own pattern, if it gave one.
    pub fn pattern(&self) -> Option<&PinPattern> {
        self.pattern.as_ref()
    }
}

/// The command that runs the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCommand(NonBlank);

impl TestCommand {
    /// A command line; blank is refused.
    pub fn new(command: impl Into<String>) -> Result<Self, ValueError> {
        NonBlank::new(command).map(Self)
    }

    /// The command line.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Where documents and tests are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    finding_dir: RelPath,
    spec_dir: RelPath,
    design_dir: RelPath,
    scan_dirs: Vec<RelPath>,
    watch_dirs: Vec<RelPath>,
}

impl Paths {
    /// The directories of the project. `watch_dirs` are searched for files that
    /// are not registered; `scan_dirs` for tests that carry pins.
    pub fn new(
        finding_dir: RelPath,
        spec_dir: RelPath,
        design_dir: RelPath,
        scan_dirs: Vec<RelPath>,
        watch_dirs: Vec<RelPath>,
    ) -> Self {
        Self {
            finding_dir,
            spec_dir,
            design_dir,
            scan_dirs,
            watch_dirs,
        }
    }

    /// Where new Findings are created.
    pub fn finding_dir(&self) -> &RelPath {
        &self.finding_dir
    }

    /// Where new Specs are created.
    pub fn spec_dir(&self) -> &RelPath {
        &self.spec_dir
    }

    /// Where new Designs are created.
    pub fn design_dir(&self) -> &RelPath {
        &self.design_dir
    }

    /// Where tests are scanned for pins.
    pub fn scan_dirs(&self) -> &[RelPath] {
        &self.scan_dirs
    }

    /// Where unregistered files are looked for.
    pub fn watch_dirs(&self) -> &[RelPath] {
        &self.watch_dirs
    }
}

/// How long the test command may run before it is stopped.
///
/// Why a type: a zero limit would stop every run at once, and "no limit" is not
/// offered, since a command that never ends holds the whole check for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestTimeout(Duration);

/// The limit when the settings give none: thirty minutes.
pub const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);

impl TestTimeout {
    /// A limit; zero is refused.
    pub fn new(limit: Duration) -> Result<Self, ValueError> {
        if limit.is_zero() {
            return Err(ValueError::TimeoutZero);
        }
        Ok(Self(limit))
    }

    /// The limit.
    pub fn get(&self) -> Duration {
        self.0
    }
}

impl Default for TestTimeout {
    fn default() -> Self {
        Self(DEFAULT_TEST_TIMEOUT)
    }
}

/// How the tests are found and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSettings {
    command: TestCommand,
    pin_search: PinSearch,
    files: Option<TestFileSet>,
    timeout: TestTimeout,
}

impl TestSettings {
    /// The command. Every file under the scan directories is a test file, the
    /// command may run for [`DEFAULT_TEST_TIMEOUT`], and pins are `PIN-*` and `UT-*`
    /// tags after `@pin:`, until the settings say otherwise.
    pub fn new(command: TestCommand) -> Self {
        Self {
            command,
            pin_search: PinSearch::default(),
            files: None,
            timeout: TestTimeout::default(),
        }
    }

    /// How pin tags are found and read (`test.pin_pattern`, and the vocabulary).
    pub fn with_pin_search(mut self, pin_search: PinSearch) -> Self {
        self.pin_search = pin_search;
        self
    }

    /// Only the files these globs name are test files (`test.files`).
    pub fn with_files(mut self, files: TestFileSet) -> Self {
        self.files = Some(files);
        self
    }

    /// The limit on a test run (`test.timeout`).
    pub fn with_timeout(mut self, timeout: TestTimeout) -> Self {
        self.timeout = timeout;
        self
    }

    /// The globs of `test.files`, if the settings gave them.
    pub fn files(&self) -> Option<&TestFileSet> {
        self.files.as_ref()
    }

    /// The limit on a test run.
    pub fn timeout(&self) -> TestTimeout {
        self.timeout
    }

    /// The command that runs the tests.
    pub fn command(&self) -> &TestCommand {
        &self.command
    }

    /// How pin tags are found in the tests and read as pins.
    pub fn pin_search(&self) -> &PinSearch {
        &self.pin_search
    }
}

/// Which generated files beyond the map are written.
///
/// The map is always written; the two Markdown files are off unless asked for,
/// because the status changes at almost every commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportSettings {
    status_md: bool,
    changelog_md: bool,
}

impl ExportSettings {
    /// Choose the optional files.
    pub fn new(status_md: bool, changelog_md: bool) -> Self {
        Self {
            status_md,
            changelog_md,
        }
    }

    /// Whether the status Markdown is written.
    pub fn status_md(&self) -> bool {
        self.status_md
    }

    /// Whether the changelog Markdown is written.
    pub fn changelog_md(&self) -> bool {
        self.changelog_md
    }
}

/// The number of changed files above which the cache is rebuilt instead of
/// updated by a diff (a branch switch changes many at once).
pub const DEFAULT_REBUILD_THRESHOLD: usize = 200;

/// Settings of ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestSettings {
    rebuild_threshold: usize,
}

impl IngestSettings {
    /// Settings with this threshold.
    pub fn new(rebuild_threshold: usize) -> Self {
        Self { rebuild_threshold }
    }

    /// More changed files than this rebuild the cache.
    pub fn rebuild_threshold(&self) -> usize {
        self.rebuild_threshold
    }
}

impl Default for IngestSettings {
    fn default() -> Self {
        Self::new(DEFAULT_REBUILD_THRESHOLD)
    }
}

/// The settings of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    project: ProjectName,
    paths: Paths,
    test: TestSettings,
    default_store: StoreKind,
    vocabulary: TagVocabulary,
    export: ExportSettings,
    ingest: IngestSettings,
    gate_policy: GatePolicy,
    user: Option<Actor>,
}

impl Config {
    /// A configuration with the required parts. The default store is the
    /// private one: a forgotten choice must not publish.
    pub fn new(project: ProjectName, paths: Paths, test: TestSettings) -> Self {
        Self {
            project,
            paths,
            test,
            default_store: StoreKind::Private,
            vocabulary: TagVocabulary::default(),
            export: ExportSettings::default(),
            ingest: IngestSettings::default(),
            gate_policy: GatePolicy::default(),
            user: None,
        }
    }

    /// What CS demands of a Spec; a passed CS records it.
    pub fn with_gate_policy(mut self, gate_policy: GatePolicy) -> Self {
        self.gate_policy = gate_policy;
        self
    }

    /// Where a new document goes when the caller does not say.
    pub fn with_default_store(mut self, store: StoreKind) -> Self {
        self.default_store = store;
        self
    }

    /// The official tags the project defines.
    pub fn with_vocabulary(mut self, vocabulary: TagVocabulary) -> Self {
        self.vocabulary = vocabulary;
        self
    }

    /// The optional generated files.
    pub fn with_export(mut self, export: ExportSettings) -> Self {
        self.export = export;
        self
    }

    /// The ingest settings.
    pub fn with_ingest(mut self, ingest: IngestSettings) -> Self {
        self.ingest = ingest;
        self
    }

    /// The name recorded as `by` when the caller gives none.
    pub fn with_user(mut self, user: Actor) -> Self {
        self.user = Some(user);
        self
    }

    /// The project's name.
    pub fn project(&self) -> &ProjectName {
        &self.project
    }

    /// The directories.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The test settings.
    pub fn test(&self) -> &TestSettings {
        &self.test
    }

    /// The files that count as tests: `test.files`, else the whole of the scan
    /// directories. `None` only when there is neither, which leaves no file to scan.
    pub fn test_files(&self) -> Option<TestFileSet> {
        self.test
            .files()
            .cloned()
            .or_else(|| TestFileSet::whole(self.paths.scan_dirs()))
    }

    /// Where a new document goes by default.
    pub fn default_store(&self) -> StoreKind {
        self.default_store
    }

    /// The official tags.
    pub fn vocabulary(&self) -> &TagVocabulary {
        &self.vocabulary
    }

    /// The optional generated files.
    pub fn export(&self) -> &ExportSettings {
        &self.export
    }

    /// The ingest settings.
    pub fn ingest(&self) -> &IngestSettings {
        &self.ingest
    }

    /// What CS demands of a Spec.
    pub fn gate_policy(&self) -> GatePolicy {
        self.gate_policy
    }

    /// The default approver.
    pub fn user(&self) -> Option<&Actor> {
        self.user.as_ref()
    }
}
