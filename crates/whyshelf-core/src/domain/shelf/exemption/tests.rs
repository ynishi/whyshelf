//! Tests of the rule of an exemption: whether a link covers a document's book
//! ([`cover_of`]), what an exemption may be made of ([`frame_exempt`]), and that the
//! acceptance and the reading ([`exemption_reading`]) agree.

use std::error::Error as _;

use crate::domain::actor::Actor;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::fixtures_tests::{
    decls, deny, deprecate, docid, fr, jid, ledger, list, reading, retire, spaces, text,
};
use crate::domain::shelf::freshness::assess;
use crate::domain::shelf::ledger::{JudgedAt, Record};
use crate::domain::shelf::schema::{CoverDecl, CoverSeverity, Schema};

use super::*;

fn book(text: &str) -> BookName {
    BookName::new(text).unwrap()
}

fn link(text: &str) -> EntryName {
    EntryName::new(text).unwrap()
}

/// The fixtures' schema with `cover = true` on the spec's `grounds` (into the
/// findings) and on the design's `specs` (into the specs).
fn covering() -> Schema {
    let mut decls = decls();
    for (b, l) in [("spec", "grounds"), ("design", "specs")] {
        decls
            .iter_mut()
            .find(|d| d.name == b)
            .and_then(|d| d.links.iter_mut().find(|k| k.name == l))
            .expect("the fixtures declare it")
            .cover = Some(CoverDecl::Truth(true));
    }
    Schema::validate(decls, &spaces()).unwrap()
}

fn shelf(readings: Vec<DocReading>) -> Shelf {
    Shelf::build(&covering(), readings, &ArtifactReadings::new())
}

/// FR-1, a valid finding no spec grounds.
fn alone() -> Shelf {
    shelf(vec![fr(1, &[])])
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

/// The exemption `judgement` of `doc` from `spec.grounds`.
fn exempt(judgement: u128, doc: u128) -> Record {
    Record::Exempt(Exempt::new(
        jid(judgement),
        ExemptOf::new(docid(doc), book("spec"), link("grounds")),
        Actor::new("alice").unwrap(),
        JudgedAt::new(50),
        NonBlank::new("kept on purpose").unwrap(),
    ))
}

fn frame(
    shelf: &Shelf,
    l: &Ledger,
    doc: u128,
    b: &str,
    k: &str,
) -> Result<ExemptOf, ExemptRefusal> {
    frame_exempt(shelf, l, docid(doc), book(b), link(k))
}

/// The reading of `of`.
fn read(shelf: &Shelf, l: &Ledger, of: &ExemptOf) -> Result<CoverSeverity, Unneeded> {
    exemption_reading(shelf, l, of)
}

#[test]
fn what_counts_is_one_statement_for_the_ledger_and_the_standing() {
    // `Standing::stands` and the reading of the ledger both are `counts`.
    let shelf = shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]);
    for records in [
        vec![],
        vec![deny(&shelf, 2, 1002, 10)],
        vec![deprecate(&shelf, 2, 1002, 10)],
        vec![retire(&shelf, 2, 1002, 10)],
    ] {
        let l = ledger(records);
        let assessment = assess(&shelf, &l, JudgedAt::new(100));
        for n in [1, 2] {
            assert_eq!(
                assessment.standing_of(docid(n)).stands(),
                counts(Decided::from(&l.status_of(docid(n)))),
                "{n}"
            );
        }
    }
}

/// A refusal and its causes, outermost first, as the command line writes them.
fn chain(error: &ExemptRefusal) -> Vec<String> {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push(cause.to_string());
        source = cause.source();
    }
    chain
}

// The rule of the link.

#[test]
fn cover_of_tells_a_covering_link_into_the_documents_book_from_every_other() {
    let schema = covering();
    let finding = book("finding");
    assert_eq!(
        cover_of(&schema, &book("spec"), &link("grounds"), Some(&finding)),
        Ok(CoverSeverity::Error)
    );
    // Of a document whose book cannot be told, only the cover is asked.
    assert_eq!(
        cover_of(&schema, &book("spec"), &link("grounds"), None),
        Ok(CoverSeverity::Error)
    );
    assert_eq!(
        cover_of(&schema, &book("finding"), &link("base"), Some(&finding)),
        Err(CoverMiss::NoCover)
    );
    for (b, l) in [("spec", "nope"), ("nobook", "grounds"), ("spec", "tests")] {
        assert_eq!(
            cover_of(&schema, &book(b), &link(l), Some(&finding)),
            Err(CoverMiss::NotDeclared),
            "{b}.{l}"
        );
    }
    assert_eq!(
        cover_of(&schema, &book("design"), &link("specs"), Some(&finding)),
        Err(CoverMiss::OtherBook {
            target: book("spec"),
            doc_book: finding,
        })
    );
}

// What an exemption may be made of.

#[test]
fn an_exemption_of_a_valid_document_from_a_link_covering_its_book_is_framed() {
    let of = frame(&alone(), &ledger(vec![]), 1, "spec", "grounds").unwrap();
    assert_eq!(of, ExemptOf::new(docid(1), book("spec"), link("grounds")));
}

#[test]
fn an_exemption_from_a_warn_cover_is_framed_too() {
    let mut decls = decls();
    decls
        .iter_mut()
        .find(|d| d.name == "spec")
        .and_then(|d| d.links.iter_mut().find(|k| k.name == "grounds"))
        .unwrap()
        .cover = Some(CoverDecl::Word("warn".to_owned()));
    let schema = Schema::validate(decls, &spaces()).unwrap();
    let shelf = Shelf::build(&schema, vec![fr(1, &[])], &ArtifactReadings::new());
    assert!(frame(&shelf, &ledger(vec![]), 1, "spec", "grounds").is_ok());
}

#[test]
fn an_exemption_of_a_document_that_is_not_valid_is_refused_with_why_in_words() {
    // FR-2 has no title, which its book requires; 9 has no file at all.
    let shelf = shelf(vec![reading("findings/fr-2.md", 2, "FR-2", &[])]);
    for (n, tag) in [(2, FailureTag::Invalid), (9, FailureTag::Gone)] {
        assert_eq!(
            frame(&shelf, &ledger(vec![]), n, "spec", "grounds"),
            Err(ExemptRefusal::DocNotValid { id: docid(n), tag }),
            "{n}"
        );
    }
    let refused = frame(&shelf, &ledger(vec![]), 2, "spec", "grounds").unwrap_err();
    assert_eq!(
        chain(&refused),
        [format!(
            "{} is not a valid document now: its files are invalid",
            docid(2)
        )]
    );
    let refused = frame(&shelf, &ledger(vec![]), 9, "spec", "grounds").unwrap_err();
    assert_eq!(
        chain(&refused),
        [format!(
            "{} is not a valid document now: no file in the books' ranges carries it",
            docid(9)
        )]
    );
}

#[test]
fn an_exemption_from_a_link_that_covers_nothing_of_the_document_is_refused() {
    let cases = [
        ("spec", "nope", CoverMiss::NotDeclared),
        ("nobook", "grounds", CoverMiss::NotDeclared),
        ("spec", "tests", CoverMiss::NotDeclared),
        ("finding", "base", CoverMiss::NoCover),
        (
            "design",
            "specs",
            CoverMiss::OtherBook {
                target: book("spec"),
                doc_book: book("finding"),
            },
        ),
    ];
    for (b, l, miss) in cases {
        assert_eq!(
            frame(&alone(), &ledger(vec![]), 1, b, l),
            Err(ExemptRefusal::NotCovering {
                book: book(b),
                link: link(l),
                miss,
            }),
            "{b}.{l}"
        );
    }
}

#[test]
fn the_cause_of_a_link_that_covers_nothing_is_told_once() {
    let cases = [
        (
            "spec",
            "nope",
            "no link of that name into a book is declared",
        ),
        ("finding", "base", "the link is declared with no cover"),
        (
            "design",
            "specs",
            "the link points into `spec`, and the document is of `finding`",
        ),
    ];
    for (b, l, cause) in cases {
        let refused = frame(&alone(), &ledger(vec![]), 1, b, l).unwrap_err();
        assert_eq!(
            chain(&refused),
            [format!("{b}.{l} cannot exempt it"), cause.to_owned()],
            "{b}.{l}"
        );
    }
}

#[test]
fn a_second_exemption_of_the_same_document_from_the_same_link_is_refused() {
    let l = ledger(vec![exempt(2001, 1)]);
    let refused = frame(&alone(), &l, 1, "spec", "grounds");
    assert_eq!(
        refused,
        Err(ExemptRefusal::AlreadyExempt {
            book: book("spec"),
            link: link("grounds"),
            record: jid(2001),
        })
    );
    assert_eq!(
        chain(&refused.unwrap_err()),
        [format!(
            "{} already exempts it from spec.grounds",
            jid(2001)
        )]
    );
    // Another document is no repeat.
    let two = shelf(vec![fr(1, &[]), fr(2, &[]), sp(3, &[])]);
    assert!(frame(&two, &l, 2, "spec", "grounds").is_ok());
}

#[test]
fn an_exemption_of_a_document_the_link_reaches_now_is_refused_naming_whence() {
    // FR-1 is grounded by SP-2 and SP-3: the exemption would be unneeded at once.
    let reached = shelf(vec![fr(1, &[]), sp(2, &["FR-1"]), sp(3, &["FR-1"])]);
    let refused = frame(&reached, &ledger(vec![]), 1, "spec", "grounds");
    let Err(ExemptRefusal::Reached {
        book: b,
        link: l,
        by,
    }) = &refused
    else {
        panic!("refused as reached: {refused:?}");
    };
    assert_eq!((b, l), (&book("spec"), &link("grounds")));
    let by: Vec<String> = by.iter().map(ToString::to_string).collect();
    assert_eq!(by, ["spec:SP-2", "spec:SP-3"]);
    assert_eq!(
        chain(&refused.unwrap_err()),
        ["spec.grounds reaches it now, from spec:SP-2, spec:SP-3"]
    );
}

#[test]
fn a_reaching_document_denied_deprecated_or_invalid_reaches_nothing() {
    // SP-2 grounds FR-1, and is decided against: FR-1 is a gap, and may be exempt.
    let by_spec = shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]);
    for decided in [
        deny(&by_spec, 2, 1002, 10),
        deprecate(&by_spec, 2, 1002, 10),
        retire(&by_spec, 2, 1002, 10),
    ] {
        let l = ledger(vec![decided]);
        assert!(frame(&by_spec, &l, 1, "spec", "grounds").is_ok());
        let of = ExemptOf::new(docid(1), book("spec"), link("grounds"));
        assert_eq!(read(&by_spec, &l, &of), Ok(CoverSeverity::Error));
    }
    // SP-2 has no title: an invalid document holds no link.
    let invalid = shelf(vec![
        fr(1, &[]),
        reading("specs/sp-2.md", 2, "SP-2", &[("grounds", list(&["FR-1"]))]),
    ]);
    assert!(frame(&invalid, &ledger(vec![]), 1, "spec", "grounds").is_ok());
    let of = ExemptOf::new(docid(1), book("spec"), link("grounds"));
    assert_eq!(
        read(&invalid, &ledger(vec![]), &of),
        Ok(CoverSeverity::Error)
    );
}

#[test]
fn an_exemption_of_a_document_decided_against_is_refused() {
    let shelf = alone();
    for decided in [
        deny(&shelf, 1, 1001, 10),
        deprecate(&shelf, 1, 1001, 10),
        retire(&shelf, 1, 1001, 10),
    ] {
        let refused = frame(&shelf, &ledger(vec![decided]), 1, "spec", "grounds");
        assert_eq!(
            refused,
            Err(ExemptRefusal::DecidedAgainst {
                id: docid(1),
                key: shelf.doc(docid(1)).map(|d| d.key().clone()),
            })
        );
        // Named by its key, as the other messages name a document.
        assert_eq!(
            chain(&refused.unwrap_err()),
            ["finding:FR-1 is decided against: no covering link expects to reach it"]
        );
    }
}

#[test]
fn acceptance_and_reading_agree() {
    // Whatever frame_exempt accepts reads as needed; whatever reads as unneeded at
    // once is refused.
    let cases: Vec<(Shelf, Ledger)> = vec![
        (alone(), ledger(vec![])),
        (shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]), ledger(vec![])),
        (alone(), ledger(vec![deny(&alone(), 1, 1001, 10)])),
        (
            shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]),
            ledger(vec![deny(
                &shelf(vec![fr(1, &[]), sp(2, &["FR-1"])]),
                2,
                1002,
                10,
            )]),
        ),
    ];
    for (shelf, l) in cases {
        let of = ExemptOf::new(docid(1), book("spec"), link("grounds"));
        let framed = frame(&shelf, &l, 1, "spec", "grounds");
        let reading = read(&shelf, &l, &of);
        assert_eq!(framed.is_ok(), reading.is_ok(), "{framed:?} / {reading:?}");
    }
}

#[test]
fn a_reason_is_never_blank() {
    // The rule is the type's: an exemption takes a `NonBlank`, which refuses it.
    assert!(NonBlank::new("   ").is_err());
    assert!(NonBlank::new("").is_err());
}

#[test]
fn a_withdrawn_exemption_no_longer_blocks_a_new_one() {
    use crate::domain::nonempty::NonEmptySet;
    use crate::domain::shelf::ledger::Revocation;
    let l = ledger(vec![
        exempt(2001, 1),
        Record::Revocation(Revocation::new(
            jid(3001),
            NonEmptySet::from_set([jid(2001)].into()).unwrap(),
            Actor::new("alice").unwrap(),
            JudgedAt::new(60),
            NonBlank::new("to be grounded").unwrap(),
        )),
    ]);
    assert!(frame(&alone(), &l, 1, "spec", "grounds").is_ok());
}
