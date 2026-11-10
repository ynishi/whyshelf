//! Tests of `judge deprecate` with a named successor: a check, not a record.
//!
//! What a successor is ([`Successors::of`]) is tested in the domain, in
//! `domain/shelf/freshness/successor_tests.rs`.

use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::freshness::Successors;
use crate::domain::shelf::ledger::{DeclaredStatus, Record};
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, block_on, docid, file, sp};
use super::{JudgeRequest, Judgement, ShelfApp, ShelfError};

/// A spec `SP-<n>` that replaces these specs (its `supersedes` link).
fn replacing(n: u128, replaced: &[&str]) -> DocReading {
    file(
        &format!("specs/sp-{n}.md"),
        n,
        &format!("SP-{n}"),
        "replacement",
        &[("supersedes", replaced)],
    )
}

/// SP-1, SP-2 that replaces it, and SP-3 that replaces nothing.
fn app() -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(vec![
        sp(1, "old", &[]),
        replacing(2, &["SP-1"]),
        sp(3, "other", &[]),
    ]))
}

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn deprecate(reference: &str, successor: Option<&str>) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Deprecate {
            successor: successor.map(r),
            reason: None,
        },
        reference: r(reference),
        actor: None,
    }
}

fn deny(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: r(reference),
        actor: None,
    }
}

#[test]
fn a_deprecation_naming_a_successor_is_written_and_says_its_successors() {
    let mut app = app();
    let judged = block_on(app.judge(deprecate("SP-1", Some("SP-2")))).unwrap();
    assert_eq!(judged.subject, docid(1));
    assert_eq!(
        judged.successors,
        Some(Successors::Some(NonEmptyVec::new(docid(2), vec![])))
    );
    assert!(matches!(judged.record, Record::Decision(_)));
    assert_eq!(app.ports().append_count(), 1);
    let ledger = crate::domain::shelf::ledger::Ledger::build(app.ports().records());
    assert!(matches!(
        ledger.status_of(docid(1)),
        DeclaredStatus::Deprecated { .. }
    ));
}

#[test]
fn a_deprecation_naming_no_successor_needs_none() {
    let mut app = app();
    let judged = block_on(app.judge(deprecate("SP-3", None))).unwrap();
    assert_eq!(judged.successors, Some(Successors::None));
    assert_eq!(app.ports().append_count(), 1);
}

#[test]
fn a_successor_that_does_not_replace_the_document_is_refused_and_nothing_is_written() {
    let mut app = app();
    let err = block_on(app.judge(deprecate("SP-1", Some("SP-3")))).unwrap_err();
    match err {
        ShelfError::NotASuccessor { subject, successor } => {
            assert_eq!(subject, docid(1));
            assert_eq!(successor, docid(3));
        }
        other => panic!("expected NotASuccessor, got {other:?}"),
    }
    assert_eq!(app.ports().append_count(), 0);
    assert_eq!(app.ports().record_count(), 0);
}

#[test]
fn a_denied_replacer_named_as_a_successor_is_refused_and_nothing_more_is_written() {
    let mut app = app();
    block_on(app.judge(deny("SP-2"))).unwrap();
    assert_eq!(app.ports().record_count(), 1);
    let err = block_on(app.judge(deprecate("SP-1", Some("SP-2")))).unwrap_err();
    assert!(
        matches!(
            err,
            ShelfError::NotASuccessor { subject, successor }
                if subject == docid(1) && successor == docid(2)
        ),
        "{err:?}"
    );
    assert_eq!(app.ports().append_count(), 1);
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn a_refused_successor_in_a_run_writes_none_of_the_run() {
    let mut app = app();
    let err =
        block_on(app.judge_all(vec![deny("SP-3"), deprecate("SP-1", Some("SP-3"))])).unwrap_err();
    assert!(matches!(err, ShelfError::NotASuccessor { .. }), "{err:?}");
    assert_eq!(app.ports().record_count(), 0);
}
