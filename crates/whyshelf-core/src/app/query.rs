//! Asking: `get` one entity, `find` many.
//!
//! [`Workspace::get`] returns any kind of entity with its content and its
//! classification together, and resolves every kind of reference: an alias, an
//! identifier, `private:<id>` (looked up by its identifier in either store, shown as
//! unresolved when nothing holds it and this environment has no private store),
//! `ext:<project>/<id>` (another project, which this layer does not look into, so
//! it is always shown as unresolved), a URL (an
//! outside document, returned as is), and `<design-ref>#<n>` for an item, also
//! `<spec-ref>#<n>` when exactly one Design refers to that Spec.
//!
//! [`Workspace::find`] evaluates one parsed [`Query`] (see
//! [`crate::domain::query`] for the language) over the model. Full-text terms are
//! answered by the cache, everything else from the model itself, so the query
//! language never depends on how the cache is laid out. A result page has a
//! `limit` (default 50, at most 200), an opaque `cursor` to continue from, and a set
//! of `fields` to return, so that a tool server cannot flood a context: results are
//! ordered by identifier (UUID v7, so roughly by creation), and the cursor is the last
//! identifier returned.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;

use crate::domain::actor::Actor;
use crate::domain::approval::ApprovalRecord;
use crate::domain::design::{Design, WorkItem};
use crate::domain::evaluate::{Cause, Unparsed};
use crate::domain::event::{EntityKind, EventId, StoreKind};
use crate::domain::finding::Finding;
use crate::domain::hash::ContentHash;
use crate::domain::id::{Alias, DesignId, EntityId, ItemNumber};
use crate::domain::location::{RelPath, Uri};
use crate::domain::nonempty::NonBlank;
use crate::domain::query::{Query, Term};
use crate::domain::reference::{ItemRef, Ref};
use crate::domain::spec::Spec;
use crate::domain::state::{EntityState, Integration, ItemProgress};
use crate::domain::tag::Tag;

use super::Workspace;
use super::error::AppError;
use super::model::{Content, DocEntry, ItemEntry, Model};
use super::ports::{IndexCachePort, PortName, Ports};

/// What a command names: an entity, or an item by `<ref>#<n>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A reference to an entity.
    Entity(Ref),
    /// An item of a Design.
    Item(ItemRef),
}

impl FromStr for Target {
    type Err = AppError;

    /// A text with `#` (that is not a URL) is an item reference; anything else
    /// is a plain reference.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains('#') && !s.contains("://") {
            Ok(Self::Item(s.parse()?))
        } else {
            Ok(Self::Entity(s.parse()?))
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entity(r) => fmt::Display::fmt(r, f),
            Self::Item(i) => fmt::Display::fmt(i, f),
        }
    }
}

/// The content of an entity as `get` returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityContent {
    /// A Finding.
    Finding(Finding),
    /// A Spec.
    Spec(Spec),
    /// A Design.
    Design(Design),
    /// A file that did not parse.
    Unparsed(Unparsed),
    /// The file is not there any more.
    FileMissing,
    /// A work item.
    Item {
        /// Its Design.
        design: DesignId,
        /// Its number there.
        number: ItemNumber,
        /// What the Design says.
        item: WorkItem,
        /// The hash gate CA approves.
        hash: ContentHash,
        /// Integration into the baseline.
        integration: Integration,
        /// Pins found and declared.
        progress: ItemProgress,
    },
}

/// One entity, content and classification together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityView {
    id: EntityId,
    kind: EntityKind,
    store: StoreKind,
    path: Option<RelPath>,
    alias: Option<Alias>,
    owner: Option<Actor>,
    tags: BTreeSet<Tag>,
    tips: BTreeSet<EventId>,
    content: EntityContent,
    state: Option<EntityState>,
    causes: Vec<Cause>,
    approvals: Vec<ApprovalRecord>,
}

impl EntityView {
    /// The identifier.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// The kind.
    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    /// The store it lives in.
    pub fn store(&self) -> StoreKind {
        self.store
    }

    /// The path; none for an item.
    pub fn path(&self) -> Option<&RelPath> {
        self.path.as_ref()
    }

    /// The alias, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The owner, if any.
    pub fn owner(&self) -> Option<&Actor> {
        self.owner.as_ref()
    }

    /// The tags.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The tips of its stream: the heads of the next write.
    pub fn tips(&self) -> &BTreeSet<EventId> {
        &self.tips
    }

    /// The content.
    pub fn content(&self) -> &EntityContent {
        &self.content
    }

    /// The two axes; none when the file is missing.
    pub fn state(&self) -> Option<&EntityState> {
        self.state.as_ref()
    }

    /// Where a Stale or Blocked came from, root cause first.
    pub fn causes(&self) -> &[Cause] {
        &self.causes
    }

    /// The approvals recorded for it, withdrawn ones included.
    pub fn approvals(&self) -> &[ApprovalRecord] {
        &self.approvals
    }
}

/// What `get` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetResult {
    /// An entity of this project.
    Entity(Box<EntityView>),
    /// A `private:` or `ext:` reference this environment cannot look up. Shown,
    /// never an error.
    Unresolved(Ref),
    /// A URL: an outside document, not looked up.
    External(Uri),
}

fn doc_view(model: &Model, doc: &DocEntry) -> EntityView {
    let (content, state, causes) = match doc.present() {
        None => (EntityContent::FileMissing, None, Vec::new()),
        Some(p) => (
            match p.content() {
                Content::Finding(f) => EntityContent::Finding(f.clone()),
                Content::Spec(s) => EntityContent::Spec(s.clone()),
                Content::Design(d) => EntityContent::Design(d.clone()),
                Content::Unparsed(u) => EntityContent::Unparsed(u.clone()),
            },
            Some(p.state().clone()),
            p.causes().to_vec(),
        ),
    };
    EntityView {
        id: doc.id(),
        kind: doc.kind().entity_kind(),
        store: doc.store(),
        path: Some(doc.path().clone()),
        alias: doc.alias().cloned(),
        owner: doc.owner().cloned(),
        tags: doc.tags().clone(),
        tips: doc.tips().clone(),
        content,
        state,
        causes,
        approvals: approvals_of(model, doc.id()),
    }
}

fn item_view(model: &Model, item: &ItemEntry) -> EntityView {
    let key = item.key();
    EntityView {
        id: key.id.entity(),
        kind: EntityKind::Item,
        store: item.store(),
        path: None,
        alias: None,
        owner: None,
        tags: BTreeSet::new(),
        tips: item.tips().clone(),
        content: EntityContent::Item {
            design: key.design,
            number: key.number,
            item: item.item().clone(),
            hash: *item.hash(),
            integration: item.evaluation().integration().clone(),
            progress: item.evaluation().progress().clone(),
        },
        state: Some(item.evaluation().state().clone()),
        causes: item.evaluation().causes().to_vec(),
        approvals: approvals_of(model, key.id.entity()),
    }
}

fn approvals_of(model: &Model, id: EntityId) -> Vec<ApprovalRecord> {
    let id = model.canonical(id);
    model
        .ledger()
        .all()
        .iter()
        .filter(|r| r.subject() == id)
        .cloned()
        .collect()
}

fn entity_view(model: &Model, id: EntityId, reference: &Ref) -> Result<EntityView, AppError> {
    if let Some(doc) = model.doc(id) {
        return Ok(doc_view(model, doc));
    }
    let id = model.canonical(id);
    model
        .items()
        .find(|i| i.key().id.entity() == id)
        .map(|i| item_view(model, i))
        .ok_or_else(|| AppError::NotFound {
            reference: reference.clone(),
        })
}

/// How many rows a page holds at most.
///
/// Zero is not a page, and a very large page defeats the purpose, so the
/// constructor clamps into `1..=MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLimit(NonZeroUsize);

impl PageLimit {
    /// The page size when the caller gives none.
    pub const DEFAULT: usize = 50;
    /// The largest page.
    pub const MAX: usize = 200;

    /// A limit, clamped into `1..=MAX`.
    pub fn new(limit: usize) -> Self {
        let clamped = limit.clamp(1, Self::MAX);
        Self(NonZeroUsize::new(clamped).unwrap_or(NonZeroUsize::MIN))
    }

    /// The limit.
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

impl Default for PageLimit {
    fn default() -> Self {
        Self::new(Self::DEFAULT)
    }
}

/// Where to continue a listing: the last identifier of the previous page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor(EntityId);

impl Cursor {
    /// The identifier the next page starts after.
    pub fn after(&self) -> EntityId {
        self.0
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for Cursor {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

/// A column a caller may ask `find` to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    /// The store.
    Store,
    /// The path.
    Path,
    /// The alias.
    Alias,
    /// The title of a Finding, the summary of an item.
    Title,
    /// The tags.
    Tags,
    /// The owner.
    Owner,
    /// The two axes of the state.
    State,
}

/// A page request. The default asks for 50 rows from the start with every field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageRequest {
    /// The page size.
    pub limit: PageLimit,
    /// Continue after this row.
    pub cursor: Option<Cursor>,
    /// The fields to return; empty means all. `id` and `kind` always come.
    pub fields: BTreeSet<Field>,
}

/// One row of a `find` result. A field that was not asked for is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundRow {
    /// The identifier.
    pub id: EntityId,
    /// The kind.
    pub kind: EntityKind,
    /// The store.
    pub store: Option<StoreKind>,
    /// The path.
    pub path: Option<RelPath>,
    /// The alias.
    pub alias: Option<Alias>,
    /// The title or summary (free text).
    pub title: Option<String>,
    /// The tags.
    pub tags: Option<BTreeSet<Tag>>,
    /// The owner.
    pub owner: Option<Actor>,
    /// The state.
    pub state: Option<EntityState>,
}

/// A page of results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundPage {
    rows: Vec<FoundRow>,
    next: Option<Cursor>,
    total: usize,
}

impl FoundPage {
    /// The rows of this page.
    pub fn rows(&self) -> &[FoundRow] {
        &self.rows
    }

    /// Where the next page starts; none on the last page.
    pub fn next(&self) -> Option<Cursor> {
        self.next
    }

    /// How many entities matched in all.
    pub fn total(&self) -> usize {
        self.total
    }
}

/// An entity as the query sees it: documents and items alike.
struct Row<'m> {
    id: EntityId,
    kind: EntityKind,
    store: StoreKind,
    doc: Option<&'m DocEntry>,
    item: Option<&'m ItemEntry>,
    state: Option<&'m EntityState>,
    upstream: Vec<EntityId>,
}

fn rows(model: &Model) -> Vec<Row<'_>> {
    let mut out = Vec::new();
    for doc in model.docs() {
        out.push(Row {
            id: doc.id(),
            kind: doc.kind().entity_kind(),
            store: doc.store(),
            doc: Some(doc),
            item: None,
            state: doc.present().map(|p| p.state()),
            upstream: doc
                .present()
                .map(|p| p.content().upstream())
                .unwrap_or_default(),
        });
    }
    for item in model.items() {
        out.push(Row {
            id: item.key().id.entity(),
            kind: EntityKind::Item,
            store: item.store(),
            doc: None,
            item: Some(item),
            state: Some(item.evaluation().state()),
            upstream: vec![item.key().design.entity()],
        });
    }
    out
}

fn resolve_query_ref(
    model: &Model,
    r: &crate::domain::reference::LocalRef,
) -> Result<EntityId, AppError> {
    model.resolve(&Ref::Local(r.clone())).map(|(id, _, _)| id)
}

impl<P: Ports> Workspace<P> {
    /// One entity of any kind, by any reference.
    pub async fn get(&self, target: &Target) -> Result<GetResult, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        match target {
            Target::Item(item_ref) => {
                let id = model.resolve_item(item_ref)?;
                let reference = Ref::Local(item_ref.base().clone());
                let view = entity_view(&model, id.entity(), &reference)?;
                Ok(GetResult::Entity(Box::new(view)))
            }
            Target::Entity(Ref::Url(uri)) => Ok(GetResult::External(uri.clone())),
            Target::Entity(reference) => match model.resolve(reference) {
                Ok((id, _, _)) => Ok(GetResult::Entity(Box::new(entity_view(
                    &model, id, reference,
                )?))),
                // What the domain's rule calls unresolved is shown, not an error.
                Err(AppError::Unresolved { reference }) => Ok(GetResult::Unresolved(reference)),
                Err(other) => Err(other),
            },
        }
    }

    /// Every path whose text has the words. The port answers at most the limit it is
    /// given, so an answer that fills the limit may be cut: ask again for more until
    /// one falls short. Cutting it silently would make `total` wrong.
    async fn every_hit(&self, text: &NonBlank) -> Result<BTreeSet<RelPath>, AppError> {
        let mut limit = 1000;
        loop {
            let hits = self
                .ports
                .cache()
                .search_fts(text, limit)
                .await
                .map_err(AppError::port(PortName::IndexCache))?;
            if hits.len() < limit {
                return Ok(hits.into_iter().collect());
            }
            limit = limit.saturating_mul(4);
        }
    }

    /// The identifiers the query selects.
    pub(crate) async fn matching_ids(
        &self,
        model: &Model,
        query: &Query,
    ) -> Result<BTreeSet<EntityId>, AppError> {
        // Full text first: it is the only term that needs a port.
        let mut text_paths: BTreeMap<usize, BTreeSet<RelPath>> = BTreeMap::new();
        for (at, term) in query.terms().iter().enumerate() {
            if let Term::Text(text) = term {
                text_paths.insert(at, self.every_hit(text).await?);
            }
        }
        let all = rows(model);
        let referenced: BTreeSet<EntityId> = all
            .iter()
            .flat_map(|r| r.upstream.iter().copied())
            .collect();
        // Terms that name another entity are resolved once.
        let mut named: BTreeMap<usize, EntityId> = BTreeMap::new();
        for (at, term) in query.terms().iter().enumerate() {
            match term {
                Term::Grounds(r) | Term::Upstream(r) => {
                    named.insert(at, resolve_query_ref(model, r)?);
                }
                _ => {}
            }
        }
        let mut out = BTreeSet::new();
        'rows: for row in &all {
            for (at, term) in query.terms().iter().enumerate() {
                let ok = match term {
                    Term::Tag(tag) => row.doc.is_some_and(|d| d.tags().contains(tag)),
                    Term::Kind(kind) => row.kind == *kind,
                    Term::State(filter) => row.state.is_some_and(|s| filter.matches(s)),
                    Term::Owner(owner) => row.doc.is_some_and(|d| d.owner() == Some(owner)),
                    Term::Grounds(_) => {
                        row.kind == EntityKind::Spec
                            && named.get(&at).is_some_and(|f| row.upstream.contains(f))
                    }
                    Term::Upstream(_) => named.get(&at).is_some_and(|u| row.upstream.contains(u)),
                    Term::Orphan(wanted) => {
                        // An item is held by its Design, so it is never an orphan.
                        let held = row.item.is_some() || referenced.contains(&row.id);
                        held != *wanted
                    }
                    Term::Text(text) => match (row.doc, row.item) {
                        (Some(doc), _) => text_paths
                            .get(&at)
                            .is_some_and(|paths| paths.contains(doc.path())),
                        (None, Some(item)) => {
                            contains_ignoring_case(item.item().summary().as_str(), text)
                        }
                        (None, None) => false,
                    },
                };
                if !ok {
                    continue 'rows;
                }
            }
            out.insert(row.id);
        }
        Ok(out)
    }

    /// The entities a query selects, a page at a time.
    pub async fn find(&self, query: &Query, page: &PageRequest) -> Result<FoundPage, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let ids = self.matching_ids(&model, query).await?;
        let total = ids.len();
        let limit = page.limit.get();
        let after = page.cursor.map(|c| c.after());
        let mut chosen: Vec<EntityId> = ids
            .into_iter()
            .filter(|id| after.is_none_or(|a| *id > a))
            .take(limit + 1)
            .collect();
        let next = if chosen.len() > limit {
            chosen.truncate(limit);
            chosen.last().map(|id| Cursor(*id))
        } else {
            None
        };
        let wants = |f: Field| page.fields.is_empty() || page.fields.contains(&f);
        let all = rows(&model);
        let rows = chosen
            .iter()
            .filter_map(|id| all.iter().find(|r| r.id == *id))
            .map(|r| FoundRow {
                id: r.id,
                kind: r.kind,
                store: wants(Field::Store).then_some(r.store),
                path: r
                    .doc
                    .filter(|_| wants(Field::Path))
                    .map(|d| d.path().clone()),
                alias: r
                    .doc
                    .filter(|_| wants(Field::Alias))
                    .and_then(|d| d.alias().cloned()),
                title: if wants(Field::Title) {
                    r.doc
                        .and_then(|d| d.title().map(str::to_owned))
                        .or_else(|| r.item.map(|i| i.item().summary().as_str().to_owned()))
                } else {
                    None
                },
                tags: r
                    .doc
                    .filter(|_| wants(Field::Tags))
                    .map(|d| d.tags().clone()),
                owner: r
                    .doc
                    .filter(|_| wants(Field::Owner))
                    .and_then(|d| d.owner().cloned()),
                state: if wants(Field::State) {
                    r.state.cloned()
                } else {
                    None
                },
            })
            .collect();
        Ok(FoundPage { rows, next, total })
    }
}

fn contains_ignoring_case(haystack: &str, needle: &NonBlank) -> bool {
    haystack
        .to_lowercase()
        .contains(&needle.as_str().trim().to_lowercase())
}
