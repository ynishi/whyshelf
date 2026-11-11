//! A new file, written whole or not at all: the one way the adapters create a file
//! where nothing is (the declaration, a new document).
//!
//! The file is opened `create_new`, so nothing that is there is ever replaced,
//! with its directories made first. If writing its bytes, or putting them on disk,
//! fails after the file was made, the file this call made is removed, and only that
//! one: a failure leaves nothing half-written behind. "Only that one" is checked:
//! on Unix the file at the path is removed only when it is still the one the call
//! opened (the same device and inode); one put in its place in between is left, and
//! the error says so. Elsewhere the platform gives no such identity through the
//! standard library, so the file at the path is removed as it is.
//!
//! [`in_the_way`] tells, before anything is written, what `create_new` would find
//! in its way: something at the path, or a parent that is no directory.

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
/// leads to). A parent that is a link to a directory is no obstacle: the file is
/// made through it, as [`create_new`] makes it.
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
/// directory there.
async fn walk(root: &Path, segments: &[&str]) -> io::Result<Walk> {
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
            match tokio::fs::metadata(&path).await {
                Ok(target) => std::fs::Metadata::is_dir(&target),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => return Err(error),
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
/// nothing.
pub(crate) async fn create_new(file: &Path, bytes: &[u8]) -> Result<Creation, CreateFailed> {
    create_with(file, async |out: &mut File| {
        match out.write_all(bytes).await {
            Ok(()) => out
                .sync_all()
                .await
                .map_err(|e| (format!("write {} to disk", file.display()), e)),
            Err(e) => Err((format!("write {}", file.display()), e)),
        }
    })
    .await
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
