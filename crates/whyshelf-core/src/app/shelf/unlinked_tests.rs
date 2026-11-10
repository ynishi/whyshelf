//! Tests of `unlinked`: the lists, the filters, the documents decided against apart,
//! a declaration that cannot be used, and a book that is not declared.
//!
//! Which document is in which list is tested in the domain, beside `linkage` in
//! `domain/shelf/trace/linkage_tests.rs`.

use crate::domain::id::DocId;
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::BookName;

use super::fake_tests::{FakeShelf, block_on, docid, file, fr, sp};
use super::{
    JudgeRequest, Judgement, ShelfApp, ShelfError, UnlinkedAsk, UnlinkedEntry, UnlinkedFilter,
    UnlinkedLists, UnlinkedReport,
};

/// The fake's books: a finding may be reached (by `grounds`) and may hold a link
/// (`base`, into its own book); a spec may hold links but is reached only by
/// `supersedes`, which replaces.
///
/// | document | links | standing | list |
/// |---|---|---|---|
/// | FR-1 | | pending | no_outgoing (SP-3 and FR-9 reach it) |
/// | FR-2 | | pending | isolated |
/// | SP-3 | grounds FR-1 | pending | none (a spec may not be reached) |
/// | SP-4 | grounds FR-5 | retired | none (it holds a link) |
/// | FR-5 | | pending | isolated (only a retired spec links to it) |
/// | SP-6 | | pending | no_outgoing (SP-7 replaces it) |
/// | SP-7 | supersedes SP-6 | pending | none |
/// | SP-8 | | retired | decided against, isolated |
/// | FR-9 | base FR-1 | pending | no_incoming |
fn app() -> ShelfApp<FakeShelf> {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "grounded", &[]),
        fr(2, "alone", &[]),
        sp(3, "cache", &["FR-1"]),
        sp(4, "dropped", &["FR-5"]),
        fr(5, "grounded by a retired spec", &[]),
        sp(6, "replaced", &[]),
        file(
            "specs/sp-7.md",
            7,
            "SP-7",
            "replacement",
            &[("supersedes", &["SP-6"])],
        ),
        sp(8, "dropped alone", &[]),
        fr(9, "on another finding", &["FR-1"]),
    ]));
    for key in ["SP-4", "SP-8"] {
        block_on(app.judge(JudgeRequest {
            judgement: Judgement::Retire { reason: None },
            reference: key.parse::<Reference>().unwrap(),
            actor: None,
        }))
        .unwrap();
    }
    app
}

fn ids(list: &[UnlinkedEntry]) -> Vec<DocId> {
    list.iter().map(|e| e.row.id).collect()
}

fn asked(list: &Option<Vec<UnlinkedEntry>>) -> Option<Vec<DocId>> {
    list.as_deref().map(ids)
}

fn lists(report: &UnlinkedReport) -> (&UnlinkedLists, &UnlinkedLists) {
    match report {
        UnlinkedReport::Answered {
            standing,
            decided_against,
            ..
        } => (standing, decided_against),
        UnlinkedReport::SchemaInvalid { problems, .. } => {
            panic!("the declaration is sound: {problems:?}")
        }
    }
}

#[test]
fn every_list_by_default_with_the_documents_decided_against_apart() {
    let report = block_on(app().unlinked(&UnlinkedFilter::default())).unwrap();
    let (standing, decided_against) = lists(&report);
    assert_eq!(ids(&standing.isolated), vec![docid(2), docid(5)]);
    assert_eq!(asked(&standing.no_incoming), Some(vec![docid(9)]));
    assert_eq!(asked(&standing.no_outgoing), Some(vec![docid(1), docid(6)]));
    assert_eq!(ids(&decided_against.isolated), vec![docid(8)]);
    assert_eq!(asked(&decided_against.no_incoming), Some(vec![]));
    assert_eq!(asked(&decided_against.no_outgoing), Some(vec![]));
    assert_eq!(standing.isolated[0].book.as_str(), "finding");
    assert_eq!(decided_against.isolated[0].book.as_str(), "spec");
}

#[test]
fn the_book_and_one_question_narrow_the_answer() {
    let app = app();
    let finding = UnlinkedFilter {
        book: Some(BookName::new("finding").unwrap()),
        ask: UnlinkedAsk::NoIncoming,
    };
    let report = block_on(app.unlinked(&finding)).unwrap();
    let (standing, decided_against) = lists(&report);
    // Neither question removes the documents linked neither way.
    assert_eq!(ids(&standing.isolated), vec![docid(2), docid(5)]);
    assert_eq!(asked(&standing.no_incoming), Some(vec![docid(9)]));
    assert_eq!(standing.no_outgoing, None);
    assert_eq!(ids(&decided_against.isolated), vec![]);
    assert_eq!(decided_against.no_outgoing, None);

    let spec = UnlinkedFilter {
        book: Some(BookName::new("spec").unwrap()),
        ask: UnlinkedAsk::NoOutgoing,
    };
    let report = block_on(app.unlinked(&spec)).unwrap();
    let (standing, decided_against) = lists(&report);
    assert_eq!(ids(&standing.isolated), vec![]);
    assert_eq!(standing.no_incoming, None);
    assert_eq!(asked(&standing.no_outgoing), Some(vec![docid(6)]));
    assert_eq!(ids(&decided_against.isolated), vec![docid(8)]);
    assert_eq!(decided_against.no_incoming, None);
}

#[test]
fn a_lone_document_of_a_wiki_is_isolated() {
    // One book, whose one link informs of its own book.
    let mut decls = super::fake_tests::decls();
    decls.truncate(1);
    decls[0].links[0].propagation = crate::domain::shelf::schema::Propagation::Informs;
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "lone", &[]),
        fr(2, "linked", &["FR-3"]),
        fr(3, "linked to", &[]),
    ]);
    *fake.decls.lock().unwrap() = decls;
    let report = block_on(ShelfApp::new(fake).unlinked(&UnlinkedFilter::default())).unwrap();
    let (standing, _) = lists(&report);
    assert_eq!(ids(&standing.isolated), vec![docid(1)]);
    assert_eq!(asked(&standing.no_incoming), Some(vec![docid(2)]));
    assert_eq!(asked(&standing.no_outgoing), Some(vec![docid(3)]));
}

#[test]
fn a_declaration_that_cannot_be_used_is_its_problems_with_the_lists_asked_for_empty() {
    let mut decls = super::fake_tests::decls();
    decls[0].links[0].target =
        crate::domain::shelf::schema::LinkTargetDecl::Book("nothing".to_owned());
    let fake = FakeShelf::new().with_files(vec![fr(1, "alone", &[])]);
    *fake.decls.lock().unwrap() = decls;
    let filter = UnlinkedFilter {
        book: None,
        ask: UnlinkedAsk::NoOutgoing,
    };
    let report = block_on(ShelfApp::new(fake).unlinked(&filter)).unwrap();
    let UnlinkedReport::SchemaInvalid { problems, lists } = report else {
        panic!("the declaration names a book it does not declare: {report:?}");
    };
    assert!(!problems.is_empty());
    assert_eq!(
        lists,
        UnlinkedLists {
            isolated: vec![],
            no_incoming: None,
            no_outgoing: Some(vec![]),
        }
    );
}

#[test]
fn a_book_that_is_not_declared_is_a_request_error() {
    let filter = UnlinkedFilter {
        book: Some(BookName::new("nothing").unwrap()),
        ask: UnlinkedAsk::Both,
    };
    let err = block_on(app().unlinked(&filter)).unwrap_err();
    assert!(
        matches!(&err, ShelfError::UnknownBook { reference: None, book } if book.as_str() == "nothing"),
        "{err:?}"
    );
    assert_eq!(err.to_string(), "there is no book `nothing`");
    assert_eq!(err.class(), super::ErrorClass::Request);
}
