//! The two stores on disk: [`StorePort`] over files.
//!
//! A store is a directory, the same shape for the repo store (`<project>/.whyshelf/`,
//! committed with the project) and for the private store (outside the project):
//!
//! ```text
//! layout                    the layout number of the store (`2`), written first
//! events/<id>.<digest>.json one record of an event per file, written once, never rewritten
//! objects/<hex>             the normalized text of a version, named by its `raw` hash
//! map.json                  the map (generated)
//! map.schema.json           the JSON Schema of the map (generated)
//! status.md                 the status, when the settings ask for it (generated)
//! changelog.md              the changelog, when the settings ask for it (generated)
//! ```
//!
//! `<hex>` and `<id>` are the 64 hex digits of the hash or of the event identifier,
//! without the `sha256:` scheme, so the names are valid on every file system.
//! `<digest>` is the 16 hex digits of the digest of the record (see
//! [`RecordDigest`]).
//!
//! # Records
//!
//! The identifier of an event does not cover the time, so the same change found by
//! two writers is one event written twice. Each write is a **record**: a file named
//! by the identifier and by the digest of the identifier and the time. The same
//! record again is the same file; the same event at another time is a file beside
//! it, with a name that does not collide, so a Git merge of two branches that
//! recorded the same change adds both files. Reading puts the files of one
//! identifier together into one event with the time of each file
//! ([`Recorded`]). The name is checked against the content, not trusted: see
//! *Integrity*.
//!
//! # The mark of the layout
//!
//! The rule for how files are named holds for the whole store, so it is held by the
//! store as a whole: a file `layout` at its root, holding the layout number
//! ([`LAYOUT`]). The first append writes it, before the first event. [`layout_of`]
//! is the one function that reads it; the store's reads and appends and the census
//! of versions ([`crate::format`]) all ask it, so none of them has its own idea of
//! what an unmarked store is.
//!
//! | the store has | [`layout_of`] says |
//! |---|---|
//! | the mark `2` | [`Layout::Current`] |
//! | no mark and no event file | [`Layout::Empty`]: an empty store |
//! | no mark and an event file, or a mark of another number, or a mark that is no number | [`StoreError::Layout`], naming what was found |
//!
//! Two branches that each write the mark first write the same bytes, and Git joins
//! two additions of identical content without a conflict.
//!
//! # Two stores
//!
//! ```text
//!   repo store     <project>/.whyshelf/             committed with the project
//!                    events/  objects/  map.json  map.schema.json
//!                    [status.md  changelog.md]      (if the settings ask)
//!                    cache/                          not a store: ignored by Git
//!
//!   private store  <data dir>/<project>-<project_id>/ outside the repository, never
//!                    events/  objects/  map.json     committed or published;
//!                    map.schema.json                 the same shape
//! ```
//!
//! Where a document goes is decided by where it is put (`[stores] default`, or
//! `--store` at `add`), never by a flag in the file. An entity lives in one store.
//! A public document can name a private entity only as `private:<id>`, the opaque
//! identifier (a UUID v7, no alias, no title); the leak lint refuses a public text
//! that carries a private alias or title. The reverse is free. The repo store's
//! `map.json` lists public entities only, and a private reference in it is the same
//! opaque `private:<id>`.
//!
//! **Publish** moves an entity from the private store to the repo store:
//!
//! ```text
//!   private store                             repo store
//!   events: ... , Published{to: repo}  ---->  Published{from: private, snapshot}
//!   objects: all versions                     objects: the latest text only
//!   approvals stay here                       items of a Design: Registered again
//!                                             (the snapshot is a root: no heads)
//! ```
//!
//! The snapshot carries the classification (kind, path, alias, owner, tags) and the
//! latest version, so the repo store folds on its own to the classification the
//! private store had. Older events and objects, and the **approvals**, stay in the
//! private store: a clone that has only the repo store sees the entity and not its
//! approvals, so it reads as a Draft there.
//!
//! **Purge and complete deletion.** `Workspace::purge` (the library; there is no
//! command or tool for it) deletes the object file of one version from both stores
//! and writes a `Purged` event in the entity's store, so the history is not cut and
//! the version is known to be gone. That removes a text from the working tree and
//! nothing more: a commit that already holds the file still holds it, so a secret
//! that has been pushed must first be revoked, and only then removed from the Git
//! history by rewriting it. The secret scan before a write (see [`crate::secret`])
//! is what keeps this from being needed.
//!
//! # Whether the private store is there
//!
//! The private store is there when its directory is. A directory that exists and
//! holds no event yet is a store with nothing in it ([`Reach::Available`]); a
//! directory that does not exist (a clone, a CI machine) is a store that cannot be
//! read ([`Reach::Absent`]). [`StorePort::events`] gives an empty list for both, so
//! the two are told apart by [`StorePort::private_reach`] alone: a `private:`
//! reference that names nothing is a mistyped id in the first case and an entity
//! this environment cannot see in the second. The directory is made by the first
//! write to the private store, and by nothing else.
//!
//! # Many writers, no lock
//!
//! The identifier of an event is the hash of its content and heads, and a record is
//! named by that identifier and the digest of its time, so the same record found by
//! two processes is the same file, the same change at another time is another file,
//! and a record is never overwritten. A file is written under a name that no other
//! writer uses (`<name>.tmp-<pid>-<counter>`) and then renamed into place, so a
//! reader never sees half a file; if the target exists already the write is a
//! success ([`AppendOutcome::AlreadyPresent`]), the same for an object. The
//! generated files are rebuilt from the sources each time, so whichever writer is
//! last wins and the state converges. Nothing here serializes writers: that is the
//! application's `&mut self` within a process, and the idempotent names between
//! processes.
//!
//! # Integrity
//!
//! The name of a record file is a claim, and it is checked against the content when
//! the file is read: the identifier and the digest are computed again from what is
//! inside, and a name that differs ([`StoreError::NameMismatch`]; a hand-edited time
//! is caught here), a name that is not `<id>.<digest>.json` at all
//! ([`StoreError::BadName`]), event files of one identifier that disagree on the
//! event as a whole (target, heads, body; [`StoreError::Corrupt`], with every path),
//! an object whose bytes do not hash to its name, and a text stored under another
//! text's hash are all refused with an error. Silence would turn a corrupt history
//! into a wrong answer.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use whyshelf_core::app::ports::{AppendOutcome, ExportDoc, ExportFile, ExportParts, StorePort};
use whyshelf_core::app::raw::NormalizedText;
use whyshelf_core::domain::actor::Timestamp;
use whyshelf_core::domain::event::{Event, EventId, RecordDigest, Recorded, StoreKind};
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::reference::Reach;

use crate::export::{self, RenderError};
use crate::format::Found;
use crate::map_dto;
use crate::wire::{self, WireError};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The layout number this build reads and writes, held by the `layout` file at the
/// root of a store.
///
/// It moves when the names or the places of the files of a store change (what the
/// module documentation lists). It is a property of the store as a whole, not of a
/// file in it, which is why it is a file of its own at the root and not a key of the
/// event files.
pub const LAYOUT: u32 = 2;

/// The layout of a store that has event files and no `layout` file: named
/// `events/<id>.json`, before the mark and the digest in the name existed.
pub const LAYOUT_BEFORE_THE_MARK: u32 = 1;

/// The name of the file at the root of a store that holds its layout number.
const LAYOUT_FILE: &str = "layout";

/// What [`layout_of`] found in a store this build can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// No mark and no event: a store nothing was written to yet.
    Empty,
    /// The mark holds [`LAYOUT`].
    Current,
}

/// Why the store could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A file or directory could not be read or written.
    #[error("cannot use {path} in the store")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// An event file could not be read.
    #[error("the event file {path} cannot be read")]
    Read {
        /// The file.
        path: PathBuf,
        /// What was wrong with it.
        #[source]
        source: WireError,
    },
    /// An event could not be written.
    #[error("cannot write the event {id}")]
    Write {
        /// The event.
        id: EventId,
        /// What was wrong.
        #[source]
        source: WireError,
    },
    /// A file in `events/` is not named `<id>.<digest>.json` (64 lowercase hex, a dot,
    /// 16 lowercase hex).
    #[error("the file {path} in the events is not named `<id>.<digest>.json`")]
    BadName {
        /// The file.
        path: PathBuf,
    },
    /// An event file is named after another identifier or another digest than the
    /// ones its content gives.
    #[error("the event file {path} holds the record {digest} of the event {id}")]
    NameMismatch {
        /// The file.
        path: PathBuf,
        /// The identifier computed from the content.
        id: EventId,
        /// The digest computed from the content's identifier and time.
        digest: RecordDigest,
    },
    /// A text was offered under a hash that is not its own.
    #[error("the text does not have the `raw` hash {key}")]
    KeyMismatch {
        /// The hash it was offered under.
        key: ContentHash,
    },
    /// What a store holds contradicts itself, in these files: a stored object that does
    /// not hash to its name or is not text (one path), or event files of one
    /// identifier that disagree on the event (the two paths).
    #[error("the store is corrupt: {}", paths_text(paths))]
    Corrupt {
        /// The files that contradict their names or each other.
        paths: Vec<PathBuf>,
    },
    /// The store is in a layout this build does not read.
    #[error(
        "the store is in layout {found}, and this build reads layout v{reads}; `whyshelf migrate` counts what each store holds"
    )]
    Layout {
        /// What the store says it is in.
        found: Found,
        /// The layout this build reads and writes.
        reads: u32,
    },
    /// A generated file could not be rendered.
    #[error("cannot render a generated file")]
    Render(#[source] RenderError),
    /// The schema of the map could not be rendered.
    #[error("cannot render the schema of the map")]
    Schema(#[source] serde_json::Error),
}

fn paths_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn io_error(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The repo store and the private store.
#[derive(Debug, Clone)]
pub struct FsStore {
    repo: PathBuf,
    private: PathBuf,
}

impl FsStore {
    /// A store over these two directories (neither has to exist yet).
    pub fn new(repo: PathBuf, private: PathBuf) -> Self {
        Self { repo, private }
    }

    fn dir(&self, scope: StoreKind) -> &Path {
        match scope {
            StoreKind::Repo => &self.repo,
            StoreKind::Private => &self.private,
        }
    }
}

/// Write `bytes` to `dir/name` through a file of its own and a rename.
///
/// With `overwrite` false an existing target is left alone and reported as
/// [`AppendOutcome::AlreadyPresent`].
pub(crate) async fn publish(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    overwrite: bool,
) -> Result<AppendOutcome, StoreError> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(io_error(dir))?;
    let target = dir.join(name);
    let exists = |target: &Path| {
        let target = target.to_path_buf();
        async move {
            tokio::fs::try_exists(&target)
                .await
                .map_err(|source| StoreError::Io {
                    path: target,
                    source,
                })
        }
    };
    if !overwrite && exists(&target).await? {
        return Ok(AppendOutcome::AlreadyPresent);
    }
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.tmp-{}-{counter}", std::process::id()));
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(io_error(&tmp))?;
    if !overwrite && exists(&target).await? {
        // Another writer got there between the check and now; its file is the same event.
        let _ = tokio::fs::remove_file(&tmp).await;
        return Ok(AppendOutcome::AlreadyPresent);
    }
    if let Err(source) = tokio::fs::rename(&tmp, &target).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(StoreError::Io {
            path: target,
            source,
        });
    }
    Ok(AppendOutcome::Written)
}

/// Write a generated file unless the bytes on disk are those already.
async fn refresh(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
    match tokio::fs::read(dir.join(name)).await {
        Ok(found) if found == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&dir.join(name))(error)),
    }
    publish(dir, name, bytes, true).await.map(|_| ())
}

/// The name of the file of one record: the identifier of the event and the digest of
/// the record, which the domain computes ([`RecordDigest`]).
fn record_name(id: &EventId, at: &Timestamp) -> String {
    format!("{}.{}.json", id.hash().hex(), RecordDigest::of(id, at))
}

/// The two halves of the name of a record file, if the name has the one shape:
/// 64 lowercase hex, a dot, 16 lowercase hex, `.json`.
fn split_record_name(name: &str) -> Option<(&str, RecordDigest)> {
    let stem = name.strip_suffix(".json")?;
    let (id, digest) = stem.split_once('.')?;
    let id_shaped = id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !id_shaped {
        return None;
    }
    Some((id, digest.parse().ok()?))
}

/// The files of the `events` directory that are record files, by path.
///
/// Every `.json` file is one: a file of that kind with a name of another shape is
/// refused by the caller, not skipped.
async fn record_files(dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let mut listing = match tokio::fs::read_dir(dir).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(dir)(error)),
    };
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(entry) = listing.next_entry().await.map_err(io_error(dir))? {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// The layout a store directory is in, decided by one function for every reader of
/// the store: the events, the append and the census of versions.
///
/// - the mark (`layout`, at the root of the store) holds the number this build
///   writes: [`Layout::Current`];
/// - no mark and no event file: [`Layout::Empty`], a store nothing was written to
///   yet (the first append writes the mark);
/// - no mark and an event file, or a mark with another number, or a mark that is no
///   number: [`StoreError::Layout`] naming what was found. Reading such a store as if
///   its files were named in the layout of this build would report a wrong cause (a
///   name of the wrong shape) instead of the true one.
///
/// A store with events and no mark is the layout before the mark existed
/// ([`LAYOUT_BEFORE_THE_MARK`]).
pub async fn layout_of(store_dir: &Path) -> Result<Layout, StoreError> {
    let mark = store_dir.join(LAYOUT_FILE);
    match tokio::fs::read(&mark).await {
        Ok(bytes) => {
            let found = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
                .map_or(Found::Unknown, Found::Version);
            if found == Found::Version(LAYOUT) {
                Ok(Layout::Current)
            } else {
                Err(StoreError::Layout {
                    found,
                    reads: LAYOUT,
                })
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if record_files(&store_dir.join("events")).await?.is_empty() {
                Ok(Layout::Empty)
            } else {
                Err(StoreError::Layout {
                    found: Found::Version(LAYOUT_BEFORE_THE_MARK),
                    reads: LAYOUT,
                })
            }
        }
        Err(error) => Err(io_error(&mark)(error)),
    }
}

impl StorePort for FsStore {
    type Error = StoreError;

    async fn append(
        &self,
        scope: StoreKind,
        event: &Event,
        at: &Timestamp,
    ) -> Result<AppendOutcome, StoreError> {
        let dir = self.dir(scope);
        let layout = layout_of(dir).await?;
        let text = wire::encode(event, at).map_err(|source| StoreError::Write {
            id: event.id(),
            source,
        })?;
        if layout == Layout::Empty {
            // The mark goes down before the first event, so a store with an event
            // always has one. Two writers that both find the store empty write the
            // same bytes, and the second finds the first's file.
            publish(dir, LAYOUT_FILE, format!("{LAYOUT}\n").as_bytes(), false).await?;
        }
        publish(
            &dir.join("events"),
            &record_name(&event.id(), at),
            text.as_bytes(),
            false,
        )
        .await
    }

    async fn events(&self, scope: StoreKind) -> Result<Vec<Recorded>, StoreError> {
        let dir = self.dir(scope);
        if layout_of(dir).await? == Layout::Empty {
            return Ok(Vec::new());
        }
        let mut events: Vec<(PathBuf, Recorded)> = Vec::new();
        let mut position: BTreeMap<EventId, usize> = BTreeMap::new();
        for path in record_files(&dir.join("events")).await? {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let Some((named_id, named_digest)) = split_record_name(name) else {
                return Err(StoreError::BadName { path });
            };
            let (named_id, named_digest) = (named_id.to_owned(), named_digest);
            let bytes = tokio::fs::read(&path).await.map_err(io_error(&path))?;
            let record = wire::decode(&bytes).map_err(|source| StoreError::Read {
                path: path.clone(),
                source,
            })?;
            // The name is a claim about the content, and the content is what counts:
            // the identifier and the digest are computed again from it.
            let id = record.event().id();
            let digest = RecordDigest::of(&id, record.shown_at());
            if id.hash().hex() != named_id || digest != named_digest {
                return Err(StoreError::NameMismatch { path, id, digest });
            }
            match position.get(&id) {
                None => {
                    position.insert(id, events.len());
                    events.push((path, record));
                }
                Some(&at) => {
                    let (first_path, held) = &mut events[at];
                    held.merge(record).map_err(|_| StoreError::Corrupt {
                        paths: vec![first_path.clone(), path],
                    })?;
                }
            }
        }
        Ok(events.into_iter().map(|(_, record)| record).collect())
    }

    async fn private_reach(&self) -> Result<Reach, StoreError> {
        // The store is there when its directory can be listed.
        match tokio::fs::read_dir(&self.private).await {
            Ok(_) => Ok(Reach::Available),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(Reach::Absent)
            }
            Err(error) => Err(io_error(&self.private)(error)),
        }
    }

    async fn put_object(
        &self,
        scope: StoreKind,
        raw: &ContentHash,
        text: &NormalizedText,
    ) -> Result<AppendOutcome, StoreError> {
        if text.raw_hash() != *raw {
            return Err(StoreError::KeyMismatch { key: *raw });
        }
        publish(
            &self.dir(scope).join("objects"),
            &raw.hex(),
            text.as_str().as_bytes(),
            false,
        )
        .await
    }

    async fn get_object(
        &self,
        scope: StoreKind,
        raw: &ContentHash,
    ) -> Result<Option<NormalizedText>, StoreError> {
        let path = self.dir(scope).join("objects").join(raw.hex());
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(&path)(error)),
        };
        if ContentHash::raw(&bytes) != *raw {
            return Err(StoreError::Corrupt { paths: vec![path] });
        }
        match String::from_utf8(bytes) {
            Ok(text) => Ok(Some(NormalizedText::new(text))),
            Err(_) => Err(StoreError::Corrupt { paths: vec![path] }),
        }
    }

    async fn purge_object(&self, scope: StoreKind, raw: &ContentHash) -> Result<bool, StoreError> {
        let path = self.dir(scope).join("objects").join(raw.hex());
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error(&path)(error)),
        }
    }

    async fn write_export(&self, scope: StoreKind, doc: &ExportDoc<'_>) -> Result<(), StoreError> {
        let dir = self.dir(scope);
        let kind = doc.kind();
        let body = export::render_body(doc).map_err(StoreError::Render)?;
        refresh(
            dir,
            export::file_name(kind),
            export::with_marker(kind, &body).as_bytes(),
        )
        .await?;
        if matches!(doc, ExportDoc::Map(_)) {
            let schema = map_dto::schema_json().map_err(StoreError::Schema)?;
            refresh(dir, "map.schema.json", schema.as_bytes()).await?;
        }
        Ok(())
    }

    async fn inspect_export(
        &self,
        scope: StoreKind,
        doc: &ExportDoc<'_>,
    ) -> Result<ExportFile, StoreError> {
        let kind = doc.kind();
        let path = self.dir(scope).join(export::file_name(kind));
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(ExportFile::Absent),
            Err(error) => return Err(io_error(&path)(error)),
        };
        let rebuilt = export::render_body(doc).map_err(StoreError::Render)?;
        Ok(match export::split_marker(kind, &bytes) {
            None => ExportFile::Unmarked,
            Some((marker, body)) => ExportFile::Marked(ExportParts::new(
                marker,
                body.into_bytes(),
                rebuilt.into_bytes(),
            )),
        })
    }
}
