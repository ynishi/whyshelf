//! The ledger: one record per file, `.whyshelf/ledger/<id>.json`.
//!
//! One file per record, named by the record's own identity, so that two branches
//! that each judge add two files and merge without a conflict. A record is never
//! changed: a write creates a new file and fails when the name is taken, and a
//! decision or a declaration is withdrawn by a revocation, which is a record of its
//! own.
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
use whyshelf_core::domain::error::ValueError;
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
    /// A file of the ledger has a name that is not a path of the project (not
    /// UTF-8, or a name a relative path refuses), so it cannot be reported by name
    /// as a file that is no record. The read stops and says so, rather than skip a
    /// file that may hold a decision.
    #[error("{} has a name that is not a relative path", .path.display())]
    Name {
        /// The path.
        path: PathBuf,
        /// Why the domain refused it, when it was the domain.
        #[source]
        source: Option<ValueError>,
    },
    /// A record's canonical form has no JSON form (a bug of this program).
    #[error("a record could not be written as JSON")]
    Json(#[source] serde_json::Error),
}

/// Why a JSON value has no canonical form.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NotCanon {
    /// A number that is not a whole number of 64 bits.
    #[error("{number} is not a whole number of 64 bits")]
    Float {
        /// The number as the JSON wrote it.
        number: serde_json::Number,
    },
}

/// The ledger in a directory of JSON files.
#[derive(Debug, Clone)]
pub struct JsonLedger {
    dir: PathBuf,
    /// The directory as the project names it (`.whyshelf/ledger`), as text: the
    /// path of every file that is no record is made from it, and it is checked as a
    /// [`RelPath`] when the ledger is read, so a bad one is an error of the read.
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
        let rel: RelPath = self.rel.parse().map_err(|e| LedgerError::Name {
            path: self.dir.clone(),
            source: Some(e),
        })?;
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
    let canon = from_json(value).map_err(|NotCanon::Float { number }| UnreadableWhy::NotWhole {
        number: number.to_string(),
    })?;
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
pub fn from_json(value: Value) -> Result<Canon, NotCanon> {
    Ok(match value {
        Value::Null => Canon::Null,
        Value::Bool(b) => Canon::Bool(b),
        Value::Number(n) => Canon::Int(n.as_i64().ok_or(NotCanon::Float { number: n })?),
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
        // The text is written by hand with the keys in the reverse order: a JSON map
        // would sort them back when it is written.
        let pairs: Vec<String> = map
            .iter()
            .rev()
            .map(|(key, value)| {
                format!(
                    "{}:{}",
                    serde_json::to_string(key).unwrap(),
                    serde_json::to_string(value).unwrap()
                )
            })
            .collect();
        let reversed = format!("{{{}}}", pairs.join(","));
        let sorted = serde_json::to_string(&Value::Object(map)).unwrap();
        assert_ne!(reversed, sorted, "the keys are in another order");
        let back = decode(reversed.as_bytes()).unwrap();
        assert_eq!(back, record);
        assert_eq!(back.hash(), record.hash());
    }

    #[test]
    fn two_spellings_of_one_text_give_one_record_and_one_hash() {
        // `é` and `é`, `\/` and `/`, are one text in JSON: a record written with
        // either reads back as the same record, with the same hash.
        let record = Record::Revocation(Revocation::new(
            jid(4),
            NonEmptySet::from_set(BTreeSet::from([jid(1)])).unwrap(),
            Actor::new("ana").unwrap(),
            JudgedAt::new(1_700_000_000_000),
            NonBlank::new("caf\u{e9} a/b").unwrap(),
        ));
        let plain = serde_json::to_string(&to_json(&record.to_canon())).unwrap();
        assert!(plain.contains("café a/b"), "{plain}");
        let escaped = plain.replace("café a/b", "caf\\u00e9 a\\/b");
        assert_ne!(plain, escaped);
        let back = decode(escaped.as_bytes()).unwrap();
        assert_eq!(back, record);
        assert_eq!(back.hash(), record.hash());
    }

    #[test]
    fn a_reason_with_quotes_backslashes_lines_and_accents_reads_back_the_same() {
        let record = Record::Revocation(Revocation::new(
            jid(3),
            NonEmptySet::from_set(BTreeSet::from([jid(1)])).unwrap(),
            Actor::new("ana").unwrap(),
            JudgedAt::new(1_700_000_000_000),
            NonBlank::new("a \"quoted\" word, a \\ and a / ,\nsecond line\twith é and 日本")
                .unwrap(),
        ));
        let text = serde_json::to_string(&to_json(&record.to_canon())).unwrap();
        let back = decode(text.as_bytes()).unwrap();
        assert_eq!(back, record);
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
        assert_eq!(
            read.unreadable[0].why,
            UnreadableWhy::NotWhole {
                number: "1.5".to_owned()
            }
        );
        assert!(matches!(
            read.unreadable[1].why,
            UnreadableWhy::NotARecord(_)
        ));
    }

    /// An approval as a file of the first version holds it: no `artifacts`, no
    /// `seen_address`, no `revision`, a pin with no `address`.
    const FIRST_VERSION_APPROVAL: &str = r#"{
  "v": 1,
  "kind": "decision",
  "verb": "approve",
  "id": "00000000-0000-7000-8000-000000000003",
  "subject": "00000000-0000-7000-8000-000000000002",
  "actor": "ana",
  "at": 1700000000000,
  "reason": null,
  "seen": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
  "frame": [
    {
      "link": "grounds",
      "target": "00000000-0000-7000-8000-000000000001",
      "key": {
        "book": "finding",
        "form": { "kind": "prefixed", "prefix": "F", "tail": { "kind": "number", "value": "1" } }
      },
      "hash": "sha256:1111111111111111111111111111111111111111111111111111111111111111"
    }
  ],
  "expires": null
}"#;

    #[test]
    fn a_record_of_the_first_version_reads_back_unchanged() {
        let record = decode(FIRST_VERSION_APPROVAL.as_bytes()).unwrap();
        let original: Value = serde_json::from_str(FIRST_VERSION_APPROVAL).unwrap();
        assert_eq!(to_json(&record.to_canon()), original);
        assert_eq!(
            record.hash(),
            Record::from_canon(&from_json(original).unwrap())
                .unwrap()
                .hash()
        );
    }

    #[test]
    fn a_float_has_no_canonical_form() {
        let value: Value = serde_json::from_str("[1.5]").unwrap();
        assert!(matches!(from_json(value), Err(NotCanon::Float { .. })));
    }

    #[tokio::test]
    async fn only_json_files_are_looked_at_and_a_syntax_error_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".whyshelf/ledger");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.json"), "{").await.unwrap();
        tokio::fs::write(dir.join("notes.txt"), "{").await.unwrap();
        let read = JsonLedger::new(dir, ".whyshelf").read().await.unwrap();
        assert_eq!(read.unreadable.len(), 1);
        assert!(matches!(
            read.unreadable[0].why,
            UnreadableWhy::NotJson { .. }
        ));
    }

    // Linux file systems take any bytes in a name; APFS (macOS) refuses a name that
    // is not UTF-8, so the file cannot be made there.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_file_whose_name_is_not_utf8_is_not_dropped() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".whyshelf/ledger");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let name = OsStr::from_bytes(b"\xff\xfe.json");
        tokio::fs::write(dir.join(name), "{}").await.unwrap();
        let result = JsonLedger::new(dir, ".whyshelf").read().await;
        assert!(matches!(result, Err(LedgerError::Name { .. })));
    }

    fn revocation_of(id: u128, target: u128) -> Record {
        Record::Revocation(Revocation::new(
            jid(id),
            NonEmptySet::from_set(BTreeSet::from([jid(target)])).unwrap(),
            Actor::new("ana").unwrap(),
            JudgedAt::new(1_700_000_000_000),
            NonBlank::new("wrong file").unwrap(),
        ))
    }

    /// The names of the files of a directory, sorted.
    async fn names(dir: &Path) -> Vec<String> {
        let mut names = Vec::new();
        let mut entries = tokio::fs::read_dir(dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name().into_string().unwrap());
        }
        names.sort();
        names
    }

    #[tokio::test]
    async fn two_branches_add_two_files_and_the_same_record_is_the_same_file() {
        // Two worktrees, each with its own ledger directory. Each writes a record of
        // its own, and both write one record that is the same.
        let root = tempfile::tempdir().unwrap();
        let one = JsonLedger::new(root.path().join("one/.whyshelf/ledger"), ".whyshelf");
        let other = JsonLedger::new(root.path().join("other/.whyshelf/ledger"), ".whyshelf");
        let (own, theirs, same) = (
            revocation_of(2, 1),
            revocation_of(3, 1),
            revocation_of(4, 1),
        );
        one.append(&own).await.unwrap();
        one.append(&same).await.unwrap();
        other.append(&theirs).await.unwrap();
        other.append(&same).await.unwrap();

        // Each file is named by the id of its record, so the records of the two
        // branches are two files, and the same record is one name with one content.
        let one_names = names(one.dir()).await;
        let other_names = names(other.dir()).await;
        assert_eq!(
            one_names,
            vec![format!("{}.json", jid(2)), format!("{}.json", jid(4))]
        );
        assert_eq!(
            other_names,
            vec![format!("{}.json", jid(3)), format!("{}.json", jid(4))]
        );
        let shared = format!("{}.json", jid(4));
        assert_eq!(
            tokio::fs::read(one.dir().join(&shared)).await.unwrap(),
            tokio::fs::read(other.dir().join(&shared)).await.unwrap()
        );

        // The merge is the union of the files.
        let merged = JsonLedger::new(root.path().join("merged/.whyshelf/ledger"), ".whyshelf");
        tokio::fs::create_dir_all(merged.dir()).await.unwrap();
        for ledger in [&one, &other] {
            for name in names(ledger.dir()).await {
                tokio::fs::copy(ledger.dir().join(&name), merged.dir().join(&name))
                    .await
                    .unwrap();
            }
        }
        let read = merged.read().await.unwrap();
        assert!(read.unreadable.is_empty());
        assert_eq!(read.records.len(), 3);
        for record in [&own, &theirs, &same] {
            assert!(read.records.contains(record), "{record:?}");
        }
    }

    /// Every record of the repository's own ledger, written by every version so far
    /// (before comparisons, before spaces), is written back exactly as it was stored:
    /// its canonical form is the stored one, so its hash is the hash of what is
    /// stored, and the file is named by its identity. Read-only.
    #[tokio::test]
    async fn every_record_of_the_repository_s_ledger_is_written_back_as_stored() {
        use whyshelf_core::domain::hash::{ContentHash, HashKind};

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.whyshelf/ledger");
        let mut entries = tokio::fs::read_dir(&dir).await.unwrap();
        let mut seen = 0;
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let bytes = tokio::fs::read(&path).await.unwrap();
            let stored = from_json(serde_json::from_slice::<Value>(&bytes).unwrap())
                .unwrap_or_else(|e| {
                    panic!("{}: {e:?}", path.display());
                });
            let record = decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(record.to_canon(), stored, "{}", path.display());
            assert_eq!(
                *record.hash().hash(),
                ContentHash::of(HashKind::ShelfJudgement, &stored),
                "{}",
                path.display()
            );
            assert_eq!(
                path.file_stem().and_then(|s| s.to_str()),
                Some(record.id().to_string().as_str()),
                "{}",
                path.display()
            );
            seen += 1;
        }
        assert!(seen > 0, "the repository has records in {}", dir.display());
    }
}
