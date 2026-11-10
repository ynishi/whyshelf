//! Tests of freshness.

use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, deprecate, docid, fr, fr_titled, ledger, retire, shelf,
};
use crate::domain::shelf::ledger::JudgedAt;

use super::*;

fn standing_of(assessment: &Assessment, n: u128) -> Standing {
    assessment.standings().get(&docid(n)).cloned().unwrap()
}

#[test]
fn an_approval_of_what_is_unchanged_holds() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    let assessment = assess(&at_approval, &ledger, JudgedAt::new(100));
    assert_eq!(
        standing_of(&assessment, 1),
        Standing::Approved(Freshness::Fresh)
    );
    assert_eq!(
        standing_of(&assessment, 2),
        Standing::Approved(Freshness::Fresh)
    );
}

#[test]
fn an_edited_upstream_degrades_what_stands_on_it_and_names_where_it_starts() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));

    let Standing::Approved(Freshness::Degraded { level, causes }) = standing_of(&assessment, 1)
    else {
        panic!("the edited document is not what was approved");
    };
    assert_eq!(level, Severity::Stale);
    assert!(
        causes
            .iter()
            .any(|cause| matches!(cause, Cause::SelfChanged { .. }))
    );

    let Standing::Approved(Freshness::Degraded { level, causes }) = standing_of(&assessment, 2)
    else {
        panic!("what stands on an edited document is degraded");
    };
    assert_eq!(level, Severity::Stale);
    assert!(causes.iter().any(
        |cause| matches!(cause, Cause::UpstreamChanged { pin, .. } if pin.target() == docid(1))
    ));
    let roots = assessment.roots_of(docid(2));
    assert!(
        roots
            .iter()
            .all(|root| !matches!(root.cause, Cause::Via { .. }))
    );
    assert!(roots.iter().any(|root| root.path.last() == Some(&docid(1))));
}

#[test]
fn an_approval_lapses_at_its_expiry_and_not_before() {
    let shelf = shelf(vec![fr(1, &[])]);
    let ledger = ledger(vec![approve(
        &shelf,
        &ledger(vec![]),
        1,
        1001,
        10,
        Some(50),
    )]);
    let before = assess(&shelf, &ledger, JudgedAt::new(49));
    assert_eq!(
        standing_of(&before, 1),
        Standing::Approved(Freshness::Fresh)
    );
    let at = assess(&shelf, &ledger, JudgedAt::new(50));
    let Standing::Approved(Freshness::Degraded { level, causes }) = standing_of(&at, 1) else {
        panic!("an approval lapses when now reaches its expiry");
    };
    assert_eq!(level, Severity::Expired);
    assert!(
        causes
            .iter()
            .any(|cause| matches!(cause, Cause::Expired { .. }))
    );
}

#[test]
fn a_document_that_is_gone_after_its_approval_is_broken() {
    let at_approval = shelf(vec![fr(1, &[])]);
    let ledger = approved(&at_approval, &[1]);
    let now = shelf(vec![]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));
    let Standing::Approved(Freshness::Degraded { level, causes }) = standing_of(&assessment, 1)
    else {
        panic!("an approved document with no file does not hold");
    };
    assert_eq!(level, Severity::Broken);
    assert!(causes.iter().any(|cause| matches!(cause, Cause::SelfGone)));
}

#[test]
fn the_other_decisions_are_standings_without_a_freshness() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[]), fr(3, &[])]);
    let ledger = ledger(vec![
        deny(&shelf, 1, 1001, 10),
        deprecate(&shelf, 2, 1002, 10),
        retire(&shelf, 3, 1003, 10),
    ]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    assert_eq!(standing_of(&assessment, 1), Standing::Denied);
    assert_eq!(
        standing_of(&assessment, 2),
        Standing::Deprecated {
            successors: Successors::None
        }
    );
    assert_eq!(standing_of(&assessment, 3), Standing::Retired);
}
