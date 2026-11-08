//! Publishing and aborting through the fake ports, and that a publication stopped at
//! any step reaches the same end when it is run again.

use super::*;
use crate::app::corpus::Snapshot;
use crate::app::corpus::fixtures::{
    DESIGN, FINDING, World, by_id, edited, item_one, place, raw_document, rel, text_of,
};
use crate::app::fake::{Reply, block_on};
use crate::app::outcome::{ExitClass, exit_class};
use crate::app::ports::{ConfirmAction, Confirmed};
use crate::domain::boundary::PrivateRoot;
use crate::domain::corpus::Entity;
use crate::domain::design::test_support::{design, identified};
use crate::domain::finding::test_support::finding;
use crate::domain::id::test_support::docid;
use crate::domain::judge::test_support::{agent, judge};
use crate::domain::judgement::JudgementKind;
use crate::domain::publication::ConfirmDigest;

use crate::app::corpus::GateCommand;

type Run<T> = Result<Outcome<T>, AppError>;

const PUBLIC: &str = "docs/f.md";
const PRIVATE: &str = "f.md";

fn finding_entity() -> Entity {
    Entity::Finding(finding(FINDING, "body"))
}

/// A private Finding that has been approved: its record and its text are in the
/// private side.
fn private_finding() -> World {
    let world = World::new();
    world.add(Visibility::Private, PRIVATE, finding_entity());
    world.approved(GateCommand::Cr, by_id(FINDING));
    world
}

fn publish_run(world: &World, to: Option<&str>) -> Run<Published> {
    block_on(publish(
        &world.fake,
        &world.config,
        Publish {
            doc: by_id(FINDING),
            to: to.map(rel),
        },
    ))
}

fn abort_run(world: &World) -> Run<Aborted> {
    block_on(abort_publish(
        &world.fake,
        &world.config,
        AbortPublish {
            doc: by_id(FINDING),
        },
    ))
}

fn done<T: std::fmt::Debug>(run: Run<T>) -> T {
    match run.expect("an outcome, not an error") {
        Outcome::Done(value) => value,
        other => panic!("done was expected: {other:?}"),
    }
}

fn refusal_of<T: std::fmt::Debug>(run: Run<T>) -> Refusal {
    match run.expect("an outcome, not an error") {
        Outcome::Refused(refusal) => refusal,
        other => panic!("a refusal was expected: {other:?}"),
    }
}

/// What a publication leaves, however it got there.
#[derive(Debug, PartialEq, Eq)]
struct End {
    public_file: Option<Vec<u8>>,
    private_file: Option<Vec<u8>>,
    copied: usize,
    published: usize,
    object_in_public_store: bool,
}

fn end(world: &World) -> End {
    let public = world.fake.records(Visibility::Public);
    End {
        public_file: world.fake.file(Visibility::Public, &rel(PUBLIC)),
        private_file: world.fake.file(Visibility::Private, &rel(PRIVATE)),
        copied: public
            .iter()
            .filter(|record| matches!(record.kind(), JudgementKind::Approve { .. }))
            .count(),
        published: public
            .iter()
            .filter(|record| matches!(record.kind(), JudgementKind::Published { .. }))
            .count(),
        object_in_public_store: world.fake.has_object(
            Visibility::Public,
            crate::domain::hash::TextHash::of(&text_of(&finding_entity())),
        ),
    }
}

/// The end of a publication that was run once, start to finish.
fn the_end() -> End {
    let world = private_finding();
    done(publish_run(&world, Some(PUBLIC)));
    end(&world)
}

/// The plan and what it was made from, as a run makes them.
fn planned(world: &World) -> (Snapshot, Evidence, PublicationPlan) {
    let snapshot =
        block_on(Snapshot::read(&world.fake, world.config.gate_policy())).expect("a snapshot");
    let doc = docid(FINDING);
    let evidence = block_on(read_evidence(&world.fake, &snapshot, doc)).expect("the evidence");
    let plan = plan_now(&snapshot, &evidence, doc, &rel(PUBLIC), &judge("ada")).expect("a plan");
    (snapshot, evidence, plan)
}

fn step_one(world: &World) {
    let (snapshot, evidence, plan) = planned(world);
    block_on(copy_records_and_objects(
        &world.fake,
        &snapshot,
        &plan,
        &evidence,
    ))
    .expect("step 1");
}

/// The public file is listed beside the private one, as a listing made after step 2
/// shows them.
fn list_the_public_file(world: &World) {
    place(
        &world.fake,
        Visibility::Public,
        PUBLIC,
        raw_document(finding_entity(), &text_of(&finding_entity())),
    );
}

fn step_two(world: &World) {
    let (_, evidence, plan) = planned(world);
    block_on(create_public_file(&world.fake, &plan, &evidence)).expect("step 2");
    list_the_public_file(world);
}

fn step_three(world: &World) {
    let (_, _, plan) = planned(world);
    block_on(write_published(
        &world.fake,
        world.config.gate_policy(),
        &plan,
        &judge("ada"),
    ))
    .expect("step 3");
}

// --- a whole publication ----------------------------------------------------------

#[test]
fn a_publication_copies_creates_commits_and_removes_in_that_order() {
    let world = private_finding();
    let private_bytes = text_of(&finding_entity()).into_bytes();
    let published = done(publish_run(&world, Some(PUBLIC)));

    assert_eq!(published.doc(), docid(FINDING));
    assert_eq!(published.destination(), &rel(PUBLIC));
    assert_eq!(published.records(), 1);
    assert_eq!(
        published.steps(),
        [
            PublicationStep::CopyRecordsAndObjects,
            PublicationStep::CreatePublicFile,
            PublicationStep::WritePublished,
            PublicationStep::RemovePrivateFile,
        ]
    );
    assert_eq!(
        end(&world),
        End {
            public_file: Some(private_bytes),
            private_file: None,
            copied: 1,
            published: 1,
            object_in_public_store: true,
        }
    );
    // The private side keeps what it had: a publication only adds to the public one.
    assert_eq!(world.records(Visibility::Private), 1);
}

#[test]
fn a_new_publication_asks_once_about_what_cannot_be_taken_back() {
    let world = private_finding();
    done(publish_run(&world, Some(PUBLIC)));
    let asked = world.fake.asked();
    assert_eq!(asked.len(), 1);
    assert!(matches!(asked[0].digest(), ConfirmDigest::Plan(_)));
    assert_eq!(asked[0].summary().action(), ConfirmAction::Publish);
    assert_eq!(asked[0].summary().count(), 1);
}

#[test]
fn an_item_stands_for_its_design() {
    let world = World::new();
    world.add(
        Visibility::Private,
        "design.md",
        Entity::Design(design(
            DESIGN,
            111,
            vec![(1, identified(131, "the item", &[]))],
        )),
    );
    let published = done(block_on(publish(
        &world.fake,
        &world.config,
        Publish {
            doc: item_one(),
            to: Some(rel("docs/design.md")),
        },
    )));
    assert_eq!(published.doc(), docid(DESIGN));
}

// --- stopped, and run again ------------------------------------------------------

#[test]
fn a_run_that_stopped_after_the_records_were_copied_reaches_the_same_end() {
    let world = private_finding();
    step_one(&world);
    // Nothing is public yet but the inert copies.
    assert_eq!(end(&world).public_file, None);
    assert_eq!(end(&world).published, 0);

    let published = done(publish_run(&world, Some(PUBLIC)));
    assert_eq!(published.steps().len(), 4);
    assert_eq!(end(&world), the_end());
}

#[test]
fn copying_again_adds_nothing() {
    let world = private_finding();
    step_one(&world);
    let once = end(&world);
    let records = world.records(Visibility::Public);
    step_one(&world);
    assert_eq!(end(&world), once);
    assert_eq!(world.records(Visibility::Public), records);
}

#[test]
fn a_run_that_stopped_after_the_public_file_was_made_reaches_the_same_end() {
    let world = private_finding();
    step_one(&world);
    step_two(&world);

    // Without saying where: the file that is there says.
    let published = done(publish_run(&world, None));
    assert_eq!(
        published.steps(),
        [
            PublicationStep::CopyRecordsAndObjects,
            PublicationStep::WritePublished,
            PublicationStep::RemovePrivateFile,
        ]
    );
    assert_eq!(end(&world), the_end());
}

#[test]
fn a_run_that_stopped_after_the_commit_point_only_removes_the_copy_and_asks_nobody() {
    let world = private_finding();
    step_one(&world);
    step_two(&world);
    step_three(&world);
    assert_eq!(end(&world).published, 1);

    let published = done(publish_run(&world, Some(PUBLIC)));
    assert_eq!(published.steps(), [PublicationStep::RemovePrivateFile]);
    // The copy is the public file byte for byte: nothing is lost with it.
    assert!(world.fake.asked().is_empty());
    assert_eq!(end(&world), the_end());
}

#[test]
fn a_private_copy_that_differs_is_not_removed_without_a_confirmation() {
    let world = private_finding();
    step_one(&world);
    step_two(&world);
    step_three(&world);
    // The private copy was edited after the publication.
    world.remove(Visibility::Private, PRIVATE);
    world.add_text(
        Visibility::Private,
        PRIVATE,
        finding_entity(),
        "an edit nobody published\n",
    );

    world.fake.set_reply(Reply::Always(Confirmed::No));
    assert_eq!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::ConfirmDeclined
    );
    assert_eq!(
        world.fake.file(Visibility::Private, &rel(PRIVATE)),
        Some(b"an edit nobody published\n".to_vec())
    );
    let asked = world.fake.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].summary().action(),
        ConfirmAction::RemoveDifferingCopy
    );

    world.fake.set_reply(Reply::Always(Confirmed::Yes));
    done(publish_run(&world, Some(PUBLIC)));
    assert_eq!(world.fake.file(Visibility::Private, &rel(PRIVATE)), None);
}

#[test]
fn a_port_that_fails_part_way_is_an_error_and_a_second_run_finishes() {
    let world = private_finding();
    world.fake.fail(PortName::Files);
    let first = publish_run(&world, Some(PUBLIC));
    assert_eq!(exit_class(&first), ExitClass::Port);
    // The records and the text were copied; the file and the commit point were not.
    let stopped = end(&world);
    assert_eq!((stopped.copied, stopped.published), (1, 0));
    assert_eq!(stopped.public_file, None);

    world.fake.heal(PortName::Files);
    done(publish_run(&world, Some(PUBLIC)));
    assert_eq!(end(&world), the_end());
}

// --- what stops a publication -----------------------------------------------------

#[test]
fn a_person_who_does_not_confirm_stops_it_before_anything_is_copied() {
    for (answer, refusal) in [
        (Confirmed::No, Refusal::ConfirmDeclined),
        (Confirmed::Unavailable, Refusal::NeedsHumanConfirm),
    ] {
        let world = private_finding();
        let before = world.fake.writes();
        world.fake.set_reply(Reply::Always(answer));
        assert_eq!(refusal_of(publish_run(&world, Some(PUBLIC))), refusal);
        assert_eq!(world.fake.writes(), before);
    }
}

#[test]
fn an_agent_is_refused_before_it_is_asked() {
    let world = private_finding();
    world.fake.set_actor(agent("bot", "tool"));
    assert_eq!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::AgentCannotConfirm
    );
    assert!(world.fake.asked().is_empty());
}

#[test]
fn a_plan_that_changed_after_the_question_is_not_the_one_confirmed() {
    let world = private_finding();
    let before = world.fake.writes();
    world.fake.on_next_ask(edited(
        Visibility::Private,
        PRIVATE,
        finding_entity(),
        "edited after the question\n",
    ));
    assert!(matches!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::ConfirmationStale { .. }
    ));
    assert_eq!(world.fake.writes(), before);
}

#[test]
fn a_leak_stops_the_plan_and_nothing_is_asked() {
    let world = private_finding();
    // Another private document has the title this one's text names.
    world.add(
        Visibility::Private,
        "other.md",
        Entity::Finding(finding(FINDING + 1, "other")),
    );
    let before = world.fake.writes();
    assert!(matches!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::Publication(PublicationError::Leaks { .. })
    ));
    assert!(world.fake.asked().is_empty());
    assert_eq!(world.fake.writes(), before);
}

#[test]
fn a_private_root_that_cannot_be_read_stops_the_leak_check() {
    let world = private_finding();
    world.fake.set_private_root(PrivateRoot::Absent);
    assert_eq!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::LeakUncheckable
    );
}

#[test]
fn a_file_at_the_destination_stops_it() {
    let world = private_finding();
    world.add(
        Visibility::Public,
        PUBLIC,
        Entity::Finding(finding(FINDING + 5, "elsewhere")),
    );
    assert_eq!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::PublishTargetExists(rel(PUBLIC))
    );
}

#[test]
fn a_public_document_is_not_published_and_a_new_one_needs_a_destination() {
    let world = World::new();
    world.add(Visibility::Public, "public.md", finding_entity());
    assert!(matches!(
        refusal_of(publish_run(&world, Some(PUBLIC))),
        Refusal::Publication(PublicationError::AlreadyPublic { .. })
    ));

    let world = private_finding();
    assert_eq!(
        refusal_of(publish_run(&world, None)),
        Refusal::Publication(PublicationError::NoneOpen {
            doc: docid(FINDING)
        })
    );
}

#[test]
fn a_name_that_matches_nothing_is_not_found() {
    let world = private_finding();
    let run = block_on(publish(
        &world.fake,
        &world.config,
        Publish {
            doc: by_id(999),
            to: Some(rel(PUBLIC)),
        },
    ));
    assert_eq!(exit_class(&run), ExitClass::NotFound);
}

// --- abort ------------------------------------------------------------------------

/// A publication that stopped after the file was made: both files, the same bytes,
/// nothing recorded.
fn copied_file() -> World {
    let world = World::new();
    world.add(Visibility::Private, PRIVATE, finding_entity());
    list_the_public_file(&world);
    world
}

#[test]
fn an_abort_removes_the_public_file_and_writes_no_record() {
    let world = copied_file();
    let records = world.records(Visibility::Public);
    let aborted = done(abort_run(&world));
    assert_eq!(aborted.doc(), docid(FINDING));
    assert_eq!(aborted.removed(), &rel(PUBLIC));
    assert_eq!(world.fake.file(Visibility::Public, &rel(PUBLIC)), None);
    assert!(
        world
            .fake
            .file(Visibility::Private, &rel(PRIVATE))
            .is_some()
    );
    assert_eq!(world.records(Visibility::Public), records);
    // The copy is the file it was copied from: there is nothing to confirm.
    assert!(world.fake.asked().is_empty());
}

#[test]
fn a_publication_that_stopped_is_finished_where_its_public_file_is() {
    // The state an abort undoes is finished by a publish with no destination.
    let world = copied_file();
    let published = done(publish_run(&world, None));
    assert_eq!(published.destination(), &rel(PUBLIC));
    assert_eq!(world.fake.file(Visibility::Private, &rel(PRIVATE)), None);
}

#[test]
fn there_is_nothing_to_abort_when_none_is_open_or_it_is_recorded() {
    let world = private_finding();
    assert_eq!(
        refusal_of(abort_run(&world)),
        Refusal::Publication(PublicationError::NoneOpen {
            doc: docid(FINDING)
        })
    );

    // Once the commit point is written the only way on is to finish.
    let world = private_finding();
    step_one(&world);
    step_two(&world);
    step_three(&world);
    assert_eq!(
        refusal_of(abort_run(&world)),
        Refusal::Publication(PublicationError::AlreadyRecorded {
            doc: docid(FINDING)
        })
    );
    assert!(world.fake.file(Visibility::Public, &rel(PUBLIC)).is_some());
}

#[test]
fn an_abort_of_a_file_changed_under_it_removes_nothing() {
    let world = copied_file();
    world
        .fake
        .race_on_next_write(Visibility::Public, &rel(PUBLIC), b"someone else\n");
    assert_eq!(
        refusal_of(abort_run(&world)),
        Refusal::ChangedUnderfoot(rel(PUBLIC))
    );
    assert_eq!(
        world.fake.file(Visibility::Public, &rel(PUBLIC)),
        Some(b"someone else\n".to_vec())
    );
}

#[test]
fn an_abort_that_a_person_is_asked_about_asks_about_the_abort() {
    // The plan of an abort asks nobody today (the files it pairs are equal); the
    // question, when it is asked, is about the abort and nothing else.
    let world = copied_file();
    let snapshot =
        block_on(Snapshot::read(&world.fake, world.config.gate_policy())).expect("a snapshot");
    let (plan, _) =
        block_on(abort_plan_now(&world.fake, &snapshot, docid(FINDING))).expect("a plan");
    assert_eq!(plan.confirmation(), None);
    assert!(ConfirmPrompt::of_abort(&plan).is_none());
}

#[test]
fn a_port_that_fails_while_aborting_is_an_error() {
    let world = copied_file();
    world.fake.fail(PortName::Files);
    let run = abort_run(&world);
    assert_eq!(exit_class(&run), ExitClass::Port);
}
