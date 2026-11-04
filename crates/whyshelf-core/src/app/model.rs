//! The model: everything a use case needs to know, loaded and evaluated once.
//!
//! A [`Model`] is a snapshot of one moment. It holds the events of both stores,
//! the classification folded from them, the document files read and turned into
//! entities, the state of each entity (upstream to downstream, see
//! [`crate::domain::evaluate`]) and what the code and the baseline say about the
//! items. It is derived data: nothing in it is written back, and a use case that
//! writes loads a new one afterwards.
//!
//! # The read path, second half: from events and files to a state
//!
//! [`Model::load`] does all of this, every time; the cache is not consulted (it is
//! used by ingest to find what changed, and by `find` for full-text search). The
//! views and the machine checks read the loaded model and nothing else.
//!
//! ```text
//!   repo store events        private store events
//!   .whyshelf/events/        <data dir>/<project>/events/
//!            \                    /
//!             v                  v
//!   1. Snapshot: each store folded (classification: tags, kind, owner, path,
//!      alias, conflicts), and one ledger of approvals over both stores
//!                              |
//!   2. for each registered doc: read its file (CurrentPort), build the entity,
//!      resolving references through the alias index; a file that cannot be
//!      built stays as Unparsed, with the reasons
//!                              |
//!   3. evaluate, upstream to downstream, each stage taking the stage above:
//!        Findings --> Specs --> Designs          (state on two axes + causes)
//!                              |
//!   4. the code and the baseline (ports): current revision, pins found in the
//!      scan directories at that revision, which approved revisions reached
//!      the baseline  =  ImplView
//!                              |
//!   5. evaluate each item of each Design that parsed (state, integration,
//!      pin progress); a number with no registration yet is "pending"
//!                              |
//!   6. list the document directories again: a file no entity owns is
//!      "unregistered"
//!                              v
//!                           Model
//!                              |
//!        status | get | find | trace | changelog | audit | check_*  (views, pure)
//! ```
//!
//! Step 2 reads every registered file, and step 4 asks Git for the pins and the
//! baseline, each time a model is loaded. A write loads a model before it decides and
//! again after it has written (to tell the subscribers), so a command costs at least
//! one full load.
//!
//! # Two stores, one model
//!
//! The repo store and the private store are folded separately, because the event
//! graph of a store is its own (a publication starts a new root in the repo
//! store). An entity belongs to one *home* store: the repo store if it appears
//! there, else the private store. References are resolved by the rules of the
//! domain: a reference without a mark stays in the entity's own store (a private
//! entity may also name a public one), `private:<id>` reaches the private store.
//! Approvals are read from both stores together, because an entity that was
//! published keeps its earlier approvals in the private store.
//!
//! # Building entities from raw files
//!
//! A file is read as a raw value (see [`raw`](super::raw)). Building the domain
//! entity needs its identifier (from the registration), its references resolved
//! (from the index of aliases), and its revisions checked (through the revision
//! port). A file that cannot be built becomes an [`Unparsed`] with the reasons and
//! is evaluated as Invalid; it is never dropped.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Actor;
use crate::domain::approval::{ApprovalSet, Attestation};
use crate::domain::classification::{
    ClassificationIssue, ClassificationView, DocClass, ItemClass, Merged, fold,
};
use crate::domain::design::{Design, WorkItem};
use crate::domain::evaluate::{
    Cause, Evaluated, Evaluator, ImplView, ItemEvaluation, ItemKey, Unparsed,
};
use crate::domain::event::{DocKind, EntityKind, Event, EventId, StoreKind, Version};
use crate::domain::finding::Finding;
use crate::domain::hash::ContentHash;
use crate::domain::id::{Alias, DesignId, EntityId, FindingId, ItemId, ItemNumber, SpecId};
use crate::domain::lint::PrivateName;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::reference::{ItemRef, LocalRef, Ref};
use crate::domain::revision::RevisionId;
use crate::domain::spec::Spec;
use crate::domain::state::{EntityState, InvalidReason, UnparsedReason};
use crate::domain::tag::Tag;

use super::config::Config;
use super::error::AppError;
use super::ports::{CodeScanPort, CurrentPort, FileStat, Ports, RevisionPort, StorePort};
use super::raw::{Candidate, ParseOutcome, RawDoc};

/// The first value of a field, whether the tips agree or not.
///
/// Where the tips disagree the classification reports the conflict; commands that
/// must act on the entity meanwhile take the smallest value, so the choice is the
/// same for every reader.
pub(crate) fn pick<T: Ord>(merged: &Merged<T>) -> Option<&T> {
    match merged {
        Merged::Agreed(value) => Some(value),
        Merged::Conflict(set) => set.iter().next(),
    }
}

/// The events and folds of one store.
#[derive(Debug)]
pub(crate) struct Side {
    events: Vec<Event>,
    view: ClassificationView,
}

impl Side {
    pub(crate) fn events(&self) -> &[Event] {
        &self.events
    }

    pub(crate) fn view(&self) -> &ClassificationView {
        &self.view
    }
}

/// The events of both stores, folded, with the ledger of approvals.
#[derive(Debug)]
pub(crate) struct Snapshot {
    repo: Side,
    private: Side,
    ledger: ApprovalSet,
}

impl Snapshot {
    pub(crate) async fn load<P: Ports>(ports: &P) -> Result<Self, AppError> {
        let repo_events = ports
            .store()
            .events(StoreKind::Repo)
            .await
            .map_err(AppError::port(super::ports::PortName::Store))?;
        let private_events = ports
            .store()
            .events(StoreKind::Private)
            .await
            .map_err(AppError::port(super::ports::PortName::Store))?;
        Ok(Self::from_events(repo_events, private_events))
    }

    pub(crate) fn from_events(repo_events: Vec<Event>, private_events: Vec<Event>) -> Self {
        // The ledger is read from both stores at once: approvals of a published
        // entity stay in the private store. The union fold is used only for its
        // duplicate registrations (which approval belongs to which survivor).
        let all: Vec<Event> = repo_events
            .iter()
            .chain(private_events.iter())
            .cloned()
            .collect();
        let union = fold(&all);
        let ledger = ApprovalSet::from_events(&all, &union);
        let repo = Side {
            view: fold(&repo_events),
            events: repo_events,
        };
        let private = Side {
            view: fold(&private_events),
            events: private_events,
        };
        Self {
            repo,
            private,
            ledger,
        }
    }

    pub(crate) fn side(&self, store: StoreKind) -> &Side {
        match store {
            StoreKind::Repo => &self.repo,
            StoreKind::Private => &self.private,
        }
    }

    pub(crate) fn ledger(&self) -> &ApprovalSet {
        &self.ledger
    }

    /// The file entities, each once, with the store they live in.
    pub(crate) fn docs(&self) -> Vec<(StoreKind, &DocClass)> {
        let mut out: Vec<(StoreKind, &DocClass)> = self
            .repo
            .view
            .docs()
            .map(|d| (StoreKind::Repo, d))
            .collect();
        out.extend(
            self.private
                .view
                .docs()
                .filter(|d| self.repo.view.doc(d.id()).is_none())
                .map(|d| (StoreKind::Private, d)),
        );
        out
    }

    /// The items, each once, with the store they live in.
    pub(crate) fn items(&self) -> Vec<(StoreKind, &ItemClass)> {
        let mut out: Vec<(StoreKind, &ItemClass)> = self
            .repo
            .view
            .items()
            .map(|i| (StoreKind::Repo, i))
            .collect();
        out.extend(
            self.private
                .view
                .items()
                .filter(|i| self.repo.view.item(i.id().entity()).is_none())
                .map(|i| (StoreKind::Private, i)),
        );
        out
    }

    /// The store an entity lives in: the repo store if it is there at all.
    pub(crate) fn home(&self, id: EntityId) -> Option<StoreKind> {
        [StoreKind::Repo, StoreKind::Private].into_iter().find(|s| {
            let view = &self.side(*s).view;
            view.doc(id).is_some() || view.item(id).is_some()
        })
    }

    /// The tips of the entity's stream in one store.
    pub(crate) fn tips_in(&self, store: StoreKind, id: EntityId) -> BTreeSet<EventId> {
        self.side(store).view.tips(id).cloned().unwrap_or_default()
    }

    /// The event with this identifier and the store it is in.
    pub(crate) fn event(&self, id: &EventId) -> Option<(StoreKind, &Event)> {
        [StoreKind::Repo, StoreKind::Private]
            .into_iter()
            .find_map(|s| {
                self.side(s)
                    .events
                    .iter()
                    .find(|e| &e.id() == id)
                    .map(|e| (s, e))
            })
    }
}

/// What a lookup of a local reference gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) id: EntityId,
    pub(crate) kind: EntityKind,
    pub(crate) store: StoreKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lookup {
    Found(Found),
    Missing,
    Ambiguous,
}

/// Where a reference is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Inside a document of this store: a public document sees only the repo
    /// store, a private one sees its own and the repo store.
    Doc(StoreKind),
    /// From the command line: both stores.
    Everywhere,
}

/// Aliases and identifiers of every entity, for resolving references.
#[derive(Debug, Default)]
pub(crate) struct Index {
    entries: BTreeMap<EntityId, Found>,
    aliases: BTreeMap<StoreKind, BTreeMap<Alias, BTreeSet<EntityId>>>,
    canonical: BTreeMap<EntityId, EntityId>,
}

impl Index {
    pub(crate) fn from_snapshot(snap: &Snapshot) -> Self {
        let mut index = Index::default();
        for store in [StoreKind::Repo, StoreKind::Private] {
            index
                .canonical
                .extend(snap.side(store).view.alias_map().clone());
        }
        for (store, class) in snap.docs() {
            let Some(kind) = pick(class.kind()) else {
                continue;
            };
            index.entries.insert(
                class.id(),
                Found {
                    id: class.id(),
                    kind: kind.entity_kind(),
                    store,
                },
            );
            if let Some(Some(alias)) = pick(class.alias()) {
                index
                    .aliases
                    .entry(store)
                    .or_default()
                    .entry(alias.clone())
                    .or_default()
                    .insert(class.id());
            }
        }
        for (store, class) in snap.items() {
            index.entries.insert(
                class.id().entity(),
                Found {
                    id: class.id().entity(),
                    kind: EntityKind::Item,
                    store,
                },
            );
        }
        index
    }

    fn visible(scope: Scope, store: StoreKind) -> bool {
        match scope {
            Scope::Everywhere => true,
            Scope::Doc(StoreKind::Repo) => store == StoreKind::Repo,
            Scope::Doc(StoreKind::Private) => true,
        }
    }

    pub(crate) fn lookup(&self, scope: Scope, reference: &LocalRef) -> Lookup {
        match reference {
            LocalRef::Id(id) => {
                let id = self.canonical.get(id).copied().unwrap_or(*id);
                match self.entries.get(&id) {
                    Some(found) if Self::visible(scope, found.store) => Lookup::Found(*found),
                    _ => Lookup::Missing,
                }
            }
            LocalRef::Alias(alias) => {
                let mut hits: BTreeSet<EntityId> = BTreeSet::new();
                for (store, by_alias) in &self.aliases {
                    if !Self::visible(scope, *store) {
                        continue;
                    }
                    if let Some(ids) = by_alias.get(alias) {
                        hits.extend(ids.iter().copied());
                    }
                }
                let mut it = hits.into_iter();
                match (it.next(), it.next()) {
                    (None, _) => Lookup::Missing,
                    (Some(id), None) => self
                        .entries
                        .get(&id)
                        .copied()
                        .map_or(Lookup::Missing, Lookup::Found),
                    (Some(_), Some(_)) => Lookup::Ambiguous,
                }
            }
        }
    }

    /// An entity of the private store by its opaque identifier.
    pub(crate) fn lookup_private(&self, id: EntityId) -> Option<Found> {
        let id = self.canonical.get(&id).copied().unwrap_or(id);
        self.entries
            .get(&id)
            .filter(|f| f.store == StoreKind::Private)
            .copied()
    }

    /// Who holds the alias in a store.
    pub(crate) fn alias_holder(&self, store: StoreKind, alias: &Alias) -> Option<EntityId> {
        self.aliases
            .get(&store)
            .and_then(|m| m.get(alias))
            .and_then(|ids| ids.iter().next().copied())
    }
}

/// The result of building an entity from a file.
#[derive(Debug, Clone)]
pub(crate) enum Built {
    Finding(Finding),
    Spec(Spec),
    Design(Design),
    Unparsed(Unparsed),
}

impl Built {
    /// The version this build is: the content hash approvals use and the raw
    /// hash of the text, or the raw hash alone when the file did not build.
    pub(crate) fn version(&self, raw: ContentHash) -> Version {
        match self {
            Self::Finding(f) => Version::Parsed {
                content: *f.hash(),
                raw,
            },
            Self::Spec(s) => Version::Parsed {
                content: *s.contract_hash(),
                raw,
            },
            Self::Design(d) => Version::Parsed {
                content: *d.design_hash(),
                raw,
            },
            Self::Unparsed(_) => Version::Unparsed { raw },
        }
    }
}

fn invalid_field(field: &str, detail: impl Into<String>) -> UnparsedReason {
    UnparsedReason::InvalidField {
        field: field.to_owned(),
        detail: detail.into(),
    }
}

fn resolve_to(
    index: &Index,
    store: StoreKind,
    reference: &Ref,
    field: &str,
    expected: EntityKind,
) -> Result<EntityId, UnparsedReason> {
    let found = match reference {
        Ref::Local(local) => match index.lookup(Scope::Doc(store), local) {
            Lookup::Found(found) => found,
            Lookup::Missing | Lookup::Ambiguous => {
                return Err(UnparsedReason::UnresolvedRef {
                    reference: reference.clone(),
                });
            }
        },
        Ref::Private(id) => match index.lookup_private(*id) {
            Some(found) => found,
            // Not found here (a clone without the private store): the opaque
            // identifier is still the identifier. The evaluator will show the
            // entity that depends on it as having a missing upstream.
            None => return Ok(*id),
        },
        Ref::External { .. } | Ref::Url(_) => {
            return Err(invalid_field(
                field,
                "an external reference cannot stand for an entity of this project",
            ));
        }
    };
    if found.kind == expected {
        Ok(found.id)
    } else {
        Err(invalid_field(
            field,
            format!(
                "`{reference}` is a {}, not a {}",
                found.kind.name(),
                expected.name()
            ),
        ))
    }
}

/// Build the entity a file describes, or the reasons it cannot be built.
///
/// `expected` is the kind the entity was registered as; a file that has become
/// another kind is reported, not silently reinterpreted. Only a failing port is
/// an error here: every problem with the file itself is an [`Unparsed`].
pub(crate) async fn build<P: Ports>(
    ports: &P,
    index: &Index,
    store: StoreKind,
    id: EntityId,
    expected: Option<DocKind>,
    candidate: &Candidate,
) -> Result<Built, AppError> {
    let unparsed = |reasons: Vec<UnparsedReason>| -> Result<Built, AppError> {
        let reasons = NonEmptyVec::from_vec(reasons)?;
        Ok(Built::Unparsed(Unparsed::new(
            candidate.path().clone(),
            reasons,
            candidate.raw_hash(),
        )))
    };
    let doc = match candidate.outcome() {
        ParseOutcome::Unparsed(reasons) => return unparsed(reasons.iter().cloned().collect()),
        ParseOutcome::Parsed(doc) => doc,
    };
    if let Some(expected) = expected
        && doc.kind() != expected
    {
        return unparsed(vec![invalid_field(
            "kind",
            format!("registered as a {expected}, the file is a {}", doc.kind()),
        )]);
    }
    match doc.clone() {
        RawDoc::Finding(raw) => {
            let revision = match raw.revision() {
                None => None,
                Some(text) => {
                    let checked = ports
                        .revision()
                        .validate(text)
                        .await
                        .map_err(AppError::port(super::ports::PortName::Revision))?;
                    match checked {
                        Some(revision) => Some(revision),
                        None => {
                            return unparsed(vec![invalid_field(
                                "revision",
                                format!("`{text}` is not a revision"),
                            )]);
                        }
                    }
                }
            };
            match raw.build(FindingId::new(id), revision) {
                Ok(finding) => Ok(Built::Finding(finding)),
                Err(reason) => unparsed(vec![reason]),
            }
        }
        RawDoc::Spec(raw) => {
            let mut grounds = Vec::new();
            let mut reasons = Vec::new();
            for reference in raw.grounds().iter() {
                match resolve_to(index, store, reference, "grounds", EntityKind::Finding) {
                    Ok(found) => grounds.push(FindingId::new(found)),
                    Err(reason) => reasons.push(reason),
                }
            }
            if !reasons.is_empty() {
                return unparsed(reasons);
            }
            let grounds = NonEmptyVec::from_vec(grounds)?;
            match raw.build(SpecId::new(id), grounds) {
                Ok(spec) => Ok(Built::Spec(spec)),
                Err(error) => unparsed(vec![invalid_field("grounds", error.to_string())]),
            }
        }
        RawDoc::Design(raw) => {
            match resolve_to(index, store, raw.spec(), "spec", EntityKind::Spec) {
                Ok(spec) => Ok(Built::Design(
                    raw.build(DesignId::new(id), SpecId::new(spec)),
                )),
                Err(reason) => unparsed(vec![reason]),
            }
        }
    }
}

/// The content of a document file as the model holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// A Finding.
    Finding(Finding),
    /// A Spec.
    Spec(Spec),
    /// A Design.
    Design(Design),
    /// A file that could not become an entity.
    Unparsed(Unparsed),
}

impl From<&Built> for Content {
    fn from(built: &Built) -> Self {
        match built {
            Built::Finding(f) => Self::Finding(f.clone()),
            Built::Spec(s) => Self::Spec(s.clone()),
            Built::Design(d) => Self::Design(d.clone()),
            Built::Unparsed(u) => Self::Unparsed(u.clone()),
        }
    }
}

impl Content {
    /// The text the cache searches: titles, sentences and summaries.
    pub fn search_text(&self) -> String {
        match self {
            Self::Finding(f) => format!("{}\n{}", f.title(), f.body()),
            Self::Spec(s) => {
                let mut out = s.contract().to_string();
                for (pin, contract) in s.pins() {
                    out.push_str(&format!("\n{pin} {}", contract.as_str()));
                }
                for (hyp, h) in s.hyps() {
                    out.push_str(&format!("\n{hyp} {} {}", h.metric(), h.target()));
                }
                out
            }
            Self::Design(d) => {
                let mut out = format!(
                    "{}\n{}\n{}",
                    d.crux().question(),
                    d.crux().chosen().summary(),
                    d.crux().chosen().reason()
                );
                for rejected in d.crux().rejected().iter() {
                    out.push_str(&format!("\n{}", rejected.summary()));
                }
                for item in d.items().values() {
                    out.push_str(&format!("\n{}", item.summary()));
                }
                out
            }
            Self::Unparsed(_) => String::new(),
        }
    }

    /// The entities this one stands on, as the file names them.
    pub fn upstream(&self) -> Vec<EntityId> {
        match self {
            Self::Spec(s) => s.grounds().iter().map(|g| g.entity()).collect(),
            Self::Design(d) => vec![d.spec().entity()],
            Self::Finding(_) | Self::Unparsed(_) => Vec::new(),
        }
    }
}

/// A document file that is there: what it holds and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Present {
    content: Content,
    state: EntityState,
    causes: Vec<Cause>,
    raw: ContentHash,
}

impl Present {
    /// What the file holds.
    pub fn content(&self) -> &Content {
        &self.content
    }

    /// The two-axis state.
    pub fn state(&self) -> &EntityState {
        &self.state
    }

    /// Where a Stale or Blocked came from, root cause first.
    pub fn causes(&self) -> &[Cause] {
        &self.causes
    }

    /// The `raw` hash of the file's text.
    pub fn raw(&self) -> &ContentHash {
        &self.raw
    }
}

/// A document the store knows, with what the file says now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    id: EntityId,
    kind: DocKind,
    store: StoreKind,
    path: RelPath,
    alias: Option<Alias>,
    owner: Option<Actor>,
    tags: BTreeSet<Tag>,
    tips: BTreeSet<EventId>,
    recorded: Version,
    present: Option<Present>,
}

impl DocEntry {
    /// The identifier.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// The kind it was registered as.
    pub fn kind(&self) -> DocKind {
        self.kind
    }

    /// The store it lives in.
    pub fn store(&self) -> StoreKind {
        self.store
    }

    /// Its path.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// Its alias, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// Its owner, if any.
    pub fn owner(&self) -> Option<&Actor> {
        self.owner.as_ref()
    }

    /// Its tags.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The tips of its stream in its store.
    pub fn tips(&self) -> &BTreeSet<EventId> {
        &self.tips
    }

    /// The version the log records.
    pub fn recorded(&self) -> &Version {
        &self.recorded
    }

    /// The file as it is now; `None` when the file is missing.
    pub fn present(&self) -> Option<&Present> {
        self.present.as_ref()
    }

    /// The title, for a Finding that parsed.
    pub fn title(&self) -> Option<&str> {
        match self.present.as_ref().map(|p| &p.content) {
            Some(Content::Finding(f)) => Some(f.title().as_str()),
            _ => None,
        }
    }
}

/// A work item of a Design, with its evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemEntry {
    key: ItemKey,
    store: StoreKind,
    item: WorkItem,
    hash: ContentHash,
    tips: BTreeSet<EventId>,
    evaluation: ItemEvaluation,
}

impl ItemEntry {
    /// The item's identifier, Design and number.
    pub fn key(&self) -> &ItemKey {
        &self.key
    }

    /// The store it lives in.
    pub fn store(&self) -> StoreKind {
        self.store
    }

    /// The item as the Design says.
    pub fn item(&self) -> &WorkItem {
        &self.item
    }

    /// The hash gate CA approves.
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }

    /// The tips of its stream in its store.
    pub fn tips(&self) -> &BTreeSet<EventId> {
        &self.tips
    }

    /// The state, integration and progress.
    pub fn evaluation(&self) -> &ItemEvaluation {
        &self.evaluation
    }
}

/// A snapshot of the project at one moment. See the module documentation.
#[derive(Debug)]
pub struct Model {
    snap: Snapshot,
    index: Index,
    docs: BTreeMap<EntityId, DocEntry>,
    items: BTreeMap<EntityId, ItemEntry>,
    findings: BTreeMap<FindingId, Evaluated<Finding>>,
    specs: BTreeMap<SpecId, Evaluated<Spec>>,
    designs: BTreeMap<DesignId, Evaluated<Design>>,
    impl_view: ImplView,
    stats: BTreeMap<RelPath, FileStat>,
    unregistered: BTreeSet<RelPath>,
    pending_items: BTreeSet<(DesignId, ItemNumber)>,
}

/// The paths a listing must cover: the watched and document directories and every
/// registered path (a file may be registered from anywhere).
pub(crate) fn listing_roots(
    config: &Config,
    registered: impl Iterator<Item = RelPath>,
) -> Vec<RelPath> {
    let paths = config.paths();
    let mut roots: BTreeSet<RelPath> = paths.watch_dirs().iter().cloned().collect();
    roots.insert(paths.finding_dir().clone());
    roots.insert(paths.spec_dir().clone());
    roots.insert(paths.design_dir().clone());
    roots.extend(registered);
    roots.into_iter().collect()
}

/// What the code and the baseline say, for evaluating items.
pub(crate) async fn impl_view_at<P: Ports>(
    ports: &P,
    config: &Config,
    revision: RevisionId,
    ledger: &ApprovalSet,
) -> Result<ImplView, AppError> {
    use super::ports::PortName;
    let label = ports
        .revision()
        .label()
        .await
        .map_err(AppError::port(PortName::Revision))?;
    let pins = ports
        .code_scan()
        .pins_at(
            &revision,
            config.paths().scan_dirs(),
            config.test().pin_pattern(),
        )
        .await
        .map_err(AppError::port(PortName::CodeScan))?;
    let approved: BTreeSet<RevisionId> = ledger
        .active()
        .filter_map(|r| match r.attestation() {
            Attestation::Ca { revision, .. } => Some(revision.base().clone()),
            _ => None,
        })
        .collect();
    let mut integrated = BTreeSet::new();
    for candidate in approved {
        let reached = ports
            .revision()
            .is_integrated(&candidate)
            .await
            .map_err(AppError::port(PortName::Revision))?;
        if reached {
            integrated.insert(candidate);
        }
    }
    Ok(ImplView::new(revision, label, pins, integrated))
}

impl Model {
    /// Load the events, read the files, build and evaluate everything.
    pub async fn load<P: Ports>(ports: &P, config: &Config) -> Result<Self, AppError> {
        use super::ports::PortName;
        let snap = Snapshot::load(ports).await?;
        let index = Index::from_snapshot(&snap);
        let ledger = snap.ledger().clone();

        // Stage 1: read each registered file and build its entity.
        struct Loaded {
            entry: DocEntry,
            built: Option<(Built, ContentHash)>,
        }
        let mut loaded: Vec<Loaded> = Vec::new();
        for (store, class) in snap.docs() {
            let (Some(kind), Some(path), Some(version)) = (
                pick(class.kind()),
                pick(class.path()),
                pick(class.version()),
            ) else {
                continue;
            };
            let candidate = ports
                .current()
                .read(path)
                .await
                .map_err(AppError::port(PortName::Current))?;
            let built = match &candidate {
                None => None,
                Some(candidate) => {
                    let built =
                        build(ports, &index, store, class.id(), Some(*kind), candidate).await?;
                    Some((built, candidate.raw_hash()))
                }
            };
            loaded.push(Loaded {
                entry: DocEntry {
                    id: class.id(),
                    kind: *kind,
                    store,
                    path: path.clone(),
                    alias: pick(class.alias()).cloned().flatten(),
                    owner: pick(class.owner()).cloned().flatten(),
                    tags: class.tags().clone(),
                    tips: class.tips().clone(),
                    recorded: version.clone(),
                    present: None,
                },
                built,
            });
        }

        // Stage 2: evaluate upstream to downstream.
        let external = |store: StoreKind, id: EntityId| -> Vec<InvalidReason> {
            snap.side(store).view().invalid_reasons(id)
        };
        let mut findings: BTreeMap<FindingId, Evaluated<Finding>> = BTreeMap::new();
        for l in &loaded {
            let id = l.entry.id;
            match &l.built {
                Some((Built::Finding(f), _)) => {
                    findings.insert(
                        f.id(),
                        Evaluator::finding(f, &ledger, &external(l.entry.store, id)),
                    );
                }
                Some((Built::Unparsed(u), _)) if l.entry.kind == DocKind::Finding => {
                    findings.insert(
                        FindingId::new(id),
                        Evaluated::from_unparsed(u.clone(), &external(l.entry.store, id)),
                    );
                }
                _ => {}
            }
        }
        let mut specs: BTreeMap<SpecId, Evaluated<Spec>> = BTreeMap::new();
        for l in &loaded {
            let id = l.entry.id;
            match &l.built {
                Some((Built::Spec(s), _)) => {
                    specs.insert(
                        s.id(),
                        Evaluator::spec(s, &findings, &ledger, &external(l.entry.store, id)),
                    );
                }
                Some((Built::Unparsed(u), _)) if l.entry.kind == DocKind::Spec => {
                    specs.insert(
                        SpecId::new(id),
                        Evaluated::from_unparsed(u.clone(), &external(l.entry.store, id)),
                    );
                }
                _ => {}
            }
        }
        let mut designs: BTreeMap<DesignId, Evaluated<Design>> = BTreeMap::new();
        for l in &loaded {
            let id = l.entry.id;
            match &l.built {
                Some((Built::Design(d), _)) => {
                    designs.insert(
                        d.id(),
                        Evaluator::design(d, &specs, &ledger, &external(l.entry.store, id)),
                    );
                }
                Some((Built::Unparsed(u), _)) if l.entry.kind == DocKind::Design => {
                    designs.insert(
                        DesignId::new(id),
                        Evaluated::from_unparsed(u.clone(), &external(l.entry.store, id)),
                    );
                }
                _ => {}
            }
        }

        // The code and the baseline.
        let revision = ports
            .revision()
            .current()
            .await
            .map_err(AppError::port(PortName::Revision))?;
        let impl_view = impl_view_at(ports, config, revision, &ledger).await?;

        // Items of the Designs that parsed.
        let mut items: BTreeMap<EntityId, ItemEntry> = BTreeMap::new();
        let mut pending_items = BTreeSet::new();
        for l in &loaded {
            let Some((Built::Design(design), _)) = &l.built else {
                continue;
            };
            let view = snap.side(l.entry.store).view();
            for (number, item) in design.items() {
                let Some(item_id) = view.item_id(design.id(), *number) else {
                    pending_items.insert((design.id(), *number));
                    continue;
                };
                let key = ItemKey {
                    id: item_id,
                    design: design.id(),
                    number: *number,
                };
                let evaluation = Evaluator::item(
                    &key,
                    item,
                    &designs,
                    &ledger,
                    &impl_view,
                    &external(l.entry.store, item_id.entity()),
                );
                items.insert(
                    item_id.entity(),
                    ItemEntry {
                        key,
                        store: l.entry.store,
                        item: item.clone(),
                        hash: item.hash(design.id(), *number),
                        tips: snap.tips_in(l.entry.store, item_id.entity()),
                        evaluation,
                    },
                );
            }
        }

        // Assemble the document entries from the evaluations.
        let mut docs: BTreeMap<EntityId, DocEntry> = BTreeMap::new();
        for l in loaded {
            let mut entry = l.entry;
            if let Some((built, raw)) = l.built {
                let id = entry.id;
                let (content, state, causes) = match built {
                    Built::Finding(_) | Built::Unparsed(_) if entry.kind == DocKind::Finding => {
                        let ev = findings.get(&FindingId::new(id));
                        pack(ev, |f: &Finding| Content::Finding(f.clone()))
                    }
                    Built::Spec(_) | Built::Unparsed(_) if entry.kind == DocKind::Spec => {
                        let ev = specs.get(&SpecId::new(id));
                        pack(ev, |s: &Spec| Content::Spec(s.clone()))
                    }
                    _ => {
                        let ev = designs.get(&DesignId::new(id));
                        pack(ev, |d: &Design| Content::Design(d.clone()))
                    }
                };
                if let (Some(content), Some(state)) = (content, state) {
                    entry.present = Some(Present {
                        content,
                        state,
                        causes,
                        raw,
                    });
                }
            }
            docs.insert(entry.id, entry);
        }

        // Files that no entity owns.
        let roots = listing_roots(config, docs.values().map(|d| d.path.clone()));
        let listed = ports
            .current()
            .list(&roots)
            .await
            .map_err(AppError::port(PortName::Current))?;
        let owned: BTreeSet<&RelPath> = snap
            .docs()
            .into_iter()
            .flat_map(|(_, c)| match c.path() {
                Merged::Agreed(p) => vec![p],
                Merged::Conflict(set) => set.iter().collect(),
            })
            .collect();
        let stats: BTreeMap<RelPath, FileStat> =
            listed.into_iter().map(|s| (s.path().clone(), s)).collect();
        let unregistered = stats
            .keys()
            .filter(|p| !owned.contains(p))
            .cloned()
            .collect();

        Ok(Self {
            snap,
            index,
            docs,
            items,
            findings,
            specs,
            designs,
            impl_view,
            stats,
            unregistered,
            pending_items,
        })
    }

    /// The document entries, by identifier.
    pub fn docs(&self) -> impl Iterator<Item = &DocEntry> {
        self.docs.values()
    }

    /// One document entry, by any of its identifiers.
    pub fn doc(&self, id: EntityId) -> Option<&DocEntry> {
        let id = self.canonical(id);
        self.docs.get(&id)
    }

    /// The work items, by identifier.
    pub fn items(&self) -> impl Iterator<Item = &ItemEntry> {
        self.items.values()
    }

    /// One work item, by any of its identifiers.
    pub fn item(&self, id: ItemId) -> Option<&ItemEntry> {
        self.items.get(&self.canonical(id.entity()))
    }

    /// The surviving identifier of an entity.
    pub fn canonical(&self, id: EntityId) -> EntityId {
        self.index.canonical.get(&id).copied().unwrap_or(id)
    }

    /// The evaluated Findings.
    pub fn findings(&self) -> &BTreeMap<FindingId, Evaluated<Finding>> {
        &self.findings
    }

    /// The evaluated Specs.
    pub fn specs(&self) -> &BTreeMap<SpecId, Evaluated<Spec>> {
        &self.specs
    }

    /// The evaluated Designs.
    pub fn designs(&self) -> &BTreeMap<DesignId, Evaluated<Design>> {
        &self.designs
    }

    /// The approvals of both stores.
    pub fn ledger(&self) -> &ApprovalSet {
        &self.snap.ledger
    }

    /// What the code and the baseline said when the model was loaded.
    pub fn impl_view(&self) -> &ImplView {
        &self.impl_view
    }

    /// The files seen by the last listing, with their stamps.
    pub fn stats(&self) -> &BTreeMap<RelPath, FileStat> {
        &self.stats
    }

    /// Files nobody registered, in path order. Never taken in silently.
    pub fn unregistered(&self) -> &BTreeSet<RelPath> {
        &self.unregistered
    }

    /// Item numbers in a Design that no registration has been written for yet
    /// (the next ingest registers them).
    pub fn pending_items(&self) -> &BTreeSet<(DesignId, ItemNumber)> {
        &self.pending_items
    }

    /// The events of one store.
    pub fn events(&self, store: StoreKind) -> &[Event] {
        self.snap.side(store).events()
    }

    /// The classification fold of one store.
    pub fn view(&self, store: StoreKind) -> &ClassificationView {
        self.snap.side(store).view()
    }

    /// Classification problems of a store the status should point at.
    pub fn issues(&self, store: StoreKind) -> &[ClassificationIssue] {
        self.snap.side(store).view().issues()
    }

    /// The store an entity lives in.
    pub fn home(&self, id: EntityId) -> Option<StoreKind> {
        self.snap.home(id)
    }

    /// The store that holds an event.
    pub fn store_of_event(&self, id: &EventId) -> Option<StoreKind> {
        self.snap.event(id).map(|(s, _)| s)
    }

    /// The tips of an entity's stream in one store: what a write there lists
    /// as its heads.
    pub fn tips_in(&self, store: StoreKind, id: EntityId) -> BTreeSet<EventId> {
        self.snap.tips_in(store, id)
    }

    pub(crate) fn index(&self) -> &Index {
        &self.index
    }

    /// The aliases and titles of private entities, for the leak lint, leaving
    /// out the entities in `except` (those being published).
    pub fn private_names(&self, except: &BTreeSet<EntityId>) -> Vec<PrivateName> {
        let mut out = Vec::new();
        for doc in self.docs.values() {
            if doc.store != StoreKind::Private || except.contains(&doc.id) {
                continue;
            }
            if let Some(alias) = &doc.alias {
                out.push(PrivateName::Alias(alias.clone()));
            }
            if let Some(Present {
                content: Content::Finding(f),
                ..
            }) = &doc.present
            {
                out.push(PrivateName::Title(f.title().clone()));
            }
        }
        out
    }

    /// Resolve a reference written by a person to an entity of this project.
    pub fn resolve(&self, reference: &Ref) -> Result<(EntityId, EntityKind, StoreKind), AppError> {
        let found = match reference {
            Ref::Local(local) => match self.index.lookup(Scope::Everywhere, local) {
                Lookup::Found(found) => found,
                Lookup::Missing => {
                    return Err(AppError::NotFound {
                        reference: reference.clone(),
                    });
                }
                Lookup::Ambiguous => {
                    return Err(AppError::Ambiguous {
                        reference: reference.clone(),
                    });
                }
            },
            Ref::Private(id) => {
                self.index
                    .lookup_private(*id)
                    .ok_or_else(|| AppError::NotFound {
                        reference: reference.clone(),
                    })?
            }
            Ref::External { .. } | Ref::Url(_) => {
                return Err(AppError::NotLocal {
                    reference: reference.clone(),
                });
            }
        };
        Ok((found.id, found.kind, found.store))
    }

    /// Resolve a reference that must name an entity of this kind.
    pub fn resolve_as(&self, reference: &Ref, expected: EntityKind) -> Result<EntityId, AppError> {
        let (id, kind, _) = self.resolve(reference)?;
        if kind == expected {
            Ok(id)
        } else {
            Err(AppError::WrongKind {
                reference: reference.clone(),
                expected,
                found: kind,
            })
        }
    }

    /// Resolve `<design-ref>#<n>` or `<spec-ref>#<n>` to a registered item.
    pub fn resolve_item(&self, reference: &ItemRef) -> Result<ItemId, AppError> {
        let base = Ref::Local(reference.base().clone());
        let (id, kind, store) = self.resolve(&base)?;
        let design = match kind {
            EntityKind::Design => DesignId::new(id),
            EntityKind::Spec => {
                let spec = SpecId::new(id);
                let designs: Vec<DesignId> = self
                    .docs
                    .values()
                    .filter_map(|d| match d.present.as_ref().map(|p| &p.content) {
                        Some(Content::Design(design)) if design.spec() == spec => Some(design.id()),
                        _ => None,
                    })
                    .collect();
                ItemRef::design_of_spec(&designs)?
            }
            found => {
                return Err(AppError::WrongKind {
                    reference: base,
                    expected: EntityKind::Design,
                    found,
                });
            }
        };
        let home = self.home(design.entity()).unwrap_or(store);
        self.snap
            .side(home)
            .view()
            .item_id(design, reference.number())
            .ok_or_else(|| AppError::ItemNotFound {
                item: reference.clone(),
            })
    }
}

fn pack<T: Clone>(
    ev: Option<&Evaluated<T>>,
    wrap: impl Fn(&T) -> Content,
) -> (Option<Content>, Option<EntityState>, Vec<Cause>) {
    match ev {
        None => (None, None, Vec::new()),
        Some(ev) => {
            let content = match (ev.entity(), ev.unparsed()) {
                (Some(entity), _) => Some(wrap(entity)),
                (None, Some(unparsed)) => Some(Content::Unparsed(unparsed.clone())),
                (None, None) => None,
            };
            (content, Some(ev.state().clone()), ev.causes().to_vec())
        }
    }
}
