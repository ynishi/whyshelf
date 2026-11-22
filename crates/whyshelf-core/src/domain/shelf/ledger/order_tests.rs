//! Tests of the order of the records ([`RecordOrder`]) and of the rule of a move apart
//! from it: the decision that stands, the readers and the declared status are read from
//! one list, a cut keeps the records before one in that order, and a move reaches the
//! pins of an approval made at or before it by time alone, of two moves of one name at
//! one time the larger identity.

use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    approve, by, docid, file_space, fr, jid, ledger, name, read_pins, shelf, shelf_with,
    sp_testing, standing,
};

use super::*;

/// The declaration `judgement`, at `at`, that `from` moved to `to` in the space `file`.
fn moved(judgement: u128, from: &str, to: &str, at: i64) -> Moved {
    Moved::new(
        jid(judgement),
        MovedOf::new(file_space(), name(from), name(to)).unwrap(),
        by(),
        JudgedAt::new(at),
        NonBlank::new("renamed").unwrap(),
    )
}

/// FR-1 approved (judgement 1, at 1) and SP-2 on it approved as `judgement` at `at`,
/// tested by `tests`, each there; then `moves`.
fn ledger_of(tests: &[&str], judgement: u128, at: i64, moves: Vec<Moved>) -> Ledger {
    let files: Vec<(&str, Option<ArtifactHash>)> = tests
        .iter()
        .map(|at| (*at, Some(ArtifactHash::of_content(at.as_bytes()))))
        .collect();
    let s = shelf_with(vec![fr(1, &[]), sp_testing(2, &["FR-1"], tests)], &files);
    let first = approve(&s, &ledger(vec![]), 1, 1, 1, None);
    let second = approve(&s, &ledger(vec![first.clone()]), 2, judgement, at, None);
    let records = [first, second]
        .into_iter()
        .chain(moves.into_iter().map(Record::Moved))
        .collect();
    ledger(records)
}

/// Each pin of SP-2's approval that stands, by the name it wrote and the name it is
/// read as, with the moves it is read through.
fn read_as(pins: &[ReadPin]) -> Vec<(String, String, Vec<JudgementId>)> {
    pins.iter()
        .map(|read| {
            (
                read.pin().artifact().name().to_string(),
                read.read_as().name().to_string(),
                read.moved()
                    .map(|m| m.declared().into_vec())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

fn row(written: &str, now: &str, through: &[u128]) -> (String, String, Vec<JudgementId>) {
    (
        written.to_owned(),
        now.to_owned(),
        through.iter().map(|n| jid(*n)).collect(),
    )
}

#[test]
fn the_decision_that_stands_its_readers_and_the_declared_status_are_of_one_list() {
    let s = shelf(vec![fr(1, &[])]);
    // Two approvals at one time and a later one: the latest in the order of the records.
    for at in [(10, 10, 20), (10, 10, 10), (30, 10, 20)] {
        let one = approve(&s, &ledger(vec![]), 1, 3, at.0, None);
        let two = approve(&s, &ledger(vec![]), 1, 5, at.1, None);
        let three = approve(&s, &ledger(vec![]), 1, 4, at.2, None);
        let l = ledger(vec![one, two, three]);
        let decision = l.standing_decision(docid(1)).map(Decision::id);
        let reader = l
            .readers(docid(1), Before::Now)
            .standing()
            .map(|v| v.judgement());
        let declared = standing(&l, docid(1)).view().judgement();
        assert_eq!(decision, reader, "{at:?}");
        assert_eq!(decision, Some(declared), "{at:?}");
        let latest = l.decisions().map(Decision::order).max();
        assert_eq!(
            l.standing_decision(docid(1)).map(Decision::order),
            latest,
            "{at:?}"
        );
    }
}

#[test]
fn a_cut_keeps_a_move_before_the_decision_in_the_order_of_the_records() {
    let l = ledger_of(&["tests/a.rs"], 5, 10, Vec::new());
    let decision = l.approval(jid(5)).unwrap().decision;
    let cut = Before::decision(decision);
    for (judgement, at, kept) in [(3, 10, true), (7, 10, false), (9, 9, true), (2, 11, false)] {
        let m = moved(judgement, "tests/a.rs", "tests/b.rs", at);
        assert_eq!(cut.keeps_move(&m), kept, "{judgement} at {at}");
        assert_eq!(cut.keeps_move(&m), m.order() < decision.order());
    }
    assert!(Before::Now.keeps_move(&moved(7, "tests/a.rs", "tests/b.rs", 10)));
}

/// The same millisecond, two names: a cut keeps the move of the smaller identity and not
/// the other, and both reach the approval's pins, which only time decides.
#[test]
fn a_move_of_the_same_millisecond_reaches_the_pins_of_the_approval_whatever_the_identities() {
    let l = ledger_of(
        &["tests/a.rs", "tests/c.rs"],
        5,
        10,
        vec![
            moved(3, "tests/a.rs", "tests/b.rs", 10),
            moved(7, "tests/c.rs", "tests/d.rs", 10),
        ],
    );
    let view = l.approval(jid(5)).unwrap();
    let cut = Before::decision(view.decision);
    assert!(cut.keeps_move(l.moved(jid(3)).unwrap()));
    assert!(!cut.keeps_move(l.moved(jid(7)).unwrap()));
    // Read now, both moves reach both pins.
    assert_eq!(
        read_as(&view.read_pins()),
        [
            row("tests/a.rs", "tests/b.rs", &[3]),
            row("tests/c.rs", "tests/d.rs", &[7])
        ]
    );
    // Read before the approval, the cut keeps the first move alone, which reaches it.
    assert_eq!(
        read_as(&l.read_pins_under(view.artifacts(), view.at(), cut)),
        [
            row("tests/a.rs", "tests/b.rs", &[3]),
            row("tests/c.rs", "tests/c.rs", &[])
        ]
    );
    assert!(view.read_through_a_move(cut));
    let none = Before::Record(RecordOrder::of(JudgedAt::new(10), jid(2)));
    assert!(!view.read_through_a_move(none));
    // A later approval asked about reads the approval that stands through the moves its
    // cut keeps: both, so it renews.
    let later = Before::Record(RecordOrder::of(JudgedAt::new(10), jid(8)));
    assert!(view.read_through_a_move(later));
}

/// The same millisecond, one name: the move of the larger identity alone is followed,
/// and a chain of moves at one time is followed to its end.
#[test]
fn of_two_moves_of_one_name_at_one_time_the_larger_identity_is_followed_and_a_chain_to_its_end() {
    let one_name = ledger_of(
        &["tests/a.rs"],
        5,
        10,
        vec![
            moved(3, "tests/a.rs", "tests/b.rs", 10),
            moved(7, "tests/a.rs", "tests/c.rs", 10),
        ],
    );
    assert_eq!(
        read_as(&read_pins(&one_name, docid(2))),
        [row("tests/a.rs", "tests/c.rs", &[7])]
    );
    let chain = ledger_of(
        &["tests/a.rs"],
        5,
        10,
        vec![
            moved(3, "tests/a.rs", "tests/b.rs", 10),
            moved(7, "tests/b.rs", "tests/c.rs", 10),
        ],
    );
    assert_eq!(
        read_as(&read_pins(&chain, docid(2))),
        [row("tests/a.rs", "tests/c.rs", &[3, 7])]
    );
}

#[test]
fn every_reader_of_a_content_is_a_view_of_its_record() {
    let s = shelf(vec![fr(1, &[])]);
    let ana = approve(&s, &ledger(vec![]), 1, 1, 10, Some(50));
    let bo = approve(&s, &ledger(vec![ana.clone()]), 1, 2, 20, None);
    let l = ledger(vec![ana, bo]);
    let readers = l.readers(docid(1), Before::Now);
    let rows: Vec<(JudgementId, Option<JudgedAt>)> = readers
        .approvals()
        .iter()
        .map(|v| (v.judgement(), v.expires()))
        .collect();
    assert_eq!(rows, [(jid(2), None), (jid(1), Some(JudgedAt::new(50)))]);
    // The second reader's own expiry is what lapses it.
    for (view, decision) in readers.approvals().iter().zip([jid(2), jid(1)]) {
        assert_eq!(view.judgement(), decision);
        assert_eq!(
            l.approval(decision).map(|v| v.expires()),
            Some(view.expires())
        );
    }
}
