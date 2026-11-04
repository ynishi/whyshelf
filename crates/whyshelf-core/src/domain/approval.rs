//! Gates, attestations and the set of approvals.
//!
//! # Gates
//!
//! Each stage has a CHECK gate with two layers. Layer 1 is a machine check of
//! form (the `lint` module); layer 2 is a human judgement of meaning, recorded as
//! an `Approved` event, and it can only be recorded after layer 1 passes.
//!
//! | gate | target | layer 1, machine | layer 2, judgement | by |
//! |---|---|---|---|---|
//! | CR | Finding | the required metadata of its type is there | the source or method is trustworthy; viewpoints are independent | Reviewer |
//! | CS | Spec | every ground exists, passed CR and is not stale; pins and hypotheses are declared | the claim follows from the Findings; the declared pins and hypotheses bind the Contract | User |
//! | CI | Design | the Spec's current hash passed CS and is not stale; every pin is assigned and none is undeclared; the Crux has rejection reasons | the approach is sound; the items are small enough | Reviewer |
//! | CA | Item | the Design's current hash passed CI and is not stale; every assigned pin exists in the tests at the approved revision, and no tag in the tests is undeclared; the tests pass | the tests are not hollow, judged against the Spec, the Design and the diff, setting the implementer's account aside | independent Reviewer |
//! | CM | measurement | the hypothesis hash fixed before the run, and the same hypothesis measured | the measurement met the hypothesis; if not, say whether it is a wiring gap (add an item to the Design) or a wrong hypothesis (new `measured_data` Finding, revise the Spec) | whoever reads the measurement |
//!
//! `by` is a declared name, as a Git author is. Nothing authenticates it.
//!
//! # The revision of a CA approval
//!
//! A CA approval records the current revision automatically; the person does not
//! type a hash. An older revision can be named explicitly. A dirty working tree
//! stops the approval ("commit first"); there is no override. The pin check and
//! the test run use the same revision that is recorded, and the pin check looks
//! only at the test files of the settings (the application passes the tags it found
//! as an `ImplView`). A test run that does not finish within its time limit has no
//! outcome ([`TestsRun`]): nothing is approved, and the gate says so.
//!
//! # What an approval records: [`Attestation`]
//!
//! The variants of [`Attestation`] are the one statement of this table; a test reads
//! [`Attestation::target_hash`] and [`Attestation::upstream`] of each gate against it.
//!
//! | gate | target `(id, hash)` | upstream recorded | what "Modified" compares |
//! |---|---|---|---|
//! | CR | Finding, its hash | none | the Finding's hash |
//! | CS | Spec, `contract_hash` | every ground, `(id, hash)` | `contract_hash` |
//! | CI | Design, `design_hash` | the Spec, `(id, contract_hash)` | `design_hash` |
//! | CA | item, its item hash | the Design, `(id, design_hash)` | the item hash |
//! | CM | Spec, `hyp_hash` | the event id of the CS approval that fixed that `hyp_hash` | nothing: a CM is a one-time record |
//!
//! A CS approval also records the `hyp_hash` it fixed, so a later CM can find it
//! ([`ApprovalSet::cs_for_cm`]).
//!
//! # An approval can be recorded only through a gate
//!
//! Layer 1 of each gate is a function of the [`gate`](super::gate) module that
//! returns a [`Passed`](super::gate::Passed) or every violation. The `Approved`
//! event is built from a `Passed` and from nothing else on the writing side: the
//! recorded body of an event, [`RecordBody`](super::event::RecordBody), has no
//! `Approved` variant.
//!
//! The same, drawn along the chain. Each approval is an event that names its own
//! target at a hash (`=>`) and the upstream facts it stood on (`->`); a later change
//! of any of those hashes is what makes the approval Stale or Modified.
//!
//! ```text
//!   Finding      CR  => Finding @ finding hash
//!      ^                (no upstream)
//!      |
//!   Spec         CS  => Spec @ contract_hash          + hyp_hash fixed here
//!      ^                -> each ground: Finding @ its hash
//!      |
//!      +-------- CM  => Spec @ hyp_hash               + result (pass |
//!      |                -> the id of the CS event        fail-wiring | fail-hyp)
//!      |                   that fixed that hyp_hash   (a record, never Modified)
//!   Design       CI  => Design @ design_hash
//!      ^                -> Spec @ contract_hash
//!      |
//!   Item         CA  => Item @ item hash              + revision + test outcome
//!                       -> Design @ design_hash
//! ```
//!
//! A hash that is not in an approval's own row does not matter to it: editing a
//! Design Notes section changes no `design_hash`, and a Design records the Spec's
//! `contract_hash`, so any change to the Spec's pins, hypotheses, grounds or
//! Contract reaches the Design, while a change to the Spec's supplementary
//! sections does not.
//!
//! # Approved is a question about a set
//!
//! "Is `(gate, id, hash)` approved?" is answered from the whole set of events: an
//! `Approved` exists and no `Revoked` names it. The answer is never "does it match
//! the latest approval of that id", because another worktree or branch may have
//! approved a different version of the same entity and the two must not break each
//! other. "Latest" is used for display (audit, and the hash shown by Modified).
//!
//! # Known limitation
//!
//! Approvals stay in the private store when an entity is published, so an entity
//! published to the repo store appears as Draft in a clone that cannot see the
//! private store.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use super::actor::{Actor, Timestamp};
use super::canon::{Canon, ToCanon};
use super::classification::ClassificationView;
use super::event::{DocKind, EntityKind, Event, EventBody, EventGraph, EventId};
use super::hash::ContentHash;
use super::id::{DesignId, EntityId, FindingId, ItemId, SpecId};
use super::nonempty::{NonBlank, NonEmptyVec};
use super::revision::Revision;
use super::vocabulary::closed_vocabulary;

/// One of the five gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gate {
    /// Validity of a Finding.
    Cr,
    /// The Contract and its acceptance declaration.
    Cs,
    /// The split into items and the assignment of pins.
    Ci,
    /// The implementation of an item against the Contract.
    Ca,
    /// The measurement of the hypotheses.
    Cm,
}

impl Gate {
    /// Every gate.
    pub const ALL: [Gate; 5] = [Gate::Cr, Gate::Cs, Gate::Ci, Gate::Ca, Gate::Cm];

    /// The two-letter name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cr => "CR",
            Self::Cs => "CS",
            Self::Ci => "CI",
            Self::Ca => "CA",
            Self::Cm => "CM",
        }
    }

    /// The kind of entity the gate's approval is about.
    pub fn target_kind(self) -> EntityKind {
        match self {
            Self::Cr => EntityKind::Finding,
            Self::Cs | Self::Cm => EntityKind::Spec,
            Self::Ci => EntityKind::Design,
            Self::Ca => EntityKind::Item,
        }
    }

    /// The gate that approves a document of this kind: the way back from
    /// [`target_kind`](Self::target_kind). A Spec has two (CS approves its Contract,
    /// CM records its measurement); the one that approves it is CS.
    pub fn of_doc(kind: DocKind) -> Gate {
        match kind {
            DocKind::Finding => Self::Cr,
            DocKind::Spec => Self::Cs,
            DocKind::Design => Self::Ci,
        }
    }
}

/// An identifier together with the hash it was approved at.
///
/// Appears in approval events only. An entity never holds the hash of its
/// upstream: the approval does.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pinned<I> {
    id: I,
    hash: ContentHash,
}

impl<I> Pinned<I> {
    /// An identifier pinned at a hash.
    pub fn new(id: I, hash: ContentHash) -> Self {
        Self { id, hash }
    }

    /// The identifier.
    pub fn id(&self) -> &I {
        &self.id
    }

    /// The hash.
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }
}

impl<I: ToCanon> ToCanon for Pinned<I> {
    fn to_canon(&self) -> Canon {
        Canon::map([("id", self.id.to_canon()), ("hash", self.hash.to_canon())])
    }
}

/// The outcome of the tests run for a CA approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TestOutcome {
    /// The tests passed.
    Passed,
    /// The tests failed.
    Failed,
}

/// What running the tests for a CA approval came to.
///
/// A timeout is not a [`TestOutcome`]: the outcome is a fact that gets recorded, and
/// a command that was stopped before it finished has no result to record. It is a
/// violation of the gate instead, and nothing is approved. This is why the recorded
/// outcome, the event format and the hashes did not change with the time limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestsRun {
    /// The command finished, and this is its outcome.
    Finished(TestOutcome),
    /// The command was still running after this long and was stopped.
    TimedOut {
        /// The time limit that applied.
        after: Duration,
    },
}

/// The result recorded by a CM.
///
/// A failure is split in two, and each part carries what the follow-up needs,
/// so the data cannot be left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmResult {
    /// The measurement met the hypotheses.
    Pass,
    /// A wiring or implementation gap: add an item to the Design.
    FailWiring {
        /// A hint at which item is missing, free text.
        item_hint: NonBlank,
    },
    /// The hypothesis was wrong: a new `measured_data` Finding records the
    /// measurement and the Spec is to be revised.
    FailHyp {
        /// The Finding that records the measurement.
        finding: FindingId,
    },
}

/// Which of the three results a CM records, without what each carries.
///
/// The words of the command line and the tool server: `pass`, `fail-wiring`,
/// `fail-hyp`. Those two adapters keep their own input shapes (a failure there
/// carries its flag), and their names are tested against this list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CmResultKind {
    /// The measurement met the hypotheses.
    Pass,
    /// A wiring or implementation gap.
    FailWiring,
    /// The hypothesis was wrong.
    FailHyp,
}

closed_vocabulary!(CmResultKind, "measurement result", {
    Pass => "pass",
    FailWiring => "fail-wiring",
    FailHyp => "fail-hyp",
});

impl CmResult {
    /// Which result this is.
    pub fn kind(&self) -> CmResultKind {
        match self {
            Self::Pass => CmResultKind::Pass,
            Self::FailWiring { .. } => CmResultKind::FailWiring,
            Self::FailHyp { .. } => CmResultKind::FailHyp,
        }
    }
}

impl ToCanon for CmResult {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Pass => Canon::map([("result", Canon::str("pass"))]),
            Self::FailWiring { item_hint } => Canon::map([
                ("result", Canon::str("fail-wiring")),
                ("item_hint", item_hint.to_canon()),
            ]),
            Self::FailHyp { finding } => Canon::map([
                ("result", Canon::str("fail-hyp")),
                ("finding", finding.to_canon()),
            ]),
        }
    }
}

/// An upstream fact recorded by an approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamRecord {
    /// An entity at a hash.
    Entity {
        /// The upstream entity.
        id: EntityId,
        /// Its hash when the approval was made.
        hash: ContentHash,
    },
    /// An earlier approval event (for a CM, the CS that fixed the hypothesis).
    Event(EventId),
}

/// What a gate's approval says, one variant per gate.
///
/// A sum type rather than a record with optional parts: a CA approval always has
/// a revision and a test outcome, a CM always has a result, and no other gate has
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attestation {
    /// A Finding passed CR.
    Cr {
        /// The Finding at the hash approved.
        finding: Pinned<FindingId>,
    },
    /// A Spec passed CS.
    Cs {
        /// The Spec at its `contract_hash`.
        spec: Pinned<SpecId>,
        /// The `hyp_hash` fixed by this approval.
        hyp: ContentHash,
        /// Every ground, at the hash it had.
        grounds: NonEmptyVec<Pinned<FindingId>>,
    },
    /// A Design passed CI.
    Ci {
        /// The Design at its `design_hash`.
        design: Pinned<DesignId>,
        /// The Spec at the `contract_hash` it had.
        spec: Pinned<SpecId>,
    },
    /// An item passed CA.
    Ca {
        /// The item at its item hash.
        item: Pinned<ItemId>,
        /// The Design at the `design_hash` it had.
        design: Pinned<DesignId>,
        /// The revision of the code that was checked.
        revision: Revision,
        /// The outcome of the tests at that revision.
        tests: TestOutcome,
    },
    /// A measurement was read.
    Cm {
        /// The Spec at the `hyp_hash` that was measured.
        spec: Pinned<SpecId>,
        /// The CS approval that fixed that `hyp_hash`.
        cs: EventId,
        /// The result.
        result: CmResult,
    },
}

impl Attestation {
    /// The gate.
    pub fn gate(&self) -> Gate {
        match self {
            Self::Cr { .. } => Gate::Cr,
            Self::Cs { .. } => Gate::Cs,
            Self::Ci { .. } => Gate::Ci,
            Self::Ca { .. } => Gate::Ca,
            Self::Cm { .. } => Gate::Cm,
        }
    }

    /// The entity approved.
    pub fn subject(&self) -> EntityId {
        match self {
            Self::Cr { finding } => finding.id.entity(),
            Self::Cs { spec, .. } | Self::Cm { spec, .. } => spec.id.entity(),
            Self::Ci { design, .. } => design.id.entity(),
            Self::Ca { item, .. } => item.id.entity(),
        }
    }

    /// The hash pinned for the entity approved: the Finding's hash, the Spec's
    /// `contract_hash` (CS) or `hyp_hash` (CM), the Design's `design_hash`, the item
    /// hash (see the table of the module).
    pub fn target_hash(&self) -> &ContentHash {
        match self {
            Self::Cr { finding } => &finding.hash,
            Self::Cs { spec, .. } | Self::Cm { spec, .. } => &spec.hash,
            Self::Ci { design, .. } => &design.hash,
            Self::Ca { item, .. } => &item.hash,
        }
    }

    /// The revision of the code the approval was made at: only a CA has one.
    pub fn revision(&self) -> Option<&Revision> {
        match self {
            Self::Ca { revision, .. } => Some(revision),
            Self::Cr { .. } | Self::Cs { .. } | Self::Ci { .. } | Self::Cm { .. } => None,
        }
    }

    /// The upstream facts recorded (see the table of the module).
    pub fn upstream(&self) -> Vec<UpstreamRecord> {
        let entity = |id: EntityId, hash: &ContentHash| UpstreamRecord::Entity { id, hash: *hash };
        match self {
            Self::Cr { .. } => Vec::new(),
            Self::Cs { grounds, .. } => grounds
                .iter()
                .map(|g| entity(g.id.entity(), &g.hash))
                .collect(),
            Self::Ci { spec, .. } => vec![entity(spec.id.entity(), &spec.hash)],
            Self::Ca { design, .. } => vec![entity(design.id.entity(), &design.hash)],
            Self::Cm { cs, .. } => vec![UpstreamRecord::Event(*cs)],
        }
    }
}

impl ToCanon for Attestation {
    fn to_canon(&self) -> Canon {
        let gate = ("gate", Canon::str(self.gate().name()));
        match self {
            Self::Cr { finding } => Canon::map([gate, ("target", finding.to_canon())]),
            Self::Cs { spec, hyp, grounds } => Canon::map([
                gate,
                ("target", spec.to_canon()),
                ("hyp", hyp.to_canon()),
                ("grounds", grounds.to_canon()),
            ]),
            Self::Ci { design, spec } => Canon::map([
                gate,
                ("target", design.to_canon()),
                ("spec", spec.to_canon()),
            ]),
            Self::Ca {
                item,
                design,
                revision,
                tests,
            } => Canon::map([
                gate,
                ("target", item.to_canon()),
                ("design", design.to_canon()),
                ("revision", revision.to_canon()),
                (
                    "tests",
                    Canon::str(match tests {
                        TestOutcome::Passed => "passed",
                        TestOutcome::Failed => "failed",
                    }),
                ),
            ]),
            Self::Cm { spec, cs, result } => Canon::map([
                gate,
                ("target", spec.to_canon()),
                ("cs", cs.to_canon()),
                ("result", result.to_canon()),
            ]),
        }
    }
}

/// An `Approved` event as the ledger keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRecord {
    event: EventId,
    subject: EntityId,
    by: Actor,
    at: Timestamp,
    attestation: Attestation,
}

impl ApprovalRecord {
    /// The identifier of the `Approved` event.
    pub fn event(&self) -> EventId {
        self.event
    }

    /// The entity approved, after duplicate registrations are folded.
    pub fn subject(&self) -> EntityId {
        self.subject
    }

    /// Who approved, as declared.
    pub fn by(&self) -> &Actor {
        &self.by
    }

    /// When it was recorded. Display only.
    pub fn at(&self) -> &Timestamp {
        &self.at
    }

    /// What was approved.
    pub fn attestation(&self) -> &Attestation {
        &self.attestation
    }
}

/// A CS approval and the hypothesis hash it fixed: what a CM is set against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsFixing {
    event: EventId,
    hyp: ContentHash,
}

impl CsFixing {
    /// The CS approval.
    pub fn event(&self) -> EventId {
        self.event
    }

    /// The `hyp_hash` it fixed.
    pub fn hyp(&self) -> &ContentHash {
        &self.hyp
    }
}

/// The approvals in a set of events, and the question "is this approved?".
///
/// Built from events and the classification fold (which folds duplicate
/// registrations, so an approval recorded against a duplicate identifier counts
/// for the surviving one).
#[derive(Debug, Clone, Default)]
pub struct ApprovalSet {
    records: Vec<ApprovalRecord>,
    revoked: BTreeSet<EventId>,
    canonical: BTreeMap<EntityId, EntityId>,
    latest: BTreeMap<(Gate, EntityId), ContentHash>,
}

impl ApprovalSet {
    /// Collect the approvals and revocations in `events`.
    pub fn from_events(events: &[Event], view: &ClassificationView) -> Self {
        let graph = EventGraph::new(events);
        let mut records = Vec::new();
        let mut revoked = BTreeSet::new();
        for event in graph.events() {
            match event.body() {
                EventBody::Approved { by, attestation } => records.push(ApprovalRecord {
                    event: event.id(),
                    subject: view.canonical(event.target()),
                    by: by.clone(),
                    at: event.at().clone(),
                    attestation: attestation.clone(),
                }),
                EventBody::Revoked { revokes, .. } => {
                    revoked.insert(*revokes);
                }
                _ => {}
            }
        }
        // The latest active approval of each (gate, subject), by ancestry: the
        // tips among the active approvals; ties go to the smallest event id.
        let mut groups: BTreeMap<(Gate, EntityId), BTreeSet<EventId>> = BTreeMap::new();
        for record in records.iter().filter(|r| !revoked.contains(&r.event)) {
            groups
                .entry((record.attestation.gate(), record.subject))
                .or_default()
                .insert(record.event);
        }
        let by_event: BTreeMap<EventId, &ApprovalRecord> =
            records.iter().map(|r| (r.event, r)).collect();
        let mut latest = BTreeMap::new();
        for (key, ids) in groups {
            if let Some(tip) = graph.tips_of(&ids).into_iter().next() {
                latest.insert(key, *by_event[&tip].attestation.target_hash());
            }
        }
        Self {
            records,
            revoked,
            canonical: view.alias_map().clone(),
            latest,
        }
    }

    /// The surviving identifier of an entity (itself unless a duplicate).
    pub fn canonical(&self, id: EntityId) -> EntityId {
        self.canonical.get(&id).copied().unwrap_or(id)
    }

    /// Whether the `Approved` event was withdrawn.
    pub fn is_revoked(&self, event: &EventId) -> bool {
        self.revoked.contains(event)
    }

    /// The approvals not withdrawn.
    pub fn active(&self) -> impl Iterator<Item = &ApprovalRecord> {
        self.records
            .iter()
            .filter(|r| !self.revoked.contains(&r.event))
    }

    /// Every approval event that exists, withdrawn or not, for the audit view.
    pub fn all(&self) -> &[ApprovalRecord] {
        &self.records
    }

    /// The active approvals of `gate` for `subject` at exactly `hash`.
    pub fn matching<'a>(
        &'a self,
        gate: Gate,
        subject: EntityId,
        hash: &'a ContentHash,
    ) -> impl Iterator<Item = &'a ApprovalRecord> + 'a {
        let subject = self.canonical(subject);
        self.active().filter(move |r| {
            r.attestation.gate() == gate
                && r.subject == subject
                && r.attestation.target_hash() == hash
        })
    }

    /// Whether `(gate, subject, hash)` is approved: an approval exists and none
    /// of those matching it is withdrawn.
    pub fn is_approved(&self, gate: Gate, subject: EntityId, hash: &ContentHash) -> bool {
        self.matching(gate, subject, hash).next().is_some()
    }

    /// The hash of the latest active approval of `gate` for `subject`, if any.
    /// For display, and for the hash a Modified entity was approved at.
    pub fn latest_hash(&self, gate: Gate, subject: EntityId) -> Option<&ContentHash> {
        self.latest.get(&(gate, self.canonical(subject)))
    }

    /// The approval record of an event, withdrawn or not.
    pub fn record(&self, event: &EventId) -> Option<&ApprovalRecord> {
        self.records.iter().find(|r| &r.event == event)
    }

    /// The CS approval a CM is set against: one that fixed `hyp`, the hypothesis
    /// hash of the Spec today, if there is one, else any (so the check can say the
    /// hypotheses changed). The smallest event identifier decides among equals.
    ///
    /// Asked of the whole set, with the duplicate registrations folded: a CS
    /// recorded against an identifier that a merge made equal to this Spec's counts.
    pub fn cs_for_cm(&self, spec: EntityId, hyp: &ContentHash) -> Option<CsFixing> {
        let subject = self.canonical(spec);
        let cs: Vec<CsFixing> = self
            .active()
            .filter(|r| r.subject() == subject)
            .filter_map(|r| match r.attestation() {
                Attestation::Cs { hyp: fixed, .. } => Some(CsFixing {
                    event: r.event(),
                    hyp: *fixed,
                }),
                _ => None,
            })
            .collect();
        cs.iter()
            .filter(|fixing| &fixing.hyp == hyp)
            .map(|fixing| fixing.event)
            .min()
            .or_else(|| cs.iter().map(|fixing| fixing.event).min())
            .and_then(|event| cs.iter().find(|fixing| fixing.event == event).copied())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::event::test_support::ts;
    use crate::domain::id::test_support::{did, fid, iid, sid};
    use crate::domain::revision::RevisionId;

    pub(crate) fn h(n: u8) -> ContentHash {
        ContentHash::raw(&[n])
    }

    pub(crate) fn alice() -> Actor {
        Actor::new("alice").unwrap()
    }

    pub(crate) fn rev(s: &str) -> Revision {
        Revision::Commit(RevisionId::from_validated(s.to_owned()).unwrap())
    }

    pub(crate) fn cr(f: u128, hash: &ContentHash) -> Attestation {
        Attestation::Cr {
            finding: Pinned::new(fid(f), *hash),
        }
    }

    pub(crate) fn cs(
        s: u128,
        hash: &ContentHash,
        hyp: &ContentHash,
        grounds: &[(u128, ContentHash)],
    ) -> Attestation {
        let mut it = grounds.iter().map(|(f, g)| Pinned::new(fid(*f), *g));
        let head = it.next().unwrap();
        Attestation::Cs {
            spec: Pinned::new(sid(s), *hash),
            hyp: *hyp,
            grounds: NonEmptyVec::new(head, it.collect()),
        }
    }

    pub(crate) fn ci(d: u128, hash: &ContentHash, s: u128, spec_hash: &ContentHash) -> Attestation {
        Attestation::Ci {
            design: Pinned::new(did(d), *hash),
            spec: Pinned::new(sid(s), *spec_hash),
        }
    }

    pub(crate) fn ca(
        i: u128,
        hash: &ContentHash,
        d: u128,
        design_hash: &ContentHash,
        revision: &str,
    ) -> Attestation {
        Attestation::Ca {
            item: Pinned::new(iid(i), *hash),
            design: Pinned::new(did(d), *design_hash),
            revision: rev(revision),
            tests: TestOutcome::Passed,
        }
    }

    /// A CM that recorded a pass for the hypotheses `hyp` of Spec `s`, against the CS
    /// approval `cs`.
    pub(crate) fn cm_pass(s: u128, hyp: &ContentHash, cs: EventId) -> Attestation {
        Attestation::Cm {
            spec: Pinned::new(sid(s), *hyp),
            cs,
            result: CmResult::Pass,
        }
    }

    pub(crate) fn approve(attestation: Attestation, heads: &[EventId]) -> Event {
        Event::new(
            attestation.subject(),
            heads.iter().copied().collect(),
            ts(),
            EventBody::Approved {
                by: alice(),
                attestation,
            },
        )
        .unwrap()
    }

    pub(crate) fn revoke(target: EntityId, approved: EventId, heads: &[EventId]) -> Event {
        Event::new(
            target,
            heads.iter().copied().collect(),
            ts(),
            EventBody::Revoked {
                revokes: approved,
                reason: NonBlank::new("found a flaw").unwrap(),
                by: alice(),
            },
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::event::DocKind;
    use crate::domain::event::test_support::{add_tag, register_doc};
    use crate::domain::id::test_support::{eid, fid, sid};

    fn sample(gate: Gate) -> Attestation {
        match gate {
            Gate::Cr => cr(1, &h(1)),
            Gate::Cs => cs(2, &h(2), &h(9), &[(1, h(1)), (3, h(3))]),
            Gate::Ci => ci(4, &h(4), 2, &h(2)),
            Gate::Ca => ca(5, &h(5), 4, &h(4), "r1"),
            Gate::Cm => Attestation::Cm {
                spec: Pinned::new(sid(2), h(9)),
                cs: approve(cs(2, &h(2), &h(9), &[(1, h(1))]), &[]).id(),
                result: CmResult::Pass,
            },
        }
    }

    /// What the table of the module says an approval records of the upstream.
    enum Upstreams {
        None,
        Grounds,
        OneEntity,
        OneEvent,
    }

    /// The table of the module documentation, row by row: the gate, the hash it pins
    /// of its target, and what it records of the upstream.
    fn documented() -> [(Gate, ContentHash, Upstreams); 5] {
        [
            (Gate::Cr, h(1), Upstreams::None),
            (Gate::Cs, h(2), Upstreams::Grounds), // the contract hash, not the hyp hash
            (Gate::Ci, h(4), Upstreams::OneEntity), // the Spec at its contract hash
            (Gate::Ca, h(5), Upstreams::OneEntity), // the Design at its design hash
            (Gate::Cm, h(9), Upstreams::OneEvent), // the hyp hash; the CS event
        ]
    }

    #[test]
    fn an_attestation_records_what_the_documented_table_says() {
        assert_eq!(documented().len(), Gate::ALL.len());
        for (gate, target_hash, upstreams) in documented() {
            let a = sample(gate);
            assert_eq!(a.gate(), gate);
            assert_eq!(a.target_hash(), &target_hash, "{gate:?} target");
            let up = a.upstream();
            match upstreams {
                Upstreams::None => assert!(up.is_empty(), "{gate:?}"),
                Upstreams::Grounds => {
                    let ids: Vec<_> = up
                        .iter()
                        .map(|u| match u {
                            UpstreamRecord::Entity { id, .. } => *id,
                            UpstreamRecord::Event(_) => panic!("grounds are entities"),
                        })
                        .collect();
                    assert_eq!(ids, vec![fid(1).entity(), fid(3).entity()]);
                }
                Upstreams::OneEntity => {
                    assert!(matches!(up.as_slice(), [UpstreamRecord::Entity { .. }]));
                }
                Upstreams::OneEvent => {
                    assert!(matches!(up.as_slice(), [UpstreamRecord::Event(_)]));
                }
            }
        }
    }

    #[test]
    fn only_a_ca_has_a_revision() {
        for gate in Gate::ALL {
            assert_eq!(
                sample(gate).revision().is_some(),
                gate == Gate::Ca,
                "{gate:?}"
            );
        }
    }

    #[test]
    fn the_subject_of_an_attestation_is_its_target() {
        assert_eq!(
            sample(Gate::Ca).subject(),
            crate::domain::id::test_support::iid(5).entity()
        );
    }

    #[test]
    fn gate_targets_are_the_expected_kinds() {
        assert_eq!(Gate::Cm.target_kind(), EntityKind::Spec);
        assert_eq!(Gate::Cs.target_kind(), EntityKind::Spec);
        assert_eq!(Gate::Ca.target_kind(), EntityKind::Item);
    }

    #[test]
    fn a_document_kind_has_the_gate_that_approves_it_and_the_kind_comes_back() {
        for kind in [DocKind::Finding, DocKind::Spec, DocKind::Design] {
            assert_eq!(Gate::of_doc(kind).target_kind(), kind.entity_kind());
        }
        assert_eq!(Gate::of_doc(DocKind::Spec), Gate::Cs, "not CM");
    }

    #[test]
    fn the_measurement_words_read_back_and_cover_every_result() {
        for kind in CmResultKind::ALL {
            assert_eq!(kind.name().parse::<CmResultKind>(), Ok(kind));
        }
        let results = [
            CmResult::Pass,
            CmResult::FailWiring {
                item_hint: NonBlank::new("x").unwrap(),
            },
            CmResult::FailHyp { finding: fid(1) },
        ];
        let kinds: Vec<_> = results.iter().map(CmResult::kind).collect();
        assert_eq!(kinds, CmResultKind::ALL.to_vec());
        // The hash keeps the same words as literals of its own.
        for result in &results {
            let canon = result.to_canon().to_bytes();
            let text = String::from_utf8(canon).unwrap();
            assert!(text.contains(result.kind().name()), "{text}");
        }
    }

    #[test]
    fn approval_events_must_target_their_attestation_subject() {
        let err = Event::new(
            eid(99),
            BTreeSet::new(),
            crate::domain::event::test_support::ts(),
            EventBody::Approved {
                by: alice(),
                attestation: cr(1, &h(1)),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::domain::error::EventError::TargetMismatch { .. }
        ));
    }

    #[test]
    fn the_person_does_not_enter_the_approval_identifier() {
        let a = approve(cr(1, &h(1)), &[]);
        let b = Event::new(
            fid(1).entity(),
            BTreeSet::new(),
            crate::domain::event::test_support::ts(),
            EventBody::Approved {
                by: Actor::new("bob").unwrap(),
                attestation: cr(1, &h(1)),
            },
        )
        .unwrap();
        assert_eq!(a.id(), b.id());
    }

    fn view_of(events: &[Event]) -> ClassificationView {
        crate::domain::classification::fold(events)
    }

    #[test]
    fn approved_means_an_approval_exists_and_is_not_revoked() {
        let reg = register_doc(1, DocKind::Finding, "docs/finding/a.md", None, 1);
        let a1 = approve(cr(1, &h(1)), &[reg.id()]);
        let events = vec![reg.clone(), a1.clone()];
        let set = ApprovalSet::from_events(&events, &view_of(&events));
        assert!(set.is_approved(Gate::Cr, fid(1).entity(), &h(1)));
        assert!(
            !set.is_approved(Gate::Cr, fid(1).entity(), &h(2)),
            "another hash"
        );
        assert!(
            !set.is_approved(Gate::Cs, fid(1).entity(), &h(1)),
            "another gate"
        );

        let r = revoke(fid(1).entity(), a1.id(), &[a1.id()]);
        let events = vec![reg.clone(), a1.clone(), r.clone()];
        let set = ApprovalSet::from_events(&events, &view_of(&events));
        assert!(!set.is_approved(Gate::Cr, fid(1).entity(), &h(1)));
        assert!(set.is_revoked(&a1.id()));
        assert_eq!(set.active().count(), 0);
        assert_eq!(set.all().len(), 1, "the audit view still sees it");
    }

    #[test]
    fn revoke_then_approve_again_is_a_new_approval_with_a_new_identifier() {
        let reg = register_doc(1, DocKind::Finding, "docs/finding/a.md", None, 1);
        let a1 = approve(cr(1, &h(1)), &[reg.id()]);
        let r = revoke(fid(1).entity(), a1.id(), &[a1.id()]);
        let a2 = approve(cr(1, &h(1)), &[r.id()]);
        assert_ne!(a1.id(), a2.id());
        let events = vec![reg, a1, r, a2.clone()];
        let set = ApprovalSet::from_events(&events, &view_of(&events));
        assert!(set.is_approved(Gate::Cr, fid(1).entity(), &h(1)));
        assert_eq!(
            set.active().map(ApprovalRecord::event).collect::<Vec<_>>(),
            vec![a2.id()]
        );
    }

    #[test]
    fn another_branch_approving_another_version_does_not_break_this_one() {
        // Two branches approve different hashes of the same Finding.
        let reg = register_doc(1, DocKind::Finding, "docs/finding/a.md", None, 1);
        let on_main = approve(cr(1, &h(1)), &[reg.id()]);
        let on_branch = approve(cr(1, &h(2)), &[reg.id()]);
        let events = vec![reg, on_main, on_branch];
        let set = ApprovalSet::from_events(&events, &view_of(&events));
        assert!(set.is_approved(Gate::Cr, fid(1).entity(), &h(1)));
        assert!(set.is_approved(Gate::Cr, fid(1).entity(), &h(2)));
    }

    #[test]
    fn latest_is_by_ancestry_and_only_for_display() {
        let reg = register_doc(1, DocKind::Finding, "docs/finding/a.md", None, 1);
        let a1 = approve(cr(1, &h(1)), &[reg.id()]);
        let a2 = approve(cr(1, &h(2)), &[a1.id()]);
        let events = vec![reg, a1, a2];
        let set = ApprovalSet::from_events(&events, &view_of(&events));
        assert_eq!(set.latest_hash(Gate::Cr, fid(1).entity()), Some(&h(2)));
        assert_eq!(set.latest_hash(Gate::Cr, fid(7).entity()), None);
    }

    #[test]
    fn approvals_on_a_duplicate_registration_count_for_the_survivor() {
        // Entities 1 and 2 registered the same file independently; 1 survives.
        let r1 = register_doc(1, DocKind::Finding, "docs/finding/a.md", None, 1);
        let r2 = register_doc(2, DocKind::Finding, "docs/finding/a.md", None, 1);
        let a = approve(cr(2, &h(1)), &[r2.id()]);
        let events = vec![r1, r2, a, add_tag(1, &[], "x")];
        let set = ApprovalSet::from_events(&events, &view_of(&events));
        assert!(set.is_approved(Gate::Cr, fid(1).entity(), &h(1)));
        assert!(
            set.is_approved(Gate::Cr, fid(2).entity(), &h(1)),
            "asking by the duplicate id also works"
        );
        assert_eq!(set.canonical(eid(2)), eid(1));
    }
}
