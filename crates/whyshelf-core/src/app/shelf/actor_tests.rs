//! Tests of who a judgement is made by, and who ran the command: a name in the
//! request wins over the one the actor port names (which the adapter takes from
//! `WHYSHELF_ACTOR`, else Git's `user.name`, and `WHYSHELF_RAN_BY`; that order is
//! tested beside the adapter, in `whyshelf-infra/src/shelf/system.rs`), and the port
//! is asked once for the names a request leaves out.

use crate::domain::actor::{Actor, Attribution};
use crate::domain::shelf::ledger::Record;
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, block_on, fr};
use super::{AttributionAsk, JudgeRequest, Judgement, ShelfApp};

fn name(text: &str) -> Actor {
    Actor::new(text).unwrap()
}

fn deny_of(reference: &str, by: AttributionAsk) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: reference.parse::<Reference>().unwrap(),
        by,
    }
}

fn deny_as(actor: Option<&str>) -> JudgeRequest {
    deny_of(
        "FR-1",
        AttributionAsk {
            actor: actor.map(name),
            ran_by: None,
        },
    )
}

fn attribution_of(record: &Record) -> &Attribution {
    match record {
        Record::Decision(d) => d.attribution(),
        other => panic!("a decision, got {other:?}"),
    }
}

fn actor_of(record: &Record) -> &str {
    attribution_of(record).actor().as_str()
}

/// Two findings no one judged, the actor `alice` and, when `ran_by` is given, that
/// name as who the port says runs the command.
fn app(ran_by: Option<&str>) -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[]), fr(2, "fast", &[])]);
    if let Some(ran_by) = ran_by {
        fake.run_by(ran_by);
    }
    ShelfApp::new(fake)
}

/// The actor and who ran the command of the one record `request` writes.
fn written_by(app: &mut ShelfApp<FakeShelf>, request: JudgeRequest) -> (String, Option<String>) {
    let judged = block_on(app.judge(request)).unwrap().judged.remove(0);
    let by = attribution_of(&judged.record);
    (by.actor().to_string(), by.ran_by().map(ToString::to_string))
}

#[test]
fn an_actor_named_in_the_request_wins_over_the_ports() {
    // The fake's port names `alice`.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let judged = block_on(app.judge(deny_as(Some("bob"))))
        .unwrap()
        .judged
        .remove(0);
    assert_eq!(actor_of(&judged.record), "bob");
}

#[test]
fn with_no_actor_in_the_request_the_ports_is_used() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let judged = block_on(app.judge(deny_as(None))).unwrap().judged.remove(0);
    assert_eq!(actor_of(&judged.record), "alice");
}

#[test]
fn an_actor_named_in_the_request_does_not_ask_the_port() {
    // A port that names no one would fail the judgement if it were asked.
    let mut fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    fake.actor = None;
    let mut app = ShelfApp::new(fake);
    assert!(block_on(app.judge(deny_as(None))).is_err());
    let judged = block_on(app.judge(deny_as(Some("bob"))))
        .unwrap()
        .judged
        .remove(0);
    assert_eq!(actor_of(&judged.record), "bob");
}

#[test]
fn a_ran_by_named_in_the_request_wins_over_the_ports() {
    let mut app = app(Some("bot"));
    let by = AttributionAsk {
        actor: None,
        ran_by: Some(name("ci")),
    };
    assert_eq!(
        written_by(&mut app, deny_of("FR-1", by)),
        ("alice".to_owned(), Some("ci".to_owned()))
    );
}

#[test]
fn with_no_ran_by_in_the_request_the_ports_is_used() {
    let mut app = app(Some("bot"));
    assert_eq!(
        written_by(&mut app, deny_as(None)),
        ("alice".to_owned(), Some("bot".to_owned()))
    );
}

#[test]
fn no_one_ran_it_when_neither_the_request_nor_the_port_names_one() {
    let mut app = app(None);
    assert_eq!(
        written_by(&mut app, deny_as(None)),
        ("alice".to_owned(), None)
    );
}

#[test]
fn a_ran_by_that_is_the_actor_is_one_name_whoever_gave_either() {
    // Both in the request.
    let mut app = app(None);
    let by = AttributionAsk {
        actor: Some(name("bob")),
        ran_by: Some(name("bob")),
    };
    assert_eq!(
        written_by(&mut app, deny_of("FR-1", by)),
        ("bob".to_owned(), None)
    );
    // The request's ran_by is the port's actor.
    let by = AttributionAsk {
        actor: None,
        ran_by: Some(name("alice")),
    };
    assert_eq!(
        written_by(&mut app, deny_of("FR-2", by)),
        ("alice".to_owned(), None)
    );
    // The port's ran_by is the request's actor.
    let mut app = self::app(Some("bob"));
    assert_eq!(
        written_by(&mut app, deny_as(Some("bob"))),
        ("bob".to_owned(), None)
    );
}

#[test]
fn names_the_request_gives_do_not_ask_the_port() {
    let mut app = app(Some("bot"));
    let by = AttributionAsk {
        actor: Some(name("bob")),
        ran_by: Some(name("ci")),
    };
    block_on(app.judge(deny_of("FR-1", by))).unwrap();
    assert_eq!(app.ports().names_asked(), (0, 0));
}

#[test]
fn the_port_is_asked_once_for_a_run_of_several_judgements() {
    let mut app = app(Some("bot"));
    let judged = block_on(app.judge_all(vec![
        deny_of("FR-1", AttributionAsk::default()),
        deny_of("FR-2", AttributionAsk::default()),
    ]))
    .unwrap()
    .judged;
    assert_eq!(app.ports().names_asked(), (1, 1));
    for one in &judged {
        assert_eq!(attribution_of(&one.record).ran_by(), Some(&name("bot")));
    }
}

#[test]
fn the_port_is_asked_once_for_each_name_when_the_requests_ask_otherwise() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        fr(2, "fast", &[]),
        fr(3, "late", &[]),
    ]);
    fake.run_by("bot");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![
        deny_of("FR-1", AttributionAsk::default()),
        deny_of(
            "FR-2",
            AttributionAsk {
                actor: Some(name("bob")),
                ran_by: None,
            },
        ),
        deny_of(
            "FR-3",
            AttributionAsk {
                actor: None,
                ran_by: Some(name("ci")),
            },
        ),
    ]))
    .unwrap();
    assert_eq!(app.ports().names_asked(), (1, 1));
}
