//! Tests of `approve_stale` over the in-memory fake.

use std::collections::BTreeMap;

use crate::domain::actor::Actor;
use crate::domain::shelf::build::{DocReading, RawValue};
use crate::domain::shelf::freshness::{Cause, Degradation, Freshness, Severity, Standing};
use crate::domain::shelf::ledger::{
    DecisionProblem, DecisionVerb, DeclaredStatus, FrameRefusal, JudgedAt, Record,
};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::order::Hold;

use super::fake_tests::{
    FakeShelf, block_on, docid, file, file_space, fr, path, reason, sp, sp_tested,
};
use super::{
    ApproveStaleRequest, ChosenApproved, ErrorClass, JudgeRequest, Judgement, LeftOutWhy, ShelfApp,
    ShelfError,
};

fn approve(reference: &str, until: Option<i64>) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: until.map(JudgedAt::new),
            reason: None,
            again: false,
        },
        reference: reference.parse::<Reference>().unwrap(),
        actor: None,
    }
}

// The error is the use case's own, read once by each test.
#[allow(clippy::result_large_err)]
fn again(app: &mut ShelfApp<FakeShelf>) -> Result<ChosenApproved, ShelfError> {
    block_on(app.approve_stale(ApproveStaleRequest {
        reason: reason("reviewed"),
        actor: None,
        until: None,
    }))
}

fn judged_ids(done: &ChosenApproved) -> Vec<crate::domain::id::DocId> {
    done.judged.iter().map(|j| j.judged.subject).collect()
}

fn why_of(done: &ChosenApproved, n: u128) -> LeftOutWhy {
    done.left_out
        .iter()
        .find(|l| l.left.doc == docid(n))
        .unwrap_or_else(|| panic!("{n} is left out"))
        .left
        .why
        .clone()
}

fn broken_of(done: &ChosenApproved, n: u128) -> Degradation {
    match why_of(done, n) {
        LeftOutWhy::Broken(degradation) => degradation,
        other => panic!("{n} is left out as broken, not {other:?}"),
    }
}

fn held_of(done: &ChosenApproved, n: u128) -> (crate::domain::id::DocId, Hold) {
    match why_of(done, n) {
        LeftOutWhy::StandsOn { held, .. } => (held.upstream, held.hold),
        other => panic!("{n} is left out as standing on another, not {other:?}"),
    }
}

/// A spec `SP-<n>` grounded in these findings, with these extra links.
fn sp_with(n: u128, links: &[(&str, &[&str])]) -> DocReading {
    file(
        &format!("specs/sp-{n}.md"),
        n,
        &format!("SP-{n}"),
        "spec",
        links,
    )
}

/// FR-5 with no title: a file of the finding that is not a valid document.
fn untitled_fr5() -> DocReading {
    let mut entries = BTreeMap::new();
    entries.insert("id".to_owned(), RawValue::Text(docid(5).to_string()));
    entries.insert("key".to_owned(), RawValue::Text("FR-5".to_owned()));
    DocReading::Parsed {
        origin: path("findings/fr-5.md"),
        entries,
        body: "no title\n".to_owned(),
    }
}

/// FR-5, and SP-2 and SP-3 grounded on it, all approved.
fn approved_diamond_top() -> ShelfApp<FakeShelf> {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        sp(2, "cache", &["FR-5"]),
        sp(3, "index", &["FR-5"]),
        fr(5, "slow", &[]),
    ]));
    block_on(app.judge_all(vec![
        approve("FR-5", None),
        approve("SP-2", None),
        approve("SP-3", None),
    ]))
    .unwrap();
    app
}

#[test]
fn an_edited_upstream_and_what_stands_on_it_are_approved_again_upstream_first() {
    let mut app = approved_diamond_top();
    app.ports().edit(fr(5, "slower", &[]), 5);
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 3);

    let done = again(&mut app).unwrap();
    let order = judged_ids(&done);
    assert_eq!(order.len(), 3);
    assert_eq!(order[0], docid(5));
    assert!(done.left_out.is_empty());
    assert_eq!(app.ports().record_count(), 6);
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    for judged in &done.judged {
        let crate::domain::shelf::ledger::Record::Decision(decision) = &judged.judged.record else {
            panic!("an approval");
        };
        assert_eq!(decision.reason().map(|r| r.as_str()), Some("reviewed"));
    }
}

#[test]
fn an_expired_approval_is_approved_again() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    block_on(app.judge(approve("FR-1", Some(2000)))).unwrap();
    app.ports().set_now(3000);
    let row = block_on(app.get(&"FR-1".parse().unwrap())).unwrap().row;
    assert!(matches!(
        row.standing,
        Standing::Approved(Freshness::Degraded(ref d)) if d.level() == Severity::Expired
    ));

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(1)]);
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    // The expiry it had is not carried over: no --until, no expiry.
    let Record::Decision(decision) = &done.judged[0].judged.record else {
        panic!("an approval");
    };
    assert!(matches!(
        decision.verb(),
        DecisionVerb::Approve { expires: None, .. }
    ));
}

#[test]
fn a_broken_approval_is_left_out_and_listed() {
    // SP-2 pins tests/a.rs, which goes; FR-5 becomes invalid, so SP-3 stands on an
    // invalid upstream. FR-1 is only edited, and is approved again.
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &[], &["tests/a.rs"]),
        sp(3, "index", &["FR-5"]),
        fr(5, "fast", &[]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![
        approve("FR-1", None),
        approve("SP-2", None),
        approve("FR-5", None),
        approve("SP-3", None),
    ]))
    .unwrap();
    app.ports().remove_file("tests/a.rs");
    app.ports().edit(untitled_fr5(), 5);
    app.ports().edit(fr(1, "slower", &[]), 1);

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(1)]);
    let left: Vec<_> = done.left_out.iter().map(|l| l.left.doc).collect();
    assert_eq!(left, vec![docid(2), docid(3), docid(5)]);
    let causes_of = |n: u128| broken_of(&done, n);
    assert!(
        causes_of(2)
            .causes()
            .iter()
            .any(|c| matches!(c, Cause::ArtifactGone { .. }))
    );
    assert!(
        causes_of(3)
            .causes()
            .iter()
            .any(|c| matches!(c, Cause::UpstreamNotValid { .. }))
    );
    assert!(
        [2, 3, 5]
            .iter()
            .all(|n| causes_of(*n).level() == Severity::Broken)
    );
    assert_eq!(
        done.left_out[0]
            .left
            .key
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("spec:SP-2")
    );
    // Only FR-1's approval was written; the broken ones still do not hold.
    assert_eq!(app.ports().record_count(), 5);
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 3);
}

#[test]
fn a_pending_document_is_not_touched() {
    let mut app = ShelfApp::new(
        FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]),
    );
    block_on(app.judge(approve("FR-1", None))).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(1)]);
    let status = block_on(app.status()).unwrap();
    let spec = status.rows.iter().find(|r| r.id == docid(2)).unwrap();
    assert_eq!(spec.declared, DeclaredStatus::Pending);
    assert_eq!(spec.standing, Standing::Pending);
}

#[test]
fn nothing_stale_is_an_empty_run_that_writes_nothing() {
    let mut app = approved_diamond_top();
    let before = app.ports().append_count();
    let done = again(&mut app).unwrap();
    assert!(done.judged.is_empty());
    assert!(done.left_out.is_empty());
    assert_eq!(app.ports().append_count(), before);
}

#[test]
fn a_frame_the_model_refuses_is_left_out_and_the_rest_is_written() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1", None), approve("SP-2", None)])).unwrap();
    // FR-1 is edited, so both are stale; SP-2 cannot be approved again while its test
    // cannot be told about.
    app.ports().edit(fr(1, "slower", &[]), 1);
    app.ports()
        .cannot_tell(&file_space(), "tests/a.rs", "offline");

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(1)]);
    assert!(matches!(
        why_of(&done, 2),
        LeftOutWhy::Refused(FrameRefusal::ArtifactUnknown { .. })
    ));
    assert_eq!(app.ports().record_count(), 3);
}

#[test]
fn an_approval_broken_only_through_its_upstream_is_left_out() {
    // FR-5 stands on FR-6, SP-3 on FR-5. FR-6 is denied: FR-5 is broken by it, and SP-3
    // only through FR-5.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(6, "base", &[]),
        fr(5, "slow", &["FR-6"]),
        sp(3, "index", &["FR-5"]),
    ]));
    block_on(app.judge_all(vec![
        approve("FR-6", None),
        approve("FR-5", None),
        approve("SP-3", None),
    ]))
    .unwrap();
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: "FR-6".parse().unwrap(),
        actor: None,
    }))
    .unwrap();
    let before = app.ports().record_count();

    let done = again(&mut app).unwrap();
    assert!(done.judged.is_empty());
    let through = broken_of(&done, 3);
    assert!(
        through
            .causes()
            .iter()
            .all(|c| matches!(c, Cause::Via { .. }))
    );
    assert_eq!(broken_of(&done, 5).level(), Severity::Broken);
    assert_eq!(app.ports().record_count(), before);
}

#[test]
fn a_stale_approval_newly_linking_a_broken_one_is_left_out_as_standing_on_it() {
    // FR-8 is broken (its upstream FR-9 is denied); FR-7, approved on nothing, now
    // links to FR-8.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(9, "base", &[]),
        fr(8, "broken", &["FR-9"]),
        fr(7, "alone", &[]),
    ]));
    block_on(app.judge_all(vec![
        approve("FR-9", None),
        approve("FR-8", None),
        approve("FR-7", None),
    ]))
    .unwrap();
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: "FR-9".parse().unwrap(),
        actor: None,
    }))
    .unwrap();
    app.ports().edit(fr(7, "alone", &["FR-8"]), 7);

    let done = again(&mut app).unwrap();
    assert!(done.judged.is_empty());
    assert_eq!(held_of(&done, 7), (docid(8), Hold::Broken));
    assert!(matches!(why_of(&done, 8), LeftOutWhy::Broken(_)));
}

#[test]
fn a_stale_approval_on_a_deprecated_upstream_is_refused_and_the_rest_is_written() {
    // Findings may supersede findings here, so FR-1 deprecated has a successor, FR-3,
    // and FR-2 on it is only stale.
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "old", &[]),
        fr(2, "on it", &["FR-1"]),
        file(
            "findings/fr-3.md",
            3,
            "FR-3",
            "new",
            &[("supersedes", &["FR-1"])],
        ),
        fr(4, "apart", &[]),
    ]);
    fake.decls.lock().unwrap()[0]
        .links
        .push(super::fake_tests::link(
            "supersedes",
            "finding",
            crate::domain::shelf::schema::Propagation::Replaces,
        ));
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![
        approve("FR-1", None),
        approve("FR-2", None),
        approve("FR-4", None),
    ]))
    .unwrap();
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Deprecate {
            successor: None,
            reason: None,
        },
        reference: "FR-1".parse().unwrap(),
        actor: None,
    }))
    .unwrap();
    app.ports().edit(fr(4, "apart, edited", &[]), 4);

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(4)]);
    assert!(matches!(
        why_of(&done, 2),
        LeftOutWhy::Refused(FrameRefusal::UpstreamNotStanding { .. })
    ));
    assert_eq!(app.ports().record_count(), 5);
}

#[test]
fn a_stale_approval_on_a_pending_upstream_is_left_out_until_that_is_approved() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        fr(2, "on it", &["FR-1"]),
        fr(3, "new", &[]),
    ]));
    block_on(app.judge_all(vec![approve("FR-1", None), approve("FR-2", None)])).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    app.ports().edit(fr(2, "on it", &["FR-1", "FR-3"]), 2);

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(1)]);
    assert_eq!(held_of(&done, 2), (docid(3), Hold::Pending));
}

#[test]
fn a_name_with_nothing_there_is_pinned_as_absent_and_told() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![sp_with(2, &[])]));
    block_on(app.judge(approve("SP-2", None))).unwrap();
    app.ports()
        .edit(sp_with(2, &[("tests", &["tests/later.rs"])]), 2);

    let done = again(&mut app).unwrap();
    assert_eq!(judged_ids(&done), vec![docid(2)]);
    let absent: Vec<String> = done.judged[0]
        .judged
        .absent
        .iter()
        .map(|(_, a)| a.name().to_string())
        .collect();
    assert_eq!(absent, ["tests/later.rs"]);
}

#[test]
fn until_and_actor_reach_every_record_and_an_old_expiry_is_not_kept() {
    let mut app = ShelfApp::new(
        FakeShelf::new().with_files(vec![fr(1, "slow", &[]), fr(2, "on it", &["FR-1"])]),
    );
    block_on(app.judge_all(vec![approve("FR-1", Some(1500)), approve("FR-2", None)])).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    let done = block_on(app.approve_stale(ApproveStaleRequest {
        reason: reason("reviewed"),
        actor: Some(Actor::new("bob").unwrap()),
        until: Some(JudgedAt::new(9000)),
    }))
    .unwrap();
    assert_eq!(judged_ids(&done), vec![docid(1), docid(2)]);
    for judged in &done.judged {
        let Record::Decision(decision) = &judged.judged.record else {
            panic!("an approval");
        };
        assert_eq!(decision.actor().as_str(), "bob");
        let DecisionVerb::Approve { expires, .. } = decision.verb() else {
            panic!("an approval");
        };
        assert_eq!(*expires, Some(JudgedAt::new(9000)));
    }
}

#[test]
fn an_until_not_after_now_is_refused_even_when_nothing_is_stale() {
    let mut app = approved_diamond_top();
    let before = app.ports().record_count();
    let err = block_on(app.approve_stale(ApproveStaleRequest {
        reason: reason("reviewed"),
        actor: None,
        until: Some(JudgedAt::new(1000)),
    }))
    .unwrap_err();
    assert!(matches!(
        err,
        ShelfError::Decision(DecisionProblem::ExpiresNotAfter { .. })
    ));
    assert_eq!(err.class(), ErrorClass::Refused);
    assert_eq!(app.ports().record_count(), before);
}
