//! Tests of the ledger.

use std::collections::BTreeSet;

use crate::domain::canon::Canon;
use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
use crate::domain::shelf::doc::UnresolvedWhy;
use crate::domain::shelf::fixtures_tests::{
    actor, approve, deny, deprecate, docid, fr, fr_titled, jid, ledger, path, reading, retire,
    shelf,
};

use super::*;

fn revoke(judgement: u128, target: u128, at: i64) -> Record {
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(BTreeSet::from([jid(target)])).unwrap(),
        actor(),
        JudgedAt::new(at),
        NonBlank::new("changed my mind").unwrap(),
    ))
}

#[test]
fn a_document_nothing_was_decided_about_is_pending() {
    assert_eq!(ledger(vec![]).status_of(docid(1)), DeclaredStatus::Pending);
}

#[test]
fn an_approval_keeps_the_hash_it_saw_and_the_latest_decision_stands() {
    let shelf = shelf(vec![fr(1, &[])]);
    let none = ledger(vec![]);
    let approval = approve(&shelf, &none, 1, 1001, 10, None);
    let seen = *shelf.doc(docid(1)).unwrap().hash();
    match ledger(vec![approval.clone()]).status_of(docid(1)) {
        DeclaredStatus::Approved {
            judgement,
            seen: approved,
            frame,
            expires,
        } => {
            assert_eq!(judgement, jid(1001));
            assert_eq!(approved, seen);
            assert!(frame.is_empty());
            assert_eq!(expires, None);
        }
        other => panic!("expected an approval, got {other:?}"),
    }

    let later = ledger(vec![approval, deny(&shelf, 1, 1002, 20)]);
    assert_eq!(
        later.status_of(docid(1)),
        DeclaredStatus::Denied {
            judgement: jid(1002)
        }
    );
}

#[test]
fn a_revocation_withdraws_the_decision_it_names() {
    let shelf = shelf(vec![fr(1, &[])]);
    let approval = approve(&shelf, &ledger(vec![]), 1, 1001, 10, None);
    let with_revocation = ledger(vec![approval, revoke(1002, 1001, 20)]);
    assert_eq!(with_revocation.status_of(docid(1)), DeclaredStatus::Pending);
    assert!(with_revocation.problems().is_empty());
}

#[test]
fn a_retirement_needs_no_file() {
    let shelf = shelf(vec![]);
    let record = retire(&shelf, 7, 1001, 10);
    assert_eq!(
        ledger(vec![record]).status_of(docid(7)),
        DeclaredStatus::Retired {
            judgement: jid(1001)
        }
    );
}

#[test]
fn a_retirement_writes_what_it_saw_as_a_state_and_a_hash_and_reads_it_back() {
    let seen_of = |record: &Record| match map_mut(&mut record.to_canon()).get("seen") {
        Some(Canon::Map(seen)) => seen.clone(),
        other => panic!("a retirement has a seen map, got {other:?}"),
    };
    let state = |state: &str, hash: Canon| {
        BTreeMap::from([
            ("state".to_owned(), Canon::str(state)),
            ("hash".to_owned(), hash),
        ])
    };
    let valid = shelf(vec![fr(1, &[])]);
    let hash = valid.doc(docid(1)).unwrap().hash().to_string();
    let invalid = shelf(vec![reading("findings/fr-1.md", 1, "FR-1", &[])]);
    let unknown = shelf(vec![DocReading::Unparsed {
        origin: path("findings/x.md"),
        id: ReadId::Unreadable,
        why: RawProblem::NotAMapping,
    }]);
    for (shelf, expected) in [
        (&valid, state("at", Canon::str(hash))),
        (&invalid, state("invalid", Canon::Null)),
        (&shelf(vec![]), state("gone", Canon::Null)),
        (&unknown, state("unknown", Canon::Null)),
    ] {
        let record = retire(shelf, 1, 1001, 10);
        assert_eq!(seen_of(&record), expected);
        assert_eq!(Record::from_canon(&record.to_canon()).unwrap(), record);
    }
}

#[test]
fn a_record_survives_its_canonical_form_and_keeps_its_hash() {
    let shelf = shelf(vec![fr(1, &[])]);
    let record = approve(&shelf, &ledger(vec![]), 1, 1001, 10, Some(500));
    let back = Record::from_canon(&record.to_canon()).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.hash(), record.hash());
}

#[test]
fn two_different_records_with_one_id_are_set_aside() {
    let shelf = shelf(vec![fr(1, &[])]);
    let first = deny(&shelf, 1, 1001, 10);
    let second = deny(&shelf, 1, 1001, 20);
    let ledger = ledger(vec![first, second]);
    assert!(
        ledger
            .problems()
            .iter()
            .any(|problem| matches!(problem, LedgerProblem::Conflict { .. }))
    );
    assert_eq!(ledger.status_of(docid(1)), DeclaredStatus::Pending);
}

/// A shelf where FR-2 depends on FR-1, and the ledger of one judgement of FR-1.
fn upstream_judged(judge: impl Fn(&Shelf) -> Record) -> (Shelf, Ledger) {
    let shelf = shelf(vec![fr_titled(1, "a", &[]), fr_titled(2, "b", &["FR-1"])]);
    let ledger = ledger(vec![judge(&shelf)]);
    (shelf, ledger)
}

#[test]
fn an_approval_is_refused_on_an_upstream_that_does_not_stand() {
    type Judge = Box<dyn Fn(&Shelf) -> Record>;
    let cases: Vec<(NotStanding, Judge)> = vec![
        (NotStanding::Denied, Box::new(|s| deny(s, 1, 1001, 10))),
        (
            NotStanding::Deprecated,
            Box::new(|s| deprecate(s, 1, 1001, 10)),
        ),
        (NotStanding::Retired, Box::new(|s| retire(s, 1, 1001, 10))),
    ];
    for (expected, judge) in cases {
        let (shelf, ledger) = upstream_judged(judge);
        match frame_approve(&shelf, &ledger, docid(2)) {
            Err(FrameRefusal::UpstreamNotStanding {
                link,
                target,
                status,
            }) => {
                assert_eq!(link.as_str(), "base");
                assert_eq!(target, docid(1));
                assert_eq!(status, expected);
            }
            other => panic!("{expected:?}: expected a refusal, got {other:?}"),
        }
    }
}

#[test]
fn an_approval_on_a_pending_upstream_is_taken() {
    let shelf = shelf(vec![fr_titled(1, "a", &[]), fr_titled(2, "b", &["FR-1"])]);
    assert!(frame_approve(&shelf, &ledger(vec![]), docid(2)).is_ok());
}

#[test]
fn an_approval_is_refused_on_an_upstream_that_is_not_valid() {
    // FR-1 has no title, which the book requires.
    let shelf = shelf(vec![
        reading("findings/fr-1.md", 1, "FR-1", &[]),
        fr_titled(2, "b", &["FR-1"]),
    ]);
    match frame_approve(&shelf, &ledger(vec![]), docid(2)) {
        // The build leaves such a link unresolved, so the refusal is this one.
        Err(FrameRefusal::UnresolvedDepends { unresolved, .. }) => {
            assert!(
                matches!(unresolved.why(), UnresolvedWhy::TargetInvalid(id) if *id == docid(1))
            );
        }
        other => panic!("expected an unresolved link, got {other:?}"),
    }
}

fn map_mut(canon: &mut Canon) -> &mut BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        other => panic!("not a map: {other:?}"),
    }
}

fn frame_of(canon: &mut Canon) -> &mut Vec<Canon> {
    match map_mut(canon).get_mut("frame") {
        Some(Canon::List(frame)) => frame,
        other => panic!("an approval has a frame, got {other:?}"),
    }
}

/// The canonical form of an approval of FR-2 with one pin on FR-1, read back after
/// `change` was given the frame.
fn approval_canon(change: impl FnOnce(&mut Vec<Canon>)) -> Result<Record, RecordProblem> {
    let shelf = shelf(vec![fr_titled(1, "a", &[]), fr_titled(2, "b", &["FR-1"])]);
    let record = approve(&shelf, &ledger(vec![]), 2, 1001, 10, None);
    let mut canon = record.to_canon();
    let frame = frame_of(&mut canon);
    assert_eq!(frame.len(), 1);
    change(frame);
    Record::from_canon(&canon)
}

fn set_tail(pin: &mut Canon, kind: &str, value: &str) {
    let key = map_mut(map_mut(pin).get_mut("key").unwrap());
    let form = map_mut(key.get_mut("form").unwrap());
    let tail = map_mut(form.get_mut("tail").unwrap());
    tail.insert("kind".to_owned(), Canon::str(kind));
    tail.insert("value".to_owned(), Canon::str(value));
}

#[test]
fn a_record_is_read_back_when_its_pin_is_sound() {
    assert!(approval_canon(|_| {}).is_ok());
    assert!(approval_canon(|frame| set_tail(&mut frame[0], "number", "2")).is_ok());
}

#[test]
fn a_pinned_key_tail_no_document_could_have_is_refused() {
    for value in ["0", "007", "12345678901234567890"] {
        let result = approval_canon(|frame| set_tail(&mut frame[0], "number", value));
        assert!(
            matches!(result, Err(RecordProblem::BadKey(_))),
            "{value}: {result:?}"
        );
    }
}

#[test]
fn a_pinned_key_tail_whose_kind_is_not_what_its_text_reads_as_is_refused() {
    // "number" over a name, and "name" over a number.
    for (kind, value) in [("number", "abc"), ("name", "7")] {
        let result = approval_canon(|frame| set_tail(&mut frame[0], kind, value));
        assert!(result.is_err(), "{kind} {value}: {result:?}");
    }
}

#[test]
fn a_kind_of_key_this_reader_does_not_know_is_not_called_a_kind_of_record() {
    // The tail of the pinned key, and the key itself.
    let result = approval_canon(|frame| set_tail(&mut frame[0], "word", "2"));
    assert!(
        matches!(&result, Err(RecordProblem::UnknownKeyKind { found }) if found == "word"),
        "{result:?}"
    );
    let result = approval_canon(|frame| {
        let key = map_mut(map_mut(&mut frame[0]).get_mut("key").unwrap());
        let form = map_mut(key.get_mut("form").unwrap());
        form.insert("kind".to_owned(), Canon::str("regex"));
    });
    assert!(
        matches!(&result, Err(RecordProblem::UnknownKeyKind { found }) if found == "regex"),
        "{result:?}"
    );
}

#[test]
fn a_frame_that_pins_one_link_to_one_document_twice_is_refused() {
    let shelf = shelf(vec![fr_titled(1, "a", &[]), fr_titled(2, "b", &["FR-1"])]);
    let other_hash = shelf.doc(docid(2)).unwrap().hash().to_string();
    let result = approval_canon(|frame| {
        // A second pin of the same link and target, with another hash.
        let mut twin = frame[0].clone();
        map_mut(&mut twin).insert("hash".to_owned(), Canon::str(other_hash));
        frame.push(twin);
    });
    assert!(matches!(
        result,
        Err(RecordProblem::Invariant(
            DecisionProblem::FramePinsTwice { .. }
        ))
    ));
}
