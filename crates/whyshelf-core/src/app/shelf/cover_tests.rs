//! Tests of `books_check` with covering links (`cover = true`): a document a
//! covering link fails to reach is listed, and the check is not clean.

use crate::domain::shelf::build::DocReading;

use super::fake_tests::{FakeShelf, block_on, decls, docid, fr, sp};
use super::{BooksCheck, ShelfApp};

/// The fake's books with the spec's `grounds` covering the findings.
fn app(files: Vec<DocReading>) -> ShelfApp<FakeShelf> {
    let mut covering = decls();
    covering
        .iter_mut()
        .find(|b| b.name == "spec")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == "grounds"))
        .expect("the fake declares it")
        .cover = true;
    let fake = FakeShelf::new().with_files(files);
    *fake.decls.lock().unwrap() = covering;
    ShelfApp::new(fake)
}

#[test]
fn books_check_lists_an_uncovered_document_and_is_not_clean() {
    let app = app(vec![
        fr(1, "grounded", &[]),
        fr(2, "alone", &[]),
        sp(3, "cache", &["FR-1"]),
    ]);
    let check = block_on(app.books_check()).unwrap();
    assert!(!check.is_clean());
    let BooksCheck::Checked {
        uncovered, health, ..
    } = &check
    else {
        panic!("the declaration is sound");
    };
    assert!(health.is_clean(), "only the coverage is wrong: {health:?}");
    assert_eq!(uncovered.len(), 1, "{uncovered:?}");
    let one = &uncovered[0];
    assert_eq!(one.doc, docid(2));
    assert_eq!(
        one.key.as_ref().map(|k| k.to_string()).as_deref(),
        Some("finding:FR-2")
    );
    assert_eq!(one.book.as_str(), "spec");
    assert_eq!(one.link.as_str(), "grounds");
}

#[test]
fn books_check_of_covered_documents_is_clean() {
    let app = app(vec![fr(1, "grounded", &[]), sp(3, "cache", &["FR-1"])]);
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
}
