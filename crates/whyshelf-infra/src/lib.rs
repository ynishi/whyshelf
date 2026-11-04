//! Adapters of whyshelf: the ports of `whyshelf_core::app` implemented with
//! real I/O (Git, the file system, SQLite, the clock).
//!
//! The outside world's formats and errors are translated here, at the edge,
//! so that nothing past this crate sees them. Markdown / YAML parsing into
//! domain types lives here too: the domain receives values, never text.
//!
//! This is a crate of its own, not a module of `whyshelf-core`, so that the
//! core cannot reach I/O: the boundary is a `[dependencies]` decision that the
//! compiler enforces.
//!
//! # The way in
//!
//! [`open`] takes a project root and returns a ready
//! [`Workspace`] over [`InfraPorts`], together with a
//! [`Shutdown`] that closes what must be closed explicitly (the cache's thread).
//! It reads `.whyshelf.toml`, clears the leftovers of interrupted test runs with
//! `git worktree prune`, and opens the cache at
//! `<project>/.whyshelf/cache/index.db`. The cache directory is not committed;
//! `whyshelf init` adds it to `.gitignore`.
//!
//! ```text
//!   open(root)                                          failure
//!     |
//!     1  canonicalize root ............................. OpenError::Root
//!     |
//!     2  config::load: <root>/.whyshelf.toml ........... OpenError::Config
//!     |    private store = <data dir>/<project>-<project_id>,
//!     |    the data dir from the environment (UserDirs)
//!     |
//!     3  git worktree prune ............................ OpenError::Git
//!     |    (clears leftovers of a killed test run;
//!     |     fails outside a Git repository)
//!     |
//!     4  build the secret scanner ...................... OpenError::Secret
//!     |
//!     5  open the cache: <root>/.whyshelf/cache/index.db  OpenError::Cache
//!     |    (starts the cache's thread; last, so that a failure above
//!     |     never leaves it running)
//!     |
//!     6  bundle the adapters as InfraPorts:
//!          GitRevision, FsStore (repo store, private store), FsCurrent,
//!          FsScaffold, GitCodeScan, GitTestRun, SqliteCache,
//!          RegexSecretScan, SystemClock, UuidV7Ids
//!        Workspace::new(ports, config)  ->  (Workspace<InfraPorts>, Shutdown)
//! ```
//!
//! Opening creates the cache directory, and neither store's `events/` nor `objects/`:
//! those appear at the first write. [`open_with`] is [`open`] with the environment given as a value,
//! which is how a test or a program places the private store itself:
//!
//! ```no_run
//! use std::path::Path;
//! use whyshelf_infra::{config::UserDirs, open_with};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let dirs = UserDirs::default().with_data_dir("/data/whyshelf");
//! let (workspace, shutdown) = open_with(Path::new("."), &dirs).await?;
//! // ... use the workspace ...
//! # let _ = &workspace;
//! shutdown.close().await?;
//! # Ok(())
//! # }
//! # fn main() {}
//! ```
//!
//! # The adapters
//!
//! | module | port or job |
//! |---|---|
//! | [`config`] | `.whyshelf.toml` into the application's `Config` |
//! | [`markdown`], [`parse`] | the one text normalization rule; front matter and sections into raw values |
//! | [`fs_current`], [`scaffold`] | the document files (read) and new files from a template (create, never overwrite) |
//! | [`wire`] | the version-1 JSON of an event file, and the reader that chooses by `v` |
//! | [`format`](mod@format) | the versions found in a project's data, counted and set against the ones this build reads (`whyshelf migrate`) |
//! | [`store_fs`], [`export`], [`map_dto`], [`status_md`] | the repo store and the private store; the generated files, their marker and the JSON Schema of the map |
//! | [`git`], [`test_run`] | the revision, the baseline (or that a clone cannot tell) and the pins in the test files; the test command at a revision, with a time limit |
//! | [`cache_sqlite`] | the file stamps and the full-text index, on `rusqlite-isle` |
//! | [`secret`], [`clock`], [`id`] | the secret scan, the time (RFC 3339), new UUID v7 identifiers |
//! | [`blocking`] | the only place with blocking I/O, inside `spawn_blocking` |
//!
//! Every adapter has an error type of its own that keeps its cause; none of them
//! flattens a failure into text.
#![warn(missing_docs)]

pub mod blocking;
pub mod cache_sqlite;
pub mod clock;
pub mod config;
pub mod export;
pub mod format;
pub mod fs_current;
pub mod git;
pub mod id;
pub mod map_dto;
pub mod markdown;
pub mod parse;
pub mod scaffold;
pub mod secret;
pub mod status_md;
pub mod store_fs;
pub mod test_run;
pub mod wire;

use std::path::{Path, PathBuf};

use whyshelf_core::app::{Ports, Workspace};

use cache_sqlite::{CacheDriver, CacheError, SqliteCache};
use clock::SystemClock;
use config::ConfigError;
use fs_current::FsCurrent;
use git::{GitCodeScan, GitError, GitRevision};
use id::UuidV7Ids;
use scaffold::FsScaffold;
use secret::{RegexSecretScan, SecretError};
use store_fs::FsStore;
use test_run::GitTestRun;

/// Why a workspace could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// The project root does not exist or cannot be resolved.
    #[error("cannot resolve the project root {path}")]
    Root {
        /// The path given.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// `.whyshelf.toml` is missing or invalid.
    #[error("cannot load the project settings")]
    Config(#[from] ConfigError),
    /// Git does not answer in the project root.
    #[error("cannot use Git in the project root")]
    Git(#[from] GitError),
    /// The cache could not be opened.
    #[error("cannot open the cache")]
    Cache(#[from] CacheError),
    /// The secret scanner could not be built.
    #[error("cannot build the secret scanner")]
    Secret(#[from] SecretError),
}

/// All the adapters, bundled as the application's [`Ports`].
#[derive(Debug)]
pub struct InfraPorts {
    revision: GitRevision,
    store: FsStore,
    current: FsCurrent,
    scaffold: FsScaffold,
    code_scan: GitCodeScan,
    test_run: GitTestRun,
    cache: SqliteCache,
    secret: RegexSecretScan,
    clock: SystemClock,
    id: UuidV7Ids,
}

impl Ports for InfraPorts {
    type Revision = GitRevision;
    type Store = FsStore;
    type Current = FsCurrent;
    type Scaffold = FsScaffold;
    type CodeScan = GitCodeScan;
    type TestRun = GitTestRun;
    type Cache = SqliteCache;
    type Secret = RegexSecretScan;
    type Clock = SystemClock;
    type Id = UuidV7Ids;

    fn revision(&self) -> &GitRevision {
        &self.revision
    }
    fn store(&self) -> &FsStore {
        &self.store
    }
    fn current(&self) -> &FsCurrent {
        &self.current
    }
    fn scaffold(&self) -> &FsScaffold {
        &self.scaffold
    }
    fn code_scan(&self) -> &GitCodeScan {
        &self.code_scan
    }
    fn test_run(&self) -> &GitTestRun {
        &self.test_run
    }
    fn cache(&self) -> &SqliteCache {
        &self.cache
    }
    fn secret(&self) -> &RegexSecretScan {
        &self.secret
    }
    fn clock(&self) -> &SystemClock {
        &self.clock
    }
    fn id(&self) -> &UuidV7Ids {
        &self.id
    }
}

/// What must be closed on purpose: the thread that owns the cache's connection.
///
/// Nothing is closed in `Drop`, because closing does I/O; call [`Shutdown::close`]
/// before the process ends.
#[derive(Debug)]
#[must_use = "call .close().await for a clean shutdown"]
pub struct Shutdown {
    cache: CacheDriver,
}

impl Shutdown {
    /// Let the cache's queued jobs finish and stop its thread.
    pub async fn close(self) -> Result<(), CacheError> {
        self.cache.shutdown().await
    }
}

/// Where the repo store is, relative to the project root.
pub const REPO_STORE_DIR: &str = ".whyshelf";

/// Open the workspace of the project at `root`, with the private store in the
/// data dir this process's environment gives ([`config::UserDirs::from_env`]).
pub async fn open(root: &Path) -> Result<(Workspace<InfraPorts>, Shutdown), OpenError> {
    open_with(root, &config::UserDirs::from_env()).await
}

/// Open the workspace of the project at `root`, with the private store in the
/// data dir that `dirs` gives.
pub async fn open_with(
    root: &Path,
    dirs: &config::UserDirs,
) -> Result<(Workspace<InfraPorts>, Shutdown), OpenError> {
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|source| OpenError::Root {
            path: root.to_path_buf(),
            source,
        })?;
    let settings = config::load(&root, dirs).await?;

    let revision = GitRevision::new(root.clone(), settings.baseline().to_owned());
    revision.prune_worktrees().await?;

    // Everything that can fail without side effects comes before the cache is
    // opened, so that a failure never leaves its thread running.
    let secret = RegexSecretScan::new()?;
    let repo_store = root.join(REPO_STORE_DIR);
    let (cache, driver) = SqliteCache::open(&repo_store.join("cache").join("index.db")).await?;

    let ports = InfraPorts {
        revision,
        store: FsStore::new(repo_store, settings.private_dir().to_path_buf()),
        current: FsCurrent::new(root.clone()),
        scaffold: FsScaffold::new(root.clone()),
        code_scan: GitCodeScan::new(root.clone()),
        test_run: GitTestRun::new(root),
        cache,
        secret,
        clock: SystemClock,
        id: UuidV7Ids,
    };
    Ok((
        Workspace::new(ports, settings.into_config()),
        Shutdown { cache: driver },
    ))
}
