//! Writing a user's document file: [`FileWriter`] over the file system, on Unix.
//!
//! [`FsWriter`] is the one way the tool changes a file a person owns. It keeps the
//! contract of the port by doing each check and the act it guards in one motion
//! on file descriptors, never "look at the path, then use the path":
//!
//! ```text
//!   root dir --open--> dir fd --openat (each element, no symlink)--> parent fd
//!                                                                      |
//!        last element: openat with O_NOFOLLOW ------------------------+
//!            |
//!            +-- create_new: O_CREAT|O_EXCL ---------------------------> Done | Exists
//!            |
//!            +-- rewrite:  read fd, hash == expect?   no -> ChangedUnderfoot
//!            |             fstat, links >= 2?         yes -> MultipleLinks
//!            |             temp file in the same dir (O_EXCL, 0600), write text
//!            |             fchown / fchmod to the original's owner, group, mode
//!            |             read the target again through parent fd, same?
//!            |                  no -> remove temp, ChangedUnderfoot
//!            |             renameat(temp, target) ----------------------> Done
//!            |
//!            +-- remove:   read fd, hash == expect? -> unlinkat --------> Done
//! ```
//!
//! # Where a path is
//!
//! A [`Location`] names a root by its visibility and a path under it. The public
//! root is always there; the private one only when the person has one, and a
//! location in a private root that is not configured is
//! [`WriterError::NoPrivateRoot`], never a write to the public root. The path is a
//! [`RelPath`](whyshelf_core::domain::location::RelPath), which cannot climb out of
//! the root; every element before the last is opened without following a symbolic
//! link too, so a link anywhere on the path is [`Write::NotRegularFile`]. For
//! [`create_new`](FileWriter::create_new) the missing directories on the way are
//! made; the other two never make one. What `create_new` makes in the public root
//! is `0644` (a directory `0755`), and in the private root `0600` (`0700`), so a
//! private document is never made readable by others; both before the umask.
//!
//! # What each answer means
//!
//! | situation | answer |
//! |---|---|
//! | last element is a symbolic link (also a dangling one), a directory, or any non-regular file | [`Write::NotRegularFile`] |
//! | `create_new`, a regular file is there | [`Write::Exists`] |
//! | `rewrite` or `remove`, the bytes are not `expect` | [`Write::ChangedUnderfoot`] |
//! | `rewrite` or `remove`, there is no file (or no directory on the way) | [`Write::ChangedUnderfoot`]: the file the caller read is gone |
//! | `rewrite`, the file has two or more hard links | [`Write::MultipleLinks`] |
//! | `rewrite`, the group cannot be kept and the original's mode gives the group any access | [`Write::CannotKeepGroup`] |
//! | `rewrite`, the owner cannot be kept and is not our own | [`Write::CannotKeepOwner`] |
//!
//! A failure of a system call is none of these: it is a [`WriterError`] with one
//! variant for each operation that failed (open, stat, read, create, write, chown,
//! chmod, rename, unlink), so a caller can tell what was being done.
//!
//! # The replacement
//!
//! The text goes to a temporary file in the same directory (so that the rename
//! stays on one file system), created with `O_EXCL` and mode `0600`, named
//! `.whyshelf-<random uuid>.tmp`: hidden, and unrelated to the target's name, so a
//! long file name cannot make it too long. Owner and group are set with `fchown`
//! and then the permission bits with `fchmod` (in that order, since a change of
//! owner may clear set-user-ID bits). A temporary file is removed on every way out
//! but the rename.
//!
//! # Known limits
//!
//! - A write by an editor that takes no advisory lock, between the last comparison
//!   and `renameat` (or `unlinkat`), cannot be shut out; the window is the time of
//!   a few system calls, not of the whole operation.
//! - Extended attributes (and ACLs kept in them) are not copied to the replacement.
//! - Timestamps are the replacement's own.
//!
//! # Blocking
//!
//! The work is a short run of system calls on a file, so it runs in
//! [`tokio::task::spawn_blocking`], as the other file adapters do.

use std::fs::File;
use std::io::{self, Read, Write as _};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::PathBuf;

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, RawMode, Stat, fchmod, fchown, fstat, mkdirat, open, openat,
    renameat, statat, unlinkat,
};
use rustix::fs::{Gid, Uid};
use rustix::io::Errno;
use uuid::Uuid;
use whyshelf_core::app::ports::{FileWriter, Write};
use whyshelf_core::domain::corpus::{Location, Visibility};
use whyshelf_core::domain::hash::BytesHash;

/// How many times a temporary name is drawn again when it is taken.
const TMP_TRIES: u8 = 8;

/// Why a file could not be written, apart from the answers of [`Write`].
#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    /// The location is in the private root, and there is none here.
    #[error("the private root is not configured, so {path} cannot be written")]
    NoPrivateRoot {
        /// The path asked for.
        path: String,
    },
    /// A file or a directory on the way could not be opened.
    #[error("cannot open {path}")]
    Open {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The state of a file could not be read (`fstat`, `fstatat`).
    #[error("cannot read the state of {path}")]
    Stat {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The bytes of a file could not be read.
    #[error("cannot read {path}")]
    Read {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// A file or a directory could not be created: the file itself, a missing
    /// directory on the way, or the temporary file of a replacement.
    #[error("cannot create a file or a directory for {path}")]
    Create {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The bytes could not be written, or made durable.
    #[error("cannot write {path}")]
    Write {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The replacement could not be given the owner or the group of the original
    /// for a reason other than not being allowed to (that is a [`Write`] answer).
    #[error("cannot set the owner or the group on the replacement of {path}")]
    Chown {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The replacement could not be given the original's permission bits.
    #[error("cannot set the permissions on the replacement of {path}")]
    Chmod {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The replacement could not be renamed over the original.
    #[error("cannot replace {path}")]
    Rename {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The file could not be removed.
    #[error("cannot remove {path}")]
    Unlink {
        /// The file the operation was for.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// The task did not run to the end (it panicked, or the runtime is closing).
    #[error("a blocking task did not finish")]
    Join(#[source] tokio::task::JoinError),
}

/// The documents' roots, written through the descriptors of their directories.
#[derive(Debug, Clone)]
pub struct FsWriter {
    public: PathBuf,
    private: Option<PathBuf>,
}

impl FsWriter {
    /// A writer for the public root at `public`, and for the private root if the
    /// person has one here.
    pub fn new(public: PathBuf, private: Option<PathBuf>) -> Self {
        Self { public, private }
    }

    /// The directory a location's root is, and the path of the file in it.
    fn target(&self, at: &Location) -> Result<Target, WriterError> {
        let root = match at.visibility() {
            Visibility::Public => &self.public,
            Visibility::Private => {
                self.private
                    .as_ref()
                    .ok_or_else(|| WriterError::NoPrivateRoot {
                        path: at.path().to_string(),
                    })?
            }
        };
        Ok(Target {
            root: root.clone(),
            rel: at.path().as_str().to_owned(),
            private: at.visibility() == Visibility::Private,
        })
    }
}

/// A file to act on: the root's directory and the `/`-separated path below it.
struct Target {
    root: PathBuf,
    rel: String,
    /// In the private root: what is made there is the person's alone.
    private: bool,
}

impl Target {
    /// The mode a new file is made with (before the umask).
    fn file_mode(&self) -> Mode {
        Mode::from_raw_mode(if self.private { 0o600 } else { 0o644 })
    }

    /// The mode a new directory is made with (before the umask).
    fn dir_mode(&self) -> Mode {
        Mode::from_raw_mode(if self.private { 0o700 } else { 0o755 })
    }

    fn full(&self) -> PathBuf {
        self.root.join(&self.rel)
    }

    // One constructor per operation, so that a failure says which call failed.

    fn open_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Open {
            path: self.full(),
            source: source.into(),
        }
    }

    fn stat_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Stat {
            path: self.full(),
            source: source.into(),
        }
    }

    fn read_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Read {
            path: self.full(),
            source: source.into(),
        }
    }

    fn create_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Create {
            path: self.full(),
            source: source.into(),
        }
    }

    fn write_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Write {
            path: self.full(),
            source: source.into(),
        }
    }

    fn chown_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Chown {
            path: self.full(),
            source: source.into(),
        }
    }

    fn chmod_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Chmod {
            path: self.full(),
            source: source.into(),
        }
    }

    fn rename_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Rename {
            path: self.full(),
            source: source.into(),
        }
    }

    fn unlink_failed(&self, source: impl Into<io::Error>) -> WriterError {
        WriterError::Unlink {
            path: self.full(),
            source: source.into(),
        }
    }

    /// The directories to walk and the name of the file in the last.
    fn split(&self) -> (Vec<&str>, &str) {
        let mut parts: Vec<&str> = self.rel.split('/').collect();
        let name = parts.pop().unwrap_or_default();
        (parts, name)
    }
}

async fn blocking<T, F>(job: F) -> Result<T, WriterError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, WriterError> + Send + 'static,
{
    tokio::task::spawn_blocking(job)
        .await
        .map_err(WriterError::Join)?
}

impl FileWriter for FsWriter {
    type Error = WriterError;

    async fn create_new(&self, at: &Location, text: &[u8]) -> Result<Write, WriterError> {
        let target = self.target(at)?;
        let text = text.to_vec();
        blocking(move || create_new(&target, &text)).await
    }

    async fn rewrite(
        &self,
        at: &Location,
        expect: BytesHash,
        text: &[u8],
    ) -> Result<Write, WriterError> {
        let target = self.target(at)?;
        let text = text.to_vec();
        blocking(move || rewrite(&target, &expect, &text)).await
    }

    async fn remove(&self, at: &Location, expect: BytesHash) -> Result<Write, WriterError> {
        let target = self.target(at)?;
        blocking(move || remove(&target, &expect)).await
    }
}

// --- walking ----------------------------------------------------------------------------

/// What walking to the parent directory gave.
enum Walk {
    Dir(OwnedFd),
    /// A link or a file is where a directory should be.
    NotRegular,
    /// A directory on the way is not there.
    Missing,
}

fn walk(target: &Target, dirs: &[&str], make: bool) -> Result<Walk, WriterError> {
    let mut dir = open(
        &target.root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| target.open_failed(e))?;
    for name in dirs {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let next = match openat(&dir, *name, flags, Mode::empty()) {
            Err(Errno::NOENT) if make => {
                match mkdirat(&dir, *name, target.dir_mode()) {
                    // Another writer made it first: that is as good.
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err(target.create_failed(e)),
                }
                openat(&dir, *name, flags, Mode::empty())
            }
            other => other,
        };
        dir = match next {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(Walk::Missing),
            // A symbolic link (`ELOOP`; `ENOTDIR` on some systems) or a file.
            Err(Errno::LOOP | Errno::NOTDIR) => return Ok(Walk::NotRegular),
            Err(e) => return Err(target.open_failed(e)),
        };
    }
    Ok(Walk::Dir(dir))
}

fn mode_of(stat: &Stat) -> RawMode {
    // The width of `st_mode` differs between systems.
    #[allow(clippy::unnecessary_cast)]
    {
        stat.st_mode as RawMode
    }
}

fn is_regular(stat: &Stat) -> bool {
    FileType::from_raw_mode(mode_of(stat)) == FileType::RegularFile
}

fn links(stat: &Stat) -> u64 {
    #[allow(clippy::useless_conversion)]
    u64::from(stat.st_nlink)
}

/// The same file: same device and inode.
fn same_file(a: &Stat, b: &Stat) -> bool {
    a.st_dev == b.st_dev && a.st_ino == b.st_ino
}

/// What opening the last element for reading gave.
enum Opened {
    File(File, Stat),
    NotRegular,
    Missing,
}

fn open_existing(target: &Target, dir: BorrowedFd<'_>, name: &str) -> Result<Opened, WriterError> {
    // `NONBLOCK` so that a named pipe cannot hold the open; it is refused below.
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd = match openat(dir, name, flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(Opened::Missing),
        // A symbolic link; some systems say `EMLINK` or `NXIO` for other cases.
        Err(Errno::LOOP | Errno::MLINK | Errno::NXIO) => return Ok(Opened::NotRegular),
        Err(e) => return Err(target.open_failed(e)),
    };
    let stat = fstat(&fd).map_err(|e| target.stat_failed(e))?;
    if !is_regular(&stat) {
        return Ok(Opened::NotRegular);
    }
    Ok(Opened::File(File::from(fd), stat))
}

fn read_all(target: &Target, file: &mut File) -> Result<Vec<u8>, WriterError> {
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| target.read_failed(e))?;
    Ok(bytes)
}

// --- create_new -------------------------------------------------------------------------

fn create_new(target: &Target, text: &[u8]) -> Result<Write, WriterError> {
    let (dirs, name) = target.split();
    let dir = match walk(target, &dirs, true)? {
        Walk::Dir(dir) => dir,
        Walk::NotRegular => return Ok(Write::NotRegularFile),
        // Made on the way, so only a race can leave one missing.
        Walk::Missing => return Err(target.open_failed(Errno::NOENT)),
    };
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match openat(&dir, name, flags, target.file_mode()) {
        Ok(fd) => fd,
        // `O_EXCL` also refuses a symbolic link, dangling or not, and a directory:
        // what is there decides the answer.
        Err(Errno::EXIST) => return exists_or_not_regular(target, &dir, name),
        Err(Errno::LOOP) => return Ok(Write::NotRegularFile),
        Err(e) => return Err(target.create_failed(e)),
    };
    let mut file = File::from(fd);
    if let Err(source) = file.write_all(text).and_then(|()| file.sync_all()) {
        // Do not leave half a file that the next attempt would call "exists".
        drop(file);
        let _ = unlinkat(&dir, name, AtFlags::empty());
        return Err(target.write_failed(source));
    }
    Ok(Write::Done)
}

fn exists_or_not_regular(target: &Target, dir: &OwnedFd, name: &str) -> Result<Write, WriterError> {
    match statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if is_regular(&stat) => Ok(Write::Exists),
        Ok(_) => Ok(Write::NotRegularFile),
        // Gone between the two calls: it was there, and it is the caller's to retry.
        Err(Errno::NOENT) => Ok(Write::Exists),
        Err(e) => Err(target.stat_failed(e)),
    }
}

// --- rewrite ----------------------------------------------------------------------------

fn rewrite(target: &Target, expect: &BytesHash, text: &[u8]) -> Result<Write, WriterError> {
    let (dirs, name) = target.split();
    let dir = match walk(target, &dirs, false)? {
        Walk::Dir(dir) => dir,
        Walk::NotRegular => return Ok(Write::NotRegularFile),
        Walk::Missing => return Ok(Write::ChangedUnderfoot),
    };
    let (mut file, stat) = match open_existing(target, dir.as_fd(), name)? {
        Opened::File(file, stat) => (file, stat),
        Opened::NotRegular => return Ok(Write::NotRegularFile),
        Opened::Missing => return Ok(Write::ChangedUnderfoot),
    };
    if BytesHash::of(&read_all(target, &mut file)?) != *expect {
        return Ok(Write::ChangedUnderfoot);
    }
    if links(&stat) >= 2 {
        return Ok(Write::MultipleLinks);
    }

    let (tmp_name, tmp) = create_temp(target, &dir)?;
    let original = Original {
        dir: &dir,
        name,
        stat: &stat,
        expect,
    };
    let outcome = replace(target, &original, text, &tmp_name, tmp);
    if !matches!(outcome, Ok(Write::Done)) {
        let _ = unlinkat(&dir, tmp_name.as_str(), AtFlags::empty());
    }
    outcome
}

/// The file being replaced, as it was read.
struct Original<'a> {
    dir: &'a OwnedFd,
    name: &'a str,
    stat: &'a Stat,
    expect: &'a BytesHash,
}

/// The temporary file of a replacement: exclusive, `0600`, in the target's directory.
fn create_temp(target: &Target, dir: &OwnedFd) -> Result<(String, File), WriterError> {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    for _ in 0..TMP_TRIES {
        let name = format!(".whyshelf-{}.tmp", Uuid::new_v4().simple());
        match openat(dir, name.as_str(), flags, Mode::from_raw_mode(0o600)) {
            Ok(fd) => return Ok((name, File::from(fd))),
            Err(Errno::EXIST) => {}
            Err(e) => return Err(target.create_failed(e)),
        }
    }
    Err(target.create_failed(Errno::EXIST))
}

/// Fill the temporary file, give it the original's attributes, check once more, and
/// rename it over the target. The caller removes the temporary file unless this is
/// `Done`.
fn replace(
    target: &Target,
    original: &Original<'_>,
    text: &[u8],
    tmp_name: &str,
    mut tmp: File,
) -> Result<Write, WriterError> {
    tmp.write_all(text).map_err(|e| target.write_failed(e))?;

    // The owner first: it may clear set-user-ID bits that the mode then sets again.
    let uid = Uid::from_raw(original.stat.st_uid);
    // The temporary file is ours: its owner is who we are.
    let ours = fstat(&tmp).map_err(|e| target.stat_failed(e))?.st_uid;
    match fchown(&tmp, Some(uid), None) {
        Ok(()) => {}
        // Our own file keeps its owner without being told to.
        Err(Errno::PERM) if original.stat.st_uid == ours => {}
        // A person can mend this (the file is another user's), so it is an answer of
        // the port and not a failure of the adapter.
        Err(Errno::PERM) => return Ok(Write::CannotKeepOwner),
        Err(e) => return Err(target.chown_failed(e)),
    }
    let have = fstat(&tmp).map_err(|e| target.stat_failed(e))?;
    if have.st_gid != original.stat.st_gid {
        let gid = Gid::from_raw(original.stat.st_gid);
        match fchown(&tmp, None, Some(gid)) {
            Ok(()) => {}
            Err(Errno::PERM) => {
                // A group with no access to the file loses nothing by the change.
                if mode_of(original.stat) & 0o070 != 0 {
                    return Ok(Write::CannotKeepGroup);
                }
            }
            Err(e) => return Err(target.chown_failed(e)),
        }
    }
    fchmod(&tmp, Mode::from_raw_mode(mode_of(original.stat) & 0o7777))
        .map_err(|e| target.chmod_failed(e))?;
    tmp.sync_all().map_err(|e| target.write_failed(e))?;

    // Just before the rename: the same file, with the same bytes, still alone.
    let (mut again, now) = match open_existing(target, original.dir.as_fd(), original.name)? {
        Opened::File(file, stat) => (file, stat),
        Opened::NotRegular => return Ok(Write::NotRegularFile),
        Opened::Missing => return Ok(Write::ChangedUnderfoot),
    };
    if !same_file(original.stat, &now)
        || BytesHash::of(&read_all(target, &mut again)?) != *original.expect
    {
        return Ok(Write::ChangedUnderfoot);
    }
    if links(&now) >= 2 {
        return Ok(Write::MultipleLinks);
    }
    renameat(original.dir, tmp_name, original.dir, original.name)
        .map_err(|e| target.rename_failed(e))?;
    Ok(Write::Done)
}

// --- remove -----------------------------------------------------------------------------

fn remove(target: &Target, expect: &BytesHash) -> Result<Write, WriterError> {
    let (dirs, name) = target.split();
    let dir = match walk(target, &dirs, false)? {
        Walk::Dir(dir) => dir,
        Walk::NotRegular => return Ok(Write::NotRegularFile),
        Walk::Missing => return Ok(Write::ChangedUnderfoot),
    };
    let (mut file, stat) = match open_existing(target, dir.as_fd(), name)? {
        Opened::File(file, stat) => (file, stat),
        Opened::NotRegular => return Ok(Write::NotRegularFile),
        Opened::Missing => return Ok(Write::ChangedUnderfoot),
    };
    if BytesHash::of(&read_all(target, &mut file)?) != *expect {
        return Ok(Write::ChangedUnderfoot);
    }
    // The name must still be the file that was read, not one put there since.
    match statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(now) if same_file(&stat, &now) => {}
        Ok(_) | Err(Errno::NOENT) => return Ok(Write::ChangedUnderfoot),
        Err(e) => return Err(target.stat_failed(e)),
    }
    unlinkat(&dir, name, AtFlags::empty()).map_err(|e| target.unlink_failed(e))?;
    Ok(Write::Done)
}

#[cfg(test)]
mod tests;
