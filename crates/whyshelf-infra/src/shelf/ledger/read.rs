//! Reading the ledger: every record file of the directory.
//!
//! The directory and each record file are read through the state of the project at
//! the side asked, every link on their paths followed as the system follows one, out
//! of the root too (`list_allowed`, `read_allowed`). A directory that is not there (a
//! broken link too) is an empty ledger. An entry is a record by its name alone (it
//! ends in `.json`), whatever its kind: a directory or nothing at it, and a link the
//! system cannot resolve (a loop) on the way to it or to the directory, stop the read
//! ([`LedgerError::Io`]), and a kind of file that is neither a file, a directory nor a
//! link (a pipe) does too, as one that cannot be told ([`LedgerError::Untold`]).

use std::ffi::OsString;
use std::io;
use std::path::Path;

use whyshelf_core::app::shelf::{
    Beyond, LedgerContents, LedgerRead, RawPart, Side, UnreadableRecord,
};
use whyshelf_core::domain::location::RelPath;

use super::super::state::paths::{
    Failure, Listed, Read, list_allowed, read_allowed, read_beyond_allowed,
};
use super::{JsonLedger, LedgerError, decode};

/// An entry of the directory that names a record: by its name in the project, or by
/// where it is when its name is no path of the project.
enum RecordEntry {
    Named(RelPath),
    Unnamed(Beyond<RelPath>),
}

/// Reading looks at the files of the directory that end in `.json` and at nothing
/// else: a file with another extension (a note, an editor's backup) is not a
/// record and is skipped on purpose. Every `.json` file is read, in the order of the
/// bytes of its name, and is a record or an [`UnreadableRecord`]. One whose name is no
/// path of the project is read as the system resolves it (`read_beyond_allowed`): a
/// record when it decodes, and when it does not, its name cannot be reported, and the
/// read fails with [`LedgerError::Name`]. None is dropped.
impl LedgerRead for JsonLedger {
    type Error = LedgerError;

    async fn read(&self, side: Side) -> Result<LedgerContents, LedgerError> {
        let mut contents = LedgerContents::default();
        let dir = self.state.path_of(side, &self.rel)?;
        let entries = match list_allowed(&self.state, side, &self.rel).await {
            Listed::Entries(entries) => entries,
            Listed::Absent => return Ok(contents),
            Listed::File => return Err(io_error(&dir, io::ErrorKind::NotADirectory.into())),
            Listed::Untold(why) => return Err(LedgerError::Untold { path: dir, why }),
            Listed::Failed(failure) => return Err(failed(&dir, failure)),
        };
        let mut files: Vec<(OsString, RecordEntry)> = Vec::new();
        for name in entries.named.keys() {
            let Some(part) = name.as_str().rsplit('/').next() else {
                continue;
            };
            if !is_record(part) {
                continue;
            }
            // The entries are named under the directory the links led to; a record is
            // named under the directory of the ledger.
            let entry = format!("{}/{part}", self.rel)
                .parse::<RelPath>()
                .map_or_else(
                    |_| {
                        RecordEntry::Unnamed(Beyond {
                            dir: Some(self.rel.clone()),
                            parts: vec![RawPart::new(OsString::from(part))],
                        })
                    },
                    RecordEntry::Named,
                );
            files.push((OsString::from(part), entry));
        }
        for (text, unnamed) in &entries.unnamed {
            if !is_record(text) {
                continue;
            }
            let Some(last) = unnamed.at.parts.last() else {
                continue;
            };
            files.push((
                last.as_os_str().to_owned(),
                RecordEntry::Unnamed(unnamed.at.clone()),
            ));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        for (raw, entry) in files {
            let (path, read) = match &entry {
                RecordEntry::Named(file) => (
                    self.state.path_of(side, file)?,
                    read_allowed(&self.state, side, file).await,
                ),
                RecordEntry::Unnamed(at) => (
                    self.state.beyond_path(side, at)?,
                    read_beyond_allowed(&self.state, side, at).await,
                ),
            };
            let bytes = match read {
                Read::Bytes(bytes) => bytes,
                Read::Absent => return Err(io_error(&path, io::ErrorKind::NotFound.into())),
                Read::Dir => return Err(io_error(&path, io::ErrorKind::IsADirectory.into())),
                Read::Untold(why) => return Err(LedgerError::Untold { path, why }),
                Read::Failed(failure) => return Err(failed(&path, failure)),
            };
            match (decode(&bytes), entry) {
                (Ok(record), _) => contents.records.push(record),
                (Err(why), RecordEntry::Named(file)) => {
                    contents.unreadable.push(UnreadableRecord { file, why });
                }
                (Err(_), RecordEntry::Unnamed(_)) => {
                    // Reported as the system names it: the error of the grammar when
                    // the name is text, none when it is not.
                    let source = raw
                        .to_str()
                        .and_then(|text| format!("{}/{text}", self.rel).parse::<RelPath>().err());
                    return Err(LedgerError::Name { path, source });
                }
            }
        }
        Ok(contents)
    }
}

/// Whether an entry of the directory names a record: its extension is `json`.
fn is_record(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|e| e == "json")
}

fn io_error(path: &Path, source: io::Error) -> LedgerError {
    LedgerError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A reading of `path` that failed, as the ledger tells it.
fn failed(path: &Path, failure: Failure) -> LedgerError {
    match failure {
        Failure::Io(source) => io_error(path, source),
        Failure::State(error) => LedgerError::State(error),
    }
}
