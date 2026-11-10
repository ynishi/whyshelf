//! Tests of `Notice::NotCovered`: a document of a book a covering link (`cover =
//! true`) points into, that no pending or approved document reaches through it.

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::fixtures_tests::{
    approved, decls, deny, deprecate, docid, fr, ledger, list, reading, retire, spaces, text,
};
use crate::domain::shelf::ledger::{JudgedAt, Ledger, Record};
use crate::domain::shelf::schema::{BookSchemaDecl, CoverDecl, CoverSeverity, Schema};

use super::*;

/// The fixtures' schema with `cover = true` on these links, as (book, link).
fn schema_covering(links: &[(&str, &str)]) -> Schema {
    let links: Vec<_> = links
        .iter()
        .map(|(book, link)| (*book, *link, CoverDecl::Truth(true)))
        .collect();
    schema_covering_as(&links)
}

/// The fixtures' schema with these `cover`s, as (book, link, cover).
fn schema_covering_as(links: &[(&str, &str, CoverDecl)]) -> Schema {
    let mut decls: Vec<BookSchemaDecl> = decls();
    for (book, link, cover) in links {
        decls
            .iter_mut()
            .find(|b| b.name == *book)
            .and_then(|b| b.links.iter_mut().find(|l| l.name == *link))
            .expect("the fixtures declare it")
            .cover = Some(cover.clone());
    }
    Schema::validate(decls, &spaces()).unwrap()
}

/// A shelf where a spec's `grounds` covers the findings.
fn shelf(readings: Vec<DocReading>) -> Shelf {
    shelf_of(&schema_covering(&[("spec", "grounds")]), readings)
}

fn shelf_of(schema: &Schema, readings: Vec<DocReading>) -> Shelf {
    Shelf::build(schema, readings, &ArtifactReadings::new())
}

/// A spec `SP-<n>` with identity `n`, grounded in these findings.
fn sp(n: u128, grounds: &[&str]) -> DocReading {
    reading(
        &format!("specs/sp-{n}.md"),
        n,
        &format!("SP-{n}"),
        &[("title", text("spec")), ("grounds", list(grounds))],
    )
}

/// The `NotCovered` notices of `n`, as (book, link).
fn not_covered(shelf: &Shelf, ledger: &Ledger, n: u128) -> BTreeSet<(String, String)> {
    assess(shelf, ledger, JudgedAt::new(100))
        .notices_of(docid(n))
        .iter()
        .filter_map(|notice| match notice {
            Notice::NotCovered { book, link, .. } => Some((book.to_string(), link.to_string())),
            _ => None,
        })
        .collect()
}

fn grounds() -> BTreeSet<(String, String)> {
    BTreeSet::from([("spec".to_owned(), "grounds".to_owned())])
}

#[test]
fn a_covered_document_gets_no_notice() {
    let shelf = shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]);
    assert!(not_covered(&shelf, &ledger(vec![]), 1).is_empty());
    let both = approved(&shelf, &[1, 2]);
    assert!(not_covered(&shelf, &both, 1).is_empty());
}

#[test]
fn an_uncovered_pending_document_gets_the_notice() {
    let shelf = shelf(vec![fr(1, &[])]);
    let none = ledger(vec![]);
    assert_eq!(not_covered(&shelf, &none, 1), grounds());
    assert_eq!(
        assess(&shelf, &none, JudgedAt::new(100)).standing_of(docid(1)),
        Standing::Pending
    );
}

#[test]
fn an_uncovered_approved_document_gets_the_notice_and_stays_fresh() {
    let shelf = shelf(vec![fr(1, &[])]);
    let ledger = approved(&shelf, &[1]);
    assert_eq!(not_covered(&shelf, &ledger, 1), grounds());
    assert_eq!(
        assess(&shelf, &ledger, JudgedAt::new(100)).standing_of(docid(1)),
        Standing::Approved(Freshness::Fresh)
    );
}

#[test]
fn a_source_that_is_denied_retired_or_deprecated_does_not_count() {
    type Judge = fn(&Shelf) -> Record;
    let cases: [(&str, Judge); 3] = [
        ("denied", |s| deny(s, 2, 1002, 10)),
        ("retired", |s| retire(s, 2, 1002, 10)),
        ("deprecated", |s| deprecate(s, 2, 1002, 10)),
    ];
    for (what, judge) in cases {
        let shelf = shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]);
        let ledger = ledger(vec![judge(&shelf)]);
        assert_eq!(not_covered(&shelf, &ledger, 1), grounds(), "a {what} spec");
    }
}

#[test]
fn a_target_that_is_denied_retired_or_deprecated_is_not_expected() {
    type Judge = fn(&Shelf) -> Record;
    let cases: [(&str, Judge); 3] = [
        ("denied", |s| deny(s, 1, 1001, 10)),
        ("retired", |s| retire(s, 1, 1001, 10)),
        ("deprecated", |s| deprecate(s, 1, 1001, 10)),
    ];
    for (what, judge) in cases {
        let shelf = shelf(vec![fr(1, &[])]);
        let ledger = ledger(vec![judge(&shelf)]);
        assert!(
            not_covered(&shelf, &ledger, 1).is_empty(),
            "a {what} finding"
        );
    }
}

#[test]
fn a_link_that_cannot_be_followed_does_not_count() {
    let shelf = shelf(vec![fr(1, &[]), sp(2, &["FR-9"])]);
    assert_eq!(not_covered(&shelf, &ledger(vec![]), 1), grounds());
}

#[test]
fn two_covering_links_into_one_book_are_each_expected() {
    let schema = schema_covering(&[("spec", "grounds"), ("finding", "base")]);
    let both = BTreeSet::from([
        ("finding".to_owned(), "base".to_owned()),
        ("spec".to_owned(), "grounds".to_owned()),
    ]);

    let alone = shelf_of(&schema, vec![fr(1, &[])]);
    assert_eq!(not_covered(&alone, &ledger(vec![]), 1), both);

    // A spec grounds FR-1: only `base` is still expected.
    let grounded = shelf_of(&schema, vec![fr(1, &[]), sp(3, &["FR-1"])]);
    assert_eq!(
        not_covered(&grounded, &ledger(vec![]), 1),
        BTreeSet::from([("finding".to_owned(), "base".to_owned())])
    );

    // FR-2 stands on FR-1 too: FR-1 is covered by both, and FR-2 by neither.
    let full = shelf_of(
        &schema,
        vec![fr(1, &[]), fr(2, &["FR-1"]), sp(3, &["FR-1"])],
    );
    assert!(not_covered(&full, &ledger(vec![]), 1).is_empty());
    assert_eq!(not_covered(&full, &ledger(vec![]), 2), both);
}

#[test]
fn each_notice_carries_the_strength_its_link_declares() {
    let schema = schema_covering_as(&[
        ("spec", "grounds", CoverDecl::Word("warn".to_owned())),
        ("finding", "base", CoverDecl::Truth(true)),
    ]);
    let shelf = shelf_of(&schema, vec![fr(1, &[])]);
    let strengths: BTreeSet<(String, CoverSeverity)> =
        assess(&shelf, &ledger(vec![]), JudgedAt::new(100))
            .notices_of(docid(1))
            .iter()
            .filter_map(|notice| match notice {
                Notice::NotCovered { link, severity, .. } => Some((link.to_string(), *severity)),
                _ => None,
            })
            .collect();
    assert_eq!(
        strengths,
        BTreeSet::from([
            ("base".to_owned(), CoverSeverity::Error),
            ("grounds".to_owned(), CoverSeverity::Warn),
        ])
    );
    // Whatever the strength, the freshness is unchanged.
    assert_eq!(
        assess(&shelf, &ledger(vec![]), JudgedAt::new(100)).standing_of(docid(1)),
        Standing::Pending
    );
}

#[test]
fn a_link_without_cover_expects_nothing() {
    let schema = Schema::validate(decls(), &spaces()).unwrap();
    let shelf = shelf_of(&schema, vec![fr(1, &[]), fr(2, &[])]);
    let assessment = assess(&shelf, &ledger(vec![]), JudgedAt::new(100));
    for n in [1, 2] {
        assert!(assessment.notices_of(docid(n)).is_empty(), "FR-{n}");
    }
}
