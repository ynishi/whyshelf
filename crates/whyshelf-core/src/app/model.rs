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
//!   .whyshelf/events/        <data dir>/<project>-<project_id>/events/
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
//!      test files at that revision, what is known of each approved revision
//!      against the baseline (reached, not reached, or cannot be told)  =  ImplView
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
//! domain ([`EntityRef::resolve`](crate::domain::reference::EntityRef::resolve)),
//! and what a resolution comes to for the entity that holds the reference is the
//! domain's too ([`reference::bind`](crate::domain::reference::bind)); the index only answers
//! the questions: a reference without a mark is looked up where the store of its
//! document sees (the rule is [`StoreKind::sees`]: a public document sees only the
//! repo store, a private one both), `private:<id>` is looked up by its identifier in
//! either store, and whether the private store is there at all is asked of the store
//! port once per load. A `private:` identifier that nothing holds, where the private
//! store is not there, leaves the entity that names it *partial*, not invalid. An
//! identifier that the document cannot see anything under makes it Invalid
//! (dangling), even though the model holds an entity of that identifier.
//! Approvals are read from both stores together, because an entity that was
//! published keeps its earlier approvals in the private store.
//!
//! A document whose kind or path two branches wrote differently is held beside the
//! typed entities, not among them ([`ConflictedEntry`]): Invalid, its file unread,
//! and read like any invalid upstream by what stands on it. The state of an upstream
//! is read from [`Model::states`], which has every entity's, such a document's among
//! them.
//!
//! # Building entities from raw files
//!
//! A file is read as a raw value (see [`raw`](super::raw)). Building the domain
//! entity needs its identifier (from the registration), its references resolved
//! (from the index of aliases), and its revisions checked (through the revision
//! port). A file that cannot be built becomes an [`Unparsed`] with the reasons and
//! is evaluated as Invalid; it is never dropped.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::{Actor, Timestamp};
use crate::domain::approval::{ApprovalRecord, ApprovalSet, Attestation};
use crate::domain::classification::{
    ClassificationIssue, ClassificationView, ConflictedDoc, DocClass, ItemClass, Merged, fold,
};
use crate::domain::design::{Design, WorkItem};
use crate::domain::error::EntityError;
use crate::domain::evaluate::{
    Cause, Evaluated, Evaluator, ImplView, ItemEvaluation, ItemKey, States, Unparsed, states_of,
};
use crate::domain::event::{
    DocKind, EntityKind, Event, EventBody, EventId, Publication, Recorded, Registration, StoreKind,
    Version, union_times,
};
use crate::domain::finding::Finding;
use crate::domain::front::{ClassificationImport, Stored};
use crate::domain::hash::{ContentHash, TextHash};
use crate::domain::id::{Alias, DesignId, EntityId, FindingId, ItemId, ItemNumber, SpecId};
use crate::domain::lint::PrivateName;
use crate::domain::location::RelPath;
use crate::domain::naming::Written;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::reference::{
    Bound, ById, Directory, ItemRef, LocalRef, Located, Lookup, Reach, Ref, Resolution, bind,
};
use crate::domain::revision::RevisionId;
use crate::domain::spec::Spec;
use crate::domain::state::{EntityState, InvalidReason, UnparsedReason};
use crate::domain::tag::Tag;

use super::config::Config;
use super::error::AppError;
use super::ports::{
    CodeScanPort, CurrentPort, FileStat, PinScan, PortName, Ports, RevisionPort, StorePort,
    UnreadablePath,
};
use super::raw::{Candidate, ParseOutcome, RawDoc};

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

/// The events of both stores, folded, with the ledger of approvals and the time of
/// every record.
///
/// The fold, the event graph and the ledger do not use time, so the events stay
/// plain [`Event`]s and the times sit beside them, by event. An event that is in
/// both stores is one event, with the times of the records of both.
#[derive(Debug)]
pub(crate) struct Snapshot {
    repo: Side,
    private: Side,
    ledger: ApprovalSet,
    reach: Reach,
    times: BTreeMap<EventId, NonEmptyVec<Timestamp>>,
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
        let reach = ports
            .store()
            .private_reach()
            .await
            .map_err(AppError::port(super::ports::PortName::Store))?;
        Ok(Self::from_events(repo_events, private_events, reach))
    }

    pub(crate) fn from_events(
        repo_recorded: Vec<Recorded>,
        private_recorded: Vec<Recorded>,
        reach: Reach,
    ) -> Self {
        let mut times: BTreeMap<EventId, NonEmptyVec<Timestamp>> = BTreeMap::new();
        for recorded in repo_recorded.iter().chain(private_recorded.iter()) {
            let id = recorded.event().id();
            let merged = match times.get(&id) {
                Some(held) => union_times(&id, held, recorded.at()),
                None => recorded.at().clone(),
            };
            times.insert(id, merged);
        }
        let repo_events: Vec<Event> = repo_recorded
            .into_iter()
            .map(Recorded::into_event)
            .collect();
        let private_events: Vec<Event> = private_recorded
            .into_iter()
            .map(Recorded::into_event)
            .collect();
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
            reach,
            times,
        }
    }

    /// The time of one record of the event, for display: see
    /// [`Recorded::shown_at`]. Every event of the snapshot has one.
    pub(crate) fn shown_at(&self, id: &EventId) -> Option<&Timestamp> {
        self.times.get(id).map(NonEmptyVec::first)
    }

    /// The time of each record of the event, in the order of their digests.
    pub(crate) fn times_of(&self, id: &EventId) -> Option<&NonEmptyVec<Timestamp>> {
        self.times.get(id)
    }

    /// Whether the private store can be read in this environment.
    pub(crate) fn reach(&self) -> Reach {
        self.reach
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

impl Located for Found {
    fn id(&self) -> EntityId {
        self.id
    }

    fn kind(&self) -> EntityKind {
        self.kind
    }
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
///
/// It holds the data and answers [`Directory`]'s questions through
/// [`Index::seen_from`]; the rule that turns the answers into a resolution is the
/// domain's.
#[derive(Debug)]
pub(crate) struct Index {
    entries: BTreeMap<EntityId, Found>,
    /// The documents whose kind or path is in conflict, and the store of each: an
    /// identifier that is there and names no entity of a kind.
    conflicted: BTreeMap<EntityId, StoreKind>,
    aliases: BTreeMap<StoreKind, BTreeMap<Alias, BTreeSet<EntityId>>>,
    canonical: BTreeMap<EntityId, EntityId>,
    reach: Reach,
}

/// The index as a reference in one place sees it.
pub(crate) struct Scoped<'a> {
    index: &'a Index,
    scope: Scope,
}

impl Directory<Found> for Scoped<'_> {
    fn local(&self, reference: &LocalRef) -> Lookup<Found> {
        self.index.lookup(self.scope, reference)
    }

    fn by_id(&self, id: EntityId) -> ById<Found> {
        self.index.by_id(id)
    }

    fn private_reach(&self) -> Reach {
        self.index.reach
    }
}

impl Index {
    pub(crate) fn from_snapshot(snap: &Snapshot) -> Self {
        let mut index = Index {
            entries: BTreeMap::new(),
            conflicted: BTreeMap::new(),
            aliases: BTreeMap::new(),
            canonical: BTreeMap::new(),
            reach: snap.reach(),
        };
        for store in [StoreKind::Repo, StoreKind::Private] {
            index
                .canonical
                .extend(snap.side(store).view.alias_map().clone());
        }
        for (store, class) in snap.docs() {
            // An alias that two branches set differently is not used to find the
            // entity: which it is, is not known.
            if let Some(Some(alias)) = class.alias().agreed() {
                index
                    .aliases
                    .entry(store)
                    .or_default()
                    .entry(alias.clone())
                    .or_default()
                    .insert(class.id());
            }
            match (ConflictedDoc::of(class), class.kind().known()) {
                (None, Some(kind)) => {
                    index.entries.insert(
                        class.id(),
                        Found {
                            id: class.id(),
                            kind: kind.entity_kind(),
                            store,
                        },
                    );
                }
                _ => {
                    index.conflicted.insert(class.id(), store);
                }
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

    /// Whether a reference read from `scope` can see an entity in `store`: what a
    /// document of a store sees is the store's own rule.
    fn visible(scope: Scope, store: StoreKind) -> bool {
        match scope {
            Scope::Everywhere => true,
            Scope::Doc(from) => from.sees(store),
        }
    }

    /// The questions of a reference read from `scope`.
    pub(crate) fn seen_from(&self, scope: Scope) -> Scoped<'_> {
        Scoped { index: self, scope }
    }

    /// What is under an identifier that is a document or an item, as seen from
    /// `scope`.
    fn at(&self, scope: Scope, id: EntityId) -> Lookup<Found> {
        match (self.entries.get(&id), self.conflicted.get(&id)) {
            (Some(found), _) if Self::visible(scope, found.store) => Lookup::Found(*found),
            (_, Some(store)) if Self::visible(scope, *store) => Lookup::Conflicted(id),
            _ => Lookup::Missing,
        }
    }

    fn lookup(&self, scope: Scope, reference: &LocalRef) -> Lookup<Found> {
        match reference {
            LocalRef::Id(id) => self.at(scope, self.canonical.get(id).copied().unwrap_or(*id)),
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
                    (Some(id), None) => self.at(scope, id),
                    (Some(_), Some(_)) => Lookup::Ambiguous,
                }
            }
        }
    }

    /// An entity by its identifier, in whichever store holds it, following the
    /// identifiers a merge made equal. A publication moves an entity to the repo
    /// store and a file keeps writing `private:<id>`, so the store is no part of
    /// the question.
    fn by_id(&self, id: EntityId) -> ById<Found> {
        let id = self.canonical.get(&id).copied().unwrap_or(id);
        match (self.entries.get(&id), self.conflicted.contains_key(&id)) {
            (Some(found), _) => ById::Found(*found),
            (None, true) => ById::Conflicted(id),
            (None, false) => ById::Missing,
        }
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
///
/// A Spec and a Design carry, beside the entity, what the file wrote about its
/// upstream identifiers that the entity cannot hold: with a mark into a store this
/// environment cannot read, or under an identifier the document can see nothing
/// under. The entity holds the identifier, and this is the fact about how it was
/// written that the evaluator needs and the entity must not hold (it would reach the
/// hashes).
#[derive(Debug, Clone)]
pub(crate) enum Built {
    Finding(Finding),
    Spec(Spec, Written),
    Design(Design, Written),
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
            Self::Spec(s, _) => Version::Parsed {
                content: *s.contract_hash(),
                raw,
            },
            Self::Design(d, _) => Version::Parsed {
                content: *d.design_hash(),
                raw,
            },
            Self::Unparsed(_) => Version::Unparsed { raw },
        }
    }
}

/// What a reference in a key comes to, in the scope of the document that wrote it.
///
/// Which lookup is made is the index's; what the answer comes to for the holder is
/// the domain's ([`bind`]).
fn bound(directory: &Scoped<'_>, reference: &Ref, field: &str, expected: EntityKind) -> Bound {
    match reference.entity_of(field) {
        Ok(entity) => bind(entity.resolve(directory), &entity, expected),
        Err(refused) => Bound::Unparsed(refused),
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
        ParseOutcome::Unparsed { reasons, .. } => {
            return unparsed(reasons.iter().cloned().collect());
        }
        ParseOutcome::Parsed(doc) => doc,
    };
    if let Some(expected) = expected
        && doc.kind() != expected
    {
        return unparsed(vec![UnparsedReason::KindChanged {
            registered: expected,
            file: doc.kind(),
        }]);
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
                            return unparsed(vec![UnparsedReason::RevisionRejected {
                                text: text.as_str().to_owned(),
                            }]);
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
            let directory = index.seen_from(Scope::Doc(store));
            let mut grounds = Vec::new();
            let mut written = Written::default();
            let mut reasons = Vec::new();
            for reference in raw.grounds().iter() {
                // An identifier is available in each case but the last: the Spec is
                // built with it, and the evaluator judges what stands behind it. No
                // ground is dropped, which would change the hash in silence.
                match bound(&directory, reference, "grounds", EntityKind::Finding) {
                    Bound::Id(found) | Bound::Conflicted(found) => {
                        grounds.push(FindingId::new(found));
                    }
                    Bound::Dangling(found) => {
                        written.dangling.insert(found);
                        grounds.push(FindingId::new(found));
                    }
                    Bound::Unresolved(unreadable) => {
                        written.unresolved.insert(unreadable);
                        grounds.push(FindingId::new(unreadable));
                    }
                    Bound::Unparsed(reason) => reasons.push(reason),
                }
            }
            if !reasons.is_empty() {
                return unparsed(reasons);
            }
            let Ok(grounds) = NonEmptyVec::from_vec(grounds) else {
                return unparsed(vec![UnparsedReason::EmptyGrounds]);
            };
            match raw.build(SpecId::new(id), grounds) {
                Ok(spec) => Ok(Built::Spec(spec, written)),
                Err(EntityError::DuplicateGround { finding }) => {
                    unparsed(vec![UnparsedReason::DuplicateGround { finding }])
                }
                Err(other) => unparsed(vec![UnparsedReason::InvalidField {
                    field: "grounds".to_owned(),
                    detail: other.to_string(),
                }]),
            }
        }
        RawDoc::Design(raw) => {
            let directory = index.seen_from(Scope::Doc(store));
            let mut written = Written::default();
            let spec = match bound(&directory, raw.spec(), "spec", EntityKind::Spec) {
                Bound::Id(spec) | Bound::Conflicted(spec) => spec,
                Bound::Dangling(spec) => {
                    written.dangling.insert(spec);
                    spec
                }
                Bound::Unresolved(spec) => {
                    written.unresolved.insert(spec);
                    spec
                }
                Bound::Unparsed(reason) => return unparsed(vec![reason]),
            };
            Ok(Built::Design(
                raw.build(DesignId::new(id), SpecId::new(spec)),
                written,
            ))
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
            Built::Spec(s, _) => Self::Spec(s.clone()),
            Built::Design(d, _) => Self::Design(d.clone()),
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
    recorded: Option<Version>,
    present: Option<Present>,
    text: Option<TextHash>,
}

/// A document whose kind or path was set differently on two branches: what is known
/// of it, beside the [`ConflictedDoc`] that decides its state.
///
/// It is not a [`DocEntry`]: there is no kind to read its file as, or no one path to
/// read it at, and so no file is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictedEntry {
    doc: ConflictedDoc,
    store: StoreKind,
    alias: Option<Alias>,
    path: Option<RelPath>,
}

impl ConflictedEntry {
    /// The document and its state.
    pub fn doc(&self) -> &ConflictedDoc {
        &self.doc
    }

    /// The identifier.
    pub fn id(&self) -> EntityId {
        self.doc.id()
    }

    /// The store it lives in.
    pub fn store(&self) -> StoreKind {
        self.store
    }

    /// Its alias, when the branches agree on one.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// Its path, when the branches agree on one.
    pub fn path(&self) -> Option<&RelPath> {
        self.path.as_ref()
    }
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

    /// The version the log records; none while two branches disagree about it.
    pub fn recorded(&self) -> Option<&Version> {
        self.recorded.as_ref()
    }

    /// The file as it is now; `None` when the file is missing.
    pub fn present(&self) -> Option<&Present> {
        self.present.as_ref()
    }

    /// The hash of the normalized text of the file as it was read, whether or not
    /// it built; `None` when the file is missing. What a gate is told it checked.
    pub fn text(&self) -> Option<&TextHash> {
        self.text.as_ref()
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

/// What a reference written by a person names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Resolved<'m> {
    /// An entity of a kind.
    Entity {
        /// The identifier.
        id: EntityId,
        /// The kind.
        kind: EntityKind,
        /// The store it lives in.
        store: StoreKind,
    },
    /// A document whose kind or path is in conflict.
    Conflicted(&'m ConflictedEntry),
}

/// A snapshot of the project at one moment. See the module documentation.
#[derive(Debug)]
pub struct Model {
    snap: Snapshot,
    index: Index,
    docs: BTreeMap<EntityId, DocEntry>,
    conflicted: BTreeMap<EntityId, ConflictedEntry>,
    items: BTreeMap<EntityId, ItemEntry>,
    findings: BTreeMap<FindingId, Evaluated<Finding>>,
    specs: BTreeMap<SpecId, Evaluated<Spec>>,
    designs: BTreeMap<DesignId, Evaluated<Design>>,
    impl_view: ImplView,
    stats: BTreeMap<RelPath, FileStat>,
    unregistered: BTreeSet<RelPath>,
    unreadable_paths: Vec<UnreadablePath>,
    pending_items: BTreeSet<(DesignId, ItemNumber)>,
    file_drift: Vec<(StoreKind, ClassificationIssue)>,
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

/// The revision the working tree stands on: the one place a missing first commit
/// becomes [`AppError::NoCommit`].
pub(crate) async fn current_revision<P: Ports>(ports: &P) -> Result<RevisionId, AppError> {
    ports
        .revision()
        .current()
        .await
        .map_err(AppError::port(PortName::Revision))?
        .ok_or(AppError::NoCommit)
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
    // No scan directory and no `test.files` leave no file that could carry a pin.
    let scan = match config.test_files() {
        Some(files) => ports
            .code_scan()
            .pins_at(
                &revision,
                config.paths().scan_dirs(),
                &files,
                config.test().pin_search(),
            )
            .await
            .map_err(AppError::port(PortName::CodeScan))?,
        None => PinScan::default(),
    };
    let approved: BTreeSet<RevisionId> = ledger
        .active()
        .filter_map(|r| match r.attestation() {
            Attestation::Ca { revision, .. } => Some(revision.base().clone()),
            _ => None,
        })
        .collect();
    let mut integrated = BTreeMap::new();
    for candidate in approved {
        let fact = ports
            .revision()
            .integration(&candidate)
            .await
            .map_err(AppError::port(PortName::Revision))?;
        integrated.insert(candidate, fact);
    }
    Ok(ImplView::new(revision, label, scan.pins, integrated).with_malformed(scan.malformed))
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
        let mut conflicted: BTreeMap<EntityId, ConflictedEntry> = BTreeMap::new();
        let mut file_drift: Vec<(StoreKind, ClassificationIssue)> = Vec::new();
        for (store, class) in snap.docs() {
            // A kind or a path in conflict: the file is not read (as which kind? at
            // which path?) and the document is held beside the typed entities.
            if let Some(doc) = ConflictedDoc::of(class) {
                conflicted.insert(
                    class.id(),
                    ConflictedEntry {
                        doc,
                        store,
                        alias: class.alias().agreed().cloned().flatten(),
                        path: class.path().agreed().cloned(),
                    },
                );
                continue;
            }
            let (Some(kind), Some(path)) = (class.kind().known(), class.path().agreed()) else {
                continue;
            };
            let candidate = ports
                .current()
                .read(path, Some(kind))
                .await
                .map_err(AppError::port(PortName::Current))?;
            // What the file's classification keys say against what the store has.
            if let Some(candidate) = &candidate {
                let import =
                    ClassificationImport::of(candidate.classification(), Stored::Registered(class));
                file_drift.extend(import.drift().iter().map(|drift| {
                    (
                        store,
                        ClassificationIssue::FileDiffers {
                            entity: class.id(),
                            drift: drift.clone(),
                        },
                    )
                }));
            }
            let built = match &candidate {
                None => None,
                Some(candidate) => {
                    let built =
                        build(ports, &index, store, class.id(), Some(kind), candidate).await?;
                    Some((built, candidate.raw_hash()))
                }
            };
            loaded.push(Loaded {
                entry: DocEntry {
                    id: class.id(),
                    kind,
                    store,
                    path: path.clone(),
                    // A value two branches set differently is reported as an issue
                    // and not used: no alias to find it by, no owner.
                    alias: class.alias().agreed().cloned().flatten(),
                    owner: class.owner().agreed().cloned().flatten(),
                    tags: class.tags().clone(),
                    tips: class.tips().clone(),
                    recorded: class.version().agreed().cloned(),
                    present: None,
                    text: candidate.as_ref().map(Candidate::text_hash),
                },
                built,
            });
        }

        // Stage 2: evaluate upstream to downstream. The state of what an entity
        // stands on is read from the state of every entity, the documents in conflict
        // among them.
        let external = |store: StoreKind, id: EntityId| -> Vec<InvalidReason> {
            snap.side(store).view().invalid_reasons(id)
        };
        let conflicted_docs = || conflicted.values().map(ConflictedEntry::doc);
        let (no_specs, no_designs) = (BTreeMap::new(), BTreeMap::new());
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
        let states = states_of(&findings, &no_specs, &no_designs, conflicted_docs());
        for l in &loaded {
            let id = l.entry.id;
            match &l.built {
                Some((Built::Spec(s, written), _)) => {
                    specs.insert(
                        s.id(),
                        Evaluator::spec(
                            s,
                            &findings,
                            &states,
                            &ledger,
                            written,
                            &external(l.entry.store, id),
                        ),
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
        let states = states_of(&findings, &specs, &no_designs, conflicted_docs());
        for l in &loaded {
            let id = l.entry.id;
            match &l.built {
                Some((Built::Design(d, written), _)) => {
                    designs.insert(
                        d.id(),
                        Evaluator::design(
                            d,
                            &specs,
                            &states,
                            &ledger,
                            written,
                            &external(l.entry.store, id),
                        ),
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
        let revision = current_revision(ports).await?;
        let impl_view = impl_view_at(ports, config, revision, &ledger).await?;

        // Items of the Designs that parsed.
        let mut items: BTreeMap<EntityId, ItemEntry> = BTreeMap::new();
        let mut pending_items = BTreeSet::new();
        let states = states_of(&findings, &specs, &designs, conflicted_docs());
        for l in &loaded {
            let Some((Built::Design(design, _), _)) = &l.built else {
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
                    &states,
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
                        hash: item.hash(design.id()),
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
                    Built::Spec(..) | Built::Unparsed(_) if entry.kind == DocKind::Spec => {
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
        let (listed, unreadable_paths) = ports
            .current()
            .list(&roots)
            .await
            .map_err(AppError::port(PortName::Current))?
            .into_parts();
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
            conflicted,
            items,
            findings,
            specs,
            designs,
            impl_view,
            stats,
            unregistered,
            unreadable_paths,
            pending_items,
            file_drift,
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

    /// The document that holds a path, a document in conflict included: one whose kind
    /// is in conflict is at its path, and one whose path is in conflict may be at any
    /// of the paths written, so all of them are held. [`docs`](Self::docs) leaves such
    /// documents out, and so is no way to ask whether a path is free.
    pub fn path_holder(&self, path: &RelPath) -> Option<EntityId> {
        self.snap
            .docs()
            .into_iter()
            .find(|(_, class)| match class.path() {
                Merged::Agreed(held) => held == path,
                Merged::Conflict(held) => held.contains(path),
            })
            .map(|(_, class)| class.id())
    }

    /// The documents whose kind or path is in conflict, by identifier.
    pub fn conflicted(&self) -> impl Iterator<Item = &ConflictedEntry> {
        self.conflicted.values()
    }

    /// One document in conflict, by any of its identifiers.
    pub fn conflicted_doc(&self, id: EntityId) -> Option<&ConflictedEntry> {
        self.conflicted.get(&self.canonical(id))
    }

    /// The state of every entity that has one, whatever its kind: where the state of
    /// an upstream is read.
    pub fn states(&self) -> States<'_> {
        states_of(
            &self.findings,
            &self.specs,
            &self.designs,
            self.conflicted.values().map(ConflictedEntry::doc),
        )
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

    /// Paths the last listing found and could not name, so they are in no other
    /// list here: reported, not dropped.
    pub fn unreadable_paths(&self) -> &[UnreadablePath] {
        &self.unreadable_paths
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

    /// The time of each record of an event, in the order of the digests of the
    /// records (see [`Recorded`]); an event that is in both stores has the times of
    /// both. `None` for an identifier the model has no event of.
    pub fn times_of(&self, id: &EventId) -> Option<&NonEmptyVec<Timestamp>> {
        self.snap.times_of(id)
    }

    /// The time to show for an event: that of one of its records, the first in the
    /// order of the digests (not the earliest). `None` for an identifier the model
    /// has no event of.
    pub fn shown_at(&self, id: &EventId) -> Option<&Timestamp> {
        self.snap.shown_at(id)
    }

    /// The events of one store, each with the time to show for it.
    ///
    /// Every event the model holds came with a time, so none is left out.
    pub(crate) fn timed_events(
        &self,
        store: StoreKind,
    ) -> impl Iterator<Item = (&Event, &Timestamp)> {
        self.events(store)
            .iter()
            .filter_map(|event| self.shown_at(&event.id()).map(|at| (event, at)))
    }

    /// The approvals of the ledger, each with the time to show for it.
    ///
    /// Every approval is an event the model holds, which came with a time, so none
    /// is left out.
    pub(crate) fn timed_approvals(&self) -> impl Iterator<Item = (&ApprovalRecord, &Timestamp)> {
        self.ledger()
            .all()
            .iter()
            .filter_map(|record| self.shown_at(&record.event()).map(|at| (record, at)))
    }

    /// The classification fold of one store.
    pub fn view(&self, store: StoreKind) -> &ClassificationView {
        self.snap.side(store).view()
    }

    /// Classification problems of a store the status should point at: those of the
    /// fold, then each classification key of a file that says something else than
    /// the store.
    pub fn issues(&self, store: StoreKind) -> Vec<ClassificationIssue> {
        let from_files = self
            .file_drift
            .iter()
            .filter(|(side, _)| *side == store)
            .map(|(_, issue)| issue.clone());
        self.snap
            .side(store)
            .view()
            .issues()
            .iter()
            .cloned()
            .chain(from_files)
            .collect()
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
        for doc in self.conflicted.values() {
            if doc.store == StoreKind::Private
                && !except.contains(&doc.id())
                && let Some(alias) = &doc.alias
            {
                out.push(PrivateName::Alias(alias.clone()));
            }
        }
        out
    }

    /// The identifiers of the private documents, which a text for the repo store may
    /// name only as `private:<id>`, leaving out those in `except` (being published).
    pub fn private_ids(&self, except: &BTreeSet<EntityId>) -> Vec<EntityId> {
        self.snap
            .docs()
            .into_iter()
            .filter(|(store, _)| *store == StoreKind::Private)
            .map(|(_, class)| class.id())
            .filter(|id| !except.contains(id))
            .collect()
    }

    /// Whether the classification of a document can be relied on: an error naming
    /// the first value two branches set differently.
    ///
    /// `publish` copies the classification into the repo store and `purge` reads the
    /// versions, so neither goes on over a value they cannot know.
    pub fn settled(&self, id: EntityId) -> Result<(), AppError> {
        let id = self.canonical(id);
        let class = [StoreKind::Repo, StoreKind::Private]
            .into_iter()
            .find_map(|store| self.snap.side(store).view().doc(id));
        match class.and_then(DocClass::conflict) {
            Some(field) => Err(AppError::ClassificationConflict { id, field }),
            None => Ok(()),
        }
    }

    /// The entities, in either store, whose version history names the text `raw`.
    ///
    /// Objects are keyed by the raw hash of the text and shared: two documents with
    /// the same text name the same object, so deleting it concerns every one of them
    /// (see `purge`).
    pub fn naming(&self, raw: &ContentHash) -> BTreeSet<EntityId> {
        let mentions = |v: &Version| v.raw() == raw;
        [StoreKind::Private, StoreKind::Repo]
            .into_iter()
            .flat_map(|store| self.events(store))
            .filter(|e| match e.body() {
                EventBody::Registered(Registration::Doc(d)) => mentions(d.first()),
                EventBody::Revised { old, new } => mentions(old) || mentions(new),
                EventBody::Published(Publication::Incoming { snapshot, .. }) => {
                    mentions(snapshot.version())
                }
                _ => false,
            })
            .map(|e| self.canonical(e.target()))
            .collect()
    }

    /// Whether the text `raw` of an entity was purged: its history says so, in either
    /// store.
    pub fn purged(&self, id: EntityId, raw: &ContentHash) -> bool {
        let id = self.canonical(id);
        [StoreKind::Private, StoreKind::Repo]
            .into_iter()
            .flat_map(|store| self.events(store))
            .any(|e| {
                self.canonical(e.target()) == id
                    && matches!(e.body(), EventBody::Purged { raw: gone, .. } if gone == raw)
            })
    }

    /// Resolve a reference written by a person to an entity of this project.
    ///
    /// The reference is looked up everywhere, in both stores. What cannot be
    /// found is [`AppError::NotFound`] (dangling), [`AppError::Ambiguous`], or
    /// [`AppError::Unresolved`] when the store it points into cannot be read in
    /// this environment; a URL names no entity ([`AppError::NotLocal`]).
    ///
    /// A document whose kind or path is in conflict names no entity of a kind, and
    /// is [`AppError::ClassificationConflict`]; the writes that settle the conflict
    /// (`reclassify`, `mv`) ask `locate` instead, which answers with it.
    pub fn resolve(&self, reference: &Ref) -> Result<(EntityId, EntityKind, StoreKind), AppError> {
        match self.locate(reference)? {
            Resolved::Entity { id, kind, store } => Ok((id, kind, store)),
            Resolved::Conflicted(entry) => Err(AppError::ClassificationConflict {
                id: entry.id(),
                field: entry.doc().field(),
            }),
        }
    }

    /// Resolve a reference to what it names, a document in conflict included.
    pub(crate) fn locate(&self, reference: &Ref) -> Result<Resolved<'_>, AppError> {
        let entity = reference.as_entity().ok_or_else(|| AppError::NotLocal {
            reference: reference.clone(),
        })?;
        match entity.resolve(&self.index.seen_from(Scope::Everywhere)) {
            Resolution::Found(found) => Ok(Resolved::Entity {
                id: found.id,
                kind: found.kind,
                store: found.store,
            }),
            Resolution::Conflicted(id) => self
                .conflicted
                .get(&id)
                .map(Resolved::Conflicted)
                .ok_or_else(|| AppError::NotFound {
                    reference: reference.clone(),
                }),
            Resolution::Dangling => Err(AppError::NotFound {
                reference: reference.clone(),
            }),
            Resolution::Ambiguous => Err(AppError::Ambiguous {
                reference: reference.clone(),
            }),
            Resolution::Unresolved => Err(AppError::Unresolved {
                reference: reference.clone(),
            }),
        }
    }

    /// Resolve a reference that must name an entity of this kind.
    ///
    /// A document in conflict over its kind is taken as the kind asked for when the
    /// kind is among those written: the gate then stops on it as an Invalid target,
    /// instead of this refusing it as another command's business.
    pub fn resolve_as(&self, reference: &Ref, expected: EntityKind) -> Result<EntityId, AppError> {
        match self.locate(reference)? {
            Resolved::Entity { id, kind, .. } if kind == expected => Ok(id),
            Resolved::Entity { kind, .. } => Err(AppError::WrongKind {
                reference: reference.clone(),
                expected,
                found: kind,
            }),
            Resolved::Conflicted(entry) => {
                let kinds = entry.doc().kinds();
                match kinds.iter().find(|kind| kind.entity_kind() == expected) {
                    Some(_) => Ok(entry.id()),
                    None => Err(AppError::WrongKind {
                        reference: reference.clone(),
                        expected,
                        found: kinds
                            .iter()
                            .next()
                            .map_or(expected, |kind| kind.entity_kind()),
                    }),
                }
            }
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
