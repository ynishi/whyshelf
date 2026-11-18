//! A new file, written whole or not at all: the one way the adapters create a file
//! where nothing is (the declaration, a new document, another file a project
//! starts with), and the one way they put such files on disk.
//!
//! The file is opened `create_new`, so nothing that is there is ever replaced,
//! with its directories made first. If writing its bytes fails after the file was
//! made, the file this call made is removed, and only that one: a failure leaves
//! nothing half-written behind. "Only that one" is checked: on Unix the file at the
//! path is removed only when it is still the one the call opened (the same device
//! and inode); one put in its place in between is left, and the error says so.
//! Elsewhere the platform gives no such identity through the standard library, so
//! the file at the path is removed as it is. [`create_new`] hands the bytes to the
//! operating system and does not put them on disk: that is [`persist`], which the
//! caller calls once for every file a run made, so a run pays for it once and not
//! once for each file. Until then a power loss can leave a file empty or partly
//! written, as it could between the write and a sync of one file; a file that is
//! there and cannot be read as a document is taken as a name in use by the next
//! run, which moves on to another key.
//!
//! [`persist`] puts files on disk by what each platform can say it does:
//!
//! - On Unix each file is opened again, read only, and `fsync`ed, one at a time (no
//!   descriptor is held until the end, which would run into the limit of open
//!   files), and so is every directory from each file's parent up to the root, each
//!   once: the entries `create_dir_all` made are covered, which Linux needs.
//! - On Apple systems the `fsync` is the system's own (`rustix::fs::fsync`), which
//!   hands the data to the drive; the standard library's `sync_all` there is a full
//!   flush of the drive's cache and costs milliseconds a file. The full flush
//!   (`F_FULLFSYNC`) is asked once for each device the files are on, after all of
//!   them were `fsync`ed, which it covers: usually once for the run. A device that
//!   cannot give a full flush (a network share, `exFAT`: `ENOTSUP`, `EINVAL` or
//!   `ENOTTY`) is left at `fsync`, the strongest form it has.
//! - On other Unix systems `fsync` is the standard library's `sync_all`. On
//!   Linux with `ext4` or `xfs` a sync of a file flushes the drive's cache each time,
//!   so doing it for each file in a row costs about what it did before this module
//!   put files on disk together; the gain is on Apple systems.
//! - On Windows each file is opened again for writing (neither truncated nor
//!   appended to) and `sync_all`ed. A directory is not synced: its entries are
//!   not covered there.
//!
//! The limits: on Linux (before 4.16, or on any kernel when the file left memory
//! between its write and the end of the run) and on NFS, a failed write-back can be
//! missed by an `fsync` made through a descriptor opened after it, so a file can be
//! reported as on disk when it is not.
//!
//! [`in_the_way`] tells, before anything is written, what `create_new` would find
//! in its way: something at the path, or a parent that is no directory, or a
//! parent that is a link leading out of the root (followed, as the space `file`
//! reads, only while its canonical path stays inside the canonical root).

#[cfg(unix)]
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use whyshelf_core::app::shelf::Creation;
use whyshelf_core::domain::location::RelPath;

/// The step of a creation that failed, with its path, and the operating system's
/// error.
#[derive(Debug)]
pub(crate) struct CreateFailed {
    /// The step (`create the directory <path>`, `write <path>`), with what became of
    /// the file this call made.
    pub(crate) step: String,
    /// The operating system's error.
    pub(crate) source: io::Error,
}

/// What is in the way of a new file at `file` under `root`, if anything: a parent
/// inside the root that is there and is no directory (a file, a link to nothing,
/// a link to what is no directory), else anything at the path itself (a file, a
/// directory, a link, even one to nothing; the link is looked at, not what it
/// leads to). A parent that is a link to a directory inside the root (by their
/// canonical paths) is no obstacle: the file is made through it, as [`create_new`]
/// makes it. A parent that is a link leading out of the root is in the way, by its
/// path: nothing is written outside the project.
pub(crate) async fn in_the_way(root: &Path, file: &RelPath) -> io::Result<Option<RelPath>> {
    let segments: Vec<&str> = file.as_str().split('/').collect();
    let Some((_, parents)) = segments.split_last() else {
        return Ok(None);
    };
    match walk(root, parents).await? {
        Walk::Blocked(at) => return Ok(Some(at)),
        Walk::Missing => return Ok(None),
        Walk::Directories => {}
    }
    match tokio::fs::symlink_metadata(root.join(file.as_str())).await {
        Ok(_) => Ok(Some(file.clone())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// What is in the way of a directory at `dir` under `root`, if anything: `dir`
/// itself or a parent inside the root that is there and is no directory, by the
/// rule [`in_the_way`] gives a parent. A directory there, or a link to one, is none.
pub(crate) async fn dir_in_the_way(root: &Path, dir: &RelPath) -> io::Result<Option<RelPath>> {
    let segments: Vec<&str> = dir.as_str().split('/').collect();
    match walk(root, &segments).await? {
        Walk::Blocked(at) => Ok(Some(at)),
        Walk::Missing | Walk::Directories => Ok(None),
    }
}

/// Where a walk down a path, a segment at a time, ended.
enum Walk {
    /// At a segment that is there and is no directory.
    Blocked(RelPath),
    /// At a segment that is not there: nothing below it is either.
    Missing,
    /// Every segment is a directory, or a link to one.
    Directories,
}

/// Walk down `segments` under `root`, each prefix once, to the first that is not a
/// directory there, or is a link whose canonical path leaves the canonical root.
///
/// A link is followed only while its canonical path stays inside the canonical
/// root, the rule the space `file` reads by (`place_of` in
/// [`artifact::file`](super::artifact::file)), so nothing is ever written outside
/// the project through a link: a link that leads out is in the way, by its path.
async fn walk(root: &Path, segments: &[&str]) -> io::Result<Walk> {
    let mut canonical_root = None;
    for end in 1..=segments.len() {
        let rel = segments[..end].join("/");
        let path = root.join(&rel);
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) => meta,
            // Nothing below a parent that is not there can be there either.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Walk::Missing),
            Err(error) => return Err(error),
        };
        // `Metadata::is_dir` reads a value already read, by the async calls here; it
        // is written in full so the workspace's lint of blocking calls, which goes
        // by the method's name, does not take it for `Path::is_dir`.
        let a_directory = if meta.is_symlink() {
            let to_dir = match tokio::fs::metadata(&path).await {
                Ok(target) => std::fs::Metadata::is_dir(&target),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => return Err(error),
            };
            to_dir && {
                let inside = match &canonical_root {
                    Some(inside) => inside,
                    None => canonical_root.insert(tokio::fs::canonicalize(root).await?),
                };
                tokio::fs::canonicalize(&path).await?.starts_with(inside)
            }
        } else {
            std::fs::Metadata::is_dir(&meta)
        };
        if !a_directory {
            return rel.parse().map(Walk::Blocked).map_err(io::Error::other);
        }
    }
    Ok(Walk::Directories)
}

/// Write `bytes` to a new file at `file`, or find something there and write
/// nothing. The bytes are handed to the operating system, whole, and are not put on
/// disk: [`persist`] does that, for every file of a run at once.
pub(crate) async fn create_new(file: &Path, bytes: &[u8]) -> Result<Creation, CreateFailed> {
    create_with(file, async |out: &mut File| {
        // `flush` waits for the writes the file still has in flight, so that when
        // this returns every byte is with the operating system (and a failure of a
        // write is told here), before `persist` opens the file again.
        let written = async {
            out.write_all(bytes).await?;
            out.flush().await
        };
        written
            .await
            .map_err(|e| (format!("write {}", file.display()), e))
    })
    .await
}

/// The step of putting files on disk that failed, with its path, and the operating
/// system's error.
#[derive(Debug, thiserror::Error)]
#[error("cannot {step}")]
pub struct PersistFailed {
    /// The step (`open <path> to put it on disk`, `put <path> on disk`).
    pub step: String,
    /// The operating system's error.
    #[source]
    pub source: io::Error,
}

/// What a failed step of putting files on disk is told as.
fn failed(step: String) -> impl FnOnce(io::Error) -> PersistFailed {
    move |source| PersistFailed { step, source }
}

/// Put the files `files` (relative to `root`), each written whole before, on disk,
/// so that they survive a power loss, by the rules of the module page. The first
/// step that fails ends it: the files may then be on disk or not.
///
/// A file or a directory is opened by the runtime's own calls and put on disk on a
/// thread of the pool for blocking work, so the executor is never stalled by a sync.
#[cfg(unix)]
pub(crate) async fn persist(root: &Path, files: &[RelPath]) -> Result<(), PersistFailed> {
    persist_with(root, files, platform_full_flush()).await
}

/// Put the files `files` (relative to `root`), each written whole before, on disk,
/// by the rules of the module page: each opened again for writing, neither
/// truncated nor appended to, and synced; no directory is.
#[cfg(not(unix))]
pub(crate) async fn persist(root: &Path, files: &[RelPath]) -> Result<(), PersistFailed> {
    for file in files {
        let path = root.join(file.as_str());
        let shown = path.display();
        let open = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .map_err(failed(format!("open {shown} to put it on disk")))?;
        open.sync_all()
            .await
            .map_err(failed(format!("put {shown} on disk")))?;
    }
    Ok(())
}

/// Run a step that blocks on a thread of the pool for blocking work.
#[cfg(unix)]
async fn blocking<T: Send + 'static>(
    step: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    match tokio::task::spawn_blocking(step).await {
        Ok(done) => done,
        Err(joined) => Err(io::Error::other(joined)),
    }
}

/// The path a relative `path` stands for under `root`; the root itself for none.
#[cfg(unix)]
fn under(root: &Path, path: &str) -> std::path::PathBuf {
    if path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(path)
    }
}

/// A full flush of the device a file is on, as the platform has one.
#[cfg(unix)]
type FullFlush = std::sync::Arc<dyn Fn(&std::fs::File) -> rustix::io::Result<()> + Send + Sync>;

/// The full flush the platform has, if it has one.
#[cfg(all(unix, target_vendor = "apple"))]
fn platform_full_flush() -> Option<FullFlush> {
    Some(std::sync::Arc::new(|file: &std::fs::File| {
        rustix::fs::fcntl_fullfsync(file)
    }))
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn platform_full_flush() -> Option<FullFlush> {
    None
}

/// `fsync` of an open file or directory: the system's own on Apple systems, where
/// the standard library's `sync_all` is a full flush; `sync_all` elsewhere.
#[cfg(all(unix, target_vendor = "apple"))]
fn fsync(file: &std::fs::File) -> io::Result<()> {
    rustix::fs::fsync(file).map_err(io::Error::from)
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn fsync(file: &std::fs::File) -> io::Result<()> {
    file.sync_all()
}

/// Whether a device that answered a full flush with `errno` cannot give one at all
/// (a file system with no such request), so that what `fsync` did is the most there
/// is. Any other answer is a failure.
#[cfg(unix)]
fn cannot_flush_fully(errno: rustix::io::Errno) -> bool {
    use rustix::io::Errno;
    errno == Errno::NOTSUP || errno == Errno::INVAL || errno == Errno::NOTTY
}

/// [`persist`] on Unix, with the full flush given (none where the platform has
/// none), so that how a device that cannot flush fully is left can be tried.
#[cfg(unix)]
async fn persist_with(
    root: &Path,
    files: &[RelPath],
    full_flush: Option<FullFlush>,
) -> Result<(), PersistFailed> {
    use std::os::unix::fs::MetadataExt;

    let mut dirs: BTreeSet<String> = BTreeSet::new();
    // One open file for each device the files are on: what the full flush is asked of.
    let mut devices: BTreeMap<u64, std::fs::File> = BTreeMap::new();
    for file in files {
        let path = under(root, file.as_str());
        let shown = path.display();
        let open = File::open(&path)
            .await
            .map_err(failed(format!("open {shown} to put it on disk")))?;
        // Written in full, as `create_with` writes it, for the lint of blocking
        // calls, which goes by the method's name.
        let device = File::metadata(&open)
            .await
            .map_err(failed(format!("look at {shown} to put it on disk")))?
            .dev();
        let open = open.into_std().await;
        // The file is given back, so one per device can be kept; every other is
        // closed here, none is held until the end.
        let open = blocking(move || fsync(&open).map(|()| open))
            .await
            .map_err(failed(format!("put {shown} on disk")))?;
        devices.entry(device).or_insert(open);
        let segments: Vec<&str> = file.as_str().split('/').collect();
        for end in 0..segments.len() {
            dirs.insert(segments[..end].join("/"));
        }
    }
    for dir in &dirs {
        let path = under(root, dir);
        let shown = path.display();
        let open = File::open(&path)
            .await
            .map_err(failed(format!(
                "open the directory {shown} to put it on disk"
            )))?
            .into_std()
            .await;
        blocking(move || fsync(&open))
            .await
            .map_err(failed(format!("put the directory {shown} on disk")))?;
    }
    if let Some(flush) = full_flush {
        for open in devices.into_values() {
            let flush = std::sync::Arc::clone(&flush);
            match blocking(move || Ok(flush(&open))).await {
                Ok(Ok(())) => {}
                Ok(Err(errno)) if cannot_flush_fully(errno) => {}
                Ok(Err(errno)) => {
                    return Err(PersistFailed {
                        step: "flush the drive's cache after putting the files on disk".to_owned(),
                        source: io::Error::from(errno),
                    });
                }
                Err(source) => {
                    return Err(PersistFailed {
                        step: "flush the drive's cache after putting the files on disk".to_owned(),
                        source,
                    });
                }
            }
        }
    }
    Ok(())
}

/// [`create_new`], with what is written into the file made given as a step.
async fn create_with(
    file: &Path,
    write: impl AsyncFnOnce(&mut File) -> Result<(), (String, io::Error)>,
) -> Result<Creation, CreateFailed> {
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|source| CreateFailed {
                step: format!("create the directory {}", dir.display()),
                source,
            })?;
    }
    let mut out = match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file)
        .await
    {
        Ok(out) => out,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Ok(Creation::AlreadyThere);
        }
        Err(source) => {
            return Err(CreateFailed {
                step: format!("create {}", file.display()),
                source,
            });
        }
    };
    // tokio's `File::metadata` is async; written in full for the lint of blocking
    // calls, which goes by the method's name. When the open file cannot tell its
    // identity, the cleanup below leaves the file and says so.
    let made = File::metadata(&out).await.ok().map(|meta| identity(&meta));
    let written = write(&mut out).await;
    drop(out);
    let Err((step, source)) = written else {
        return Ok(Creation::Created);
    };
    let left = remove_if_made(file, made).await;
    Err(CreateFailed {
        step: format!("{step} ({left})"),
        source,
    })
}

/// What identifies a file on this platform: its device and inode on Unix, nothing
/// elsewhere.
#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(not(unix))]
type Identity = ();

#[cfg(unix)]
fn identity(meta: &std::fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn identity(_meta: &std::fs::Metadata) -> Identity {}

/// Remove the file at `file` when it is the one the creation made (`made`, its
/// identity when the open file could tell it), and say what became of it.
async fn remove_if_made(file: &Path, made: Option<Identity>) -> String {
    let now = match tokio::fs::symlink_metadata(file).await {
        Ok(meta) => identity(&meta),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return "the file it made is gone already".to_owned();
        }
        Err(error) => return format!("the file it made could not be looked at: {error}"),
    };
    let Some(made) = made else {
        return "the file it made was left: it could not tell that file from another".to_owned();
    };
    if made != now {
        return "the file at the path is no longer the one it made, and was left".to_owned();
    }
    match tokio::fs::remove_file(file).await {
        Ok(()) => "the file it made was removed".to_owned(),
        Err(error) => format!("the file it made could not be removed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_new_file_is_written_whole_with_its_directories_and_nothing_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a/b/c.txt");
        assert_eq!(create_new(&file, b"one").await.unwrap(), Creation::Created);
        assert_eq!(
            create_new(&file, b"two").await.unwrap(),
            Creation::AlreadyThere
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"one");
    }

    fn rel(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    #[tokio::test]
    async fn what_is_at_a_path_or_a_parent_that_is_no_directory_is_in_the_way() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(in_the_way(root, &rel("a/b.md")).await.unwrap(), None);
        tokio::fs::create_dir_all(root.join("a/b.md"))
            .await
            .unwrap();
        assert_eq!(
            in_the_way(root, &rel("a/b.md")).await.unwrap(),
            Some(rel("a/b.md"))
        );
        tokio::fs::write(root.join("f"), b"a file").await.unwrap();
        assert_eq!(
            in_the_way(root, &rel("f/g/h.md")).await.unwrap(),
            Some(rel("f"))
        );
        #[cfg(unix)]
        {
            tokio::fs::symlink(root.join("nowhere"), root.join("dangling"))
                .await
                .unwrap();
            assert_eq!(
                in_the_way(root, &rel("dangling")).await.unwrap(),
                Some(rel("dangling"))
            );
            assert_eq!(
                in_the_way(root, &rel("dangling/x.md")).await.unwrap(),
                Some(rel("dangling"))
            );
            assert_eq!(
                create_new(&root.join("dangling"), b"x").await.unwrap(),
                Creation::AlreadyThere
            );
            tokio::fs::symlink(root.join("f"), root.join("to-file"))
                .await
                .unwrap();
            assert_eq!(
                in_the_way(root, &rel("to-file/x.md")).await.unwrap(),
                Some(rel("to-file"))
            );
            // A link to a directory is a way through.
            tokio::fs::symlink(root.join("a"), root.join("to-dir"))
                .await
                .unwrap();
            assert_eq!(in_the_way(root, &rel("to-dir/new.md")).await.unwrap(), None);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_that_leads_out_of_the_root_is_in_the_way() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        tokio::fs::create_dir_all(root.join("inside"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(outer.path().join("out"))
            .await
            .unwrap();
        tokio::fs::symlink("../out", root.join("away"))
            .await
            .unwrap();
        tokio::fs::symlink("inside", root.join("near"))
            .await
            .unwrap();
        assert_eq!(
            in_the_way(&root, &rel("away/table.csv")).await.unwrap(),
            Some(rel("away"))
        );
        assert_eq!(
            dir_in_the_way(&root, &rel("away/books")).await.unwrap(),
            Some(rel("away"))
        );
        assert_eq!(
            dir_in_the_way(&root, &rel("away")).await.unwrap(),
            Some(rel("away"))
        );
        // One that stays inside is still a way through.
        assert_eq!(
            in_the_way(&root, &rel("near/table.csv")).await.unwrap(),
            None
        );
    }

    /// A write step that writes part of the bytes and then fails.
    async fn fails(out: &mut File) -> Result<(), (String, io::Error)> {
        out.write_all(b"hal").await.unwrap();
        Err(("write".to_owned(), io::Error::other("the disk is full")))
    }

    #[tokio::test]
    async fn a_write_that_fails_after_the_file_was_made_removes_that_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("half.txt");
        let beside = dir.path().join("beside.txt");
        tokio::fs::write(&beside, b"kept").await.unwrap();
        let failed = create_with(&file, fails).await.unwrap_err();
        assert_eq!(failed.step, "write (the file it made was removed)");
        assert_eq!(failed.source.to_string(), "the disk is full");
        assert!(tokio::fs::symlink_metadata(&file).await.is_err());
        assert_eq!(tokio::fs::read(&beside).await.unwrap(), b"kept");
    }

    #[tokio::test]
    async fn files_made_in_nested_directories_are_put_on_disk_whole() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("a/b/c.md");
        let top = root.join("d.md");
        assert_eq!(
            create_new(&nested, b"nested").await.unwrap(),
            Creation::Created
        );
        assert_eq!(create_new(&top, b"top").await.unwrap(), Creation::Created);
        persist(root, &[rel("a/b/c.md"), rel("d.md")])
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&nested).await.unwrap(), b"nested");
        assert_eq!(tokio::fs::read(&top).await.unwrap(), b"top");
        // Putting files on disk changes none of them, and may be asked again.
        persist(root, &[rel("a/b/c.md")]).await.unwrap();
        assert_eq!(tokio::fs::read(&nested).await.unwrap(), b"nested");
    }

    #[tokio::test]
    async fn a_file_that_is_not_there_cannot_be_put_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("here.md"), b"x").await.unwrap();
        let failed = persist(root, &[rel("here.md"), rel("gone/nowhere.md")])
            .await
            .unwrap_err();
        assert!(failed.step.starts_with("open "), "{}", failed.step);
        assert!(failed.step.contains("nowhere.md"), "{}", failed.step);
        assert_eq!(failed.source.kind(), io::ErrorKind::NotFound);
    }

    /// A full flush that answers `errno` every time, and counts how often it was
    /// asked.
    #[cfg(unix)]
    fn flush_answering(
        errno: rustix::io::Errno,
    ) -> (FullFlush, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let asked = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&asked);
        let flush: FullFlush = Arc::new(move |_: &std::fs::File| {
            counted.fetch_add(1, Ordering::SeqCst);
            Err(errno)
        });
        (flush, asked)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_device_that_cannot_flush_fully_is_left_at_fsync_and_any_other_failure_is_told() {
        use rustix::io::Errno;
        use std::sync::atomic::Ordering;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("a")).await.unwrap();
        tokio::fs::write(root.join("a/one.md"), b"1").await.unwrap();
        tokio::fs::write(root.join("two.md"), b"2").await.unwrap();
        let files = [rel("a/one.md"), rel("two.md")];

        for unsupported in [Errno::NOTSUP, Errno::INVAL, Errno::NOTTY] {
            let (flush, asked) = flush_answering(unsupported);
            persist_with(root, &files, Some(flush)).await.unwrap();
            // Both files are on one device: one full flush, however many files.
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{unsupported:?}");
        }

        let (broken, asked) = flush_answering(Errno::IO);
        let failed = persist_with(root, &files, Some(broken)).await.unwrap_err();
        assert!(failed.step.starts_with("flush "), "{}", failed.step);
        assert_eq!(asked.load(Ordering::SeqCst), 1);

        // Without a full flush (a platform that has none) there is nothing to fall
        // back from.
        persist_with(root, &files, None).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_file_put_in_the_place_of_the_one_made_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("half.txt");
        let path = file.clone();
        let failed = create_with(&file, async move |_out: &mut File| {
            // Someone moves the file made away and puts one of theirs in its place.
            tokio::fs::rename(&path, path.with_extension("moved"))
                .await
                .unwrap();
            tokio::fs::write(&path, b"theirs").await.unwrap();
            Err(("write".to_owned(), io::Error::other("the disk is full")))
        })
        .await
        .unwrap_err();
        assert_eq!(
            failed.step,
            "write (the file at the path is no longer the one it made, and was left)"
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"theirs");
    }
}
