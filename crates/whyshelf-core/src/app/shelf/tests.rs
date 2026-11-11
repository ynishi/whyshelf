//! Tests of the use cases of the shelf, over the in-memory fake.

use crate::domain::canon::Canon;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::freshness::{Cause, Freshness, Severity, Standing};
use crate::domain::shelf::ledger::{DeclaredStatus, JudgedAt, Record};

use super::fake_tests::{FakeShelf, block_on, decls, docid, fr, sp};
use super::{BooksCheck, ErrorClass, JudgeRequest, Judgement, MapReport, ShelfApp, ShelfError};
use crate::domain::shelf::build::DocPresence;
use crate::domain::shelf::lookup::Reference;

fn app(files: Vec<crate::domain::shelf::build::DocReading>) -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(files))
}

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: r(reference),
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
            .all(|book| book.tally.valid == 1 && book.tally.invalid == 0)
    );
}

#[test]
fn books_check_lists_the_files_a_book_skips_and_stays_clean() {
    use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
    use crate::domain::shelf::schema::ScopeDecl;
    let none = |file: &str| DocReading::Unparsed {
        origin: file.parse().unwrap(),
        id: ReadId::Unreadable,
        why: RawProblem::NoFrontMatter,
    };
    let broken = DocReading::Unparsed {
        origin: "findings/broken.md".parse().unwrap(),
        id: ReadId::Unreadable,
        why: RawProblem::Unclosed,
    };
    let files = || {
        vec![
            fr(1, "slow", &[]),
            none("findings/notes.md"),
            none("findings/old/a.md"),
        ]
    };
    let skipping = ScopeDecl {
        include: None,
        exclude: None,
        without_front_matter: Some("skip".to_owned()),
    };

    let fake = FakeShelf::new().with_files(files());
    fake.decls.lock().unwrap()[0].scope = skipping.clone();
    let check = block_on(ShelfApp::new(fake).books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
    let BooksCheck::Checked { books, .. } = &check else {
        panic!("the declaration is sound");
    };
    let finding = &books[0];
    assert_eq!((finding.tally.valid, finding.tally.invalid), (1, 0));
    let skipped: Vec<&str> = finding
        .skipped
        .iter()
        .map(|file| file.origin().as_str())
        .collect();
    assert_eq!(skipped, ["findings/notes.md", "findings/old/a.md"]);
    assert!(books[1].skipped.is_empty());

    // A broken front matter is still invalid under `skip`.
    let mut with_broken = files();
    with_broken.push(broken);
    let fake = FakeShelf::new().with_files(with_broken);
    fake.decls.lock().unwrap()[0].scope = skipping;
    let check = block_on(ShelfApp::new(fake).books_check()).unwrap();
    assert!(!check.is_clean());

    // Without `skip`, the same files make the check fail, as before.
    let check = block_on(app(files()).books_check()).unwrap();
    assert!(!check.is_clean());
    let BooksCheck::Checked { books, .. } = &check else {
        panic!("the declaration is sound");
    };
    assert_eq!(books[0].tally.invalid, 2);
    assert!(books[0].skipped.is_empty());
}

#[test]
fn a_link_to_a_key_only_an_excluded_file_writes_is_unresolved_as_missing() {
    use crate::domain::shelf::doc::{LinkState, UnresolvedWhy};
    use crate::domain::shelf::schema::ScopeDecl;
    let hidden = super::fake_tests::file("findings/hidden/fr-2.md", 2, "FR-2", "hidden", &[]);
    let fake = FakeShelf::new().with_files(vec![fr(1, "slow", &["FR-2"]), hidden]);
    fake.decls.lock().unwrap()[0].scope = ScopeDecl {
        include: None,
        exclude: Some(vec!["hidden/**".to_owned()]),
        without_front_matter: None,
    };
    let app = ShelfApp::new(fake);
    let check = block_on(app.books_check()).unwrap();
    let BooksCheck::Checked { books, .. } = &check else {
        panic!("the declaration is sound");
    };
    // The excluded file is no document: neither valid nor invalid.
    assert_eq!((books[0].tally.valid, books[0].tally.invalid), (1, 0));

    let doc = block_on(app.get(&r("FR-1"))).unwrap().doc.unwrap();
    let states = doc.links()["base"].states();
    let [LinkState::Unresolved(link)] = states else {
        panic!("{states:?}");
    };
    assert_eq!(link.written(), "FR-2");
    assert_eq!(link.why(), &UnresolvedWhy::Missing);
    // Nothing answers to its identity either.
    assert!(block_on(app.get(&r("FR-2"))).is_err());
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
fn map_answers_the_declaration_in_its_order() {
    let app = app(vec![fr(1, "slow", &[])]);
    // Declared out of the order of their names, so the answer cannot be in that
    // order by chance.
    let mut reversed = decls();
    reversed.reverse();
    *app.ports().decls.lock().unwrap() = reversed;
    let MapReport::Mapped(map) = block_on(app.map()).unwrap() else {
        panic!("the declaration is sound");
    };
    let books: Vec<&str> = map.books.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(books, ["spec", "finding"]);
    let holders: Vec<&str> = map.links.iter().map(|l| l.from.as_str()).collect();
    assert_eq!(holders.first(), Some(&"spec"));
    assert!(!map.links.is_empty());
    assert_eq!(block_on(app.map()).unwrap(), MapReport::Mapped(map));
    assert_eq!(app.ports().artifact_read_count(), 0);
}

#[test]
fn books_check_lists_the_books_in_the_order_of_the_declaration() {
    let app = app(vec![fr(1, "slow", &[])]);
    let mut reversed = decls();
    reversed.reverse();
    *app.ports().decls.lock().unwrap() = reversed;
    let BooksCheck::Checked { books, .. } = block_on(app.books_check()).unwrap() else {
        panic!("the declaration is sound");
    };
    let names: Vec<&str> = books.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["spec", "finding"]);
}

#[test]
fn map_of_an_unsound_declaration_is_its_problems_as_check_answers_them() {
    let app = app(vec![fr(1, "slow", &[])]);
    let mut twice = decls();
    twice.push(decls().remove(0));
    *app.ports().decls.lock().unwrap() = twice;
    let BooksCheck::SchemaInvalid(checked) = block_on(app.books_check()).unwrap() else {
        panic!("expected an invalid declaration");
    };
    assert_eq!(
        block_on(app.map()).unwrap(),
        MapReport::SchemaInvalid(checked)
    );
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
    let report = block_on(app.get(&r("FR-1"))).unwrap();
    assert_eq!(report.row.id, docid(1));
    assert!(matches!(report.row.presence, DocPresence::Valid { .. }));
    assert!(report.doc.is_some());

    let err = block_on(app.get(&r("FR-99"))).unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }));
    assert_eq!(err.class(), ErrorClass::Request);
}

#[test]
fn get_of_a_book_nobody_declared_is_an_unknown_book() {
    let app = app(vec![fr(1, "slow", &[])]);
    let err = block_on(app.get(&r("nobook:X-1"))).unwrap_err();
    assert!(matches!(&err, ShelfError::UnknownBook { book, .. } if book.as_str() == "nobook"));
    assert_eq!(err.class(), ErrorClass::Request);
}

#[test]
fn get_of_a_short_id_two_documents_end_with_is_ambiguous() {
    let other = 0x1_0000_0001;
    let app = app(vec![fr(1, "slow", &[]), fr(other, "fast", &[])]);
    let err = block_on(app.get(&r("00000001"))).unwrap_err();
    match err {
        ShelfError::Ambiguous { candidates, .. } => assert_eq!(candidates.len(), 2),
        other => panic!("expected an ambiguous reference, got {other:?}"),
    }
}

#[test]
fn an_approval_is_appended_and_holds_until_an_upstream_is_edited() {
    let mut app = app(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    block_on(app.judge(approve("FR-1"))).unwrap();
    let judged = block_on(app.judge(approve("SP-2")))
        .unwrap()
        .judged
        .remove(0);
    assert_eq!(judged.subject, docid(2));
    assert!(matches!(judged.record, Record::Decision(_)));
    assert_eq!(app.ports().record_count(), 2);

    let report = block_on(app.stale()).unwrap();
    assert!(report.entries.is_empty());
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
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
    let Standing::Approved(Freshness::Degraded(degradation)) = &spec.row.standing else {
        panic!("the approval of the spec should be degraded");
    };
    let causes = degradation.causes();
    assert_eq!(degradation.level(), Severity::Stale);
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
        reference: r("FR-1"),
        actor: None,
    }))
    .unwrap();
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert_eq!(row.standing, Standing::Denied);
    assert!(matches!(row.declared, DeclaredStatus::Denied { .. }));
    let records = app.ports().records();
    assert_eq!(records.len(), 1);
    assert!(matches!(&records[0], Record::Decision(_)));
    assert!(
        matches!(records[0].to_canon(), Canon::Map(m) if matches!(m.get("reason"), Some(Canon::Str(_))))
    );
}

#[test]
fn a_row_carries_the_decision_that_stands_with_who_when_and_why() {
    let mut app = app(vec![fr(1, "slow", &[])]);
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert_eq!(row.decision, None, "pending: no decision");

    let mut approval = approve("FR-1");
    approval.judgement = Judgement::Approve {
        until: None,
        reason: Some(NonBlank::new("as seen").unwrap()),
        again: false,
    };
    let approved = block_on(app.judge(approval)).unwrap().judged.remove(0);
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    let decision = row.decision.expect("approved: a decision");
    assert_eq!(decision.id(), approved.record.id());
    assert_eq!(decision.reason().map(NonBlank::as_str), Some("as seen"));

    // A later denial is the decision that stands, the one `declared` is read from.
    let denied = block_on(app.judge(JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: r("FR-1"),
        actor: None,
    }))
    .unwrap()
    .judged
    .remove(0);
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    let decision = row.decision.expect("denied: a decision");
    assert_eq!(decision.id(), denied.record.id());
    assert_eq!(decision.reason(), None);
    assert!(
        matches!(row.declared, DeclaredStatus::Denied { judgement } if judgement == decision.id())
    );
    assert!(
        block_on(app.status())
            .unwrap()
            .rows
            .iter()
            .all(|row| row.decision.is_some())
    );

    // Withdrawing the denial brings the approval back as the decision that stands;
    // withdrawing that too leaves none.
    let revoke = || JudgeRequest {
        judgement: Judgement::Revoke {
            reason: NonBlank::new("withdrawn").unwrap(),
        },
        reference: r("FR-1"),
        actor: None,
    };
    block_on(app.judge(revoke())).unwrap();
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert_eq!(
        row.decision.map(|d| d.id()),
        Some(approved.record.id()),
        "the approval stands again"
    );
    assert!(
        matches!(row.declared, DeclaredStatus::Approved { judgement, .. } if judgement == approved.record.id())
    );
    block_on(app.judge(revoke())).unwrap();
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert_eq!(row.decision, None);
    assert_eq!(row.declared, DeclaredStatus::Pending);
}

#[test]
fn an_approval_with_an_expiry_lapses_when_the_clock_reaches_it() {
    let mut app = app(vec![fr(1, "slow", &[])]);
    block_on(app.judge(JudgeRequest {
        judgement: Judgement::Approve {
            until: Some(JudgedAt::new(2000)),
            reason: None,
            again: false,
        },
        reference: r("FR-1"),
        actor: None,
    }))
    .unwrap();
    let held = block_on(app.get(&r("FR-1"))).unwrap().row;
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
        reference: r("FR-1"),
        actor: None,
    };
    let err = block_on(app.judge(revoke("changed my mind"))).unwrap_err();
    assert!(matches!(err, ShelfError::NothingToRevoke { .. }));
    assert_eq!(app.ports().append_count(), 0);

    block_on(app.judge(approve("FR-1"))).unwrap();
    let judged = block_on(app.judge(revoke("changed my mind")))
        .unwrap()
        .judged
        .remove(0);
    assert!(matches!(judged.record, Record::Revocation(_)));
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert_eq!(row.standing, Standing::Pending);
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn judge_all_approves_an_upstream_and_its_downstream_in_one_run() {
    let mut app = app(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    let judged = block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")]))
        .unwrap()
        .judged;
    assert_eq!(judged.len(), 2);
    assert_eq!(judged[0].subject, docid(1));
    assert_eq!(judged[1].subject, docid(2));
    assert_eq!(app.ports().append_count(), 2);
    assert_eq!(app.ports().record_count(), 2);

    // The spec was framed on the approval of the finding made one step before it, so
    // it holds, and it is told when that finding is edited.
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    app.ports().edit(fr(1, "slower", &[]), 1);
    let ids: Vec<_> = block_on(app.stale())
        .unwrap()
        .entries
        .iter()
        .map(|e| e.row.id)
        .collect();
    assert_eq!(ids, vec![docid(1), docid(2)]);
}

#[test]
fn judge_all_writes_nothing_when_a_later_request_is_refused() {
    let mut app = app(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    let err = block_on(app.judge_all(vec![approve("FR-1"), approve("SP-9")])).unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }));
    assert_eq!(app.ports().append_count(), 0);
    assert_eq!(app.ports().record_count(), 0);
    // The first, alone, was sound: the refusal is the second's.
    block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn judge_all_of_nothing_is_an_empty_run_that_writes_nothing() {
    let mut app = app(vec![fr(1, "slow", &[])]);
    let run = block_on(app.judge_all(vec![])).unwrap();
    assert!(run.judged.is_empty() && run.left_out.is_empty());
    assert_eq!(app.ports().append_count(), 0);
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
