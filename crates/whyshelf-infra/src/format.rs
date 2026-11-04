//! The versions of the data of a project, counted, and set against what this build
//! reads: the check behind `whyshelf migrate`.
//!
//! A version is a fact about a file format, and the domain knows no file format, so
//! the types and the judgement are here. The versions this build reads are the
//! constants of the modules that read them ([`crate::config::SUPPORTED_VERSION`],
//! [`crate::wire::VERSION`], [`MAP_VERSION`], [`crate::cache_sqlite::SCHEMA_VERSION`]);
//! nothing is repeated.
//!
//! # What is counted
//!
//! The specification keeps the **sources of truth** apart from what is derived from
//! them, and so does the census.
//!
//! | kind | where | counts toward the verdict |
//! |---|---|---|
//! | [`Source::Settings`] | `version` of `.whyshelf.toml` | yes |
//! | [`Source::Event`] (repo) | `v` of each file in `.whyshelf/events/` | yes |
//! | [`Source::Event`] (private) | `v` of each file in the private store's `events/` | yes |
//! | [`Projection::Map`] | `version` of each `map.json` | no: shown; `whyshelf rebuild` writes it again |
//! | [`Projection::Cache`] | `schema_version` of the cache database | no: shown; `whyshelf rebuild` writes it again |
//!
//! The generated Markdown files and the export marker carry no version (a marker is
//! the hash of the content), so there is nothing to count in them.
//!
//! # Looking without touching
//!
//! A check that the data cannot be read must work when the data cannot be read, so the
//! census opens no workspace. The settings file is read by a small shape of its own
//! (`version`, `project`, `project_id`), whatever else it holds or lacks. The events
//! are counted from the directory, reading only the `v` of each. The cache is opened
//! read-only and immutable, because opening it the normal way migrates it (and drops
//! it, if it is of another version): a check that looks does not write.
//!
//! The private store is placed as the workspace places it ([`crate::config::private_dir`]),
//! from the project's name and id. When that cannot be decided, the store is not
//! looked at and the census says so ([`Unplaced`]): a store that was not read is not
//! "readable".
//!
//! ```text
//!   .whyshelf.toml --- version -----------------------------> Source::Settings
//!        |  project, project_id (only if the version is read)
//!        v
//!   private store ? --- decided --> events/ ---------------> Source::Event(Private)
//!        |              not decided -> Unplaced { reason }
//!   .whyshelf/events/ ------------------------------------> Source::Event(Repo)
//!   map.json (both stores), cache/index.db ----------------> Projection (shown only)
//! ```
//!
//! # The verdict
//!
//! [`Census::verdict`] is the one place that judges.
//!
//! | situation | verdict |
//! |---|---|
//! | every source file has a version this build reads, and every store was looked at | [`Verdict::Readable`] |
//! | some source file has a version it does not read (or none) | [`Verdict::Unreadable`], with each group and how many files |
//! | a store could not be placed, so it was not looked at | [`Verdict::Incomplete`], with why, and the unreadable groups found anyway |
//!
//! A store whose directory does not exist (a clone that has no private store yet) was
//! looked at and holds nothing: that is zero files, not a store left unread.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use whyshelf_core::app::projection::MAP_VERSION;
use whyshelf_core::domain::event::StoreKind;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::reference::{ProjectId, ProjectName};

use crate::blocking::{self, BlockingError};
use crate::cache_sqlite::SCHEMA_VERSION;
use crate::config::{self, FILE_NAME, SUPPORTED_VERSION, UserDirs};
use crate::{REPO_STORE_DIR, wire};

/// Something whose version decides whether the data can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// The settings file.
    Settings,
    /// The event files of a store.
    Event(StoreKind),
}

impl Source {
    /// Every source, in the order a table shows them.
    pub const ALL: [Source; 3] = [
        Source::Settings,
        Source::Event(StoreKind::Repo),
        Source::Event(StoreKind::Private),
    ];

    /// The name a person reads: `settings`, `events (repo)`, `events (private)`.
    pub fn name(self) -> String {
        match self {
            Self::Settings => "settings".to_owned(),
            Self::Event(store) => format!("events ({})", store.as_str()),
        }
    }

    /// The versions this build reads.
    pub fn reads(self) -> Vec<u32> {
        match self {
            Self::Settings => vec![SUPPORTED_VERSION],
            Self::Event(_) => vec![wire::VERSION],
        }
    }
}

/// Something derived from the sources, whose version is shown and judges nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Projection {
    /// A `map.json`.
    Map,
    /// The cache database.
    Cache,
}

impl Projection {
    /// Every projection, in the order a table shows them.
    pub const ALL: [Projection; 2] = [Projection::Map, Projection::Cache];

    /// The name a person reads: `map`, `cache`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Cache => "cache",
        }
    }

    /// The versions this build reads (and writes).
    pub fn reads(self) -> Vec<u32> {
        match self {
            Self::Map => vec![MAP_VERSION],
            Self::Cache => SCHEMA_VERSION.parse().into_iter().collect(),
        }
    }
}

/// What a file says its version is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Found {
    /// A version number.
    Version(u32),
    /// No readable version: the file is not of the shape that holds one, or is not
    /// JSON (or TOML, or a database) at all.
    Unknown,
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version(v) => write!(f, "v{v}"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// Why a source could not be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnplacedReason {
    /// The settings file cannot be read as a settings file of a version this build
    /// reads, so what it says of the project cannot be relied on.
    SettingsUnreadable,
    /// The settings file has no `project_id` (or none that is a UUID).
    NoProjectId,
    /// The environment gives no usable data directory.
    DataDirUnusable,
}

impl UnplacedReason {
    /// The words a table uses.
    pub fn describe(self) -> &'static str {
        match self {
            Self::SettingsUnreadable => "the settings file cannot be read",
            Self::NoProjectId => "the settings have no project_id",
            Self::DataDirUnusable => "no usable data directory",
        }
    }
}

/// A source that was not looked at because its place cannot be decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unplaced {
    /// What was not looked at.
    pub source: Source,
    /// Why.
    pub reason: UnplacedReason,
}

/// Files of a source that this build cannot read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadableGroup {
    /// Where they are.
    pub source: Source,
    /// The version they are.
    pub found: Found,
    /// How many.
    pub count: NonZeroUsize,
}

/// What the census comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Everything that was counted can be read, and every store was looked at.
    Readable,
    /// Some source files are of a version this build cannot read.
    Unreadable(NonEmptyVec<UnreadableGroup>),
    /// Some source was not read at all, so nothing can be said about it.
    Incomplete {
        /// What was not looked at.
        unplaced: NonEmptyVec<Unplaced>,
        /// What was looked at and cannot be read.
        unreadable: Vec<UnreadableGroup>,
    },
}

/// The versions found, by what holds them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    sources: BTreeMap<Source, BTreeMap<Found, usize>>,
    projections: BTreeMap<Projection, BTreeMap<Found, usize>>,
    unplaced: Vec<Unplaced>,
}

impl Census {
    /// The sources that were looked at, with the versions found and how many files
    /// of each. A source with no file is present with nothing counted.
    pub fn sources(&self) -> &BTreeMap<Source, BTreeMap<Found, usize>> {
        &self.sources
    }

    /// The projections found. One that has no file is absent.
    pub fn projections(&self) -> &BTreeMap<Projection, BTreeMap<Found, usize>> {
        &self.projections
    }

    /// The sources that were not looked at, and why.
    pub fn unplaced(&self) -> &[Unplaced] {
        &self.unplaced
    }

    /// The one judgement: read the versions against the ones this build reads.
    pub fn verdict(&self) -> Verdict {
        let mut unreadable = Vec::new();
        for (source, counted) in &self.sources {
            let reads = source.reads();
            for (found, count) in counted {
                let readable = matches!(found, Found::Version(v) if reads.contains(v));
                if let (false, Some(count)) = (readable, NonZeroUsize::new(*count)) {
                    unreadable.push(UnreadableGroup {
                        source: *source,
                        found: *found,
                        count,
                    });
                }
            }
        }
        match NonEmptyVec::from_vec(self.unplaced.clone()) {
            Ok(unplaced) => Verdict::Incomplete {
                unplaced,
                unreadable,
            },
            Err(_) => match NonEmptyVec::from_vec(unreadable) {
                Ok(groups) => Verdict::Unreadable(groups),
                Err(_) => Verdict::Readable,
            },
        }
    }

    fn count(&mut self, source: Source, found: Found) {
        *self
            .sources
            .entry(source)
            .or_default()
            .entry(found)
            .or_default() += 1;
    }

    fn count_projection(&mut self, projection: Projection, found: Found) {
        *self
            .projections
            .entry(projection)
            .or_default()
            .entry(found)
            .or_default() += 1;
    }

    /// A source that was looked at and has no file.
    fn looked_at(&mut self, source: Source) {
        self.sources.entry(source).or_default();
    }
}

/// Why a census could not be made.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The directory has no settings file, so it is not a project: reading nothing
    /// and calling it readable would be a false answer.
    #[error("{root} is not a whyshelf project: it has no {FILE_NAME}")]
    NotAProject {
        /// The directory.
        root: PathBuf,
    },
    /// A file or directory could not be read.
    #[error("cannot read {path}")]
    Io {
        /// The path.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> FormatError {
    let path = path.to_path_buf();
    move |source| FormatError::Io { path, source }
}

/// What the census reads of a settings file, whatever else it holds.
#[derive(Deserialize)]
struct SettingsHead {
    version: Option<toml::Value>,
    project: Option<String>,
    project_id: Option<String>,
}

/// Where the private store is, or why that is not known.
enum Placed {
    At(PathBuf),
    Unplaced(UnplacedReason),
}

fn version_found(number: u64) -> Found {
    u32::try_from(number).map_or(Found::Unknown, Found::Version)
}

/// Read the settings file's version and, if it is one this build reads, decide where
/// the private store is.
fn settings(text: &str, census: &mut Census, dirs: &UserDirs) -> Placed {
    let Ok(head) = toml::from_str::<SettingsHead>(text) else {
        census.count(Source::Settings, Found::Unknown);
        return Placed::Unplaced(UnplacedReason::SettingsUnreadable);
    };
    let found = head
        .version
        .as_ref()
        .and_then(toml::Value::as_integer)
        .and_then(|number| u64::try_from(number).ok())
        .map_or(Found::Unknown, version_found);
    census.count(Source::Settings, found);
    // What a settings file of another version says of the project is not to be relied
    // on: the key may have moved.
    if !matches!(found, Found::Version(v) if Source::Settings.reads().contains(&v)) {
        return Placed::Unplaced(UnplacedReason::SettingsUnreadable);
    }
    let Some(project) = head.project.and_then(|p| p.parse::<ProjectName>().ok()) else {
        return Placed::Unplaced(UnplacedReason::SettingsUnreadable);
    };
    let id = match head.project_id.as_deref().map(str::parse::<ProjectId>) {
        None => return Placed::Unplaced(UnplacedReason::NoProjectId),
        Some(Err(_)) => return Placed::Unplaced(UnplacedReason::NoProjectId),
        Some(Ok(id)) => id,
    };
    match config::private_dir(dirs, &project, id) {
        Ok(dir) => Placed::At(dir),
        Err(_) => Placed::Unplaced(UnplacedReason::DataDirUnusable),
    }
}

/// Count the event files of a store by the `v` each says.
async fn events(store_dir: &Path, source: Source, census: &mut Census) -> Result<(), FormatError> {
    census.looked_at(source);
    let dir = store_dir.join("events");
    let mut listing = match tokio::fs::read_dir(&dir).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(&dir)(error)),
    };
    while let Some(entry) = listing.next_entry().await.map_err(io(&dir))? {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let bytes = tokio::fs::read(&path).await.map_err(io(&path))?;
        census.count(
            source,
            wire::version_of(&bytes).map_or(Found::Unknown, version_found),
        );
    }
    Ok(())
}

/// Count the `version` of the `map.json` of a store, if there is one.
async fn map(store_dir: &Path, census: &mut Census) -> Result<(), FormatError> {
    #[derive(Deserialize)]
    struct Head {
        version: Option<u64>,
    }
    let path = store_dir.join("map.json");
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(&path)(error)),
    };
    let found = serde_json::from_slice::<Head>(&bytes)
        .ok()
        .and_then(|head| head.version)
        .map_or(Found::Unknown, version_found);
    census.count_projection(Projection::Map, found);
    Ok(())
}

/// Count the versions of the data of the project at `root`, looking at the private
/// store where `dirs` and the settings place it.
///
/// Nothing is written, and the workspace is not opened.
pub async fn census(root: &Path, dirs: &UserDirs) -> Result<Census, FormatError> {
    let settings_path = root.join(FILE_NAME);
    let text = match tokio::fs::read(&settings_path).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(FormatError::NotAProject {
                root: root.to_path_buf(),
            });
        }
        Err(error) => return Err(io(&settings_path)(error)),
    };
    let mut census = Census::default();
    let placed = settings(&text, &mut census, dirs);

    let repo_store = root.join(REPO_STORE_DIR);
    events(&repo_store, Source::Event(StoreKind::Repo), &mut census).await?;
    map(&repo_store, &mut census).await?;

    let private = Source::Event(StoreKind::Private);
    match placed {
        Placed::At(dir) => {
            events(&dir, private, &mut census).await?;
            map(&dir, &mut census).await?;
        }
        Placed::Unplaced(reason) => census.unplaced.push(Unplaced {
            source: private,
            reason,
        }),
    }

    let cache = repo_store.join("cache").join("index.db");
    let found = match blocking::cache_schema_version(cache).await {
        Ok(None) => None,
        Ok(Some(version)) => Some(version.parse().map_or(Found::Unknown, Found::Version)),
        // A cache that cannot be opened is a cache of no version we can read; it is
        // derived data, so it judges nothing.
        Err(BlockingError::Sqlite { .. } | BlockingError::Io { .. }) => Some(Found::Unknown),
        Err(other @ (BlockingError::Join(_) | BlockingError::Walk(_))) => {
            return Err(FormatError::Io {
                path: root.join(REPO_STORE_DIR).join("cache"),
                source: std::io::Error::other(other),
            });
        }
    };
    if let Some(found) = found {
        census.count_projection(Projection::Cache, found);
    }
    Ok(census)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn census_of(sources: &[(Source, &[(Found, usize)])]) -> Census {
        let mut census = Census::default();
        for (source, counted) in sources {
            census.looked_at(*source);
            for (found, count) in *counted {
                for _ in 0..*count {
                    census.count(*source, *found);
                }
            }
        }
        census
    }

    const REPO: Source = Source::Event(StoreKind::Repo);
    const PRIVATE: Source = Source::Event(StoreKind::Private);

    fn v(n: u32) -> Found {
        Found::Version(n)
    }

    #[test]
    fn names_are_the_words_of_a_table_and_every_source_is_listed_once() {
        let names: Vec<String> = Source::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names, ["settings", "events (repo)", "events (private)"]);
        let projections: Vec<&str> = Projection::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(projections, ["map", "cache"]);
        assert_eq!(v(1).to_string(), "v1");
        assert_eq!(Found::Unknown.to_string(), "unknown");
    }

    #[test]
    fn the_versions_read_are_the_constants_of_the_modules_that_read_them() {
        assert_eq!(Source::Settings.reads(), [SUPPORTED_VERSION]);
        assert_eq!(REPO.reads(), [wire::VERSION]);
        assert_eq!(Projection::Map.reads(), [MAP_VERSION]);
        assert_eq!(Projection::Cache.reads(), [1]);
    }

    #[test]
    fn the_verdict_is_readable_only_when_all_is_read_and_all_was_looked_at() {
        let census = census_of(&[
            (Source::Settings, &[(v(1), 1)]),
            (REPO, &[(v(1), 42)]),
            (PRIVATE, &[]),
        ]);
        assert_eq!(census.verdict(), Verdict::Readable);
    }

    #[test]
    fn a_version_this_build_does_not_read_is_unreadable_with_its_count() {
        let census = census_of(&[
            (Source::Settings, &[(v(1), 1)]),
            (REPO, &[(v(1), 40), (v(2), 2), (Found::Unknown, 1)]),
        ]);
        let Verdict::Unreadable(groups) = census.verdict() else {
            panic!("unreadable");
        };
        let groups: Vec<(Source, Found, usize)> = groups
            .iter()
            .map(|g| (g.source, g.found, g.count.get()))
            .collect();
        assert_eq!(groups, [(REPO, v(2), 2), (REPO, Found::Unknown, 1)]);
    }

    #[test]
    fn a_store_that_was_not_looked_at_makes_the_verdict_incomplete_and_keeps_what_was_found() {
        let mut census = census_of(&[(Source::Settings, &[(v(2), 1)]), (REPO, &[(v(1), 3)])]);
        census.unplaced.push(Unplaced {
            source: PRIVATE,
            reason: UnplacedReason::SettingsUnreadable,
        });
        let Verdict::Incomplete {
            unplaced,
            unreadable,
        } = census.verdict()
        else {
            panic!("incomplete");
        };
        assert_eq!(unplaced.first().reason, UnplacedReason::SettingsUnreadable);
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].source, Source::Settings);
    }

    #[test]
    fn projections_are_shown_and_judge_nothing() {
        let mut census = census_of(&[(Source::Settings, &[(v(1), 1)]), (REPO, &[(v(1), 1)])]);
        census.count_projection(Projection::Map, v(0));
        census.count_projection(Projection::Cache, Found::Unknown);
        assert_eq!(census.projections().len(), 2);
        assert_eq!(census.verdict(), Verdict::Readable);
    }
}
