//! Tests of the successors of a deprecated document ([`Successors::of`]) and of what
//! they make of the freshness of what stands on it.
//!
//! The check that `judge deprecate --successor` makes of a named successor is a use
//! case, so its tests are in `app/shelf/successor_tests.rs`.

use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::fixtures_tests::{
    approve, deny, deprecate, docid, fr, ledger, list, reading, retire, shelf, text,
};
use crate::domain::shelf::ledger::{JudgedAt, Record};

use super::*;

/// A finding `FR-<n>` with identity `n`, a title and these links.
fn finding(n: u128, links: &[(&str, &[&str])]) -> DocReading {
    let mut extra = vec![("title", text("title"))];
    for (name, keys) in links {
        extra.push((*name, list(keys)));
    }
    reading(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        &extra,
    )
}

/// A finding that replaces these findings (its `supersedes` link).
fn replacing(n: u128, replaced: &[&str]) -> DocReading {
    finding(n, &[("supersedes", replaced)])
}

fn some(ids: &[u128]) -> Successors {
    let ids: Vec<DocId> = ids.iter().map(|n| docid(*n)).collect();
    Successors::Some(NonEmptyVec::from_vec(ids).unwrap())
}

#[test]
fn a_pending_replacer_is_a_successor() {
    let shelf = shelf(vec![fr(1, &[]), replacing(2, &["FR-1"])]);
    let ledger = ledger(vec![deprecate(&shelf, 1, 1001, 10)]);
    assert_eq!(Successors::of(&shelf, &ledger, docid(1)), some(&[2]));
}

#[test]
fn an_approved_replacer_is_a_successor() {
    let shelf = shelf(vec![fr(1, &[]), replacing(2, &["FR-1"])]);
    let none = ledger(vec![]);
    let ledger = ledger(vec![
        deprecate(&shelf, 1, 1001, 10),
        approve(&shelf, &none, 2, 1002, 10, None),
    ]);
    assert_eq!(Successors::of(&shelf, &ledger, docid(1)), some(&[2]));
}

#[test]
fn every_pending_or_approved_replacer_is_counted_smallest_first() {
    let shelf = shelf(vec![
        fr(1, &[]),
        replacing(2, &["FR-1"]),
        replacing(3, &["FR-1"]),
        fr(4, &[]),
    ]);
    let none = ledger(vec![]);
    let ledger = ledger(vec![
        deprecate(&shelf, 1, 1001, 10),
        approve(&shelf, &none, 3, 1003, 10, None),
    ]);
    assert_eq!(Successors::of(&shelf, &ledger, docid(1)), some(&[2, 3]));
}

#[test]
fn a_document_nothing_replaces_has_no_successor() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![deprecate(&shelf, 1, 1001, 10)]);
    assert_eq!(Successors::of(&shelf, &ledger, docid(1)), Successors::None);
}

#[test]
fn a_denied_retired_or_deprecated_replacer_is_no_successor() {
    type Judge = fn(&Shelf) -> Record;
    let cases: [(&str, Judge); 3] = [
        ("denied", |s| deny(s, 2, 1002, 10)),
        ("retired", |s| retire(s, 2, 1002, 10)),
        ("deprecated", |s| deprecate(s, 2, 1002, 10)),
    ];
    for (what, judge) in cases {
        let shelf = shelf(vec![fr(1, &[]), replacing(2, &["FR-1"])]);
        let ledger = ledger(vec![deprecate(&shelf, 1, 1001, 10), judge(&shelf)]);
        assert_eq!(
            Successors::of(&shelf, &ledger, docid(1)),
            Successors::None,
            "a {what} replacer"
        );
    }
}

#[test]
fn the_chain_of_successors_is_not_followed() {
    // FR-2 replaces FR-1 and is itself deprecated; FR-3 replaces FR-2.
    let shelf = shelf(vec![
        fr(1, &[]),
        replacing(2, &["FR-1"]),
        replacing(3, &["FR-2"]),
    ]);
    let ledger = ledger(vec![
        deprecate(&shelf, 1, 1001, 10),
        deprecate(&shelf, 2, 1002, 10),
    ]);
    assert_eq!(Successors::of(&shelf, &ledger, docid(1)), Successors::None);
    assert_eq!(Successors::of(&shelf, &ledger, docid(2)), some(&[3]));

    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    assert_eq!(
        assessment.standing_of(docid(1)),
        Standing::Deprecated {
            successors: Successors::None
        }
    );
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Deprecated {
            successors: some(&[3])
        }
    );
}

/// FR-2 stands on FR-1, both approved at 10, and FR-1 is deprecated at 20; `others`
/// are on the shelf beside them.
fn downstream_of_a_deprecation(others: Vec<DocReading>) -> (Shelf, Vec<Cause>, Severity) {
    let mut readings = vec![fr(1, &[]), fr(2, &["FR-1"])];
    readings.extend(others);
    let shelf = shelf(readings);
    let first = approve(&shelf, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let ledger = ledger(vec![first, second, deprecate(&shelf, 1, 1003, 20)]);
    let Standing::Approved(Freshness::Degraded(d)) =
        assess(&shelf, &ledger, JudgedAt::new(100)).standing_of(docid(2))
    else {
        panic!("what stands on a deprecated document does not hold");
    };
    (shelf, d.causes().clone().into_vec(), d.level())
}

#[test]
fn a_downstream_of_a_deprecation_with_a_successor_is_stale() {
    let (_, causes, level) = downstream_of_a_deprecation(vec![replacing(3, &["FR-1"])]);
    assert_eq!(level, Severity::Stale);
    assert!(
        causes.iter().any(|c| matches!(
            c,
            Cause::UpstreamDeclared {
                pin,
                status: UpstreamStatus::Deprecated { successors },
            } if pin.target() == docid(1) && *successors == some(&[3])
        )),
        "{causes:?}"
    );
}

#[test]
fn a_downstream_of_a_deprecation_with_no_successor_is_broken() {
    let (_, causes, level) = downstream_of_a_deprecation(vec![]);
    assert_eq!(level, Severity::Broken);
    assert!(
        causes.iter().any(|c| matches!(
            c,
            Cause::UpstreamDeclared {
                pin,
                status: UpstreamStatus::Deprecated {
                    successors: Successors::None
                },
            } if pin.target() == docid(1)
        )),
        "{causes:?}"
    );
}

#[test]
fn a_downstream_of_a_deprecation_whose_only_replacer_is_denied_is_broken() {
    let (shelf, _, _) = downstream_of_a_deprecation(vec![replacing(3, &["FR-1"])]);
    let first = approve(&shelf, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let ledger = ledger(vec![
        first,
        second,
        deprecate(&shelf, 1, 1003, 20),
        deny(&shelf, 3, 1004, 20),
    ]);
    let standing = assess(&shelf, &ledger, JudgedAt::new(100)).standing_of(docid(2));
    let Standing::Approved(Freshness::Degraded(d)) = standing else {
        panic!("what stands on a deprecated document does not hold");
    };
    assert_eq!(d.level(), Severity::Broken);
}
