//! Writing the documents: a new document's file, a file written over in the form or
//! put back, a file removed, and whether a file may be written over.

use std::io;
use std::path::Path;

use tokio::io::AsyncWriteExt;
use whyshelf_core::app::shelf::{
    Creation, DocRemove, DocReplace, DocWrite, Removal, Replaceable, Replacement,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::build::FormEntries;

use super::super::create;
use super::{DocsError, MarkdownDocs, render};

/// Whether a file may be written over, from the file's metadata:
///
/// - [`Replaceable::Linked`]: it has more than one link (a hard link). The rename that
///   replaces it would give this path a new file and leave the other links on the
///   old one, so the two would part; and the new file would be the writer's, with
///   none of the old one's owner or extended attributes. (Only on Unix, where the
///   number of links is told.)
/// - [`Replaceable::ReadOnly`]: the user cannot write it. It is opened for writing,
///   which changes nothing in it (no byte, no time); a refusal (no permission, or a
///   file system mounted read-only) is the answer.
/// - [`Replaceable::Yes`] otherwise.
pub(super) async fn replaceable(file: &Path) -> Result<Replaceable, DocsError> {
    let failed = |source| DocsError::Io {
        path: file.to_path_buf(),
        source,
    };
    let meta = tokio::fs::metadata(file).await.map_err(failed)?;
    if links(&meta) > 1 {
        return Ok(Replaceable::Linked);
    }
    match tokio::fs::OpenOptions::new().write(true).open(file).await {
        Ok(_) => Ok(Replaceable::Yes),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            Ok(Replaceable::ReadOnly)
        }
        Err(source) => Err(failed(source)),
    }
}

/// How many links the file has.
#[cfg(unix)]
fn links(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink()
}

/// How many links the file has: not told here, so one.
#[cfg(not(unix))]
fn links(_meta: &std::fs::Metadata) -> u64 {
    1
}

impl DocWrite for MarkdownDocs {
    type Error = DocsError;

    async fn in_the_way(&self, path: &RelPath) -> Result<Option<RelPath>, DocsError> {
        create::in_the_way(&self.root, path)
            .await
            .map_err(|source| DocsError::Io {
                path: self.root.join(path.as_str()),
                source,
            })
    }

    async fn dir_in_the_way(&self, dir: &RelPath) -> Result<Option<RelPath>, DocsError> {
        create::dir_in_the_way(&self.root, dir)
            .await
            .map_err(|source| DocsError::Io {
                path: self.root.join(dir.as_str()),
                source,
            })
    }

    /// Created `create_new` with its directories; a write that fails removes the
    /// file it made, and only it.
    async fn create(
        &self,
        path: &RelPath,
        entries: &FormEntries,
        body: &str,
    ) -> Result<Creation, DocsError> {
        let file = self.root.join(path.as_str());
        create::create_new(&file, render(entries.as_map(), body).as_bytes())
            .await
            .map_err(|failed| DocsError::Write {
                step: failed.step,
                source: failed.source,
            })
    }
}

/// The error of a step of a write, told as the step with its path.
fn write_step(step: String) -> impl FnOnce(io::Error) -> DocsError {
    move |source| DocsError::Write { step, source }
}

/// Writes over a document's file after comparing it with the bytes read: the text goes
/// into a new file beside it (a name starting with `.` and ending in `.tmp`, so it
/// is never read as a document), with the file's permissions, and is renamed over
/// it. A new file that cannot be written is removed.
///
/// The comparison and the rename are two steps: an edit saved to the file in the
/// short time between them is overwritten, and a file removed in it is written
/// again. A read-only file and one with more than one link are never given here:
/// [`DocSource::replaceable`] says so ([`Replaceable::ReadOnly`],
/// [`Replaceable::Linked`]) and `fmt` leaves them alone.
///
/// [`DocReplace::restore`] puts a file back as it was by the same steps, from the text
/// that was written to the bytes that were replaced.
///
/// [`DocSource::replaceable`]: whyshelf_core::app::shelf::DocSource::replaceable
impl DocReplace for MarkdownDocs {
    type Error = DocsError;

    async fn replace_read(
        &self,
        path: &RelPath,
        read: &[u8],
        text: &[u8],
    ) -> Result<Replacement, DocsError> {
        self.replace_whole(path, read, text).await
    }

    async fn restore(
        &self,
        path: &RelPath,
        written: &[u8],
        before: &[u8],
    ) -> Result<Replacement, DocsError> {
        self.replace_whole(path, written, before).await
    }
}

/// Removes a document's file after comparing it with the bytes it is expected to hold:
/// a file that is not what was read is [`Removal::Changed`] and left as it is, and one that
/// is gone is [`Removal::Gone`]. The comparison
/// and the removal are two steps, so an edit saved in the short time between them is
/// lost with the file. The directory is put on disk by
/// [`FileSync::persist_gone`](whyshelf_core::app::shelf::FileSync::persist_gone).
impl DocRemove for MarkdownDocs {
    type Error = DocsError;

    async fn remove_read(&self, path: &RelPath, read: &[u8]) -> Result<Removal, DocsError> {
        let file = self.root.join(path.as_str());
        match tokio::fs::read(&file).await {
            Ok(now) if now == read => {}
            Ok(_) => return Ok(Removal::Changed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Removal::Gone);
            }
            Err(source) => return Err(DocsError::Io { path: file, source }),
        }
        match tokio::fs::remove_file(&file).await {
            Ok(()) => Ok(Removal::Removed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Removal::Gone),
            Err(source) => Err(DocsError::Write {
                step: format!("remove {}", file.display()),
                source,
            }),
        }
    }
}

impl MarkdownDocs {
    /// Replace the file at `path`, which must hold `expected` just before, by `text`,
    /// whole: [`replace_read`](DocReplace::replace_read), and, with the text that was
    /// written as `expected`, [`restore`](DocReplace::restore).
    async fn replace_whole(
        &self,
        path: &RelPath,
        expected: &[u8],
        text: &[u8],
    ) -> Result<Replacement, DocsError> {
        let file = self.root.join(path.as_str());
        match tokio::fs::read(&file).await {
            Ok(now) if now == expected => {}
            Ok(_) => return Ok(Replacement::Changed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Replacement::Changed);
            }
            Err(source) => {
                return Err(DocsError::Io { path: file, source });
            }
        }
        let permissions = tokio::fs::metadata(&file)
            .await
            .map_err(|source| DocsError::Io {
                path: file.clone(),
                source,
            })?
            .permissions();
        let name = path.as_str().rsplit('/').next().unwrap_or(path.as_str());
        let beside = file.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::now_v7()));
        let shown = beside.display();
        let written = async {
            let mut out = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&beside)
                .await
                .map_err(write_step(format!("create the temporary file {shown}")))?;
            out.write_all(text)
                .await
                .map_err(write_step(format!("write the temporary file {shown}")))?;
            out.set_permissions(permissions)
                .await
                .map_err(write_step(format!(
                    "give the temporary file {shown} the permissions of {}",
                    file.display()
                )))?;
            out.sync_all().await.map_err(write_step(format!(
                "write the temporary file {shown} to disk"
            )))?;
            drop(out);
            tokio::fs::rename(&beside, &file)
                .await
                .map_err(write_step(format!(
                    "rename {shown} over {}",
                    file.display()
                )))
        }
        .await;
        if let Err(error) = written {
            // The new file is not the document; whether it can be removed changes
            // nothing of the error told.
            let _ = tokio::fs::remove_file(&beside).await;
            return Err(error);
        }
        Ok(Replacement::Replaced)
    }
}
