//! Tests of the JSON of coverage: the `uncovered` list of `check`, and the
//! `not_covered` notice.

use serde_json::{Value, json};
use whyshelf_core::app::shelf::{BooksCheck, Health, Names, Uncovered};
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::shelf::freshness::Notice;
use whyshelf_core::domain::shelf::schema::{BookName, EntryName};

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
    for field in ["uncovered", "doc", "book", "link"] {
        assert!(check.contains(&format!("\"{field}\"")), "{field}");
    }
}

#[test]
fn a_not_covered_notice_serializes_with_its_kind_and_link() {
    let names = Names::default();
    let notice = Notice::NotCovered {
        book: book("spec"),
        link: link("grounds"),
    };
    let out = serde_json::to_value(Namer(&names).notice(&notice)).unwrap();
    assert_eq!(out["kind"], "not_covered");
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

#[test]
fn an_uncovered_document_is_listed_and_the_check_is_not_clean() {
    let check = BooksCheck::Checked {
        books: Vec::new(),
        health: Health::default(),
        uncovered: vec![Uncovered {
            doc: docid(2),
            key: None,
            book: book("spec"),
            link: link("grounds"),
        }],
        absent: Vec::new(),
        unknown: Vec::new(),
    };
    let out = serde_json::to_value(books_check(&check)).unwrap();
    assert_eq!(out["clean"], false);
    assert_eq!(
        out["uncovered"],
        json!([{
            "doc": { "id": docid(2).to_string(), "key": null },
            "book": "spec",
            "link": "grounds",
        }])
    );
}
