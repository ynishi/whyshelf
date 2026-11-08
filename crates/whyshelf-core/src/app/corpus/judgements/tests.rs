//! Revoking, retiring and purging through the fake ports.

use super::*;
use crate::app::corpus::fixtures::{DESIGN, FINDING, ITEM, World, by_id, item_one};
use crate::app::corpus::{GateCommand, Snapshot};
use crate::app::fake::block_on;
use crate::app::outcome::{ExitClass, Refusal, exit_class};
use crate::app::ports::PortName;
use crate::domain::corpus::Entity;
use crate::domain::error::{JudgementError, RevokeError};
use crate::domain::finding::test_support::finding;
use crate::domain::id::test_support::{docid, iid};
use crate::domain::judge::test_support::judge;
use crate::domain::judgement::test_support::{jid, why};

type Run<T> = Result<Outcome<T>, AppError>;

fn revoke_run(world: &World, by: RevokeBy, force: bool) -> Run<Revoked> {
    block_on(revoke(
        &world.fake,
        &world.config,
        Revoke {
            by,
            reason: why(),
            force,
        },
    ))
}

fn retire_run(world: &World, target: Target) -> Run<Retired> {
    block_on(retire(
        &world.fake,
        &world.config,
        Retire {
            target,
            reason: why(),
        },
    ))
}

fn purge_run(world: &World, doc: DocId, text: TextHash) -> Run<Purged> {
    block_on(purge(
        &world.fake,
        &world.config,
        Purge {
            doc,
            text,
            reason: why(),
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

fn snapshot(world: &World) -> Snapshot {
    block_on(Snapshot::read(&world.fake, world.config.gate_policy())).expect("a snapshot")
}

fn one(record: JudgementId) -> RevokeBy {
    RevokeBy::Records(NonEmptyVec::new(record, Vec::new()))
}

// --- revoke -----------------------------------------------------------------------

#[test]
fn a_revocation_takes_the_named_record_back() {
    let world = World::project(Visibility::Public);
    let approved = world.approved(GateCommand::Cr, by_id(FINDING));
    let revoked = done(revoke_run(&world, one(approved.id()), false));
    assert_eq!(revoked.records(), [approved.id()]);
    assert_eq!(revoked.ledger(), Visibility::Public);
    assert_eq!(world.records(Visibility::Public), 2);
    assert!(snapshot(&world).ledgers().is_revoked(&approved.id()));
}

#[test]
fn a_revocation_is_written_where_what_it_names_is_written() {
    let world = World::project(Visibility::Private);
    let approved = world.approved(GateCommand::Cr, by_id(FINDING));
    let revoked = done(revoke_run(&world, one(approved.id()), false));
    assert_eq!(revoked.ledger(), Visibility::Private);
    assert_eq!(world.records(Visibility::Public), 0);
}

#[test]
fn another_judges_record_is_revoked_only_when_forced() {
    let world = World::project(Visibility::Public);
    let approved = world.approved(GateCommand::Cr, by_id(FINDING));
    world.fake.set_actor(judge("grace"));
    assert_eq!(
        refusal_of(revoke_run(&world, one(approved.id()), false)),
        Refusal::RevokeOthers {
            owner: judge("ada")
        }
    );
    assert_eq!(world.records(Visibility::Public), 1);
    done(revoke_run(&world, one(approved.id()), true));
    assert_eq!(world.records(Visibility::Public), 2);
}

#[test]
fn what_cannot_be_revoked_is_refused_and_nothing_is_written() {
    let world = World::project(Visibility::Public);
    let approved = world.approved(GateCommand::Cr, by_id(FINDING));
    let written = world.fake.writes();

    // A record that is in neither ledger.
    let unknown = jid(900);
    assert_eq!(
        refusal_of(revoke_run(&world, one(unknown), false)),
        Refusal::Judgement(JudgementError::UnknownRecord { record: unknown })
    );

    // A revocation cannot be revoked.
    let revoked = done(revoke_run(&world, one(approved.id()), false));
    assert_eq!(
        refusal_of(revoke_run(&world, one(revoked.id()), false)),
        Refusal::NotRevocable(revoked.id())
    );

    // A record revoked already.
    assert_eq!(
        refusal_of(revoke_run(&world, one(approved.id()), false)),
        Refusal::Revoke(RevokeError::AlreadyRevoked {
            record: approved.id()
        })
    );
    // Only the one revocation that went through was written.
    assert_eq!(world.fake.writes(), written + 1);
}

#[test]
fn a_retirement_is_taken_back_by_the_target() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    world.remove(Visibility::Public, "finding.md");
    let retired = done(retire_run(&world, Target::Doc(docid(FINDING))));

    let revoked = done(revoke_run(
        &world,
        RevokeBy::Retirement(Target::Doc(docid(FINDING))),
        false,
    ));
    assert_eq!(revoked.records(), [retired.id()]);
    // Nothing is left to take back.
    assert_eq!(
        refusal_of(revoke_run(
            &world,
            RevokeBy::Retirement(Target::Doc(docid(FINDING))),
            false
        )),
        Refusal::Revoke(RevokeError::NothingToRevoke)
    );
}

// --- retire -----------------------------------------------------------------------

#[test]
fn something_whose_file_is_gone_is_retired() {
    let world = World::project(Visibility::Public);
    world.approved_to_ci();
    world.approved(World::ca(), item_one());
    world.remove(Visibility::Public, "finding.md");
    world.remove(Visibility::Public, "design.md");

    let retired = done(retire_run(&world, Target::Doc(docid(FINDING))));
    assert_eq!(retired.target(), Target::Doc(docid(FINDING)));
    assert_eq!(retired.ledger(), Visibility::Public);

    let item = done(retire_run(&world, Target::Item(iid(ITEM))));
    assert_eq!(item.target(), Target::Item(iid(ITEM)));
    let snapshot = snapshot(&world);
    assert_eq!(
        snapshot
            .evaluation()
            .presence_of(&Target::Doc(docid(FINDING))),
        Some(crate::domain::state::Presence::Retired)
    );
}

#[test]
fn something_whose_file_is_there_is_not_retired() {
    let world = World::project(Visibility::Public);
    world.approved(GateCommand::Cr, by_id(FINDING));
    assert_eq!(
        refusal_of(retire_run(&world, Target::Doc(docid(FINDING)))),
        Refusal::Judgement(JudgementError::TargetPresent {
            target: Target::Doc(docid(FINDING))
        })
    );
    assert_eq!(world.records(Visibility::Public), 1);
}

#[test]
fn something_nothing_knows_of_cannot_be_retired() {
    let world = World::project(Visibility::Public);
    let target = Target::Doc(docid(999));
    assert_eq!(
        refusal_of(retire_run(&world, target)),
        Refusal::Judgement(JudgementError::UnknownTarget { target })
    );
}

#[test]
fn a_document_whose_publication_is_cut_short_is_not_retired_or_purged() {
    let world = World::new();
    let entity = Entity::Finding(finding(FINDING, "body"));
    world.add(Visibility::Public, "public.md", entity.clone());
    world.add(Visibility::Private, "private.md", entity);
    let doc = docid(FINDING);
    assert_eq!(
        refusal_of(retire_run(&world, Target::Doc(doc))),
        Refusal::PublishInProgress(doc)
    );
    assert_eq!(
        refusal_of(purge_run(&world, doc, TextHash::of("x"))),
        Refusal::PublishInProgress(doc)
    );
    assert_eq!(world.fake.writes(), 0);
}

// --- purge ------------------------------------------------------------------------

#[test]
fn a_purge_deletes_the_text_and_says_so_in_the_ledger_of_the_document() {
    let world = World::project(Visibility::Public);
    let text = TextHash::of(&format!(
        "{:?}\n",
        Entity::Finding(finding(FINDING, "body"))
    ));
    world.approved(GateCommand::Cr, by_id(FINDING));
    assert!(world.fake.has_object(Visibility::Public, text));

    let purged = done(purge_run(&world, docid(FINDING), text));
    assert_eq!(purged.text(), text);
    assert_eq!(purged.ledger(), Visibility::Public);
    assert!(!world.fake.has_object(Visibility::Public, text));
    assert!(snapshot(&world).ledgers().is_purged(&text));
}

#[test]
fn a_purge_of_a_private_document_is_a_private_record() {
    let world = World::project(Visibility::Private);
    let text = TextHash::of(&format!(
        "{:?}\n",
        Entity::Finding(finding(FINDING, "body"))
    ));
    world.approved(GateCommand::Cr, by_id(FINDING));
    let purged = done(purge_run(&world, docid(FINDING), text));
    assert_eq!(purged.ledger(), Visibility::Private);
    assert_eq!(world.records(Visibility::Public), 0);
}

#[test]
fn a_purge_of_a_document_nothing_knows_of_is_refused() {
    let world = World::project(Visibility::Public);
    let target = Target::Doc(docid(DESIGN + 100));
    assert_eq!(
        refusal_of(purge_run(&world, docid(DESIGN + 100), TextHash::of("x"))),
        Refusal::Judgement(JudgementError::UnknownTarget { target })
    );
}

#[test]
fn a_port_that_fails_is_an_error_naming_the_port() {
    let world = World::project(Visibility::Public);
    world.fake.fail(PortName::Ledger);
    let run = purge_run(&world, docid(FINDING), TextHash::of("x"));
    assert_eq!(exit_class(&run), ExitClass::Port);
}
