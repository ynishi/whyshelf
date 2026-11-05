//! Registration and classification: `add`, `tag`, `reclassify`, `mv`, `publish`,
//! `purge`.
//!
//! The tool never rewrites a user's file. What it owns is the *classification* of
//! an entity (its identifier, kind, alias, owner, tags, path and store), and that
//! lives in events, never in the document. Registering gives a file an identifier
//! and records where it is; from then on renaming the file does not lose its
//! approvals, because the identifier, not the path, is the identity.
//!
//! | use case | records |
//! |---|---|
//! | [`add`](Workspace::add) | `Registered` with a new identifier, and a `Registered` for each item number of a Design; the text is stored as an object |
//! | [`tag`](Workspace::tag), [`reclassify`](Workspace::reclassify) | `Reclassified`: tags, kind, owner, alias. Official tags must be in the project's vocabulary; an alias must be free in the store. Never affects an approval |
//! | [`mv`](Workspace::mv) | `Moved`: the file is already at the new path (the tool does not move files); the identifier is unchanged |
//! | [`publish`](Workspace::publish) | `Published` in each store: an ordinary event in the private store and a root with a snapshot of the classification in the repo store, so the repo store alone folds to the same classification. The latest text goes to the repo store, past versions stay private. The alias must be free in the repo store |
//! | [`purge`](Workspace::purge) | `Purged`, for every entity whose history names the text: a stored text is deleted (a secret slipped through), the history is not |
//!
//! Where a new document goes is decided by where it is put: the project's default
//! store, or the caller's choice, never by a flag inside the document.

use std::collections::BTreeSet;

use crate::domain::classification::{ConflictField, Merged};
use crate::domain::event::{
    ClassChange, ClassSnapshot, DocKind, DocRegistration, EntityKind, Publication, RecordBody,
    Registration, StoreKind,
};
use crate::domain::front::{ClassificationImport, Guess, Stored};
use crate::domain::hash::ContentHash;
use crate::domain::id::{Alias, EntityId};
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::reference::Ref;
use crate::domain::tag::{Tag, TagChange};

use super::Workspace;
use super::commit::{Committed, Draft};
use super::error::AppError;
use super::model::{Built, Model, Resolved, build};
use super::ports::{CurrentPort, IdPort, PortName, Ports, StorePort};
use super::raw::Candidate;

/// What to register a file as.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddOptions {
    kind: Option<DocKind>,
    tags: BTreeSet<Tag>,
    alias: Option<Alias>,
    store: Option<StoreKind>,
}

impl AddOptions {
    /// Options that leave everything to the file and the configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// The kind the file must be (needed for a file that does not parse yet).
    pub fn with_kind(mut self, kind: DocKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Tags to start with.
    pub fn with_tags(mut self, tags: BTreeSet<Tag>) -> Self {
        self.tags = tags;
        self
    }

    /// An alias; only for a single file.
    pub fn with_alias(mut self, alias: Alias) -> Self {
        self.alias = Some(alias);
        self
    }

    /// The store to put the entity in, instead of the project's default.
    pub fn with_store(mut self, store: StoreKind) -> Self {
        self.store = Some(store);
        self
    }
}

/// What `add` did.
#[derive(Debug)]
pub struct Added {
    registered: Vec<(EntityId, RelPath)>,
    skipped: Vec<RelPath>,
    committed: Committed,
}

impl Added {
    /// The files registered, with their new identifiers.
    pub fn registered(&self) -> &[(EntityId, RelPath)] {
        &self.registered
    }

    /// Files that were registered already and left alone.
    pub fn skipped(&self) -> &[RelPath] {
        &self.skipped
    }

    /// The write and its subscribers' warnings.
    pub fn committed(&self) -> &Committed {
        &self.committed
    }
}

impl<P: Ports> Workspace<P> {
    pub(crate) async fn new_entity_id(&self) -> Result<EntityId, AppError> {
        self.ports
            .id()
            .new_entity_id()
            .await
            .map_err(AppError::port(PortName::Id))
    }

    /// Give a file an identifier and put its registration in the draft.
    pub(crate) async fn register_candidate(
        &self,
        model: &Model,
        draft: &mut Draft,
        candidate: &Candidate,
        options: &AddOptions,
        store: StoreKind,
    ) -> Result<EntityId, AppError> {
        if model.path_holder(candidate.path()).is_some() {
            return Err(AppError::AlreadyRegistered {
                path: candidate.path().clone(),
            });
        }
        // A file is read as the kind asked for; the keys still say what it looks like,
        // and a file that looks like another kind is refused rather than registered
        // as an Invalid document of the wrong kind. Without a kind asked for, only
        // keys that name exactly one kind register anything.
        let kind = match (candidate.guess(), candidate.kind(), options.kind) {
            (Guess::One(found), _, Some(requested)) if found != requested => {
                return Err(AppError::KindMismatch {
                    path: candidate.path().clone(),
                    requested,
                    found,
                });
            }
            (_, Some(found), Some(requested)) if found != requested => {
                return Err(AppError::KindMismatch {
                    path: candidate.path().clone(),
                    requested,
                    found,
                });
            }
            (_, Some(found), _) => found,
            (_, None, Some(requested)) => requested,
            (guess, None, None) => {
                return Err(AppError::KindUnknown {
                    path: candidate.path().clone(),
                    guess,
                });
            }
        };
        for tag in &options.tags {
            self.config.vocabulary().admit(tag)?;
        }
        let held = |alias: &Alias| {
            model
                .index()
                .alias_holder(store, alias)
                .or_else(|| draft.alias_holder(store, alias))
        };
        if let Some(alias) = &options.alias
            && let Some(holder) = held(alias)
        {
            return Err(AppError::AliasTaken {
                alias: alias.clone(),
                holder,
            });
        }
        // What the file's own classification keys say is taken in once, here. What
        // the options say wins; whatever cannot be taken in (an alias another document
        // holds, a tag outside the vocabulary) is left out and shows up as a drift.
        let import = ClassificationImport::of(candidate.classification(), Stored::New { kind });
        let mut tags = options.tags.clone();
        if import
            .tags()
            .iter()
            .all(|tag| self.config.vocabulary().admit(tag).is_ok())
        {
            tags.extend(import.tags().iter().cloned());
        }
        let alias = options.alias.clone().or_else(|| {
            import
                .alias()
                .filter(|alias| held(alias).is_none())
                .cloned()
        });
        let owner = import.owner().cloned();
        let id = self.new_entity_id().await?;
        let built = build(&self.ports, model.index(), store, id, Some(kind), candidate).await?;
        let version = built.version(candidate.raw_hash());
        draft.push(
            model,
            store,
            id,
            RecordBody::Registered(Registration::Doc(DocRegistration::new(
                kind,
                candidate.path().clone(),
                alias,
                owner,
                tags,
                version,
            ))),
        )?;
        draft.object(store, candidate.text().clone());
        if let Built::Design(design, _) = &built {
            for number in design.items().keys() {
                let item = self.new_entity_id().await?;
                draft.push(
                    model,
                    store,
                    item,
                    RecordBody::Registered(Registration::Item {
                        design: design.id(),
                        number: *number,
                    }),
                )?;
            }
        }
        Ok(id)
    }

    /// Register files (or every document under a directory).
    ///
    /// A file that is registered already is skipped and reported. All files of
    /// the call are written together or not at all: a secret in one of them stops
    /// the call. An alias a file's own keys name is taken in unless another
    /// document holds it, or a file registered earlier in the same call does; then it
    /// is left out and shown as a difference.
    pub async fn add(
        &mut self,
        paths: &[RelPath],
        options: &AddOptions,
    ) -> Result<Added, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let store = options.store.unwrap_or(self.config.default_store());
        let listed = self
            .ports
            .current()
            .list(paths)
            .await
            .map_err(AppError::port(PortName::Current))?;
        let (known, fresh): (Vec<_>, Vec<_>) = listed
            .into_iter()
            .map(|s| s.path().clone())
            .partition(|p| model.path_holder(p).is_some());
        if options.alias.is_some() && fresh.len() > 1 {
            return Err(AppError::AliasForSeveralFiles { count: fresh.len() });
        }
        let mut draft = self.draft().await?;
        let mut registered = Vec::new();
        for path in fresh {
            let candidate = self
                .ports
                .current()
                .read(&path, options.kind)
                .await
                .map_err(AppError::port(PortName::Current))?
                .ok_or_else(|| AppError::FileMissing { path: path.clone() })?;
            let id = self
                .register_candidate(&model, &mut draft, &candidate, options, store)
                .await?;
            registered.push((id, path));
        }
        let committed = if draft.is_empty() {
            Committed::nothing()
        } else {
            self.commit(&model, draft).await?
        };
        Ok(Added {
            registered,
            skipped: known,
            committed,
        })
    }

    /// Change the classification of a document: tags, kind, owner, alias.
    pub async fn reclassify(
        &mut self,
        target: &Ref,
        changes: NonEmptyVec<ClassChange>,
    ) -> Result<Committed, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        // A document in conflict can be reclassified: that is how a conflict over the
        // kind is settled.
        let (id, store) = match model.locate(target)? {
            Resolved::Entity { id, kind, store } => {
                if kind == EntityKind::Item {
                    return Err(AppError::NotADocument { id });
                }
                (id, store)
            }
            Resolved::Conflicted(entry) => (entry.id(), entry.store()),
        };
        for change in changes.iter() {
            match change {
                ClassChange::Tag(TagChange::Add(tag)) => {
                    self.config.vocabulary().admit(tag)?;
                }
                ClassChange::SetAlias(alias) => {
                    if let Some(holder) = model.index().alias_holder(store, alias)
                        && holder != id
                    {
                        return Err(AppError::AliasTaken {
                            alias: alias.clone(),
                            holder,
                        });
                    }
                }
                _ => {}
            }
        }
        let mut draft = self.draft().await?;
        draft.push(&model, store, id, RecordBody::Reclassified { changes })?;
        self.commit(&model, draft).await
    }

    /// Put or take off tags. A shorthand for [`reclassify`](Self::reclassify).
    pub async fn tag(
        &mut self,
        target: &Ref,
        changes: NonEmptyVec<TagChange>,
    ) -> Result<Committed, AppError> {
        let changes: Vec<ClassChange> = changes
            .into_vec()
            .into_iter()
            .map(ClassChange::Tag)
            .collect();
        self.reclassify(target, NonEmptyVec::from_vec(changes)?)
            .await
    }

    /// Record that a document is now at another path. The file must already be
    /// there: the tool does not move files.
    ///
    /// A path that two branches set differently is settled by this: the move lists
    /// both among its heads. Any other conflict of the classification is refused.
    pub async fn mv(&mut self, target: &Ref, new_path: RelPath) -> Result<Committed, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let (id, store, old) = match model.locate(target)? {
            Resolved::Entity { id, kind, store } => {
                if kind == EntityKind::Item {
                    return Err(AppError::NotADocument { id });
                }
                let doc = model.doc(id).ok_or(AppError::NotADocument { id })?;
                if doc.path() == &new_path {
                    return Ok(Committed::nothing());
                }
                (doc.id(), store, doc.path().clone())
            }
            Resolved::Conflicted(entry) if entry.doc().field() == ConflictField::Path => {
                // The event names the path it moves from, and there are several: it
                // names one of them (the smallest). Which, decides nothing.
                let old = model
                    .view(entry.store())
                    .doc(entry.id())
                    .and_then(|class| match class.path() {
                        Merged::Conflict(paths) => paths.iter().next().cloned(),
                        Merged::Agreed(path) => Some(path.clone()),
                    })
                    .ok_or(AppError::NotADocument { id: entry.id() })?;
                (entry.id(), entry.store(), old)
            }
            Resolved::Conflicted(entry) => {
                return Err(AppError::ClassificationConflict {
                    id: entry.id(),
                    field: entry.doc().field(),
                });
            }
        };
        if let Some(holder) = model.path_holder(&new_path)
            && holder != id
        {
            return Err(AppError::PathOccupied {
                path: new_path,
                holder,
            });
        }
        let there = self
            .ports
            .current()
            .read(&new_path, None)
            .await
            .map_err(AppError::port(PortName::Current))?;
        if there.is_none() {
            return Err(AppError::FileMissing { path: new_path });
        }
        let mut draft = self.draft().await?;
        draft.push(&model, store, id, RecordBody::Moved { old, new: new_path })?;
        self.commit(&model, draft).await
    }

    /// Move a document from the private store to the repo store.
    ///
    /// The latest recorded text goes to the repo store and passes the leak lint
    /// there: a text that still carries the alias or title of a private entity is
    /// refused, and the document must name it as `private:<id>` instead. The
    /// document's alias must be free in the repo store ([`AppError::AliasTaken`]).
    pub async fn publish(&mut self, target: &Ref) -> Result<Committed, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let (id, kind, store) = model.resolve(target)?;
        if kind == EntityKind::Item {
            return Err(AppError::NotADocument { id });
        }
        if store == StoreKind::Repo {
            return Err(AppError::AlreadyPublished { id });
        }
        // The classification is copied into the repo store: a value two branches set
        // differently cannot be, and is not guessed.
        model.settled(id)?;
        let doc = model.doc(id).ok_or(AppError::NotADocument { id })?;
        // The snapshot's alias becomes the entity's alias in the repo store, where it
        // must be free, as it must be when it is set anywhere else.
        if let Some(alias) = doc.alias()
            && let Some(holder) = model.index().alias_holder(StoreKind::Repo, alias)
            && holder != doc.id()
        {
            return Err(AppError::AliasTaken {
                alias: alias.clone(),
                holder,
            });
        }
        let version = doc
            .recorded()
            .cloned()
            .ok_or(AppError::ClassificationConflict {
                id,
                field: ConflictField::Version,
            })?;
        let text = self
            .ports
            .store()
            .get_object(StoreKind::Private, version.raw())
            .await
            .map_err(AppError::port(PortName::Store))?
            .ok_or_else(|| {
                // A text deleted on purpose is told apart from one that is lost.
                if model.purged(id, version.raw()) {
                    AppError::ObjectPurged {
                        id,
                        raw: *version.raw(),
                    }
                } else {
                    AppError::ObjectMissing {
                        raw: *version.raw(),
                        store: StoreKind::Private,
                    }
                }
            })?;
        let snapshot = ClassSnapshot::new(
            doc.kind(),
            doc.path().clone(),
            doc.alias().cloned(),
            doc.owner().cloned(),
            doc.tags().clone(),
            version,
        );
        let mut draft = self.draft().await?;
        draft.publishing(doc.id());
        draft.object(StoreKind::Repo, text);
        draft.push(
            &model,
            StoreKind::Private,
            doc.id(),
            RecordBody::Published(Publication::Outgoing {
                to: StoreKind::Repo,
            }),
        )?;
        draft.push(
            &model,
            StoreKind::Repo,
            doc.id(),
            RecordBody::Published(Publication::Incoming {
                from: StoreKind::Private,
                snapshot,
            }),
        )?;
        // The items of a Design are registered in the repo store under the
        // identifiers they already have.
        for item in model
            .items()
            .filter(|i| i.key().design.entity() == doc.id())
        {
            draft.push(
                &model,
                StoreKind::Repo,
                item.key().id.entity(),
                RecordBody::Registered(Registration::Item {
                    design: item.key().design,
                    number: item.key().number,
                }),
            )?;
        }
        self.commit(&model, draft).await
    }

    /// Delete a stored text (after a secret slipped through) and record that
    /// it was deleted. The history of the entity stays; only the text goes.
    ///
    /// Objects are keyed by the raw hash, so one text may be a version of several
    /// entities (two files with the same content). The text leaves both stores
    /// whichever entity was named, and a `Purged` is recorded for every entity, in
    /// either store, whose history names that raw hash: none is left with a version
    /// that is missing without a word about it.
    pub async fn purge(
        &mut self,
        target: &Ref,
        raw: ContentHash,
        reason: NonBlank,
    ) -> Result<Committed, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let (id, kind, store) = model.resolve(target)?;
        if kind == EntityKind::Item {
            return Err(AppError::NotADocument { id });
        }
        let id = model.canonical(id);
        model.settled(id)?;
        // The object is keyed by the raw hash and shared by every entity whose
        // history names it, so the deletion is recorded for each of them, in the
        // store each lives in: none is left pointing at a text that is gone with no
        // word about it.
        let naming = model.naming(&raw);
        if !naming.contains(&id) {
            return Err(AppError::NotAVersion { id, raw });
        }
        let mut draft = self.draft().await?;
        for other in naming {
            let home = if other == id {
                store
            } else {
                model.home(other).unwrap_or(store)
            };
            draft.push(
                &model,
                home,
                other,
                RecordBody::Purged {
                    raw,
                    reason: reason.clone(),
                },
            )?;
        }
        let committed = self.commit(&model, draft).await?;
        for scope in [StoreKind::Private, StoreKind::Repo] {
            self.ports
                .store()
                .purge_object(scope, &raw)
                .await
                .map_err(AppError::port(PortName::Store))?;
        }
        Ok(committed)
    }
}
