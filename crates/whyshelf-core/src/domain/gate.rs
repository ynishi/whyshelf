//! The gates: layer 1 of each CHECK, and the proof that it passed.
//!
//! A gate has two layers (see [`approval`](super::approval)). Layer 1 is here: one
//! function per gate, [`cr::check`], [`cs::check`], [`ci::check`], [`ca::check`] and
//! [`cm::check`], each a pure function of an input struct that returns either every
//! violation found (never just the first) or a [`Passed`]. Layer 2, the person's
//! judgement, is recorded as an `Approved` event, and the only thing an event of that
//! kind can be built from is a `Passed`: its constructor is private to this module,
//! so a gate's check is the one way to get one.
//!
//! ```compile_fail
//! use whyshelf_core::domain::approval::{Attestation, Gate};
//! use whyshelf_core::domain::gate::Passed;
//!
//! // `Passed` has private fields: only a gate's check can make one.
//! fn forged(attestation: Attestation) -> Passed {
//!     Passed { gate: Gate::Cr, attestation }
//! }
//! ```
//!
//! # What each gate reads
//!
//! The application collects the values; the gate decides. Each input is the target
//! as it was evaluated, the things it stands on as they were evaluated, and, for the
//! gates that look at an upstream, [`States`]: the state of every entity whatever
//! its kind, which is where the state of an upstream is read (a document whose kind
//! is in conflict has a state there and no typed evaluation).
//!
//! | gate | input | looks at | pins, if it passes |
//! |---|---|---|---|
//! | CR | [`CrInput`] | the Finding's body and its label | the Finding at its hash |
//! | CS | [`CsInput`] | each ground's state; the Spec's pins and hypotheses, as far as the [`GatePolicy`] demands them | the Spec at its contract hash, the hypothesis hash, every ground at its hash, the policy it passed under |
//! | CI | [`CiInput`] | the Spec's state; the pin coverage | the Design at its hash, the Spec at its contract hash |
//! | CA | [`CaInput`] | the Design's state; the pins in the tests, in both directions; the tests | the item at its hash, the Design at its hash, the revision, the outcome |
//! | CM | [`CmInput`] | the CS approval that fixed the hypotheses; the Finding of a `fail-hyp` | the Spec at its hypothesis hash, that CS approval, the result |
//!
//! # An Invalid target is a violation
//!
//! In every gate (the measurement included) an entity that is Invalid yields one
//! violation, [`CheckViolation::TargetInvalid`], with its reasons, and nothing else
//! is checked: the content is not one that can be judged. It is a violation like the
//! others, so it is shown as one and the command's exit status says so; it is not
//! an error of the tool.
//!
//! # What stops on an upstream
//!
//! An upstream is judged by [`upstream_problems`], which holds the one order in
//! which its problems are shown; the gate turns each into a
//! [`CheckViolation::Upstream`]. An upstream that is a document in conflict over its
//! kind is Invalid like any other, so a Spec that takes it as a ground is stopped at
//! CS.

use std::collections::BTreeMap;

use super::approval::{
    ApprovalSet, Attestation, CmResult, CsFixing, Gate, GateBasis, Pinned, TestOutcome, TestsRun,
};
use super::design::{Design, WorkItem};
use super::evaluate::{Evaluated, ImplView, ItemEvaluation, ItemKey, States, Subject};
use super::finding::{EvidenceLabel, Finding, FindingSource};
use super::id::{DesignId, EntityId, FindingId, SpecId};
use super::lint::{CheckViolation, UpstreamProblem, upstream_problems};
use super::nonempty::NonEmptyVec;
use super::policy::GatePolicy;
use super::revision::{Revision, RevisionId};
use super::spec::{DeclaredPins, Spec};
use super::state::{Approval, Completeness, EntityState, InvalidReason};

/// Proof that a gate's machine check found nothing: the only way to record an
/// approval.
///
/// It holds the attestation the approval will carry, built by the check from the
/// values it looked at. See the module documentation for why it cannot be made
/// anywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed {
    gate: Gate,
    attestation: Attestation,
}

impl Passed {
    /// The gate that let it through.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// What the approval will say.
    pub fn attestation(&self) -> &Attestation {
        &self.attestation
    }

    /// The entity the approval is about.
    pub fn subject(&self) -> EntityId {
        self.attestation.subject()
    }

    /// The policy the check passed under: only CS reads one.
    pub fn gate_basis(&self) -> GateBasis {
        self.attestation.gate_basis()
    }
}

#[cfg(test)]
impl Passed {
    /// A proof made without a check, for the tests of the modules that consume one.
    /// It exists only in a test build, so the rule above holds for the library.
    pub(crate) fn for_test(attestation: Attestation) -> Self {
        Self {
            gate: attestation.gate(),
            attestation,
        }
    }
}

/// Every violation a check found: at least one, or it would have passed.
pub type Violations = NonEmptyVec<CheckViolation>;

/// A check's answer.
pub type Checked = Result<Passed, Violations>;

/// The violations of a list, or the value the caller builds if there are none.
fn settle(
    violations: Vec<CheckViolation>,
    pass: impl FnOnce() -> Option<Attestation>,
    gate: Gate,
    unpinnable: impl FnOnce() -> Violations,
) -> Checked {
    match NonEmptyVec::from_vec(violations) {
        Ok(violations) => Err(violations),
        Err(_) => match pass() {
            Some(attestation) => Ok(Passed { gate, attestation }),
            None => Err(unpinnable()),
        },
    }
}

fn one(violation: CheckViolation) -> Violations {
    NonEmptyVec::new(violation, Vec::new())
}

/// The reason an upstream could not be pinned although it raised no problem: the
/// application did not hand over what the check needs. It is shown as an upstream
/// that cannot be read, which stops the approval.
fn unpinnable(upstream: EntityId) -> Violations {
    one(CheckViolation::Upstream {
        upstream,
        problem: UpstreamProblem::Unresolved,
    })
}

/// The violation of an Invalid state, if the state is Invalid: the one rule that
/// an Invalid target is a violation of every gate.
///
/// Public for the application, which holds states that have no entity to run a
/// check on (a document whose kind is in conflict) and must say the same thing.
pub fn invalid_target(state: &EntityState) -> Option<Violations> {
    match state.approval() {
        Approval::Invalid(reasons) => Some(one(CheckViolation::TargetInvalid {
            reasons: reasons.clone(),
        })),
        Approval::Draft | Approval::Approved | Approval::Modified { .. } => None,
    }
}

/// The entity a gate is run on, or the violation that it is Invalid.
fn target<T>(evaluated: &Evaluated<T>) -> Result<&T, Violations> {
    if let Some(violations) = invalid_target(evaluated.state()) {
        return Err(violations);
    }
    match evaluated.subject() {
        Subject::Parsed(entity) => Ok(entity),
        // A file that did not parse is evaluated as Invalid, so this is not reached
        // through an `Evaluated`; the reasons are the file's own all the same.
        Subject::Unparsed(unparsed) => {
            let mut reasons = unparsed
                .reasons()
                .iter()
                .cloned()
                .map(InvalidReason::Unparsed);
            let first = reasons
                .next()
                .unwrap_or_else(|| InvalidReason::Unparsed(unparsed.reasons().first().clone()));
            Err(one(CheckViolation::TargetInvalid {
                reasons: NonEmptyVec::new(first, reasons.collect()),
            }))
        }
    }
}

/// One violation for each upstream that is not ready, in the order of
/// [`upstream_problems`].
fn upstream_violations(
    target: &Completeness,
    upstreams: &[(EntityId, Option<&EntityState>)],
) -> Vec<CheckViolation> {
    upstream_problems(target, upstreams)
        .into_iter()
        .map(|(upstream, problem)| CheckViolation::Upstream { upstream, problem })
        .collect()
}

fn state_of<'a>(states: &States<'a>, id: EntityId) -> Option<&'a EntityState> {
    states.get(&id).copied()
}

/// What CR reads: the Finding as it was evaluated.
#[derive(Debug, Clone, Copy)]
pub struct CrInput<'a> {
    /// The Finding.
    pub finding: &'a Evaluated<Finding>,
}

/// CR, layer 1: the Finding is complete as a record.
pub mod cr {
    use super::*;

    /// The metadata each type requires is already held by the type. What remains is
    /// the body, and the label a measurement must carry.
    pub fn check(input: CrInput<'_>) -> Checked {
        let finding = target(input.finding)?;
        let mut violations = Vec::new();
        if finding.body().trim().is_empty() {
            violations.push(CheckViolation::EmptyBody);
        }
        if matches!(finding.source(), FindingSource::MeasuredData { .. })
            && finding.evidence() != EvidenceLabel::Measured
        {
            violations.push(CheckViolation::MeasuredDataNotLabelledMeasured);
        }
        settle(
            violations,
            || {
                Some(Attestation::Cr {
                    finding: Pinned::new(finding.id(), *finding.hash()),
                })
            },
            Gate::Cr,
            || unpinnable(finding.id().entity()),
        )
    }
}

/// What CS reads.
#[derive(Debug, Clone)]
pub struct CsInput<'a> {
    /// The Spec.
    pub spec: &'a Evaluated<Spec>,
    /// The Findings, as evaluated, for their hashes.
    pub grounds: BTreeMap<FindingId, &'a Evaluated<Finding>>,
    /// The state of every entity, the grounds' among them.
    pub states: &'a States<'a>,
    /// What the project demands of a Spec; a passed CS records it.
    pub policy: GatePolicy,
}

/// CS, layer 1: every ground exists and can be read here, passed CR and is
/// current; pins and hypotheses are declared, as far as the [`GatePolicy`] demands.
pub mod cs {
    use super::*;

    /// The Spec's own state says, in its completeness, which grounds were written
    /// with a mark into a store this environment cannot read. Such a ground is a
    /// violation: it cannot be checked, and the hash of it cannot be recorded.
    ///
    /// Pins and hypotheses are demanded unless the policy says otherwise. A Spec
    /// that declares none of either and passes under a relaxed policy is recorded
    /// with that policy.
    pub fn check(input: CsInput<'_>) -> Checked {
        let spec = target(input.spec)?;
        let upstreams: Vec<_> = spec
            .grounds()
            .iter()
            .map(|ground| (ground.entity(), state_of(input.states, ground.entity())))
            .collect();
        let mut violations = upstream_violations(input.spec.state().completeness(), &upstreams);
        if input.policy.cs_requires_pins() && spec.pins().is_empty() {
            violations.push(CheckViolation::NoPinsDeclared);
        }
        if input.policy.cs_requires_hypotheses() && spec.hyps().is_empty() {
            violations.push(CheckViolation::NoHypsDeclared);
        }
        // An attestation names every ground at its hash. A ground that cannot be
        // pinned is a violation, never a ground left out.
        let pin = |ground: &FindingId| {
            input
                .grounds
                .get(ground)
                .and_then(|evaluated| evaluated.entity())
                .map(|finding| Pinned::new(*ground, *finding.hash()))
        };
        let head = pin(spec.grounds().first());
        let tail: Option<Vec<_>> = spec.grounds().iter().skip(1).map(pin).collect();
        for ground in spec.grounds().iter() {
            let known = violations.iter().any(
                |v| matches!(v, CheckViolation::Upstream { upstream, .. } if *upstream == ground.entity()),
            );
            if !known && pin(ground).is_none() {
                violations.push(CheckViolation::Upstream {
                    upstream: ground.entity(),
                    problem: UpstreamProblem::Unresolved,
                });
            }
        }
        settle(
            violations,
            || match (head, tail) {
                (Some(head), Some(tail)) => Some(Attestation::Cs {
                    spec: Pinned::new(spec.id(), *spec.contract_hash()),
                    hyp: *spec.hyp_hash(),
                    grounds: NonEmptyVec::new(head, tail),
                    policy: input.policy,
                }),
                _ => None,
            },
            Gate::Cs,
            || unpinnable(spec.id().entity()),
        )
    }
}

/// What CI reads.
#[derive(Debug, Clone)]
pub struct CiInput<'a> {
    /// The Design.
    pub design: &'a Evaluated<Design>,
    /// The Specs, as evaluated.
    pub specs: BTreeMap<SpecId, &'a Evaluated<Spec>>,
    /// The state of every entity, the Spec's among them.
    pub states: &'a States<'a>,
}

/// CI, layer 1: the Spec passed CS, is current and can be read in full here; pins
/// are covered in both directions.
pub mod ci {
    use super::*;

    /// An approved Spec that is Partial (something it stands on cannot be read here)
    /// is a violation: whether it is still current cannot be told. The Design's own
    /// state names, in its completeness, a Spec it wrote with a mark into a store
    /// this environment cannot read.
    ///
    /// That the Crux gives a reason for each rejected option is held by its type, so
    /// there is nothing to check for it here.
    pub fn check(input: CiInput<'_>) -> Checked {
        let design = target(input.design)?;
        let spec_id = design.spec().entity();
        let mut violations = upstream_violations(
            input.design.state().completeness(),
            &[(spec_id, state_of(input.states, spec_id))],
        );
        let spec = input
            .specs
            .get(&design.spec())
            .and_then(|evaluated| evaluated.entity());
        if let Some(spec) = spec {
            violations.extend(
                design
                    .check_pin_coverage(spec)
                    .into_iter()
                    .map(CheckViolation::PinCoverage),
            );
        }
        settle(
            violations,
            || {
                spec.map(|spec| Attestation::Ci {
                    design: Pinned::new(design.id(), *design.design_hash()),
                    spec: Pinned::new(spec.id(), *spec.contract_hash()),
                })
            },
            Gate::Ci,
            || unpinnable(spec_id),
        )
    }
}

/// What CA reads.
#[derive(Debug, Clone)]
pub struct CaInput<'a> {
    /// The item's identifier, Design and number.
    pub key: &'a ItemKey,
    /// The item as the Design said when it was handed over.
    pub item: &'a WorkItem,
    /// The item's evaluation: its state decides whether it can be approved at all.
    pub evaluation: &'a ItemEvaluation,
    /// The Designs, as evaluated.
    pub designs: BTreeMap<DesignId, &'a Evaluated<Design>>,
    /// The pins that any Spec of the project declares.
    pub declared: &'a DeclaredPins,
    /// The state of every entity, the Design's among them.
    pub states: &'a States<'a>,
    /// The pins found in the tests at the revision, and the tags that are none.
    pub impl_view: &'a ImplView,
    /// The revision the tests and the pin scan ran at, and that the approval records.
    pub revision: RevisionId,
    /// What running the tests at that revision came to.
    pub tests: TestsRun,
}

/// CA, layer 1: the Design passed CI, is current and can be read in full here; the
/// item's pins and the tests' tags match in both directions at the revision; the
/// tests passed.
pub mod ca {
    use super::*;

    /// An approved Design that is Partial, through its Spec or further up, is a
    /// violation, for the same reason as at CI. An item is named by its number inside
    /// its Design, never by a mark, so it has nothing of its own that cannot be read.
    pub fn check(input: CaInput<'_>) -> Checked {
        if let Some(violations) = invalid_target(input.evaluation.state()) {
            return Err(violations);
        }
        let CaInput {
            key,
            item,
            designs,
            declared,
            states,
            impl_view,
            revision,
            tests,
            ..
        } = input;
        let design_id = key.design.entity();
        let mut violations = upstream_violations(
            &Completeness::Complete,
            &[(design_id, state_of(states, design_id))],
        );
        let design = designs
            .get(&key.design)
            .and_then(|evaluated| evaluated.entity());
        if let Some(design) = design
            && design.items().get(&key.number) != Some(item)
        {
            violations.push(CheckViolation::ItemNotInDesign);
        }
        for pin in item.assigned_pins() {
            if !impl_view.has_pin(pin) {
                violations.push(CheckViolation::PinMissingInTests { pin: pin.clone() });
            }
        }
        for pin in impl_view.pins_at_revision().keys() {
            if !declared.contains(pin) {
                violations.push(CheckViolation::PinUndeclaredInTests { pin: pin.clone() });
            }
        }
        for malformed in impl_view.malformed_pins() {
            violations.push(CheckViolation::MalformedPin {
                location: malformed.location().clone(),
                text: malformed.text().clone(),
            });
        }
        let outcome = match tests {
            TestsRun::Finished(outcome) => Some(outcome),
            TestsRun::TimedOut { after } => {
                violations.push(CheckViolation::TestsTimedOut { after });
                None
            }
        };
        if outcome == Some(TestOutcome::Failed) {
            violations.push(CheckViolation::TestsFailed);
        }
        settle(
            violations,
            || {
                let tests = outcome?;
                design.map(|design| Attestation::Ca {
                    item: Pinned::new(key.id, item.hash(key.design)),
                    design: Pinned::new(design.id(), *design.design_hash()),
                    revision: Revision::Commit(revision),
                    tests,
                })
            },
            Gate::Ca,
            || unpinnable(design_id),
        )
    }
}

/// What CM reads.
#[derive(Debug, Clone)]
pub struct CmInput<'a> {
    /// The Spec that was measured.
    pub spec: &'a Evaluated<Spec>,
    /// The approvals, for the CS approval that fixed the hypotheses.
    pub ledger: &'a ApprovalSet,
    /// What the measurement showed.
    pub result: CmResult,
    /// For a `fail-hyp`, the Finding it names, as evaluated; none if it is not
    /// there.
    pub hyp_finding: Option<&'a Evaluated<Finding>>,
}

/// CM, layer 1: the CS approval that fixed the hypotheses exists, and fixed them as
/// they are.
pub mod cm {
    use super::*;

    /// A `fail-hyp` also needs the Finding it names to be a measurement. The CS
    /// approval is looked for in the whole set of approvals, with the duplicate
    /// registrations folded, so one recorded before two branches merged still counts.
    pub fn check(input: CmInput<'_>) -> Checked {
        let spec = target(input.spec)?;
        let mut violations = Vec::new();
        if let CmResult::FailHyp { finding } = &input.result {
            let measured = input
                .hyp_finding
                .and_then(|evaluated| evaluated.entity())
                .is_some_and(|f| matches!(f.source(), FindingSource::MeasuredData { .. }));
            if !measured {
                violations.push(CheckViolation::MeasurementNotMeasuredData { finding: *finding });
            }
        }
        let fixing: Option<CsFixing> = input.ledger.cs_for_cm(spec.id().entity(), spec.hyp_hash());
        match &fixing {
            None => violations.push(CheckViolation::CsApprovalNotFound),
            Some(fixing) if fixing.hyp() != spec.hyp_hash() => {
                violations.push(CheckViolation::HypothesisChanged {
                    fixed: *fixing.hyp(),
                    current: *spec.hyp_hash(),
                });
            }
            Some(_) => {}
        }
        settle(
            violations,
            || {
                fixing.map(|fixing| Attestation::Cm {
                    spec: Pinned::new(spec.id(), *spec.hyp_hash()),
                    cs: fixing.event(),
                    result: input.result,
                })
            },
            Gate::Cm,
            || unpinnable(spec.id().entity()),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::approval::test_support::{
        approve, ci as ci_att, cr as cr_att, cs as cs_att, revoke,
    };
    use crate::domain::canon::ToCanon;
    use crate::domain::classification::{ConflictedDoc, fold};
    use crate::domain::design::CoverageIssue;
    use crate::domain::design::test_support::{design, item, num};
    use crate::domain::evaluate::test_support::{NO_DESIGNS, NO_FINDINGS, NO_SPECS};
    use crate::domain::evaluate::{Evaluator, Written, states_of};
    use crate::domain::event::test_support::{reclassify, register_doc};
    use crate::domain::event::{ClassChange, DocKind, Event};
    use crate::domain::finding::test_support::{finding, usage_source};
    use crate::domain::id::test_support::{did, eid, fid, iid, sid};
    use crate::domain::location::CodeLocation;
    use crate::domain::nonempty::NonBlank;
    use crate::domain::pin::MalformedPin;
    use crate::domain::spec::test_support::{hyps, pins, spec};
    use crate::domain::state::UnparsedReason;

    fn ledger(events: &[Event]) -> ApprovalSet {
        ApprovalSet::from_events(events, &fold(events))
    }

    /// Findings, Specs and Designs evaluated against a ledger, as the application
    /// holds them.
    struct World {
        findings: BTreeMap<FindingId, Evaluated<Finding>>,
        specs: BTreeMap<SpecId, Evaluated<Spec>>,
        designs: BTreeMap<DesignId, Evaluated<Design>>,
        conflicted: Vec<ConflictedDoc>,
    }

    impl World {
        fn new(
            findings: &[&Finding],
            specs: &[&Spec],
            designs: &[&Design],
            events: &[Event],
        ) -> Self {
            World::with_conflicts(findings, specs, designs, events, Vec::new())
        }

        fn with_conflicts(
            findings: &[&Finding],
            specs: &[&Spec],
            designs: &[&Design],
            events: &[Event],
            conflicted: Vec<ConflictedDoc>,
        ) -> Self {
            let l = ledger(events);
            let findings: BTreeMap<_, _> = findings
                .iter()
                .map(|f| (f.id(), Evaluator::finding(f, &l, &[])))
                .collect();
            let specs: BTreeMap<_, _> = {
                let states = states_of(&findings, &NO_SPECS, &NO_DESIGNS, &conflicted);
                specs
                    .iter()
                    .map(|s| {
                        (
                            s.id(),
                            Evaluator::spec(s, &findings, &states, &l, &Written::default(), &[]),
                        )
                    })
                    .collect()
            };
            let designs: BTreeMap<_, _> = {
                let states = states_of(&findings, &specs, &NO_DESIGNS, &conflicted);
                designs
                    .iter()
                    .map(|d| {
                        (
                            d.id(),
                            Evaluator::design(d, &specs, &states, &l, &Written::default(), &[]),
                        )
                    })
                    .collect()
            };
            World {
                findings,
                specs,
                designs,
                conflicted,
            }
        }

        fn states(&self) -> States<'_> {
            states_of(&self.findings, &self.specs, &self.designs, &self.conflicted)
        }

        fn grounds(&self) -> BTreeMap<FindingId, &Evaluated<Finding>> {
            self.findings.iter().map(|(id, ev)| (*id, ev)).collect()
        }

        fn spec_refs(&self) -> BTreeMap<SpecId, &Evaluated<Spec>> {
            self.specs.iter().map(|(id, ev)| (*id, ev)).collect()
        }

        fn design_refs(&self) -> BTreeMap<DesignId, &Evaluated<Design>> {
            self.designs.iter().map(|(id, ev)| (*id, ev)).collect()
        }

        fn cs(&self, spec: &Spec) -> Checked {
            self.cs_under(spec, GatePolicy::default())
        }

        fn cs_under(&self, spec: &Spec, policy: GatePolicy) -> Checked {
            let states = self.states();
            cs::check(CsInput {
                spec: &self.specs[&spec.id()],
                grounds: self.grounds(),
                states: &states,
                policy,
            })
        }

        fn ci(&self, design: &Design) -> Checked {
            let states = self.states();
            ci::check(CiInput {
                design: &self.designs[&design.id()],
                specs: self.spec_refs(),
                states: &states,
            })
        }
    }

    /// Ground 1 written with a mark into a store that cannot be read here.
    fn unreadable_ground() -> Written {
        Written {
            unresolved: BTreeSet::from([eid(1)]),
            ..Written::default()
        }
    }

    fn violations(checked: Checked) -> Vec<CheckViolation> {
        match checked {
            Ok(passed) => panic!("expected violations, passed {:?}", passed.gate()),
            Err(violations) => violations.into_vec(),
        }
    }

    fn passed(checked: Checked) -> Passed {
        match checked {
            Ok(passed) => passed,
            Err(violations) => panic!("expected a pass, found {:?}", violations.into_vec()),
        }
    }

    fn upstream(id: u128, problem: UpstreamProblem) -> CheckViolation {
        CheckViolation::Upstream {
            upstream: eid(id),
            problem,
        }
    }

    // --- CR ---------------------------------------------------------------

    fn cr_of(f: &Finding) -> Checked {
        let world = World::new(&[f], &[], &[], &[]);
        cr::check(CrInput {
            finding: &world.findings[&f.id()],
        })
    }

    #[test]
    fn cr_accepts_a_complete_finding_and_pins_it_at_its_hash() {
        let f = finding(1, "Login takes 8 s.");
        let passed = passed(cr_of(&f));
        assert_eq!(passed.gate(), Gate::Cr);
        assert_eq!(passed.attestation(), &cr_att(1, f.hash()));
        assert_eq!(passed.subject(), fid(1).entity());
    }

    #[test]
    fn cr_rejects_an_empty_body_and_an_unlabelled_measurement() {
        assert_eq!(
            violations(cr_of(&finding(1, " \n"))),
            vec![CheckViolation::EmptyBody]
        );
        let measured = |label| {
            Finding::new(
                fid(1),
                NonBlank::new("slow login").unwrap(),
                FindingSource::MeasuredData {
                    measured_at: crate::domain::actor::MeasuredAt::from_validated(
                        "2026-11-04T12:40:00Z".into(),
                    )
                    .unwrap(),
                    revision: RevisionId::from_validated("a1".into()).unwrap(),
                    environment: NonBlank::new("4 cores").unwrap(),
                },
                label,
                "8 s".into(),
            )
        };
        assert!(cr_of(&measured(EvidenceLabel::Measured)).is_ok());
        for label in [EvidenceLabel::Theoretical, EvidenceLabel::Estimated] {
            assert_eq!(
                violations(cr_of(&measured(label))),
                vec![CheckViolation::MeasuredDataNotLabelledMeasured]
            );
        }
        // Other types do not need the label.
        let usage = Finding::new(
            fid(1),
            NonBlank::new("t").unwrap(),
            usage_source(),
            EvidenceLabel::Estimated,
            "b".into(),
        );
        assert!(cr_of(&usage).is_ok());
    }

    #[test]
    fn a_finding_that_did_not_parse_is_an_invalid_target_at_cr() {
        let unparsed = crate::domain::evaluate::Unparsed::new(
            "docs/finding/a.md".parse().unwrap(),
            NonEmptyVec::new(UnparsedReason::MergeConflictMarker, vec![]),
            crate::domain::hash::ContentHash::raw(b"x"),
        );
        let ev = Evaluated::<Finding>::from_unparsed(unparsed, &[]);
        let got = violations(cr::check(CrInput { finding: &ev }));
        assert!(
            matches!(&got[..], [CheckViolation::TargetInvalid { reasons }]
            if reasons.first() == &InvalidReason::Unparsed(UnparsedReason::MergeConflictMarker))
        );
    }

    // --- CS ---------------------------------------------------------------

    #[test]
    fn cs_requires_grounds_that_exist_and_passed_cr() {
        let f1 = finding(1, "one");
        let f2 = finding(2, "two");
        let a1 = approve(cr_att(1, f1.hash()), &[]);
        let a2 = approve(cr_att(2, f2.hash()), &[]);
        let s = spec(11, &[1, 2], &["PIN-A"]);

        // Ground 2 does not exist: the Spec is Invalid for it (a dangling reference),
        // and the gate says that and nothing else.
        let world = World::new(&[&f1], &[&s], &[], std::slice::from_ref(&a1));
        let got = violations(world.cs(&s));
        assert!(
            matches!(&got[..], [CheckViolation::TargetInvalid { reasons }]
            if reasons.first() == &InvalidReason::DanglingRef { upstream: eid(2) })
        );

        // Ground 2 present but a draft.
        let world = World::new(&[&f1, &f2], &[&s], &[], std::slice::from_ref(&a1));
        assert_eq!(
            violations(world.cs(&s)),
            vec![upstream(2, UpstreamProblem::NotApproved)]
        );

        // Both approved: clean, and every ground is pinned at its hash.
        let both = [a1, a2];
        let world = World::new(&[&f1, &f2], &[&s], &[], &both);
        let passed = passed(world.cs(&s));
        assert_eq!(passed.gate(), Gate::Cs);
        assert_eq!(
            passed.attestation(),
            &cs_att(
                11,
                s.contract_hash(),
                s.hyp_hash(),
                &[(1, *f1.hash()), (2, *f2.hash())]
            )
        );
    }

    #[test]
    fn cs_requires_declared_pins_and_hypotheses() {
        let f1 = finding(1, "one");
        let approved = [approve(cr_att(1, f1.hash()), &[])];
        let bare = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        let world = World::new(&[&f1], &[&bare], &[], &approved);
        assert_eq!(
            violations(world.cs(&bare)),
            vec![
                CheckViolation::NoPinsDeclared,
                CheckViolation::NoHypsDeclared
            ]
        );
        let no_hyp = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            pins(&["PIN-A"]),
            hyps(&[]),
        )
        .unwrap();
        let world = World::new(&[&f1], &[&no_hyp], &[], &approved);
        assert_eq!(
            violations(world.cs(&no_hyp)),
            vec![CheckViolation::NoHypsDeclared]
        );
    }

    #[test]
    fn cs_stops_on_a_ground_it_cannot_read_and_pins_nothing() {
        let s = spec(11, &[1], &["PIN-A"]);
        let l = ledger(&[]);
        let states = states_of(&NO_FINDINGS, &NO_SPECS, &NO_DESIGNS, []);
        let unreadable = Evaluator::spec(&s, &NO_FINDINGS, &states, &l, &unreadable_ground(), &[]);
        let got = violations(cs::check(CsInput {
            spec: &unreadable,
            grounds: BTreeMap::new(),
            states: &states,
            policy: GatePolicy::default(),
        }));
        assert_eq!(got, vec![upstream(1, UpstreamProblem::Unresolved)]);
    }

    #[test]
    fn a_relaxed_policy_lets_a_spec_without_pins_through_and_is_recorded() {
        let f1 = finding(1, "one");
        let approved = [approve(cr_att(1, f1.hash()), &[])];
        let spec_with = |n: u128, declared_pins: &[&str], declared_hyps: &[&str]| {
            Spec::new(
                sid(n),
                NonEmptyVec::new(fid(1), vec![]),
                NonBlank::new("c").unwrap(),
                pins(declared_pins),
                hyps(declared_hyps),
            )
            .unwrap()
        };
        let no_pins = spec_with(11, &[], &["HYP-A"]);
        let no_hyps = spec_with(12, &["PIN-A"], &[]);
        let bare = spec_with(13, &[], &[]);
        let world = World::new(&[&f1], &[&no_pins, &no_hyps, &bare], &[], &approved);

        // The default policy refuses each, as it always did.
        assert_eq!(
            violations(world.cs(&no_pins)),
            vec![CheckViolation::NoPinsDeclared]
        );
        assert_eq!(
            violations(world.cs(&no_hyps)),
            vec![CheckViolation::NoHypsDeclared]
        );

        // Relaxing one demand lets that one through, and only that one.
        let no_pins_ok = GatePolicy::new(false, true);
        let passed_no_pins = passed(world.cs_under(&no_pins, no_pins_ok));
        assert_eq!(
            passed_no_pins.gate_basis(),
            GateBasis::Cs { policy: no_pins_ok }
        );
        assert_eq!(
            violations(world.cs_under(&no_hyps, no_pins_ok)),
            vec![CheckViolation::NoHypsDeclared]
        );
        let no_hyps_ok = GatePolicy::new(true, false);
        assert_eq!(
            passed(world.cs_under(&no_hyps, no_hyps_ok)).gate_basis(),
            GateBasis::Cs { policy: no_hyps_ok }
        );
        assert_eq!(
            violations(world.cs_under(&no_pins, no_hyps_ok)),
            vec![CheckViolation::NoPinsDeclared]
        );

        // Relaxing both passes a Spec with neither.
        let both = GatePolicy::new(false, false);
        assert_eq!(
            passed(world.cs_under(&bare, both)).gate_basis(),
            GateBasis::Cs { policy: both }
        );
    }

    #[test]
    fn two_approvals_of_one_spec_under_different_policies_are_distinguishable() {
        let f1 = finding(1, "one");
        let approved = [approve(cr_att(1, f1.hash()), &[])];
        let s = spec(11, &[1], &["PIN-A"]);
        let world = World::new(&[&f1], &[&s], &[], &approved);
        let strict = passed(world.cs_under(&s, GatePolicy::default()));
        let relaxed = passed(world.cs_under(&s, GatePolicy::new(false, true)));
        // The same Spec at the same hashes: only the policy differs, and the
        // attestation, and with it the bytes a record is compared by, differ.
        assert_ne!(strict.attestation(), relaxed.attestation());
        assert_ne!(
            strict.attestation().to_canon().to_bytes(),
            relaxed.attestation().to_canon().to_bytes()
        );
        assert_eq!(
            strict.attestation().target_hash(),
            relaxed.attestation().target_hash()
        );
        assert_eq!(
            strict.gate_basis(),
            GateBasis::Cs {
                policy: GatePolicy::default()
            }
        );
        assert_eq!(
            relaxed.gate_basis(),
            GateBasis::Cs {
                policy: GatePolicy::new(false, true)
            }
        );
    }

    #[test]
    fn only_cs_records_a_policy() {
        let f = finding(1, "one");
        assert_eq!(
            passed(cr_of(&f)).gate_basis(),
            GateBasis::Fixed { gate: Gate::Cr }
        );
    }

    #[test]
    fn a_ground_that_is_a_document_in_conflict_stops_cs_with_an_invalid_upstream() {
        // Document 1 was registered as a Finding and then set to be a Design by one
        // branch and a Spec by another: its kind is in conflict.
        let reg = register_doc(1, DocKind::Finding, "docs/finding/a.md", None, 1);
        let k1 = reclassify(1, &[reg.id()], vec![ClassChange::SetKind(DocKind::Spec)]);
        let k2 = reclassify(1, &[reg.id()], vec![ClassChange::SetKind(DocKind::Design)]);
        let events = [reg, k1, k2];
        let view = fold(&events);
        let conflicted = ConflictedDoc::of(view.doc(eid(1)).unwrap()).expect("in conflict");

        let s = spec(11, &[1], &["PIN-A"]);
        let world = World::with_conflicts(&[], &[&s], &[], &[], vec![conflicted]);
        // The Spec is Blocked by it, not Invalid: the identifier is there.
        assert!(matches!(
            world.specs[&s.id()].state().upstream(),
            crate::domain::state::Upstream::Blocked(_)
        ));
        assert_eq!(
            violations(world.cs(&s)),
            vec![upstream(1, UpstreamProblem::Invalid)]
        );
    }

    // --- CI ---------------------------------------------------------------

    #[test]
    fn ci_checks_the_spec_state_and_pin_coverage() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A", "UT-B"]);
        let d = design(21, 11, vec![(1, item("a", &["PIN-A"]))]);

        // Spec is a draft, and UT-B is unassigned: the Design is Invalid for it, so
        // that is all the gate says.
        let world = World::new(&[&f1], &[&s], &[&d], &[]);
        let got = violations(world.ci(&d));
        assert!(
            matches!(&got[..], [CheckViolation::TargetInvalid { reasons }]
            if reasons.first() == &InvalidReason::PinCoverage(CoverageIssue::Unassigned("UT-B".parse().unwrap())))
        );

        // Spec approved; design assigns everything.
        let events = [
            approve(cr_att(1, f1.hash()), &[]),
            approve(
                cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        let ok_design = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        let world = World::new(&[&f1], &[&s], &[&ok_design], &events);
        let passed = passed(world.ci(&ok_design));
        assert_eq!(
            passed.attestation(),
            &ci_att(21, ok_design.design_hash(), 11, s.contract_hash())
        );
    }

    #[test]
    fn ci_stops_on_a_spec_that_is_a_draft() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let d = design(21, 11, vec![(1, item("a", &["PIN-A"]))]);
        let world = World::new(&[&f1], &[&s], &[&d], &[]);
        assert_eq!(
            violations(world.ci(&d)),
            vec![upstream(11, UpstreamProblem::NotApproved)]
        );
    }

    // --- CA ---------------------------------------------------------------

    struct CaWorld {
        world: World,
        design: Design,
    }

    fn ca_world(approved: bool) -> CaWorld {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A", "UT-B"]);
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        let mut events = vec![
            approve(cr_att(1, f1.hash()), &[]),
            approve(
                cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        if approved {
            events.push(approve(
                ci_att(21, d.design_hash(), 11, s.contract_hash()),
                &[],
            ));
        }
        CaWorld {
            world: World::new(&[&f1], &[&s], &[&d], &events),
            design: d,
        }
    }

    fn view(pins: &[&str]) -> ImplView {
        ImplView::new(
            RevisionId::from_validated("head".into()).unwrap(),
            None,
            pins.iter()
                .map(|p| (p.parse().unwrap(), vec!["tests/a.rs:3".parse().unwrap()]))
                .collect(),
            Default::default(),
        )
    }

    fn key(number: u32) -> ItemKey {
        ItemKey {
            id: iid(31),
            design: did(21),
            number: num(number),
        }
    }

    fn evaluation_of(w: &CaWorld, number: u32, view: &ImplView) -> ItemEvaluation {
        let states = w.world.states();
        Evaluator::item(
            &key(number),
            &w.design.items()[&num(number)],
            &w.world.designs,
            &states,
            &ledger(&[]),
            view,
            &[],
        )
    }

    fn declared_by(w: &CaWorld) -> DeclaredPins {
        DeclaredPins::of(w.world.specs.values().filter_map(Evaluated::entity))
    }

    fn run_ca_with(
        w: &CaWorld,
        number: u32,
        item: &WorkItem,
        view: &ImplView,
        tests: TestsRun,
    ) -> Checked {
        let states = w.world.states();
        let key = key(number);
        let evaluation = evaluation_of(w, number, view);
        ca::check(CaInput {
            key: &key,
            item,
            evaluation: &evaluation,
            designs: w.world.design_refs(),
            declared: &declared_by(w),
            states: &states,
            impl_view: view,
            revision: RevisionId::from_validated("head".into()).unwrap(),
            tests,
        })
    }

    fn run_ca(w: &CaWorld, number: u32, pins: &[&str], tests: TestOutcome) -> Checked {
        let it = w.design.items()[&num(number)].clone();
        run_ca_with(w, number, &it, &view(pins), TestsRun::Finished(tests))
    }

    #[test]
    fn ca_counts_a_pin_declared_by_any_spec_of_the_project() {
        // A second Spec declares PIN-OTHER; the tests of the project carry it. The
        // item of the first Spec must still pass: the tag is declared, by another Spec.
        let f1 = finding(1, "one");
        let first = spec(11, &[1], &["PIN-A", "UT-B"]);
        let second = spec(12, &[1], &["PIN-OTHER"]);
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        let events = vec![
            approve(cr_att(1, f1.hash()), &[]),
            approve(
                cs_att(
                    11,
                    first.contract_hash(),
                    first.hyp_hash(),
                    &[(1, *f1.hash())],
                ),
                &[],
            ),
            approve(ci_att(21, d.design_hash(), 11, first.contract_hash()), &[]),
        ];
        let w = CaWorld {
            world: World::new(&[&f1], &[&first, &second], &[&d], &events),
            design: d,
        };
        assert!(run_ca(&w, 1, &["PIN-A", "PIN-OTHER"], TestOutcome::Passed).is_ok());
        assert_eq!(
            violations(run_ca(
                &w,
                1,
                &["PIN-A", "PIN-NOWHERE"],
                TestOutcome::Passed
            )),
            vec![CheckViolation::PinUndeclaredInTests {
                pin: "PIN-NOWHERE".parse().unwrap()
            }]
        );
    }

    #[test]
    fn ca_reports_a_malformed_tag_and_a_timeout_and_records_neither() {
        let w = ca_world(true);
        let it = w.design.items()[&num(1)].clone();
        let at: CodeLocation = "tests/a.rs:9".parse().unwrap();
        let text = NonBlank::new("PIN-foo").unwrap();
        let tagged =
            view(&["PIN-A"]).with_malformed(vec![MalformedPin::new(at.clone(), text.clone())]);
        assert_eq!(
            violations(run_ca_with(
                &w,
                1,
                &it,
                &tagged,
                TestsRun::Finished(TestOutcome::Passed)
            )),
            vec![CheckViolation::MalformedPin { location: at, text }]
        );
        let after = std::time::Duration::from_secs(60);
        assert_eq!(
            violations(run_ca_with(
                &w,
                1,
                &it,
                &view(&["PIN-A"]),
                TestsRun::TimedOut { after }
            )),
            vec![CheckViolation::TestsTimedOut { after }]
        );
    }

    #[test]
    fn ca_passes_when_everything_lines_up_and_records_the_revision() {
        let w = ca_world(true);
        let passed = passed(run_ca(&w, 1, &["PIN-A"], TestOutcome::Passed));
        assert_eq!(passed.gate(), Gate::Ca);
        let Attestation::Ca {
            item,
            design,
            revision,
            tests,
        } = passed.attestation()
        else {
            panic!("a CA attestation");
        };
        assert_eq!(item.id(), &iid(31));
        assert_eq!(item.hash(), &w.design.item_hash(num(1)).unwrap());
        assert_eq!(design.hash(), w.design.design_hash());
        assert_eq!(
            revision,
            &Revision::Commit(RevisionId::from_validated("head".into()).unwrap())
        );
        assert_eq!(*tests, TestOutcome::Passed);
    }

    #[test]
    fn ca_checks_both_directions_of_the_pin_match() {
        let w = ca_world(true);
        // Item 1 is assigned PIN-A; the tests have PIN-Z (undeclared) and not PIN-A.
        assert_eq!(
            violations(run_ca(&w, 1, &["PIN-Z"], TestOutcome::Passed)),
            vec![
                CheckViolation::PinMissingInTests {
                    pin: "PIN-A".parse().unwrap()
                },
                CheckViolation::PinUndeclaredInTests {
                    pin: "PIN-Z".parse().unwrap()
                },
            ]
        );
        // A tag of a sibling item in the tests is declared, so it is fine.
        assert!(run_ca(&w, 1, &["PIN-A", "UT-B"], TestOutcome::Passed).is_ok());
    }

    #[test]
    fn ca_requires_passing_tests_and_a_ready_design() {
        let w = ca_world(true);
        assert_eq!(
            violations(run_ca(&w, 1, &["PIN-A"], TestOutcome::Failed)),
            vec![CheckViolation::TestsFailed]
        );
        let draft = ca_world(false);
        assert_eq!(
            violations(run_ca(&draft, 1, &["PIN-A"], TestOutcome::Passed)),
            vec![upstream(21, UpstreamProblem::NotApproved)]
        );
    }

    #[test]
    fn ca_rejects_an_item_that_is_not_the_one_in_the_design() {
        let w = ca_world(true);
        let other = item("a, but reworded", &["PIN-A"]);
        assert_eq!(
            violations(run_ca_with(
                &w,
                1,
                &other,
                &view(&["PIN-A"]),
                TestsRun::Finished(TestOutcome::Passed)
            )),
            vec![CheckViolation::ItemNotInDesign]
        );
    }

    #[test]
    fn an_invalid_item_is_an_invalid_target_at_ca() {
        let w = ca_world(true);
        let states = w.world.states();
        let key = key(1);
        let view = view(&["PIN-A"]);
        // The item's Design is not in the map: the item is Invalid (dangling).
        let evaluation = Evaluator::item(
            &key,
            &w.design.items()[&num(1)],
            &NO_DESIGNS,
            &BTreeMap::new(),
            &ledger(&[]),
            &view,
            &[],
        );
        let got = violations(ca::check(CaInput {
            key: &key,
            item: &w.design.items()[&num(1)],
            evaluation: &evaluation,
            designs: w.world.design_refs(),
            declared: &declared_by(&w),
            states: &states,
            impl_view: &view,
            revision: RevisionId::from_validated("head".into()).unwrap(),
            tests: TestsRun::Finished(TestOutcome::Passed),
        }));
        assert!(
            matches!(&got[..], [CheckViolation::TargetInvalid { reasons }]
            if reasons.first() == &InvalidReason::DanglingRef { upstream: eid(21) })
        );
    }

    // --- upstreams that cannot be read here ---------------------------------

    /// A Spec that was approved where the private store is, looked at where it is not.
    fn approved_partial_spec() -> (Spec, Evaluated<Spec>, Vec<Event>) {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A", "UT-B"]);
        let events = vec![
            approve(cr_att(1, f1.hash()), &[]),
            approve(
                cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        let states = states_of(&NO_FINDINGS, &NO_SPECS, &NO_DESIGNS, []);
        let es = Evaluator::spec(
            &s,
            &NO_FINDINGS,
            &states,
            &ledger(&events),
            &unreadable_ground(),
            &[],
        );
        (s, es, events)
    }

    #[test]
    fn ci_stops_on_an_approved_spec_that_is_partial() {
        let (s, es, _) = approved_partial_spec();
        assert_eq!(es.state().approval(), &Approval::Approved);
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        let specs = BTreeMap::from([(s.id(), es)]);
        let designs = {
            let states = states_of(&NO_FINDINGS, &specs, &NO_DESIGNS, []);
            BTreeMap::from([(
                d.id(),
                Evaluator::design(&d, &specs, &states, &ledger(&[]), &Written::default(), &[]),
            )])
        };
        let states = states_of(&NO_FINDINGS, &specs, &designs, []);
        assert_eq!(
            violations(ci::check(CiInput {
                design: &designs[&d.id()],
                specs: specs.iter().map(|(id, ev)| (*id, ev)).collect(),
                states: &states,
            })),
            vec![upstream(11, UpstreamProblem::Unresolved)]
        );
    }

    #[test]
    fn ca_stops_on_an_approved_design_that_is_partial_further_up() {
        let (s, es, mut events) = approved_partial_spec();
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        events.push(approve(
            ci_att(21, d.design_hash(), 11, s.contract_hash()),
            &[],
        ));
        let specs = BTreeMap::from([(s.id(), es)]);
        let ed = {
            let states = states_of(&NO_FINDINGS, &specs, &NO_DESIGNS, []);
            Evaluator::design(
                &d,
                &specs,
                &states,
                &ledger(&events),
                &Written::default(),
                &[],
            )
        };
        assert_eq!(ed.state().approval(), &Approval::Approved);
        let designs = BTreeMap::from([(did(21), ed)]);
        let states = states_of(&NO_FINDINGS, &specs, &designs, []);
        let view = view(&["PIN-A"]);
        let key = key(1);
        let evaluation = Evaluator::item(
            &key,
            &d.items()[&num(1)],
            &designs,
            &states,
            &ledger(&events),
            &view,
            &[],
        );
        assert_eq!(
            violations(ca::check(CaInput {
                key: &key,
                item: &d.items()[&num(1)],
                evaluation: &evaluation,
                designs: designs.iter().map(|(id, ev)| (*id, ev)).collect(),
                declared: &DeclaredPins::of(specs.values().filter_map(Evaluated::entity)),
                states: &states,
                impl_view: &view,
                revision: RevisionId::from_validated("head".into()).unwrap(),
                tests: TestsRun::Finished(TestOutcome::Passed),
            })),
            vec![upstream(21, UpstreamProblem::Unresolved)]
        );
    }

    // --- CM ---------------------------------------------------------------

    fn measured_finding(n: u128) -> Finding {
        Finding::new(
            fid(n),
            NonBlank::new("measurement").unwrap(),
            FindingSource::MeasuredData {
                measured_at: crate::domain::actor::MeasuredAt::from_validated(
                    "2026-11-04T12:40:00Z".into(),
                )
                .unwrap(),
                revision: RevisionId::from_validated("a1".into()).unwrap(),
                environment: NonBlank::new("4 cores").unwrap(),
            },
            EvidenceLabel::Measured,
            "8 s".into(),
        )
    }

    fn cm_of(
        s: &Spec,
        events: &[Event],
        result: CmResult,
        hyp_finding: Option<&Finding>,
    ) -> Checked {
        // Ground 1 is there, so the Spec is not Invalid for it.
        let ground = finding(1, "one");
        let world = World::new(&[&ground], &[s], &[], events);
        let l = ledger(events);
        let hyp = hyp_finding.map(|f| Evaluator::finding(f, &l, &[]));
        cm::check(CmInput {
            spec: &world.specs[&s.id()],
            ledger: &l,
            result,
            hyp_finding: hyp.as_ref(),
        })
    }

    #[test]
    fn cm_needs_the_cs_approval_that_fixed_the_current_hypotheses() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let a_cs = approve(
            cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[],
        );
        let passed = passed(cm_of(&s, std::slice::from_ref(&a_cs), CmResult::Pass, None));
        assert_eq!(passed.gate(), Gate::Cm);
        assert_eq!(
            passed.attestation(),
            &Attestation::Cm {
                spec: Pinned::new(s.id(), *s.hyp_hash()),
                cs: a_cs.id(),
                result: CmResult::Pass,
            }
        );

        // No CS approval at all, or only for another Spec's.
        assert_eq!(
            violations(cm_of(&s, &[], CmResult::Pass, None)),
            vec![CheckViolation::CsApprovalNotFound]
        );
        let other = spec(12, &[1], &["PIN-A"]);
        assert_eq!(
            violations(cm_of(
                &other,
                std::slice::from_ref(&a_cs),
                CmResult::Pass,
                None
            )),
            vec![CheckViolation::CsApprovalNotFound]
        );

        // Only a CR approval of the same kind of id: not a CS approval.
        let a_cr = approve(cr_att(1, f1.hash()), &[]);
        assert_eq!(
            violations(cm_of(&s, &[a_cs.clone(), a_cr][1..], CmResult::Pass, None)),
            vec![CheckViolation::CsApprovalNotFound]
        );

        // Withdrawn.
        let rev = revoke(sid(11).entity(), a_cs.id(), &[a_cs.id()]);
        assert_eq!(
            violations(cm_of(&s, &[a_cs.clone(), rev], CmResult::Pass, None)),
            vec![CheckViolation::CsApprovalNotFound]
        );

        // The hypotheses changed after the approval.
        let changed = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            pins(&["PIN-A"]),
            BTreeMap::from([(
                "HYP-OTHER".parse().unwrap(),
                crate::domain::pin::Hypothesis::new("m", "1").unwrap(),
            )]),
        )
        .unwrap();
        assert_eq!(
            violations(cm_of(
                &changed,
                std::slice::from_ref(&a_cs),
                CmResult::Pass,
                None
            )),
            vec![CheckViolation::HypothesisChanged {
                fixed: *s.hyp_hash(),
                current: *changed.hyp_hash()
            }]
        );
    }

    #[test]
    fn cm_prefers_the_cs_approval_that_fixed_the_hypotheses_as_they_are() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let older = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            pins(&["PIN-A"]),
            BTreeMap::from([(
                "HYP-OLD".parse().unwrap(),
                crate::domain::pin::Hypothesis::new("m", "1").unwrap(),
            )]),
        )
        .unwrap();
        let old = approve(
            cs_att(
                11,
                older.contract_hash(),
                older.hyp_hash(),
                &[(1, *f1.hash())],
            ),
            &[],
        );
        let current = approve(
            cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[old.id()],
        );
        let passed = passed(cm_of(&s, &[old, current.clone()], CmResult::Pass, None));
        assert!(matches!(passed.attestation(),
            Attestation::Cm { cs, .. } if *cs == current.id()));
    }

    /// Two branches registered the same Spec file before they met. The one that
    /// survives the merge is the smallest identifier; a CS approval recorded
    /// against the other still counts for it.
    #[test]
    fn cm_finds_a_cs_approval_recorded_before_two_registrations_merged() {
        let f1 = finding(1, "one");
        let first = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let second = register_doc(2, DocKind::Spec, "docs/spec/a.md", None, 1);
        let s = spec(1, &[1], &["PIN-A"]);
        // The CS approval was recorded against the duplicate, identifier 2.
        let a_cs = approve(
            cs_att(2, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[second.id()],
        );
        let events = [first, second, a_cs.clone()];
        let l = ledger(&events);
        let world = World::new(&[&f1], &[&s], &[], &events);
        let passed = passed(cm::check(CmInput {
            spec: &world.specs[&s.id()],
            ledger: &l,
            result: CmResult::Pass,
            hyp_finding: None,
        }));
        assert!(matches!(passed.attestation(), Attestation::Cm { cs, .. } if *cs == a_cs.id()));
    }

    #[test]
    fn a_fail_hyp_needs_a_measured_data_finding() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let a_cs = approve(
            cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[],
        );
        let events = [a_cs];
        let result = |n: u128| CmResult::FailHyp { finding: fid(n) };
        // A usage observation is not a measurement.
        assert_eq!(
            violations(cm_of(&s, &events, result(2), Some(&finding(2, "text")))),
            vec![CheckViolation::MeasurementNotMeasuredData { finding: fid(2) }]
        );
        // A Finding that is not there at all.
        assert_eq!(
            violations(cm_of(&s, &events, result(2), None)),
            vec![CheckViolation::MeasurementNotMeasuredData { finding: fid(2) }]
        );
        // A measurement.
        assert!(cm_of(&s, &events, result(2), Some(&measured_finding(2))).is_ok());
        // Both can be wrong together, and both are reported.
        assert_eq!(
            violations(cm_of(&s, &[], result(2), None)),
            vec![
                CheckViolation::MeasurementNotMeasuredData { finding: fid(2) },
                CheckViolation::CsApprovalNotFound
            ]
        );
    }

    #[test]
    fn an_invalid_spec_cannot_have_a_measurement_recorded() {
        // The Spec's ground does not exist, so the Spec is Invalid.
        let s = spec(11, &[1], &["PIN-A"]);
        let world = World::new(&[], &[&s], &[], &[]);
        let got = violations(cm::check(CmInput {
            spec: &world.specs[&s.id()],
            ledger: &ledger(&[]),
            result: CmResult::Pass,
            hyp_finding: None,
        }));
        assert!(
            matches!(&got[..], [CheckViolation::TargetInvalid { reasons }]
            if reasons.first() == &InvalidReason::DanglingRef { upstream: eid(1) })
        );
    }

    #[test]
    fn the_gates_pin_what_the_documented_table_says() {
        // CR/CS/CI/CA/CM each pin their own attestation (see the module table):
        // the gate of a Passed is the gate of the attestation it carries.
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let events = [
            approve(cr_att(1, f1.hash()), &[]),
            approve(
                cs_att(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        let world = World::new(&[&f1], &[&s], &[], &events);
        let cr = passed(cr::check(CrInput {
            finding: &world.findings[&f1.id()],
        }));
        let cm = passed(cm_of(&s, &events, CmResult::Pass, None));
        for p in [&cr, &cm] {
            assert_eq!(p.attestation().gate(), p.gate());
        }
    }
}
