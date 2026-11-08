//! The table from a next step to its use case, and that the use case it names, run from
//! the state that produced the step, is done.

use super::*;
use crate::app::corpus::fixtures::{
    DESIGN, FINDING, ITEM, SPEC, World, by_id, item_one, place, raw_document, rel, text_of,
};
use crate::app::corpus::{
    AbortPublish, Adopt, Approve, GateCommand, Import, Measured, Publish, Retire, abort_publish,
    adopt, approve, import, publish, retire,
};
use crate::app::fake::block_on;
use crate::app::outcome::Outcome;
use crate::app::ports::{ConfirmAction, Confirmed};
use crate::domain::corpus::{Location, Visibility};
use crate::domain::event::EntityKind;
use crate::domain::finding::test_support::finding;
use crate::domain::id::test_support::{docid, iid};
use crate::domain::judgement::Target;
use crate::domain::knowable::Knowable;
use crate::domain::naming::Ref;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::plan_next::Need;
use crate::domain::publication::ConfirmDigest;
use crate::domain::state::InvalidReason;

fn finding_entity() -> Entity {
    Entity::Finding(finding(FINDING, "body"))
}

/// Every action there is, once. A new action must be added here as well as to
/// [`use_case_for`], whose `match` the compiler holds to the list of actions.
fn every_action() -> Vec<NextAction> {
    let gates = Gate::ALL;
    let mut actions = vec![
        NextAction::Edit(EditFor::Restore),
        NextAction::Import,
        NextAction::Adopt,
        NextAction::Measure,
        NextAction::Retire,
        NextAction::ResumePublish,
        NextAction::AbortPublish,
    ];
    actions.extend(gates.into_iter().map(NextAction::Approve));
    actions.extend(gates.into_iter().map(NextAction::Reapprove));
    actions
}

fn gate_command(gate: Gate) -> GateCommand {
    match gate {
        Gate::Cr => GateCommand::Cr,
        Gate::Cs => GateCommand::Cs,
        Gate::Ci => GateCommand::Ci,
        Gate::Ca => World::ca(),
        Gate::Cm => GateCommand::Cm {
            result: Measured::Pass,
        },
    }
}

fn target_of(gate: Gate) -> Ref {
    match gate {
        Gate::Cr => by_id(FINDING),
        Gate::Cs | Gate::Cm => by_id(SPEC),
        Gate::Ci => by_id(DESIGN),
        Gate::Ca => item_one(),
    }
}

fn is_done<T>(run: Result<Outcome<T>, crate::app::error::AppError>) -> bool {
    matches!(run, Ok(Outcome::Done(_)))
}

// --- the table --------------------------------------------------------------------

#[test]
fn every_action_has_the_use_case_the_table_names() {
    for action in every_action() {
        let expected = match &action {
            NextAction::Edit(hint) => UseCase::EditFile { hint: hint.clone() },
            NextAction::Import => UseCase::Import,
            NextAction::Approve(gate) | NextAction::Reapprove(gate) => UseCase::Approve(*gate),
            NextAction::Measure => UseCase::Approve(Gate::Cm),
            NextAction::Adopt => UseCase::Adopt,
            NextAction::Retire => UseCase::Retire,
            NextAction::ResumePublish => UseCase::Publish,
            NextAction::AbortPublish => UseCase::AbortPublish,
        };
        assert_eq!(use_case_for(&action), expected, "{action:?}");
    }
}

#[test]
fn resuming_a_publication_is_publishing_with_no_flag() {
    // Whether a run starts or finishes a publication is the plan's. The table has one
    // `Publish` for the one action that finishes it.
    assert_eq!(use_case_for(&NextAction::ResumePublish), UseCase::Publish);
}

// --- an action is done from the state that produced it ----------------------------

#[test]
fn approving_a_gate_is_done_from_the_state_that_asks_for_it() {
    for gate in Gate::ALL {
        let world = World::project(Visibility::Public);
        match gate {
            Gate::Cr => {}
            Gate::Cs | Gate::Cm => {
                world.approved(GateCommand::Cr, by_id(FINDING));
                if gate == Gate::Cm {
                    world.approved(GateCommand::Cs, by_id(SPEC));
                }
            }
            Gate::Ci => {
                world.approved(GateCommand::Cr, by_id(FINDING));
                world.approved(GateCommand::Cs, by_id(SPEC));
            }
            Gate::Ca => world.approved_to_ci(),
        }
        for action in [NextAction::Approve(gate), NextAction::Reapprove(gate)] {
            let UseCase::Approve(named) = use_case_for(&action) else {
                panic!("{action:?} should be an approval");
            };
            assert_eq!(named, gate);
            let run = block_on(approve(
                &world.fake,
                &world.config,
                Approve {
                    target: target_of(named),
                    gate: gate_command(named),
                },
            ));
            assert!(is_done(run), "{action:?} did not come to done");
        }
    }
}

#[test]
fn measuring_is_the_approval_of_cm_and_is_done_once_the_hypotheses_are_fixed() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    world.approved(GateCommand::Cs, by_id(SPEC));
    let UseCase::Approve(gate) = use_case_for(&NextAction::Measure) else {
        panic!("a measurement is an approval");
    };
    assert_eq!(gate, Gate::Cm);
    let run = block_on(approve(
        &world.fake,
        &world.config,
        Approve {
            target: by_id(SPEC),
            gate: gate_command(gate),
        },
    ));
    assert!(is_done(run));
}

#[test]
fn importing_is_done_for_a_file_with_no_id_and_asks_nobody() {
    let world = World::new();
    world.unidentified(
        Visibility::Public,
        "new.md",
        "---\nkind: finding\n---\nbody\n",
    );
    assert_eq!(use_case_for(&NextAction::Import), UseCase::Import);
    let run = block_on(import(
        &world.fake,
        &world.config,
        Import::File {
            at: Location::new(Visibility::Public, rel("new.md"), Knowable::Unknown),
            adopt: None,
        },
    ));
    assert!(is_done(run));
    // `Import` has no need of a confirmation, and an import that adopts nothing asks
    // for none.
    assert!(NextAction::Import.needs().is_empty());
    assert!(world.fake.asked().is_empty());
}

#[test]
fn adopting_is_done_after_a_person_confirms() {
    let world = World::project(Visibility::Public);
    assert_eq!(use_case_for(&NextAction::Adopt), UseCase::Adopt);
    let run = block_on(adopt(
        &world.fake,
        &world.config,
        Adopt {
            gate: Gate::Cr,
            target: by_id(FINDING),
            source: "the old tracker".to_owned(),
        },
    ));
    assert!(is_done(run));
    assert_eq!(world.fake.asked().len(), 1);
}

#[test]
fn retiring_is_done_for_something_whose_file_is_gone() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    world.remove(Visibility::Public, "finding.md");
    assert_eq!(use_case_for(&NextAction::Retire), UseCase::Retire);
    let run = block_on(retire(
        &world.fake,
        &world.config,
        Retire {
            target: Target::Doc(docid(FINDING)),
            reason: crate::domain::judgement::test_support::why(),
        },
    ));
    assert!(is_done(run));
}

/// A publication that stopped after the file was made: both files, the same bytes.
fn copied_file() -> World {
    let world = World::new();
    world.add(Visibility::Private, "f.md", finding_entity());
    place(
        &world.fake,
        Visibility::Public,
        "docs/f.md",
        raw_document(finding_entity(), &text_of(&finding_entity())),
    );
    world
}

#[test]
fn resuming_a_publication_is_done_after_a_person_confirms_what_cannot_be_taken_back() {
    let world = copied_file();
    assert_eq!(use_case_for(&NextAction::ResumePublish), UseCase::Publish);
    let run = block_on(publish(
        &world.fake,
        &world.config,
        Publish {
            doc: by_id(FINDING),
            to: None,
        },
    ));
    assert!(is_done(run));
    let asked = world.fake.asked();
    assert_eq!(asked.len(), 1);
    assert!(matches!(asked[0].digest(), ConfirmDigest::Plan(_)));
    assert_eq!(asked[0].summary().action(), ConfirmAction::Publish);
}

#[test]
fn aborting_a_publication_is_done() {
    let world = copied_file();
    assert_eq!(
        use_case_for(&NextAction::AbortPublish),
        UseCase::AbortPublish
    );
    let run = block_on(abort_publish(
        &world.fake,
        &world.config,
        AbortPublish {
            doc: by_id(FINDING),
        },
    ));
    assert!(is_done(run));
}

// --- the plan decides whether a person is asked, and the action does not -----------

#[test]
fn whether_a_person_is_asked_is_the_plans_and_not_what_the_action_says_it_needs() {
    // Both actions say they need a confirmation if the plan has a step that cannot be
    // taken back.
    for action in [NextAction::ResumePublish, NextAction::AbortPublish] {
        assert!(action.needs().contains(&Need::ConfirmationIfIrreversible));
    }

    // A plan that has such a step is asked about: the run needs the reply.
    let world = copied_file();
    world
        .fake
        .set_reply(crate::app::fake::Reply::Always(Confirmed::No));
    let resumed = block_on(publish(
        &world.fake,
        &world.config,
        Publish {
            doc: by_id(FINDING),
            to: None,
        },
    ));
    assert!(matches!(
        resumed,
        Ok(Outcome::Refused(
            crate::app::outcome::Refusal::ConfirmDeclined
        ))
    ));
    assert_eq!(world.fake.asked().len(), 1);

    // A plan that has none is not, whatever the action says, and the reply that would
    // have stopped a run that asked does not stop this one.
    let world = copied_file();
    world
        .fake
        .set_reply(crate::app::fake::Reply::Always(Confirmed::No));
    let aborted = block_on(abort_publish(
        &world.fake,
        &world.config,
        AbortPublish {
            doc: by_id(FINDING),
        },
    ));
    assert!(is_done(aborted));
    assert!(world.fake.asked().is_empty());
}

// --- an edit ----------------------------------------------------------------------

#[test]
fn an_edit_is_the_persons_and_keeps_what_it_is_for() {
    let hints = [
        EditFor::Restore,
        EditFor::StableItemRef,
        EditFor::DropMark(docid(FINDING)),
        EditFor::Implement {
            pins: NonEmptyVec::new("PIN-A".parse().expect("a pin"), Vec::new()),
        },
        EditFor::Fix(NonEmptyVec::new(
            InvalidReason::KindChanged {
                was: EntityKind::Finding,
                now: EntityKind::Spec,
            },
            Vec::new(),
        )),
    ];
    for hint in hints {
        assert_eq!(
            use_case_for(&NextAction::Edit(hint.clone())),
            UseCase::EditFile { hint }
        );
    }
}

#[test]
fn the_file_of_an_edit_is_where_the_target_is() {
    let world = World::project(Visibility::Public);
    world.unidentified(Visibility::Private, "new.md", "text\n");
    world.broken(Visibility::Public, "broken.md", FINDING + 50);
    let snapshot = block_on(crate::app::corpus::Snapshot::read(
        &world.fake,
        world.config.gate_policy(),
    ))
    .expect("a snapshot");
    let corpus = snapshot.corpus();

    assert_eq!(
        edit_path(&EntityRef::Doc(docid(SPEC)), corpus),
        Some(rel("spec.md"))
    );
    // An item is in the file of its Design.
    assert_eq!(
        edit_path(&EntityRef::Item(iid(ITEM)), corpus),
        Some(rel("design.md"))
    );
    // A file with no id is known by where it is.
    let at = Location::new(Visibility::Private, rel("new.md"), Knowable::Unknown);
    assert_eq!(
        edit_path(&EntityRef::File(Box::new(at)), corpus),
        Some(rel("new.md"))
    );
    // A document that cannot be read still has its file.
    assert_eq!(
        edit_path(&EntityRef::Doc(docid(FINDING + 50)), corpus),
        Some(rel("broken.md"))
    );
    // A document that has no file has nothing to edit.
    assert_eq!(edit_path(&EntityRef::Doc(docid(999)), corpus), None);
    assert_eq!(edit_path(&EntityRef::Item(iid(999)), corpus), None);
}
