//! Tests of the revocations the ledger sets aside: of an id no record has, of a
//! revocation, and of an id whose records conflict. Each withdraws nothing for that
//! target, and the other targets of the same revocation still count.

use std::collections::BTreeSet;

use crate::domain::shelf::fixtures_tests::{actor, deny, docid, fr, jid, ledger, shelf};

use super::*;

/// A revocation `judgement` of every one of `targets`.
fn revoke(judgement: u128, targets: &[u128], at: i64) -> Record {
    let targets: BTreeSet<JudgementId> = targets.iter().map(|n| jid(*n)).collect();
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(targets).unwrap(),
        actor(),
        JudgedAt::new(at),
        NonBlank::new("changed my mind").unwrap(),
    ))
}

fn denied(judgement: u128) -> DeclaredStatus {
    DeclaredStatus::Denied {
        judgement: jid(judgement),
    }
}

#[test]
fn a_revocation_of_an_unknown_id_is_set_aside_and_its_other_targets_count() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![
        deny(&shelf, 1, 1001, 10),
        deny(&shelf, 2, 1002, 10),
        revoke(2001, &[9999, 1001], 20),
    ]);
    assert_eq!(
        ledger.problems(),
        &[LedgerProblem::RevokesUnknown {
            revocation: jid(2001),
            target: jid(9999),
        }]
    );
    assert_eq!(ledger.status_of(docid(1)), DeclaredStatus::Pending);
    assert_eq!(ledger.status_of(docid(2)), denied(1002));
}

#[test]
fn a_revocation_of_a_revocation_is_set_aside_and_its_other_targets_count() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![
        deny(&shelf, 1, 1001, 10),
        deny(&shelf, 2, 1002, 10),
        revoke(2001, &[1001], 20),
        revoke(2002, &[2001, 1002], 30),
    ]);
    assert_eq!(
        ledger.problems(),
        &[LedgerProblem::RevokesRevocation {
            revocation: jid(2002),
            target: jid(2001),
        }]
    );
    // 2001 is not withdrawn: FR-1's denial stays revoked.
    assert_eq!(ledger.status_of(docid(1)), DeclaredStatus::Pending);
    assert_eq!(ledger.status_of(docid(2)), DeclaredStatus::Pending);
}

#[test]
fn a_revocation_of_a_conflicted_id_is_set_aside_and_its_other_targets_count() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![
        // An earlier denial of FR-1 that nothing touches.
        deny(&shelf, 1, 1000, 5),
        // Two different records with the id 1001.
        deny(&shelf, 1, 1001, 10),
        deny(&shelf, 1, 1001, 20),
        deny(&shelf, 2, 1002, 10),
        revoke(2001, &[1001, 1002], 30),
    ]);
    let problems = ledger.problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, LedgerProblem::Conflict { id, .. } if *id == jid(1001)))
    );
    assert!(problems.contains(&LedgerProblem::RevokesConflicted {
        revocation: jid(2001),
        target: jid(1001),
    }));
    // The conflicted records are not used, and the revocation withdrew nothing of
    // FR-1: the earlier denial stands.
    assert_eq!(ledger.status_of(docid(1)), denied(1000));
    assert!(!ledger.revoked.contains(&jid(1001)));
    assert_eq!(ledger.status_of(docid(2)), DeclaredStatus::Pending);
}
