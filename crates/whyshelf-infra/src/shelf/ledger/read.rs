//! Reading the ledger: every record file of the directory.

use std::io;

use whyshelf_core::app::shelf::{LedgerContents, LedgerRead, UnreadableRecord};
use whyshelf_core::domain::location::RelPath;

use super::{JsonLedger, LedgerError, decode};

/// Reading looks at the files of the directory that end in `.json` and at nothing
/// else: a file with another extension (a note, an editor's backup) is not a
/// record and is skipped on purpose. Every `.json` file is a record or an
/// [`UnreadableRecord`]; one whose name cannot be reported fails the read with
/// [`LedgerError::Name`]. None is dropped.
impl LedgerRead for JsonLedger {
    type Error = LedgerError;

    async fn read(&self) -> Result<LedgerContents, LedgerError> {
        let mut contents = LedgerContents::default();
        let mut entries = match tokio::fs::read_dir(&self.dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(contents),
            Err(source) => {
                return Err(LedgerError::Io {
                    path: self.dir.clone(),
                    source,
                });
            }
        };
        let rel = &self.rel;
        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(self.io(&self.dir))? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                files.push(path);
            }
        }
        files.sort();
        for path in files {
            let bytes = tokio::fs::read(&path).await.map_err(self.io(&path))?;
            match decode(&bytes) {
                Ok(record) => contents.records.push(record),
                Err(why) => {
                    let name = |source| LedgerError::Name {
                        path: path.clone(),
                        source,
                    };
                    let segment = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| name(None))?;
                    let file = format!("{rel}/{segment}")
                        .parse::<RelPath>()
                        .map_err(|e| name(Some(e)))?;
                    contents.unreadable.push(UnreadableRecord { file, why });
                }
            }
        }
        Ok(contents)
    }
}
