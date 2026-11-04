//! Ingest: from the files people edit to the events that record them.
//!
//! The document files are edited by hand (or by an AI), and the tool never rewrites
//! them. Ingest is how an edit becomes history. See the module documentation of
//! [`app`](super) for the five steps; this is what each does:
//!
//! 1. **Diff by stamp.** The listing of files (path, modification stamp, size) is
//!    compared with the cache, and only changed files are read for events. If more
//!    files changed than `rebuild_threshold` (a branch switch), every file is read and
//!    the cache is replaced instead of patched.
//! 2. **Rename.** An unregistered file whose text equals the last recorded text of a
//!    registered file that has vanished is that file moved: a `Moved` event. The
//!    comparison is by the `raw` hash, because a file that does not parse has no
//!    content hash.
//! 3. **Deletion.** A vanished file writes **no event**. The entity stays in the log
//!    and is reported as "file missing"; the cache row goes.
//! 4. **Parse failure.** A file that does not parse (broken front matter, a conflict
//!    marker, a reference that names nothing) is recorded as a `Revised` whose new
//!    version has only a raw hash, with its text stored, after the secret scan. The
//!    entity is Invalid until the file is fixed, and fixing it is the next `Revised`.
//! 5. **Success.** The entity is built (references resolved, revisions checked). A
//!    changed content hash, or a changed supplementary section alone (same content
//!    hash, new raw hash), writes a `Revised`; a Design's new item numbers each write
//!    a `Registered` with a new identifier, and an item whose number leaves the Design
//!    is just no longer shown (its events stay).
//!
//! Each file is gated on its own: a secret or a private name in one file blocks that
//! file (listed in [`IngestReport::blocked`], and retried by the next ingest, since
//! its cache row is not updated) without stopping the others. The files that pass are
//! written together.
//!
//! # The read path, first half: from file changes to events
//!
//! ```text
//!   document files (edited by hand)            cache rows from the last run
//!   docs/finding, docs/spec, docs/design,      path, mtime, size, raw hash
//!   the watched dirs, every registered path
//!            \                                      /
//!             +--- list (path, mtime, size) ------+
//!                              |  compare stamps
//!                              v
//!                    changed files          more than `rebuild_threshold`,
//!                              |            or `rebuild`?  -> read every file,
//!                              |                              replace the cache
//!                              v
//!                  read + normalize + parse -> Candidate
//!                              |             (a raw value, or Unparsed + reasons)
//!      one candidate at a time |
//!        registered path --> build entity; version differs?  --> Revised (+ text)
//!                            a Design with a new item number --> Registered (item)
//!        unregistered    --> text equals a vanished file's    --> Moved
//!                            last text? otherwise: listed as unregistered
//!                              |
//!                  gate this file's part alone (secret scan, leak lint)
//!                     refused --> `blocked`, the others go on
//!                              |
//!                              v
//!               one draft -> commit (gate, append, notify)      [write path]
//!                              |
//!                              v
//!               cache: rows of the files that passed are written,
//!               rows of vanished files removed (or the whole cache replaced)
//!                              |
//!                              v
//!               the next Model::load sees the new events and the same files
//!                                                       [read path, 2nd half]
//! ```
//!
//! The second half, from the events and the files to a state, is
//! [`Model::load`](super::model::Model::load).

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::event::{RecordBody, Registration};
use crate::domain::id::EntityId;
use crate::domain::location::RelPath;

use super::Workspace;
use super::commit::Committed;
use super::error::AppError;
use super::model::{Built, Content, Model, build};
use super::ports::{CacheEntry, CurrentPort, FileStat, IndexCachePort, PortName, Ports};
use super::raw::Candidate;
use super::subscriber::SubscriberWarning;

/// A file ingest could not record.
#[derive(Debug)]
pub struct BlockedFile {
    path: RelPath,
    error: AppError,
}

impl BlockedFile {
    /// The file.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// Why: a secret, a private name in a public text, and so on.
    pub fn error(&self) -> &AppError {
        &self.error
    }
}

/// What an ingest found and recorded.
#[derive(Debug, Default)]
pub struct IngestReport {
    revised: Vec<EntityId>,
    moved: Vec<(EntityId, RelPath, RelPath)>,
    items_registered: usize,
    unregistered: Vec<RelPath>,
    missing: Vec<RelPath>,
    blocked: Vec<BlockedFile>,
    rebuilt: bool,
    committed: Option<Committed>,
    warnings: Vec<SubscriberWarning>,
}

impl IngestReport {
    /// Documents whose text changed and was recorded.
    pub fn revised(&self) -> &[EntityId] {
        &self.revised
    }

    /// Documents recognized at a new path: identifier, old path, new path.
    pub fn moved(&self) -> &[(EntityId, RelPath, RelPath)] {
        &self.moved
    }

    /// How many new items were registered.
    pub fn items_registered(&self) -> usize {
        self.items_registered
    }

    /// Files nobody registered. Never taken in silently.
    pub fn unregistered(&self) -> &[RelPath] {
        &self.unregistered
    }

    /// Registered files that are gone. No event was written for them.
    pub fn missing(&self) -> &[RelPath] {
        &self.missing
    }

    /// Files the gate stopped.
    pub fn blocked(&self) -> &[BlockedFile] {
        &self.blocked
    }

    /// Whether the cache was rebuilt instead of patched.
    pub fn rebuilt(&self) -> bool {
        self.rebuilt
    }

    /// The write, when there was one.
    pub fn committed(&self) -> Option<&Committed> {
        self.committed.as_ref()
    }

    /// Subscriber failures of the write (or of the rebuild).
    pub fn warnings(&self) -> &[SubscriberWarning] {
        match &self.committed {
            Some(committed) => committed.warnings(),
            None => &self.warnings,
        }
    }
}

/// Whether ingest trusts the cache's stamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Diff,
    Full,
}

fn is_gate_refusal(error: &AppError) -> bool {
    matches!(
        error,
        AppError::SecretFound { .. } | AppError::LeakFound { .. }
    )
}

impl<P: Ports> Workspace<P> {
    /// Bring the log up to date with the files.
    pub async fn ingest(&mut self) -> Result<IngestReport, AppError> {
        self.run_ingest(Mode::Diff).await
    }

    /// Read every file, replace the cache, and write the generated files again,
    /// even when no event is needed (after a merge of the map, say).
    pub async fn rebuild(&mut self) -> Result<IngestReport, AppError> {
        let mut report = self.run_ingest(Mode::Full).await?;
        if report.committed.is_none() {
            report.warnings = self.notify(&[]).await;
        }
        Ok(report)
    }

    async fn run_ingest(&self, mode: Mode) -> Result<IngestReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let mut report = IngestReport::default();

        // 1. Which files changed since the cache.
        let cached: BTreeMap<RelPath, CacheEntry> = self
            .ports
            .cache()
            .load()
            .await
            .map_err(AppError::port(PortName::IndexCache))?
            .into_iter()
            .map(|e| (e.path().clone(), e))
            .collect();
        let listed: &BTreeMap<RelPath, FileStat> = model.stats();
        let changed: Vec<&FileStat> = listed
            .values()
            .filter(|s| cached.get(s.path()).is_none_or(|c| !c.stat().same_as(s)))
            .collect();
        report.rebuilt =
            mode == Mode::Full || changed.len() > self.config.ingest().rebuild_threshold();
        let to_read: Vec<&FileStat> = if report.rebuilt {
            listed.values().collect()
        } else {
            changed
        };

        let mut candidates: Vec<Candidate> = Vec::new();
        for stat in to_read {
            // A registered file is read as its kind; a file nobody registered has none.
            let kind = model
                .docs()
                .find(|d| d.path() == stat.path())
                .map(|d| d.kind());
            if let Some(candidate) = self
                .ports
                .current()
                .read(stat.path(), kind)
                .await
                .map_err(AppError::port(PortName::Current))?
            {
                candidates.push(candidate);
            }
        }

        // The registered documents whose file has gone, by path.
        let vanished: Vec<_> = model.docs().filter(|d| d.present().is_none()).collect();
        report.missing = vanished.iter().map(|d| d.path().clone()).collect();
        let mut claimed: BTreeSet<EntityId> = BTreeSet::new();

        let mut draft = self.draft().await?;
        let mut blocked_paths: BTreeSet<RelPath> = BTreeSet::new();
        let mut entries: Vec<CacheEntry> = Vec::new();
        for candidate in &candidates {
            let mut part = draft.like();
            let mut revised: Option<EntityId> = None;
            let mut moved: Option<(EntityId, RelPath, RelPath)> = None;
            let mut registered = 0usize;
            let mut body = candidate.text().as_str().to_owned();
            let owner = model.docs().find(|d| d.path() == candidate.path());
            match owner {
                Some(doc) => {
                    let built = build(
                        &self.ports,
                        model.index(),
                        doc.store(),
                        doc.id(),
                        Some(doc.kind()),
                        candidate,
                    )
                    .await?;
                    body = Content::from(&built).search_text();
                    let new_version = built.version(candidate.raw_hash());
                    // While two branches disagree about the recorded version there is
                    // no single old one: the event names the new one twice and heals
                    // the fork by listing every tip.
                    let recorded = doc.recorded().cloned();
                    if recorded.as_ref() != Some(&new_version) {
                        part.push(
                            &model,
                            doc.store(),
                            doc.id(),
                            RecordBody::Revised {
                                old: recorded.unwrap_or_else(|| new_version.clone()),
                                new: new_version,
                            },
                        )?;
                        part.object(doc.store(), candidate.text().clone());
                        revised = Some(doc.id());
                    }
                    if let Built::Design(design, _) = &built {
                        let view = model.view(doc.store());
                        for number in design.items().keys() {
                            if view.item_id(design.id(), *number).is_none() {
                                let item = self.new_entity_id().await?;
                                part.push(
                                    &model,
                                    doc.store(),
                                    item,
                                    RecordBody::Registered(Registration::Item {
                                        design: design.id(),
                                        number: *number,
                                    }),
                                )?;
                                registered += 1;
                            }
                        }
                    }
                }
                None => {
                    // 2. A new path whose text is a vanished file's last text.
                    let raw = candidate.raw_hash();
                    let renamed = vanished.iter().find(|d| {
                        !claimed.contains(&d.id())
                            && d.recorded().is_some_and(|v| v.raw() == &raw)
                            && candidate.kind().is_none_or(|k| k == d.kind())
                    });
                    if let Some(doc) = renamed {
                        claimed.insert(doc.id());
                        part.push(
                            &model,
                            doc.store(),
                            doc.id(),
                            RecordBody::Moved {
                                old: doc.path().clone(),
                                new: candidate.path().clone(),
                            },
                        )?;
                        moved = Some((doc.id(), doc.path().clone(), candidate.path().clone()));
                    }
                }
            }
            if let Err(error) = self.gate(&model, &part).await {
                if is_gate_refusal(&error) {
                    blocked_paths.insert(candidate.path().clone());
                    report.blocked.push(BlockedFile {
                        path: candidate.path().clone(),
                        error,
                    });
                    continue;
                }
                return Err(error);
            }
            draft.merge(part);
            report.revised.extend(revised);
            report.moved.extend(moved);
            report.items_registered += registered;
            if let Some(stat) = listed.get(candidate.path()) {
                entries.push(CacheEntry::new(stat.clone(), candidate.raw_hash(), body));
            }
        }
        // A renamed file is no longer missing.
        report
            .missing
            .retain(|p| !report.moved.iter().any(|(_, old, _)| old == p));
        report.unregistered = model
            .unregistered()
            .iter()
            .filter(|p| !report.moved.iter().any(|(_, _, new)| &new == p))
            .cloned()
            .collect();

        if !draft.is_empty() {
            let committed = self.commit(&model, draft).await?;
            report.committed = Some(committed);
        }

        // 3. The cache follows the files: rows of changed files that passed,
        // none for files that went.
        let gone: Vec<RelPath> = cached
            .keys()
            .filter(|p| !listed.contains_key(*p))
            .cloned()
            .collect();
        if report.rebuilt {
            self.ports
                .cache()
                .rebuild(&entries)
                .await
                .map_err(AppError::port(PortName::IndexCache))?;
        } else {
            if !gone.is_empty() {
                self.ports
                    .cache()
                    .remove(&gone)
                    .await
                    .map_err(AppError::port(PortName::IndexCache))?;
            }
            if !entries.is_empty() {
                self.ports
                    .cache()
                    .upsert(&entries)
                    .await
                    .map_err(AppError::port(PortName::IndexCache))?;
            }
        }
        Ok(report)
    }
}
