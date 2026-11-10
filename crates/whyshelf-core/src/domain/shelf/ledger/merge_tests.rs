//! Tests of DEC-1: two branches that judge separately merge without a conflict.
//!
//! Two worktrees start from one common set of records. Each adds records of its own,
//! with identities of its own; the merge is the union of the records, and
//! [`Ledger::build`] of it is the ledger of the merged branch.

use crate::domain::shelf::fixtures_tests::{
    approve, deny, deprecate, docid, fr, jid, ledger, shelf,
};

use super::*;

/// FR-1 and FR-2 approved at 10, and FR-3 and FR-4 that nobody judged yet.
fn common() -> (Shelf, Vec<Record>) {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[]), fr(3, &[]), fr(4, &[])]);
    let none = ledger(vec![]);
    let records = vec![
        approve(&shelf, &none, 1, 1001, 10, None),
        approve(&shelf, &none, 2, 1002, 10, None),
    ];
    (shelf, records)
}

/// The records of a branch: the common ones, then its own.
fn branch(common: &[Record], own: Vec<Record>) -> Vec<Record> {
    let mut records = common.to_vec();
    records.extend(own);
    records
}

/// The merge of two branches: every record of either, the common ones twice.
fn merge(one: &[Record], other: &[Record]) -> Vec<Record> {
    let mut records = one.to_vec();
    records.extend(other.iter().cloned());
    records
}

#[test]
fn two_branches_that_judged_separately_merge_with_every_record_and_no_problem() {
    let (shelf, common) = common();
    let none = ledger(vec![]);
    // One branch approves FR-3 and denies FR-2; the other denies FR-4 and, later,
    // deprecates FR-2.
    let one = branch(
        &common,
        vec![
            approve(&shelf, &none, 3, 2001, 20, None),
            deny(&shelf, 2, 2002, 30),
        ],
    );
    let other = branch(
        &common,
        vec![deny(&shelf, 4, 3001, 25), deprecate(&shelf, 2, 3002, 40)],
    );

    let merged = ledger(merge(&one, &other));
    assert!(merged.problems().is_empty(), "{:?}", merged.problems());
    assert_eq!(merged.decisions.len(), 6);
    for n in [1001, 1002, 2001, 2002, 3001, 3002] {
        assert!(merged.decisions.contains_key(&jid(n)), "{n} counts");
    }

    assert!(matches!(
        merged.status_of(docid(1)),
        DeclaredStatus::Approved { judgement, .. } if judgement == jid(1001)
    ));
    assert!(matches!(
        merged.status_of(docid(3)),
        DeclaredStatus::Approved { judgement, .. } if judgement == jid(2001)
    ));
    assert_eq!(
        merged.status_of(docid(4)),
        DeclaredStatus::Denied {
            judgement: jid(3001)
        }
    );
    // Both judged FR-2: the later decision stands.
    assert_eq!(
        merged.status_of(docid(2)),
        DeclaredStatus::Deprecated {
            judgement: jid(3002)
        }
    );

    // The order of the union does not matter.
    assert_eq!(ledger(merge(&other, &one)), merged);
}

#[test]
fn of_two_decisions_at_one_time_the_larger_id_stands() {
    let (shelf, common) = common();
    let one = branch(&common, vec![deprecate(&shelf, 2, 3002, 30)]);
    let other = branch(&common, vec![deny(&shelf, 2, 2002, 30)]);
    let merged = ledger(merge(&one, &other));
    assert!(merged.problems().is_empty(), "{:?}", merged.problems());
    assert_eq!(
        merged.status_of(docid(2)),
        DeclaredStatus::Deprecated {
            judgement: jid(3002)
        }
    );

    // The time comes first: an earlier decision with the larger id does not stand.
    let one = branch(&common, vec![deprecate(&shelf, 2, 3002, 30)]);
    let other = branch(&common, vec![deny(&shelf, 2, 2002, 31)]);
    assert_eq!(
        ledger(merge(&one, &other)).status_of(docid(2)),
        DeclaredStatus::Denied {
            judgement: jid(2002)
        }
    );
}

#[test]
fn the_same_record_written_on_both_branches_is_one_record_not_a_conflict() {
    let (shelf, common) = common();
    let none = ledger(vec![]);
    // Both branches wrote the same approval of FR-3: one id, one content.
    let same = approve(&shelf, &none, 3, 2001, 20, None);
    let one = branch(&common, vec![same.clone()]);
    let other = branch(&common, vec![same.clone(), deny(&shelf, 4, 3001, 25)]);

    let merged = ledger(merge(&one, &other));
    assert!(merged.problems().is_empty(), "{:?}", merged.problems());
    assert_eq!(merged.decisions.len(), 4);
    assert!(matches!(
        merged.status_of(docid(3)),
        DeclaredStatus::Approved { judgement, .. } if judgement == jid(2001)
    ));
    assert_eq!(
        merged,
        ledger(branch(&common, vec![same, deny(&shelf, 4, 3001, 25)]))
    );
}
