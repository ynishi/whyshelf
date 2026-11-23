//! Reading the documents: the files the ranges of the books take, a document's file
//! now, and where the content of each is (or would be) kept.
//!
//! Every reading is of the state of the project at the side asked. The directory of a
//! range, and the file of a document, are read with every link on their path followed
//! as the system follows one, out of the root too (`read_allowed`, `list_allowed`: a
//! broken link is nothing there, a loop the system's error); an entry of a
//! range is taken by its own kind, so an entry that is a link (to a file or a
//! directory), and one of another kind (a pipe), are passed over. A directory of a
//! range of another kind than a directory or a link, and a document's file that is,
//! cannot be told ([`DocsError::Untold`]).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use whyshelf_core::app::shelf::{
    Beyond, DocSource, EntryKind, FileRead, Rendered, Replaceable, Side, StateRead,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::ContentAddress;
use whyshelf_core::domain::shelf::build::{DocReading, FormEntries};
use whyshelf_core::domain::shelf::schema::BookRange;

use super::super::state::StateError;
use super::super::state::paths::{
    Failure, Listed, Read, list_allowed, list_beyond_allowed, read_allowed,
};
use super::write::replaceable;
use super::{DocsError, MarkdownDocs, file_read, not_text, render};

impl DocSource for MarkdownDocs {
    type Error = DocsError;

    async fn read_docs(
        &self,
        side: Side,
        ranges: &[BookRange],
    ) -> Result<Vec<DocReading>, DocsError> {
        Ok(self
            .files_of(side, ranges)
            .await?
            .into_iter()
            .map(|(origin, bytes)| match std::str::from_utf8(&bytes) {
                Ok(text) => super::read(origin, text),
                Err(_) => not_text(origin),
            })
            .collect())
    }

    async fn forms(&self, side: Side, ranges: &[BookRange]) -> Result<Vec<FileRead>, DocsError> {
        Ok(self
            .files_of(side, ranges)
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

    /// Nothing there, a directory there, and a path through a file is no file now.
    async fn current(&self, side: Side, file: &RelPath) -> Result<Option<Vec<u8>>, DocsError> {
        let path = self.state.path_of(side, file)?;
        match read_allowed(&self.state, side, file).await {
            Read::Bytes(bytes) => Ok(Some(bytes)),
            Read::Absent | Read::Dir => Ok(None),
            Read::Untold(why) => Err(DocsError::Untold { path, why }),
            Read::Failed(Failure::Io(source))
                if matches!(
                    source.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::NotADirectory
                        | io::ErrorKind::IsADirectory
                ) =>
            {
                Ok(None)
            }
            Read::Failed(failure) => Err(failed(&path, failure)),
        }
    }

    async fn addresses(
        &self,
        side: Side,
        files: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, DocsError> {
        let mut there = BTreeSet::new();
        for file in files {
            if self.current(side, file).await?.is_some() {
                there.insert(file.clone());
            }
        }
        if there.is_empty() {
            return Ok(BTreeMap::new());
        }
        self.state
            .addresses(side, &there)
            .await
            .map_err(state_error)
    }
}

impl MarkdownDocs {
    /// Every `.md` file the ranges take at `side`, with its bytes, in the order of the
    /// books and, under each, a stable order. Whether a file is in a range is asked of
    /// the range and of nothing else here ([`range_files`](Self::range_files)); a file
    /// out of range is not read.
    async fn files_of(
        &self,
        side: Side,
        ranges: &[BookRange],
    ) -> Result<Vec<(RelPath, Vec<u8>)>, DocsError> {
        let mut files = Vec::new();
        for range in ranges {
            for origin in self.range_files(side, range).await? {
                let bytes = self.file_bytes(side, &origin).await?;
                files.push((origin, bytes));
            }
        }
        Ok(files)
    }

    /// The bytes of a file a range takes, every link on its path followed.
    async fn file_bytes(&self, side: Side, origin: &RelPath) -> Result<Vec<u8>, DocsError> {
        let path = self.state.path_of(side, origin)?;
        match read_allowed(&self.state, side, origin).await {
            Read::Bytes(bytes) => Ok(bytes),
            Read::Absent => Err(io_error(&path, io::ErrorKind::NotFound.into())),
            Read::Dir => Err(io_error(&path, io::ErrorKind::IsADirectory.into())),
            Read::Untold(why) => Err(DocsError::Untold { path, why }),
            Read::Failed(failure) => Err(failed(&path, failure)),
        }
    }

    /// Every file under the directory of `range` that the range takes at `side`, in a
    /// stable order (by the parts of its path). A directory that is not there has none.
    ///
    /// The range is asked before anything else is done with a name: a directory it
    /// takes nothing under (an `exclude` of `<dir>/**`) is not opened, and a file it
    /// does not take is not read, so a name that is no path, or a directory that cannot
    /// be read, out of the range is no error. In the range, both are. A name is asked of
    /// the range as its text; one that is no path of the project is asked with its bad
    /// bytes replaced. A directory of such a name that the range does not leave out is
    /// walked as the system resolves it (`list_beyond_allowed`), and when the range
    /// takes a file whose name is no path, the read fails ([`DocsError::Name`], the
    /// first such file by its path) once the walk is done.
    async fn range_files(&self, side: Side, range: &BookRange) -> Result<Vec<RelPath>, DocsError> {
        let mut found = Vec::new();
        let mut unnamed_files = Vec::new();
        let mut pending = vec![(Place::Named(range.dir().as_rel().clone()), String::new())];
        while let Some((place, below)) = pending.pop() {
            let (path, listed) = match &place {
                Place::Named(dir) => (
                    self.state.path_of(side, dir)?,
                    list_allowed(&self.state, side, dir).await,
                ),
                Place::Beyond(at) => (
                    self.state.beyond_path(side, at)?,
                    list_beyond_allowed(&self.state, side, at).await,
                ),
            };
            let entries = match listed {
                Listed::Entries(entries) => entries,
                Listed::Absent => continue,
                Listed::File => {
                    return Err(io_error(&path, io::ErrorKind::NotADirectory.into()));
                }
                Listed::Untold(why) => return Err(DocsError::Untold { path, why }),
                Listed::Failed(failure) => return Err(failed(&path, failure)),
            };
            let at = |part: &str| {
                if below.is_empty() {
                    part.to_owned()
                } else {
                    format!("{below}/{part}")
                }
            };
            // The entries are named under the directory the links led to; a file
            // of the range is named under the directory asked.
            let asked = match &place {
                Place::Named(dir) => Some(dir),
                Place::Beyond(_) => None,
            };
            for (name, kind) in &entries.named {
                let (Some(dir), Some(part)) = (asked, name.as_str().rsplit('/').next()) else {
                    continue;
                };
                let here = at(part);
                let child = || {
                    format!("{dir}/{part}")
                        .parse::<RelPath>()
                        .map_err(|e| DocsError::Name {
                            path: path.join(part),
                            source: Some(e),
                        })
                };
                match kind {
                    EntryKind::Dir if !range.leaves_out_dir(&here) => {
                        pending.push((Place::Named(child()?), here));
                    }
                    EntryKind::File if range.takes_below(&here) => found.push(child()?),
                    EntryKind::Dir | EntryKind::File | EntryKind::Link | EntryKind::Other => {}
                }
            }
            for (text, unnamed) in &entries.unnamed {
                let here = at(text);
                match unnamed.kind {
                    EntryKind::Dir if !range.leaves_out_dir(&here) => {
                        pending.push((Place::Beyond(unnamed.at.clone()), here));
                    }
                    EntryKind::File if range.takes_below(&here) => {
                        unnamed_files.push(self.state.beyond_path(side, &unnamed.at)?);
                    }
                    EntryKind::Dir | EntryKind::File | EntryKind::Link | EntryKind::Other => {}
                }
            }
        }
        if let Some(path) = unnamed_files.into_iter().min() {
            return Err(DocsError::Name { path, source: None });
        }
        found.sort_by(|a, b| a.as_str().split('/').cmp(b.as_str().split('/')));
        Ok(found)
    }
}

/// A directory a range is walked through: a name of the project, or a place below one
/// whose name is no path.
enum Place {
    Named(RelPath),
    Beyond(Beyond<RelPath>),
}

fn io_error(path: &Path, source: io::Error) -> DocsError {
    DocsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A reading of `path` that failed, as the documents tell it.
fn failed(path: &Path, failure: Failure) -> DocsError {
    match failure {
        Failure::Io(source) => io_error(path, source),
        Failure::State(error) => state_error(error),
    }
}

/// What else stops the state, as the documents tell it: Git asked through it is Git.
fn state_error(error: StateError) -> DocsError {
    match error {
        StateError::Git(git) => DocsError::Git(git),
        other => DocsError::State(other),
    }
}
