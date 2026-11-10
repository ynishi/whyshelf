//! Tests of `books_check` with covering links: a document a covering link fails to
//! reach is listed with the link's strength, and only a gap of strength `error`
//! makes the check not clean.

use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::schema::{CoverDecl, CoverSeverity};

use super::fake_tests::{FakeShelf, block_on, decls, docid, fr, sp};
use super::{BooksCheck, ShelfApp};

/// The fake's books with the spec's `grounds` covering the findings as `cover`
/// says.
fn app_covering(cover: CoverDecl, files: Vec<DocReading>) -> ShelfApp<FakeShelf> {
    let mut covering = decls();
    covering
        .iter_mut()
        .find(|b| b.name == "spec")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == "grounds"))
        .expect("the fake declares it")
        .cover = Some(cover);
    let fake = FakeShelf::new().with_files(files);
    *fake.decls.lock().unwrap() = covering;
    ShelfApp::new(fake)
}

/// The fake's books with `cover = true` on the spec's `grounds`.
fn app(files: Vec<DocReading>) -> ShelfApp<FakeShelf> {
    app_covering(CoverDecl::Truth(true), files)
}

/// FR-1 grounded by SP-3, and FR-2 alone.
fn one_alone() -> Vec<DocReading> {
    vec![
        fr(1, "grounded", &[]),
        fr(2, "alone", &[]),
        sp(3, "cache", &["FR-1"]),
    ]
}

/// The one document listed as uncovered by `check`: FR-2, through the spec's
/// `grounds`, with this strength.
fn assert_lists_fr2(check: &BooksCheck, severity: CoverSeverity) {
    let BooksCheck::Checked {
        uncovered, health, ..
    } = check
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
    assert_eq!(one.severity, severity);
}

#[test]
fn books_check_lists_an_uncovered_document_and_is_not_clean() {
    let check = block_on(app(one_alone()).books_check()).unwrap();
    assert!(!check.is_clean());
    assert_lists_fr2(&check, CoverSeverity::Error);
}

#[test]
fn an_error_gap_fails_the_check() {
    let app = app_covering(CoverDecl::Word("error".to_owned()), one_alone());
    let check = block_on(app.books_check()).unwrap();
    assert!(!check.is_clean());
    assert_lists_fr2(&check, CoverSeverity::Error);
}

#[test]
fn a_warn_gap_is_listed_with_its_strength_and_leaves_the_check_clean() {
    let app = app_covering(CoverDecl::Word("warn".to_owned()), one_alone());
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
    assert_lists_fr2(&check, CoverSeverity::Warn);
}

#[test]
fn books_check_of_covered_documents_is_clean() {
    let app = app(vec![fr(1, "grounded", &[]), sp(3, "cache", &["FR-1"])]);
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
}
