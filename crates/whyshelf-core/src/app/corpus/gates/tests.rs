//! The gates through the fake ports: what a check finds, what an approval writes, and
//! that a check writes nothing.

use super::*;
use crate::app::corpus::fixtures::{DESIGN, FINDING, ITEM, SPEC, World, by_id, item_one, revision};
use crate::app::fake::block_on;
use crate::app::outcome::{ExitClass, Miss, exit_class};
use crate::app::ports::CorpusRead;
use crate::domain::approval::{Attestation, CaChange, CsRef, TestOutcome};
use crate::domain::corpus::{Entity, Visibility};
use crate::domain::finding::test_support::finding;
use crate::domain::hash::TextHash;
use crate::domain::id::test_support::{docid, fid, iid, sid};
use crate::domain::integration::{FingerprintUnavailable, Fingerprinting};
use crate::domain::lint::CheckViolation;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::spec::Spec;
use crate::domain::spec::test_support::{hyps, pins};

type Run<T> = Result<Outcome<T>, AppError>;

fn approve_run(world: &World, gate: GateCommand, target: Ref) -> Run<Approved> {
    block_on(approve(
        &world.fake,
        &world.config,
        Approve { target, gate },
    ))
}

fn check_run(world: &World, gate: GateCommand, target: Ref) -> Run<CheckPassed> {
    block_on(check(&world.fake, &world.config, Check { target, gate }))
}

fn outcome<T>(run: Run<T>) -> Outcome<T> {
    run.expect("an outcome, not an error")
}

fn violations_of<T: std::fmt::Debug>(run: Run<T>) -> Vec<CheckViolation> {
    match outcome(run) {
        Outcome::Violations(violations) => violations.into_vec(),
        other => panic!("violations were expected: {other:?}"),
    }
}

fn refusal_of<T: std::fmt::Debug>(run: Run<T>) -> Refusal {
    match outcome(run) {
        Outcome::Refused(refusal) => refusal,
        other => panic!("a refusal was expected: {other:?}"),
    }
}

/// What the record `id` of a ledger attests.
fn attestation_of(
    world: &World,
    side: Visibility,
    id: crate::domain::judgement::JudgementId,
) -> Attestation {
    let record = world
        .fake
        .records(side)
        .into_iter()
        .find(|record| record.id() == id)
        .expect("the record is in the ledger");
    match record.kind() {
        JudgementKind::Approve { passed, .. } => passed.attestation().clone(),
        other => panic!("an approval was expected: {other:?}"),
    }
}

fn cm_pass() -> GateCommand {
    GateCommand::Cm {
        result: Measured::Pass,
    }
}

// --- check: the machine layer, which writes nothing --------------------------------

#[test]
fn a_check_that_finds_nothing_is_done_and_calls_no_write() {
    let world = World::project(Visibility::Public);
    let passed = outcome(check_run(&world, GateCommand::Cr, by_id(FINDING)));
    let Outcome::Done(passed) = passed else {
        panic!("the check should pass");
    };
    assert_eq!(passed.gate(), Gate::Cr);
    assert_eq!(passed.target(), Target::Doc(docid(FINDING)));
    assert_eq!(world.fake.writes(), 0);
    assert_eq!(world.records(Visibility::Public), 0);
}

#[test]
fn every_gate_checks_without_a_write() {
    let world = World::project(Visibility::Public);
    world.approved_to_ci();
    world.approved(World::ca(), item_one());
    world.approved(cm_pass(), by_id(SPEC));
    let written = world.fake.writes();
    assert!(written > 0);

    let runs = [
        (GateCommand::Cr, by_id(FINDING), Target::Doc(docid(FINDING))),
        (GateCommand::Cs, by_id(SPEC), Target::Doc(docid(SPEC))),
        (GateCommand::Ci, by_id(DESIGN), Target::Doc(docid(DESIGN))),
        (World::ca(), item_one(), Target::Item(iid(ITEM))),
        (cm_pass(), by_id(SPEC), Target::Doc(docid(SPEC))),
    ];
    for (gate, target, expected) in runs {
        let name = gate.gate();
        match outcome(check_run(&world, gate, target)) {
            Outcome::Done(passed) => {
                assert_eq!(passed.gate(), name);
                assert_eq!(passed.target(), expected);
            }
            other => panic!("{name:?} should pass: {other:?}"),
        }
    }
    assert_eq!(
        world.fake.writes(),
        written,
        "a check called a write method"
    );
}

#[test]
fn a_check_reports_every_violation_and_an_approval_over_them_writes_nothing() {
    let world = World::new();
    world.add(
        Visibility::Public,
        "finding.md",
        Entity::Finding(finding(FINDING, "")),
    );
    let found = violations_of(check_run(&world, GateCommand::Cr, by_id(FINDING)));
    assert!(found.contains(&CheckViolation::EmptyBody));

    let refused = approve_run(&world, GateCommand::Cr, by_id(FINDING));
    assert_eq!(exit_class(&refused), ExitClass::Violations);
    assert!(violations_of(refused).contains(&CheckViolation::EmptyBody));
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_gate_stops_on_an_upstream_that_is_not_approved() {
    let world = World::project(Visibility::Public);
    let found = violations_of(approve_run(&world, GateCommand::Cs, by_id(SPEC)));
    assert!(
        found
            .iter()
            .any(|violation| matches!(violation, CheckViolation::Upstream { .. }))
    );
    assert_eq!(world.records(Visibility::Public), 0);
}

// --- approve ----------------------------------------------------------------------

#[test]
fn each_gate_approves_and_writes_its_record_and_its_text() {
    let world = World::project(Visibility::Public);
    let cr = world.approved(GateCommand::Cr, by_id(FINDING));
    assert_eq!(cr.gate(), Gate::Cr);
    assert_eq!(cr.target(), Target::Doc(docid(FINDING)));
    assert_eq!(cr.ledger(), Visibility::Public);
    let cs = world.approved(GateCommand::Cs, by_id(SPEC));
    let ci = world.approved(GateCommand::Ci, by_id(DESIGN));
    let ca = world.approved(World::ca(), item_one());
    assert_eq!(ca.target(), Target::Item(iid(ITEM)));
    let cm = world.approved(cm_pass(), by_id(SPEC));
    assert_eq!(world.records(Visibility::Public), 5);
    assert_eq!(world.records(Visibility::Private), 0);

    assert_eq!(ci.gate(), Gate::Ci);
    assert_eq!(cm.gate(), Gate::Cm);

    // The text of the version approved is kept, under the hash the gate read.
    assert!(world.fake.has_object(
        Visibility::Public,
        TextHash::of(&format!(
            "{:?}\n",
            Entity::Finding(finding(FINDING, "body"))
        ))
    ));

    // A measurement is set against the CS approval that fixed the hypotheses.
    match attestation_of(&world, Visibility::Public, cm.id()) {
        Attestation::Cm { cs: fixed, .. } => assert_eq!(fixed, CsRef::Judgement(cs.id())),
        other => panic!("a CM attestation was expected: {other:?}"),
    }
}

#[test]
fn the_record_goes_to_the_ledger_of_the_documents_root() {
    let world = World::project(Visibility::Private);
    let cr = world.approved(GateCommand::Cr, by_id(FINDING));
    assert_eq!(cr.ledger(), Visibility::Private);
    assert_eq!(world.records(Visibility::Private), 1);
    assert_eq!(world.records(Visibility::Public), 0);
    assert!(world.fake.has_object(
        Visibility::Private,
        TextHash::of(&format!(
            "{:?}\n",
            Entity::Finding(finding(FINDING, "body"))
        ))
    ));
}

#[test]
fn an_edited_document_is_approved_again_by_a_new_record() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    world.remove(Visibility::Public, "finding.md");
    world.add(
        Visibility::Public,
        "finding.md",
        Entity::Finding(finding(FINDING, "a better body")),
    );
    world.approved(GateCommand::Cr, by_id(FINDING));
    assert_eq!(world.records(Visibility::Public), 2);
}

// --- CA ---------------------------------------------------------------------------

#[test]
fn a_ca_approval_records_the_revision_and_what_version_control_said_of_the_change() {
    let world = World::project(Visibility::Public);
    world.approved_to_ci();

    let nothing = world.approved(World::ca(), item_one());
    match attestation_of(&world, Visibility::Public, nothing.id()) {
        Attestation::Ca {
            revision: recorded,
            change,
            tests,
            ..
        } => {
            assert_eq!(
                recorded,
                crate::domain::revision::Revision::Commit(revision())
            );
            assert_eq!(change, CaChange::NothingToFingerprint);
            assert_eq!(tests, TestOutcome::Passed);
        }
        other => panic!("a CA attestation was expected: {other:?}"),
    }
    assert_eq!(world.fake.runner().tests_ran_at(), vec![revision()]);

    // A change that cannot be cut into hunks here is recorded as that, not refused.
    world
        .fake
        .set_fingerprinting(Fingerprinting::Unavailable(FingerprintUnavailable::Shallow));
    let shallow = world.approved(World::ca(), item_one());
    match attestation_of(&world, Visibility::Public, shallow.id()) {
        Attestation::Ca { change, .. } => {
            assert_eq!(
                change,
                CaChange::NotFingerprinted(FingerprintUnavailable::Shallow)
            );
        }
        other => panic!("a CA attestation was expected: {other:?}"),
    }
}

#[test]
fn a_ca_approval_needs_a_clean_tree_and_says_why_when_it_cannot_tell() {
    use crate::app::ports::WorkTree;
    let world = World::project(Visibility::Public);
    world.approved_to_ci();
    let before = world.records(Visibility::Public);

    world.fake.set_work_tree(Knowable::Known(WorkTree::Dirty));
    assert_eq!(
        refusal_of(approve_run(&world, World::ca(), item_one())),
        Refusal::DirtyTree
    );
    // A tree whose state cannot be told is not said to be dirty.
    world.fake.set_work_tree(Knowable::Unknown);
    assert_eq!(
        refusal_of(approve_run(&world, World::ca(), item_one())),
        Refusal::WorkTreeUnknown
    );
    assert_eq!(world.records(Visibility::Public), before);

    // The machine check alone does not need a clean tree.
    world.fake.set_work_tree(Knowable::Known(WorkTree::Dirty));
    assert!(matches!(
        outcome(check_run(&world, World::ca(), item_one())),
        Outcome::Done(_)
    ));
}

#[test]
fn ca_stops_on_failing_tests_a_timeout_and_the_pins_of_the_tests() {
    let world = World::project(Visibility::Public);
    world.approved_to_ci();

    world.fake.runner().set_tests(TestOutcome::Failed);
    assert!(
        violations_of(approve_run(&world, World::ca(), item_one()))
            .contains(&CheckViolation::TestsFailed)
    );

    world.fake.runner().set_tests(TestOutcome::Passed);
    world.fake.runner().set_tests_time_out(true);
    assert!(
        violations_of(approve_run(&world, World::ca(), item_one()))
            .iter()
            .any(|violation| matches!(violation, CheckViolation::TestsTimedOut { .. }))
    );

    world.fake.runner().set_tests_time_out(false);
    world.fake.scanner().set_pins(&[]);
    assert!(
        violations_of(approve_run(&world, World::ca(), item_one()))
            .iter()
            .any(|violation| matches!(violation, CheckViolation::PinMissingInTests { .. }))
    );

    world.fake.scanner().set_pins(&["PIN-A", "PIN-B"]);
    assert!(
        violations_of(approve_run(&world, World::ca(), item_one()))
            .iter()
            .any(|violation| matches!(violation, CheckViolation::PinUndeclaredInTests { .. }))
    );

    world.fake.scanner().set_pins(&["PIN-A"]);
    world
        .fake
        .scanner()
        .set_malformed(&[("tests/t.rs", 4, "@pin: PIN-foo")]);
    assert!(
        violations_of(approve_run(&world, World::ca(), item_one()))
            .iter()
            .any(|violation| matches!(violation, CheckViolation::MalformedPin { .. }))
    );
    assert_eq!(world.records(Visibility::Public), 3);
}

// --- CM ---------------------------------------------------------------------------

fn spec_with_hyps(hyp_ids: &[&str]) -> Spec {
    Spec::new(
        sid(SPEC),
        NonEmptyVec::new(fid(FINDING), Vec::new()),
        crate::domain::nonempty::NonBlank::new("Results are judged on structure, not on text.")
            .expect("a contract"),
        pins(&["PIN-A"]),
        hyps(hyp_ids),
    )
    .expect("a spec")
}

#[test]
fn a_measurement_needs_the_cs_approval_that_fixed_the_hypotheses() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    // No CS approval yet.
    assert!(
        violations_of(approve_run(&world, cm_pass(), by_id(SPEC)))
            .contains(&CheckViolation::CsApprovalNotFound)
    );
    world.approved(GateCommand::Cs, by_id(SPEC));

    // The hypotheses changed since.
    world.remove(Visibility::Public, "spec.md");
    world.add(
        Visibility::Public,
        "spec.md",
        Entity::Spec(spec_with_hyps(&["HYP-OTHER"])),
    );
    assert!(
        violations_of(approve_run(&world, cm_pass(), by_id(SPEC)))
            .iter()
            .any(|violation| matches!(violation, CheckViolation::HypothesisChanged { .. }))
    );
}

#[test]
fn a_failed_measurement_names_what_the_follow_up_needs() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    world.approved(GateCommand::Cs, by_id(SPEC));
    let hint = crate::domain::nonempty::NonBlank::new("an item for the cache").expect("a hint");

    world.approved(
        GateCommand::Cm {
            result: Measured::FailWiring { item_hint: hint },
        },
        by_id(SPEC),
    );

    // The Finding a `fail-hyp` names must be a measurement; this one records a usage.
    let fail_hyp = |finding: Ref| GateCommand::Cm {
        result: Measured::FailHyp { finding },
    };
    assert!(
        violations_of(approve_run(&world, fail_hyp(by_id(FINDING)), by_id(SPEC)))
            .iter()
            .any(|violation| matches!(
                violation,
                CheckViolation::MeasurementNotMeasuredData { .. }
            ))
    );
    // A Finding that is not there is a name that matches nothing.
    let missing = by_id(999);
    assert_eq!(
        outcome(approve_run(&world, fail_hyp(missing.clone()), by_id(SPEC))),
        Outcome::NotFound {
            query: missing,
            miss: Miss::Unmatched {
                hint: Knowable::Known(None)
            }
        }
    );
    // An item names no document, so it is no Finding either.
    assert_eq!(
        refusal_of(approve_run(&world, fail_hyp(item_one()), by_id(SPEC))),
        Refusal::WrongTarget {
            gate: Gate::Cr,
            target: Target::Item(iid(ITEM)),
        }
    );
    // A Spec is no measurement: the gate says so, as it does of any Finding that is not.
    assert!(
        violations_of(approve_run(&world, fail_hyp(by_id(SPEC)), by_id(SPEC)))
            .iter()
            .any(|violation| matches!(
                violation,
                CheckViolation::MeasurementNotMeasuredData { .. }
            ))
    );
}

// --- the target -------------------------------------------------------------------

#[test]
fn a_gate_that_does_not_judge_what_the_name_points_at_is_refused() {
    let world = World::project(Visibility::Public);
    assert_eq!(
        refusal_of(approve_run(&world, GateCommand::Cs, by_id(FINDING))),
        Refusal::WrongTarget {
            gate: Gate::Cs,
            target: Target::Doc(docid(FINDING)),
        }
    );
    assert_eq!(
        refusal_of(approve_run(&world, GateCommand::Cr, item_one())),
        Refusal::WrongTarget {
            gate: Gate::Cr,
            target: Target::Item(iid(ITEM)),
        }
    );
    assert_eq!(
        refusal_of(approve_run(&world, World::ca(), by_id(SPEC))),
        Refusal::WrongTarget {
            gate: Gate::Ca,
            target: Target::Doc(docid(SPEC)),
        }
    );
    for gate in [GateCommand::Ci, cm_pass()] {
        let name = gate.gate();
        assert_eq!(
            refusal_of(approve_run(&world, gate, by_id(FINDING))),
            Refusal::WrongTarget {
                gate: name,
                target: Target::Doc(docid(FINDING)),
            }
        );
    }
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_name_that_matches_nothing_or_cannot_be_looked_up_is_not_a_gate_failure() {
    let world = World::project(Visibility::Public);
    let missing = by_id(999);
    let result = approve_run(&world, GateCommand::Cr, missing.clone());
    assert_eq!(exit_class(&result), ExitClass::NotFound);
    assert!(matches!(
        outcome(result),
        Outcome::NotFound {
            miss: Miss::Unmatched { .. },
            ..
        }
    ));

    let private: Ref = "private:0192f3a4-5b6c-7d8e-8f90-000000000001"
        .parse()
        .expect("a reference");
    let result = check_run(&world, GateCommand::Cr, private);
    assert_eq!(exit_class(&result), ExitClass::Refused);
    assert!(matches!(refusal_of(result), Refusal::CannotNameHere { .. }));
}

#[test]
fn a_document_that_cannot_be_trusted_is_a_violation_whatever_its_file_says() {
    // Two files with one id.
    let world = World::new();
    let one = Entity::Finding(finding(FINDING, "one"));
    let other = Entity::Finding(finding(FINDING, "other"));
    world.add(Visibility::Public, "a.md", one);
    world.add(Visibility::Public, "b.md", other);
    let found = violations_of(approve_run(&world, GateCommand::Cr, by_id(FINDING)));
    assert!(
        found
            .iter()
            .any(|violation| matches!(violation, CheckViolation::TargetInvalid { .. }))
    );

    // A file that cannot be read but shows its id.
    let world = World::new();
    world.broken(Visibility::Public, "broken.md", FINDING);
    let found = violations_of(check_run(&world, GateCommand::Cr, by_id(FINDING)));
    assert!(
        found
            .iter()
            .any(|violation| matches!(violation, CheckViolation::TargetInvalid { .. }))
    );
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_document_whose_publication_is_cut_short_is_not_approved() {
    let world = World::new();
    let entity = Entity::Finding(finding(FINDING, "body"));
    world.add(Visibility::Public, "public.md", entity.clone());
    world.add(Visibility::Private, "private.md", entity);
    assert_eq!(
        refusal_of(approve_run(&world, GateCommand::Cr, by_id(FINDING))),
        Refusal::PublishInProgress(docid(FINDING))
    );
}

// --- what stops the write ----------------------------------------------------------

#[test]
fn a_secret_in_the_text_stops_the_approval_before_anything_is_stored() {
    let world = World::new();
    world.add_text(
        Visibility::Public,
        "finding.md",
        Entity::Finding(finding(FINDING, "body")),
        "AKIA-SECRET\n",
    );
    let refusal = refusal_of(approve_run(&world, GateCommand::Cr, by_id(FINDING)));
    assert!(matches!(refusal, Refusal::Secret(_)));
    assert_eq!(world.fake.writes(), 0);
}

#[test]
fn a_purged_version_is_not_approved_again() {
    let world = World::new();
    world.add_text(
        Visibility::Public,
        "finding.md",
        Entity::Finding(finding(FINDING, "body")),
        "a purged text\n",
    );
    let purged = TextHash::of("a purged text\n");
    world.fake.purge(Visibility::Public, purged);
    assert_eq!(
        refusal_of(approve_run(&world, GateCommand::Cr, by_id(FINDING))),
        Refusal::ObjectPurged(purged)
    );
    assert_eq!(world.records(Visibility::Public), 0);
}

#[test]
fn a_port_that_fails_is_an_error_and_nothing_is_recorded() {
    use crate::app::ports::PortName;
    let world = World::project(Visibility::Public);
    world.fake.fail(PortName::Ledger);
    let result = approve_run(&world, GateCommand::Cr, by_id(FINDING));
    assert_eq!(exit_class(&result), ExitClass::Port);
    assert!(matches!(
        result,
        Err(AppError::Port {
            port: PortName::Ledger,
            ..
        })
    ));
}
