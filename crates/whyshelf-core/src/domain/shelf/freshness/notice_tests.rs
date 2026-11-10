//! Tests of the notices of a document: what is worth knowing and changes no
//! freshness.

use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, docid, fr, fr_titled, ledger, list, reading, shelf, text,
};
use crate::domain::shelf::ledger::JudgedAt;

use super::*;

/// A finding `FR-<n>` with identity `n` that informs of these findings (its
/// `related` link).
fn informing(n: u128, related: &[&str]) -> DocReading {
    reading(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        &[("title", text("title")), ("related", list(related))],
    )
}

fn informs_not_fresh(notices: &[Notice]) -> Vec<(DocId, StandingKind)> {
    notices
        .iter()
        .filter_map(|n| match n {
            Notice::InformsNotFresh {
                link,
                target,
                standing,
            } => {
                assert_eq!(link.as_str(), "related");
                Some((*target, *standing))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn an_informs_link_to_a_pending_document_is_a_notice_carrying_its_standing() {
    let shelf = shelf(vec![fr(1, &[]), informing(2, &["FR-1"])]);
    let ledger = approved(&shelf, &[2]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    assert_eq!(
        informs_not_fresh(assessment.notices_of(docid(2))),
        vec![(docid(1), StandingKind::Pending)]
    );
    // The notice changes no freshness.
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );
}

#[test]
fn an_informs_link_to_a_denied_document_carries_denied() {
    let shelf = shelf(vec![fr(1, &[]), informing(2, &["FR-1"])]);
    let ledger = ledger(vec![deny(&shelf, 1, 1001, 10)]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    assert_eq!(
        informs_not_fresh(assessment.notices_of(docid(2))),
        vec![(docid(1), StandingKind::Denied)]
    );
}

#[test]
fn an_informs_link_to_a_degraded_approval_carries_its_level() {
    let at_approval = shelf(vec![fr(1, &[]), informing(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), informing(2, &["FR-1"])]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));
    assert_eq!(
        informs_not_fresh(assessment.notices_of(docid(2))),
        vec![(docid(1), StandingKind::Approved(FreshnessLevel::Stale))]
    );
    // An `Informs` link carries no staleness.
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );
}

#[test]
fn an_informs_link_to_a_fresh_approval_is_no_notice() {
    let shelf = shelf(vec![fr(1, &[]), informing(2, &["FR-1"])]);
    let ledger = approved(&shelf, &[1, 2]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    assert!(assessment.notices_of(docid(2)).is_empty());
}

#[test]
fn an_informs_link_that_cannot_be_followed_is_a_notice() {
    let shelf = shelf(vec![informing(2, &["FR-9"])]);
    let ledger = approved(&shelf, &[2]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    let notices = assessment.notices_of(docid(2));
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        matches!(
            &notices[0],
            Notice::InformsUnresolved { link, unresolved }
                if link.as_str() == "related" && unresolved.written() == "FR-9"
        ),
        "{notices:?}"
    );
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );
}

#[test]
fn an_upstream_whose_approval_only_expired_is_a_notice_and_no_via() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let first = approve(&shelf, &ledger(vec![]), 1, 1001, 10, Some(50));
    let second = approve(&shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let ledger = ledger(vec![first, second]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(60));

    let Standing::Approved(Freshness::Degraded(d)) = assessment.standing_of(docid(1)) else {
        panic!("the upstream lapsed");
    };
    assert_eq!(d.level(), Severity::Expired);

    // The downstream stays fresh: no `Via` is made of an expiry.
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );
    assert!(assessment.roots_of(docid(2)).is_empty());
    let notices = assessment.notices_of(docid(2));
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        matches!(
            &notices[0],
            Notice::UpstreamExpired { pin, since }
                if pin.target() == docid(1) && *since == JudgedAt::new(50)
        ),
        "{notices:?}"
    );
}

#[test]
fn notices_are_given_whatever_the_documents_own_standing_is() {
    // FR-2 is pending, FR-3 is denied: each still has its notices.
    let shelf = shelf(vec![
        fr(1, &[]),
        informing(2, &["FR-1", "FR-9"]),
        informing(3, &["FR-1"]),
    ]);
    let ledger = ledger(vec![deny(&shelf, 3, 1003, 10)]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));

    assert_eq!(assessment.standing_of(docid(2)), Standing::Pending);
    let notices = assessment.notices_of(docid(2));
    assert_eq!(
        informs_not_fresh(notices),
        vec![(docid(1), StandingKind::Pending)]
    );
    assert!(
        notices
            .iter()
            .any(|n| matches!(n, Notice::InformsUnresolved { unresolved, .. }
                if unresolved.written() == "FR-9")),
        "{notices:?}"
    );

    assert_eq!(assessment.standing_of(docid(3)), Standing::Denied);
    assert_eq!(
        informs_not_fresh(assessment.notices_of(docid(3))),
        vec![(docid(1), StandingKind::Pending)]
    );
}
