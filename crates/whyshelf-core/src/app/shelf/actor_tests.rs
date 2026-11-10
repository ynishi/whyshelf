//! Tests of who a judgement is made by: an actor named in the request wins over the
//! one the actor port names (which the adapter takes from `WHYSHELF_ACTOR`, else
//! Git's `user.name`; that order is tested beside the adapter, in
//! `whyshelf-infra/src/shelf/system.rs`).

use crate::domain::actor::Actor;
use crate::domain::shelf::ledger::Record;
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, block_on, fr};
use super::{JudgeRequest, Judgement, ShelfApp};

fn deny_as(actor: Option<&str>) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: "FR-1".parse::<Reference>().unwrap(),
        actor: actor.map(|a| Actor::new(a).unwrap()),
    }
}

fn actor_of(record: &Record) -> &str {
    match record {
        Record::Decision(d) => d.actor().as_str(),
        other => panic!("a decision, got {other:?}"),
    }
}

#[test]
fn an_actor_named_in_the_request_wins_over_the_ports() {
    // The fake's port names `alice`.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let judged = block_on(app.judge(deny_as(Some("bob")))).unwrap();
    assert_eq!(actor_of(&judged.record), "bob");
}

#[test]
fn with_no_actor_in_the_request_the_ports_is_used() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let judged = block_on(app.judge(deny_as(None))).unwrap();
    assert_eq!(actor_of(&judged.record), "alice");
}

#[test]
fn an_actor_named_in_the_request_does_not_ask_the_port() {
    // A port that names no one would fail the judgement if it were asked.
    let mut fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    fake.actor = None;
    let mut app = ShelfApp::new(fake);
    assert!(block_on(app.judge(deny_as(None))).is_err());
    let judged = block_on(app.judge(deny_as(Some("bob")))).unwrap();
    assert_eq!(actor_of(&judged.record), "bob");
}
