//! What the use cases that write have in common.

use super::*;
use crate::app::commit::confirm::confirm;
use crate::app::commit::fixtures::{Aborting, adoptable_finding};
use crate::app::corpus::fixtures::{DESIGN, FINDING, World, rel};
use crate::app::corpus::snapshot::Snapshot;
use crate::app::fake::block_on;
use crate::app::outcome::Outcome;
use crate::app::ports::{ConfirmPrompt, Confirmed};
use crate::domain::adoption;
use crate::domain::corpus::Visibility;
use crate::domain::finding::test_support::finding;
use crate::domain::id::test_support::docid;
use crate::domain::judge::test_support::{agent, judge};
use crate::domain::judgement::test_support::{jid, ts, why};
use crate::domain::judgement::{FilePresence, Target};

fn read(world: &World) -> Snapshot {
    block_on(Snapshot::read(&world.fake, world.config.gate_policy())).expect("a snapshot")
}

fn confirmed() -> HumanConfirmation {
    let prompt = ConfirmPrompt::of_adoption(&adoptable_finding()).expect("adoption asks");
    block_on(confirm(&World::new().fake, &judge("ada"), &prompt)).expect("a yes")
}

fn adopt_digest() -> ConfirmDigest {
    ConfirmDigest::Adopt(adoption::digest(&adoptable_finding()))
}

#[test]
fn a_confirmation_holds_for_the_same_plan_and_for_none_that_needs_it_no_more() {
    let confirmation = confirmed();
    assert_eq!(
        still_holds(Some(&confirmation), Some(adopt_digest())),
        Ok(())
    );
    // The plan no longer has anything that cannot be taken back.
    assert_eq!(still_holds(Some(&confirmation), None), Ok(()));
    // No question was asked and none is needed.
    assert_eq!(still_holds(None, None), Ok(()));
}

#[test]
fn a_confirmation_does_not_hold_for_another_plan() {
    let confirmation = confirmed();
    let other = ConfirmDigest::Abort(Aborting::of(b"x").plan.digest());
    assert_eq!(
        still_holds(Some(&confirmation), Some(other)),
        Err(Refusal::ConfirmationStale {
            confirmed: adopt_digest(),
            now: other,
        })
    );
}

#[test]
fn a_plan_that_came_to_need_a_person_after_none_was_asked_is_refused() {
    assert_eq!(
        still_holds(None, Some(adopt_digest())),
        Err(Refusal::NeedsHumanConfirm)
    );
}

#[test]
fn a_person_is_asked_only_when_the_plan_gave_a_prompt() {
    let world = World::new();
    let by = judge("ada");
    assert_eq!(block_on(ask(&world.fake, &by, None)).ok(), Some(None));
    assert!(world.fake.asked().is_empty());

    let prompt = ConfirmPrompt::of_adoption(&adoptable_finding());
    let asked = block_on(ask(&world.fake, &by, prompt)).ok();
    assert!(matches!(asked, Some(Some(_))));
    assert_eq!(world.fake.asked().len(), 1);
}

#[test]
fn an_agent_is_refused_before_it_is_asked() {
    let world = World::new();
    let prompt = ConfirmPrompt::of_adoption(&adoptable_finding());
    let stopped = block_on(ask(&world.fake, &agent("bot", "tool"), prompt));
    assert!(matches!(
        stopped,
        Err(Flow::Stopped(stopped)) if *stopped == Outcome::Refused(Refusal::AgentCannotConfirm)
    ));
    assert!(world.fake.asked().is_empty());
}

#[test]
fn a_person_who_does_not_confirm_stops_the_run() {
    for (answer, refusal) in [
        (Confirmed::No, Refusal::ConfirmDeclined),
        (Confirmed::Unavailable, Refusal::NeedsHumanConfirm),
    ] {
        let world = World::new();
        world
            .fake
            .set_reply(crate::app::fake::Reply::Always(answer));
        let prompt = ConfirmPrompt::of_adoption(&adoptable_finding());
        let stopped = block_on(ask(&world.fake, &judge("ada"), prompt));
        assert!(matches!(
            stopped,
            Err(Flow::Stopped(stopped)) if *stopped == Outcome::Refused(refusal.clone())
        ));
    }
}

#[test]
fn a_document_whose_publication_is_cut_short_takes_no_new_judgement() {
    let world = World::new();
    let entity = crate::domain::corpus::Entity::Finding(finding(FINDING, "body"));
    world.add(Visibility::Public, "public.md", entity.clone());
    world.add(Visibility::Private, "private.md", entity);
    let snapshot = read(&world);
    assert_eq!(
        not_publishing(&snapshot, docid(FINDING)),
        Err(Refusal::PublishInProgress(docid(FINDING)))
    );
    // Another document is not held back.
    assert_eq!(not_publishing(&snapshot, docid(DESIGN)), Ok(()));
}

#[test]
fn a_file_that_went_after_the_listing_is_a_file_that_changed_under_the_run() {
    let world = World::project(Visibility::Public);
    let snapshot = read(&world);
    let file = snapshot.corpus().by_id(&docid(FINDING))[0].facts().clone();
    assert!(block_on(read_bytes(&world.fake, &file)).is_ok());

    world.fake.lose_file(Visibility::Public, &rel("finding.md"));
    assert!(matches!(
        block_on(read_bytes(&world.fake, &file)),
        Err(Flow::Stopped(stopped))
            if *stopped == Outcome::Refused(Refusal::ChangedUnderfoot(rel("finding.md")))
    ));
}

#[test]
fn a_write_that_did_not_write_is_the_refusal_it_stands_for() {
    let at = rel("a.md");
    assert_eq!(written(Write::Done, &at), Ok(()));
    assert_eq!(written(Write::Exists, &at), Ok(()));
    assert_eq!(
        written(Write::ChangedUnderfoot, &at),
        Err(Refusal::ChangedUnderfoot(at.clone()))
    );
    assert_eq!(
        written(Write::CannotKeepOwner, &at),
        Err(Refusal::CannotKeepOwner(at))
    );
}

#[test]
fn who_when_and_which_id_come_from_the_ports() {
    let world = World::new();
    world.fake.set_actor(judge("grace"));
    let mint = block_on(Mint::take(&world.fake)).expect("a mint");
    assert_eq!(mint.judge, judge("grace"));
    let next = block_on(Mint::take(&world.fake)).expect("a mint");
    assert_ne!(mint.id, next.id);
    assert_ne!(mint.at, next.at);
}

#[test]
fn a_judgement_of_the_wrong_kind_for_the_chain_is_the_tools_mistake() {
    let world = World::project(Visibility::Public);
    let snapshot = read(&world);
    let retirement = Judgement::retire(
        jid(1),
        ts(),
        judge("ada"),
        Visibility::Public,
        Target::Doc(docid(FINDING)),
        FilePresence::Absent,
        why(),
    )
    .expect("a retirement");
    let stopped = block_on(record_with_object(
        &world.fake,
        &snapshot,
        retirement.clone(),
    ));
    assert!(matches!(
        stopped,
        Err(Flow::Failed(AppError::Internal(ChainError::NoObject)))
    ));

    let approval = crate::domain::judgement::test_support::approve(
        2,
        &judge("ada"),
        Visibility::Public,
        crate::domain::judgement::test_support::cr_passed(1, "v1"),
    );
    let stopped = block_on(record_plain(
        &world.fake,
        snapshot.corpus(),
        snapshot.ledgers(),
        approval,
    ));
    assert!(matches!(
        stopped,
        Err(Flow::Failed(AppError::Internal(ChainError::KeepsObject)))
    ));
    assert_eq!(world.fake.writes(), 0);
}
