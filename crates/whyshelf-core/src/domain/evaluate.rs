//! The evaluator: from content and approvals to a state, upstream to downstream.
//!
//! Evaluation runs in the order Finding, Spec, Design, Item. Each stage takes the
//! stage above it as an already evaluated map, so a Spec is evaluated against
//! `BTreeMap<FindingId, Evaluated<Finding>>`, a Design against the evaluated Specs,
//! and an item against the evaluated Designs. The result of a stage carries its own
//! [`EntityState`] and a chain of [`Cause`]s that says where a Stale or Blocked came
//! from.
//!
//! # How a state is decided
//!
//! 1. **Invalid** if the entity's own content is unusable: it did not parse
//!    ([`Unparsed`]), an upstream identifier names nothing in the map (dangling), the
//!    Design does not cover the Spec's pins exactly, or the caller reports a
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
//! When several approvals exist for one `(gate, entity, hash)`, the entity is Stale
//! only if none of them recorded the upstream as it is today. Two worktrees may have
//! approved the same content at different times and either one suffices.
//!
//! # Inputs from the application
//!
//! The application resolves aliases to identifiers and parses files before calling
//! here. A file that failed to parse is passed as [`Unparsed`] and evaluated as
//! Invalid. What the code scan found and which revisions reached the baseline come
//! in as an [`ImplView`]; it names no version-control tool.

use std::collections::{BTreeMap, BTreeSet};

use super::approval::{ApprovalSet, Attestation, Gate, UpstreamRecord};
use super::design::{Design, WorkItem};
use super::finding::Finding;
use super::hash::ContentHash;
use super::id::{DesignId, EntityId, FindingId, ItemId, ItemNumber, SpecId};
use super::location::{CodeLocation, RelPath};
use super::nonempty::NonEmptyVec;
use super::pin::PinId;
use super::revision::RevisionId;
use super::spec::Spec;
use super::state::{
    Approval, BlockReason, EntityState, Integration, InvalidReason, ItemProgress, StaleReason,
    UnparsedReason, Upstream,
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
    reason: CauseReason,
}

impl Cause {
    /// The upstream entity this link goes through.
    pub fn via(&self) -> EntityId {
        self.via
    }

    /// What that upstream did to the entity.
    pub fn reason(&self) -> &CauseReason {
        &self.reason
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
            ),
            causes: Vec::new(),
        }
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
    integrated: BTreeSet<RevisionId>,
}

impl ImplView {
    /// The view: the current revision, a label for display only, the pins found in
    /// the tests with where they were found, and the approved revisions that have
    /// reached the baseline.
    pub fn new(
        current_revision: RevisionId,
        workspace_label: Option<String>,
        pins_at_revision: BTreeMap<PinId, Vec<CodeLocation>>,
        integrated: BTreeSet<RevisionId>,
    ) -> Self {
        Self {
            current_revision,
            workspace_label,
            pins_at_revision,
            integrated,
        }
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

    /// Whether a revision has reached the baseline.
    pub fn is_integrated(&self, revision: &RevisionId) -> bool {
        self.integrated.contains(revision)
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

struct UpView<'a> {
    id: EntityId,
    approval: &'a Approval,
    upstream: &'a Upstream,
    causes: &'a [Cause],
    hash: Option<&'a ContentHash>,
}

impl<'a, T> From<(EntityId, &'a Evaluated<T>, Option<&'a ContentHash>)> for UpView<'a> {
    fn from((id, ev, hash): (EntityId, &'a Evaluated<T>, Option<&'a ContentHash>)) -> Self {
        Self {
            id,
            approval: ev.state.approval(),
            upstream: ev.state.upstream(),
            causes: &ev.causes,
            hash,
        }
    }
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
            self.block_causes.extend_from_slice(up.causes);
        }
        self.block_causes.push(Cause {
            via: up.id,
            reason: CauseReason::Blocked(reason.clone()),
        });
        self.blocks.push(reason);
    }

    fn stale(&mut self, up: &UpView<'_>, reason: StaleReason, inherit: bool) {
        if inherit {
            self.stale_causes.extend_from_slice(up.causes);
        }
        self.stale_causes.push(Cause {
            via: up.id,
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

/// The hash-level staleness of an approved entity: for each upstream, whether
/// some matching approval recorded it as it is today.
fn hash_staleness(
    basis: &Basis<'_>,
    ups: &[UpView<'_>],
    ledger: &ApprovalSet,
) -> Vec<(EntityId, StaleReason)> {
    let currents: Vec<(EntityId, &ContentHash)> = ups
        .iter()
        .filter_map(|u| u.hash.map(|h| (u.id, h)))
        .collect();
    let mut first_failure: Option<Vec<(EntityId, StaleReason)>> = None;
    for record in ledger.matching(basis.gate, basis.subject, basis.hash) {
        let recorded = record.attestation().upstream();
        let reasons: Vec<(EntityId, StaleReason)> = currents
            .iter()
            .filter_map(|(id, current)| {
                let at_approval = recorded.iter().find_map(|r| match r {
                    UpstreamRecord::Entity { id: rid, hash }
                        if ledger.canonical(*rid) == ledger.canonical(*id) =>
                    {
                        Some(hash)
                    }
                    _ => None,
                });
                match at_approval {
                    Some(h) if h == *current => None,
                    Some(h) => Some((
                        *id,
                        StaleReason::UpstreamChanged {
                            upstream: *id,
                            recorded: *h,
                            current: **current,
                        },
                    )),
                    None => Some((*id, StaleReason::UpstreamNotRecorded { upstream: *id })),
                }
            })
            .collect();
        if reasons.is_empty() {
            return Vec::new();
        }
        first_failure.get_or_insert(reasons);
    }
    first_failure.unwrap_or_default()
}

fn assess(
    basis: Option<Basis<'_>>,
    ups: &[UpView<'_>],
    ledger: &ApprovalSet,
) -> (Upstream, Vec<Cause>) {
    let mut health = Health::default();
    let mut changed: BTreeSet<EntityId> = BTreeSet::new();
    let approved = basis.is_some();
    if let Some(basis) = &basis {
        for (id, reason) in hash_staleness(basis, ups, ledger) {
            changed.insert(id);
            if let Some(up) = ups.iter().find(|u| u.id == id) {
                health.stale(up, reason, false);
            }
        }
    }
    for up in ups {
        let blocked_by_approval = match up.approval {
            Approval::Invalid(_) => Some(BlockReason::UpstreamInvalid { upstream: up.id }),
            Approval::Draft => Some(BlockReason::UpstreamUnapproved { upstream: up.id }),
            Approval::Modified { .. } if !(approved && changed.contains(&up.id)) => {
                Some(BlockReason::UpstreamUnapproved { upstream: up.id })
            }
            Approval::Modified { .. } | Approval::Approved => None,
        };
        if let Some(reason) = blocked_by_approval {
            health.block(up, reason, true);
            continue;
        }
        match up.upstream {
            Upstream::Ok => {}
            Upstream::Blocked(_) => {
                health.block(up, BlockReason::UpstreamBlocked { upstream: up.id }, true);
            }
            Upstream::Stale(_) if approved => {
                health.stale(up, StaleReason::UpstreamStale { upstream: up.id }, true);
            }
            Upstream::Stale(_) => {
                health.block(up, BlockReason::UpstreamStale { upstream: up.id }, true);
            }
        }
    }
    health.finish()
}

fn own_approval(
    gate: Gate,
    subject: EntityId,
    hash: &ContentHash,
    ledger: &ApprovalSet,
    invalid: Vec<InvalidReason>,
) -> Approval {
    if let Ok(reasons) = NonEmptyVec::from_vec(invalid) {
        return Approval::Invalid(reasons);
    }
    if ledger.is_approved(gate, subject, hash) {
        Approval::Approved
    } else if let Some(approved) = ledger.latest_hash(gate, subject) {
        Approval::Modified {
            approved: *approved,
        }
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
        ledger: &ApprovalSet,
        external: &[InvalidReason],
    ) -> Evaluated<Finding> {
        let approval = own_approval(
            Gate::Cr,
            f.id().entity(),
            f.hash(),
            ledger,
            external.to_vec(),
        );
        Evaluated {
            subject: Subject::Parsed(f.clone()),
            state: EntityState::new(approval, Upstream::Ok),
            causes: Vec::new(),
        }
    }

    /// Evaluate a Spec against its evaluated grounds.
    ///
    /// A ground missing from `findings` makes the Spec Invalid (dangling).
    pub fn spec(
        s: &Spec,
        findings: &BTreeMap<FindingId, Evaluated<Finding>>,
        ledger: &ApprovalSet,
        external: &[InvalidReason],
    ) -> Evaluated<Spec> {
        let mut invalid = external.to_vec();
        let mut ups = Vec::new();
        for ground in s.grounds().iter() {
            match findings.get(ground) {
                None => invalid.push(InvalidReason::DanglingRef {
                    upstream: ground.entity(),
                }),
                Some(ev) => ups.push(UpView::from((
                    ground.entity(),
                    ev,
                    ev.entity().map(Finding::hash),
                ))),
            }
        }
        let subject = s.id().entity();
        let approval = own_approval(Gate::Cs, subject, s.contract_hash(), ledger, invalid);
        let basis = basis_if_approved(&approval, Gate::Cs, subject, s.contract_hash());
        let (upstream, causes) = assess(basis, &ups, ledger);
        Evaluated {
            subject: Subject::Parsed(s.clone()),
            state: EntityState::new(approval, upstream),
            causes,
        }
    }

    /// Evaluate a Design against its evaluated Spec.
    ///
    /// A Spec missing from `specs` makes the Design Invalid (dangling). When the
    /// Spec parsed, its pins are checked against the Design's assignment and every
    /// mismatch is an Invalid reason.
    pub fn design(
        d: &Design,
        specs: &BTreeMap<SpecId, Evaluated<Spec>>,
        ledger: &ApprovalSet,
        external: &[InvalidReason],
    ) -> Evaluated<Design> {
        let mut invalid = external.to_vec();
        let mut ups = Vec::new();
        match specs.get(&d.spec()) {
            None => invalid.push(InvalidReason::DanglingRef {
                upstream: d.spec().entity(),
            }),
            Some(ev) => {
                if let Some(spec) = ev.entity() {
                    invalid.extend(
                        d.check_pin_coverage(spec)
                            .into_iter()
                            .map(InvalidReason::PinCoverage),
                    );
                }
                ups.push(UpView::from((
                    d.spec().entity(),
                    ev,
                    ev.entity().map(Spec::contract_hash),
                )));
            }
        }
        let subject = d.id().entity();
        let approval = own_approval(Gate::Ci, subject, d.design_hash(), ledger, invalid);
        let basis = basis_if_approved(&approval, Gate::Ci, subject, d.design_hash());
        let (upstream, causes) = assess(basis, &ups, ledger);
        Evaluated {
            subject: Subject::Parsed(d.clone()),
            state: EntityState::new(approval, upstream),
            causes,
        }
    }

    /// Evaluate a work item against its evaluated Design, with the integration and
    /// the progress.
    pub fn item(
        key: &ItemKey,
        item: &WorkItem,
        designs: &BTreeMap<DesignId, Evaluated<Design>>,
        ledger: &ApprovalSet,
        impl_view: &ImplView,
        external: &[InvalidReason],
    ) -> ItemEvaluation {
        let mut invalid = external.to_vec();
        let mut ups = Vec::new();
        match designs.get(&key.design) {
            None => invalid.push(InvalidReason::DanglingRef {
                upstream: key.design.entity(),
            }),
            Some(ev) => ups.push(UpView::from((
                key.design.entity(),
                ev,
                ev.entity().map(Design::design_hash),
            ))),
        }
        let hash = item.hash(key.design, key.number);
        let subject = key.id.entity();
        let approval = own_approval(Gate::Ca, subject, &hash, ledger, invalid);
        let basis = basis_if_approved(&approval, Gate::Ca, subject, &hash);
        let (upstream, causes) = assess(basis, &ups, ledger);
        let integration = if matches!(approval, Approval::Approved) {
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
            state: EntityState::new(approval, upstream),
            causes,
            integration,
            progress: ItemProgress::new(found, declared),
        }
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
    if let Some(done) = revisions.iter().find(|r| impl_view.is_integrated(r)) {
        return Integration::Integrated {
            revision: (*done).clone(),
        };
    }
    match revisions.first() {
        Some(first) => Integration::ApprovedPending {
            revision: (*first).clone(),
        },
        None => Integration::Unapproved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::test_support::{approve, ca, ci, cr, cs, revoke};
    use crate::domain::classification::fold;
    use crate::domain::design::test_support::{design, item, num};
    use crate::domain::event::Event;
    use crate::domain::finding::test_support::finding;
    use crate::domain::id::test_support::{did, eid, fid, iid, sid};
    use crate::domain::spec::test_support::spec;

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
            integrated.iter().map(|r| rev(r)).collect(),
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
        let es = Evaluator::spec(
            spec_,
            &BTreeMap::from([(finding_.id(), ef.clone())]),
            &l,
            &[],
        );
        let ed = Evaluator::design(
            design_,
            &BTreeMap::from([(spec_.id(), es.clone())]),
            &l,
            &[],
        );
        let key = ItemKey {
            id: crate::domain::id::ItemId::new(eid(I)),
            design: design_.id(),
            number: num(1),
        };
        let it = Evaluator::item(
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
                recorded: *f0.hash(),
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
        let r = Evaluator::spec(&s(), &BTreeMap::new(), &l, &[]);
        let Approval::Invalid(reasons) = r.state().approval() else {
            panic!("expected Invalid");
        };
        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            vec![&InvalidReason::DanglingRef { upstream: eid(F) }]
        );
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
        let r = Evaluator::spec(&s(), &BTreeMap::from([(fid(F), ef)]), &l, &[]);
        assert_eq!(r.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(F) }]
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
        let es = Evaluator::spec(&s(), &BTreeMap::from([(fid(F), r)]), &l, &[]);
        assert_eq!(
            blocked_reasons(es.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(F) }]
        );
    }

    #[test]
    fn a_design_whose_spec_is_missing_is_dangling() {
        let l = ledger(&[]);
        let r = Evaluator::design(&d(), &BTreeMap::new(), &l, &[]);
        assert!(matches!(r.state().approval(), Approval::Invalid(_)));
    }

    #[test]
    fn pin_coverage_failures_make_the_design_invalid_in_both_directions() {
        let (f0, s0) = (f("body"), s());
        let l = ledger(&approvals(&f0, &s0, &d()));
        let ef = Evaluator::finding(&f0, &l, &[]);
        let es = Evaluator::spec(&s0, &BTreeMap::from([(fid(F), ef)]), &l, &[]);
        // PIN-A is assigned; UT-B is left out; PIN-Z is not declared.
        let bad = design(D, S, vec![(1, item("do a", &["PIN-A", "PIN-Z"]))]);
        let r = Evaluator::design(&bad, &BTreeMap::from([(sid(S), es)]), &l, &[]);
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
        let r = Evaluator::item(
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
        let r = Evaluator::design(&d(), &BTreeMap::from([(sid(S), es)]), &l, &[]);
        assert_eq!(r.state().approval(), &Approval::Draft);
        assert_eq!(
            blocked_reasons(r.state().upstream()),
            vec![BlockReason::UpstreamInvalid { upstream: eid(S) }]
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
                design: Pinned::new(did(D), *d0.design_hash()),
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
