//! Starting a new document: `finding new`, `spec new`, `design new`.
//!
//! A scaffold creates the file from a [`Template`] (the
//! adapter writes the front matter; this layer knows no file format), never
//! overwriting, and registers it with its alias in the same call, because an alias is
//! classification and lives in events. The file will often not parse yet (a
//! template asks for values only a person can give); that is allowed: it is
//! registered as a version that does not parse, shown as Invalid, and becomes valid
//! when it is filled in and ingested.
//!
//! The file name comes from the alias (`spec/cache-timeout` becomes
//! `<spec_dir>/cache-timeout.md`) or, for a Finding without an alias, from the title.
//! Where the new document lives follows the project's default store; a public file
//! that stands on a private entity is written with the opaque `private:<id>`, so the
//! leak lint has nothing to object to. Which mark a reference carries is the
//! domain's ([`StoreKind::mark_for`]): this writes what it says.

use crate::domain::event::{DocKind, EntityKind, RefMark, StoreKind};
use crate::domain::finding::FindingType;
use crate::domain::id::{Alias, EntityId};
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::reference::Ref;

use super::Workspace;
use super::commit::Committed;
use super::error::AppError;
use super::model::Model;
use super::ports::{CreateOutcome, CurrentPort, PortName, Ports, ScaffoldPort, Template};
use super::register::AddOptions;
use super::view::ref_of;

/// A document that was created and registered.
#[derive(Debug)]
pub struct Scaffolded {
    id: EntityId,
    path: RelPath,
    alias: Alias,
    committed: Committed,
}

impl Scaffolded {
    /// The new identifier.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// Where the file is.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// The alias it was registered with.
    pub fn alias(&self) -> &Alias {
        &self.alias
    }

    /// The write and its subscribers' warnings.
    pub fn committed(&self) -> &Committed {
        &self.committed
    }
}

/// A file-name stem from a title: ASCII letters and digits, words joined by `-`.
fn slug(title: &str) -> Option<String> {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    let out = out.trim_matches('-').to_owned();
    (!out.is_empty()).then_some(out)
}

/// The path of a new document: the directory, then the alias without its
/// kind prefix, `/` turned to `-`.
fn path_in(dir: &RelPath, kind: DocKind, alias: &Alias) -> Result<RelPath, AppError> {
    let prefix = format!("{kind}/");
    let leaf = alias
        .as_str()
        .strip_prefix(&prefix)
        .unwrap_or(alias.as_str());
    Ok(format!("{dir}/{}.md", leaf.replace('/', "-")).parse()?)
}

impl<P: Ports> Workspace<P> {
    /// How the new file names an existing entity: by alias, or by the opaque
    /// identifier when a public file names a private entity.
    fn written_ref(model: &Model, upstream: EntityId, new_store: StoreKind) -> Ref {
        let mark = model
            .home(upstream)
            .map(|home| new_store.mark_for(home))
            .unwrap_or(RefMark::Unmarked);
        match mark {
            RefMark::Private => Ref::Private(upstream),
            RefMark::Unmarked => {
                let alias = model.doc(upstream).and_then(|d| d.alias());
                Ref::Local(ref_of(upstream, alias))
            }
        }
    }

    async fn scaffold(
        &self,
        model: &Model,
        kind: DocKind,
        alias: Alias,
        path: RelPath,
        template: Template,
        store: StoreKind,
    ) -> Result<Scaffolded, AppError> {
        if let Some(holder) = model.index().alias_holder(store, &alias) {
            return Err(AppError::AliasTaken { alias, holder });
        }
        let outcome = self
            .ports
            .scaffold()
            .create_new(&path, &template)
            .await
            .map_err(AppError::port(PortName::Scaffold))?;
        if outcome == CreateOutcome::AlreadyExists {
            return Err(AppError::PathExists { path });
        }
        let candidate = self
            .ports
            .current()
            .read(&path, Some(kind))
            .await
            .map_err(AppError::port(PortName::Current))?
            .ok_or_else(|| AppError::FileMissing { path: path.clone() })?;
        let options = AddOptions::new()
            .with_kind(kind)
            .with_alias(alias.clone())
            .with_store(store);
        let mut draft = self.draft().await?;
        let id = self
            .register_candidate(model, &mut draft, &candidate, &options, store)
            .await?;
        let committed = self.commit(model, draft).await?;
        Ok(Scaffolded {
            id,
            path,
            alias,
            committed,
        })
    }

    /// Start a Finding of a type, with a title. The alias defaults to
    /// `finding/<title as a slug>`.
    pub async fn finding_new(
        &mut self,
        finding_type: FindingType,
        title: NonBlank,
        alias: Option<Alias>,
    ) -> Result<Scaffolded, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let alias = match alias {
            Some(alias) => alias,
            None => {
                let stem = slug(title.as_str()).ok_or(AppError::NoSlug)?;
                format!("finding/{stem}").parse()?
            }
        };
        let path = path_in(self.config.paths().finding_dir(), DocKind::Finding, &alias)?;
        let store = self.config.default_store();
        self.scaffold(
            &model,
            DocKind::Finding,
            alias,
            path,
            Template::Finding {
                finding_type,
                title,
            },
            store,
        )
        .await
    }

    /// Start a Spec that stands on a Finding.
    pub async fn spec_new(&mut self, alias: Alias, from: &Ref) -> Result<Scaffolded, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let finding = model.resolve_as(from, EntityKind::Finding)?;
        let store = self.config.default_store();
        let path = path_in(self.config.paths().spec_dir(), DocKind::Spec, &alias)?;
        let grounds = NonEmptyVec::new(Self::written_ref(&model, finding, store), Vec::new());
        self.scaffold(
            &model,
            DocKind::Spec,
            alias,
            path,
            Template::Spec { grounds },
            store,
        )
        .await
    }

    /// Start a Design for a Spec.
    pub async fn design_new(&mut self, alias: Alias, spec: &Ref) -> Result<Scaffolded, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let spec = model.resolve_as(spec, EntityKind::Spec)?;
        let store = self.config.default_store();
        let path = path_in(self.config.paths().design_dir(), DocKind::Design, &alias)?;
        self.scaffold(
            &model,
            DocKind::Design,
            alias,
            path,
            Template::Design {
                spec: Self::written_ref(&model, spec, store),
            },
            store,
        )
        .await
    }
}
