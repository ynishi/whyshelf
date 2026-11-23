//! Reading the documents: the files the ranges of the books take, a document's file
//! now, and where the content of each is (or would be) kept.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use whyshelf_core::app::shelf::{DocSource, FileRead, Rendered, Replaceable};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::ContentAddress;
use whyshelf_core::domain::shelf::build::{DocReading, FormEntries};
use whyshelf_core::domain::shelf::schema::BookRange;

use super::write::replaceable;
use super::{DocsError, MarkdownDocs, file_read, not_text, render};

impl DocSource for MarkdownDocs {
    type Error = DocsError;

    async fn read_docs(&self, ranges: &[BookRange]) -> Result<Vec<DocReading>, DocsError> {
        Ok(self
            .files_of(ranges)
            .await?
            .into_iter()
            .map(|(origin, bytes)| match std::str::from_utf8(&bytes) {
                Ok(text) => super::read(origin, text),
                Err(_) => not_text(origin),
            })
            .collect())
    }

    async fn forms(&self, ranges: &[BookRange]) -> Result<Vec<FileRead>, DocsError> {
        Ok(self
            .files_of(ranges)
            .await?
            .into_iter()
            .map(|(origin, bytes)| file_read(origin, bytes))
            .collect())
    }

    fn render(&self, origin: &RelPath, entries: &FormEntries, body: &str) -> Rendered {
        let text = render(entries.as_map(), body);
        Rendered {
            reads_as: super::read(origin.clone(), &text),
            text: text.into_bytes(),
        }
    }

    async fn replaceable(&self, file: &RelPath) -> Result<Replaceable, DocsError> {
        replaceable(&self.root.join(file.as_str())).await
    }

    async fn current(&self, file: &RelPath) -> Result<Option<Vec<u8>>, DocsError> {
        let full = self.root.join(file.as_str());
        match tokio::fs::read(&full).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::NotADirectory
                        | io::ErrorKind::IsADirectory
                ) =>
            {
                Ok(None)
            }
            Err(source) => Err(DocsError::Io { path: full, source }),
        }
    }

    async fn addresses(
        &self,
        files: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, DocsError> {
        let mut there = Vec::new();
        for file in files {
            if self.current(file).await?.is_some() {
                there.push(file);
            }
        }
        let Some(names) = self.git.blob_names(&there).await? else {
            return Ok(BTreeMap::new());
        };
        Ok(there.into_iter().cloned().zip(names).collect())
    }
}

impl MarkdownDocs {
    /// Every `.md` file the ranges take, with its bytes, in the order of the books
    /// and, under each, a stable order. Whether a file is in a range is asked of
    /// the range and of nothing else here ([`range_files`]); a file out of range is
    /// not read.
    async fn files_of(&self, ranges: &[BookRange]) -> Result<Vec<(RelPath, Vec<u8>)>, DocsError> {
        let mut files = Vec::new();
        for range in ranges {
            for file in range_files(&self.root.join(range.dir().as_rel().as_str()), range).await? {
                let origin = relative(&self.root, &file)?;
                let bytes = tokio::fs::read(&file)
                    .await
                    .map_err(|source| DocsError::Io {
                        path: file.clone(),
                        source,
                    })?;
                files.push((origin, bytes));
            }
        }
        Ok(files)
    }
}

/// Every file under `dir`, the directory of `range`, that the range takes, in a
/// stable order. A directory that is not there has none.
///
/// The range is asked before anything else is done with a name: a directory it
/// takes nothing under (an `exclude` of `<dir>/**`) is not opened, and a file it
/// does not take is not made a path, so a name that is no path, or a directory
/// that cannot be read, out of the range is no error. In the range, both are, as
/// they were. A name is asked of the range as its text; one that is not UTF-8 is
/// asked with its bad bytes replaced, and when the range takes that, it fails
/// where every name that is no path fails ([`relative`]).
async fn range_files(dir: &Path, range: &BookRange) -> Result<Vec<PathBuf>, DocsError> {
    let mut found = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((next, below)) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(&next).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(DocsError::Io { path: next, source }),
        };
        let io = |source| DocsError::Io {
            path: next.clone(),
            source,
        };
        while let Some(entry) = entries.next_entry().await.map_err(io)? {
            let kind = entry.file_type().await.map_err(io)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let at = if below.is_empty() {
                name.into_owned()
            } else {
                format!("{below}/{name}")
            };
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            if kind.is_dir() {
                if !range.leaves_out_dir(&at) {
                    pending.push((entry.path(), at));
                }
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            } else if kind.is_file() && range.takes_below(&at) {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The path of `file` relative to `root`, with `/` between segments.
fn relative(root: &Path, file: &Path) -> Result<RelPath, DocsError> {
    let name = |source| DocsError::Name {
        path: file.to_path_buf(),
        source,
    };
    let rel = file.strip_prefix(root).map_err(|_| name(None))?;
    let mut segments = Vec::new();
    for part in rel.components() {
        segments.push(part.as_os_str().to_str().ok_or_else(|| name(None))?);
    }
    segments.join("/").parse().map_err(|e| name(Some(e)))
}
