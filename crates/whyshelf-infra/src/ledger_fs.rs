//! The two ledgers and their objects on disk: [`LedgerRead`], [`LedgerAppend`],
//! [`ObjectRead`] and [`ObjectPut`] over files, in layout 3.
//!
//! A *side* is a directory of the same shape, the public one in the project
//! (`<project>/.whyshelf/`, committed with it) and the private one outside it:
//!
//! ```text
//! layout                  the layout number of the side (`3`), written before the first record
//! ledger/<id>             one judgement per file, named by its JudgementId; the canonical bytes
//! ledger/<id>.<digest>    a record of the same id with other bytes, kept beside the first
//! objects/<hex>           the normalized text of a version, named by its TextHash
//! ```
//!
//! `<id>` is the hyphenated UUID of the [`JudgementId`], `<hex>` the 64 hex digits of
//! the [`TextHash`], and `<digest>` the 64 hex digits of the [`BytesHash`] of the
//! record's bytes. No name carries a scheme prefix or a character a file system
//! refuses. Layout 2 (event files) is a different store: it is left alone, and
//! this module refuses to read or write one ([`LedgerFsError::Layout`]); nothing
//! here migrates.
//!
//! # What a record file holds
//!
//! **The canonical bytes of the judgement** ([`Judgement::canonical_bytes`](whyshelf_core::domain::judgement::Judgement::canonical_bytes)), as
//! they are, and nothing else. The alternative was a JSON form that decodes to a
//! judgement whose canonical bytes are the same; it was not taken, for these
//! reasons:
//!
//! - *Byte-equal is the rule of the ledger* (a ledger is a set of records, and two
//!   records of one id are one record exactly when their canonical bytes are
//!   equal). A file that holds those bytes makes "the same record" a comparison of
//!   two files, with no encoder in between that could disagree about a record.
//! - *A publication copies a record byte for byte.* The copy is the same file
//!   content on both sides, so it cannot be told apart by anything but the side
//!   it is on.
//! - *There is no second definition to keep in step.* A JSON form is a second wire
//!   format of every field of every kind of record, and a field added to the
//!   canonical form without it would silently make the two forms disagree.
//!
//! The price is that the file is not for reading by eye. A person who wants to
//! read the ledger asks the tool.
//!
//! # Many writers, no lock
//!
//! A record is written under its id by a rename of a file of its own, never over
//! another ([`AppendOutcome`] in the way [`FsStore`](crate::store_fs::FsStore)
//! does it). The same record again is the file that is there, and that is success.
//! A record of the same id with other bytes (two writers that made two decisions
//! under one id, a record edited by hand) is written beside the first, as
//! `<id>.<digest>`, and never in its place; the reader hands both to the domain,
//! which sets both aside as corrupt ([`Ledger::is_corrupt`](whyshelf_core::domain::ledger::Ledger::is_corrupt)). A Git merge of two
//! branches is a union of files, and the union of two ledgers is the ledger.
//!
//! # Reading
//!
//! Every file of `ledger/` whose name is `<id>` or `<id>.<digest>` is a record;
//! any other name (a temporary file of a writer, a stray file) is not looked at. A
//! record the adapter itself can tell is wrong is reported, not dropped
//! ([`CorruptRecord`]):
//!
//! | the file | is |
//! |---|---|
//! | not the canonical encoding of a value, or not a map with an `id` | [`RecordProblem::Undecodable`] |
//! | a record of another identifier than its name | [`RecordProblem::NameDisagrees`] |
//!
//! A side that is not there reads as empty, as a clone without the private ledger
//! must.
//!
//! # Objects
//!
//! The objects of a side form a set that can also delete (a 2P-set). Whether a hash
//! is purged is **read from the ledger of the same side every time**: a purge record
//! there is the whole of the fact, so a branch that merged in a purge, or an object
//! file that came back with a merge, is read as gone with no index to bring up to
//! date. [`FsObjects::put`](ObjectPut::put) answers [`Put::Purged`] for such a
//! hash and writes nothing; [`FsObjects::get`](ObjectRead::get) answers `None`.
//! Appending a purge record also deletes the object file, so that a purged text does
//! not stay on disk. A record that is itself corrupt (two files of one id, a name
//! that disagrees) purges nothing, as in the domain.
//!
//! A text is stored under the hash of its own bytes and read back checked against
//! it: a file that does not hash to its name is corrupt, not a text.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use whyshelf_core::app::commit::record::{Recordable, Scanned};
use whyshelf_core::app::ports::{
    AppendOutcome, CorruptRecord, LedgerAppend, LedgerContents, LedgerRead, ObjectPut, ObjectRead,
    Put, RecordProblem,
};
use whyshelf_core::domain::canon::Canon;
use whyshelf_core::domain::corpus::Visibility;
use whyshelf_core::domain::hash::{BytesHash, TextHash};
use whyshelf_core::domain::judgement::{JudgementId, JudgementKind};

use crate::format::Found;
use crate::store_fs::{StoreError, io_error, publish};

/// The layout number this module reads and writes, held by the `layout` file at the
/// root of a side.
///
/// Layout 2 is [`FsStore`](crate::store_fs::FsStore)'s (event files); layout 1 is the
/// one before that.
pub const LAYOUT: u32 = 3;

const LAYOUT_FILE: &str = "layout";
const LEDGER_DIR: &str = "ledger";
const OBJECTS_DIR: &str = "objects";

/// Containers nested deeper than this are not a record (a record is a few levels).
const MAX_DEPTH: usize = 32;

/// Why a ledger or an object could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum LedgerFsError {
    /// A file or directory could not be used.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The side is in a layout this build does not read.
    #[error(
        "the store at {path} is in layout {found}, and this build reads layout v{LAYOUT}; it is left as it is"
    )]
    Layout {
        /// The side.
        path: PathBuf,
        /// What it says it is in.
        found: Found,
    },
    /// A record is there and the domain gives no way to rebuild a judgement from its
    /// canonical bytes: the adapter can store and tell apart the records, and cannot
    /// hand them back as [`Judgement`](whyshelf_core::domain::judgement::Judgement)s.
    #[error("{records} record(s) in the {side:?} ledger cannot be turned back into judgements")]
    DecoderMissing {
        /// The ledger.
        side: Visibility,
        /// How many records are well-formed.
        records: usize,
    },
    /// An object file does not hash to its name, or is not text.
    #[error("the object {path} is not the text its name says")]
    CorruptObject {
        /// The file.
        path: PathBuf,
    },
}

/// The two sides on disk.
#[derive(Debug, Clone)]
pub struct StoreDirs {
    public: PathBuf,
    private: PathBuf,
}

impl StoreDirs {
    /// The public side (`<project>/.whyshelf`) and the private one. Neither has to
    /// exist yet.
    pub fn new(public: PathBuf, private: PathBuf) -> Self {
        Self { public, private }
    }

    /// The directory of a side.
    pub fn side(&self, side: Visibility) -> &Path {
        match side {
            Visibility::Public => &self.public,
            Visibility::Private => &self.private,
        }
    }
}

/// What a side's `layout` file says.
enum Mark {
    /// No mark and nothing written: a side nothing was put in.
    Empty,
    /// The mark is [`LAYOUT`].
    Current,
}

async fn has_entries(dir: &Path) -> Result<bool, LedgerFsError> {
    let mut listing = match tokio::fs::read_dir(dir).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_error(dir)(error).into()),
    };
    Ok(listing.next_entry().await.map_err(io_error(dir))?.is_some())
}

/// The one function that reads a side's mark, for every reader and writer.
///
/// - the mark holds [`LAYOUT`]: current;
/// - no mark, and no `events/` and no `ledger/` files: empty;
/// - anything else (another number, a mark that is no number, event files without a
///   mark, ledger files without one): [`LedgerFsError::Layout`], naming what was
///   found. A side is never read as if it were in this layout when it is not.
async fn mark_of(side: &Path) -> Result<Mark, LedgerFsError> {
    let layout = |found| LedgerFsError::Layout {
        path: side.to_path_buf(),
        found,
    };
    let mark = side.join(LAYOUT_FILE);
    match tokio::fs::read(&mark).await {
        Ok(bytes) => {
            let found = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
                .map_or(Found::Unknown, Found::Version);
            if found == Found::Version(LAYOUT) {
                Ok(Mark::Current)
            } else {
                Err(layout(found))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if has_entries(&side.join("events")).await? {
                Err(layout(Found::Version(1)))
            } else if has_entries(&side.join(LEDGER_DIR)).await? {
                Err(layout(Found::Unknown))
            } else {
                Ok(Mark::Empty)
            }
        }
        Err(error) => Err(io_error(&mark)(error).into()),
    }
}

/// Make sure the side can be written to: the mark goes down before the first record,
/// so a side with a record always has one. Two writers that both find the side empty
/// write the same bytes, and the second finds the first's file.
async fn prepare_for_write(side: &Path) -> Result<(), LedgerFsError> {
    if let Mark::Empty = mark_of(side).await? {
        publish(side, LAYOUT_FILE, format!("{LAYOUT}\n").as_bytes(), false).await?;
    }
    Ok(())
}

// --- the names of record files -----------------------------------------------------

/// The identifier a file of `ledger/` is named by, if the name is a record's.
fn record_name(name: &str) -> Option<JudgementId> {
    let (stem, twin) = match name.split_once('.') {
        None => (name, None),
        Some((stem, digest)) => (stem, Some(digest)),
    };
    if let Some(digest) = twin
        && !(digest.len() == 64
            && digest
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    {
        return None;
    }
    let id: JudgementId = stem.parse().ok()?;
    // Only the one spelling the identifier writes itself in.
    (id.to_string() == stem).then_some(id)
}

// --- reading a record's bytes ------------------------------------------------------

/// What one record file turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// The canonical encoding of a record whose `id` is the name's.
    Record(Canon),
    /// Not the canonical encoding of a map with an `id`.
    Undecodable,
    /// A record of another identifier than its name.
    NameDisagrees,
}

/// One file of a ledger, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    named: JudgementId,
    bytes: Vec<u8>,
    shape: Shape,
}

impl RawEntry {
    /// The identifier the file is named by.
    pub fn named(&self) -> JudgementId {
        self.named
    }

    /// The bytes of the file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// What the bytes are.
    pub fn shape(&self) -> &Shape {
        &self.shape
    }
}

fn shape_of(named: JudgementId, bytes: &[u8]) -> Shape {
    let Some(Canon::Map(map)) = decode_canon(bytes) else {
        return Shape::Undecodable;
    };
    let Some(Canon::Str(id)) = map.get("id") else {
        return Shape::Undecodable;
    };
    match id.parse::<JudgementId>() {
        Ok(found) if found == named => Shape::Record(Canon::Map(map)),
        Ok(_) => Shape::NameDisagrees,
        Err(_) => Shape::Undecodable,
    }
}

/// The files of one ledger, as read: the records, with what the adapter could tell
/// of each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawLedger {
    entries: Vec<RawEntry>,
}

impl RawLedger {
    /// Every record file, in the order of their names.
    pub fn entries(&self) -> &[RawEntry] {
        &self.entries
    }

    /// The records the adapter can tell are wrong, once each.
    pub fn corrupt(&self) -> Vec<CorruptRecord> {
        let found: BTreeSet<(JudgementId, RecordProblem)> = self
            .entries
            .iter()
            .filter_map(|entry| match entry.shape {
                Shape::Record(_) => None,
                Shape::Undecodable => Some((entry.named, RecordProblem::Undecodable)),
                Shape::NameDisagrees => Some((entry.named, RecordProblem::NameDisagrees)),
            })
            .collect();
        found
            .into_iter()
            .map(|(id, problem)| CorruptRecord::new(id, problem))
            .collect()
    }
}

/// Read the files of `ledger/` of one side, whatever they hold.
async fn read_raw(side: &Path) -> Result<RawLedger, LedgerFsError> {
    if let Mark::Empty = mark_of(side).await? {
        return Ok(RawLedger::default());
    }
    let dir = side.join(LEDGER_DIR);
    let mut listing = match tokio::fs::read_dir(&dir).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(RawLedger::default()),
        Err(error) => return Err(io_error(&dir)(error).into()),
    };
    let mut named: Vec<(String, JudgementId, PathBuf)> = Vec::new();
    while let Some(entry) = listing.next_entry().await.map_err(io_error(&dir))? {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if let Some(id) = record_name(name) {
            named.push((name.to_owned(), id, entry.path()));
        }
    }
    named.sort();
    let mut entries = Vec::with_capacity(named.len());
    for (_, id, path) in named {
        let bytes = tokio::fs::read(&path).await.map_err(io_error(&path))?;
        let shape = shape_of(id, &bytes);
        entries.push(RawEntry {
            named: id,
            bytes,
            shape,
        });
    }
    Ok(RawLedger { entries })
}

/// The text hashes the purge records of one side name, as written in a record
/// (`sha256:<hex>`).
///
/// Only a record that stands counts: one that is the only file of its identifier
/// and agrees with its name. Two files of one identifier that differ are both set
/// aside by the domain, so neither purges anything here either.
async fn purged_in(side: &Path) -> Result<BTreeSet<String>, LedgerFsError> {
    let raw = read_raw(side).await?;
    let mut by_id: BTreeMap<JudgementId, Vec<&RawEntry>> = BTreeMap::new();
    for entry in raw.entries() {
        by_id.entry(entry.named).or_default().push(entry);
    }
    let mut purged = BTreeSet::new();
    for group in by_id.values() {
        let [entry] = group.as_slice() else {
            continue;
        };
        let Shape::Record(Canon::Map(map)) = entry.shape() else {
            continue;
        };
        let Some(Canon::Map(body)) = map.get("body") else {
            continue;
        };
        if body.get("kind") == Some(&Canon::str("purge"))
            && let Some(Canon::Str(object)) = body.get("object")
        {
            purged.insert(object.clone());
        }
    }
    Ok(purged)
}

// --- the canonical encoding, read back ----------------------------------------------

/// A value from its canonical bytes, or `None` for bytes that are not the canonical
/// encoding of one.
///
/// "Canonical" is checked, not assumed: the value is encoded again and must give the
/// bytes it came from, which refuses trailing bytes, keys out of order, repeated
/// keys and every other spelling of a value that the encoder would not write.
fn decode_canon(bytes: &[u8]) -> Option<Canon> {
    let mut rest = bytes;
    let value = take_value(&mut rest, 0)?;
    (rest.is_empty() && value.to_bytes() == bytes).then_some(value)
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if rest.len() < n {
        return None;
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Some(head)
}

fn take_value(rest: &mut &[u8], depth: usize) -> Option<Canon> {
    if depth > MAX_DEPTH {
        return None;
    }
    let tag = take(rest, 1)?[0];
    let len = u64::from_be_bytes(take(rest, 8)?.try_into().ok()?);
    match tag {
        0x00 => (len == 0).then_some(Canon::Null),
        0x01 => match (len, take(rest, 1)?[0]) {
            (1, 0) => Some(Canon::Bool(false)),
            (1, 1) => Some(Canon::Bool(true)),
            _ => None,
        },
        0x02 => {
            let body = take(rest, 8)?;
            (len == 8).then(|| Canon::Int(i64::from_be_bytes(body.try_into().unwrap_or([0; 8]))))
        }
        0x03 => {
            let body = take(rest, usize::try_from(len).ok()?)?;
            std::str::from_utf8(body).ok().map(Canon::str)
        }
        0x04 => {
            // Every element is at least a tag and a length: more elements than that
            // is a length that lies, and is refused before anything is allocated.
            let count = usize::try_from(len).ok().filter(|n| *n <= rest.len() / 9)?;
            (0..count)
                .map(|_| take_value(rest, depth + 1))
                .collect::<Option<Vec<_>>>()
                .map(Canon::List)
        }
        0x05 => {
            let count = usize::try_from(len)
                .ok()
                .filter(|n| *n <= rest.len() / 18)?;
            let mut map = BTreeMap::new();
            for _ in 0..count {
                let Canon::Str(key) = take_value(rest, depth + 1)? else {
                    return None;
                };
                map.insert(key, take_value(rest, depth + 1)?);
            }
            Some(Canon::Map(map))
        }
        _ => None,
    }
}

// --- the ledger ---------------------------------------------------------------------

/// The two ledgers, in files.
#[derive(Debug, Clone)]
pub struct FsLedger {
    dirs: StoreDirs,
}

impl FsLedger {
    /// The ledgers of these two sides.
    pub fn new(dirs: StoreDirs) -> Self {
        Self { dirs }
    }

    /// The files of one ledger, whatever they hold, with what the adapter could tell
    /// of each. This is what [`LedgerRead::read`] stands on.
    pub async fn read_raw(&self, side: Visibility) -> Result<RawLedger, LedgerFsError> {
        read_raw(self.dirs.side(side)).await
    }
}

impl LedgerRead for FsLedger {
    type Error = LedgerFsError;

    async fn read(&self, side: Visibility) -> Result<LedgerContents, LedgerFsError> {
        let raw = self.read_raw(side).await?;
        let well_formed = raw
            .entries()
            .iter()
            .filter(|entry| matches!(entry.shape(), Shape::Record(_)))
            .count();
        if well_formed > 0 {
            return Err(LedgerFsError::DecoderMissing {
                side,
                records: well_formed,
            });
        }
        Ok(LedgerContents::new(Vec::new(), raw.corrupt()))
    }
}

impl LedgerAppend for FsLedger {
    type Error = LedgerFsError;

    async fn append(&self, record: &Recordable) -> Result<(), LedgerFsError> {
        let side = self.dirs.side(record.ledger());
        let judgement = record.judgement();
        prepare_for_write(side).await?;

        let bytes = judgement.canonical_bytes();
        let dir = side.join(LEDGER_DIR);
        let name = judgement.id().to_string();
        let first = publish(&dir, &name, &bytes, false).await?;
        // The id is taken by a record: the same one is success, another goes beside.
        let held_is_same = match first {
            AppendOutcome::Written => true,
            AppendOutcome::AlreadyPresent => {
                let path = dir.join(&name);
                tokio::fs::read(&path).await.map_err(io_error(&path))? == bytes
            }
        };
        if !held_is_same {
            let twin = format!("{name}.{}", BytesHash::of(&bytes).hex());
            publish(&dir, &twin, &bytes, false).await?;
        }

        if let JudgementKind::Purge { object, .. } = judgement.kind() {
            let path = side.join(OBJECTS_DIR).join(object.hex());
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(&path)(error).into()),
            }
        }
        Ok(())
    }
}

// --- the objects --------------------------------------------------------------------

/// The texts of the versions that were judged, in files.
#[derive(Debug, Clone)]
pub struct FsObjects {
    dirs: StoreDirs,
}

impl FsObjects {
    /// The objects of these two sides.
    pub fn new(dirs: StoreDirs) -> Self {
        Self { dirs }
    }

    async fn read_object(
        &self,
        side: &Path,
        key: TextHash,
    ) -> Result<Option<String>, LedgerFsError> {
        let path = side.join(OBJECTS_DIR).join(key.hex());
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(&path)(error).into()),
        };
        match String::from_utf8(bytes) {
            Ok(text) if TextHash::of(&text) == key => Ok(Some(text)),
            _ => Err(LedgerFsError::CorruptObject { path }),
        }
    }
}

impl ObjectPut for FsObjects {
    type Error = LedgerFsError;

    async fn put(&self, text: &Scanned<'_>) -> Result<Put, LedgerFsError> {
        let side = self.dirs.side(text.ledger());
        let key = *text.key().text();
        if purged_in(side).await?.contains(&key.to_string()) {
            return Ok(Put::Purged);
        }
        prepare_for_write(side).await?;
        let outcome = publish(
            &side.join(OBJECTS_DIR),
            &key.hex(),
            text.text().as_bytes(),
            false,
        )
        .await?;
        if let AppendOutcome::AlreadyPresent = outcome {
            // Whatever is under the name must be the text; a file that is not is
            // reported rather than taken for it.
            self.read_object(side, key).await?;
        }
        Ok(Put::Stored)
    }
}

impl ObjectRead for FsObjects {
    type Error = LedgerFsError;

    async fn get(&self, side: Visibility, key: TextHash) -> Result<Option<String>, LedgerFsError> {
        let side = self.dirs.side(side);
        if purged_in(side).await?.contains(&key.to_string()) {
            return Ok(None);
        }
        self.read_object(side, key).await
    }
}

#[cfg(test)]
mod tests;
