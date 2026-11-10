//! Tests of the ledger.

use std::collections::BTreeSet;

use crate::domain::shelf::fixtures_tests::{
    actor, approve, deny, docid, fr, jid, ledger, retire, shelf,
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
