//! Tests of how an exemption is read: in the place of a `NotCovered`, or unneeded
//! (reached now, its cover gone, the document not valid or decided against).

use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::exemption::CoverMiss;
use crate::domain::shelf::fixtures_tests::{
    actor, decls, deny, docid, fr, jid, ledger, list, reading, spaces, text,
};
use crate::domain::shelf::ledger::{Exempt, ExemptOf, Record, Revocation};
use crate::domain::shelf::schema::{BookSchemaDecl, CoverDecl, CoverSeverity, Schema};

use super::*;

/// The exemption `judgement` of `doc` from the link `link` of `book`.
fn exempt_from(judgement: u128, doc: u128, book: &str, link: &str) -> Record {
    Record::Exempt(Exempt::new(
        jid(judgement),
        ExemptOf::new(
            docid(doc),
            BookName::new(book).unwrap(),
            EntryName::new(link).unwrap(),
        ),
        actor(),
        JudgedAt::new(50),
        NonBlank::new("kept on purpose").unwrap(),
    ))
}

#[test]
fn an_exemption_from_a_link_into_another_book_is_unneeded() {
    // The design's `specs` covers the specs; FR-1 is a finding.
    let mut decls: Vec<BookSchemaDecl> = decls();
    decls
        .iter_mut()
        .find(|b| b.name == "design")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == "specs"))
        .expect("the fixtures declare it")
        .cover = Some(CoverDecl::Truth(true));
    let schema = Schema::validate(decls, &spaces()).unwrap();
    let shelf = shelf_of(&schema, vec![fr(1, &[])]);
    let l = ledger(vec![exempt_from(2001, 1, "design", "specs")]);
    assert_eq!(
        notices(&shelf, &l, 1),
        [Notice::ExemptionUnneeded {
            book: BookName::new("design").unwrap(),
            link: EntryName::new("specs").unwrap(),
            exemption: jid(2001),
            why: Unneeded::Cover(CoverMiss::OtherBook {
                target: BookName::new("spec").unwrap(),
                doc_book: BookName::new("finding").unwrap(),
            }),
        }]
    );
}

#[test]
fn two_exemptions_of_one_document_from_one_link_are_told_as_repeated() {
    let shelf = shelf_of(&covering(), vec![fr(1, &[])]);
    let l = ledger(vec![
        exempt_from(2001, 1, "spec", "grounds"),
        exempt_from(2002, 1, "spec", "grounds"),
    ]);
    let (book, link) = grounds();
    let exempt = |n| Notice::Exempt {
        book: book.clone(),
        link: link.clone(),
        severity: CoverSeverity::Error,
        exemption: jid(n),
    };
    assert_eq!(
        notices(&shelf, &l, 1),
        [
            exempt(2001),
            exempt(2002),
            Notice::ExemptionRepeated {
                book: book.clone(),
                link: link.clone(),
                exemptions: TwoOrMore::new(jid(2001), jid(2002), Vec::new()),
            },
        ]
    );
}

/// The fixtures' schema with the spec's `grounds` covering the findings as `cover`
/// says (none: declared with no cover).
fn schema_with(cover: Option<CoverDecl>) -> Schema {
    let mut decls: Vec<BookSchemaDecl> = decls();
    decls
        .iter_mut()
        .find(|b| b.name == "spec")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == "grounds"))
        .expect("the fixtures declare it")
        .cover = cover;
    Schema::validate(decls, &spaces()).unwrap()
}

/// The fixtures' schema with no `grounds` link at all.
fn schema_without_grounds() -> Schema {
    let mut decls: Vec<BookSchemaDecl> = decls();
    decls
        .iter_mut()
        .find(|b| b.name == "spec")
        .expect("the fixtures declare it")
        .links
        .retain(|l| l.name != "grounds");
    Schema::validate(decls, &spaces()).unwrap()
}

fn covering() -> Schema {
    schema_with(Some(CoverDecl::Truth(true)))
}

fn shelf_of(schema: &Schema, readings: Vec<DocReading>) -> Shelf {
    Shelf::build(schema, readings, &ArtifactReadings::new())
}

/// A spec `SP-<n>` grounded in these findings.
fn sp(n: u128, grounds: &[&str]) -> DocReading {
    reading(
        &format!("specs/sp-{n}.md"),
        n,
        &format!("SP-{n}"),
        &[("title", text("spec")), ("grounds", list(grounds))],
    )
}

fn grounds() -> (BookName, EntryName) {
    (
        BookName::new("spec").unwrap(),
        EntryName::new("grounds").unwrap(),
    )
}

/// The exemption 2001 of `doc` from `spec.grounds`.
fn exempt(doc: u128) -> Record {
    let (book, link) = grounds();
    Record::Exempt(Exempt::new(
        jid(2001),
        ExemptOf::new(docid(doc), book, link),
        actor(),
        JudgedAt::new(50),
        NonBlank::new("kept on purpose").unwrap(),
    ))
}

fn notices(shelf: &Shelf, ledger: &Ledger, n: u128) -> Vec<Notice> {
    assess(shelf, ledger, JudgedAt::new(100))
        .notices_of(docid(n))
        .to_vec()
}

fn unneeded(why: Unneeded) -> Notice {
    let (book, link) = grounds();
    Notice::ExemptionUnneeded {
        book,
        link,
        exemption: jid(2001),
        why,
    }
}

#[test]
fn an_exempt_document_is_not_uncovered_by_that_link() {
    let shelf = shelf_of(&covering(), vec![fr(1, &[])]);
    let before = notices(&shelf, &ledger(vec![]), 1);
    assert!(
        matches!(before.as_slice(), [Notice::NotCovered { .. }]),
        "{before:?}"
    );
    let (book, link) = grounds();
    assert_eq!(
        notices(&shelf, &ledger(vec![exempt(1)]), 1),
        [Notice::Exempt {
            book,
            link,
            severity: CoverSeverity::Error,
            exemption: jid(2001),
        }]
    );
}

#[test]
fn an_exemption_of_a_document_the_link_reaches_is_unneeded() {
    let shelf = shelf_of(&covering(), vec![fr(1, &[]), sp(2, &["FR-1"])]);
    assert_eq!(
        notices(&shelf, &ledger(vec![exempt(1)]), 1),
        [unneeded(Unneeded::Reached {
            by: NonEmptyVec::new(docid(2), Vec::new()),
        })]
    );
}

#[test]
fn an_exemption_whose_cover_is_gone_is_unneeded_with_why() {
    let readings = || vec![fr(1, &[])];
    let no_cover = shelf_of(&schema_with(None), readings());
    assert_eq!(
        notices(&no_cover, &ledger(vec![exempt(1)]), 1),
        [unneeded(Unneeded::Cover(CoverMiss::NoCover))]
    );
    let false_cover = shelf_of(&schema_with(Some(CoverDecl::Truth(false))), readings());
    assert_eq!(
        notices(&false_cover, &ledger(vec![exempt(1)]), 1),
        [unneeded(Unneeded::Cover(CoverMiss::NoCover))]
    );
    let no_link = shelf_of(&schema_without_grounds(), readings());
    assert_eq!(
        notices(&no_link, &ledger(vec![exempt(1)]), 1),
        [unneeded(Unneeded::Cover(CoverMiss::NotDeclared))]
    );
}

#[test]
fn an_exemption_of_a_document_decided_against_or_not_valid_is_unneeded() {
    let shelf = shelf_of(&covering(), vec![fr(1, &[])]);
    let denied = ledger(vec![deny(&shelf, 1, 1001, 10), exempt(1)]);
    assert_eq!(
        notices(&shelf, &denied, 1),
        [unneeded(Unneeded::DecidedAgainst)]
    );
    // 9 has no file and no decision: its notice is told all the same.
    let assessment = assess(&shelf, &ledger(vec![exempt(9)]), JudgedAt::new(100));
    let noticed: Vec<(DocId, Vec<Notice>)> = assessment
        .noticed()
        .map(|(id, n)| (id, n.to_vec()))
        .filter(|(id, _)| *id == docid(9))
        .collect();
    assert_eq!(
        noticed,
        [(
            docid(9),
            vec![unneeded(Unneeded::NotValid(FailureTag::Gone))]
        )]
    );
}

#[test]
fn an_exemption_withdrawn_leaves_the_gap_as_it_was() {
    let shelf = shelf_of(&covering(), vec![fr(1, &[])]);
    let withdrawn = ledger(vec![
        exempt(1),
        Record::Revocation(Revocation::new(
            jid(3001),
            NonEmptySet::from_set([jid(2001)].into()).unwrap(),
            actor(),
            JudgedAt::new(60),
            NonBlank::new("it is to be grounded").unwrap(),
        )),
    ]);
    let after = notices(&shelf, &withdrawn, 1);
    assert!(
        matches!(after.as_slice(), [Notice::NotCovered { .. }]),
        "{after:?}"
    );
}

#[test]
fn an_exemption_changes_no_freshness() {
    let shelf = shelf_of(&covering(), vec![fr(1, &[])]);
    let assessment = assess(&shelf, &ledger(vec![exempt(1)]), JudgedAt::new(100));
    assert_eq!(assessment.standing_of(docid(1)), Standing::Pending);
}
