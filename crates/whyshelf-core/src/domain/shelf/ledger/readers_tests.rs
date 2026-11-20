//! Tests of the readers of a document's content and of what a new approval is beside
//! them: the one place two approvals are matched by what they stand on and who made
//! them.

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::shelf::fixtures_tests::{deny, docid, fr_titled, jid, ledger, shelf};

use super::*;

fn who(name: &str) -> Actor {
    Actor::new(name).unwrap()
}

/// FR-1 with this title, alone on the shelf: another title is other content.
fn titled(title: &str) -> Shelf {
    shelf(vec![fr_titled(1, title, &[])])
}

/// An approval of FR-1 as `shelf` holds it, by `by`, at `at`, lapsing at `until`.
fn approval(shelf: &Shelf, judgement: u128, by: &str, at: i64, until: Option<i64>) -> Record {
    let frame = frame_approve(shelf, &ledger(vec![]), docid(1)).unwrap();
    Record::Decision(
        Decision::approve(
            jid(judgement),
            frame,
            until.map(JudgedAt::new),
            who(by),
            JudgedAt::new(at),
            None,
        )
        .unwrap(),
    )
}

fn revoke(judgement: u128, target: u128, at: i64) -> Record {
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(BTreeSet::from([jid(target)])).unwrap(),
        who("ana"),
        JudgedAt::new(at),
        NonBlank::new("not read").unwrap(),
    ))
}

/// What an approval of FR-1 as `shelf` holds it, by `by`, at `at`, lapsing at
/// `until`, would be beside `records`.
fn kind(records: &[Record], shelf: &Shelf, by: &str, at: i64, until: Option<i64>) -> ApprovalKind {
    let ledger = ledger(records.to_vec());
    let frame = frame_approve(shelf, &ledger, docid(1)).unwrap();
    let by = who(by);
    let asked = Asked::frame(&frame, &by, until.map(JudgedAt::new), JudgedAt::new(at));
    ledger.kind_of(&asked, Before::Now)
}

/// The readers of FR-1 now, by the identity of their approvals, the latest first.
fn readers(records: &[Record]) -> Vec<JudgementId> {
    let ledger = ledger(records.to_vec());
    ledger
        .readers(docid(1), Before::Now)
        .approvals()
        .iter()
        .map(|d| d.id())
        .collect()
}

fn actors(records: &[Record]) -> Vec<String> {
    let ledger = ledger(records.to_vec());
    ledger
        .readers(docid(1), Before::Now)
        .actors()
        .into_iter()
        .map(|a| a.as_str().to_owned())
        .collect()
}

#[test]
fn the_same_actor_approving_the_same_content_again_repeats() {
    let a = titled("a");
    let records = [approval(&a, 1, "ana", 10, None)];
    assert_eq!(
        kind(&records, &a, "ana", 20, None),
        ApprovalKind::Repeat { standing: jid(1) }
    );
}

#[test]
fn another_actor_approving_the_same_content_is_a_second_reading() {
    let a = titled("a");
    let records = [approval(&a, 1, "ana", 10, None)];
    assert_eq!(
        kind(&records, &a, "bo", 20, None),
        ApprovalKind::SecondReading { standing: jid(1) }
    );
}

#[test]
fn after_a_second_reading_either_reader_repeats_and_a_third_actor_reads_again() {
    let a = titled("a");
    let records = [
        approval(&a, 1, "ana", 10, None),
        approval(&a, 2, "bo", 20, None),
    ];
    assert_eq!(readers(&records), [jid(2), jid(1)]);
    assert_eq!(actors(&records), ["ana", "bo"]);
    // The approval that stands is the latest: bo's.
    assert_eq!(
        kind(&records, &a, "ana", 30, None),
        ApprovalKind::Repeat { standing: jid(2) }
    );
    assert_eq!(
        kind(&records, &a, "bo", 30, None),
        ApprovalKind::Repeat { standing: jid(2) }
    );
    assert_eq!(
        kind(&records, &a, "cy", 30, None),
        ApprovalKind::SecondReading { standing: jid(2) }
    );
}

#[test]
fn other_content_in_between_starts_the_readers_again() {
    // X by ana, then Y by cy, then X again by bo: X's readers are bo alone.
    let (x, y) = (titled("x"), titled("y"));
    let records = [
        approval(&x, 1, "ana", 10, None),
        approval(&y, 2, "cy", 20, None),
        approval(&x, 3, "bo", 30, None),
    ];
    assert_eq!(readers(&records), [jid(3)]);
    assert_eq!(actors(&records), ["bo"]);
    assert_eq!(
        kind(&records, &x, "ana", 40, None),
        ApprovalKind::SecondReading { standing: jid(3) }
    );
}

#[test]
fn an_approval_of_other_content_or_with_none_standing_is_new() {
    let (a, b) = (titled("a"), titled("b"));
    assert_eq!(kind(&[], &a, "ana", 10, None), ApprovalKind::New);
    let records = [approval(&a, 1, "ana", 10, None)];
    assert_eq!(kind(&records, &b, "ana", 20, None), ApprovalKind::New);
    assert_eq!(kind(&records, &b, "bo", 20, None), ApprovalKind::New);
}

#[test]
fn a_decision_that_is_no_approval_ends_the_readers() {
    let a = titled("a");
    let denied = [approval(&a, 1, "ana", 10, None), deny(&a, 1, 2, 20)];
    assert!(readers(&denied).is_empty());
    assert_eq!(kind(&denied, &a, "ana", 30, None), ApprovalKind::New);
    // An approval after the denial starts a list of its own.
    let again = [
        approval(&a, 1, "ana", 10, None),
        deny(&a, 1, 2, 20),
        approval(&a, 3, "bo", 30, None),
    ];
    assert_eq!(readers(&again), [jid(3)]);
    assert_eq!(
        kind(&again, &a, "ana", 40, None),
        ApprovalKind::SecondReading { standing: jid(3) }
    );
}

#[test]
fn a_revoked_approval_reads_nothing() {
    let a = titled("a");
    let records = [
        approval(&a, 1, "ana", 10, None),
        approval(&a, 2, "bo", 20, None),
        revoke(3, 2, 30),
    ];
    assert_eq!(readers(&records), [jid(1)]);
    assert_eq!(
        kind(&records, &a, "bo", 40, None),
        ApprovalKind::SecondReading { standing: jid(1) }
    );
}

#[test]
fn a_reader_whose_reading_lapsed_or_lapses_at_another_time_renews_it() {
    let a = titled("a");
    // ana's reading lapses at 100, bo's, which stands, at 200.
    let records = [
        approval(&a, 1, "ana", 10, Some(100)),
        approval(&a, 2, "bo", 20, Some(200)),
    ];
    // ana asking for 100 again would move when the document lapses, to 100.
    assert_eq!(
        kind(&records, &a, "ana", 50, Some(100)),
        ApprovalKind::Renewal
    );
    // Asking for 200 moves when her own reading lapses; bo asking for it repeats.
    assert_eq!(
        kind(&records, &a, "ana", 50, Some(200)),
        ApprovalKind::Renewal
    );
    assert_eq!(
        kind(&records, &a, "bo", 50, Some(200)),
        ApprovalKind::Repeat { standing: jid(2) }
    );
    assert_eq!(kind(&records, &a, "ana", 50, None), ApprovalKind::Renewal);
    assert_eq!(
        kind(&records, &a, "ana", 150, Some(200)),
        ApprovalKind::Renewal
    );
    // An expiry already past repeats nothing: no approval can be made with it.
    assert_eq!(
        kind(&records, &a, "ana", 150, Some(100)),
        ApprovalKind::Renewal
    );
}

#[test]
fn once_the_approval_that_stands_lapsed_a_reader_whose_own_has_not_renews_it() {
    let a = titled("a");
    // ana's reading never lapses; bo's, which stands, lapses at 30.
    let records = [
        approval(&a, 1, "ana", 10, None),
        approval(&a, 2, "bo", 20, Some(30)),
    ];
    // Before 30, ana asking for no expiry would take the document's expiry away.
    assert_eq!(kind(&records, &a, "ana", 25, None), ApprovalKind::Renewal);
    // After it the document is expired: her approval approves it again.
    assert_eq!(kind(&records, &a, "ana", 50, None), ApprovalKind::Renewal);
}

#[test]
fn another_actor_asking_another_expiry_than_the_approval_that_stands_renews_it() {
    let a = titled("a");
    let records = [approval(&a, 1, "ana", 10, None)];
    // bo's approval would make the document lapse at 2000: it moves what freshness
    // reads, so it is no second reading, and nor is one taking an expiry away.
    assert_eq!(
        kind(&records, &a, "bo", 20, Some(2000)),
        ApprovalKind::Renewal
    );
    let records = [approval(&a, 1, "ana", 10, Some(2000))];
    assert_eq!(kind(&records, &a, "bo", 20, None), ApprovalKind::Renewal);
    assert_eq!(
        kind(&records, &a, "bo", 20, Some(2000)),
        ApprovalKind::SecondReading { standing: jid(1) }
    );
}

#[test]
fn while_a_move_reads_a_pin_of_the_approval_that_stands_a_new_one_renews_it() {
    use crate::domain::shelf::artifact::ArtifactHash;
    use crate::domain::shelf::fixtures_tests::{
        actor, approve, file_space, fr, name, shelf_with, sp_testing,
    };
    // SP-2 pins tests/a.rs, approved at 10; a move to tests/b.rs is declared at 50.
    let s = shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&s, &ledger(vec![]), 1, 1, 10, None);
    let second = approve(&s, &ledger(vec![first.clone()]), 2, 2, 10, None);
    let moved = Record::Moved(Moved::new(
        jid(3),
        MovedOf::new(file_space(), name("tests/a.rs"), name("tests/b.rs")).unwrap(),
        actor(),
        JudgedAt::new(50),
        NonBlank::new("renamed").unwrap(),
    ));
    let kind_at = |records: Vec<Record>, by: &Actor, before: Before| {
        let ledger = ledger(records);
        let frame = frame_approve(&s, &ledger, docid(2)).unwrap();
        let asked = Asked::frame(&frame, by, None, JudgedAt::new(60));
        ledger.kind_of(&asked, before)
    };
    let records = vec![first.clone(), second.clone()];
    assert_eq!(
        kind_at(records, &actor(), Before::Now),
        ApprovalKind::Repeat { standing: jid(2) }
    );
    // The same pins, but freshness reads the standing one through the move: a new
    // approval, which pins tests/a.rs read through none, changes what it reads.
    let records = vec![first.clone(), second.clone(), moved.clone()];
    assert_eq!(
        kind_at(records.clone(), &actor(), Before::Now),
        ApprovalKind::Renewal
    );
    assert_eq!(
        kind_at(records.clone(), &who("bo"), Before::Now),
        ApprovalKind::Renewal
    );
    // Read before the move was declared, it is not read through it.
    let before = Before::Record {
        at: JudgedAt::new(40),
        id: jid(9),
    };
    assert_eq!(
        kind_at(records, &actor(), before),
        ApprovalKind::Repeat { standing: jid(2) }
    );
}

#[test]
fn once_the_approval_that_stands_lapsed_another_actor_renews_it() {
    let a = titled("a");
    let records = [approval(&a, 1, "ana", 10, Some(30))];
    // Before 30, bo's approval is a second reading; after it, it approves the
    // expired document again, as ana's would.
    assert_eq!(
        kind(&records, &a, "bo", 25, Some(30)),
        ApprovalKind::SecondReading { standing: jid(1) }
    );
    assert_eq!(kind(&records, &a, "bo", 50, None), ApprovalKind::Renewal);
    assert_eq!(kind(&records, &a, "ana", 50, None), ApprovalKind::Renewal);
}

#[test]
fn before_a_record_reads_the_ledger_as_it_was_and_by_identity_at_one_time() {
    let a = titled("a");
    // ana and bo at 10, cy at 20: of one time, the smaller identity comes first.
    let records = vec![
        approval(&a, 1, "ana", 10, None),
        approval(&a, 2, "bo", 10, None),
        approval(&a, 3, "cy", 20, None),
        approval(&a, 4, "cy", 30, None),
    ];
    let ledger = ledger(records);
    let kind_of = |n: u128| {
        let decision = ledger.decisions().find(|d| d.id() == jid(n)).unwrap();
        let asked = Asked::decision(decision).unwrap();
        ledger.kind_of(&asked, Before::decision(decision))
    };
    assert_eq!(kind_of(1), ApprovalKind::New);
    assert_eq!(kind_of(2), ApprovalKind::SecondReading { standing: jid(1) });
    assert_eq!(kind_of(3), ApprovalKind::SecondReading { standing: jid(2) });
    assert_eq!(kind_of(4), ApprovalKind::Repeat { standing: jid(3) });
    let before_cy: Vec<JudgementId> = ledger
        .readers(
            docid(1),
            Before::Record {
                at: JudgedAt::new(20),
                id: jid(3),
            },
        )
        .approvals()
        .iter()
        .map(|d| d.id())
        .collect();
    assert_eq!(before_cy, [jid(2), jid(1)]);
}

#[test]
fn a_decision_that_is_no_approval_is_asked_nothing() {
    let a = titled("a");
    let records = vec![deny(&a, 1, 1, 10)];
    let ledger = ledger(records);
    let decision = ledger.decisions().next().unwrap();
    assert!(Asked::decision(decision).is_none());
}

#[test]
fn a_named_run_leaves_out_a_repeat_unless_again_and_a_chosen_run_a_second_reading() {
    let standing = jid(1);
    let repeat = ApprovalKind::Repeat { standing };
    let second = ApprovalKind::SecondReading { standing };
    let repeats = Some(LeaveOut::Repeats { approval: standing });
    let second_reading = Some(LeaveOut::SecondReading { approval: standing });
    assert_eq!(repeat.left_out(Taking::Named, false), repeats);
    assert_eq!(repeat.left_out(Taking::Named, true), None);
    assert_eq!(repeat.left_out(Taking::Chosen, false), repeats);
    assert_eq!(second.left_out(Taking::Named, false), None);
    assert_eq!(second.left_out(Taking::Chosen, false), second_reading);
    for kind in [ApprovalKind::New, ApprovalKind::Renewal] {
        for taking in [Taking::Named, Taking::Chosen] {
            assert_eq!(kind.left_out(taking, false), None);
        }
    }
}
