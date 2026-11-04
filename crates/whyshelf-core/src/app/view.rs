//! The views: status, trace, changelog, audit and the context line.
//!
//! A view is derived from the model and the events each time it is asked for, and
//! returned as a value. Rendering it (a table, JSON, Markdown) is the caller's job.
//!
//! | view | answers |
//! |---|---|
//! | [`StatusReport`] | for every entity: the approval axis, the upstream axis, the integration state, how many pins are found of those declared, and the command to run next; plus files nobody registered, files that vanished, and generated files that drifted |
//! | [`TraceReport`] | the chain Finding to Spec to Design to item to approved revision, around one entity |
//! | [`Changelog`] | the events of one entity, in the order of its event graph |
//! | [`AuditReport`] | who approved which version at which revision, and when, and whether it was withdrawn |
//! | [`ContextLine`] | `[context: project=.., worktree=.., revision=..]`, the first line of every answer of a tool server, so a caller cannot confuse the target |
//!
//! The next command is a typed [`NextAction`], never a sentence built from state:
//! the caller prints it, a tool server returns it, and the rules for choosing it live
//! in one function.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::actor::{Actor, Timestamp};
use crate::domain::approval::{ApprovalRecord, Attestation, CmResult, Gate, TestOutcome};
use crate::domain::classification::ClassificationIssue;
use crate::domain::evaluate::Cause;
use crate::domain::event::{DocKind, EntityKind, Event, EventId, StoreKind};
use crate::domain::hash::ContentHash;
use crate::domain::id::{Alias, DesignId, EntityId, ItemNumber};
use crate::domain::lint::{ExportDrift, export_drift};
use crate::domain::location::RelPath;
use crate::domain::query::Query;
use crate::domain::reference::{ItemRef, LocalRef, ProjectName, Ref};
use crate::domain::revision::{Revision, RevisionId};
use crate::domain::state::{Approval, EntityState, Integration, Upstream};

use super::Workspace;
use super::config::Config;
use super::error::AppError;
use super::model::{Content, DocEntry, ItemEntry, Model};
use super::ports::{
    ExportDoc, ExportFile, ExportKind, PortName, Ports, RevisionPort, StorePort, WorktreeRoot,
};
use super::projection::{ChangelogDoc, MapDoc};
use super::query::Target;

/// The line that opens every answer of a tool server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLine {
    project: ProjectName,
    worktree: WorktreeRoot,
    revision: RevisionId,
    label: Option<String>,
}

impl ContextLine {
    /// The project's name.
    pub fn project(&self) -> &ProjectName {
        &self.project
    }

    /// The working directory.
    pub fn worktree(&self) -> &WorktreeRoot {
        &self.worktree
    }

    /// The current revision.
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    /// A name for the line of work (a branch, say), display only.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}

impl fmt::Display for ContextLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[context: project={}, worktree={}, revision={}",
            self.project, self.worktree, self.revision
        )?;
        if let Some(label) = &self.label {
            write!(f, " ({label})")?;
        }
        f.write_str("]")
    }
}

/// Ask the revision port for the context.
pub(crate) async fn context_line<P: Ports>(
    ports: &P,
    config: &Config,
) -> Result<ContextLine, AppError> {
    let revision = ports
        .revision()
        .current()
        .await
        .map_err(AppError::port(PortName::Revision))?;
    let worktree = ports
        .revision()
        .worktree()
        .await
        .map_err(AppError::port(PortName::Revision))?;
    let label = ports
        .revision()
        .label()
        .await
        .map_err(AppError::port(PortName::Revision))?;
    Ok(ContextLine {
        project: config.project().clone(),
        worktree,
        revision,
        label,
    })
}

/// The answer of [`Workspace::context`]: where the tool is pointed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextReport {
    line: ContextLine,
    default_store: StoreKind,
    repo_events: usize,
    private_events: usize,
}

impl ContextReport {
    /// The context line.
    pub fn line(&self) -> &ContextLine {
        &self.line
    }

    /// Where a new document goes unless told otherwise.
    pub fn default_store(&self) -> StoreKind {
        self.default_store
    }

    /// How many events the store holds.
    pub fn events_in(&self, store: StoreKind) -> usize {
        match store {
            StoreKind::Repo => self.repo_events,
            StoreKind::Private => self.private_events,
        }
    }
}

/// The reference to print for an entity: its alias if it has one, else its
/// identifier.
pub(crate) fn ref_of(id: EntityId, alias: Option<&Alias>) -> Ref {
    Ref::Local(match alias {
        Some(alias) => LocalRef::Alias(alias.clone()),
        None => LocalRef::Id(id),
    })
}

fn ref_of_doc(doc: &DocEntry) -> Ref {
    ref_of(doc.id(), doc.alias())
}

fn gate_of(kind: DocKind) -> Gate {
    match kind {
        DocKind::Finding => Gate::Cr,
        DocKind::Spec => Gate::Cs,
        DocKind::Design => Gate::Ci,
    }
}

/// The command to run next for an entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// Nothing is waiting on a person.
    None,
    /// An unregistered file: register it.
    Register {
        /// The file.
        path: RelPath,
    },
    /// The file is missing: put it back, or `mv` the entity to where it went.
    RestoreFile,
    /// The file is Invalid: fix it.
    FixFile,
    /// Run the check of the gate (and approve if it passes).
    Approve {
        /// The gate.
        gate: Gate,
        /// The entity.
        target: Target,
    },
    /// The approval no longer holds (edited, or an upstream changed): run the
    /// check again.
    Reapprove {
        /// The gate.
        gate: Gate,
        /// The entity.
        target: Target,
    },
    /// Something upstream must be approved first.
    ApproveUpstream {
        /// The upstream that stands in the way.
        upstream: Ref,
    },
    /// The item's tests do not yet carry its pins.
    Implement,
    /// Approved at a revision that has not reached the baseline.
    AwaitIntegration,
    /// Every item is integrated: record the measurement.
    AwaitingCm {
        /// The Spec.
        target: Ref,
    },
}

impl fmt::Display for NextAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let gate = |g: &Gate| g.name().to_ascii_lowercase();
        match self {
            Self::None => f.write_str("-"),
            Self::Register { path } => write!(f, "whyshelf add {path}"),
            Self::RestoreFile => f.write_str("file missing: restore it or whyshelf mv"),
            Self::FixFile => f.write_str("fix the file"),
            Self::Approve { gate: g, target } => write!(f, "whyshelf check {} {target}", gate(g)),
            Self::Reapprove { gate: g, target } => {
                write!(f, "changed: whyshelf check {} {target}", gate(g))
            }
            Self::ApproveUpstream { upstream } => write!(f, "approve upstream {upstream} first"),
            Self::Implement => f.write_str("implementing"),
            Self::AwaitIntegration => f.write_str("awaiting integration"),
            Self::AwaitingCm { target } => write!(f, "awaiting CM: whyshelf check cm {target}"),
        }
    }
}

/// How many of the declared pins the tests carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinTally {
    found: usize,
    declared: usize,
}

impl PinTally {
    /// Pins found in the tests.
    pub fn found(&self) -> usize {
        self.found
    }

    /// Pins declared.
    pub fn declared(&self) -> usize {
        self.declared
    }
}

/// One entity in the status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    id: EntityId,
    kind: EntityKind,
    store: StoreKind,
    alias: Option<Alias>,
    path: Option<RelPath>,
    number: Option<ItemNumber>,
    parent: Option<EntityId>,
    label: Option<String>,
    state: Option<EntityState>,
    integration: Option<Integration>,
    pins: Option<PinTally>,
    implementing: bool,
    next: NextAction,
}

impl StatusRow {
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

    /// The alias, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The path; none for an item.
    pub fn path(&self) -> Option<&RelPath> {
        self.path.as_ref()
    }

    /// The number inside its Design, for an item.
    pub fn number(&self) -> Option<ItemNumber> {
        self.number
    }

    /// The Design, for an item.
    pub fn parent(&self) -> Option<EntityId> {
        self.parent
    }

    /// The title of a Finding or the summary of an item (free text).
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// The two axes; none when the file is missing.
    pub fn state(&self) -> Option<&EntityState> {
        self.state.as_ref()
    }

    /// The integration state, for an item.
    pub fn integration(&self) -> Option<&Integration> {
        self.integration.as_ref()
    }

    /// Pins found of those declared, for a Spec or an item.
    pub fn pins(&self) -> Option<PinTally> {
        self.pins
    }

    /// Whether the item's tests do not yet carry its pins (shown as
    /// implementing; never Invalid).
    pub fn implementing(&self) -> bool {
        self.implementing
    }

    /// The command to run next.
    pub fn next(&self) -> &NextAction {
        &self.next
    }
}

/// A classification problem in one store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreIssue {
    /// The store.
    pub store: StoreKind,
    /// The problem.
    pub issue: ClassificationIssue,
}

/// What is wrong with a generated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportProblem {
    /// The file is not there.
    Missing,
    /// Somebody edited the file; rebuild writes it again.
    EditedByHand,
    /// The file is behind its sources (after a merge, say); rebuild fixes it.
    Outdated,
}

/// A generated file that is not as it should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportNote {
    /// The store.
    pub scope: StoreKind,
    /// Which file.
    pub kind: ExportKind,
    /// What is wrong.
    pub problem: ExportProblem,
}

/// The status of every entity, and everything else the status points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    context: ContextLine,
    rows: Vec<StatusRow>,
    unregistered: Vec<RelPath>,
    missing: Vec<RelPath>,
    changed_since_ingest: Vec<RelPath>,
    issues: Vec<StoreIssue>,
    exports: Vec<ExportNote>,
}

impl StatusReport {
    /// The context line.
    pub fn context(&self) -> &ContextLine {
        &self.context
    }

    /// One row per entity: documents in kind order, each Design followed by its
    /// items.
    pub fn rows(&self) -> &[StatusRow] {
        &self.rows
    }

    /// Files nobody registered, each with `whyshelf add` as its next action.
    pub fn unregistered(&self) -> &[RelPath] {
        &self.unregistered
    }

    /// The next action of an unregistered file.
    pub fn next_for_unregistered(path: &RelPath) -> NextAction {
        NextAction::Register { path: path.clone() }
    }

    /// Registered files that are no longer there.
    pub fn missing(&self) -> &[RelPath] {
        &self.missing
    }

    /// Registered files whose text differs from the last recorded version: the
    /// next ingest records them.
    pub fn changed_since_ingest(&self) -> &[RelPath] {
        &self.changed_since_ingest
    }

    /// Conflicts and collisions the person must settle.
    pub fn issues(&self) -> &[StoreIssue] {
        &self.issues
    }

    /// Generated files that drifted.
    pub fn exports(&self) -> &[ExportNote] {
        &self.exports
    }

    /// The status without the private entities, for a file that is committed.
    pub fn public_part(&self) -> StatusReport {
        let public: BTreeSet<EntityId> = self
            .rows
            .iter()
            .filter(|r| r.store == StoreKind::Repo)
            .map(|r| r.id)
            .collect();
        let paths: BTreeSet<&RelPath> = self
            .rows
            .iter()
            .filter(|r| r.store == StoreKind::Repo)
            .filter_map(|r| r.path.as_ref())
            .collect();
        StatusReport {
            context: self.context.clone(),
            rows: self
                .rows
                .iter()
                .filter(|r| public.contains(&r.id))
                .cloned()
                .collect(),
            unregistered: self.unregistered.clone(),
            missing: self
                .missing
                .iter()
                .filter(|p| paths.contains(p))
                .cloned()
                .collect(),
            changed_since_ingest: self
                .changed_since_ingest
                .iter()
                .filter(|p| paths.contains(p))
                .cloned()
                .collect(),
            issues: self
                .issues
                .iter()
                .filter(|i| i.store == StoreKind::Repo)
                .cloned()
                .collect(),
            exports: self
                .exports
                .iter()
                .filter(|e| e.scope == StoreKind::Repo)
                .copied()
                .collect(),
        }
    }

    /// Derive the status from a model.
    pub fn build(model: &Model, context: ContextLine) -> StatusReport {
        let mut docs: Vec<&DocEntry> = model.docs().collect();
        docs.sort_by(|a, b| {
            (a.kind(), a.alias(), a.path(), a.id()).cmp(&(b.kind(), b.alias(), b.path(), b.id()))
        });
        let mut rows = Vec::new();
        let mut missing = Vec::new();
        let mut changed = Vec::new();
        for doc in docs {
            if doc.present().is_none() {
                missing.push(doc.path().clone());
            } else if doc.present().map(|p| p.raw()) != Some(doc.recorded().raw()) {
                changed.push(doc.path().clone());
            }
            rows.push(doc_row(model, doc));
            if let Some(Content::Design(design)) = doc.present().map(|p| p.content()) {
                let mut items: Vec<&ItemEntry> = model
                    .items()
                    .filter(|i| i.key().design == design.id())
                    .collect();
                items.sort_by_key(|i| i.key().number);
                rows.extend(items.into_iter().map(|i| item_row(model, doc, i)));
            }
        }
        let mut issues = Vec::new();
        for store in [StoreKind::Repo, StoreKind::Private] {
            issues.extend(model.issues(store).iter().map(|issue| StoreIssue {
                store,
                issue: issue.clone(),
            }));
        }
        StatusReport {
            context,
            rows,
            unregistered: model.unregistered().iter().cloned().collect(),
            missing,
            changed_since_ingest: changed,
            issues,
            exports: Vec::new(),
        }
    }

    fn with_exports(mut self, exports: Vec<ExportNote>) -> Self {
        self.exports = exports;
        self
    }
}

fn pin_tally(model: &Model, spec: &crate::domain::spec::Spec) -> PinTally {
    PinTally {
        found: spec
            .pins()
            .keys()
            .filter(|p| model.impl_view().has_pin(p))
            .count(),
        declared: spec.pins().len(),
    }
}

/// Whether every item of every Design of the Spec is integrated, and there is at
/// least one, and no measurement was recorded for the current hypotheses.
fn awaits_cm(model: &Model, spec: &crate::domain::spec::Spec) -> bool {
    let designs: BTreeSet<DesignId> = model
        .docs()
        .filter_map(|d| match d.present().map(|p| p.content()) {
            Some(Content::Design(design)) if design.spec() == spec.id() => Some(design.id()),
            _ => None,
        })
        .collect();
    let items: Vec<&ItemEntry> = model
        .items()
        .filter(|i| designs.contains(&i.key().design))
        .collect();
    let all_integrated = !items.is_empty()
        && items
            .iter()
            .all(|i| matches!(i.evaluation().integration(), Integration::Integrated { .. }));
    let measured = model.ledger().active().any(|r| {
        matches!(r.attestation(), Attestation::Cm { spec: s, .. }
            if s.id() == &spec.id() && s.hash() == spec.hyp_hash())
    });
    all_integrated && !measured
}

fn next_from_state(
    model: &Model,
    gate: Gate,
    target: Target,
    state: &EntityState,
    causes: &[Cause],
) -> Option<NextAction> {
    match (state.approval(), state.upstream()) {
        (Approval::Invalid(_), _) => Some(NextAction::FixFile),
        (_, Upstream::Blocked(_)) => {
            let upstream = causes
                .first()
                .map(|c| c.via())
                .and_then(|id| model.doc(id))
                .map(ref_of_doc)?;
            Some(NextAction::ApproveUpstream { upstream })
        }
        (Approval::Modified { .. }, _) | (_, Upstream::Stale(_)) => {
            Some(NextAction::Reapprove { gate, target })
        }
        (Approval::Draft, _) => Some(NextAction::Approve { gate, target }),
        (Approval::Approved, Upstream::Ok) => None,
    }
}

fn doc_row(model: &Model, doc: &DocEntry) -> StatusRow {
    let target = Target::Entity(ref_of_doc(doc));
    let present = doc.present();
    let next = match present {
        None => NextAction::RestoreFile,
        Some(p) => next_from_state(model, gate_of(doc.kind()), target, p.state(), p.causes())
            .unwrap_or_else(|| match p.content() {
                Content::Spec(spec) if awaits_cm(model, spec) => NextAction::AwaitingCm {
                    target: ref_of_doc(doc),
                },
                _ => NextAction::None,
            }),
    };
    let pins = match present.map(|p| p.content()) {
        Some(Content::Spec(spec)) => Some(pin_tally(model, spec)),
        _ => None,
    };
    StatusRow {
        id: doc.id(),
        kind: doc.kind().entity_kind(),
        store: doc.store(),
        alias: doc.alias().cloned(),
        path: Some(doc.path().clone()),
        number: None,
        parent: None,
        label: doc.title().map(str::to_owned),
        state: present.map(|p| p.state().clone()),
        integration: None,
        pins,
        implementing: false,
        next,
    }
}

fn item_row(model: &Model, design: &DocEntry, item: &ItemEntry) -> StatusRow {
    let key = item.key();
    let evaluation = item.evaluation();
    let design_ref = ref_of_doc(design);
    let target = format!("{design_ref}{}", key.number)
        .parse::<ItemRef>()
        .map(Target::Item)
        .unwrap_or_else(|_| Target::Entity(design_ref.clone()));
    let progress = evaluation.progress();
    let next = next_from_state(
        model,
        Gate::Ca,
        target,
        evaluation.state(),
        evaluation.causes(),
    )
    .map(|action| match action {
        NextAction::Approve { .. } if progress.is_implementing() => NextAction::Implement,
        other => other,
    })
    .unwrap_or_else(|| match evaluation.integration() {
        Integration::ApprovedPending { .. } => NextAction::AwaitIntegration,
        _ => NextAction::None,
    });
    StatusRow {
        id: key.id.entity(),
        kind: EntityKind::Item,
        store: item.store(),
        alias: None,
        path: None,
        number: Some(key.number),
        parent: Some(key.design.entity()),
        label: Some(item.item().summary().as_str().to_owned()),
        state: Some(evaluation.state().clone()),
        integration: Some(evaluation.integration().clone()),
        pins: Some(PinTally {
            found: progress.found().len(),
            declared: progress.declared().len(),
        }),
        implementing: progress.is_implementing(),
        next,
    }
}

/// One entity in a trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceNode {
    id: EntityId,
    kind: EntityKind,
    alias: Option<Alias>,
    path: Option<RelPath>,
    number: Option<ItemNumber>,
    state: Option<EntityState>,
    approved: Vec<ApprovedRevision>,
}

impl TraceNode {
    /// The identifier.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// The kind, which is also its stage.
    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    /// The alias, if any.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The path; none for an item.
    pub fn path(&self) -> Option<&RelPath> {
        self.path.as_ref()
    }

    /// The number, for an item.
    pub fn number(&self) -> Option<ItemNumber> {
        self.number
    }

    /// The two axes; none when the file is missing.
    pub fn state(&self) -> Option<&EntityState> {
        self.state.as_ref()
    }

    /// For an item: the revisions approved at its current content.
    pub fn approved(&self) -> &[ApprovedRevision] {
        &self.approved
    }
}

/// A revision a CA approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedRevision {
    /// The approval event.
    pub event: EventId,
    /// The revision.
    pub revision: Revision,
    /// Who approved.
    pub by: Actor,
    /// Whether the revision reached the baseline.
    pub integrated: bool,
}

/// The chain around one entity: everything it stands on and everything that
/// stands on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceReport {
    target: EntityId,
    nodes: Vec<TraceNode>,
    links: BTreeSet<(EntityId, EntityId)>,
}

impl TraceReport {
    /// The entity traced.
    pub fn target(&self) -> EntityId {
        self.target
    }

    /// The nodes, upstream stage first (Finding, Spec, Design, item).
    pub fn nodes(&self) -> &[TraceNode] {
        &self.nodes
    }

    /// The edges as `(upstream, downstream)`.
    pub fn links(&self) -> &BTreeSet<(EntityId, EntityId)> {
        &self.links
    }
}

/// One line of a changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    store: StoreKind,
    event: Event,
}

impl ChangeEntry {
    /// The store the event is in.
    pub fn store(&self) -> StoreKind {
        self.store
    }

    /// The event.
    pub fn event(&self) -> &Event {
        &self.event
    }
}

/// The history of one entity, from its events.
///
/// Ordered by the event graph (an event after the ones it saw), never by time; the
/// time is on each event for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changelog {
    entity: EntityId,
    entries: Vec<ChangeEntry>,
}

impl Changelog {
    /// The entity.
    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> &[ChangeEntry] {
        &self.entries
    }

    pub(crate) fn of(model: &Model, id: EntityId) -> Changelog {
        let id = model.canonical(id);
        let mut entries = Vec::new();
        // The private store's history comes first: a published entity was
        // private before it was public.
        for store in [StoreKind::Private, StoreKind::Repo] {
            let events: Vec<&Event> = model
                .events(store)
                .iter()
                .filter(|e| model.canonical(e.target()) == id)
                .collect();
            let by_id: BTreeMap<EventId, &Event> = events.iter().map(|e| (e.id(), *e)).collect();
            let mut depth: BTreeMap<EventId, usize> = BTreeMap::new();
            // Heads form a DAG; resolve depth by repeated passes (bounded by the
            // number of events, so a malformed cycle cannot loop forever).
            for _ in 0..=events.len() {
                for e in &events {
                    let d = e
                        .heads()
                        .iter()
                        .filter(|h| by_id.contains_key(*h))
                        .map(|h| depth.get(h).copied().unwrap_or(0) + 1)
                        .max()
                        .unwrap_or(0);
                    depth.insert(e.id(), d);
                }
            }
            let mut ordered = events;
            ordered.sort_by_key(|e| (depth.get(&e.id()).copied().unwrap_or(0), e.id()));
            entries.extend(ordered.into_iter().map(|e| ChangeEntry {
                store,
                event: e.clone(),
            }));
        }
        Changelog {
            entity: id,
            entries,
        }
    }
}

/// One approval in the audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    event: EventId,
    gate: Gate,
    subject: EntityId,
    by: Actor,
    at: Timestamp,
    hash: ContentHash,
    revision: Option<Revision>,
    tests: Option<TestOutcome>,
    result: Option<CmResult>,
    revoked: bool,
}

impl AuditRow {
    fn of(record: &ApprovalRecord, revoked: bool) -> Self {
        let (revision, tests, result) = match record.attestation() {
            Attestation::Ca {
                revision, tests, ..
            } => (Some(revision.clone()), Some(*tests), None),
            Attestation::Cm { result, .. } => (None, None, Some(result.clone())),
            _ => (None, None, None),
        };
        Self {
            event: record.event(),
            gate: record.attestation().gate(),
            subject: record.subject(),
            by: record.by().clone(),
            at: record.at().clone(),
            hash: *record.attestation().target_hash(),
            revision,
            tests,
            result,
            revoked,
        }
    }

    /// The `Approved` event.
    pub fn event(&self) -> EventId {
        self.event
    }

    /// The gate.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// The entity approved.
    pub fn subject(&self) -> EntityId {
        self.subject
    }

    /// Who approved, as declared.
    pub fn by(&self) -> &Actor {
        &self.by
    }

    /// When it was recorded (display only).
    pub fn at(&self) -> &Timestamp {
        &self.at
    }

    /// The hash that was approved.
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }

    /// For a CA: the revision.
    pub fn revision(&self) -> Option<&Revision> {
        self.revision.as_ref()
    }

    /// For a CA: the outcome of the tests.
    pub fn tests(&self) -> Option<TestOutcome> {
        self.tests
    }

    /// For a CM: the result.
    pub fn result(&self) -> Option<&CmResult> {
        self.result.as_ref()
    }

    /// Whether a later event withdrew it.
    pub fn revoked(&self) -> bool {
        self.revoked
    }
}

/// Who approved what, including what was withdrawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReport {
    rows: Vec<AuditRow>,
}

impl AuditReport {
    /// The approvals, by gate then subject then event.
    pub fn rows(&self) -> &[AuditRow] {
        &self.rows
    }
}

impl<P: Ports> Workspace<P> {
    /// Where the tool is pointed: project, worktree, revision, and the stores.
    pub async fn context(&self) -> Result<ContextReport, AppError> {
        let line = context_line(&self.ports, &self.config).await?;
        let repo_events = self
            .ports
            .store()
            .events(StoreKind::Repo)
            .await
            .map_err(AppError::port(PortName::Store))?
            .len();
        let private_events = self
            .ports
            .store()
            .events(StoreKind::Private)
            .await
            .map_err(AppError::port(PortName::Store))?
            .len();
        Ok(ContextReport {
            line,
            default_store: self.config.default_store(),
            repo_events,
            private_events,
        })
    }

    /// The status of everything.
    pub async fn status(&self) -> Result<StatusReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let context = context_line(&self.ports, &self.config).await?;
        let exports = self.export_notes(&model).await?;
        Ok(StatusReport::build(&model, context).with_exports(exports))
    }

    /// Compare each generated file with its marker and with a fresh render.
    async fn export_notes(&self, model: &Model) -> Result<Vec<ExportNote>, AppError> {
        let mut notes = Vec::new();
        for store in [StoreKind::Repo, StoreKind::Private] {
            if model.events(store).is_empty() {
                continue;
            }
            let map = MapDoc::build(model, store);
            let mut checks = vec![(store, ExportDoc::Map(&map))];
            let changelog;
            if store == StoreKind::Repo && self.config.export().changelog_md() {
                changelog = ChangelogDoc::build(model, store);
                checks.push((store, ExportDoc::Changelog(&changelog)));
            }
            for (scope, doc) in checks {
                let found = self
                    .ports
                    .store()
                    .inspect_export(scope, &doc)
                    .await
                    .map_err(AppError::port(PortName::Store))?;
                let problem = match found {
                    ExportFile::Absent => Some(ExportProblem::Missing),
                    ExportFile::Unmarked => Some(ExportProblem::EditedByHand),
                    ExportFile::Marked(parts) => {
                        match export_drift(parts.marker(), parts.body(), parts.rebuilt()) {
                            ExportDrift::InSync => None,
                            ExportDrift::EditedByHand => Some(ExportProblem::EditedByHand),
                            ExportDrift::Outdated => Some(ExportProblem::Outdated),
                        }
                    }
                };
                if let Some(problem) = problem {
                    notes.push(ExportNote {
                        scope,
                        kind: doc.kind(),
                        problem,
                    });
                }
            }
        }
        Ok(notes)
    }

    /// The chain around one entity.
    pub async fn trace(&self, target: &Ref) -> Result<TraceReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let (id, _, _) = model.resolve(target)?;
        // Edges as (upstream, downstream).
        let mut edges: BTreeSet<(EntityId, EntityId)> = BTreeSet::new();
        for doc in model.docs() {
            if let Some(present) = doc.present() {
                for up in present.content().upstream() {
                    edges.insert((up, doc.id()));
                }
            }
        }
        for item in model.items() {
            edges.insert((item.key().design.entity(), item.key().id.entity()));
        }
        let mut keep: BTreeSet<EntityId> = BTreeSet::from([id]);
        for direction_up in [true, false] {
            let mut frontier = vec![id];
            while let Some(at) = frontier.pop() {
                for (up, down) in &edges {
                    let (from, to) = if direction_up { (down, up) } else { (up, down) };
                    if *from == at && keep.insert(*to) {
                        frontier.push(*to);
                    }
                }
            }
        }
        let mut nodes = Vec::new();
        for entity in &keep {
            if let Some(doc) = model.doc(*entity) {
                nodes.push(TraceNode {
                    id: doc.id(),
                    kind: doc.kind().entity_kind(),
                    alias: doc.alias().cloned(),
                    path: Some(doc.path().clone()),
                    number: None,
                    state: doc.present().map(|p| p.state().clone()),
                    approved: Vec::new(),
                });
            } else if let Some(item) = model.items().find(|i| i.key().id.entity() == *entity) {
                let approved = model
                    .ledger()
                    .matching(Gate::Ca, *entity, item.hash())
                    .filter_map(|r| match r.attestation() {
                        Attestation::Ca { revision, .. } => Some(ApprovedRevision {
                            event: r.event(),
                            revision: revision.clone(),
                            by: r.by().clone(),
                            integrated: model.impl_view().is_integrated(revision.base()),
                        }),
                        _ => None,
                    })
                    .collect();
                nodes.push(TraceNode {
                    id: *entity,
                    kind: EntityKind::Item,
                    alias: None,
                    path: None,
                    number: Some(item.key().number),
                    state: Some(item.evaluation().state().clone()),
                    approved,
                });
            }
        }
        nodes.sort_by_key(|n| (n.kind, n.number, n.id));
        let links = edges
            .into_iter()
            .filter(|(a, b)| keep.contains(a) && keep.contains(b))
            .collect();
        Ok(TraceReport {
            target: id,
            nodes,
            links,
        })
    }

    /// The history of one entity.
    pub async fn changelog(&self, target: &Ref) -> Result<Changelog, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let (id, _, _) = model.resolve(target)?;
        Ok(Changelog::of(&model, id))
    }

    /// Who approved what, optionally only for the entities a query selects.
    pub async fn audit(&self, filter: Option<&Query>) -> Result<AuditReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let selected = match filter {
            None => None,
            Some(query) => Some(self.matching_ids(&model, query).await?),
        };
        let ledger = model.ledger();
        let mut rows: Vec<AuditRow> = ledger
            .all()
            .iter()
            .filter(|r| selected.as_ref().is_none_or(|s| s.contains(&r.subject())))
            .map(|r| AuditRow::of(r, ledger.is_revoked(&r.event())))
            .collect();
        rows.sort_by_key(|r| (r.gate, r.subject, r.event));
        Ok(AuditReport { rows })
    }
}
