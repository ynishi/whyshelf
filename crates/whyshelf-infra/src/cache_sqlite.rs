//! The cache: [`IndexCachePort`] over SQLite, through `rusqlite-isle`.
//!
//! The cache holds, for each document file, the stamp it had when it was last read
//! (so that ingest can see what changed without reading anything), the `raw` hash
//! of its text and the text to search. It is never a source of truth: delete the
//! file and the next ingest builds it again. It lives in the working tree
//! (`<worktree>/.whyshelf/cache/index.db`), one per worktree, and is not
//! committed or merged.
//!
//! | table | holds |
//! |---|---|
//! | `meta` | `schema_version`; a cache of another version is dropped and made again |
//! | `files` | `path`, `mtime`, `size`, `raw` of each document |
//! | `files_fts` | an FTS5 index of the text, one row per file, with the same `rowid` as `files` |
//!
//! # A cache per worktree, never merged
//!
//! Each worktree has its own `index.db`, because its rows describe the files that
//! worktree holds. The directory is in `.gitignore` (`whyshelf init` puts it there),
//! so a merge never brings one in, and two worktrees of a project can disagree about
//! the cache without any harm: each mirrors its own checkout. What a merge, a rebase
//! or a branch switch changes in the documents shows up as files whose stamps differ
//! from the rows; the next command's ingest reads those files, and if there are more
//! than `ingest.rebuild_threshold` of them it replaces the whole cache instead of
//! patching it. `whyshelf rebuild` does that on request. The cache is read for two
//! things only: the stamps (what changed) and the full-text search of `find`; the
//! state of an entity is never taken from it.
//!
//! # One thread owns the connection
//!
//! `rusqlite-isle` keeps the SQLite connection on a thread of its own and gives the
//! async code a cloneable handle ([`AsyncIsle`]); a call waits on a channel and
//! never blocks the executor. The thread's life belongs to a separate owner
//! ([`CacheDriver`]) that is closed explicitly and asynchronously with
//! [`CacheDriver::shutdown`], which lets the queued jobs finish before the thread
//! is joined. Nothing is done in `Drop`: dropping the handles and the driver without
//! a shutdown only lets the thread end by itself.
//!
//! The database is in WAL mode with a busy timeout, so that a command line and a
//! tool server working in the same worktree wait for each other instead of failing,
//! and every write runs in a transaction that takes the write lock first.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OptionalExtension, Transaction, params};
use rusqlite_isle::{AsyncIsle, AsyncIsleDriver, IsleError};
use whyshelf_core::app::ports::{CacheEntry, FileStat, IndexCachePort, Mtime};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;

/// The shape of the tables this build writes.
pub const SCHEMA_VERSION: &str = "1";

/// How long a statement waits for another connection's lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the cache failed.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// SQLite, or the thread that owns the connection, failed.
    #[error("the cache database failed")]
    Database(#[from] IsleError),
    /// The directory of the database could not be made, or an old one removed.
    #[error("cannot prepare the cache file {path}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// A number does not fit what SQLite stores.
    #[error("a number is too large for the cache")]
    Range,
    /// A row holds a value that the rule of its type refuses (a damaged cache).
    #[error("the cache holds a value the domain refuses")]
    Row(#[source] ValueError),
}

fn is_damaged(error: &IsleError) -> bool {
    matches!(
        error,
        IsleError::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
            if matches!(failure.code, ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

/// Make the tables, or drop and make them again when they are of another version.
fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )?;
    let found: Option<String> = tx
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if found.as_deref() != Some(SCHEMA_VERSION) {
        tx.execute_batch("DROP TABLE IF EXISTS files_fts; DROP TABLE IF EXISTS files;")?;
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
             path TEXT PRIMARY KEY,
             mtime INTEGER NOT NULL,
             size INTEGER NOT NULL,
             raw TEXT NOT NULL
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(body);",
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION],
    )?;
    tx.commit()
}

/// A cache row, in the types SQLite stores.
#[derive(Debug)]
struct Row {
    path: String,
    mtime: i64,
    size: i64,
    raw: String,
    body: String,
}

impl Row {
    fn of(entry: &CacheEntry) -> Result<Self, CacheError> {
        Ok(Self {
            path: entry.path().to_string(),
            mtime: i64::try_from(entry.stat().mtime().get()).map_err(|_| CacheError::Range)?,
            size: i64::try_from(entry.stat().size()).map_err(|_| CacheError::Range)?,
            raw: entry.raw().to_string(),
            body: entry.body().to_owned(),
        })
    }

    fn into_entry(self) -> Result<CacheEntry, CacheError> {
        let path: RelPath = self.path.parse().map_err(CacheError::Row)?;
        let raw: ContentHash = self.raw.parse().map_err(CacheError::Row)?;
        let mtime = u64::try_from(self.mtime).map_err(|_| CacheError::Range)?;
        let size = u64::try_from(self.size).map_err(|_| CacheError::Range)?;
        Ok(CacheEntry::new(
            FileStat::new(path, Mtime::new(mtime), size),
            raw,
            self.body,
        ))
    }
}

fn rows_of(entries: &[CacheEntry]) -> Result<Vec<Row>, CacheError> {
    entries.iter().map(Row::of).collect()
}

fn delete_path(tx: &Transaction<'_>, path: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM files_fts WHERE rowid IN (SELECT rowid FROM files WHERE path = ?1)",
        [path],
    )?;
    tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
    Ok(())
}

fn insert(tx: &Transaction<'_>, row: &Row) -> rusqlite::Result<()> {
    delete_path(tx, &row.path)?;
    tx.execute(
        "INSERT INTO files (path, mtime, size, raw) VALUES (?1, ?2, ?3, ?4)",
        params![row.path, row.mtime, row.size, row.raw],
    )?;
    let rowid = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO files_fts (rowid, body) VALUES (?1, ?2)",
        params![rowid, row.body],
    )?;
    Ok(())
}

/// The query for FTS5: every word is a quoted phrase, so punctuation in the
/// words cannot be taken for the query language, and the words are all required.
fn fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The handle the ports use. Cloning it shares the one connection.
#[derive(Clone)]
pub struct SqliteCache {
    isle: AsyncIsle,
}

impl std::fmt::Debug for SqliteCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteCache").finish_non_exhaustive()
    }
}

/// The owner of the thread that holds the connection.
///
/// Close it with [`CacheDriver::shutdown`]; there is no `Drop` that does I/O.
#[must_use = "call .shutdown().await for a clean close"]
pub struct CacheDriver {
    driver: AsyncIsleDriver,
}

impl std::fmt::Debug for CacheDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheDriver").finish_non_exhaustive()
    }
}

impl CacheDriver {
    /// Let the queued jobs finish, stop the thread and join it.
    pub async fn shutdown(self) -> Result<(), CacheError> {
        self.driver.shutdown().await.map_err(CacheError::from)
    }
}

async fn spawn(path: &Path) -> Result<(AsyncIsle, AsyncIsleDriver), IsleError> {
    AsyncIsle::builder()
        .thread_name("whyshelf-cache")
        .wal(BUSY_TIMEOUT)
        .spawn(path, migrate)
        .await
}

async fn remove_database(path: &Path) -> Result<(), CacheError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let file = PathBuf::from(name);
        match tokio::fs::remove_file(&file).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(CacheError::Io { path: file, source }),
        }
    }
    Ok(())
}

impl SqliteCache {
    /// Open (and, if need be, make) the database file.
    ///
    /// A file that is not a database, or is damaged, is a cache like any other:
    /// it is removed and made again once.
    pub async fn open(path: &Path) -> Result<(Self, CacheDriver), CacheError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| CacheError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
        }
        let (isle, driver) = match spawn(path).await {
            Ok(opened) => opened,
            Err(error) if is_damaged(&error) => {
                remove_database(path).await?;
                spawn(path).await?
            }
            Err(error) => return Err(error.into()),
        };
        Ok((Self { isle }, CacheDriver { driver }))
    }
}

impl IndexCachePort for SqliteCache {
    type Error = CacheError;

    async fn load(&self) -> Result<Vec<CacheEntry>, CacheError> {
        let rows = self
            .isle
            .call(|conn| {
                let mut statement = conn.prepare(
                    "SELECT f.path, f.mtime, f.size, f.raw, t.body
                     FROM files f JOIN files_fts t ON t.rowid = f.rowid
                     ORDER BY f.path",
                )?;
                statement
                    .query_map([], |row| {
                        Ok(Row {
                            path: row.get(0)?,
                            mtime: row.get(1)?,
                            size: row.get(2)?,
                            raw: row.get(3)?,
                            body: row.get(4)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<Row>>>()
            })
            .await?;
        rows.into_iter().map(Row::into_entry).collect()
    }

    async fn upsert(&self, entries: &[CacheEntry]) -> Result<(), CacheError> {
        let rows = rows_of(entries)?;
        self.isle
            .call_write(move |tx| rows.iter().try_for_each(|row| insert(tx, row)))
            .await
            .map_err(CacheError::from)
    }

    async fn remove(&self, paths: &[RelPath]) -> Result<(), CacheError> {
        let paths: Vec<String> = paths.iter().map(ToString::to_string).collect();
        self.isle
            .call_write(move |tx| paths.iter().try_for_each(|path| delete_path(tx, path)))
            .await
            .map_err(CacheError::from)
    }

    async fn rebuild(&self, entries: &[CacheEntry]) -> Result<(), CacheError> {
        let rows = rows_of(entries)?;
        self.isle
            .call_write(move |tx| {
                tx.execute_batch("DELETE FROM files_fts; DELETE FROM files;")?;
                rows.iter().try_for_each(|row| insert(tx, row))
            })
            .await
            .map_err(CacheError::from)
    }

    async fn search_fts(&self, text: &NonBlank, limit: usize) -> Result<Vec<RelPath>, CacheError> {
        let query = fts_query(text.as_str());
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let paths = self
            .isle
            .call(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT f.path FROM files_fts
                     JOIN files f ON f.rowid = files_fts.rowid
                     WHERE files_fts MATCH ?1
                     ORDER BY rank
                     LIMIT ?2",
                )?;
                statement
                    .query_map(params![query, limit], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            })
            .await?;
        paths
            .into_iter()
            .map(|path| path.parse().map_err(CacheError::Row))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, mtime: u64, size: u64, body: &str) -> CacheEntry {
        CacheEntry::new(
            FileStat::new(path.parse().unwrap(), Mtime::new(mtime), size),
            ContentHash::raw(body.as_bytes()),
            body.to_owned(),
        )
    }

    fn words(text: &str) -> NonBlank {
        NonBlank::new(text).unwrap()
    }

    async fn open_in(dir: &Path) -> (SqliteCache, CacheDriver) {
        SqliteCache::open(&dir.join("cache").join("index.db"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn entries_are_stored_replaced_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let (cache, driver) = open_in(dir.path()).await;
        assert!(cache.load().await.unwrap().is_empty());

        let a = entry("docs/a.md", 10, 5, "login is slow");
        let b = entry("docs/b.md", 11, 7, "cache timeout");
        cache.upsert(&[a.clone(), b.clone()]).await.unwrap();
        assert_eq!(cache.load().await.unwrap(), vec![a.clone(), b.clone()]);

        let a2 = entry("docs/a.md", 12, 9, "login is fast now");
        cache.upsert(std::slice::from_ref(&a2)).await.unwrap();
        assert_eq!(cache.load().await.unwrap(), vec![a2.clone(), b.clone()]);

        cache.remove(&["docs/a.md".parse().unwrap()]).await.unwrap();
        assert_eq!(cache.load().await.unwrap(), vec![b]);
        driver.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn text_is_searched_by_words_and_best_first() {
        let dir = tempfile::tempdir().unwrap();
        let (cache, driver) = open_in(dir.path()).await;
        cache
            .upsert(&[
                entry("docs/a.md", 1, 1, "the login is slow on a cold start"),
                entry("docs/b.md", 1, 1, "login login login"),
                entry("docs/c.md", 1, 1, "unrelated text"),
            ])
            .await
            .unwrap();
        let found = cache.search_fts(&words("login"), 10).await.unwrap();
        let names: Vec<&str> = found.iter().map(RelPath::as_str).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"docs/a.md") && names.contains(&"docs/b.md"));

        let both = cache.search_fts(&words("login cold"), 10).await.unwrap();
        assert_eq!(both, vec!["docs/a.md".parse::<RelPath>().unwrap()]);
        assert_eq!(cache.search_fts(&words("login"), 1).await.unwrap().len(), 1);
        // Punctuation is not the query language.
        assert!(
            cache
                .search_fts(&words("\"login\" OR (NEAR"), 10)
                .await
                .unwrap()
                .is_empty()
        );
        // A removed file is no longer found.
        cache.remove(&["docs/a.md".parse().unwrap()]).await.unwrap();
        assert_eq!(cache.search_fts(&words("cold"), 10).await.unwrap(), vec![]);
        driver.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn rebuild_replaces_everything() {
        let dir = tempfile::tempdir().unwrap();
        let (cache, driver) = open_in(dir.path()).await;
        cache
            .upsert(&[entry("docs/old.md", 1, 1, "old words")])
            .await
            .unwrap();
        let fresh = entry("docs/new.md", 2, 2, "new words");
        cache.rebuild(std::slice::from_ref(&fresh)).await.unwrap();
        assert_eq!(cache.load().await.unwrap(), vec![fresh]);
        assert!(
            cache
                .search_fts(&words("old"), 10)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(cache.search_fts(&words("new"), 10).await.unwrap().len(), 1);
        driver.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn the_cache_survives_a_shutdown_and_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let (cache, driver) = open_in(dir.path()).await;
        let a = entry("docs/a.md", 3, 4, "kept");
        cache.upsert(std::slice::from_ref(&a)).await.unwrap();
        driver.shutdown().await.unwrap();
        // The handle reports the closed thread after the shutdown.
        assert!(matches!(
            cache.load().await,
            Err(CacheError::Database(IsleError::Closed))
        ));

        let (again, driver) = open_in(dir.path()).await;
        assert_eq!(again.load().await.unwrap(), vec![a]);
        driver.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_cache_of_another_version_is_dropped_and_made_again() {
        let dir = tempfile::tempdir().unwrap();
        let (cache, driver) = open_in(dir.path()).await;
        cache
            .upsert(&[entry("docs/a.md", 1, 1, "x")])
            .await
            .unwrap();
        cache
            .isle
            .call(|conn| {
                conn.execute(
                    "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
                    [],
                )
            })
            .await
            .unwrap();
        driver.shutdown().await.unwrap();

        let (again, driver) = open_in(dir.path()).await;
        assert!(again.load().await.unwrap().is_empty());
        let version: String = again
            .isle
            .call(|conn| {
                conn.query_row(
                    "SELECT value FROM meta WHERE key = 'schema_version'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        driver.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_file_that_is_not_a_database_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        tokio::fs::write(&path, vec![b'x'; 4096]).await.unwrap();
        let (cache, driver) = SqliteCache::open(&path).await.unwrap();
        assert!(cache.load().await.unwrap().is_empty());
        driver.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn two_handles_on_one_file_see_each_others_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (first, first_driver) = open_in(dir.path()).await;
        let (second, second_driver) = open_in(dir.path()).await;
        let a = entry("docs/a.md", 1, 1, "shared");
        first.upsert(std::slice::from_ref(&a)).await.unwrap();
        assert_eq!(second.load().await.unwrap(), vec![a]);
        first_driver.shutdown().await.unwrap();
        second_driver.shutdown().await.unwrap();
    }
}
