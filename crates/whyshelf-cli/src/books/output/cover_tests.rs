//! Tests of the JSON of coverage: the `uncovered` list of `check`, and the
//! `not_covered` notice, each with the strength of the link.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use whyshelf_core::app::shelf::{BooksCheck, Health, Names, Uncovered};
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactKey, ArtifactName, ArtifactReadings, Listings, Space,
};
use whyshelf_core::domain::shelf::build::{DocReading, RawValue, Shelf};
use whyshelf_core::domain::shelf::coverage::{CoverAssessment, assess as assess_cover};
use whyshelf_core::domain::shelf::freshness::Notice;
use whyshelf_core::domain::shelf::ledger::{Admit, Ledger};
use whyshelf_core::domain::shelf::schema::{
    BookName, BookSchemaDecl, Cardinality, CoverDecl, CoverSeverity, EntryName, KeyPatternDecl,
    LinkEntryDecl, LinkTargetDecl, Propagation, Schema,
};

use super::*;

fn docid(n: u8) -> DocId {
    DocId::new(
        format!("01900000-0000-7000-8000-0000000000{n:02x}")
            .parse::<EntityId>()
            .unwrap(),
    )
}

fn book(text: &str) -> BookName {
    BookName::new(text).unwrap()
}

fn link(text: &str) -> EntryName {
    EntryName::new(text).unwrap()
}

#[test]
fn the_check_schema_has_the_uncovered_documents() {
    let check = serde_json::to_string(&schemas().outputs["check"]).unwrap();
    for field in ["uncovered", "doc", "book", "link", "severity"] {
        assert!(check.contains(&format!("\"{field}\"")), "{field}");
    }
}

#[test]
fn a_not_covered_notice_serializes_with_its_kind_link_and_severity() {
    let names = Names::default();
    for (severity, word) in [
        (CoverSeverity::Warn, "warn"),
        (CoverSeverity::Error, "error"),
    ] {
        let notice = Notice::NotCovered {
            book: book("spec"),
            link: link("grounds"),
            severity,
        };
        let out = serde_json::to_value(Namer(&names).notice(&notice)).unwrap();
        assert_eq!(out["kind"], "not_covered");
        assert_eq!(out["severity"], word);
        assert_eq!(out["link"], "grounds");
        assert_eq!(out["target"], Value::Null);
        assert_eq!(out["artifact"], Value::Null);
        assert_eq!(out["declared"], json!([]));
        let detail = out["detail"].as_str().unwrap();
        assert!(
            detail.contains("spec") && detail.contains("grounds"),
            "{detail}"
        );
    }
}

#[test]
fn a_notice_of_another_kind_has_no_severity() {
    let names = Names::default();
    let notice = Notice::ArtifactAbsent {
        link: link("tests"),
        artifact: ArtifactKey::new(
            Space::new("file").unwrap(),
            ArtifactName::new("later.rs").unwrap(),
        ),
    };
    let out = serde_json::to_value(Namer(&names).notice(&notice)).unwrap();
    assert_eq!(out["kind"], "artifact_absent");
    assert_eq!(out["severity"], Value::Null);
}

#[test]
fn only_a_warn_gap_is_a_warning_line() {
    assert_eq!(
        check_warnings(&check_with(CoverSeverity::Warn)),
        [format!(
            "warning: {} is reached by no spec.grounds (cover = \"warn\")",
            docid(2)
        )]
    );
    assert!(check_warnings(&check_with(CoverSeverity::Error)).is_empty());
}

/// The assessment of coverage of a shelf of one spec, SP-2, which `spec.grounds` (a
/// link into its own book, of this strength) expects to be reached and no document
/// reaches: the gap `check_with` lists.
fn assessed(severity: CoverSeverity) -> CoverAssessment {
    let schema = Schema::validate(
        vec![BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "specs".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: "SP".to_owned(),
            },
            description: None,
            fields: Vec::new(),
            links: vec![LinkEntryDecl {
                name: "grounds".to_owned(),
                target: LinkTargetDecl::Book("spec".to_owned()),
                cardinality: Cardinality::Many,
                propagation: Propagation::Depends,
                space: None,
                compare: None,
                cover: Some(CoverDecl::Word(severity.word().to_owned())),
            }],
        }],
        &BTreeSet::new(),
    )
    .unwrap();
    let readings = vec![DocReading::Parsed {
        origin: "specs/sp-2.md".parse().unwrap(),
        entries: BTreeMap::from([
            ("id".to_owned(), RawValue::Text(docid(2).to_string())),
            ("key".to_owned(), RawValue::Text("SP-2".to_owned())),
        ]),
        body: "body\n".to_owned(),
    }];
    let shelf = Shelf::build(&schema, readings, &ArtifactReadings::new());
    let cover = assess_cover(
        &shelf,
        &Ledger::build(Vec::new(), &Admit::unruled()),
        &Listings::NotAsked,
    );
    assert_eq!(cover.gaps().count(), 1, "the shelf has the gap it tells");
    cover
}

fn check_with(severity: CoverSeverity) -> BooksCheck {
    BooksCheck::Checked {
        books: Vec::new(),
        health: Health::default(),
        uncovered: vec![Uncovered {
            doc: docid(2),
            key: None,
            book: book("spec"),
            link: link("grounds"),
            severity,
        }],
        absent: Vec::new(),
        unknown: Vec::new(),
        moved: Vec::new(),
        exempt: Vec::new(),
        unneeded: Vec::new(),
        repeated: Vec::new(),
        awaiting: Vec::new(),
        same_meaning_short: Vec::new(),
        collisions: Vec::new(),
        cover: assessed(severity),
        artifacts: Default::default(),
        names: Default::default(),
    }
}

fn listed(severity: &str) -> Value {
    json!([{
        "doc": { "id": docid(2).to_string(), "key": null },
        "book": "spec",
        "link": "grounds",
        "severity": severity,
    }])
}

#[test]
fn an_uncovered_document_is_listed_and_the_check_is_not_clean() {
    let out = serde_json::to_value(books_check(&check_with(CoverSeverity::Error))).unwrap();
    assert_eq!(out["clean"], false);
    assert_eq!(out["uncovered"], listed("error"));
}

#[test]
fn a_warn_gap_is_listed_and_the_check_is_clean() {
    let out = serde_json::to_value(books_check(&check_with(CoverSeverity::Warn))).unwrap();
    assert_eq!(out["clean"], true);
    assert_eq!(out["uncovered"], listed("warn"));
}
