//! The project's settings, as values.
//!
//! Reading `.whyshelf.toml` is the adapter's work (a file format); the use cases
//! see only this value. Everything here is a *rule of the project* that a use
//! case enforces: where new documents go, which official tags exist, which
//! directories are scanned for tests, how many changed files make a full rebuild
//! cheaper than a diff, who approves by default.

use crate::domain::actor::Actor;
use crate::domain::error::ValueError;
use crate::domain::event::StoreKind;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonBlank;
use crate::domain::reference::ProjectName;
use crate::domain::tag::TagVocabulary;

/// The regular expression that finds a pin tag in a test file.
///
/// Why a type: it is a rule of the project that the code scanner needs, and a
/// blank pattern would match nothing and hide every pin.
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

/// How the tests are found and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSettings {
    command: TestCommand,
    pin_pattern: PinPattern,
}

impl TestSettings {
    /// The command and the pattern of the pin tag.
    pub fn new(command: TestCommand, pin_pattern: PinPattern) -> Self {
        Self {
            command,
            pin_pattern,
        }
    }

    /// The command that runs the tests.
    pub fn command(&self) -> &TestCommand {
        &self.command
    }

    /// The pattern of the pin tag.
    pub fn pin_pattern(&self) -> &PinPattern {
        &self.pin_pattern
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
            user: None,
        }
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

    /// The default approver.
    pub fn user(&self) -> Option<&Actor> {
        self.user.as_ref()
    }
}
