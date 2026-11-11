//! Tests of `approve_chosen` with the pending rule, alone and with the stale one,
//! over the in-memory fake.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::{DocReading, RawValue};
use crate::domain::shelf::freshness::StandingKind;
use crate::domain::shelf::ledger::{DecisionVerb, FrameRefusal, JudgedAt, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::order::{DecidedKind, Hold, NotPending, Rule};

use super::fake_tests::{FakeShelf, block_on, docid, file_space, fr, path, reason, sp, sp_tested};
use super::{
    ApproveChosenRequest, Choose, ChosenApproved, ErrorClass, JudgeRequest, Judgement, LeftOutWhy,
    PendingScope, ShelfApp, ShelfError,
};

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: reference.parse::<Reference>().unwrap(),
        actor: None,
    }
}

fn upstream_of(references: &[&str]) -> PendingScope {
    let refs: Vec<Reference> = references.iter().map(|r| r.parse().unwrap()).collect();
    PendingScope::UpstreamOf(NonEmptyVec::from_vec(refs).unwrap())
}

// The error is the use case's own, read once by each test.
#[allow(clippy::result_large_err)]
fn run(app: &mut ShelfApp<FakeShelf>, choose: Choose) -> Result<ChosenApproved, ShelfError> {
    block_on(app.approve_chosen(ApproveChosenRequest {
        choose,
        reason: reason("reviewed"),
        actor: None,
        until: None,
    }))
}

fn judged(done: &ChosenApproved) -> Vec<(DocId, Rule)> {
    done.judged
        .iter()
        .map(|j| (j.judged.subject, j.rule))
        .collect()
}

fn held_of(done: &ChosenApproved, n: u128) -> (DocId, Hold) {
    match &done
        .left_out
        .iter()
        .find(|l| l.left.doc == docid(n))
        .unwrap_or_else(|| panic!("{n} is left out"))
        .left
        .why
    {
        LeftOutWhy::StandsOn { held, .. } => (held.upstream, held.hold),
        other => panic!("{n} is left out as standing on another, not {other:?}"),
    }
}

fn pending_count(app: &ShelfApp<FakeShelf>) -> usize {
    block_on(app.status())
        .unwrap()
        .rows
        .iter()
        .filter(|r| r.standing.kind() == StandingKind::Pending)
        .count()
}

/// FR-1, FR-2 on it, SP-3 grounded on FR-2: a chain, none decided.
fn chain() -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(vec![
        sp(3, "cache", &["FR-2"]),
        fr(2, "slow", &["FR-1"]),
        fr(1, "base", &[]),
    ]))
}

#[test]
fn every_pending_document_is_approved_upstream_first_a_withdrawn_one_too() {
    let mut app = chain();
    // FR-1 approved, then that approval withdrawn: pending again.
    block_on(app.judge(approve("FR-1"))).unwrap();
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Revoke {
            reason: reason("not yet"),
        },
        reference: "FR-1".parse().unwrap(),
        actor: None,
    }))
    .unwrap();
    assert_eq!(pending_count(&app), 3);

    let done = run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    assert_eq!(
        judged(&done),
        vec![
            (docid(1), Rule::Pending),
            (docid(2), Rule::Pending),
            (docid(3), Rule::Pending)
        ]
    );
    assert!(done.left_out.is_empty());
    assert_eq!(pending_count(&app), 0);
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
}

#[test]
fn a_reference_takes_itself_and_its_pending_upstream_only() {
    // SP-5 also stands on FR-1, and FR-4 is apart: neither is upstream of SP-3.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        sp(3, "cache", &["FR-2"]),
        fr(2, "slow", &["FR-1"]),
        fr(1, "base", &[]),
        fr(4, "apart", &[]),
        sp(5, "index", &["FR-1"]),
    ]));
    let done = run(&mut app, Choose::Pending(upstream_of(&["SP-3"]))).unwrap();
    assert_eq!(
        judged(&done).iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![docid(1), docid(2), docid(3)]
    );
    assert_eq!(pending_count(&app), 2);
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
}

#[test]
fn a_named_document_that_is_not_pending_is_an_error_of_the_request() {
    let mut app = chain();
    block_on(app.judge(approve("FR-1"))).unwrap();
    let before = app.ports().record_count();
    let err = run(&mut app, Choose::Pending(upstream_of(&["SP-3", "FR-1"]))).unwrap_err();
    let ShelfError::NotPending {
        reference,
        subject,
        why,
    } = &err
    else {
        panic!("not pending, not {err:?}");
    };
    assert_eq!(reference.to_string(), "FR-1");
    assert_eq!(*subject, docid(1));
    assert!(matches!(why, NotPending::Decided(DecidedKind::Approved(_))));
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().record_count(), before);
}

#[test]
fn a_named_document_that_is_not_valid_is_not_pending() {
    let mut entries = BTreeMap::new();
    entries.insert("id".to_owned(), RawValue::Text(docid(5).to_string()));
    entries.insert("key".to_owned(), RawValue::Text("FR-5".to_owned()));
    let untitled = DocReading::Parsed {
        origin: path("findings/fr-5.md"),
        entries,
        body: "no title\n".to_owned(),
    };
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "base", &[]), untitled]));
    let err = run(
        &mut app,
        Choose::Pending(upstream_of(&[&docid(5).to_string()])),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ShelfError::NotPending {
            why: NotPending::NotValid,
            ..
        }
    ));
    // And all of them takes only the valid one.
    let done = run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    assert_eq!(judged(&done), vec![(docid(1), Rule::Pending)]);
}

#[test]
fn a_broken_upstream_leaves_the_named_one_out_as_standing_on_it() {
    // FR-5 on FR-6, both approved; FR-6 then denied, so FR-5 is broken. SP-3 is
    // pending and grounded on FR-5.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(6, "base", &[]),
        fr(5, "slow", &["FR-6"]),
        sp(3, "index", &["FR-5"]),
    ]));
    block_on(app.judge_all(vec![approve("FR-6"), approve("FR-5")])).unwrap();
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: "FR-6".parse().unwrap(),
        actor: None,
    }))
    .unwrap();
    let before = app.ports().record_count();

    let done = run(&mut app, Choose::Pending(upstream_of(&["SP-3"]))).unwrap();
    assert!(done.judged.is_empty());
    assert_eq!(held_of(&done, 3), (docid(5), Hold::Broken));
    assert_eq!(app.ports().record_count(), before);
}

/// FR-1 approved and edited (stale); SP-2 pending, grounded on it.
fn pending_on_stale() -> ShelfApp<FakeShelf> {
    let mut app = ShelfApp::new(
        FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]),
    );
    block_on(app.judge(approve("FR-1"))).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    app
}

#[test]
fn a_stale_upstream_under_pending_alone_is_not_approved_and_holds_the_named_one_back() {
    let mut app = pending_on_stale();
    let before = app.ports().record_count();
    let done = run(&mut app, Choose::Pending(upstream_of(&["SP-2"]))).unwrap();
    assert!(done.judged.is_empty());
    assert_eq!(held_of(&done, 2), (docid(1), Hold::Stale));
    assert_eq!(app.ports().record_count(), before);
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 1);
}

#[test]
fn pending_and_stale_together_are_one_run_upstream_first() {
    let mut app = pending_on_stale();
    let before = app.ports().record_count();
    let done = run(&mut app, Choose::StaleAndPending(PendingScope::All)).unwrap();
    assert_eq!(
        judged(&done),
        vec![(docid(1), Rule::Stale), (docid(2), Rule::Pending)]
    );
    assert!(done.left_out.is_empty());
    let records = app.ports().records();
    let times: BTreeSet<JudgedAt> = records[before..]
        .iter()
        .map(|r| match r {
            Record::Decision(d) => d.at(),
            other => panic!("a decision, not {other:?}"),
        })
        .collect();
    assert_eq!(times.len(), 1);
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    assert_eq!(pending_count(&app), 0);
}

#[test]
fn a_refused_one_does_not_stop_the_rest() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
        fr(3, "apart", &[]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    fake.cannot_tell(&file_space(), "tests/a.rs", "offline");
    let mut app = ShelfApp::new(fake);
    let done = run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    assert_eq!(
        judged(&done).iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![docid(1), docid(3)]
    );
    assert_eq!(done.left_out.len(), 1);
    assert_eq!(done.left_out[0].rule, Rule::Pending);
    assert!(matches!(
        done.left_out[0].left.why,
        LeftOutWhy::Refused(FrameRefusal::ArtifactUnknown { .. })
    ));
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn the_approvals_of_one_run_share_one_time() {
    let mut app = chain();
    app.ports().set_clock_step(7);
    run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    let times: BTreeSet<JudgedAt> = app
        .ports()
        .records()
        .iter()
        .map(|r| match r {
            Record::Decision(d) => d.at(),
            other => panic!("a decision, not {other:?}"),
        })
        .collect();
    assert_eq!(app.ports().record_count(), 3);
    assert_eq!(times.len(), 1, "one time: {times:?}");
}

#[test]
fn until_and_actor_reach_every_record() {
    let mut app = chain();
    let done = block_on(app.approve_chosen(ApproveChosenRequest {
        choose: Choose::Pending(PendingScope::All),
        reason: reason("reviewed"),
        actor: Some(Actor::new("bob").unwrap()),
        until: Some(JudgedAt::new(9000)),
    }))
    .unwrap();
    assert_eq!(done.judged.len(), 3);
    for one in &done.judged {
        let Record::Decision(decision) = &one.judged.record else {
            panic!("an approval");
        };
        assert_eq!(decision.actor().as_str(), "bob");
        assert_eq!(decision.reason().map(|r| r.as_str()), Some("reviewed"));
        let DecisionVerb::Approve { expires, .. } = decision.verb() else {
            panic!("an approval");
        };
        assert_eq!(*expires, Some(JudgedAt::new(9000)));
    }
}

#[test]
fn an_upstream_stale_only_through_one_the_run_approves_holds_nothing_back() {
    // FR-2 is approved on FR-1 while FR-1 is pending, so it is stale at once; SP-3,
    // pending, stands on FR-2. Approving FR-1 in the run makes FR-2 fresh, so SP-3 is
    // approved after it.
    let mut app = chain();
    block_on(app.judge(approve("FR-2"))).unwrap();
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 1);

    let done = run(&mut app, Choose::Pending(upstream_of(&["SP-3"]))).unwrap();
    assert_eq!(
        judged(&done),
        vec![(docid(1), Rule::Pending), (docid(3), Rule::Pending)]
    );
    assert!(done.left_out.is_empty());
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    assert_eq!(pending_count(&app), 0);
}

#[test]
fn a_stale_document_on_a_pending_one_is_decided_after_it_in_one_run() {
    // FR-2 and FR-4 are approved on FR-1 while it is pending (stale at once); FR-4 is
    // then edited. One run of both rules approves FR-1, then FR-4 again; FR-2 would
    // only repeat its approval, which holds once FR-1 is approved.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "base", &[]),
        fr(2, "on it", &["FR-1"]),
        fr(4, "on it too", &["FR-1"]),
    ]));
    block_on(app.judge_all(vec![approve("FR-2"), approve("FR-4")])).unwrap();
    app.ports().edit(fr(4, "on it too, edited", &["FR-1"]), 4);
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 2);

    let done = run(&mut app, Choose::StaleAndPending(PendingScope::All)).unwrap();
    assert_eq!(
        judged(&done),
        vec![(docid(1), Rule::Pending), (docid(4), Rule::Stale)]
    );
    assert_eq!(done.left_out.len(), 1);
    assert_eq!(done.left_out[0].rule, Rule::Stale);
    assert_eq!(done.left_out[0].left.doc, docid(2));
    assert!(matches!(
        done.left_out[0].left.why,
        LeftOutWhy::Repeats { .. }
    ));
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
}

#[test]
fn a_superseded_pending_document_is_taken_with_its_replacement() {
    use crate::domain::shelf::schema::Propagation;
    // FR-3 supersedes FR-1; neither is decided. The ledger knows no superseded
    // standing, so both are pending and both are approved.
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "old", &[]),
        super::fake_tests::file(
            "findings/fr-3.md",
            3,
            "FR-3",
            "new",
            &[("supersedes", &["FR-1"])],
        ),
    ]);
    fake.decls.lock().unwrap()[0]
        .links
        .push(super::fake_tests::link(
            "supersedes",
            "finding",
            Propagation::Replaces,
        ));
    let mut app = ShelfApp::new(fake);
    let done = run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    assert_eq!(
        judged(&done).iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![docid(1), docid(3)]
    );
}

#[test]
fn nothing_pending_is_an_empty_run_that_writes_nothing() {
    let mut app = chain();
    run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    let before = app.ports().append_count();
    let done = run(&mut app, Choose::Pending(PendingScope::All)).unwrap();
    assert!(done.judged.is_empty());
    assert!(done.left_out.is_empty());
    assert_eq!(app.ports().append_count(), before);
}
