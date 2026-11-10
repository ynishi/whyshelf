//! The ledger: one record per file, `.whyshelf/ledger/<id>.json`.
//!
//! One file per record, named by the record's own identity, so that two branches
//! that each judge add two files and merge without a conflict. A record is never
//! changed: a write creates a new file and fails when the name is taken, and a
//! decision is withdrawn by a revocation, which is a record of its own.
//!
//! JSON is the wire form and nothing more. A record is written from its canonical
//! form ([`Record::to_canon`]) and read back into one ([`Record::from_canon`]); the
//! hash of a record is the hash of that form, so the order of the keys, the spaces
//! and the escapes of a file change nothing.
//!
//! ```text
//!   Record --to_canon--> Canon --json--> <id>.json
//!   <id>.json --json--> Canon --from_canon--> Record   (or an UnreadableRecord)
//! ```

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use whyshelf_core::app::shelf::{
    LedgerAppend, LedgerContents, LedgerRead, UnreadableRecord, UnreadableWhy,
};
use whyshelf_core::domain::canon::Canon;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::ledger::Record;

/// Why the ledger could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// The directory or a file could not be used.
    #[error("cannot use {}", .path.display())]
    Io {
        /// The path.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// A record with this identity is already there. Records are never replaced.
    #[error("{} is already there: a record is never replaced", .path.display())]
    Taken {
        /// The file.
        path: PathBuf,
    },
    /// A record's canonical form has no JSON form (a bug of this program).
    #[error("a record could not be written as JSON")]
    Json(#[source] serde_json::Error),
}

/// The ledger in a directory of JSON files.
#[derive(Debug, Clone)]
pub struct JsonLedger {
    dir: PathBuf,
    /// The directory as the project names it, for the paths of the files that are no
    /// record (`.whyshelf/ledger`).
    rel: String,
}

impl JsonLedger {
    /// The ledger in `dir`, which the project names `<whyshelf_dir>/ledger`.
    pub fn new(dir: PathBuf, whyshelf_dir: &str) -> Self {
        Self {
            dir,
            rel: format!("{whyshelf_dir}/ledger"),
        }
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn io(&self, path: &Path) -> impl FnOnce(io::Error) -> LedgerError {
        let path = path.to_path_buf();
        move |source| LedgerError::Io { path, source }
    }
}

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
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    let file = format!("{}/{name}", self.rel).parse::<RelPath>();
                    if let Ok(file) = file {
                        contents.unreadable.push(UnreadableRecord { file, why });
                    }
                }
            }
        }
        Ok(contents)
    }
}

impl LedgerAppend for JsonLedger {
    type Error = LedgerError;

    async fn append(&self, record: &Record) -> Result<(), LedgerError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(self.io(&self.dir))?;
        let path = self.dir.join(format!("{}.json", record.id()));
        let mut text = serde_json::to_string_pretty(&to_json(&record.to_canon()))
            .map_err(LedgerError::Json)?;
        text.push('\n');
        let mut file = match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(LedgerError::Taken { path });
            }
            Err(source) => return Err(LedgerError::Io { path, source }),
        };
        file.write_all(text.as_bytes())
            .await
            .map_err(self.io(&path))?;
        file.sync_all().await.map_err(self.io(&path))
    }
}

/// The record a file's bytes hold.
pub fn decode(bytes: &[u8]) -> Result<Record, UnreadableWhy> {
    let text = std::str::from_utf8(bytes).map_err(|_| UnreadableWhy::NotText)?;
    let value: Value = serde_json::from_str(text).map_err(|e| UnreadableWhy::NotJson {
        detail: e.to_string(),
    })?;
    let canon = from_json(value).map_err(|detail| UnreadableWhy::NotJson { detail })?;
    Record::from_canon(&canon).map_err(UnreadableWhy::NotARecord)
}

/// The JSON form of a canonical value.
pub fn to_json(canon: &Canon) -> Value {
    match canon {
        Canon::Null => Value::Null,
        Canon::Bool(b) => Value::Bool(*b),
        Canon::Int(n) => Value::from(*n),
        Canon::Str(s) => Value::String(s.clone()),
        Canon::List(items) => Value::Array(items.iter().map(to_json).collect()),
        Canon::Map(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), to_json(v))).collect())
        }
    }
}

/// The canonical value a JSON value is the form of. A number that is not a whole
/// number of 64 bits has none.
pub fn from_json(value: Value) -> Result<Canon, String> {
    Ok(match value {
        Value::Null => Canon::Null,
        Value::Bool(b) => Canon::Bool(b),
        Value::Number(n) => Canon::Int(
            n.as_i64()
                .ok_or_else(|| format!("{n} is not a whole number of 64 bits"))?,
        ),
        Value::String(s) => Canon::Str(s),
        Value::Array(items) => {
            Canon::List(items.into_iter().map(from_json).collect::<Result<_, _>>()?)
        }
        Value::Object(map) => Canon::Map(
            map.into_iter()
                .map(|(k, v)| from_json(v).map(|v| (k, v)))
                .collect::<Result<BTreeMap<_, _>, _>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use uuid::Uuid;
    use whyshelf_core::domain::actor::Actor;
    use whyshelf_core::domain::id::EntityId;
    use whyshelf_core::domain::nonempty::{NonBlank, NonEmptySet};
    use whyshelf_core::domain::shelf::ledger::{JudgedAt, JudgementId, Revocation};

    use super::*;

    fn jid(n: u128) -> JudgementId {
        let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
        JudgementId::new(EntityId::new(Uuid::from_u128(bits)).unwrap())
    }

    fn revocation() -> Record {
        Record::Revocation(Revocation::new(
            jid(2),
            NonEmptySet::from_set(BTreeSet::from([jid(1)])).unwrap(),
            Actor::new("ana").unwrap(),
            JudgedAt::new(1_700_000_000_000),
            NonBlank::new("wrong file").unwrap(),
        ))
    }

    #[tokio::test]
    async fn a_record_is_written_once_and_read_back_the_same() {
        let root = tempfile::tempdir().unwrap();
        let ledger = JsonLedger::new(root.path().join(".whyshelf/ledger"), ".whyshelf");
        assert!(ledger.read().await.unwrap().records.is_empty());
        assert!(!ledger.dir().exists(), "a read creates nothing");

        let record = revocation();
        ledger.append(&record).await.unwrap();
        assert!(matches!(
            ledger.append(&record).await,
            Err(LedgerError::Taken { .. })
        ));
        let read = ledger.read().await.unwrap();
        assert_eq!(read.records, vec![record]);
        assert!(read.unreadable.is_empty());
    }

    #[test]
    fn the_hash_does_not_depend_on_the_order_of_the_keys() {
        let record = revocation();
        let Value::Object(map) = to_json(&record.to_canon()) else {
            panic!("an object");
        };
        let reversed: serde_json::Map<String, Value> = map.into_iter().rev().collect();
        let text = serde_json::to_string(&Value::Object(reversed)).unwrap();
        let back = decode(text.as_bytes()).unwrap();
        assert_eq!(back.hash(), record.hash());
    }

    #[tokio::test]
    async fn a_file_that_is_no_record_is_reported_and_not_dropped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".whyshelf/ledger");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("x.json"), "{\"v\": 1.5}")
            .await
            .unwrap();
        tokio::fs::write(dir.join("y.json"), "{}").await.unwrap();
        let read = JsonLedger::new(dir, ".whyshelf").read().await.unwrap();
        assert!(read.records.is_empty());
        assert_eq!(read.unreadable.len(), 2);
        assert_eq!(read.unreadable[0].file.as_str(), ".whyshelf/ledger/x.json");
        assert!(matches!(
            read.unreadable[0].why,
            UnreadableWhy::NotJson { .. }
        ));
        assert!(matches!(
            read.unreadable[1].why,
            UnreadableWhy::NotARecord(_)
        ));
    }
}
