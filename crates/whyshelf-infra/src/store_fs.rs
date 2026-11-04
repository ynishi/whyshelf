//! The two stores on disk: [`StorePort`] over files.
//!
//! A store is a directory, the same shape for the repo store (`<project>/.whyshelf/`,
//! committed with the project) and for the private store (outside the project):
//!
//! ```text
//! events/<hex>.json     one event per file, written once, never rewritten
//! objects/<hex>         the normalized text of a version, named by its `raw` hash
//! map.json              the map (generated)
//! map.schema.json       the JSON Schema of the map (generated)
//! status.md             the status, when the settings ask for it (generated)
//! changelog.md          the changelog, when the settings ask for it (generated)
//! ```
//!
//! `<hex>` is the 64 hex digits of the identifier or hash, without the `sha256:`
//! scheme, so the names are valid on every file system.
//!
//! # Two stores
//!
//! ```text
//!   repo store     <project>/.whyshelf/             committed with the project
//!                    events/  objects/  map.json  map.schema.json
//!                    [status.md  changelog.md]      (if the settings ask)
//!                    cache/                          not a store: ignored by Git
//!
//!   private store  <data dir>/<project>/            outside the repository, never
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
//! # Many writers, no lock
//!
//! The identifier of an event is the hash of its content and heads, so the same
//! change found by two processes is the same file, and an event is never
//! overwritten. A file is written under a name that no other writer uses
//! (`<name>.tmp-<pid>-<counter>`) and then renamed into place, so a reader never
//! sees half a file; if the target exists already the write is a success
//! ([`AppendOutcome::AlreadyPresent`]), the same for an object. The generated files
//! are rebuilt from the sources each time, so whichever writer is last wins and the
//! state converges. Nothing here serializes writers: that is the application's
//! `&mut self` within a process, and the idempotent names between processes.
//!
//! # Integrity
//!
//! An event file whose name is not its identifier, an object whose bytes do not
//! hash to its name, and a text stored under another text's hash are all refused
//! with an error. Silence would turn a corrupt history into a wrong answer.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use whyshelf_core::app::ports::{AppendOutcome, ExportDoc, ExportFile, ExportParts, StorePort};
use whyshelf_core::app::raw::NormalizedText;
use whyshelf_core::domain::event::{Event, EventId, StoreKind};
use whyshelf_core::domain::hash::ContentHash;

use crate::export::{self, RenderError};
use crate::map_dto;
use crate::wire::{self, WireError};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    /// An event file is named after another identifier than the one inside it.
    #[error("the event file {path} holds the event {id}")]
    NameMismatch {
        /// The file.
        path: PathBuf,
        /// The identifier inside.
        id: EventId,
    },
    /// A text was offered under a hash that is not its own.
    #[error("the text does not have the `raw` hash {key}")]
    KeyMismatch {
        /// The hash it was offered under.
        key: ContentHash,
    },
    /// A stored object does not hash to its name, or is not text.
    #[error("the stored object {path} does not match its name")]
    Corrupt {
        /// The file.
        path: PathBuf,
    },
    /// A generated file could not be rendered.
    #[error("cannot render a generated file")]
    Render(#[source] RenderError),
    /// The schema of the map could not be rendered.
    #[error("cannot render the schema of the map")]
    Schema(#[source] serde_json::Error),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
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
async fn publish(
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

fn event_name(id: &EventId) -> String {
    format!("{}.json", id.hash().hex())
}

impl StorePort for FsStore {
    type Error = StoreError;

    async fn append(&self, scope: StoreKind, event: &Event) -> Result<AppendOutcome, StoreError> {
        let text = wire::encode(event).map_err(|source| StoreError::Write {
            id: event.id(),
            source,
        })?;
        publish(
            &self.dir(scope).join("events"),
            &event_name(&event.id()),
            text.as_bytes(),
            false,
        )
        .await
    }

    async fn events(&self, scope: StoreKind) -> Result<Vec<Event>, StoreError> {
        let dir = self.dir(scope).join("events");
        let mut listing = match tokio::fs::read_dir(&dir).await {
            Ok(listing) => listing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(&dir)(error)),
        };
        let mut files: Vec<PathBuf> = Vec::new();
        while let Some(entry) = listing.next_entry().await.map_err(io_error(&dir))? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                files.push(path);
            }
        }
        files.sort();
        let mut events = Vec::with_capacity(files.len());
        for path in files {
            let bytes = tokio::fs::read(&path).await.map_err(io_error(&path))?;
            let event = wire::decode(&bytes).map_err(|source| StoreError::Read {
                path: path.clone(),
                source,
            })?;
            if path.file_name().and_then(|n| n.to_str()) != Some(event_name(&event.id()).as_str()) {
                return Err(StoreError::NameMismatch {
                    path,
                    id: event.id(),
                });
            }
            events.push(event);
        }
        Ok(events)
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
            return Err(StoreError::Corrupt { path });
        }
        match String::from_utf8(bytes) {
            Ok(text) => Ok(Some(NormalizedText::new(text))),
            Err(_) => Err(StoreError::Corrupt { path }),
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
