//! Tests of the use cases of the shelf, over the in-memory fake.

use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::freshness::{Cause, Freshness, Severity, Standing};
use crate::domain::shelf::ledger::{DeclaredStatus, JudgedAt, Record};

use super::fake_tests::{FakeShelf, block_on, decls, docid, fr, sp};
use super::{BooksCheck, ErrorClass, JudgeRequest, Judgement, Presence, ShelfApp, ShelfError};

fn app(files: Vec<crate::domain::shelf::build::DocReading>) -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(files))
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
        },
        reference: reference.to_owned(),
        actor: None,
    }
}

#[test]
fn books_check_of_a_sound_shelf_is_clean_and_counts_the_documents() {
    let app = app(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean());
    let BooksCheck::Checked { books, .. } = check else {
        panic!("the declaration is sound");
    };
    assert_eq!(books.len(), 2);
    assert!(
        books
            .iter()
            .all(|book| book.valid == 1 && book.invalid == 0)
    );
}

#[test]
fn books_check_of_an_unsound_declaration_is_an_answer_and_reads_nothing_else() {
    let app = app(vec![fr(1, "slow", &[])]);
    let mut twice = decls();
    twice.push(decls().remove(0));
    *app.ports().decls.lock().unwrap() = twice;
    match block_on(app.books_check()).unwrap() {
        BooksCheck::SchemaInvalid(problems) => assert!(!problems.is_empty()),
        other => panic!("expected an invalid declaration, got {other:?}"),
    }
}

#[test]
fn status_lists_every_document_pending_before_any_decision() {
    let app = app(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    let report = block_on(app.status()).unwrap();
    assert_eq!(report.rows.len(), 2);
    assert!(
        report
            .rows
            .iter()
            .all(|row| row.standing == Standing::Pending)
    );
    assert!(report.health.is_clean());
    assert_eq!(app.ports().append_count(), 0);
}

#[test]
fn get_finds_a_document_by_its_key_and_says_what_is_not_there() {
    let app = app(vec![fr(1, "slow", &[])]);
    let report = block_on(app.get("FR-1")).unwrap();
    assert_eq!(report.row.id, docid(1));
    assert!(matches!(report.row.presence, Presence::Valid { .. }));
    assert!(report.doc.is_some());

    let err = block_on(app.get("FR-99")).unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }));
    assert_eq!(err.class(), ErrorClass::Request);
}

#[test]
fn get_of_a_short_id_two_documents_end_with_is_ambiguous() {
    let other = 0x1_0000_0001;
    let app = app(vec![fr(1, "slow", &[]), fr(other, "fast", &[])]);
    let err = block_on(app.get("00000001")).unwrap_err();
    match err {
        ShelfError::Ambiguous { candidates, .. } => assert_eq!(candidates.len(), 2),
        other => panic!("expected an ambiguous reference, got {other:?}"),
    }
}

#[test]
fn an_approval_is_appended_and_holds_until_an_upstream_is_edited() {
    let mut app = app(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    block_on(app.judge(approve("FR-1"))).unwrap();
    let judged = block_on(app.judge(approve("SP-2"))).unwrap();
    assert_eq!(judged.subject, docid(2));
    assert!(matches!(judged.record, Record::Decision(_)));
    assert_eq!(app.ports().record_count(), 2);

    let report = block_on(app.stale()).unwrap();
    assert!(report.entries.is_empty());
    let row = block_on(app.get("SP-2")).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));

    // The finding is edited: its approval no longer holds, and the spec that stands
    // on it is told which upstream changed.
    app.ports().edit(fr(1, "slower", &[]), 1);
    let report = block_on(app.stale()).unwrap();
    let ids: Vec<_> = report.entries.iter().map(|e| e.row.id).collect();
    assert_eq!(ids, vec![docid(1), docid(2)]);

    let spec = report
        .entries
        .iter()
        .find(|e| e.row.id == docid(2))
        .unwrap();
    let Standing::Approved(Freshness::Degraded { level, causes }) = &spec.row.standing else {
        panic!("the approval of the spec should be degraded");
    };
    assert_eq!(*level, Severity::Stale);
    assert!(causes.iter().any(
        |cause| matches!(cause, Cause::UpstreamChanged { pin, .. } if pin.target() == docid(1))
    ));
    // Where it starts: the finding that changed.
    assert!(spec.roots.iter().any(|root| {
        root.path.last() == Some(&docid(1)) && matches!(root.cause, Cause::SelfChanged { .. })
    }));
}

#[test]
fn a_refused_judgement_writes_nothing() {
    let mut app = app(vec![sp(2, "cache", &["FR-1"])]);
    // The link of the spec cannot be followed: there is no FR-1.
    let err = block_on(app.judge(approve("SP-2"))).unwrap_err();
    assert!(matches!(err, ShelfError::Refused(_)));
    assert_eq!(err.class(), ErrorClass::Refused);
    let err = block_on(app.judge(approve("SP-9"))).unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }));
    assert_eq!(app.ports().append_count(), 0);
    assert_eq!(app.ports().record_count(), 0);
}

#[test]
fn a_denial_and_a_retirement_are_recorded_with_their_reasons() {
    let mut app = app(vec![fr(1, "slow", &[])]);
    let reason = NonBlank::new("not what we saw").unwrap();
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Deny {
            reason: Some(reason),
        },
        reference: "FR-1".to_owned(),
        actor: None,
    }))
    .unwrap();
    let row = block_on(app.get("FR-1")).unwrap().row;
    assert_eq!(row.standing, Standing::Denied);
    assert!(matches!(row.declared, DeclaredStatus::Denied { .. }));
    let records = app.ports().records();
    assert_eq!(records.len(), 1);
    assert!(matches!(&records[0], Record::Decision(d) if d.reason().is_some()));
}

#[test]
fn an_approval_with_an_expiry_lapses_when_the_clock_reaches_it() {
    let mut app = app(vec![fr(1, "slow", &[])]);
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Approve {
            until: Some(JudgedAt::new(2000)),
            reason: None,
        },
        reference: "FR-1".to_owned(),
        actor: None,
    }))
    .unwrap();
    let held = block_on(app.get("FR-1")).unwrap().row;
    assert_eq!(held.standing, Standing::Approved(Freshness::Fresh));

    app.ports().set_now(3000);
    let report = block_on(app.stale()).unwrap();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].level, Severity::Expired);
}

#[test]
fn revoke_withdraws_the_standing_decision_and_needs_one() {
    let mut app = app(vec![fr(1, "slow", &[])]);
    let revoke = |why: &str| JudgeRequest {
        judgement: Judgement::Revoke {
            reason: NonBlank::new(why).unwrap(),
        },
        reference: "FR-1".to_owned(),
        actor: None,
    };
    let err = block_on(app.judge(revoke("changed my mind"))).unwrap_err();
    assert!(matches!(err, ShelfError::NothingToRevoke { .. }));
    assert_eq!(app.ports().append_count(), 0);

    block_on(app.judge(approve("FR-1"))).unwrap();
    let judged = block_on(app.judge(revoke("changed my mind"))).unwrap();
    assert!(matches!(judged.record, Record::Revocation(_)));
    let row = block_on(app.get("FR-1")).unwrap().row;
    assert_eq!(row.standing, Standing::Pending);
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn a_failing_port_is_an_error_of_the_run_not_an_answer() {
    let mut fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    fake.failing = Some("disk");
    let app = ShelfApp::new(fake);
    let err = block_on(app.status()).unwrap_err();
    assert!(matches!(err, ShelfError::Port { .. }));
    assert_eq!(err.class(), ErrorClass::Failed);
}
