//! The evaluator: from content and approvals to a state, upstream to downstream.
//!
//! Evaluation runs in the order Finding, Spec, Design, Item. Each stage takes the
//! stage above it as an already evaluated map, so a Spec is evaluated against
//! `BTreeMap<FindingId, Evaluated<Finding>>`, a Design against the evaluated Specs,
//! and an item against the evaluated Designs. The result of a stage carries its own
//! [`EntityState`] and a chain of [`Cause`]s that says where a Stale or Blocked came
//! from.
//!
//! The state of an upstream is read from [`States`], one map of every entity's state
//! whatever its kind, and not from the typed map: a document whose kind is in conflict
//! (see `ConflictedDoc` of the classification) has a state and no typed evaluation,
//! and the entities that stand on it must read it. The typed maps give what only their
//! kind has: the entity (for its hash, and for the pin coverage of a Design) and the
//! causes.
//!
//! ```text
//!   events (ledger of approvals)     files, built into entities
//!              \                        /
//!               v                      v
//!   Evaluator::finding   -->  Evaluated<Finding>   state { approval, upstream=Ok }
//!                                  |  map by FindingId
//!                                  v
//!   Evaluator::spec      -->  Evaluated<Spec>      each ground: Ok|Blocked|Stale
//!                                  |  map by SpecId
//!                                  v
//!   Evaluator::design    -->  Evaluated<Design>    the Spec, and the pin coverage
//!                                  |  map by DesignId
//!                                  v
//!   Evaluator::item      -->  ItemEvaluation       the Design; + integration,
//!                                                  progress (ImplView: pins found,
//!                                                  revisions at the baseline)
//! ```
//!
//! The chain of causes. Take Finding F, Spec S on F, Design D on S, item I of D, all
//! approved, then correct F. Each stage looks only at the stage above, and appends
//! its own link to the causes it inherits, so the chain reads from the root:
//!
//! ```text
//!   F  edited          approval  Modified (its hash is not the approved one)
//!   S  approved at its own hash, but its CS recorded F's old hash
//!        upstream = Stale [UpstreamChanged{F}]                causes: [F]
//!   D  approved, recorded S's contract_hash, which has not changed
//!        upstream = Stale [UpstreamStale{S}]                  causes: [F, S]
//!   I  likewise
//!        upstream = Stale [UpstreamStale{D}]                  causes: [F, S, D]
//!
//!   Each entry of `causes` is a Cause { via, through, reason }: the upstream entity
//!   the link goes through, the direct upstream of the holder it is reached by (the
//!   chains of several upstreams share the list), and the StaleReason or
//!   BlockReason it gave. The first entry of a chain is its root: it is where to
//!   start fixing.
//! ```
//!
//! # How a state is decided
//!
//! 1. **Invalid** if the entity's own content is unusable: it did not parse
//!    ([`Unparsed`]), an upstream identifier names nothing in the map (dangling) and
//!    was not written with a mark that points into a store this environment cannot
//!    read, the Design does not cover the Spec's pins exactly, or the caller reports a
//!    classification problem (a path or alias collision) as an external reason.
//! 2. Otherwise **Approved** if an unrevoked approval exists for the current hash,
//!    **Modified** if approvals exist but none for the current hash, else **Draft**.
//! 3. The upstream axis, for every upstream the entity names:
//!    - an upstream that is Invalid or did not parse makes the entity
//!      **Blocked** (`UpstreamInvalid`);
//!    - an upstream that is Draft (or Modified) makes it **Blocked**
//!      (`UpstreamUnapproved`);
//!    - an approved entity whose approval recorded an upstream hash that is not the
//!      upstream's hash today is **Stale** (`UpstreamChanged`);
//!    - an upstream that is itself Blocked makes the entity Blocked and an upstream
//!      that is itself Stale makes it Stale, so the cause travels down.
//!
//!    Blocked outranks Stale when both apply.
//!
//! A Draft entity recorded no upstream hashes and so cannot be Stale. When it sits
//! on a Stale upstream it is shown **Blocked** (`UpstreamStale`) rather than Stale:
//! it cannot be approved until the upstream is approved again.
//!
//! # Completeness
//!
//! A third fact, beside the two axes: whether everything the entity stands on can be
//! read here ([`Completeness`]). The application passes, for each entity, the set of
//! upstream identifiers that the entity wrote with a mark pointing into a store this
//! environment cannot read (`unresolved`). An upstream that is not in the map is then
//! one of two things: in that set, the entity is **Partial** with the reason
//! `(root: it, upstream: it)`; not in it, the entity is Invalid with a dangling
//! reference. An upstream that is in the map and Partial passes its reasons on, each
//! keeping its `root` and taking that upstream as `upstream`. Reasons are collected
//! by `(root, upstream)`, so several upstreams that lead to the same root give one
//! reason each. An item names no upstream by mark (its Design is in its own file), so
//! it takes no set and passes its Design's reasons on.
//!
//! When several approvals exist for one `(gate, entity, hash)`, the entity is Stale
//! only if none of them recorded the upstream as it is today. Two worktrees may have
//! approved the same content at different times and either one suffices.
//!
//! An approval may have pinned a private upstream by a keyed pin
//! ([`keyring`](super::keyring)), which can be compared only with the key. Each
//! approval then holds (every upstream is as recorded), is refuted (some upstream
//! differs) or is undetermined (none differs, and some pin has no key here). The
//! entity is Current if one holds; failing that, Unverifiable if one is undetermined,
//! and only if every approval is refuted is it Stale. Unverifiable is not Stale: the
//! entity is Partial, with the upstream as the root of what it cannot read. The
//! reason is [`UnresolvedCause::KeyMissing`](super::state::UnresolvedCause::KeyMissing),
//! naming the key, and not the cause of an upstream in a store out of reach, since
//! the upstream is there to be read and what is lacking is the key.
//!
//! # The store of approvals
//!
//! What the rules ask of the approvals is the trait [`ApprovalView`]: whether
//! `(gate, subject, hash)` is approved, the hash an earlier approval was at, what
//! each approval recorded of its upstreams, which identifier survives a merge, and
//! the keys. The event log answers it through [`ApprovalSet`]; the judgements of the
//! ledgers answer it in [`corpus_eval`](super::corpus_eval). The rules are written
//! once, here.
//!
//! # Inputs from the application
//!
//! The application resolves aliases to identifiers and parses files before calling
//! here. A file that failed to parse is passed as [`Unparsed`] and evaluated as
//! Invalid. What the code scan found and which revisions reached the baseline come
//! in as an [`ImplView`]; it names no version-control tool.

use std::collections::{BTreeMap, BTreeSet};

use super::approval::{ApprovalSet, Attestation, Gate, UpstreamRecord};
use super::classification::ConflictedDoc;
use super::design::{Design, WorkItem};
use super::event::DocKind;
use super::finding::Finding;
use super::hash::ContentHash;
use super::id::{DesignId, EntityId, FindingId, ItemId, ItemNumber, SpecId};
use super::keyring::{Kid, PinKeyring, PinMatch};
use super::location::{CodeLocation, RelPath};
use super::nonempty::NonEmptyVec;
use super::pin::{MalformedPin, PinId};
use super::revision::{IntegrationFact, RevisionId};
use super::spec::Spec;
use super::state::{
    Approval, ApprovalProblem, BlockReason, Completeness, EntityState, HealthProblem, Integration,
    InvalidReason, ItemProgress, StaleReason, UnparsedReason, UnresolvedReason, Upstream,
    UpstreamFacts,
};

/// A file that could not become an entity.
///
/// The application passes it in place of the entity; the evaluator reads it as
/// Invalid with the reasons given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unparsed {
    path: RelPath,
    reasons: NonEmptyVec<UnparsedReason>,
    raw_hash: ContentHash,
}

impl Unparsed {
    /// A failed parse: the file, why, and the hash of its normalized text.
    pub fn new(path: RelPath, reasons: NonEmptyVec<UnparsedReason>, raw_hash: ContentHash) -> Self {
        Self {
            path,
            reasons,
            raw_hash,
        }
    }

    /// The file.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// Why it failed.
    pub fn reasons(&self) -> &NonEmptyVec<UnparsedReason> {
        &self.reasons
    }

    /// The `raw` hash of its text, the key of the stored object.
    pub fn raw_hash(&self) -> &ContentHash {
        &self.raw_hash
    }
}

/// Why an entity is Stale or Blocked because of one upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CauseReason {
    /// A staleness.
    Stale(StaleReason),
    /// A block.
    Blocked(BlockReason),
}

/// One link in the chain that explains a Stale or Blocked state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause {
    via: EntityId,
    through: EntityId,
    reason: CauseReason,
}

impl Cause {
    /// The upstream entity this link goes through.
    pub fn via(&self) -> EntityId {
        self.via
    }

    /// The direct upstream of the entity that holds the chain, through which this
    /// link is reached (equal to [`via`](Self::via) for the last link of a chain).
    ///
    /// An entity with several upstreams has the chains of all of them in one list;
    /// this says which chain a link belongs to, so the root of one upstream's chain
    /// is the first link whose `through` is that upstream.
    pub fn through(&self) -> EntityId {
        self.through
    }

    /// What that upstream did to the entity.
    pub fn reason(&self) -> &CauseReason {
        &self.reason
    }

    /// The same link, seen from the entity one step down, which reaches it through
    /// `upstream`.
    fn reached_through(&self, upstream: EntityId) -> Self {
        Self {
            through: upstream,
            ..self.clone()
        }
    }

    /// A link made by hand, for the tests of what reads a chain of causes.
    #[cfg(test)]
    pub(crate) fn for_test(via: EntityId, through: EntityId) -> Self {
        Self {
            via,
            through,
            reason: CauseReason::Blocked(BlockReason::UpstreamUnapproved { upstream: via }),
        }
    }
}

/// An entity, or the file that failed to become one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject<T> {
    /// The parsed entity.
    Parsed(T),
    /// The failed parse.
    Unparsed(Unparsed),
}

/// An entity with its evaluated state and the causes of that state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated<T> {
    subject: Subject<T>,
    state: EntityState,
    causes: Vec<Cause>,
}

impl<T> Evaluated<T> {
    /// Evaluate a failed parse: Invalid with its reasons, plus any external ones.
    pub fn from_unparsed(unparsed: Unparsed, external: &[InvalidReason]) -> Self {
        let mut reasons: Vec<InvalidReason> = unparsed
            .reasons
            .iter()
            .cloned()
            .map(InvalidReason::Unparsed)
            .collect();
        reasons.extend_from_slice(external);
        let mut it = reasons.into_iter();
        let first = it
            .next()
            .unwrap_or_else(|| InvalidReason::Unparsed(unparsed.reasons.first().clone()));
        Self {
            subject: Subject::Unparsed(unparsed),
            state: EntityState::new(
                Approval::Invalid(NonEmptyVec::new(first, it.collect())),
                Upstream::Ok,
                Completeness::Complete,
            ),
            causes: Vec::new(),
        }
    }

    /// What was evaluated: the entity, or the file that failed to become one.
    pub fn subject(&self) -> &Subject<T> {
        &self.subject
    }

    /// The parsed entity, if it parsed.
    pub fn entity(&self) -> Option<&T> {
        match &self.subject {
            Subject::Parsed(t) => Some(t),
            Subject::Unparsed(_) => None,
        }
    }

    /// The failed parse, if it failed.
    pub fn unparsed(&self) -> Option<&Unparsed> {
        match &self.subject {
            Subject::Parsed(_) => None,
            Subject::Unparsed(u) => Some(u),
        }
    }

    /// The state.
    pub fn state(&self) -> &EntityState {
        &self.state
    }

    /// Where a Stale or Blocked came from, root cause first. Empty when the
    /// upstream axis is Ok.
    pub fn causes(&self) -> &[Cause] {
        &self.causes
    }
}

/// What the application learned from the code and the baseline.
///
/// Built by the application from its ports. Names no version-control tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplView {
    current_revision: RevisionId,
    workspace_label: Option<String>,
    pins_at_revision: BTreeMap<PinId, Vec<CodeLocation>>,
    malformed: Vec<MalformedPin>,
    integrated: BTreeMap<RevisionId, IntegrationFact>,
}

impl ImplView {
    /// The view: the current revision, a label for display only, the pins found in
    /// the tests with where they were found, and what is known of each approved
    /// revision against the baseline.
    pub fn new(
        current_revision: RevisionId,
        workspace_label: Option<String>,
        pins_at_revision: BTreeMap<PinId, Vec<CodeLocation>>,
        integrated: BTreeMap<RevisionId, IntegrationFact>,
    ) -> Self {
        Self {
            current_revision,
            workspace_label,
            pins_at_revision,
            malformed: Vec::new(),
            integrated,
        }
    }

    /// The same view with the tags that were found in the tests and are no pin name.
    pub fn with_malformed(mut self, malformed: Vec<MalformedPin>) -> Self {
        self.malformed = malformed;
        self
    }

    /// The tags found in the tests whose name is not a pin's, in the order found.
    /// They are not dropped: the CA gate reports each.
    pub fn malformed_pins(&self) -> &[MalformedPin] {
        &self.malformed
    }

    /// The revision the scan was made at.
    pub fn current_revision(&self) -> &RevisionId {
        &self.current_revision
    }

    /// A name for display (a branch name, say). Decides nothing.
    pub fn workspace_label(&self) -> Option<&str> {
        self.workspace_label.as_deref()
    }

    /// The pins found in test files at the current revision, with locations.
    pub fn pins_at_revision(&self) -> &BTreeMap<PinId, Vec<CodeLocation>> {
        &self.pins_at_revision
    }

    /// What is known of a revision against the baseline. A revision nobody asked
    /// about is not integrated: only the approved revisions are asked.
    pub fn integration_of(&self, revision: &RevisionId) -> IntegrationFact {
        self.integrated
            .get(revision)
            .copied()
            .unwrap_or(IntegrationFact::NotIntegrated)
    }

    /// Whether a pin was found at least once.
    pub fn has_pin(&self, pin: &PinId) -> bool {
        self.pins_at_revision
            .get(pin)
            .is_some_and(|at| !at.is_empty())
    }
}

/// What identifies an item for evaluation: its identifier and its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemKey {
    /// The item's identifier, from the registration fold.
    pub id: ItemId,
    /// The Design it belongs to.
    pub design: DesignId,
    /// Its number in that Design.
    pub number: ItemNumber,
}

/// The evaluation of a work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemEvaluation {
    state: EntityState,
    causes: Vec<Cause>,
    integration: Integration,
    progress: ItemProgress,
}

impl ItemEvaluation {
    /// The state.
    pub fn state(&self) -> &EntityState {
        &self.state
    }

    /// Where a Stale or Blocked came from, root cause first.
    pub fn causes(&self) -> &[Cause] {
        &self.causes
    }

    /// Whether the approved revision reached the baseline.
    pub fn integration(&self) -> &Integration {
        &self.integration
    }

    /// How many of the item's pins the tests carry.
    pub fn progress(&self) -> &ItemProgress {
        &self.progress
    }
}

/// An upstream as the entity that stands on it looks at it.
///
/// Its state is read from the map of every entity's state, whatever the kind, so an
/// upstream that has no typed evaluation (a document in conflict over its kind) is
/// read like any other. Its causes and its hash come from the typed evaluation when
/// there is one; the one without has neither.
struct UpView<'a> {
    id: EntityId,
    state: &'a EntityState,
    causes: &'a [Cause],
    hash: Option<&'a ContentHash>,
}

impl<'a> UpView<'a> {
    fn new<T>(
        id: EntityId,
        state: &'a EntityState,
        typed: Option<&'a Evaluated<T>>,
        hash_of: fn(&T) -> &ContentHash,
    ) -> Self {
        Self {
            id,
            state,
            causes: typed.map_or(&[], Evaluated::causes),
            hash: typed.and_then(Evaluated::entity).map(hash_of),
        }
    }
}

/// How a file wrote the upstream identifiers it holds that no state can answer for.
///
/// What an entity holds is the identifier alone (the mark it was written with is a
/// fact about the file, and stays out of every hash), so the evaluator is told the
/// two facts about how it was written that it could not know: the identifier of a
/// store this environment cannot read, and the identifier that the document, where
/// it stands, can see nothing under.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Written {
    /// Written with a mark that points into a store that cannot be read here.
    pub unresolved: BTreeSet<EntityId>,
    /// Written as an identifier that nothing visible from the document holds.
    pub dangling: BTreeSet<EntityId>,
}

/// The state of every entity that has one, whatever its kind, keyed by identifier.
///
/// This is what an entity reads its upstream's state from. Findings, Specs, Designs
/// and the documents in conflict all have a place in it; the typed maps of
/// evaluations keep what only their kind has (the entity, the causes).
pub type States<'a> = BTreeMap<EntityId, &'a EntityState>;

/// The [`States`] of what has been evaluated so far: the Findings, Specs and Designs,
/// and the documents in conflict. Items are not in it: nothing stands on an item.
pub fn states_of<'a>(
    findings: &'a BTreeMap<FindingId, Evaluated<Finding>>,
    specs: &'a BTreeMap<SpecId, Evaluated<Spec>>,
    designs: &'a BTreeMap<DesignId, Evaluated<Design>>,
    conflicted: impl IntoIterator<Item = &'a ConflictedDoc>,
) -> States<'a> {
    let mut states = States::new();
    states.extend(findings.iter().map(|(id, ev)| (id.entity(), ev.state())));
    states.extend(specs.iter().map(|(id, ev)| (id.entity(), ev.state())));
    states.extend(designs.iter().map(|(id, ev)| (id.entity(), ev.state())));
    states.extend(conflicted.into_iter().map(|doc| (doc.id(), doc.state())));
    states
}

struct Basis<'a> {
    gate: Gate,
    subject: EntityId,
    hash: &'a ContentHash,
}

#[derive(Default)]
struct Health {
    blocks: Vec<BlockReason>,
    stales: Vec<StaleReason>,
    block_causes: Vec<Cause>,
    stale_causes: Vec<Cause>,
}

impl Health {
    fn block(&mut self, up: &UpView<'_>, reason: BlockReason, inherit: bool) {
        if inherit {
            self.block_causes
                .extend(up.causes.iter().map(|c| c.reached_through(up.id)));
        }
        self.block_causes.push(Cause {
            via: up.id,
            through: up.id,
            reason: CauseReason::Blocked(reason.clone()),
        });
        self.blocks.push(reason);
    }

    fn stale(&mut self, up: &UpView<'_>, reason: StaleReason, inherit: bool) {
        if inherit {
            self.stale_causes
                .extend(up.causes.iter().map(|c| c.reached_through(up.id)));
        }
        self.stale_causes.push(Cause {
            via: up.id,
            through: up.id,
            reason: CauseReason::Stale(reason.clone()),
        });
        self.stales.push(reason);
    }

    fn finish(self) -> (Upstream, Vec<Cause>) {
        match NonEmptyVec::from_vec(self.blocks) {
            Ok(blocks) => (Upstream::Blocked(blocks), self.block_causes),
            Err(_) => match NonEmptyVec::from_vec(self.stales) {
                Ok(stales) => (Upstream::Stale(stales), self.stale_causes),
                Err(_) => (Upstream::Ok, Vec::new()),
            },
        }
    }
}

/// What an entity cannot read here, collected while its upstreams are looked at.
///
/// A set keyed by `(root, upstream)`, so the same pair reached twice is one reason.
#[derive(Default)]
struct Gaps(BTreeSet<UnresolvedReason>);

impl Gaps {
    /// `upstream` is written with a mark that points into a store that cannot be
    /// read here: it is itself what cannot be read.
    fn unreadable(&mut self, upstream: EntityId) {
        self.0.insert(UnresolvedReason::new(upstream, upstream));
    }

    /// `upstream` is in the map; whatever it cannot read, this entity cannot
    /// either, through it.
    fn inherit(&mut self, upstream: EntityId, state: &EntityState) {
        self.0.extend(
            state
                .completeness()
                .reasons()
                .map(|reason| reason.reached_through(upstream)),
        );
    }

    /// `pins` are upstreams that are there, which an approval of this entity pinned by
    /// a keyed pin whose key is not in the keyring: the state of each cannot be
    /// compared with the approval here. They are not unreadable (a store out of
    /// reach); what is missing is the key, and the reason says which.
    fn unverifiable(&mut self, pins: &BTreeSet<(EntityId, Kid)>) {
        for (upstream, kid) in pins {
            self.0
                .insert(UnresolvedReason::key_missing(*upstream, kid.clone()));
        }
    }

    fn finish(self) -> Completeness {
        Completeness::from_reasons(self.0)
    }
}

/// What the evaluator asks of the store of approvals.
///
/// The trait is the evaluator's own (the consumer defines it): the old event log
/// answers through [`ApprovalSet`], the ledgers of judgements through
/// [`corpus_eval`](super::corpus_eval). The rules of Approved, Modified and Stale
/// are written once, over these five questions.
pub trait ApprovalView {
    /// Whether `(gate, subject, hash)` has an approval that stands.
    fn is_approved_at(&self, gate: Gate, subject: EntityId, hash: &ContentHash) -> bool;

    /// The hash an approval of `gate` for `subject` that stands is at, when none is at
    /// the hash today. Which one, when there are several, is for display only: the
    /// hash a Modified entity says it was approved at.
    fn latest_approved(&self, gate: Gate, subject: EntityId) -> Option<ContentHash>;

    /// The upstream facts each approval at exactly `(gate, subject, hash)` that stands
    /// recorded: one list per approval.
    fn recorded_upstreams(
        &self,
        gate: Gate,
        subject: EntityId,
        hash: &ContentHash,
    ) -> Vec<Vec<UpstreamRecord>>;

    /// The surviving identifier of an entity (itself unless a merge made it equal to
    /// another).
    fn canonical_of(&self, id: EntityId) -> EntityId;

    /// The keys the keyed pins of the approvals are compared under.
    fn keyring(&self) -> &PinKeyring;
}

static NO_KEYS: PinKeyring = PinKeyring::empty();

impl ApprovalView for ApprovalSet {
    fn is_approved_at(&self, gate: Gate, subject: EntityId, hash: &ContentHash) -> bool {
        self.is_approved(gate, subject, hash)
    }

    fn latest_approved(&self, gate: Gate, subject: EntityId) -> Option<ContentHash> {
        self.latest_hash(gate, subject).copied()
    }

    fn recorded_upstreams(
        &self,
        gate: Gate,
        subject: EntityId,
        hash: &ContentHash,
    ) -> Vec<Vec<UpstreamRecord>> {
        self.matching(gate, subject, hash)
            .map(|record| record.attestation().upstream())
            .collect()
    }

    fn canonical_of(&self, id: EntityId) -> EntityId {
        self.canonical(id)
    }

    /// The event log never made a keyed pin, so it has no keys.
    fn keyring(&self) -> &PinKeyring {
        &NO_KEYS
    }
}

/// What the approvals of an entity at its current hash say about its upstreams today.
enum Staleness {
    /// Some approval recorded every upstream as it is (or there is no upstream to
    /// compare).
    Current,
    /// Every approval recorded some upstream otherwise; the reasons are those of the
    /// first approval.
    Stale(Vec<(EntityId, StaleReason)>),
    /// No approval is refuted, and some cannot be checked: a keyed pin whose key is
    /// not in the keyring. Not stale, since nothing says it changed; the upstreams
    /// whose pins cannot be checked are what the entity cannot read here, each with
    /// the key that is lacking.
    Unverifiable(BTreeSet<(EntityId, Kid)>),
}

/// The hash-level staleness of an approved entity: for each upstream, whether
/// some matching approval recorded it as it is today.
///
/// An approval holds if every upstream it recorded is as it is, is refuted if some
/// upstream differs (or was not recorded), and is undetermined if none differs and
/// some pin cannot be compared. The entity is Stale only when every approval is
/// refuted; one that holds makes it Current, and failing that an undetermined one
/// leaves it Unverifiable.
fn hash_staleness(
    basis: &Basis<'_>,
    ups: &[UpView<'_>],
    ledger: &(impl ApprovalView + ?Sized),
) -> Staleness {
    let currents: Vec<(EntityId, &ContentHash)> = ups
        .iter()
        .filter_map(|u| u.hash.map(|h| (u.id, h)))
        .collect();
    let mut first_failure: Option<Vec<(EntityId, StaleReason)>> = None;
    let mut undetermined: BTreeSet<(EntityId, Kid)> = BTreeSet::new();
    for recorded in ledger.recorded_upstreams(basis.gate, basis.subject, basis.hash) {
        let mut reasons: Vec<(EntityId, StaleReason)> = Vec::new();
        let mut unknown: BTreeSet<(EntityId, Kid)> = BTreeSet::new();
        for (id, current) in &currents {
            let at_approval = recorded.iter().find_map(|r| match r {
                UpstreamRecord::Entity { id: rid, hash }
                    if ledger.canonical_of(*rid) == ledger.canonical_of(*id) =>
                {
                    Some((*rid, hash))
                }
                _ => None,
            });
            match at_approval {
                None => reasons.push((*id, StaleReason::UpstreamNotRecorded { upstream: *id })),
                // A keyed pin was made over the identifier the approval recorded.
                Some((rid, pin)) => match pin.check(rid, current, ledger.keyring()) {
                    PinMatch::Same => {}
                    PinMatch::Unknown { kid } => {
                        unknown.insert((*id, kid));
                    }
                    PinMatch::Differs => reasons.push((
                        *id,
                        StaleReason::UpstreamChanged {
                            upstream: *id,
                            recorded: pin.clone(),
                            current: **current,
                        },
                    )),
                },
            }
        }
        if reasons.is_empty() && unknown.is_empty() {
            return Staleness::Current;
        }
        if reasons.is_empty() {
            undetermined.extend(unknown);
        } else {
            first_failure.get_or_insert(reasons);
        }
    }
    if undetermined.is_empty() {
        Staleness::Stale(first_failure.unwrap_or_default())
    } else {
        Staleness::Unverifiable(undetermined)
    }
}

/// What the upstream axis of an entity came to, with what could not be checked.
struct Assessment {
    upstream: Upstream,
    causes: Vec<Cause>,
    /// The upstreams whose recorded pins cannot be compared here, with the key each
    /// was pinned under.
    unverifiable: BTreeSet<(EntityId, Kid)>,
}

/// The two axes of an entity from its upstreams.
///
/// What an upstream asks of the entity is [`UpstreamFacts`]; this only turns the
/// facts into reasons. An upstream that has a problem on the approval axis is
/// reported by it alone (the entity cannot be approved until that is repaired,
/// whatever the upstream's own health). The completeness fact is not mapped here:
/// it stays a fact of its own (see [`Gaps`]), and the upstreams whose pins cannot
/// be compared are handed back for the caller to put there.
fn assess(
    basis: Option<Basis<'_>>,
    ups: &[UpView<'_>],
    ledger: &(impl ApprovalView + ?Sized),
) -> Assessment {
    let mut health = Health::default();
    let mut changed: BTreeSet<EntityId> = BTreeSet::new();
    let mut unverifiable = BTreeSet::new();
    let approved = basis.is_some();
    if let Some(basis) = &basis {
        match hash_staleness(basis, ups, ledger) {
            Staleness::Current => {}
            Staleness::Stale(reasons) => {
                for (id, reason) in reasons {
                    changed.insert(id);
                    if let Some(up) = ups.iter().find(|u| u.id == id) {
                        health.stale(up, reason, false);
                    }
                }
            }
            Staleness::Unverifiable(ids) => unverifiable = ids,
        }
    }
    for up in ups {
        let facts = UpstreamFacts::of(up.state, approved, changed.contains(&up.id));
        let upstream = up.id;
        if let Some(problem) = facts.approval() {
            let reason = match problem {
                ApprovalProblem::Invalid => BlockReason::UpstreamInvalid { upstream },
                ApprovalProblem::NotApproved => BlockReason::UpstreamUnapproved { upstream },
            };
            health.block(up, reason, true);
            continue;
        }
        match facts.health() {
            None => {}
            Some(HealthProblem::Blocked) => {
                health.block(up, BlockReason::UpstreamBlocked { upstream }, true);
            }
            Some(HealthProblem::Stale) if approved => {
                health.stale(up, StaleReason::UpstreamStale { upstream }, true);
            }
            Some(HealthProblem::Stale) => {
                health.block(up, BlockReason::UpstreamStale { upstream }, true);
            }
        }
    }
    let (upstream, causes) = health.finish();
    Assessment {
        upstream,
        causes,
        unverifiable,
    }
}

fn own_approval(
    gate: Gate,
    subject: EntityId,
    hash: &ContentHash,
    ledger: &(impl ApprovalView + ?Sized),
    invalid: Vec<InvalidReason>,
) -> Approval {
    if let Ok(reasons) = NonEmptyVec::from_vec(invalid) {
        return Approval::Invalid(reasons);
    }
    if ledger.is_approved_at(gate, subject, hash) {
        Approval::Approved
    } else if let Some(approved) = ledger.latest_approved(gate, subject) {
        Approval::Modified { approved }
    } else {
        Approval::Draft
    }
}

fn basis_if_approved<'a>(
    approval: &Approval,
    gate: Gate,
    subject: EntityId,
    hash: &'a ContentHash,
) -> Option<Basis<'a>> {
    matches!(approval, Approval::Approved).then_some(Basis {
        gate,
        subject,
        hash,
    })
}

/// The domain service that evaluates each stage.
pub struct Evaluator;

impl Evaluator {
    /// Evaluate a Finding. `external` are reasons found outside its content
    /// (a path or alias collision).
    pub fn finding(
        f: &Finding,
        ledger: &(impl ApprovalView + ?Sized),
        external: &[InvalidReason],
    ) -> Evaluated<Finding> {
        let approval = own_approval(
            Gate::of_doc(DocKind::Finding),
            f.id().entity(),
            f.hash(),
            ledger,
            external.to_vec(),
        );
        Evaluated {
            subject: Subject::Parsed(f.clone()),
            state: EntityState::new(approval, Upstream::Ok, Completeness::Complete),
            causes: Vec::new(),
        }
    }

    /// Evaluate a Spec against its evaluated grounds.
    ///
    /// The state of each ground is read from `states`, which holds every entity
    /// whatever its kind, a document in conflict among them; `findings` holds the
    /// causes and the hash of those that are Findings.
    ///
    /// `unresolved` are the grounds the Spec wrote with a mark that points into a
    /// store this environment cannot read. `dangling` are those it wrote that the
    /// document could not see anything under, where it stands (a plain identifier of
    /// an entity in the private store, written in a public document, is under no
    /// name it can use). A ground with no state makes the Spec Partial when it is
    /// in `unresolved`, and a ground in `dangling` or with no state otherwise makes
    /// it Invalid (dangling): whatever `states` holds under the identifier, the
    /// document has no right to read.
    pub fn spec(
        s: &Spec,
        findings: &BTreeMap<FindingId, Evaluated<Finding>>,
        states: &States<'_>,
        ledger: &(impl ApprovalView + ?Sized),
        written: &Written,
        external: &[InvalidReason],
    ) -> Evaluated<Spec> {
        let mut invalid = external.to_vec();
        let mut gaps = Gaps::default();
        let mut ups = Vec::new();
        for ground in s.grounds().iter() {
            let id = ground.entity();
            match states.get(&id) {
                Some(state) if !written.dangling.contains(&id) => {
                    gaps.inherit(id, state);
                    ups.push(UpView::new(id, state, findings.get(ground), Finding::hash));
                }
                None if written.unresolved.contains(&id) => gaps.unreadable(id),
                _ => invalid.push(InvalidReason::DanglingRef { upstream: id }),
            }
        }
        let gate = Gate::of_doc(DocKind::Spec);
        let subject = s.id().entity();
        let approval = own_approval(gate, subject, s.contract_hash(), ledger, invalid);
        let basis = basis_if_approved(&approval, gate, subject, s.contract_hash());
        let assessed = assess(basis, &ups, ledger);
        gaps.unverifiable(&assessed.unverifiable);
        Evaluated {
            subject: Subject::Parsed(s.clone()),
            state: EntityState::new(approval, assessed.upstream, gaps.finish()),
            causes: assessed.causes,
        }
    }

    /// Evaluate a Design against its evaluated Spec.
    ///
    /// The Spec's state is read from `states` (see [`Evaluator::spec`]), and its
    /// entity, causes and hash from `specs`. `written` says how the Design wrote the
    /// Spec when that is not a plain name that resolved: with a mark that points
    /// into a store this environment cannot read (the Design is then Partial), or
    /// under an identifier it cannot see anything under (Invalid, dangling), as for
    /// a Spec's grounds. A Spec with no state otherwise makes the Design Invalid
    /// (dangling) too. When the Spec parsed, its pins are checked against the
    /// Design's assignment and every mismatch is an Invalid reason.
    pub fn design(
        d: &Design,
        specs: &BTreeMap<SpecId, Evaluated<Spec>>,
        states: &States<'_>,
        ledger: &(impl ApprovalView + ?Sized),
        written: &Written,
        external: &[InvalidReason],
    ) -> Evaluated<Design> {
        let mut invalid = external.to_vec();
        let mut gaps = Gaps::default();
        let mut ups = Vec::new();
        let spec_id = d.spec().entity();
        let typed = specs.get(&d.spec());
        match states.get(&spec_id) {
            Some(state) if !written.dangling.contains(&spec_id) => {
                if let Some(spec) = typed.and_then(Evaluated::entity) {
                    invalid.extend(
                        d.check_pin_coverage(spec)
                            .into_iter()
                            .map(InvalidReason::PinCoverage),
                    );
                }
                gaps.inherit(spec_id, state);
                ups.push(UpView::new(spec_id, state, typed, Spec::contract_hash));
            }
            None if written.unresolved.contains(&spec_id) => gaps.unreadable(spec_id),
            _ => invalid.push(InvalidReason::DanglingRef { upstream: spec_id }),
        }
        let gate = Gate::of_doc(DocKind::Design);
        let subject = d.id().entity();
        let approval = own_approval(gate, subject, d.design_hash(), ledger, invalid);
        let basis = basis_if_approved(&approval, gate, subject, d.design_hash());
        let assessed = assess(basis, &ups, ledger);
        gaps.unverifiable(&assessed.unverifiable);
        Evaluated {
            subject: Subject::Parsed(d.clone()),
            state: EntityState::new(approval, assessed.upstream, gaps.finish()),
            causes: assessed.causes,
        }
    }

    /// Evaluate a work item against its evaluated Design, with the integration and
    /// the progress.
    pub fn item(
        key: &ItemKey,
        item: &WorkItem,
        designs: &BTreeMap<DesignId, Evaluated<Design>>,
        states: &States<'_>,
        ledger: &ApprovalSet,
        impl_view: &ImplView,
        external: &[InvalidReason],
    ) -> ItemEvaluation {
        let (state, causes) = Self::item_state(key, item, designs, states, ledger, external);
        let hash = item.hash(key.design);
        let subject = key.id.entity();
        let integration = if matches!(state.approval(), Approval::Approved) {
            integration_of(subject, &hash, ledger, impl_view)
        } else {
            Integration::Unapproved
        };
        let declared = item.assigned_pins().clone();
        let found = declared
            .iter()
            .filter(|pin| impl_view.has_pin(pin))
            .cloned()
            .collect();
        ItemEvaluation {
            state,
            causes,
            integration,
            progress: ItemProgress::new(found, declared),
        }
    }

    /// The state of a work item and the causes of it, without its integration and
    /// progress: the part of [`Evaluator::item`] that reads only the approvals and
    /// the Design.
    pub fn item_state(
        key: &ItemKey,
        item: &WorkItem,
        designs: &BTreeMap<DesignId, Evaluated<Design>>,
        states: &States<'_>,
        ledger: &(impl ApprovalView + ?Sized),
        external: &[InvalidReason],
    ) -> (EntityState, Vec<Cause>) {
        let mut invalid = external.to_vec();
        let mut gaps = Gaps::default();
        let mut ups = Vec::new();
        let design_id = key.design.entity();
        match states.get(&design_id) {
            None => invalid.push(InvalidReason::DanglingRef {
                upstream: design_id,
            }),
            Some(state) => {
                gaps.inherit(design_id, state);
                ups.push(UpView::new(
                    design_id,
                    state,
                    designs.get(&key.design),
                    Design::design_hash,
                ));
            }
        }
        let hash = item.hash(key.design);
        let subject = key.id.entity();
        let approval = own_approval(Gate::Ca, subject, &hash, ledger, invalid);
        let basis = basis_if_approved(&approval, Gate::Ca, subject, &hash);
        let assessed = assess(basis, &ups, ledger);
        gaps.unverifiable(&assessed.unverifiable);
        (
            EntityState::new(approval, assessed.upstream, gaps.finish()),
            assessed.causes,
        )
    }
}

fn integration_of(
    subject: EntityId,
    hash: &ContentHash,
    ledger: &ApprovalSet,
    impl_view: &ImplView,
) -> Integration {
    let revisions: Vec<&RevisionId> = ledger
        .matching(Gate::Ca, subject, hash)
        .filter_map(|r| match r.attestation() {
            Attestation::Ca { revision, .. } => Some(revision.base()),
            _ => None,
        })
        .collect();
    // Reached the baseline wins; failing that, an approval that cannot be placed leaves
    // the question open rather than answering "pending" for it.
    if let Some(done) = revisions
        .iter()
        .find(|r| impl_view.integration_of(r) == IntegrationFact::Integrated)
    {
        return Integration::Integrated {
            revision: (*done).clone(),
        };
    }
    let unknown = revisions
        .iter()
        .find_map(|r| match impl_view.integration_of(r) {
            IntegrationFact::Unknown { reason } => Some(((*r).clone(), reason)),
            _ => None,
        });
    if let Some((revision, reason)) = unknown {
        return Integration::Unknown { revision, reason };
    }
    match revisions.first() {
        Some(first) => Integration::ApprovedPending {
            revision: (*first).clone(),
        },
        None => Integration::Unapproved,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    // Empty maps that live long enough to be borrowed into a `States`.
    pub(crate) static NO_FINDINGS: BTreeMap<FindingId, Evaluated<Finding>> = BTreeMap::new();
    pub(crate) static NO_SPECS: BTreeMap<SpecId, Evaluated<Spec>> = BTreeMap::new();
    pub(crate) static NO_DESIGNS: BTreeMap<DesignId, Evaluated<Design>> = BTreeMap::new();
}

#[cfg(test)]
mod tests {
    use super::test_support::{NO_DESIGNS, NO_FINDINGS, NO_SPECS};
    use super::*;
    use crate::domain::approval::test_support::{approve, ca, ci, cr, cs, revoke};
    use crate::domain::classification::fold;
    use crate::domain::design::test_support::{design, item, num};
    use crate::domain::event::Event;
    use crate::domain::finding::test_support::finding;
    use crate::domain::id::test_support::{did, eid, fid, iid, sid};
    use crate::domain::spec::test_support::spec;

    // The evaluators read the state of an upstream from the map of every entity's
    // state. These take the typed maps the tests build and make that map of them.
    fn eval_spec(
        s: &Spec,
        findings: &BTreeMap<FindingId, Evaluated<Finding>>,
        ledger: &ApprovalSet,
        unresolved: &BTreeSet<EntityId>,
        external: &[InvalidReason],
    ) -> Evaluated<Spec> {
        let states = states_of(findings, &NO_SPECS, &NO_DESIGNS, []);
        let written = Written {
            unresolved: unresolved.clone(),
            dangling: BTreeSet::new(),
        };
        Evaluator::spec(s, findings, &states, ledger, &written, external)
    }

    fn eval_design(
        d: &Design,
        specs: &BTreeMap<SpecId, Evaluated<Spec>>,
        ledger: &ApprovalSet,
        unresolved: &BTreeSet<EntityId>,
        external: &[InvalidReason],
    ) -> Evaluated<Design> {
        let states = states_of(&NO_FINDINGS, specs, &NO_DESIGNS, []);
        let written = Written {
            unresolved: unresolved.clone(),
            dangling: BTreeSet::new(),
        };
        Evaluator::design(d, specs, &states, ledger, &written, external)
    }

    fn eval_item(
        key: &ItemKey,
        item: &WorkItem,
        designs: &BTreeMap<DesignId, Evaluated<Design>>,
        ledger: &ApprovalSet,
        impl_view: &ImplView,
        external: &[InvalidReason],
    ) -> ItemEvaluation {
        let states = states_of(&NO_FINDINGS, &NO_SPECS, designs, []);
        Evaluator::item(key, item, designs, &states, ledger, impl_view, external)
    }

    // The chain used throughout: finding 1 <- spec 11 <- design 21 <- item 31.
    const F: u128 = 1;
    const S: u128 = 11;
    const D: u128 = 21;
    const I: u128 = 31;

    fn f(body: &str) -> Finding {
        finding(F, body)
    }

    fn s() -> Spec {
        spec(S, &[F], &["PIN-A", "UT-B"])
    }

    fn d() -> Design {
        design(
            D,
            S,
            vec![(1, item("do a", &["PIN-A"])), (2, item("do b", &["UT-B"]))],
        )
    }

    fn ledger(events: &[Event]) -> ApprovalSet {
        ApprovalSet::from_events(events, &fold(events))
    }

    fn rev(s: &str) -> RevisionId {
        RevisionId::from_validated(s.to_owned()).unwrap()
    }

    fn view(pins: &[&str], integrated: &[&str]) -> ImplView {
        ImplView::new(
            rev("head"),
            Some("main".into()),
            pins.iter()
                .map(|p| (p.parse().unwrap(), vec!["src/a.rs:1".parse().unwrap()]))
                .collect(),
            integrated
                .iter()
                .map(|r| (rev(r), IntegrationFact::Integrated))
                .collect(),
        )
    }

    struct Evaluated4 {
        finding: Evaluated<Finding>,
        spec: Evaluated<Spec>,
        design: Evaluated<Design>,
        item: ItemEvaluation,
    }

    fn evaluate_all(
        finding_: &Finding,
        spec_: &Spec,
        design_: &Design,
        events: &[Event],
        pins: &[&str],
        integrated: &[&str],
    ) -> Evaluated4 {
        let l = ledger(events);
        let ef = Evaluator::finding(finding_, &l, &[]);
        let es = eval_spec(
            spec_,
            &BTreeMap::from([(finding_.id(), ef.clone())]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let ed = eval_design(
            design_,
            &BTreeMap::from([(spec_.id(), es.clone())]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let key = ItemKey {
            id: crate::domain::id::ItemId::new(eid(I)),
            design: design_.id(),
            number: num(1),
        };
        let it = eval_item(
            &key,
            &design_.items()[&num(1)],
            &BTreeMap::from([(design_.id(), ed.clone())]),
            &l,
            &view(pins, integrated),
            &[],
        );
        Evaluated4 {
            finding: ef,
            spec: es,
            design: ed,
            item: it,
        }
    }

    fn approvals(f_: &Finding, s_: &Spec, d_: &Design) -> Vec<Event> {
        let a_cr = approve(cr(F, f_.hash()), &[]);
        let a_cs = approve(
            cs(S, s_.contract_hash(), s_.hyp_hash(), &[(F, *f_.hash())]),
            &[],
        );
        let a_ci = approve(ci(D, d_.design_hash(), S, s_.contract_hash()), &[]);
        let a_ca = approve(
            ca(I, &d_.item_hash(num(1)).unwrap(), D, d_.design_hash(), "r1"),
            &[],
        );
        vec![a_cr, a_cs, a_ci, a_ca]
    }

    fn is_ok(u: &Upstream) -> bool {
        matches!(u, Upstream::Ok)
    }

    fn blocked_reasons(u: &Upstream) -> Vec<BlockReason> {
        match u {
            Upstream::Blocked(r) => r.iter().cloned().collect(),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    fn stale_reasons(u: &Upstream) -> Vec<StaleReason> {
        match u {
            Upstream::Stale(r) => r.iter().cloned().collect(),
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    // --- the state table of the model ------------------------------------

    #[test]
    fn nothing_approved_is_draft_everywhere_and_blocked_below_the_finding() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let r = evaluate_all(&f0, &s0, &d0, &[], &[], &[]);
        assert_eq!(r.finding.state().approval(), &Approval::Draft);
        assert!(is_ok(r.finding.state().upstream()));
        assert_eq!(r.spec.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.spec.state().upstream()),
            vec![BlockReason::UpstreamUnapproved { upstream: eid(F) }]
        );
        assert_eq!(r.design.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.design.state().upstream()),
            vec![BlockReason::UpstreamUnapproved { upstream: eid(S) }]
        );
        assert_eq!(r.item.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.item.state().upstream()),
            vec![BlockReason::UpstreamUnapproved { upstream: eid(D) }]
        );
        assert_eq!(r.item.integration(), &Integration::Unapproved);
    }

    #[test]
    fn a_draft_spec_on_an_approved_finding_is_draft_and_ok() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = vec![approve(cr(F, f0.hash()), &[])];
        let r = evaluate_all(&f0, &s0, &d0, &events, &[], &[]);
        assert_eq!(r.finding.state().approval(), &Approval::Approved);
        assert_eq!(r.spec.state().approval(), &Approval::Draft);
        assert!(
            is_ok(r.spec.state().upstream()),
            "a Draft is never Stale and here not Blocked"
        );
    }

    #[test]
    fn everything_approved_and_current_is_approved_and_ok() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = approvals(&f0, &s0, &d0);
        let r = evaluate_all(&f0, &s0, &d0, &events, &["PIN-A"], &[]);
        for state in [
            r.finding.state(),
            r.spec.state(),
            r.design.state(),
            r.item.state(),
        ] {
            assert_eq!(state.approval(), &Approval::Approved);
            assert!(is_ok(state.upstream()));
        }
        assert!(r.spec.causes().is_empty());
    }

    #[test]
    fn editing_the_finding_makes_it_modified_and_the_chain_stale() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = approvals(&f0, &s0, &d0);
        let edited = f("body, corrected");
        let r = evaluate_all(&edited, &s0, &d0, &events, &[], &[]);
        assert_eq!(
            r.finding.state().approval(),
            &Approval::Modified {
                approved: *f0.hash()
            }
        );
        // Spec: approved at its own hash; the finding it recorded has changed.
        assert_eq!(r.spec.state().approval(), &Approval::Approved);
        assert_eq!(
            stale_reasons(r.spec.state().upstream()),
            vec![StaleReason::UpstreamChanged {
                upstream: eid(F),
                recorded: (*f0.hash()).into(),
                current: *edited.hash(),
            }]
        );
        // Design and item: stale by inheritance, and the cause chain says why.
        assert_eq!(
            stale_reasons(r.design.state().upstream()),
            vec![StaleReason::UpstreamStale { upstream: eid(S) }]
        );
        assert_eq!(
            stale_reasons(r.item.state().upstream()),
            vec![StaleReason::UpstreamStale { upstream: eid(D) }]
        );
        let chain: Vec<EntityId> = r.item.causes().iter().map(Cause::via).collect();
        assert_eq!(chain, vec![eid(F), eid(S), eid(D)], "root cause first");
        assert!(
            r.item
                .causes()
                .iter()
                .all(|c| matches!(c.reason(), CauseReason::Stale(_)))
        );
    }

    #[test]
    fn a_finding_corrected_and_approved_again_still_leaves_the_spec_stale() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let mut events = approvals(&f0, &s0, &d0);
        let edited = f("body, corrected");
        events.push(approve(cr(F, edited.hash()), &[]));
        let r = evaluate_all(&edited, &s0, &d0, &events, &[], &[]);
        assert_eq!(r.finding.state().approval(), &Approval::Approved);
        assert_eq!(r.spec.state().approval(), &Approval::Approved);
        assert_eq!(stale_reasons(r.spec.state().upstream()).len(), 1);
    }

    #[test]
    fn re_approving_the_spec_against_the_new_finding_clears_the_stale() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let mut events = approvals(&f0, &s0, &d0);
        let edited = f("body, corrected");
        events.push(approve(cr(F, edited.hash()), &[]));
        events.push(approve(
            cs(S, s0.contract_hash(), s0.hyp_hash(), &[(F, *edited.hash())]),
            &[],
        ));
        let r = evaluate_all(&edited, &s0, &d0, &events, &[], &[]);
        assert!(
            is_ok(r.spec.state().upstream()),
            "one approval recorded today's upstream"
        );
        // The design still recorded the old spec hash? The spec hash did not change.
        assert!(is_ok(r.design.state().upstream()));
    }

    #[test]
    fn a_revoked_finding_blocks_the_approved_spec() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let mut events = approvals(&f0, &s0, &d0);
        let a_cr = events[0].clone();
        events.push(revoke(eid(F), a_cr.id(), &[a_cr.id()]));
        let r = evaluate_all(&f0, &s0, &d0, &events, &[], &[]);
        assert_eq!(r.finding.state().approval(), &Approval::Draft);
        assert_eq!(r.spec.state().approval(), &Approval::Approved);
        assert_eq!(
            blocked_reasons(r.spec.state().upstream()),
            vec![BlockReason::UpstreamUnapproved { upstream: eid(F) }]
        );
        assert_eq!(
            blocked_reasons(r.design.state().upstream()),
            vec![BlockReason::UpstreamBlocked { upstream: eid(S) }],
            "the block travels down as a block"
        );
        let chain: Vec<EntityId> = r.design.causes().iter().map(Cause::via).collect();
        assert_eq!(chain, vec![eid(F), eid(S)], "root cause first");
    }

    #[test]
    fn editing_the_spec_itself_makes_it_modified_and_blocks_nothing_above() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = approvals(&f0, &s0, &d0);
        let edited = spec(S, &[F], &["PIN-A", "UT-B", "PIN-C"]);
        let r = evaluate_all(&f0, &edited, &d0, &events, &[], &[]);
        assert_eq!(
            r.spec.state().approval(),
            &Approval::Modified {
                approved: *s0.contract_hash()
            }
        );
        assert!(is_ok(r.spec.state().upstream()));
        // The design recorded the old contract hash and the design now also fails coverage.
        assert!(matches!(r.design.state().approval(), Approval::Invalid(_)));
        assert!(matches!(r.design.state().upstream(), Upstream::Blocked(_)));
    }

    // --- invalid and dangling ---------------------------------------------

    #[test]
    fn a_ground_missing_from_the_map_is_dangling_and_invalid() {
        let l = ledger(&[]);
        let r = eval_spec(&s(), &BTreeMap::new(), &l, &BTreeSet::new(), &[]);
        let Approval::Invalid(reasons) = r.state().approval() else {
            panic!("expected Invalid");
        };
        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            vec![&InvalidReason::DanglingRef { upstream: eid(F) }]
        );
    }

    #[test]
    fn a_ground_written_under_an_identifier_the_document_cannot_see_is_dangling_though_the_model_holds_it()
     {
        let l = ledger(&[]);
        let findings = BTreeMap::from([(fid(F), Evaluator::finding(&f("body"), &l, &[]))]);
        let states = states_of(&findings, &NO_SPECS, &NO_DESIGNS, []);
        let written = Written {
            dangling: BTreeSet::from([eid(F)]),
            ..Written::default()
        };
        let r = Evaluator::spec(&s(), &findings, &states, &l, &written, &[]);
        let Approval::Invalid(reasons) = r.state().approval() else {
            panic!("expected Invalid");
        };
        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            vec![&InvalidReason::DanglingRef { upstream: eid(F) }]
        );
        assert!(!r.state().completeness().is_partial());
        // Without that fact the same model gives a Spec on an upstream that is there.
        let r = Evaluator::spec(&s(), &findings, &states, &l, &Written::default(), &[]);
        assert_eq!(r.state().approval(), &Approval::Draft);
    }

    #[test]
    fn an_unparsed_upstream_blocks_with_upstream_invalid() {
        let l = ledger(&[]);
        let unparsed = Unparsed::new(
            "docs/finding/a.md".parse().unwrap(),
            NonEmptyVec::new(UnparsedReason::MergeConflictMarker, vec![]),
            ContentHash::raw(b"<<<<<<<"),
        );
        let ef: Evaluated<Finding> = Evaluated::from_unparsed(unparsed, &[]);
        assert!(matches!(ef.state().approval(), Approval::Invalid(_)));
        assert!(ef.entity().is_none() && ef.unparsed().is_some());
        let r = eval_spec(
            &s(),
            &BTreeMap::from([(fid(F), ef)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(r.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(F) }]
        );
    }

    /// A document whose kind two branches wrote differently, standing where the
    /// Finding of the chain should be.
    fn conflicted_finding() -> ConflictedDoc {
        use crate::domain::event::test_support::{reclassify, register_doc};
        use crate::domain::event::{ClassChange, DocKind};
        let reg = register_doc(F, DocKind::Finding, "docs/finding/a.md", None, 1);
        let k1 = reclassify(F, &[reg.id()], vec![ClassChange::SetKind(DocKind::Finding)]);
        let k2 = reclassify(F, &[reg.id()], vec![ClassChange::SetKind(DocKind::Spec)]);
        let view = fold(&[reg, k1, k2]);
        ConflictedDoc::of(view.doc(eid(F)).unwrap()).expect("a conflict")
    }

    #[test]
    fn a_ground_that_is_a_document_in_conflict_blocks_the_spec_as_invalid() {
        let l = ledger(&[]);
        let conflicted = conflicted_finding();
        let states = states_of(&NO_FINDINGS, &NO_SPECS, &NO_DESIGNS, [&conflicted]);
        let r = Evaluator::spec(&s(), &NO_FINDINGS, &states, &l, &Written::default(), &[]);
        // The identifier is known, so the Spec is not Invalid for a dangling reference;
        // the ground's state is read like any invalid upstream's.
        assert_eq!(r.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(F) }]
        );
        assert!(!r.state().completeness().is_partial());
    }

    #[test]
    fn a_design_and_an_item_on_a_spec_that_stands_on_a_document_in_conflict_are_blocked() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let conflicted = conflicted_finding();
        let l = ledger(&approvals(&f0, &s0, &d0));
        let states = states_of(&NO_FINDINGS, &NO_SPECS, &NO_DESIGNS, [&conflicted]);
        let es = Evaluator::spec(&s0, &NO_FINDINGS, &states, &l, &Written::default(), &[]);
        // Approved at its own hash, and blocked: the ground cannot be depended on.
        assert_eq!(es.state().approval(), &Approval::Approved);
        assert_eq!(
            blocked_reasons(es.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(F) }]
        );
        let specs = BTreeMap::from([(s0.id(), es)]);
        let states = states_of(&NO_FINDINGS, &specs, &NO_DESIGNS, [&conflicted]);
        let ed = Evaluator::design(&d0, &specs, &states, &l, &Written::default(), &[]);
        assert_eq!(
            blocked_reasons(ed.state().upstream()),
            vec![BlockReason::UpstreamBlocked { upstream: eid(S) }]
        );
        let causes: Vec<EntityId> = ed.causes().iter().map(Cause::via).collect();
        assert_eq!(
            causes,
            vec![eid(F), eid(S)],
            "the root is the document in conflict"
        );
    }

    #[test]
    fn an_unparsed_file_carries_every_reason_and_the_external_ones() {
        let unparsed = Unparsed::new(
            "docs/finding/a.md".parse().unwrap(),
            NonEmptyVec::new(
                UnparsedReason::MergeConflictMarker,
                vec![UnparsedReason::MissingField {
                    field: "title".into(),
                }],
            ),
            ContentHash::raw(b"x"),
        );
        let external = [InvalidReason::PathCollision {
            path: "docs/finding/a.md".parse().unwrap(),
        }];
        let ev: Evaluated<Finding> = Evaluated::from_unparsed(unparsed, &external);
        let Approval::Invalid(reasons) = ev.state().approval() else {
            panic!("expected Invalid");
        };
        assert_eq!(reasons.len(), 3);
    }

    #[test]
    fn external_reasons_make_a_parsed_entity_invalid() {
        let l = ledger(&[]);
        let external = [InvalidReason::AliasCollision {
            alias: "spec/a".parse().unwrap(),
        }];
        let r = Evaluator::finding(&f("b"), &l, &external);
        assert!(matches!(r.state().approval(), Approval::Invalid(_)));
        // And an invalid finding blocks the spec above it.
        let es = eval_spec(
            &s(),
            &BTreeMap::from([(fid(F), r)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(
            blocked_reasons(es.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(F) }]
        );
    }

    #[test]
    fn a_design_whose_spec_is_missing_is_dangling() {
        let l = ledger(&[]);
        let r = eval_design(&d(), &BTreeMap::new(), &l, &BTreeSet::new(), &[]);
        assert!(matches!(r.state().approval(), Approval::Invalid(_)));
    }

    #[test]
    fn pin_coverage_failures_make_the_design_invalid_in_both_directions() {
        let (f0, s0) = (f("body"), s());
        let l = ledger(&approvals(&f0, &s0, &d()));
        let ef = Evaluator::finding(&f0, &l, &[]);
        let es = eval_spec(
            &s0,
            &BTreeMap::from([(fid(F), ef)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        // PIN-A is assigned; UT-B is left out; PIN-Z is not declared.
        let bad = design(D, S, vec![(1, item("do a", &["PIN-A", "PIN-Z"]))]);
        let r = eval_design(
            &bad,
            &BTreeMap::from([(sid(S), es)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let Approval::Invalid(reasons) = r.state().approval() else {
            panic!("expected Invalid");
        };
        assert_eq!(reasons.len(), 2);
    }

    #[test]
    fn an_item_whose_design_is_missing_is_dangling() {
        let l = ledger(&[]);
        let key = ItemKey {
            id: iid(I),
            design: did(D),
            number: num(1),
        };
        let r = eval_item(
            &key,
            &item("x", &["PIN-A"]),
            &BTreeMap::new(),
            &l,
            &view(&[], &[]),
            &[],
        );
        assert!(matches!(r.state().approval(), Approval::Invalid(_)));
    }

    #[test]
    fn a_design_over_an_unparsed_spec_is_blocked_not_checked() {
        let l = ledger(&[]);
        let unparsed = Unparsed::new(
            "docs/spec/a.md".parse().unwrap(),
            NonEmptyVec::new(
                UnparsedReason::MalformedFrontmatter {
                    detail: "bad".into(),
                },
                vec![],
            ),
            ContentHash::raw(b"x"),
        );
        let es: Evaluated<Spec> = Evaluated::from_unparsed(unparsed, &[]);
        let r = eval_design(
            &d(),
            &BTreeMap::from([(sid(S), es)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(r.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(S) }]
        );
    }

    // --- completeness ------------------------------------------------------

    const F2: u128 = 2;

    fn s2() -> Spec {
        spec(S, &[F, F2], &["PIN-A", "UT-B"])
    }

    fn only(ids: &[u128]) -> BTreeSet<EntityId> {
        ids.iter().map(|n| eid(*n)).collect()
    }

    fn reasons_of(state: &EntityState) -> Vec<(EntityId, EntityId)> {
        state
            .completeness()
            .reasons()
            .map(|r| (r.root(), r.upstream()))
            .collect()
    }

    #[test]
    fn a_ground_written_with_a_mark_into_an_unreadable_store_makes_the_spec_partial() {
        let l = ledger(&[]);
        let r = eval_spec(&s(), &BTreeMap::new(), &l, &only(&[F]), &[]);
        assert_eq!(
            r.state().approval(),
            &Approval::Draft,
            "not Invalid: the ground is not missing, it cannot be read"
        );
        assert!(is_ok(r.state().upstream()));
        assert_eq!(reasons_of(r.state()), vec![(eid(F), eid(F))]);
    }

    #[test]
    fn a_ground_that_is_not_marked_unresolved_stays_dangling() {
        let l = ledger(&[]);
        // Another identifier is marked, not this one.
        let r = eval_spec(&s(), &BTreeMap::new(), &l, &only(&[F2]), &[]);
        assert!(matches!(r.state().approval(), Approval::Invalid(_)));
        assert!(!r.state().completeness().is_partial());
    }

    #[test]
    fn a_spec_that_can_read_everything_is_complete() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let r = evaluate_all(&f0, &s0, &d0, &[], &[], &[]);
        for state in [
            r.finding.state(),
            r.spec.state(),
            r.design.state(),
            r.item.state(),
        ] {
            assert_eq!(state.completeness(), &Completeness::Complete);
        }
    }

    #[test]
    fn a_design_marks_its_spec_unresolved_the_same_way() {
        let l = ledger(&[]);
        let r = eval_design(&d(), &BTreeMap::new(), &l, &only(&[S]), &[]);
        assert_eq!(r.state().approval(), &Approval::Draft);
        assert_eq!(reasons_of(r.state()), vec![(eid(S), eid(S))]);
    }

    #[test]
    fn partial_passes_down_keeping_the_root_and_naming_the_upstream() {
        let l = ledger(&[]);
        let es = eval_spec(&s(), &BTreeMap::new(), &l, &only(&[F]), &[]);
        let ed = eval_design(
            &d(),
            &BTreeMap::from([(sid(S), es)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(reasons_of(ed.state()), vec![(eid(F), eid(S))]);
        let key = ItemKey {
            id: iid(I),
            design: did(D),
            number: num(1),
        };
        let it = eval_item(
            &key,
            &d().items()[&num(1)],
            &BTreeMap::from([(did(D), ed)]),
            &l,
            &view(&[], &[]),
            &[],
        );
        assert_eq!(
            reasons_of(it.state()),
            vec![(eid(F), eid(D))],
            "an item takes no set of its own and names its Design"
        );
    }

    #[test]
    fn a_reason_is_kept_once_per_root_and_upstream() {
        let mut gaps = Gaps::default();
        gaps.unreadable(eid(1));
        gaps.unreadable(eid(1));
        gaps.unreadable(eid(2));
        let complete = gaps.finish();
        assert_eq!(complete.reasons().count(), 2);

        // Two upstreams that lead to one root give a reason each.
        let state = |through: u128| {
            EntityState::new(
                Approval::Draft,
                Upstream::Ok,
                Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(
                    eid(9),
                    eid(through),
                )])),
            )
        };
        let mut gaps = Gaps::default();
        gaps.inherit(eid(3), &state(7));
        gaps.inherit(eid(4), &state(8));
        gaps.inherit(eid(4), &state(8));
        let merged = gaps.finish();
        let pairs: Vec<_> = merged.reasons().map(|r| (r.root(), r.upstream())).collect();
        assert_eq!(pairs, vec![(eid(9), eid(3)), (eid(9), eid(4))]);
    }

    #[test]
    fn stale_and_partial_are_both_kept() {
        let (f0, s0, d0) = (f("body"), s2(), d());
        let events = vec![
            approve(cr(F, f0.hash()), &[]),
            approve(
                cs(S, s0.contract_hash(), s0.hyp_hash(), &[(F, *f0.hash())]),
                &[],
            ),
        ];
        let l = ledger(&events);
        let edited = f("body, corrected");
        let ef = Evaluator::finding(&edited, &l, &[]);
        let es = eval_spec(&s0, &BTreeMap::from([(fid(F), ef)]), &l, &only(&[F2]), &[]);
        assert_eq!(es.state().approval(), &Approval::Approved);
        assert_eq!(
            stale_reasons(es.state().upstream()),
            vec![StaleReason::UpstreamChanged {
                upstream: eid(F),
                recorded: (*f0.hash()).into(),
                current: *edited.hash(),
            }]
        );
        assert_eq!(reasons_of(es.state()), vec![(eid(F2), eid(F2))]);
        // The Design under it is Stale by inheritance and Partial by inheritance.
        let ed = eval_design(
            &d0,
            &BTreeMap::from([(sid(S), es)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        assert!(!is_ok(ed.state().upstream()));
        assert_eq!(reasons_of(ed.state()), vec![(eid(F2), eid(S))]);
    }

    #[test]
    fn blocked_and_partial_are_both_kept() {
        let l = ledger(&[]);
        // F is a Draft (blocks); F2 cannot be read.
        let ef = Evaluator::finding(&f("body"), &l, &[]);
        let es = eval_spec(
            &s2(),
            &BTreeMap::from([(fid(F), ef)]),
            &l,
            &only(&[F2]),
            &[],
        );
        assert_eq!(
            blocked_reasons(es.state().upstream()),
            vec![BlockReason::UpstreamUnapproved { upstream: eid(F) }]
        );
        assert_eq!(reasons_of(es.state()), vec![(eid(F2), eid(F2))]);
    }

    #[test]
    fn the_chain_of_one_upstream_is_told_from_the_chain_of_another() {
        let l = ledger(&[]);
        // Both grounds are Drafts: a Cause for each, each reached through its own ground.
        let (f1, f2) = (f("one"), finding(F2, "two"));
        let e1 = Evaluator::finding(&f1, &l, &[]);
        let e2 = Evaluator::finding(&f2, &l, &[]);
        let es = eval_spec(
            &s2(),
            &BTreeMap::from([(fid(F), e1), (fid(F2), e2)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let through: Vec<_> = es.causes().iter().map(|c| (c.via(), c.through())).collect();
        assert_eq!(
            through,
            vec![(eid(F), eid(F)), (eid(F2), eid(F2))],
            "a link knows the upstream it is reached by"
        );
        // And one level down the same links are seen through the Spec.
        let ed = eval_design(
            &d(),
            &BTreeMap::from([(sid(S), es)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let down: Vec<_> = ed.causes().iter().map(|c| (c.via(), c.through())).collect();
        assert_eq!(
            down,
            vec![(eid(F), eid(S)), (eid(F2), eid(S)), (eid(S), eid(S))]
        );
    }

    // --- draft below a stale upstream -------------------------------------

    #[test]
    fn a_draft_on_a_stale_upstream_is_blocked_not_stale() {
        let (f0, s0, d0) = (f("body"), s(), d());
        // Approve finding and spec only; the design stays a draft.
        let events = vec![
            approve(cr(F, f0.hash()), &[]),
            approve(
                cs(S, s0.contract_hash(), s0.hyp_hash(), &[(F, *f0.hash())]),
                &[],
            ),
        ];
        let edited = f("body, corrected");
        let r = evaluate_all(&edited, &s0, &d0, &events, &[], &[]);
        assert_eq!(r.design.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.design.state().upstream()),
            vec![BlockReason::UpstreamStale { upstream: eid(S) }]
        );
    }

    // --- items -------------------------------------------------------------

    #[test]
    fn an_approved_item_is_pending_until_its_revision_reaches_the_baseline() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = approvals(&f0, &s0, &d0);
        let pending = evaluate_all(&f0, &s0, &d0, &events, &["PIN-A"], &[]);
        assert_eq!(
            pending.item.integration(),
            &Integration::ApprovedPending {
                revision: rev("r1")
            }
        );
        let done = evaluate_all(&f0, &s0, &d0, &events, &["PIN-A"], &["r1"]);
        assert_eq!(
            done.item.integration(),
            &Integration::Integrated {
                revision: rev("r1")
            }
        );
    }

    #[test]
    fn an_approval_that_cannot_be_placed_leaves_the_integration_unknown_not_pending() {
        use crate::domain::revision::UnknownIntegration;
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = approvals(&f0, &s0, &d0);
        let l = ledger(&events);
        let ef = Evaluator::finding(&f0, &l, &[]);
        let es = eval_spec(
            &s0,
            &BTreeMap::from([(f0.id(), ef)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let ed = eval_design(
            &d0,
            &BTreeMap::from([(s0.id(), es)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let key = ItemKey {
            id: crate::domain::id::ItemId::new(eid(I)),
            design: d0.id(),
            number: num(1),
        };
        let integration = |fact: IntegrationFact| {
            let view = ImplView::new(
                rev("head"),
                None,
                BTreeMap::new(),
                BTreeMap::from([(rev("r1"), fact)]),
            );
            eval_item(
                &key,
                &d0.items()[&num(1)],
                &BTreeMap::from([(d0.id(), ed.clone())]),
                &l,
                &view,
                &[],
            )
            .integration()
            .clone()
        };
        let reason = UnknownIntegration::BaselineUnresolved;
        assert_eq!(
            integration(IntegrationFact::Unknown { reason }),
            Integration::Unknown {
                revision: rev("r1"),
                reason
            }
        );
        assert_eq!(
            integration(IntegrationFact::NotIntegrated),
            Integration::ApprovedPending {
                revision: rev("r1")
            }
        );
        assert_eq!(
            integration(IntegrationFact::Integrated),
            Integration::Integrated {
                revision: rev("r1")
            }
        );
    }

    #[test]
    fn a_patch_is_integrated_through_its_base() {
        use crate::domain::approval::{Pinned, TestOutcome};
        use crate::domain::revision::Revision;
        let (f0, s0, d0) = (f("body"), s(), d());
        let mut events = approvals(&f0, &s0, &d0);
        events.retain(|e| {
            !matches!(
                e.body(),
                crate::domain::event::EventBody::Approved {
                    attestation: Attestation::Ca { .. },
                    ..
                }
            )
        });
        events.push(approve(
            Attestation::Ca {
                item: Pinned::new(iid(I), d0.item_hash(num(1)).unwrap()),
                design: Pinned::new(did(D), (*d0.design_hash()).into()),
                revision: Revision::Patch {
                    base: rev("b1"),
                    patch: ContentHash::raw(b"diff"),
                },
                tests: TestOutcome::Passed,
            },
            &[],
        ));
        let r = evaluate_all(&f0, &s0, &d0, &events, &[], &["b1"]);
        assert_eq!(
            r.item.integration(),
            &Integration::Integrated {
                revision: rev("b1")
            }
        );
    }

    #[test]
    fn progress_shows_implementing_until_every_assigned_pin_is_in_the_tests() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let none = evaluate_all(&f0, &s0, &d0, &[], &[], &[]);
        assert!(none.item.progress().is_implementing());
        assert_eq!(
            none.item.state().approval(),
            &Approval::Draft,
            "implementing is never Invalid"
        );
        let all = evaluate_all(&f0, &s0, &d0, &[], &["PIN-A"], &[]);
        assert!(!all.item.progress().is_implementing());
        let other = evaluate_all(&f0, &s0, &d0, &[], &["UT-B"], &[]);
        assert!(
            other.item.progress().is_implementing(),
            "a pin of another item does not count"
        );
    }

    #[test]
    fn an_edited_item_is_modified() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let events = approvals(&f0, &s0, &d0);
        // Same design id and number, other summary.
        let d1 = design(
            D,
            S,
            vec![
                (1, item("do a differently", &["PIN-A"])),
                (2, item("do b", &["UT-B"])),
            ],
        );
        let r = evaluate_all(&f0, &s0, &d1, &events, &["PIN-A"], &[]);
        assert_eq!(
            r.item.state().approval(),
            &Approval::Modified {
                approved: d0.item_hash(num(1)).unwrap()
            }
        );
        assert_eq!(r.item.integration(), &Integration::Unapproved);
        // The design hash changed too, so the item sees a stale (or blocked) design.
        assert!(!is_ok(r.item.state().upstream()));
    }

    #[test]
    fn two_approvals_of_one_hash_stale_only_if_neither_recorded_todays_upstream() {
        let (f0, s0, d0) = (f("body"), s(), d());
        let edited = f("body, corrected");
        let mut events = vec![
            approve(cr(F, f0.hash()), &[]),
            approve(cr(F, edited.hash()), &[]),
            // Two CS approvals of the same contract hash: one against each finding version.
            approve(
                cs(S, s0.contract_hash(), s0.hyp_hash(), &[(F, *f0.hash())]),
                &[],
            ),
        ];
        let stale = evaluate_all(&edited, &s0, &d0, &events, &[], &[]);
        assert_eq!(stale_reasons(stale.spec.state().upstream()).len(), 1);
        events.push(approve(
            cs(S, s0.contract_hash(), s0.hyp_hash(), &[(F, *edited.hash())]),
            &[],
        ));
        let fresh = evaluate_all(&edited, &s0, &d0, &events, &[], &[]);
        assert!(is_ok(fresh.spec.state().upstream()));
    }
}
