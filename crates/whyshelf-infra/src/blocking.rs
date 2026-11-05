//! The one place where blocking I/O is allowed.
//!
//! The rest of this crate runs on an async runtime and uses `tokio::fs` and
//! `tokio::process`. Two jobs have no async form: walking a directory tree
//! (`walkdir`) and creating a file that must not exist yet with the standard
//! library's atomic `create_new` followed by a write. They run inside
//! `tokio::task::spawn_blocking`, so a slow disk never stalls the executor, and
//! their code is kept here so that the architecture rule `no-sync-io` can name this
//! file, and only this file, as an exception.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use walkdir::{DirEntry, WalkDir};
use whyshelf_core::app::ports::{FileStat, Mtime};
use whyshelf_core::domain::location::RelPath;

/// Why a blocking job failed.
#[derive(Debug, thiserror::Error)]
pub enum BlockingError {
    /// The task did not run to the end (it panicked, or the runtime is closing).
    #[error("a blocking task did not finish")]
    Join(#[source] tokio::task::JoinError),
    /// A directory tree could not be walked.
    #[error("cannot walk a directory tree")]
    Walk(#[source] walkdir::Error),
    /// A database file could not be opened or read as a cache.
    #[error("cannot read the database {path}")]
    Sqlite {
        /// The file.
        path: PathBuf,
        /// SQLite's error.
        #[source]
        source: rusqlite::Error,
    },
    /// A file could not be examined, created or written.
    #[error("cannot use the file {path}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
}

/// The `schema_version` of a cache database, read without changing anything.
///
/// The cache is a WAL database, and a read-only connection to one may still try to
/// create its `-shm` file; the `immutable=1` URI tells SQLite that nothing changes
/// the file while it is read, so it neither writes nor locks. `None` when the file is
/// not there; an error when it is there and cannot be read as a cache. Opening the
/// cache the normal way would migrate it (and drop it, if it is of another version),
/// which a check that only looks must not do.
pub async fn cache_schema_version(path: PathBuf) -> Result<Option<String>, BlockingError> {
    run(move || {
        match std::fs::metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(BlockingError::Io { path, source }),
        }
        let sqlite = |source: rusqlite::Error| BlockingError::Sqlite {
            path: path.clone(),
            source,
        };
        // A path is part of a URI here: the characters that end or split one are
        // written as escapes.
        let escaped = path
            .to_string_lossy()
            .replace('%', "%25")
            .replace('?', "%3F")
            .replace('#', "%23");
        let uri = format!("file:{escaped}?mode=ro&immutable=1");
        let conn = rusqlite::Connection::open_with_flags(
            uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(sqlite)?;
        let found = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(sqlite);
        match found {
            Ok(version) => Ok(Some(version)),
            // A database with no such row is a database of no version, which is an
            // answer: the caller counts it as one it cannot read.
            Err(BlockingError::Sqlite {
                source: rusqlite::Error::QueryReturnedNoRows,
                ..
            }) => Ok(Some(String::new())),
            Err(other) => Err(other),
        }
    })
    .await
}

/// Run a blocking job on the runtime's pool for blocking work.
async fn run<T, F>(job: F) -> Result<T, BlockingError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, BlockingError> + Send + 'static,
{
    tokio::task::spawn_blocking(job)
        .await
        .map_err(BlockingError::Join)?
}

/// The modification time as nanoseconds since the Unix epoch, which is the
/// stamp the cache compares for equality.
fn stamp(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| {
            u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
        })
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.')
}

fn relative(root: &Path, full: &Path) -> Option<RelPath> {
    let rel = full.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    parts?.join("/").parse().ok()
}

fn list_sync(root: &Path, roots: &[RelPath]) -> Result<Vec<FileStat>, BlockingError> {
    let mut found: BTreeMap<RelPath, FileStat> = BTreeMap::new();
    for rel in roots {
        let start = root.join(rel.as_str());
        match std::fs::symlink_metadata(&start) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(BlockingError::Io {
                    path: start,
                    source,
                });
            }
        }
        let walk = WalkDir::new(&start)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| !is_hidden(entry));
        for entry in walk {
            let entry = entry.map_err(BlockingError::Walk)?;
            if !entry.file_type().is_file() {
                continue;
            }
            // A file named on its own is taken whatever it is called; a directory is
            // searched for Markdown only.
            let is_markdown = entry.path().extension().is_some_and(|ext| ext == "md");
            if entry.depth() > 0 && !is_markdown {
                continue;
            }
            let meta = entry.metadata().map_err(BlockingError::Walk)?;
            if let Some(path) = relative(root, entry.path()) {
                let stat = FileStat::new(path.clone(), Mtime::new(stamp(&meta)), meta.len());
                found.insert(path, stat);
            }
        }
    }
    Ok(found.into_values().collect())
}

/// The Markdown files under these paths (a path may be a directory or one file),
/// by path, each with its modification stamp and size. A path that does not
/// exist gives nothing, and hidden directories (`.git`, `.whyshelf`) are skipped.
pub async fn list_markdown(
    root: PathBuf,
    roots: Vec<RelPath>,
) -> Result<Vec<FileStat>, BlockingError> {
    run(move || list_sync(&root, &roots)).await
}

/// Create a file that does not exist yet, with its directories, and write
/// `bytes` into it. `false` when the file exists: it is left as it is.
///
/// `create_new` makes the creation atomic, so two writers cannot both succeed
/// and a file is never overwritten.
pub async fn create_new_file(path: PathBuf, bytes: Vec<u8>) -> Result<bool, BlockingError> {
    run(move || {
        let io_error = |source| BlockingError::Io {
            path: path.clone(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            Err(error) => return Err(io_error(error)),
        };
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.flush()) {
            // Do not leave half a file that the next attempt would refuse to replace.
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(io_error(error));
        }
        Ok(true)
    })
    .await
}

/// Remove a file only if its content is exactly `bytes`: the undo of
/// [`create_new_file`] that cannot take away what somebody wrote since.
///
/// Returns whether the file was removed (a file that is gone or has other content
/// is left alone).
pub async fn remove_file_if_content(path: PathBuf, bytes: Vec<u8>) -> Result<bool, BlockingError> {
    run(move || {
        let io_error = |source| BlockingError::Io {
            path: path.clone(),
            source,
        };
        match std::fs::read(&path) {
            Ok(found) if found == bytes => {
                std::fs::remove_file(&path).map_err(io_error)?;
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error(error)),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    #[tokio::test]
    async fn markdown_files_are_listed_by_path_and_hidden_directories_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [
            ("docs/b.md", "bb"),
            ("docs/a.md", "a"),
            ("docs/note.txt", "x"),
            ("docs/.hidden/c.md", "c"),
            ("docs/deep/d.md", "dddd"),
        ] {
            let path = dir.path().join(name);
            tokio::fs::create_dir_all(path.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(path, text).await.unwrap();
        }
        let listed = list_markdown(dir.path().to_path_buf(), vec![rel("docs"), rel("missing")])
            .await
            .unwrap();
        let names: Vec<(&str, u64)> = listed
            .iter()
            .map(|s| (s.path().as_str(), s.size()))
            .collect();
        assert_eq!(
            names,
            [("docs/a.md", 1), ("docs/b.md", 2), ("docs/deep/d.md", 4)]
        );
    }

    #[tokio::test]
    async fn a_file_named_on_its_own_is_listed_even_if_it_is_not_markdown() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("notes.txt"), "x")
            .await
            .unwrap();
        let listed = list_markdown(dir.path().to_path_buf(), vec![rel("notes.txt")])
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn create_new_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/new.md");
        assert!(
            create_new_file(path.clone(), b"first".to_vec())
                .await
                .unwrap()
        );
        assert!(
            !create_new_file(path.clone(), b"second".to_vec())
                .await
                .unwrap()
        );
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "first");
    }
}
