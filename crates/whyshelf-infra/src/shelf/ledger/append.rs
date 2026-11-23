//! Adding a record to the ledger: a new file of its own.

use whyshelf_core::app::shelf::{Creation, LedgerAppend};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::ledger::{AdmittedRecord, Record};

use super::super::create;
use super::{JsonLedger, LedgerError, to_json};

impl LedgerAppend for JsonLedger {
    type Error = LedgerError;

    /// `<rel>/<id>.json`. The identity of a record is hexadecimal digits and `-`, so
    /// a [`RelPath`] always takes it; were it ever not, the answer is the directory,
    /// which still says where the file would have been.
    fn path_of(&self, record: &Record) -> RelPath {
        self.file_of(record).unwrap_or_else(|_| self.rel.clone())
    }

    /// The record is written to a new file by `create_new` (with the
    /// directory made first): whole, or removed, and not put on disk, which is
    /// [`FileSync::persist`]'s, for every record of a run at once.
    ///
    /// [`FileSync::persist`]: whyshelf_core::app::shelf::FileSync::persist
    async fn append(&self, record: &AdmittedRecord) -> Result<RelPath, LedgerError> {
        let record = record.record();
        let file = self.file_of(record).map_err(|e| LedgerError::Name {
            path: self.dir.clone(),
            source: Some(e),
        })?;
        let path = self.dir.join(format!("{}.json", record.id()));
        let mut text = serde_json::to_string_pretty(&to_json(&record.to_canon()))
            .map_err(LedgerError::Json)?;
        text.push('\n');
        match create::create_new(&path, text.as_bytes()).await {
            Ok(Creation::Created) => Ok(file),
            Ok(Creation::AlreadyThere) => Err(LedgerError::Taken { path }),
            Err(failed) => Err(LedgerError::Write {
                step: failed.step,
                source: failed.source,
            }),
        }
    }
}
