//! Tests of `status` with a filter: the rows kept, the filter that comes back with
//! them, the health that stays of the whole project, and a filter or a declaration
//! that cannot be used.
//!
//! Whether one document passes is tested in the domain, beside the rule, in
//! `domain/shelf/filter.rs`.

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
use crate::domain::shelf::filter::{FieldAsk, FilterProblem, StatusFilterAsk};
use crate::domain::shelf::freshness::{FreshnessLevel, StandingKind};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{
    BookName, EntryName, FieldEntryDecl, FieldPresence, FieldTypeDecl, LinkTargetDecl,
};

use super::fake_tests::{FakeShelf, block_on, docid, file, fr, path, sp};
use super::{ErrorClass, JudgeRequest, Judgement, ShelfApp, ShelfError, StatusReport};

/// The fake's two books, the findings with a list field `topic` besides.
///
/// | document | book | topic | standing |
/// |---|---|---|---|
/// | FR-1 | finding | #41 | pending |
/// | FR-2 | finding | #42, #41 | pending |
/// | FR-3 | finding | | pending |
/// | SP-4 | spec | | retired |
/// | SP-5 | spec | | pending |
///
/// and a file of the spec book that is no document, so there is a fault around the
/// documents whichever rows are asked for.
fn app() -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        file("findings/fr-1.md", 1, "FR-1", "one", &[("topic", &["#41"])]),
        file(
            "findings/fr-2.md",
            2,
            "FR-2",
            "two",
            &[("topic", &["#42", "#41"])],
        ),
        fr(3, "three", &[]),
        sp(4, "four", &[]),
        sp(5, "five", &[]),
        DocReading::Unparsed {
            origin: path("specs/broken.md"),
            id: ReadId::Read(docid(6)),
            why: RawProblem::NotAMapping,
        },
    ]);
    fake.decls.lock().unwrap()[0].fields.push(FieldEntryDecl {
        name: "topic".to_owned(),
        ty: FieldTypeDecl::TextList,
        presence: FieldPresence::Optional,
    });
    let mut app = ShelfApp::new(fake);
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Retire { reason: None },
        reference: "SP-4".parse::<Reference>().unwrap(),
        actor: None,
    }))
    .unwrap();
    app
}

fn ask_books(names: &[&str]) -> StatusFilterAsk {
    StatusFilterAsk {
        books: names.iter().map(|n| BookName::new(n).unwrap()).collect(),
        ..StatusFilterAsk::default()
    }
}

fn where_topic(value: &str) -> FieldAsk {
    FieldAsk {
        field: EntryName::new("topic").unwrap(),
        value: value.to_owned(),
    }
}

fn ids(report: &StatusReport) -> BTreeSet<DocId> {
    report.rows.iter().map(|row| row.id).collect()
}

fn set(ns: &[u128]) -> BTreeSet<DocId> {
    ns.iter().map(|n| docid(*n)).collect()
}

#[test]
fn status_is_status_filtered_with_nothing_asked() {
    let app = app();
    let all = block_on(app.status()).unwrap();
    let none = block_on(app.status_filtered(&StatusFilterAsk::default())).unwrap();
    assert_eq!(all, none);
    assert_eq!(all.filters, StatusFilterAsk::default());
    // Every identity: the five documents and the invalid file that names one.
    assert_eq!(ids(&all), set(&[1, 2, 3, 4, 5, 6]));
}

#[test]
fn the_rows_are_those_that_pass_and_the_filter_comes_back_as_given() {
    let app = app();
    let ask = ask_books(&["spec"]);
    let report = block_on(app.status_filtered(&ask)).unwrap();
    // The invalid file lies in the book `spec`, so its identity is a row of it.
    assert_eq!(ids(&report), set(&[4, 5, 6]));
    assert_eq!(report.filters, ask);

    let ask = StatusFilterAsk {
        standings: [StandingKind::Retired].into(),
        ..StatusFilterAsk::default()
    };
    let report = block_on(app.status_filtered(&ask)).unwrap();
    assert_eq!(ids(&report), set(&[4]));
    assert_eq!(report.filters, ask);

    let ask = StatusFilterAsk {
        fields: vec![where_topic("#41")],
        ..StatusFilterAsk::default()
    };
    let report = block_on(app.status_filtered(&ask)).unwrap();
    assert_eq!(ids(&report), set(&[1, 2]));
    assert_eq!(report.filters, ask);
}

#[test]
fn the_kinds_of_filter_combine() {
    let ask = StatusFilterAsk {
        books: ask_books(&["finding", "spec"]).books,
        standings: [
            StandingKind::Pending,
            StandingKind::Approved(FreshnessLevel::Fresh),
        ]
        .into(),
        fields: vec![where_topic("#42")],
    };
    let report = block_on(app().status_filtered(&ask)).unwrap();
    assert_eq!(ids(&report), set(&[2]));
}

#[test]
fn the_health_and_the_names_are_of_the_whole_project() {
    let app = app();
    let whole = block_on(app.status()).unwrap();
    let narrow = block_on(app.status_filtered(&ask_books(&["finding"]))).unwrap();
    assert_eq!(ids(&narrow), set(&[1, 2, 3]));
    assert!(!whole.health.is_clean(), "the broken file is a fault");
    assert_eq!(narrow.health, whole.health);
    assert_eq!(narrow.names, whole.names);
    // Even a filter that keeps no row.
    let none = block_on(app.status_filtered(&StatusFilterAsk {
        fields: vec![where_topic("#99")],
        ..StatusFilterAsk::default()
    }))
    .unwrap();
    assert!(none.rows.is_empty());
    assert_eq!(none.health, whole.health);
    assert_eq!(none.names, whole.names);
}

#[test]
fn a_filter_that_does_not_fit_the_declaration_is_every_problem_and_a_request_error() {
    let ask = StatusFilterAsk {
        books: ask_books(&["nothing"]).books,
        fields: vec![
            where_topic("fine"),
            FieldAsk {
                field: EntryName::new("title").unwrap(),
                value: "fine".to_owned(),
            },
            FieldAsk {
                field: EntryName::new("absent").unwrap(),
                value: "x".to_owned(),
            },
        ],
        ..StatusFilterAsk::default()
    };
    let err = block_on(app().status_filtered(&ask)).unwrap_err();
    let ShelfError::Filter(problems) = &err else {
        panic!("{err:?}");
    };
    assert_eq!(
        problems.iter().cloned().collect::<Vec<_>>(),
        vec![
            FilterProblem::UnknownBook(BookName::new("nothing").unwrap()),
            FilterProblem::UnknownField(EntryName::new("absent").unwrap()),
        ]
    );
    assert_eq!(err.to_string(), "the filter has 2 problem(s)");
    assert_eq!(err.class(), ErrorClass::Request);
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn a_value_no_book_reads_is_a_problem() {
    let mut fake = FakeShelf::new().with_files(vec![fr(1, "one", &[])]);
    fake.decls.get_mut().unwrap()[0]
        .fields
        .push(FieldEntryDecl {
            name: "count".to_owned(),
            ty: FieldTypeDecl::Integer,
            presence: FieldPresence::Optional,
        });
    let ask = StatusFilterAsk {
        fields: vec![FieldAsk {
            field: EntryName::new("count").unwrap(),
            value: "many".to_owned(),
        }],
        ..StatusFilterAsk::default()
    };
    let err = block_on(ShelfApp::new(fake).status_filtered(&ask)).unwrap_err();
    assert!(
        matches!(&err, ShelfError::Filter(problems) if matches!(
            problems.first(),
            FilterProblem::ValueNotOfField { value, .. } if value == "many"
        )),
        "{err:?}"
    );
}

#[test]
fn a_declaration_that_cannot_be_used_is_as_it_is_for_status() {
    let mut decls = super::fake_tests::decls();
    decls[0].links[0].target = LinkTargetDecl::Book("nothing".to_owned());
    let fake = FakeShelf::new().with_files(vec![fr(1, "one", &[])]);
    *fake.decls.lock().unwrap() = decls;
    let app = ShelfApp::new(fake);
    let plain = block_on(app.status()).unwrap_err();
    assert!(matches!(plain, ShelfError::Schema(_)), "{plain:?}");
    // The filter is not looked at before the declaration is usable.
    let asked = block_on(app.status_filtered(&ask_books(&["nothing"]))).unwrap_err();
    assert!(matches!(asked, ShelfError::Schema(_)), "{asked:?}");
}
